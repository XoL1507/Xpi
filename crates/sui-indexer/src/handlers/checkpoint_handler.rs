// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::sync::Arc;

use fastcrypto::traits::ToFromBytes;
use futures::future::join_all;
use futures::FutureExt;
use move_core_types::ident_str;
use prometheus::Registry;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use models::transactions::Transaction;
use mysten_metrics::spawn_monitored_task;
use sui_core::event_handler::EventHandler;
use sui_json_rpc_types::{
    OwnedObjectRef, SuiGetPastObjectRequest, SuiObjectData, SuiObjectDataOptions, SuiRawData,
    SuiTransactionBlockDataAPI, SuiTransactionBlockEffectsAPI,
};
use sui_sdk::error::Error;
use sui_sdk::SuiClient;
use sui_types::messages_checkpoint::{CheckpointCommitment, CheckpointSequenceNumber};
use sui_types::sui_system_state::sui_system_state_summary::SuiSystemStateSummary;
use sui_types::sui_system_state::{get_sui_system_state, SuiSystemStateTrait};
use sui_types::SUI_SYSTEM_ADDRESS;

use crate::errors::IndexerError;
use crate::metrics::IndexerCheckpointHandlerMetrics;
use crate::models;
use crate::models::checkpoints::Checkpoint;
use crate::models::epoch::{DBEpochInfo, SystemEpochInfoEvent};
use crate::models::objects::{DeletedObject, Object, ObjectStatus};
use crate::models::packages::Package;
use crate::store::{
    CheckpointData, IndexerStore, TemporaryCheckpointStore, TemporaryEpochStore,
    TransactionObjectChanges,
};
use crate::types::{CheckpointTransactionBlockResponse, TemporaryTransactionBlockResponseStore};
use crate::utils::multi_get_full_transactions;

const HANDLER_RETRY_INTERVAL_IN_SECS: u64 = 10;
const MULTI_GET_CHUNK_SIZE: usize = 500;

pub struct CheckpointHandler<S> {
    state: S,
    rpc_client: SuiClient,
    event_handler: Arc<EventHandler>,
    metrics: IndexerCheckpointHandlerMetrics,
}

impl<S> CheckpointHandler<S>
where
    S: IndexerStore + Sync + Send + 'static,
{
    pub fn new(
        state: S,
        rpc_client: SuiClient,
        event_handler: Arc<EventHandler>,
        prometheus_registry: &Registry,
    ) -> Self {
        Self {
            state,
            rpc_client,
            event_handler,
            metrics: IndexerCheckpointHandlerMetrics::new(prometheus_registry),
        }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        spawn_monitored_task!(async move {
            let mut checkpoint_handler_exec_res = self.start().await;
            while let Err(e) = &checkpoint_handler_exec_res {
                warn!(
                    "Indexer checkpoint handler failed with error: {:?}, retrying after {:?} secs...",
                    e, HANDLER_RETRY_INTERVAL_IN_SECS
                );
                tokio::time::sleep(std::time::Duration::from_secs(
                    HANDLER_RETRY_INTERVAL_IN_SECS,
                ))
                .await;
                checkpoint_handler_exec_res = self.start().await;
            }
        })
    }

    async fn start(&self) -> Result<(), IndexerError> {
        info!("Indexer checkpoint handler started...");
        let mut next_cursor_sequence_number =
            self.state.get_latest_checkpoint_sequence_number().await?;
        if next_cursor_sequence_number > 0 {
            info!("Resuming from checkpoint {next_cursor_sequence_number}");
        }
        next_cursor_sequence_number += 1;

        loop {
            // Download checkpoint data
            self.metrics.total_checkpoint_requested.inc();
            let checkpoint = self
                .download_checkpoint_data(next_cursor_sequence_number as u64)
                .await.map_err(|e| {
                    error!(
                        "Failed to download checkpoint data with checkpoint sequence number {} and error {:?}, retrying...",
                        next_cursor_sequence_number, e
                    );
                    e
                })?;
            self.metrics.total_checkpoint_received.inc();

            // Index checkpoint data
            let index_guard = self.metrics.checkpoint_index_latency.start_timer();
            let (indexed_checkpoint, indexed_epoch) = self.index_checkpoint(&checkpoint)?;
            index_guard.stop_and_record();

            // for the first epoch, we need to store the epoch data first
            if let Some(store) = indexed_epoch.as_ref() {
                if store.last_epoch.is_none() {
                    let epoch_db_guard = self.metrics.epoch_db_commit_latency.start_timer();
                    self.state.persist_epoch(store).await?;
                    epoch_db_guard.stop_and_record();
                    self.metrics.total_epoch_committed.inc();
                }
            }

            // Write checkpoint to DB
            let tx_count = indexed_checkpoint.transactions.len();
            let object_count = indexed_checkpoint.objects_changes.len();

            let checkpoint_db_guard = self.metrics.checkpoint_db_commit_latency.start_timer();
            self.state.persist_checkpoint(&indexed_checkpoint).await?;
            checkpoint_db_guard.stop_and_record();

            self.metrics.total_checkpoint_committed.inc();
            self.metrics
                .total_transaction_committed
                .inc_by(tx_count as u64);
            info!(
                "Checkpoint {} committed with {tx_count} transactions and {object_count} object changes.",
                next_cursor_sequence_number
            );
            self.metrics
                .transaction_per_checkpoint
                .observe(tx_count as f64);

            // Write epoch to DB if needed
            if let Some(indexed_epoch) = indexed_epoch {
                if indexed_epoch.last_epoch.is_some() {
                    let epoch_db_guard = self.metrics.epoch_db_commit_latency.start_timer();
                    self.state.persist_epoch(&indexed_epoch).await?;
                    epoch_db_guard.stop_and_record();
                    self.metrics.total_epoch_committed.inc();
                }
            }

            // Process websocket subscription
            let ws_guard = self.metrics.subscription_process_latency.start_timer();
            for tx in &checkpoint.transactions {
                self.event_handler
                    .process_events(&tx.effects, &tx.events)
                    .await?;
            }
            ws_guard.stop_and_record();

            next_cursor_sequence_number += 1;
        }
    }

    /// Download all the data we need for one checkpoint.
    async fn download_checkpoint_data(
        &self,
        seq: CheckpointSequenceNumber,
    ) -> Result<CheckpointData, IndexerError> {
        let mut checkpoint = self
            .rpc_client
            .read_api()
            .get_checkpoint(seq.into())
            .await
            .map_err(|e| {
                IndexerError::FullNodeReadingError(format!(
                    "Failed to get checkpoint with sequence number {} and error {:?}",
                    seq, e
                ))
            });
        let mut fn_checkpoint_guard = self
            .metrics
            .fullnode_checkpoint_download_latency
            .start_timer();
        while checkpoint.is_err() {
            // sleep for 1 second and retry if latest checkpoint is not available yet
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            fn_checkpoint_guard = self
                .metrics
                .fullnode_checkpoint_download_latency
                .start_timer();
            checkpoint = self
                .rpc_client
                .read_api()
                .get_checkpoint(seq.into())
                .await
                .map_err(|e| {
                    IndexerError::FullNodeReadingError(format!(
                        "Failed to get checkpoint with sequence number {} and error {:?}",
                        seq, e
                    ))
                })
        }
        fn_checkpoint_guard.stop_and_record();
        // unwrap here is safe because we checked for error above
        let checkpoint = checkpoint.unwrap();

        let fn_transaction_guard = self
            .metrics
            .fullnode_transaction_download_latency
            .start_timer();
        let transactions = join_all(checkpoint.transactions.chunks(MULTI_GET_CHUNK_SIZE).map(
            |digests| multi_get_full_transactions(self.rpc_client.read_api(), digests.to_vec()),
        ))
        .await
        .into_iter()
        .try_fold(vec![], |mut acc, chunk| {
            acc.extend(chunk?);
            Ok::<_, IndexerError>(acc)
        })?;
        fn_transaction_guard.stop_and_record();

        let object_changes = transactions
            .iter()
            .flat_map(|tx| {
                let effects = &tx.effects;
                let created = effects
                    .created()
                    .iter()
                    .map(|o: &OwnedObjectRef| (o, ObjectStatus::Created));
                let mutated = effects
                    .mutated()
                    .iter()
                    .map(|o: &OwnedObjectRef| (o, ObjectStatus::Mutated));
                let unwrapped = effects
                    .unwrapped()
                    .iter()
                    .map(|o: &OwnedObjectRef| (o, ObjectStatus::Unwrapped));
                created.chain(mutated).chain(unwrapped)
            })
            .fold(
                vec![],
                |mut acc, (o, status): (&OwnedObjectRef, ObjectStatus)| {
                    acc.push((o.reference.object_id, o.reference.version, status));
                    acc
                },
            );

        let fn_object_guard = self.metrics.fullnode_object_download_latency.start_timer();
        let rpc = self.rpc_client.clone();
        let changed_objects =
            join_all(object_changes.chunks(MULTI_GET_CHUNK_SIZE).map(|objects| {
                let wanted_past_object_statuses: Vec<ObjectStatus> =
                    objects.iter().map(|(_, _, status)| *status).collect();

                let wanted_past_object_request = objects
                    .iter()
                    .map(|(id, seq_num, _)| SuiGetPastObjectRequest {
                        object_id: *id,
                        version: *seq_num,
                    })
                    .collect();

                rpc.read_api()
                    .try_multi_get_parsed_past_object(
                        wanted_past_object_request,
                        SuiObjectDataOptions::bcs_lossless(),
                    )
                    .map(move |resp| (resp, wanted_past_object_statuses))
            }))
            .await
            .into_iter()
            .try_fold(vec![], |mut acc, chunk| {
                let object_datas = chunk.0?.into_iter().try_fold(vec![], |mut acc, resp| {
                    let object_data = resp.into_object()?;
                    acc.push(object_data);
                    Ok::<Vec<SuiObjectData>, Error>(acc)
                })?;
                let mutated_object_chunk = chunk.1.into_iter().zip(object_datas);
                acc.extend(mutated_object_chunk);
                Ok::<_, Error>(acc)
            })
            .map_err(|e| {
                IndexerError::SerdeError(format!(
                    "Failed to generate changed objects of checkpoint sequence {} with err {:?}",
                    seq, e
                ))
            })?;
        fn_object_guard.stop_and_record();

        Ok(CheckpointData {
            checkpoint,
            transactions,
            changed_objects,
        })
    }

    fn index_checkpoint(
        &self,
        data: &CheckpointData,
    ) -> Result<(TemporaryCheckpointStore, Option<TemporaryEpochStore>), IndexerError> {
        let CheckpointData {
            checkpoint,
            transactions,
            changed_objects,
        } = data;

        // Index transaction
        let temp_tx_store_iter = transactions
            .iter()
            .map(|tx| TemporaryTransactionBlockResponseStore::from(tx.clone()));
        let db_transactions: Vec<Transaction> = temp_tx_store_iter
            .map(|tx| tx.try_into())
            .collect::<Result<Vec<Transaction>, _>>()?;

        // Index events
        let events = transactions
            .iter()
            .flat_map(|tx| tx.events.data.iter().map(move |event| event.clone().into()))
            .collect::<Vec<_>>();

        // Index objects
        let tx_objects = changed_objects
            .iter()
            // Unwrap safe here as we requested previous tx data in the request.
            .fold(BTreeMap::<_, Vec<_>>::new(), |mut acc, (status, o)| {
                if let Some(digest) = &o.previous_transaction {
                    acc.entry(*digest).or_default().push((status, o));
                }
                acc
            });

        let objects_changes = transactions
            .iter()
            .map(|tx| {
                let changed_objects = tx_objects
                    .get(&tx.digest)
                    .unwrap_or(&vec![])
                    .iter()
                    .map(|(status, o)| {
                        Object::from(
                            &checkpoint.epoch,
                            &checkpoint.sequence_number.into(),
                            status,
                            o,
                        )
                    })
                    .collect::<Vec<_>>();
                let deleted = tx.effects.deleted().iter();
                let deleted = deleted.map(|o| (ObjectStatus::Deleted, o));
                let wrapped = tx.effects.wrapped().iter();
                let wrapped = wrapped.map(|o| (ObjectStatus::Wrapped, o));
                let unwrapped_then_deleted = tx.effects.unwrapped_then_deleted().iter();
                let unwrapped_then_deleted =
                    unwrapped_then_deleted.map(|o| (ObjectStatus::UnwrappedThenDeleted, o));
                let all_deleted_objects = deleted
                    .chain(wrapped)
                    .chain(unwrapped_then_deleted)
                    .map(|(status, oref)| {
                        DeletedObject::from(
                            &checkpoint.epoch,
                            &checkpoint.sequence_number.into(),
                            oref,
                            &tx.digest,
                            status,
                        )
                    })
                    .collect();
                TransactionObjectChanges {
                    mutated_objects: changed_objects,
                    deleted_objects: all_deleted_objects,
                }
            })
            .collect();

        // Index packages
        let packages = Self::index_packages(transactions, changed_objects)?;

        // Store input objects, move calls and recipients separately for transaction query indexing.
        let input_objects = transactions
            .iter()
            .map(|tx| tx.get_input_objects(checkpoint.epoch))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let move_calls = transactions
            .iter()
            .flat_map(|tx| tx.get_move_calls(checkpoint.epoch, checkpoint.sequence_number.into()))
            .collect();
        let recipients = transactions
            .iter()
            .flat_map(|tx| tx.get_recipients(checkpoint.epoch, checkpoint.sequence_number.into()))
            .collect();

        // Index addresses
        let addresses = transactions
            .iter()
            .flat_map(|tx| {
                tx.get_addresses(checkpoint.epoch, <u64>::from(checkpoint.sequence_number))
            })
            .collect();

        // Index epoch
        let epoch_index = if checkpoint.epoch == 0 && <u64>::from(checkpoint.sequence_number) == 0 {
            // very first epoch
            let system_state = get_sui_system_state(data)?;
            let system_state: SuiSystemStateSummary = system_state.into_sui_system_state_summary();
            let validators = system_state
                .active_validators
                .iter()
                .map(|v| (system_state.epoch, v.clone()).into())
                .collect();

            Some(TemporaryEpochStore {
                last_epoch: None,
                new_epoch: DBEpochInfo {
                    epoch: 0,
                    first_checkpoint_id: 0,
                    epoch_start_timestamp: system_state.epoch_start_timestamp_ms as i64,
                    ..Default::default()
                },
                system_state: system_state.into(),
                validators,
            })
        } else if let Some(end_of_epoch_data) = &checkpoint.end_of_epoch_data {
            // Find system state object
            let system_state = get_sui_system_state(data)?;
            let system_state: SuiSystemStateSummary = system_state.into_sui_system_state_summary();

            let epoch_event = transactions.iter().find_map(|tx| {
                tx.events.data.iter().find(|ev| {
                    ev.type_.address == SUI_SYSTEM_ADDRESS
                        && ev.type_.module.as_ident_str() == ident_str!("sui_system_state_inner")
                        && ev.type_.name.as_ident_str() == ident_str!("SystemEpochInfoEvent")
                })
            });

            let event = epoch_event
                .map(|e| bcs::from_bytes::<SystemEpochInfoEvent>(&e.bcs))
                .transpose()?;

            let validators = system_state
                .active_validators
                .iter()
                .map(|v| (system_state.epoch, v.clone()).into())
                .collect();

            let epoch_commitments = end_of_epoch_data
                .epoch_commitments
                .iter()
                .map(|c| match c {
                    CheckpointCommitment::ECMHLiveObjectSetDigest(d) => {
                        Some(d.digest.digest.to_vec())
                    }
                })
                .collect();

            let (next_epoch_committee, next_epoch_committee_stake) =
                end_of_epoch_data.next_epoch_committee.iter().fold(
                    (vec![], vec![]),
                    |(mut names, mut stakes), (name, stake)| {
                        names.push(Some(name.as_bytes().to_vec()));
                        stakes.push(Some(*stake as i64));
                        (names, stakes)
                    },
                );

            let event = event.as_ref();

            Some(TemporaryEpochStore {
                last_epoch: Some(DBEpochInfo {
                    epoch: system_state.epoch as i64 - 1,
                    first_checkpoint_id: 0,
                    last_checkpoint_id: Some(<u64>::from(checkpoint.sequence_number) as i64),
                    epoch_start_timestamp: 0,
                    epoch_end_timestamp: Some(checkpoint.timestamp_ms as i64),
                    epoch_total_transactions: 0,
                    next_epoch_version: Some(
                        end_of_epoch_data.next_epoch_protocol_version.as_u64() as i64,
                    ),
                    next_epoch_committee,
                    next_epoch_committee_stake,
                    stake_subsidy_amount: event.map(|e| e.stake_subsidy_amount),
                    reference_gas_price: event.map(|e| e.reference_gas_price),
                    storage_fund_balance: event.map(|e| e.storage_fund_balance),
                    total_gas_fees: event.map(|e| e.total_gas_fees),
                    total_stake_rewards_distributed: event
                        .map(|e| e.total_stake_rewards_distributed),
                    total_stake: event.map(|e| e.total_stake),
                    storage_fund_reinvestment: event.map(|e| e.storage_fund_reinvestment),
                    storage_charge: event.map(|e| e.storage_charge),
                    protocol_version: event.map(|e| e.protocol_version),
                    storage_rebate: event.map(|e| e.storage_rebate),
                    leftover_storage_fund_inflow: event.map(|e| e.leftover_storage_fund_inflow),
                    epoch_commitments,
                }),
                new_epoch: DBEpochInfo {
                    epoch: system_state.epoch as i64,
                    first_checkpoint_id: <u64>::from(checkpoint.sequence_number) as i64 + 1,
                    epoch_start_timestamp: system_state.epoch_start_timestamp_ms as i64,
                    ..Default::default()
                },
                system_state: system_state.into(),
                validators,
            })
        } else {
            None
        };

        let total_transactions = db_transactions.iter().map(|t| t.transaction_count).sum();

        Ok((
            TemporaryCheckpointStore {
                checkpoint: Checkpoint::from(checkpoint, total_transactions)?,
                transactions: db_transactions,
                events,
                objects_changes,
                addresses,
                packages,
                input_objects,
                move_calls,
                recipients,
            },
            epoch_index,
        ))
    }

    fn index_packages(
        transactions: &[CheckpointTransactionBlockResponse],
        changed_objects: &[(ObjectStatus, SuiObjectData)],
    ) -> Result<Vec<Package>, IndexerError> {
        let object_map = changed_objects
            .iter()
            .filter_map(|(_, o)| {
                if let SuiRawData::Package(p) = &o
                    .bcs
                    .as_ref()
                    .expect("Expect the content field to be non-empty from data fetching")
                {
                    Some((o.object_id, p))
                } else {
                    None
                }
            })
            .collect::<BTreeMap<_, _>>();

        transactions
            .iter()
            .flat_map(|tx| {
                tx.effects.created().iter().map(|oref| {
                    object_map
                        .get(&oref.reference.object_id)
                        .map(|o| Package::try_from(*tx.transaction.data.sender(), o))
                })
            })
            .flatten()
            .collect()
    }
}
