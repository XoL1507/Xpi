// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::errors::IndexerError;
use crate::models::checkpoints::Checkpoint;
use crate::models::error_logs::commit_error_logs;
use crate::models::events::Event;
use crate::models::objects::Object;
use crate::models::transactions::Transaction;

use crate::schema::{
    addresses, checkpoints, checkpoints::dsl as checkpoints_dsl, events, move_calls,
    move_calls::dsl as move_calls_dsl, objects, objects::dsl as objects_dsl, objects_history,
    packages, recipients, recipients::dsl as recipients_dsl, transactions,
    transactions::dsl as transactions_dsl,
};
use crate::store::indexer_store::TemporaryCheckpointStore;
use crate::store::module_resolver::IndexerModuleResolver;
use crate::store::{IndexerStore, TemporaryEpochStore};
use crate::{get_pg_pool_connection, PgConnectionPool};
use async_trait::async_trait;
use diesel::dsl::{count, max};
use diesel::sql_types::VarChar;
use diesel::upsert::excluded;
use diesel::{ExpressionMethods, PgArrayExpressionMethods};
use diesel::{OptionalExtension, QueryableByName};
use diesel::{QueryDsl, RunQueryDsl};
use move_bytecode_utils::module_cache::SyncModuleCache;
use std::collections::BTreeMap;
use std::sync::Arc;
use sui_json_rpc_types::{CheckpointId, EventPage, SuiEventEnvelope};
use sui_types::base_types::{ObjectID, SequenceNumber};
use sui_types::committee::EpochId;
use sui_types::object::ObjectRead;
use tracing::{error, info};

use sui_types::event::EventID;
use sui_types::query::EventQuery;

const MAX_EVENT_PAGE_SIZE: usize = 1000;

const GET_PARTITION_SQL: &str = r#"
SELECT parent.relname                           AS table_name,
       MAX(SUBSTRING(child.relname FROM '\d$')) AS last_partition
FROM pg_inherits
         JOIN pg_class parent ON pg_inherits.inhparent = parent.oid
         JOIN pg_class child ON pg_inherits.inhrelid = child.oid
         JOIN pg_namespace nmsp_parent ON nmsp_parent.oid = parent.relnamespace
         JOIN pg_namespace nmsp_child ON nmsp_child.oid = child.relnamespace
WHERE parent.relkind = 'p'
GROUP BY table_name;
"#;

#[derive(Clone)]
pub struct PgIndexerStore {
    cp: PgConnectionPool,
    partition_manager: PartitionManager,
    pub module_cache: Arc<SyncModuleCache<IndexerModuleResolver>>,
}

impl PgIndexerStore {
    pub fn new(cp: PgConnectionPool) -> Self {
        let module_cache = Arc::new(SyncModuleCache::new(IndexerModuleResolver::new(cp.clone())));
        PgIndexerStore {
            cp: cp.clone(),
            partition_manager: PartitionManager::new(cp).unwrap(),
            module_cache,
        }
    }
}

#[async_trait]
impl IndexerStore for PgIndexerStore {
    fn get_latest_checkpoint_sequence_number(&self) -> Result<i64, IndexerError> {
        let mut pg_pool_conn = get_pg_pool_connection(&self.cp)?;
        pg_pool_conn
            .build_transaction()
            .read_only()
            .run(|conn| {
                checkpoints_dsl::checkpoints
                    .select(max(checkpoints::sequence_number))
                    .first::<Option<i64>>(conn)
                    // -1 to differentiate between no checkpoints and the first checkpoint
                    .map(|o| o.unwrap_or(-1))
            })
            .map_err(|e| {
                IndexerError::PostgresReadError(format!(
                    "Failed reading latest checkpoint sequence number in PostgresDB with error {:?}",
                    e
                ))
            })
    }

    fn get_checkpoint(&self, id: CheckpointId) -> Result<Checkpoint, IndexerError> {
        let mut pg_pool_conn = get_pg_pool_connection(&self.cp)?;
        pg_pool_conn
            .build_transaction()
            .read_only()
            .run(|conn| match id {
                CheckpointId::SequenceNumber(seq) => checkpoints_dsl::checkpoints
                    .filter(checkpoints::sequence_number.eq(seq as i64))
                    .limit(1)
                    .first::<Checkpoint>(conn),
                CheckpointId::Digest(digest) => checkpoints_dsl::checkpoints
                    .filter(checkpoints::checkpoint_digest.eq(digest.base58_encode()))
                    .limit(1)
                    .first::<Checkpoint>(conn),
            })
            .map_err(|e| {
                IndexerError::PostgresReadError(format!(
                    "Failed reading previous checkpoint in PostgresDB with error {:?}",
                    e
                ))
            })
    }

    fn get_event(&self, id: EventID) -> Result<Event, IndexerError> {
        let mut pg_pool_conn = get_pg_pool_connection(&self.cp)?;
        pg_pool_conn
            .build_transaction()
            .read_only()
            .run(|conn| {
                events::table
                    .filter(events::dsl::transaction_digest.eq(id.tx_digest.base58_encode()))
                    .filter(events::dsl::event_sequence.eq(id.event_seq))
                    .first::<Event>(conn)
            })
            .map_err(|e| {
                IndexerError::PostgresReadError(format!(
                    "Failed reading event in PostgresDB with error {:?}",
                    e
                ))
            })
    }

    fn get_events(
        &self,
        query: EventQuery,
        cursor: Option<EventID>,
        limit: Option<usize>,
        descending_order: bool,
    ) -> Result<EventPage, IndexerError> {
        let mut pg_pool_conn = get_pg_pool_connection(&self.cp)?;
        let mut boxed_query = events::table.into_boxed();
        match query {
            EventQuery::All => {}
            EventQuery::Transaction(digest) => {
                boxed_query =
                    boxed_query.filter(events::dsl::transaction_digest.eq(digest.base58_encode()));
            }
            EventQuery::MoveModule { package, module } => {
                boxed_query = boxed_query
                    .filter(events::dsl::package.eq(package.to_string()))
                    .filter(events::dsl::module.eq(module));
            }
            EventQuery::MoveEvent(struct_name) => {
                boxed_query = boxed_query.filter(events::dsl::event_type.eq(struct_name));
            }
            EventQuery::Sender(sender) => {
                boxed_query = boxed_query.filter(events::dsl::sender.eq(sender.to_string()));
            }
            EventQuery::TimeRange {
                start_time,
                end_time,
            } => {
                boxed_query = boxed_query
                    .filter(events::dsl::event_time_ms.ge(start_time as i64))
                    .filter(events::dsl::event_time_ms.lt(end_time as i64));
            }
            EventQuery::EventType(_) => {}
            _ => {
                return Err(IndexerError::NotImplementedError(
                    "Querying events by Recipient and Object is deprecated.".to_string(),
                ));
            }
        }

        let mut page_limit = limit.unwrap_or(MAX_EVENT_PAGE_SIZE);
        if page_limit > MAX_EVENT_PAGE_SIZE {
            Err(IndexerError::InvalidArgumentError(format!(
                "Limit {} exceeds the maximum page size {}",
                page_limit, MAX_EVENT_PAGE_SIZE
            )))?;
        }
        // fetch one more item to tell if there is next page
        page_limit += 1;

        let pg_cursor = cursor
            .map(|c| {
                self.get_event(c)?
                    .id
                    .ok_or_else(|| IndexerError::PostgresReadError("Event ID is None".to_string()))
            })
            .transpose()?;
        let events_vec: Vec<Event> = pg_pool_conn
            .build_transaction()
            .read_only()
            .run(|conn| {
                if let Some(pg_cursor) = pg_cursor {
                    if descending_order {
                        boxed_query = boxed_query.filter(events::dsl::id.lt(pg_cursor));
                    } else {
                        boxed_query = boxed_query.filter(events::dsl::id.gt(pg_cursor));
                    }
                }
                if descending_order {
                    boxed_query = boxed_query.order(events::id.desc());
                } else {
                    boxed_query = boxed_query.order(events::id.asc());
                }
                boxed_query.load(conn)
            })
            .map_err(|e| {
                IndexerError::PostgresReadError(format!(
                    "Failed reading events in PostgresDB with error {:?}",
                    e
                ))
            })?;

        let mut event_envelope_vec: Vec<SuiEventEnvelope> = events_vec
            .into_iter()
            .map(|event| event.try_into())
            .collect::<Result<Vec<_>, _>>()?;
        // reset to original limit for checking and truncating
        page_limit -= 1;
        let has_next_page = event_envelope_vec.len() > page_limit;
        event_envelope_vec.truncate(page_limit);
        let next_cursor = event_envelope_vec.last().map(|e| e.id.clone());
        Ok(EventPage {
            data: event_envelope_vec,
            next_cursor,
            has_next_page,
        })
    }

    fn get_total_transaction_number(&self) -> Result<i64, IndexerError> {
        let mut pg_pool_conn = get_pg_pool_connection(&self.cp)?;
        pg_pool_conn
            .build_transaction()
            .read_only()
            .run(|conn| {
                transactions_dsl::transactions
                    .select(count(transactions_dsl::id))
                    .first::<i64>(conn)
            })
            .map_err(|e| {
                IndexerError::PostgresReadError(format!(
                    "Failed reading total transaction number with err: {:?}",
                    e
                ))
            })
    }

    fn get_transaction_by_digest(&self, txn_digest: &str) -> Result<Transaction, IndexerError> {
        let mut pg_pool_conn = get_pg_pool_connection(&self.cp)?;
        pg_pool_conn
            .build_transaction()
            .read_only()
            .run(|conn| {
                transactions_dsl::transactions
                    .filter(transactions_dsl::transaction_digest.eq(txn_digest))
                    .first::<Transaction>(conn)
            })
            .map_err(|e| {
                IndexerError::PostgresReadError(format!(
                    "Failed reading transaction with digest {} and err: {:?}",
                    txn_digest, e
                ))
            })
    }

    fn get_transaction_sequence_by_digest(
        &self,
        txn_digest: Option<String>,
        is_descending: bool,
    ) -> Result<Option<i64>, IndexerError> {
        txn_digest
            .map(|digest| {
                let mut pg_pool_conn = get_pg_pool_connection(&self.cp)?;
                pg_pool_conn
                    .build_transaction()
                    .read_only()
                    .run(|conn| {
                        let mut boxed_query = transactions_dsl::transactions
                            .filter(transactions_dsl::transaction_digest.eq(digest.clone()))
                            .select(transactions_dsl::id)
                            .into_boxed();
                        if is_descending {
                            boxed_query = boxed_query.order(transactions_dsl::id.desc());
                        } else {
                            boxed_query = boxed_query.order(transactions_dsl::id.asc());
                        }
                        boxed_query.first::<i64>(conn)
                    })
                    .map_err(|e| {
                        IndexerError::PostgresReadError(format!(
                            "Failed reading transaction sequence with digest {} and err: {:?}",
                            digest, e
                        ))
                    })
            })
            .transpose()
    }

    fn get_object(
        &self,
        object_id: ObjectID,
        version: Option<SequenceNumber>,
    ) -> Result<ObjectRead, IndexerError> {
        let mut pg_pool_conn = get_pg_pool_connection(&self.cp)?;
        let object = pg_pool_conn
            .build_transaction()
            .read_only()
            .run(|conn| {
                if let Some(version) = version {
                    objects_history::dsl::objects_history
                        .filter(objects_history::object_id.eq(object_id.to_string()))
                        .filter(objects_history::version.eq(version.value() as i64))
                        .get_result(conn)
                        .optional()
                } else {
                    objects_dsl::objects
                        .filter(objects_dsl::object_id.eq(object_id.to_string()))
                        .first::<Object>(conn)
                        .optional()
                }
            })
            .map_err(|e| {
                IndexerError::PostgresReadError(format!(
                    "Failed reading object with id {} and err: {:?}",
                    object_id, e
                ))
            })?;

        match object {
            None => Ok(ObjectRead::NotExists(object_id)),
            Some(o) => o.try_into_object_read(&self.module_cache),
        }
    }

    fn get_move_call_sequence_by_digest(
        &self,
        txn_digest: Option<String>,
        is_descending: bool,
    ) -> Result<Option<i64>, IndexerError> {
        txn_digest
            .map(|digest| {
                let mut pg_pool_conn = get_pg_pool_connection(&self.cp)?;
                pg_pool_conn
                    .build_transaction()
                    .read_only()
                    .run(|conn| {
                        let mut boxed_query = move_calls_dsl::move_calls
                            .filter(move_calls_dsl::transaction_digest.eq(digest.clone()))
                            .into_boxed();
                        if is_descending {
                            boxed_query = boxed_query.order(move_calls_dsl::id.desc());
                        } else {
                            boxed_query = boxed_query.order(move_calls_dsl::id.asc());
                        }
                        boxed_query.select(move_calls_dsl::id).first::<i64>(conn)
                    })
                    .map_err(|e| {
                        IndexerError::PostgresReadError(format!(
                            "Failed reading move call sequence with digest {} and err: {:?}",
                            digest, e
                        ))
                    })
            })
            .transpose()
    }

    fn get_recipient_sequence_by_digest(
        &self,
        txn_digest: Option<String>,
        is_descending: bool,
    ) -> Result<Option<i64>, IndexerError> {
        txn_digest
            .map(|txn_digest| {
                let mut pg_pool_conn = get_pg_pool_connection(&self.cp)?;
                pg_pool_conn
                    .build_transaction()
                    .read_only()
                    .run(|conn| {
                        let mut boxed_query = recipients_dsl::recipients
                            .filter(recipients_dsl::transaction_digest.eq(&txn_digest))
                            .into_boxed();
                        if is_descending {
                            boxed_query = boxed_query.order(recipients_dsl::id.desc());
                        } else {
                            boxed_query = boxed_query.order(recipients_dsl::id.asc());
                        }
                        boxed_query.select(recipients_dsl::id).first::<i64>(conn)
                    })
                    .map_err(|e| {
                        IndexerError::PostgresReadError(format!(
                            "Failed reading recipients sequence with digest {} and err: {:?}",
                            txn_digest, e
                        ))
                    })
            })
            .transpose()
    }

    fn get_all_transaction_digest_page(
        &self,
        start_sequence: Option<i64>,
        limit: usize,
        is_descending: bool,
    ) -> Result<Vec<String>, IndexerError> {
        let mut pg_pool_conn = get_pg_pool_connection(&self.cp)?;
        pg_pool_conn
            .build_transaction()
            .read_only()
            .run(|conn| {
                let mut boxed_query = transactions_dsl::transactions.into_boxed();
                if is_descending {
                    boxed_query = boxed_query.order(transactions_dsl::id.desc());
                } else {
                    boxed_query = boxed_query.order(transactions_dsl::id.asc());
                }

                if is_descending {
                    boxed_query
                        .order(transactions_dsl::id.desc())
                        .limit((limit + 1) as i64)
                        .select(transactions_dsl::transaction_digest)
                        .load::<String>(conn)
                } else {
                    boxed_query
                        .order(transactions_dsl::id.asc())
                        .limit((limit + 1) as i64)
                        .select(transactions_dsl::transaction_digest)
                        .load::<String>(conn)
                }
            }).map_err(|e| {
            IndexerError::PostgresReadError(format!(
                "Failed reading all transaction digests with start_sequence {:?} and limit {} and err: {:?}",
                start_sequence, limit, e
            ))
        })
    }

    fn get_transaction_digest_page_by_move_call(
        &self,
        package_name: String,
        module_name: Option<String>,
        function_name: Option<String>,
        start_sequence: Option<i64>,
        limit: usize,
        is_descending: bool,
    ) -> Result<Vec<String>, IndexerError> {
        let mut pg_pool_conn = get_pg_pool_connection(&self.cp)?;
        pg_pool_conn
            .build_transaction()
            .read_only()
            .run(|conn| {
                let mut builder = move_calls_dsl::move_calls.filter(move_calls_dsl::move_package.eq(package_name.clone()))
                    .group_by(move_calls_dsl::transaction_digest)
                    .select((move_calls_dsl::transaction_digest, max(move_calls_dsl::id)))
                    .into_boxed();
                if let Some(module_name) = module_name.clone() {
                    builder = builder.filter(move_calls_dsl::move_module.eq(module_name));
                }
                if let Some(function_name) = function_name.clone() {
                    builder = builder.filter(move_calls_dsl::move_function.eq(function_name));
                }
                if let Some(start_sequence) = start_sequence {
                    if is_descending {
                        builder = builder.filter(move_calls_dsl::id.lt(start_sequence));
                    } else {
                        builder = builder.filter(move_calls_dsl::id.gt(start_sequence));
                    }
                }

                if is_descending {
                    builder.order(move_calls_dsl::id.desc())
                        .limit(limit as i64)
                        .load::<(String, Option<i64>)>(conn)
                } else {
                    builder.order(move_calls_dsl::id.asc())
                        .limit(limit as i64)
                        .load::<(String, Option<i64>)>(conn)
                }
            }).map(|v| v.into_iter().map(|(digest, _)| digest).collect()).map_err(|e| {
            IndexerError::PostgresReadError(format!(
                "Failed reading transaction digests with package_name {} module_name {:?} and function_name {:?} and start_sequence {:?} and limit {} and err: {:?}",
                package_name, module_name, function_name, start_sequence, limit, e
            ))
        })
    }

    fn get_transaction_digest_page_by_mutated_object(
        &self,
        object_id: String,
        start_sequence: Option<i64>,
        limit: usize,
        is_descending: bool,
    ) -> Result<Vec<String>, IndexerError> {
        let mut pg_pool_conn = get_pg_pool_connection(&self.cp)?;
        pg_pool_conn
            .build_transaction()
            .read_only()
            .run(|conn| {
                let mut boxed_query = transactions_dsl::transactions
                    .filter(transactions_dsl::mutated.contains(vec![Some(object_id.clone())]))
                    .into_boxed();
                if let Some(start_sequence) = start_sequence {
                    if is_descending {
                        boxed_query = boxed_query
                            .filter(transactions_dsl::id.lt(start_sequence));
                    } else {
                        boxed_query = boxed_query
                            .filter(transactions_dsl::id.gt(start_sequence));
                    }
                }
                if is_descending {
                    boxed_query
                        .order(transactions_dsl::id.desc())
                        .limit(limit as i64)
                        .select(transactions_dsl::transaction_digest)
                        .load::<String>(conn)
                } else {
                    boxed_query
                        .order(transactions_dsl::id.asc())
                        .limit(limit as i64)
                        .select(transactions_dsl::transaction_digest)
                        .load::<String>(conn)
                }
            }).map_err(|e| {
            IndexerError::PostgresReadError(format!(
                "Failed reading transaction digests by mutated object id {} with start_sequence {:?} and limit {} and err: {:?}",
                object_id, start_sequence, limit, e
            ))
        })
    }

    fn get_transaction_digest_page_by_sender_address(
        &self,
        sender_address: String,
        start_sequence: Option<i64>,
        limit: usize,
        is_descending: bool,
    ) -> Result<Vec<String>, IndexerError> {
        let mut pg_pool_conn = get_pg_pool_connection(&self.cp)?;
        pg_pool_conn
            .build_transaction()
            .read_only()
            .run(|conn| {
                    let mut boxed_query = transactions_dsl::transactions
                        .filter(transactions_dsl::sender.eq(sender_address.clone()))
                        .into_boxed();
                    if let Some(start_sequence) = start_sequence {
                        if is_descending {
                            boxed_query = boxed_query
                                .filter(transactions_dsl::id.lt(start_sequence));
                        } else {
                            boxed_query = boxed_query
                                .filter(transactions_dsl::id.gt(start_sequence));
                        }
                    }

                    if is_descending {
                        boxed_query
                            .order(transactions_dsl::id.desc())
                            .limit(limit as i64)
                            .select(transactions_dsl::transaction_digest)
                            .load::<String>(conn)
                    } else {
                        boxed_query
                            .order(transactions_dsl::id.asc())
                            .limit(limit as i64)
                            .select(transactions_dsl::transaction_digest)
                            .load::<String>(conn)
                    }
            }).map_err(|e| {
            IndexerError::PostgresReadError(format!(
                "Failed reading transaction digests by sender address {} with start_sequence {:?} and limit {} and err: {:?}",
                sender_address, start_sequence, limit, e
            ))
        })
    }

    fn get_transaction_digest_page_by_recipient_address(
        &self,
        recipient_address: String,
        start_sequence: Option<i64>,
        limit: usize,
        is_descending: bool,
    ) -> Result<Vec<String>, IndexerError> {
        #[derive(QueryableByName, Debug, Clone)]
        struct TempDigestTable {
            #[diesel(sql_type = VarChar)]
            digest_name: String,
        }

        let mut pg_pool_conn = get_pg_pool_connection(&self.cp)?;
        Ok(pg_pool_conn.build_transaction()
            .read_only()
            .run(|conn| {
                let sql_query = format!(
                    "SELECT transaction_digest as digest_name FROM (
                        SELECT transaction_digest, max(id) AS max_id 
                        FROM recipients WHERE recipient = '{}' {} GROUP BY transaction_digest ORDER BY max_id {} LIMIT {}
                    ) AS t",
                    recipient_address.clone(),
                    if let Some(start_sequence) = start_sequence {
                        if is_descending {
                            format!("AND id < {}", start_sequence)
                        } else {
                            format!("AND id > {}", start_sequence)
                        }
                    } else {
                        "".to_string()
                    },
                    if is_descending {
                        "DESC"
                    } else {
                        "ASC"
                    },
                    limit
                );
                diesel::sql_query(sql_query).load(conn)
            })
            .map_err(|e| {
            IndexerError::PostgresReadError(format!(
                "Failed reading transaction digests by recipient address {} with start_sequence {:?} and limit {} and err: {:?}",
                recipient_address, start_sequence, limit, e
            ))
        })?.into_iter().map(|table: TempDigestTable| table.digest_name ).collect())
    }

    fn read_transactions(
        &self,
        last_processed_id: i64,
        limit: usize,
    ) -> Result<Vec<Transaction>, IndexerError> {
        let mut pg_pool_conn = get_pg_pool_connection(&self.cp)?;
        pg_pool_conn
            .build_transaction()
            .read_only()
            .run(|conn| {
                transactions_dsl::transactions
                    .filter(transactions_dsl::id.gt(last_processed_id))
                    .limit(limit as i64)
                    .load::<Transaction>(conn)
            })
            .map_err(|e| {
                IndexerError::PostgresReadError(format!(
                    "Failed reading transactions with last_processed_id {} and err: {:?}",
                    last_processed_id, e
                ))
            })
    }

    fn persist_checkpoint(&self, data: &TemporaryCheckpointStore) -> Result<usize, IndexerError> {
        let TemporaryCheckpointStore {
            checkpoint,
            transactions,
            events,
            objects_changes,
            addresses,
            packages,
            move_calls,
            recipients, // TODO: store raw object
        } = data;

        let mut pg_pool_conn = get_pg_pool_connection(&self.cp)?;

        // Commit indexed checkpoint in one transaction
        pg_pool_conn
            .build_transaction()
            .serializable()
            .read_write()
            .run(|conn| {
                diesel::insert_into(checkpoints::table)
                    .values(checkpoint)
                    .execute(conn)?;

                diesel::insert_into(transactions::table)
                    .values(transactions)
                    .execute(conn)?;

                diesel::insert_into(events::table)
                    .values(events)
                    .execute(conn)?;

                // Object need to bulk insert by transaction to prevent same object mutated twice in the same sql call,
                // which will result in "ON CONFLICT DO UPDATE command cannot affect row a second time" error
                for changes in objects_changes {
                    diesel::insert_into(objects::table)
                        .values(&changes.mutated_objects)
                        .on_conflict(objects::object_id)
                        .do_update()
                        .set((
                            objects::epoch.eq(excluded(objects::epoch)),
                            objects::checkpoint.eq(excluded(objects::checkpoint)),
                            objects::version.eq(excluded(objects::version)),
                            objects::object_digest.eq(excluded(objects::object_digest)),
                            objects::owner_address.eq(excluded(objects::owner_address)),
                            objects::previous_transaction.eq(excluded(objects::previous_transaction)),
                            objects::object_status.eq(excluded(objects::object_status)),
                        ))
                        .execute(conn)?;

                    diesel::insert_into(objects::table)
                        .values(&changes.deleted_objects)
                        .on_conflict(objects::object_id)
                        .do_update()
                        .set((
                            objects::epoch.eq(excluded(objects::epoch)),
                            objects::checkpoint.eq(excluded(objects::checkpoint)),
                            objects::version.eq(excluded(objects::version)),
                            objects::previous_transaction.eq(excluded(objects::previous_transaction)),
                            objects::object_status.eq(excluded(objects::object_status)),
                        ))
                        .execute(conn)?;
                }

                // Only insert once for address, skip if conflict
                diesel::insert_into(addresses::table)
                    .values(addresses)
                    .on_conflict(addresses::account_address)
                    .do_nothing()
                    .execute(conn)?;

                diesel::insert_into(packages::table)
                    .values(packages)
                    // We need to keep multiple version of the object in the database because of package upgrade.
                    // Package with the same version number will not change, ignoring conflicts.
                    .on_conflict_do_nothing()
                    .execute(conn)?;

                diesel::insert_into(move_calls::table)
                    .values(move_calls)
                    .execute(conn)?;

                diesel::insert_into(recipients::table)
                    .values(recipients)
                    .execute(conn)
            })
            .map_err(|e| {
                IndexerError::PostgresWriteError(format!(
                    "Failed writing checkpoint to PostgresDB with transactions {:?} and error: {:?}",
                    transactions, e
                ))
            })
    }

    fn persist_epoch(&self, _data: &TemporaryEpochStore) -> Result<usize, IndexerError> {
        // TODO: create new partition on epoch change
        self.partition_manager.advance_epoch(1)
    }

    fn log_errors(&self, errors: Vec<IndexerError>) -> Result<(), IndexerError> {
        if !errors.is_empty() {
            let mut pg_pool_conn = get_pg_pool_connection(&self.cp)?;
            let new_error_logs = errors.into_iter().map(|e| e.into()).collect();
            if let Err(e) = commit_error_logs(&mut pg_pool_conn, new_error_logs) {
                error!("Failed writing error logs with error {:?}", e);
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
struct PartitionManager {
    cp: PgConnectionPool,
    tables: Vec<String>,
}

impl PartitionManager {
    fn new(cp: PgConnectionPool) -> Result<Self, IndexerError> {
        // Find all tables with partition
        let mut manager = Self { cp, tables: vec![] };
        let tables = manager.get_table_partitions()?;
        info!(
            "Found {} tables with partitions : [{:?}]",
            tables.len(),
            tables
        );
        for (table, _) in tables {
            manager.tables.push(table)
        }
        Ok(manager)
    }
    fn advance_epoch(&self, next_epoch_id: EpochId) -> Result<usize, IndexerError> {
        let mut pg_pool_conn = get_pg_pool_connection(&self.cp)?;
        pg_pool_conn
            .build_transaction()
            .read_write().serializable()
            .run(|conn| {
                for table in &self.tables {
                    let sql = format!("CREATE TABLE {table}_partition_{next_epoch_id} PARTITION OF {table} FOR VALUES FROM ({next_epoch_id}) TO ({});", next_epoch_id+1);
                    diesel::sql_query(sql).execute(conn)?;
                }
                Ok::<_, diesel::result::Error>(self.tables.len())
            })
            .map_err(|e| IndexerError::PostgresReadError(e.to_string()))
    }

    fn get_table_partitions(&self) -> Result<BTreeMap<String, String>, IndexerError> {
        let mut pg_pool_conn = get_pg_pool_connection(&self.cp)?;

        #[derive(QueryableByName, Debug, Clone)]
        struct PartitionedTable {
            #[diesel(sql_type = VarChar)]
            table_name: String,
            #[diesel(sql_type = VarChar)]
            last_partition: String,
        }

        Ok(pg_pool_conn
            .build_transaction()
            .read_only()
            .run(|conn| diesel::sql_query(GET_PARTITION_SQL).load(conn))
            .map_err(|e| IndexerError::PostgresReadError(e.to_string()))?
            .into_iter()
            .map(|table: PartitionedTable| (table.table_name, table.last_partition))
            .collect())
    }
}
