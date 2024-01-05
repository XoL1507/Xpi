// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use core::result::Result::Ok;
use itertools::Itertools;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tap::Tap;

use async_trait::async_trait;
use diesel::dsl::max;
use diesel::upsert::excluded;
use diesel::ExpressionMethods;
use diesel::OptionalExtension;
use diesel::{QueryDsl, RunQueryDsl};
use move_bytecode_utils::module_cache::SyncModuleCache;
use tracing::info;

use sui_types::base_types::{ObjectID, SequenceNumber};
use sui_types::object::ObjectRead;

use crate::errors::{Context, IndexerError};
use crate::handlers::EpochToCommit;
use crate::handlers::TransactionObjectChangesToCommit;
use crate::metrics::IndexerMetrics;

use crate::models_v2::checkpoints::StoredCheckpoint;
use crate::models_v2::epoch::StoredEpochInfo;
use crate::models_v2::events::StoredEvent;
use crate::models_v2::objects::StoredObject;
use crate::models_v2::packages::StoredPackage;
use crate::models_v2::transactions::StoredTransaction;
use crate::models_v2::tx_indices::StoredTxIndex;
use crate::schema_v2::{checkpoints, epochs, events, objects, packages, transactions, tx_indices};
use crate::store::diesel_macro::{read_only_blocking, transactional_blocking_with_retry};
use crate::store::module_resolver_v2::IndexerStoreModuleResolver;
use crate::types_v2::{
    IndexedCheckpoint, IndexedEvent, IndexedPackage, IndexedTransaction, TxIndex,
};
use crate::PgConnectionPool;

use super::IndexerStoreV2;

#[macro_export]
macro_rules! chunk {
    ($data: expr, $size: expr) => {{
        $data
            .into_iter()
            .chunks($size)
            .into_iter()
            .map(|c| c.collect())
            .collect::<Vec<Vec<_>>>()
    }};
}

// In one DB transaction, the update could be chunked into
// a few statements, this is the amount of rows to update in one statement
// TODO: I think with the `per_db_tx` params, `PG_COMMIT_CHUNK_SIZE_INTRA_DB_TX`
// is now less relevant. We should do experiments and remove it if it's true.
const PG_COMMIT_CHUNK_SIZE_INTRA_DB_TX: usize = 1000;
// The amount of rows to update in one DB transcation
const PG_COMMIT_PARALLEL_CHUNK_SIZE_PER_DB_TX: usize = 500;
// The amount of rows to update in one DB transcation, for objects particularly
// Having this number too high may cause many db deadlocks because of
// optimistic locking.
const PG_COMMIT_OBJECTS_PARALLEL_CHUNK_SIZE_PER_DB_TX: usize = 500;

#[derive(Clone)]
pub struct PgIndexerStoreV2 {
    blocking_cp: PgConnectionPool,
    module_cache: Arc<SyncModuleCache<IndexerStoreModuleResolver>>,
    metrics: IndexerMetrics,
    parallel_chunk_size: usize,
    parallel_objects_chunk_size: usize,
}

impl PgIndexerStoreV2 {
    pub fn new(blocking_cp: PgConnectionPool, metrics: IndexerMetrics) -> Self {
        let module_cache: Arc<SyncModuleCache<IndexerStoreModuleResolver>> = Arc::new(
            SyncModuleCache::new(IndexerStoreModuleResolver::new(blocking_cp.clone())),
        );
        let parallel_chunk_size = std::env::var("PG_COMMIT_PARALLEL_CHUNK_SIZE")
            .unwrap_or_else(|_e| PG_COMMIT_PARALLEL_CHUNK_SIZE_PER_DB_TX.to_string())
            .parse::<usize>()
            .unwrap();
        let parallel_objects_chunk_size = std::env::var("PG_COMMIT_OBJECTS_PARALLEL_CHUNK_SIZE")
            .unwrap_or_else(|_e| PG_COMMIT_OBJECTS_PARALLEL_CHUNK_SIZE_PER_DB_TX.to_string())
            .parse::<usize>()
            .unwrap();
        Self {
            blocking_cp,
            module_cache,
            metrics,
            parallel_chunk_size,
            parallel_objects_chunk_size,
        }
    }

    fn get_latest_tx_checkpoint_sequence_number(&self) -> Result<Option<u64>, IndexerError> {
        read_only_blocking!(&self.blocking_cp, |conn| {
            checkpoints::dsl::checkpoints
                .select(max(checkpoints::sequence_number))
                .first::<Option<i64>>(conn)
                .map(|v| v.map(|v| v as u64))
        })
        .context("Failed reading latest checkpoint sequence number from PostgresDB")
    }

    // Note: here we treat Deleted as NotExists too
    fn get_object_read(
        &self,
        object_id: ObjectID,
        version: Option<SequenceNumber>,
    ) -> Result<ObjectRead, IndexerError> {
        // TOOD: read remote object_history kv store
        read_only_blocking!(&self.blocking_cp, |conn| {
            let query =
                objects::dsl::objects.filter(objects::dsl::object_id.eq(object_id.to_vec()));
            let boxed_query = if let Some(version) = version {
                query
                    .filter(objects::dsl::object_version.eq(version.value() as i64))
                    .into_boxed()
            } else {
                query.into_boxed()
            };
            match boxed_query.first::<StoredObject>(conn).optional()? {
                None => Ok(ObjectRead::NotExists(object_id)),
                Some(obj) => obj.try_into_object_read(self.module_cache.as_ref()),
            }
        })
        .context("Failed to read object from PostgresDB")
    }

    fn persist_objects_chunk(
        &self,
        objects: Vec<ObjectChangeToCommit>,
    ) -> Result<(), IndexerError> {
        let guard = self
            .metrics
            .checkpoint_db_commit_latency_objects_chunks
            .start_timer();

        let mut mutated_objects = vec![];
        let mut deleted_object_ids = vec![];
        for object in objects {
            match object {
                ObjectChangeToCommit::MutatedObject(o) => {
                    mutated_objects.push(o);
                }
                ObjectChangeToCommit::DeletedObject(id) => {
                    deleted_object_ids.push(id);
                }
            }
        }

        transactional_blocking_with_retry!(
            &self.blocking_cp,
            |conn| {
                // Persist mutated objects
                for mutated_object_change_chunk in
                    mutated_objects.chunks(PG_COMMIT_CHUNK_SIZE_INTRA_DB_TX)
                {
                    diesel::insert_into(objects::table)
                        .values(mutated_object_change_chunk)
                        .on_conflict(objects::object_id)
                        .do_update()
                        .set((
                            objects::object_id.eq(excluded(objects::object_id)),
                            objects::object_version.eq(excluded(objects::object_version)),
                            objects::object_digest.eq(excluded(objects::object_digest)),
                            objects::checkpoint_sequence_number
                                .eq(excluded(objects::checkpoint_sequence_number)),
                            objects::owner_type.eq(excluded(objects::owner_type)),
                            objects::owner_id.eq(excluded(objects::owner_id)),
                            objects::serialized_object.eq(excluded(objects::serialized_object)),
                            objects::coin_type.eq(excluded(objects::coin_type)),
                            objects::coin_balance.eq(excluded(objects::coin_balance)),
                            objects::df_kind.eq(excluded(objects::df_kind)),
                            objects::df_name.eq(excluded(objects::df_name)),
                            objects::df_object_type.eq(excluded(objects::df_object_type)),
                            objects::df_object_id.eq(excluded(objects::df_object_id)),
                        ))
                        .execute(conn)
                        .map_err(IndexerError::from)
                        .context("Failed to write object mutation to PostgresDB")?;
                }

                // Persist deleted objects
                for deleted_objects_chunk in
                    deleted_object_ids.chunks(PG_COMMIT_CHUNK_SIZE_INTRA_DB_TX)
                {
                    diesel::delete(
                        objects::table.filter(
                            objects::object_id.eq_any(
                                deleted_objects_chunk
                                    .iter()
                                    .map(|o| o.to_vec())
                                    .collect::<Vec<_>>(),
                            ),
                        ),
                    )
                    .execute(conn)
                    .map_err(IndexerError::from)
                    .context("Failed to write object deletion to PostgresDB")?;
                }

                Ok::<(), IndexerError>(())
            },
            Duration::from_secs(60)
        )
        .tap(|_| {
            let elapsed = guard.stop_and_record();
            info!(
                elapsed,
                "Persisted {} chunked objects",
                mutated_objects.len() + deleted_object_ids.len(),
            )
        })
    }

    fn persist_checkpoints(&self, checkpoints: Vec<IndexedCheckpoint>) -> Result<(), IndexerError> {
        if checkpoints.is_empty() {
            return Ok(());
        }
        let guard = self
            .metrics
            .checkpoint_db_commit_latency_checkpoints
            .start_timer();

        let checkpoints = checkpoints
            .iter()
            .map(StoredCheckpoint::from)
            .collect::<Vec<_>>();
        transactional_blocking_with_retry!(
            &self.blocking_cp,
            |conn| {
                for checkpoint_chunk in checkpoints.chunks(PG_COMMIT_CHUNK_SIZE_INTRA_DB_TX) {
                    diesel::insert_into(checkpoints::table)
                        .values(checkpoint_chunk)
                        .on_conflict_do_nothing()
                        .execute(conn)
                        .map_err(IndexerError::from)
                        .context("Failed to write checkpoints to PostgresDB")?;
                }
                Ok::<(), IndexerError>(())
            },
            Duration::from_secs(60)
        )
        .tap(|_| {
            let elapsed = guard.stop_and_record();
            info!(elapsed, "Persisted {} checkpoints", checkpoints.len());
        })
    }

    fn persist_transactions_chunk(
        &self,
        transactions: Vec<IndexedTransaction>,
    ) -> Result<(), IndexerError> {
        let guard = self
            .metrics
            .checkpoint_db_commit_latency_transactions_chunks
            .start_timer();
        let transformation_guard = self
            .metrics
            .checkpoint_db_commit_latency_transactions_chunks_transformation
            .start_timer();
        let transactions = transactions
            .iter()
            .map(StoredTransaction::from)
            .collect::<Vec<_>>();
        drop(transformation_guard);

        transactional_blocking_with_retry!(
            &self.blocking_cp,
            |conn| {
                for transaction_chunk in transactions.chunks(PG_COMMIT_CHUNK_SIZE_INTRA_DB_TX) {
                    diesel::insert_into(transactions::table)
                        .values(transaction_chunk)
                        .on_conflict_do_nothing()
                        .execute(conn)
                        .map_err(IndexerError::from)
                        .context("Failed to write transactions to PostgresDB")?;
                }
                Ok::<(), IndexerError>(())
            },
            Duration::from_secs(60)
        )
        .tap(|_| {
            let elapsed = guard.stop_and_record();
            info!(
                elapsed,
                "Persisted {} chunked transactions",
                transactions.len()
            )
        })
    }

    fn persist_events_chunk(&self, events: Vec<IndexedEvent>) -> Result<(), IndexerError> {
        let guard = self
            .metrics
            .checkpoint_db_commit_latency_events_chunks
            .start_timer();
        let events = events
            .into_iter()
            .map(StoredEvent::from)
            .collect::<Vec<_>>();
        transactional_blocking_with_retry!(
            &self.blocking_cp,
            |conn| {
                for event_chunk in events.chunks(PG_COMMIT_CHUNK_SIZE_INTRA_DB_TX) {
                    diesel::insert_into(events::table)
                        .values(event_chunk)
                        .on_conflict_do_nothing()
                        .execute(conn)
                        .map_err(IndexerError::from)
                        .context("Failed to write events to PostgresDB")?;
                }
                Ok::<(), IndexerError>(())
            },
            Duration::from_secs(60)
        )
        .tap(|_| {
            let elapsed = guard.stop_and_record();
            info!(elapsed, "Persisted {} chunked events", events.len())
        })
    }

    fn persist_packages(&self, packages: Vec<IndexedPackage>) -> Result<(), IndexerError> {
        if packages.is_empty() {
            return Ok(());
        }
        let guard = self
            .metrics
            .checkpoint_db_commit_latency_packages
            .start_timer();
        let packages = packages
            .into_iter()
            .map(StoredPackage::from)
            .collect::<Vec<_>>();
        transactional_blocking_with_retry!(
            &self.blocking_cp,
            |conn| {
                for packages_chunk in packages.chunks(PG_COMMIT_CHUNK_SIZE_INTRA_DB_TX) {
                    diesel::insert_into(packages::table)
                        .values(packages_chunk)
                        // System packages such as 0x2/0x9 will have their package_id
                        // unchanged during upgrades. In this case, we override the modules
                        // TODO: race condition is possible here. Figure out how to avoid/detect
                        .on_conflict(packages::package_id)
                        .do_update()
                        .set(packages::move_package.eq(excluded(packages::move_package)))
                        .execute(conn)
                        .map_err(IndexerError::from)
                        .context("Failed to write packages to PostgresDB")?;
                }
                Ok::<(), IndexerError>(())
            },
            Duration::from_secs(60)
        )
        .tap(|_| {
            let elapsed = guard.stop_and_record();
            info!(elapsed, "Persisted {} packages", packages.len())
        })
    }

    fn persist_tx_indices_chunk(&self, indices: Vec<TxIndex>) -> Result<(), IndexerError> {
        let guard = self
            .metrics
            .checkpoint_db_commit_latency_tx_indices_chunks
            .start_timer();
        let indices = indices
            .into_iter()
            .map(StoredTxIndex::from)
            .collect::<Vec<_>>();
        transactional_blocking_with_retry!(
            &self.blocking_cp,
            |conn| {
                for indices_chunk in indices.chunks(PG_COMMIT_CHUNK_SIZE_INTRA_DB_TX) {
                    diesel::insert_into(tx_indices::table)
                        .values(indices_chunk)
                        .on_conflict_do_nothing()
                        .execute(conn)
                        .map_err(IndexerError::from)
                        .context("Failed to write tx_indices to PostgresDB")?;
                }
                Ok::<(), IndexerError>(())
            },
            Duration::from_secs(60)
        )
        .tap(|_| {
            let elapsed = guard.stop_and_record();
            info!(elapsed, "Persisted {} chunked tx_indices", indices.len())
        })
    }

    fn persist_epoch(&self, data: &Vec<EpochToCommit>) -> Result<(), IndexerError> {
        if data.is_empty() {
            return Ok(());
        }
        let guard = self
            .metrics
            .checkpoint_db_commit_latency_epochs
            .start_timer();
        transactional_blocking_with_retry!(
            &self.blocking_cp,
            |conn| {
                for epoch_data in data {
                    if let Some(last_epoch) = &epoch_data.last_epoch {
                        let last_epoch_id = last_epoch.epoch;
                        let last_epoch = StoredEpochInfo::from_epoch_end_info(last_epoch);
                        info!(last_epoch_id, "Persisting epoch end data: {:?}", last_epoch);
                        diesel::insert_into(epochs::table)
                            .values(last_epoch)
                            .on_conflict(epochs::epoch)
                            .do_update()
                            .set((
                                // Note: it's crucial that we don't include epoch beinning info
                                // below as we don't want to override them. They are
                                // validators, first_checkpoint_id, epoch_start_timestamp and so on.
                                epochs::epoch_total_transactions
                                    .eq(excluded(epochs::epoch_total_transactions)),
                                epochs::last_checkpoint_id.eq(excluded(epochs::last_checkpoint_id)),
                                epochs::epoch_end_timestamp
                                    .eq(excluded(epochs::epoch_end_timestamp)),
                                epochs::storage_fund_reinvestment
                                    .eq(excluded(epochs::storage_fund_reinvestment)),
                                epochs::storage_charge.eq(excluded(epochs::storage_charge)),
                                epochs::storage_rebate.eq(excluded(epochs::storage_rebate)),
                                epochs::storage_fund_balance
                                    .eq(excluded(epochs::storage_fund_balance)),
                                epochs::stake_subsidy_amount
                                    .eq(excluded(epochs::stake_subsidy_amount)),
                                epochs::total_gas_fees.eq(excluded(epochs::total_gas_fees)),
                                epochs::total_stake_rewards_distributed
                                    .eq(excluded(epochs::total_stake_rewards_distributed)),
                                epochs::leftover_storage_fund_inflow
                                    .eq(excluded(epochs::leftover_storage_fund_inflow)),
                                epochs::new_total_stake.eq(excluded(epochs::new_total_stake)),
                                epochs::epoch_commitments.eq(excluded(epochs::epoch_commitments)),
                                epochs::next_epoch_reference_gas_price
                                    .eq(excluded(epochs::next_epoch_reference_gas_price)),
                                epochs::next_epoch_protocol_version
                                    .eq(excluded(epochs::next_epoch_protocol_version)),
                            ))
                            .execute(conn)?;
                    }
                    let epoch_id = epoch_data.new_epoch.epoch;
                    info!(epoch_id, "Persisting initial epoch state");
                    let new_epoch =
                        StoredEpochInfo::from_epoch_beginning_info(&epoch_data.new_epoch);
                    diesel::insert_into(epochs::table)
                        .values(new_epoch)
                        .on_conflict_do_nothing()
                        .execute(conn)?;
                }
                Ok::<(), IndexerError>(())
            },
            Duration::from_secs(60)
        )
        .tap(|_| {
            let elapsed = guard.stop_and_record();
            info!(elapsed, "Persisted {} epochs", data.len())
        })
    }

    fn get_network_total_transactions_by_end_of_epoch(
        &self,
        epoch: u64,
    ) -> Result<u64, IndexerError> {
        read_only_blocking!(&self.blocking_cp, |conn| {
            checkpoints::table
                .filter(checkpoints::epoch.eq(epoch as i64))
                .select(max(checkpoints::network_total_transactions))
                .first::<Option<i64>>(conn)
                .map(|o| o.unwrap_or(0))
        })
        .context("Failed to get network total transactions in epoch")
        .map(|v| v as u64)
    }

    async fn execute_in_blocking_worker<F, R>(&self, f: F) -> Result<R, IndexerError>
    where
        F: FnOnce(Self) -> Result<R, IndexerError> + Send + 'static,
        R: Send + 'static,
    {
        let this = self.clone();
        let current_span = tracing::Span::current();
        tokio::task::spawn_blocking(move || {
            let _guard = current_span.enter();
            f(this)
        })
        .await
        .map_err(Into::into)
        .and_then(std::convert::identity)
    }

    fn spawn_blocking_task<F, R>(
        &self,
        f: F,
    ) -> tokio::task::JoinHandle<std::result::Result<R, IndexerError>>
    where
        F: FnOnce(Self) -> Result<R, IndexerError> + Send + 'static,
        R: Send + 'static,
    {
        let this = self.clone();
        let current_span = tracing::Span::current();
        tokio::task::spawn_blocking(move || {
            let _guard = current_span.enter();
            f(this)
        })
    }
}

#[async_trait]
impl IndexerStoreV2 for PgIndexerStoreV2 {
    type ModuleCache = SyncModuleCache<IndexerStoreModuleResolver>;

    async fn get_latest_tx_checkpoint_sequence_number(&self) -> Result<Option<u64>, IndexerError> {
        self.execute_in_blocking_worker(|this| this.get_latest_tx_checkpoint_sequence_number())
            .await
    }

    async fn get_object_read(
        &self,
        object_id: ObjectID,
        version: Option<SequenceNumber>,
    ) -> Result<ObjectRead, IndexerError> {
        self.execute_in_blocking_worker(move |this| this.get_object_read(object_id, version))
            .await
    }

    async fn persist_objects(
        &self,
        object_changes: Vec<TransactionObjectChangesToCommit>,
    ) -> Result<(), IndexerError> {
        if object_changes.is_empty() {
            return Ok(());
        }
        let guard = self
            .metrics
            .checkpoint_db_commit_latency_objects
            .start_timer();
        let objects = make_final_list_of_objects_to_commit(object_changes);
        let len = objects.len();
        let chunks = chunk!(objects, self.parallel_objects_chunk_size);
        let futures = chunks
            .into_iter()
            .map(|c| self.spawn_blocking_task(move |this| this.persist_objects_chunk(c)))
            .collect::<Vec<_>>();

        futures::future::join_all(futures)
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| {
                IndexerError::PostgresWriteError(format!(
                    "Failed to persist all objects chunks: {:?}",
                    e
                ))
            })?;
        let elapsed = guard.stop_and_record();
        info!(elapsed, "Persisted {} objects", len);
        Ok(())
    }

    async fn persist_checkpoints(
        &self,
        checkpoints: Vec<IndexedCheckpoint>,
    ) -> Result<(), IndexerError> {
        self.execute_in_blocking_worker(move |this| this.persist_checkpoints(checkpoints))
            .await
    }

    async fn persist_transactions(
        &self,
        transactions: Vec<IndexedTransaction>,
    ) -> Result<(), IndexerError> {
        let guard = self
            .metrics
            .checkpoint_db_commit_latency_transactions
            .start_timer();
        let len = transactions.len();

        let chunks = chunk!(transactions, self.parallel_chunk_size);
        let futures = chunks
            .into_iter()
            .map(|c| self.spawn_blocking_task(move |this| this.persist_transactions_chunk(c)))
            .collect::<Vec<_>>();

        futures::future::join_all(futures)
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| {
                IndexerError::PostgresWriteError(format!(
                    "Failed to persist all transactions chunks: {:?}",
                    e
                ))
            })?;
        let elapsed = guard.stop_and_record();
        info!(elapsed, "Persisted {} transactions", len);
        Ok(())
    }

    async fn persist_events(&self, events: Vec<IndexedEvent>) -> Result<(), IndexerError> {
        if events.is_empty() {
            return Ok(());
        }
        let len = events.len();
        let guard = self
            .metrics
            .checkpoint_db_commit_latency_events
            .start_timer();
        let chunks = chunk!(events, self.parallel_chunk_size);
        let futures = chunks
            .into_iter()
            .map(|c| self.spawn_blocking_task(move |this| this.persist_events_chunk(c)))
            .collect::<Vec<_>>();

        futures::future::join_all(futures)
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| {
                IndexerError::PostgresWriteError(format!(
                    "Failed to persist all events chunks: {:?}",
                    e
                ))
            })?;
        let elapsed = guard.stop_and_record();
        info!(elapsed, "Persisted {} events", len);
        Ok(())
    }

    async fn persist_packages(&self, packages: Vec<IndexedPackage>) -> Result<(), IndexerError> {
        if packages.is_empty() {
            return Ok(());
        }
        self.execute_in_blocking_worker(move |this| this.persist_packages(packages))
            .await
    }

    async fn persist_tx_indices(&self, indices: Vec<TxIndex>) -> Result<(), IndexerError> {
        if indices.is_empty() {
            return Ok(());
        }
        let len = indices.len();
        let guard = self
            .metrics
            .checkpoint_db_commit_latency_tx_indices
            .start_timer();
        let chunks = chunk!(indices, self.parallel_chunk_size);

        let futures = chunks
            .into_iter()
            .map(|c| self.spawn_blocking_task(move |this| this.persist_tx_indices_chunk(c)))
            .collect::<Vec<_>>();
        futures::future::join_all(futures)
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| {
                IndexerError::PostgresWriteError(format!(
                    "Failed to persist all tx_indices chunks: {:?}",
                    e
                ))
            })?;
        let elapsed = guard.stop_and_record();
        info!(elapsed, "Persisted {} tx_indices", len);
        Ok(())
    }

    async fn persist_epoch(&self, data: Vec<EpochToCommit>) -> Result<(), IndexerError> {
        self.execute_in_blocking_worker(move |this| this.persist_epoch(&data))
            .await
    }

    async fn get_network_total_transactions_by_end_of_epoch(
        &self,
        epoch: u64,
    ) -> Result<u64, IndexerError> {
        self.execute_in_blocking_worker(move |this| {
            this.get_network_total_transactions_by_end_of_epoch(epoch)
        })
        .await
    }

    fn module_cache(&self) -> Arc<Self::ModuleCache> {
        self.module_cache.clone()
    }
}

/// Construct deleted objects and mutated objects to commit.
/// In particular, filter mutated objects updates that would
/// be override immediately.
fn make_final_list_of_objects_to_commit(
    tx_object_changes: Vec<TransactionObjectChangesToCommit>,
) -> Vec<ObjectChangeToCommit> {
    let deleted_objects = tx_object_changes
        .iter()
        .flat_map(|changes| &changes.deleted_objects)
        .map(|o| o.0)
        .collect::<HashSet<_>>();

    let mutated_objects = tx_object_changes
        .into_iter()
        .flat_map(|changes| changes.changed_objects);
    let mut latest_objects = HashMap::new();
    for object in mutated_objects {
        if deleted_objects.contains(&object.object_id) {
            continue;
        }
        match latest_objects.entry(object.object_id) {
            Entry::Vacant(e) => {
                e.insert(object);
            }
            Entry::Occupied(mut e) => {
                if object.object_version > e.get().object_version {
                    e.insert(object);
                }
            }
        }
    }
    deleted_objects
        .into_iter()
        .map(ObjectChangeToCommit::DeletedObject)
        .chain(
            latest_objects
                .into_values()
                .map(StoredObject::from)
                .map(ObjectChangeToCommit::MutatedObject),
        )
        .collect()
}

enum ObjectChangeToCommit {
    MutatedObject(StoredObject),
    DeletedObject(ObjectID),
}
