// Copyright (c) 2021, Facebook, Inc. and its affiliates
// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use anyhow::anyhow;
use arc_swap::Guard;
use chrono::prelude::*;
use fastcrypto::traits::KeyPair;
use move_bytecode_utils::module_cache::SyncModuleCache;
use move_core_types::account_address::AccountAddress;
use move_core_types::identifier::Identifier;
use move_core_types::parser::parse_struct_tag;
use move_core_types::{language_storage::ModuleId, resolver::ModuleResolver};
use move_vm_runtime::{move_vm::MoveVM, native_functions::NativeFunctionTable};
use mysten_metrics::{monitored_scope, spawn_monitored_task};
use prometheus::{
    exponential_buckets, register_histogram_with_registry, register_int_counter_with_registry,
    register_int_gauge_with_registry, Histogram, IntCounter, IntGauge, Registry,
};
use std::cmp::Ordering as CmpOrdering;
use std::hash::Hash;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use std::{
    collections::{HashMap, VecDeque},
    pin::Pin,
    sync::{atomic::Ordering, Arc},
};
use tap::TapFallible;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc::unbounded_channel;
use tracing::{debug, error, instrument, warn, Instrument};
use typed_store::Map;

pub use authority_notify_read::EffectsNotifyRead;
pub use authority_store::{AuthorityStore, ResolverWrapper, UpdateType};
use narwhal_config::{
    Committee as ConsensusCommittee, WorkerCache as ConsensusWorkerCache,
    WorkerId as ConsensusWorkerId,
};
use narwhal_types::CommittedSubDag;
use sui_adapter::{adapter, execution_mode};
use sui_config::genesis::Genesis;
use sui_json_rpc_types::{
    type_and_fields_from_move_struct, SuiEvent, SuiEventEnvelope, SuiTransactionEffects,
};
use sui_simulator::nondeterministic;
use sui_storage::write_ahead_log::WriteAheadLog;
use sui_storage::{
    event_store::{EventStore, EventStoreType, StoredEvent},
    node_sync_store::NodeSyncStore,
    write_ahead_log::{DBTxGuard, TxGuard},
    IndexStore,
};
use sui_types::committee::EpochId;
use sui_types::crypto::{AuthorityKeyPair, NetworkKeyPair};
use sui_types::event::{Event, EventID};
use sui_types::messages_checkpoint::{CheckpointRequest, CheckpointResponse};
use sui_types::object::{Owner, PastObjectRead};
use sui_types::query::{EventQuery, TransactionQuery};
use sui_types::sui_system_state::SuiSystemState;
use sui_types::temporary_store::InnerTemporaryStore;
pub use sui_types::temporary_store::TemporaryStore;
use sui_types::{
    base_types::*,
    batch::{TxSequenceNumber, UpdateItem},
    committee::Committee,
    crypto::AuthoritySignature,
    error::{SuiError, SuiResult},
    fp_ensure,
    messages::*,
    object::{Object, ObjectFormatOptions, ObjectRead},
    storage::{BackingPackageStore, DeleteKind},
    MOVE_STDLIB_ADDRESS, SUI_FRAMEWORK_ADDRESS, SUI_SYSTEM_STATE_OBJECT_ID,
};

use crate::authority::authority_notifier::TransactionNotifierTicket;
use crate::authority::authority_notify_read::NotifyRead;
use crate::authority::authority_per_epoch_store::AuthorityPerEpochStore;
use crate::authority_aggregator::AuthorityAggregator;
use crate::checkpoints::CheckpointServiceNotify;
use crate::consensus_handler::{
    SequencedConsensusTransaction, VerifiedSequencedConsensusTransaction,
};
use crate::epoch::committee_store::CommitteeStore;
use crate::epoch::reconfiguration::ReconfigState;
use crate::execution_driver::execution_process;
use crate::module_cache_gauge::ModuleCacheGauge;
use crate::scoped_counter;
use crate::{
    authority_batch::{BroadcastReceiver, BroadcastSender},
    event_handler::EventHandler,
    execution_engine,
    query_helpers::QueryHelpers,
    transaction_input_checker,
    transaction_manager::TransactionManager,
    transaction_streamer::TransactionStreamer,
};

use sui_types::gas::GasCostSummary;

use self::authority_store::ObjectKey;

#[cfg(test)]
#[path = "unit_tests/authority_tests.rs"]
pub mod authority_tests;

#[cfg(test)]
#[path = "unit_tests/batch_transaction_tests.rs"]
mod batch_transaction_tests;

#[cfg(test)]
#[path = "unit_tests/move_integration_tests.rs"]
pub mod move_integration_tests;

#[cfg(test)]
#[path = "unit_tests/gas_tests.rs"]
mod gas_tests;

#[cfg(test)]
#[path = "unit_tests/tbls_tests.rs"]
mod tbls_tests;

pub mod authority_per_epoch_store;

pub mod authority_store_pruner;
pub mod authority_store_tables;

pub mod authority_notifier;
pub(crate) mod authority_notify_read;
pub(crate) mod authority_store;

pub const MAX_ITEMS_LIMIT: u64 = 1_000;
const BROADCAST_CAPACITY: usize = 10_000;

pub(crate) const MAX_TX_RECOVERY_RETRY: u32 = 3;
type CertTxGuard<'a> =
    DBTxGuard<'a, TrustedCertificate, (InnerTemporaryStore, SignedTransactionEffects)>;

pub type ReconfigConsensusMessage = (
    AuthorityKeyPair,
    NetworkKeyPair,
    ConsensusCommittee,
    Vec<(ConsensusWorkerId, NetworkKeyPair)>,
    ConsensusWorkerCache,
);

/// Prometheus metrics which can be displayed in Grafana, queried and alerted on
pub struct AuthorityMetrics {
    tx_orders: IntCounter,
    total_certs: IntCounter,
    total_cert_attempts: IntCounter,
    total_effects: IntCounter,
    pub shared_obj_tx: IntCounter,
    tx_already_processed: IntCounter,
    num_input_objs: Histogram,
    num_shared_objects: Histogram,
    batch_size: Histogram,

    prepare_certificate_latency: Histogram,
    commit_certificate_latency: Histogram,
    handle_transaction_latency: Histogram,
    handle_certificate_latency: Histogram,
    handle_node_sync_certificate_latency: Histogram,

    pub(crate) transaction_manager_num_missing_objects: IntGauge,
    pub(crate) transaction_manager_num_pending_certificates: IntGauge,
    pub(crate) transaction_manager_num_executing_certificates: IntGauge,
    pub(crate) transaction_manager_num_ready: IntGauge,

    pub(crate) execution_driver_executed_transactions: IntCounter,
    pub(crate) execution_driver_execution_failures: IntCounter,

    skipped_consensus_txns: IntCounter,

    pub follower_items_streamed: IntCounter,
    pub follower_items_loaded: IntCounter,
    pub follower_connections: IntCounter,
    pub follower_connections_concurrent: IntGauge,
    pub follower_txes_streamed: IntCounter,
    pub follower_batches_streamed: IntCounter,
    pub follower_start_seq_num: Histogram,

    // TODO: consolidate these into GossipMetrics
    // (issue: https://github.com/MystenLabs/sui/issues/3926)
    pub gossip_queued_count: IntCounter,
    pub gossip_sync_count: IntCounter,
    pub gossip_task_success_count: IntCounter,
    pub gossip_task_error_count: IntCounter,

    /// Post processing metrics
    post_processing_total_events_emitted: IntCounter,
    post_processing_total_tx_indexed: IntCounter,
    post_processing_total_tx_added_to_streamer: IntCounter,
    post_processing_total_tx_had_event_processed: IntCounter,
    post_processing_total_tx_sent_to_post_processing: IntCounter,
    post_processing_latest_seq_seen: IntGauge,

    pub num_post_processing_tasks: IntGauge,
    pub num_batch_service_tasks: IntGauge,

    /// Batch service metrics
    pub(crate) batch_service_total_tx_broadcasted: IntCounter,
    pub(crate) batch_service_latest_seq_broadcasted: IntGauge,
    pub(crate) batch_svc_is_running: IntCounter,

    pending_notify_read: IntGauge,

    /// Consensus handler metrics
    pub consensus_handler_processed_batches: IntCounter,
    pub consensus_handler_processed_bytes: IntCounter,
}

// Override default Prom buckets for positive numbers in 0-50k range
const POSITIVE_INT_BUCKETS: &[f64] = &[
    1., 2., 5., 10., 20., 50., 100., 200., 500., 1000., 2000., 5000., 10000., 20000., 50000.,
];

const LATENCY_SEC_BUCKETS: &[f64] = &[
    0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1., 2.5, 5., 10., 20., 30., 60., 90.,
];

impl AuthorityMetrics {
    pub fn new(registry: &prometheus::Registry) -> AuthorityMetrics {
        // buckets are: 100, 10k, 1M, 100M, 10B, 1T, 100T, 10Q
        // Safe to unwarp because the values are all valid.
        let follower_seq_num_buckets = exponential_buckets(100., 100., 8).unwrap();
        Self {
            tx_orders: register_int_counter_with_registry!(
                "total_transaction_orders",
                "Total number of transaction orders",
                registry,
            )
            .unwrap(),
            total_certs: register_int_counter_with_registry!(
                "total_transaction_certificates",
                "Total number of transaction certificates handled",
                registry,
            )
            .unwrap(),
            total_cert_attempts: register_int_counter_with_registry!(
                "total_handle_certificate_attempts",
                "Number of calls to handle_certificate",
                registry,
            )
            .unwrap(),
            // total_effects == total transactions finished
            total_effects: register_int_counter_with_registry!(
                "total_transaction_effects",
                "Total number of transaction effects produced",
                registry,
            )
            .unwrap(),

            shared_obj_tx: register_int_counter_with_registry!(
                "num_shared_obj_tx",
                "Number of transactions involving shared objects",
                registry,
            )
            .unwrap(),
            tx_already_processed: register_int_counter_with_registry!(
                "num_tx_already_processed",
                "Number of transaction orders already processed previously",
                registry,
            )
            .unwrap(),
            num_input_objs: register_histogram_with_registry!(
                "num_input_objects",
                "Distribution of number of input TX objects per TX",
                POSITIVE_INT_BUCKETS.to_vec(),
                registry,
            )
            .unwrap(),
            num_shared_objects: register_histogram_with_registry!(
                "num_shared_objects",
                "Number of shared input objects per TX",
                POSITIVE_INT_BUCKETS.to_vec(),
                registry,
            )
            .unwrap(),
            batch_size: register_histogram_with_registry!(
                "batch_size",
                "Distribution of size of transaction batch",
                POSITIVE_INT_BUCKETS.to_vec(),
                registry,
            )
            .unwrap(),
            prepare_certificate_latency: register_histogram_with_registry!(
                "validator_prepare_certificate_latency",
                "Latency of preparing certificate",
                LATENCY_SEC_BUCKETS.to_vec(),
                registry,
            )
            .unwrap(),
            commit_certificate_latency: register_histogram_with_registry!(
                "validator_commit_certificate_latency",
                "Latency of committing certificate",
                LATENCY_SEC_BUCKETS.to_vec(),
                registry,
            )
            .unwrap(),
            handle_transaction_latency: register_histogram_with_registry!(
                "validator_handle_transaction_latency",
                "Latency of committing certificate",
                LATENCY_SEC_BUCKETS.to_vec(),
                registry,
            )
            .unwrap(),
            handle_certificate_latency: register_histogram_with_registry!(
                "validator_handle_certificate_latency",
                "Latency of handling certificate",
                LATENCY_SEC_BUCKETS.to_vec(),
                registry,
            )
            .unwrap(),
            handle_node_sync_certificate_latency: register_histogram_with_registry!(
                "fullnode_handle_node_sync_certificate_latency",
                "Latency of fullnode handling certificate from node sync",
                LATENCY_SEC_BUCKETS.to_vec(),
                registry,
            )
            .unwrap(),
            transaction_manager_num_missing_objects: register_int_gauge_with_registry!(
                "transaction_manager_num_missing_objects",
                "Current number of missing objects in TransactionManager",
                registry,
            )
            .unwrap(),
            transaction_manager_num_pending_certificates: register_int_gauge_with_registry!(
                "transaction_manager_num_pending_certificates",
                "Number of certificates pending in TransactionManager, with at least 1 missing input object",
                registry,
            )
            .unwrap(),
            transaction_manager_num_executing_certificates: register_int_gauge_with_registry!(
                "transaction_manager_num_executing_certificates",
                "Nnumber of executing certificates, including queued and actually running certificates",
                registry,
            )
            .unwrap(),
            transaction_manager_num_ready: register_int_gauge_with_registry!(
                "transaction_manager_num_ready",
                "Number of ready transactions in TransactionManager",
                registry,
            )
            .unwrap(),
            execution_driver_executed_transactions: register_int_counter_with_registry!(
                "execution_driver_executed_transactions",
                "Cumulative number of transaction executed by execution driver",
                registry,
            )
            .unwrap(),
            execution_driver_execution_failures: register_int_counter_with_registry!(
                "execution_driver_execution_failures",
                "Cumulative number of transactions failed to be executed by execution driver",
                registry,
            )
            .unwrap(),
            skipped_consensus_txns: register_int_counter_with_registry!(
                "skipped_consensus_txns",
                "Total number of consensus transactions skipped",
                registry,
            )
            .unwrap(),
            follower_items_streamed: register_int_counter_with_registry!(
                "follower_items_streamed",
                "Number of transactions/signed batches streamed to followers",
                registry,
            )
            .unwrap(),
            follower_items_loaded: register_int_counter_with_registry!(
                "follower_items_loaded",
                "Number of transactions/signed batches loaded from db to be streamed to followers",
                registry,
            )
            .unwrap(),
            follower_connections: register_int_counter_with_registry!(
                "follower_connections",
                "Number of follower connections initiated",
                registry,
            )
            .unwrap(),
            follower_connections_concurrent: register_int_gauge_with_registry!(
                "follower_connections_concurrent",
                "Current number of concurrent follower connections",
                registry,
            )
            .unwrap(),
            follower_batches_streamed: register_int_counter_with_registry!(
                "follower_batches_streamed",
                "Number of signed batches streamed to followers",
                registry,
            )
            .unwrap(),
            follower_txes_streamed: register_int_counter_with_registry!(
                "follower_txes_streamed",
                "Number of transactions streamed to followers",
                registry,
            )
            .unwrap(),
            follower_start_seq_num: register_histogram_with_registry!(
                "follower_start_seq_num",
                "The start seq number this validator receives from fullnodes node_sync/follower process",
                follower_seq_num_buckets,
                registry,
            )
            .unwrap(),
            gossip_queued_count: register_int_counter_with_registry!(
                "gossip_queued_count",
                "Number of digests queued from gossip peers",
                registry,
            )
            .unwrap(),
            gossip_sync_count: register_int_counter_with_registry!(
                "gossip_sync_count",
                "Number of certificates downloaded from gossip peers",
                registry,
            )
            .unwrap(),
            gossip_task_success_count: register_int_counter_with_registry!(
                "gossip_task_success_count",
                "Number of gossip tasks that completed successfully",
                registry,
            )
            .unwrap(),
            gossip_task_error_count: register_int_counter_with_registry!(
                "gossip_task_error_count",
                "Number of gossip tasks that completed with errors",
                registry,
            )
            .unwrap(),
            post_processing_total_events_emitted: register_int_counter_with_registry!(
                "post_processing_total_events_emitted",
                "Total number of events emitted in post processing",
                registry,
            )
            .unwrap(),
            post_processing_total_tx_indexed: register_int_counter_with_registry!(
                "post_processing_total_tx_indexed",
                "Total number of txes indexed in post processing",
                registry,
            )
            .unwrap(),
            post_processing_total_tx_added_to_streamer: register_int_counter_with_registry!(
                "post_processing_total_tx_added_to_streamer",
                "Total number of txes added to tx streamer in post processing",
                registry,
            )
            .unwrap(),
            post_processing_total_tx_had_event_processed: register_int_counter_with_registry!(
                "post_processing_total_tx_had_event_processed",
                "Total number of txes finished event processing in post processing",
                registry,
            )
            .unwrap(),
            post_processing_total_tx_sent_to_post_processing: register_int_counter_with_registry!(
                "post_processing_total_tx_sent_to_post_processing",
                "Total number of txes sent to post processing",
                registry,
            )
            .unwrap(),
            post_processing_latest_seq_seen: register_int_gauge_with_registry!(
                "post_processing_latest_seq_seen",
                "The latest seq number of tx that is seen in post processing",
                registry,
            )
            .unwrap(),
            num_post_processing_tasks: register_int_gauge_with_registry!(
                "num_post_processing_tasks",
                "Number of post processing tasks currently running.",
                registry,
            )
            .unwrap(),
            num_batch_service_tasks: register_int_gauge_with_registry!(
                "num_batch_service_tasks",
                "Number of batch service tasks currently running.",
                registry,
            )
            .unwrap(),
            batch_service_total_tx_broadcasted: register_int_counter_with_registry!(
                "batch_service_total_tx_broadcasted",
                "Total number of txes broadcasted in batch service",
                registry,
            )
            .unwrap(),
            batch_service_latest_seq_broadcasted: register_int_gauge_with_registry!(
                "batch_service_latest_seq_broadcasted",
                "The latest seq number of tx that is broadcasted in batch service",
                registry,
            )
            .unwrap(),
            batch_svc_is_running: register_int_counter_with_registry!(
                "batch_svc_is_running",
                "Sanity check to ensure batch service is running",
                registry,
            ).unwrap(),
            pending_notify_read: register_int_gauge_with_registry!(
                "pending_notify_read",
                "Pending notify read requests",
                registry,
            )
                .unwrap(),
            consensus_handler_processed_batches: register_int_counter_with_registry!(
                "consensus_handler_processed_batches",
                "Number of batches processed by consensus_handler",
                registry
            ).unwrap(),
            consensus_handler_processed_bytes: register_int_counter_with_registry!(
                "consensus_handler_processed_bytes",
                "Number of bytes processed by consensus_handler",
                registry
            ).unwrap(),
        }
    }
}

/// a Trait object for `signature::Signer` that is:
/// - Pin, i.e. confined to one place in memory (we don't want to copy private keys).
/// - Sync, i.e. can be safely shared between threads.
///
/// Typically instantiated with Box::pin(keypair) where keypair is a `KeyPair`
///
pub type StableSyncAuthoritySigner =
    Pin<Arc<dyn signature::Signer<AuthoritySignature> + Send + Sync>>;

pub struct AuthorityState {
    // Fixed size, static, identity of the authority
    /// The name of this authority.
    pub name: AuthorityName,
    /// The signature key of the authority.
    pub secret: StableSyncAuthoritySigner,

    /// Move native functions that are available to invoke
    pub(crate) _native_functions: NativeFunctionTable,
    pub(crate) move_vm: Arc<MoveVM>,

    /// The database
    pub(crate) database: Arc<AuthorityStore>, // TODO: remove pub

    pub node_sync_store: Arc<NodeSyncStore>,

    indexes: Option<Arc<IndexStore>>,

    pub module_cache: Arc<SyncModuleCache<ResolverWrapper<AuthorityStore>>>, // TODO: use strategies (e.g. LRU?) to constraint memory usage

    pub event_handler: Option<Arc<EventHandler>>,
    pub transaction_streamer: Option<Arc<TransactionStreamer>>,

    committee_store: Arc<CommitteeStore>,

    /// Manages pending certificates and their missing input objects.
    pub(crate) transaction_manager: Arc<TransactionManager>,

    // Structures needed for handling batching and notifications.
    /// The sender to notify of new transactions
    /// and create batches for this authority.
    /// Keep as None if there is no need for this.
    pub(crate) batch_channels: BroadcastSender, // TODO: remove pub

    // The Transaction notifier ticketing engine.
    pub(crate) batch_notifier: Arc<authority_notifier::TransactionNotifier>, // TODO: remove pub

    pub metrics: Arc<AuthorityMetrics>,
}

/// The authority state encapsulates all state, drives execution, and ensures safety.
///
/// Note the authority operations can be accessed through a read ref (&) and do not
/// require &mut. Internally a database is synchronized through a mutex lock.
///
/// Repeating valid commands should produce no changes and return no error.
impl AuthorityState {
    pub fn is_validator(&self) -> bool {
        self.epoch_store().committee().authority_exists(&self.name)
    }

    pub fn is_fullnode(&self) -> bool {
        !self.is_validator()
    }

    /// Get a broadcast receiver for updates
    pub fn subscribe_batch(&self) -> BroadcastReceiver {
        self.batch_channels.subscribe()
    }

    pub fn epoch(&self) -> EpochId {
        self.database.epoch_store().epoch()
    }

    pub fn committee_store(&self) -> &Arc<CommitteeStore> {
        &self.committee_store
    }

    /// This is a private method and should be kept that way. It doesn't check whether
    /// the provided transaction is a system transaction, and hence can only be called internally.
    async fn handle_transaction_impl(
        &self,
        transaction: VerifiedTransaction,
    ) -> Result<VerifiedTransactionInfoResponse, SuiError> {
        let transaction_digest = *transaction.digest();

        let (_gas_status, input_objects) = transaction_input_checker::check_transaction_input(
            &self.database,
            &transaction.data().intent_message.value,
        )
        .await?;

        let owned_objects = input_objects.filter_owned_objects();

        let signed_transaction =
            VerifiedSignedTransaction::new(self.epoch(), transaction, self.name, &*self.secret);

        // Check and write locks, to signed transaction, into the database
        // The call to self.set_transaction_lock checks the lock is not conflicting,
        // and returns ConflictingTransaction error in case there is a lock on a different
        // existing transaction.
        self.set_transaction_lock(&owned_objects, signed_transaction)
            .await?;

        // Return the signed Transaction or maybe a cert.
        self.make_transaction_info(&transaction_digest).await
    }

    /// Initiate a new transaction.
    pub async fn handle_transaction(
        &self,
        transaction: VerifiedTransaction,
    ) -> Result<VerifiedTransactionInfoResponse, SuiError> {
        let transaction_digest = *transaction.digest();
        debug!(tx_digest=?transaction_digest, "handle_transaction. Tx data: {:?}", &transaction.data().intent_message.value);

        // Ensure an idempotent answer. This is checked before the system_tx check so that
        // a validator is able to return the signed system tx if it was already signed locally.
        if self
            .database
            .transaction_exists(self.epoch(), &transaction_digest)?
        {
            self.metrics.tx_already_processed.inc();
            return self.make_transaction_info(&transaction_digest).await;
        }

        // CRITICAL! Validators should never sign an external system transaction.
        fp_ensure!(
            !transaction.is_system_tx(),
            SuiError::InvalidSystemTransaction
        );

        let _metrics_guard = self.metrics.handle_transaction_latency.start_timer();

        self.metrics.tx_orders.inc();

        let response = self.handle_transaction_impl(transaction).await;
        match response {
            Ok(r) => Ok(r),
            // If we see an error, it is possible that a certificate has already been processed.
            // In that case, we could still return Ok to avoid showing confusing errors.
            Err(err) => self
                .get_tx_info_already_executed(&transaction_digest)
                .await?
                .ok_or(err),
        }
    }

    /// Executes a certificate that's known to have correct effects.
    /// For such certificate, we don't have to wait for consensus to set shared object
    /// locks because we already know the shared object versions based on the effects.
    /// This function can be called either by a fullnode after seeing a quorum of signed effects,
    /// or by a validator after seeing the certificate included by a certified checkpoint.
    /// TODO: down the road, we may want to execute a shared object tx on a validator when f+1
    /// validators have executed it.
    #[instrument(level = "trace", skip_all)]
    pub async fn execute_certificate_with_effects<S>(
        &self,
        certificate: &VerifiedCertificate,
        // NOTE: the caller of this must promise to wait until it
        // knows for sure this tx is finalized, namely, it has seen a
        // CertifiedTransactionEffects or at least f+1 identifical effects
        // digests matching this TransactionEffectsEnvelope, before calling
        // this function, in order to prevent a byzantine validator from
        // giving us incorrect effects.
        // TODO: allow CertifiedTransactionEffects only
        effects: &TransactionEffectsEnvelope<S>,
    ) -> SuiResult {
        let _metrics_guard = self
            .metrics
            .handle_node_sync_certificate_latency
            .start_timer();
        let digest = *certificate.digest();
        debug!(tx_digest = ?digest, "execute_certificate_with_effects");
        fp_ensure!(
            effects.data().transaction_digest == digest,
            SuiError::ErrorWhileProcessingCertificate {
                err: "effects/tx digest mismatch".to_string()
            }
        );

        if certificate.contains_shared_object() {
            self.database
                .acquire_shared_locks_from_effects(certificate, effects.data())
                .await?;
        }

        let expected_effects_digest = effects.digest();

        let certs = vec![certificate.clone()];
        self.database
            .epoch_store()
            .insert_pending_certificates(&certs)?;
        self.transaction_manager.enqueue(certs).await?;

        let observed_effects = self
            .database
            .notify_read_effects(vec![digest])
            .await?
            .pop()
            .expect("notify_read_effects should return exactly 1 element");

        let observed_effects_digest = observed_effects.digest();
        if observed_effects_digest != expected_effects_digest {
            error!(
                ?expected_effects_digest,
                ?observed_effects_digest,
                expected_effects=?effects.data(),
                observed_effects=?observed_effects.data(),
                input_objects = ?certificate.data().intent_message.value.input_objects(),
                "Locally executed effects do not match canonical effects!");
        }
        Ok(())
    }

    /// Executes a certificate for its effects.
    #[instrument(level = "trace", skip_all)]
    pub async fn execute_certificate(
        &self,
        certificate: &VerifiedCertificate,
    ) -> SuiResult<VerifiedTransactionInfoResponse> {
        let tx_digest = *certificate.digest();
        debug!(?tx_digest, "execute_certificate");

        self.metrics.total_cert_attempts.inc();

        if self.is_fullnode() {
            return Err(SuiError::GenericStorageError(
                "cannot execute cert without effects on fullnode".into(),
            ));
        }

        if !certificate.is_system_tx() && self.is_cert_awaiting_sequencing(certificate)? {
            debug!("shared object cert has not been sequenced by narwhal");
            return Err(SuiError::SharedObjectLockNotSetError);
        }

        let certs = vec![certificate.clone()];
        self.database
            .epoch_store()
            .insert_pending_certificates(&certs)?;
        self.transaction_manager.enqueue(certs).await?;

        self.notify_read_transaction_info(certificate).await
    }

    /// Internal logic to execute a certificate.
    ///
    /// Guarantees that
    /// - If input objects are available, it should return no permanent failure.
    /// - Execution and persisting output are atomic. i.e. outputs are only written to storage,
    /// on successful execution; crashed execution has no observable effect and can be retried.
    ///
    /// Since this function does not set locks or guarantee input objects are available, it should
    /// only be called by the execution driver, for systems transactions, or in tests.
    ///
    /// TODO: Reduce callsites and restrict visibility to pub(crate).
    #[instrument(level = "trace", skip_all)]
    pub async fn execute_certificate_internal(
        &self,
        certificate: &VerifiedCertificate,
    ) -> SuiResult<VerifiedTransactionInfoResponse> {
        let tx_digest = *certificate.digest();
        debug!(?tx_digest, "handle_certificate");

        let _metrics_guard = self.metrics.handle_certificate_latency.start_timer();

        // This acquires a lock on the tx digest to prevent multiple concurrent executions of the
        // same tx. While we don't need this for safety (tx sequencing is ultimately atomic), it is
        // very common to receive the same tx multiple times simultaneously due to gossip, so we
        // may as well hold the lock and save the cpu time for other requests.
        //
        // Note that this lock has some false contention (since it uses a MutexTable), so you can't
        // assume that different txes can execute concurrently. This is probably the fastest way
        // to do this, since the false contention can be made arbitrarily low (no cost for 1.0 -
        // epsilon of txes) while solutions without false contention have slightly higher cost
        // for every tx.
        let span = tracing::debug_span!(
            "handle_certificate_tx_guard",
            ?tx_digest,
            tx_kind = certificate.data().intent_message.value.kind_as_str()
        );
        let epoch_store = self.epoch_store();
        let tx_guard = epoch_store
            .acquire_tx_guard(certificate)
            .instrument(span)
            .await?;

        self.process_certificate(tx_guard, certificate)
            .await
            .tap_err(|e| debug!(?tx_digest, "process_certificate failed: {e}"))
    }

    pub async fn notify_read_transaction_info(
        &self,
        certificate: &VerifiedCertificate,
    ) -> SuiResult<VerifiedTransactionInfoResponse> {
        let tx_digest = *certificate.digest();
        let effects = self
            .database
            .notify_read_effects(vec![tx_digest])
            .await?
            .pop()
            .expect("notify_read_effects should return exactly 1 element");
        Ok(VerifiedTransactionInfoResponse {
            signed_transaction: self.database.get_transaction(&tx_digest)?,
            certified_transaction: Some(certificate.clone()),
            signed_effects: Some(effects),
        })
    }

    #[instrument(level = "trace", skip_all)]
    async fn check_owned_locks(&self, owned_object_refs: &[ObjectRef]) -> SuiResult {
        self.database.check_owned_locks(owned_object_refs).await
    }

    #[instrument(level = "trace", skip_all)]
    async fn check_shared_locks(
        &self,
        transaction_digest: &TransactionDigest,
        // inputs: &[(InputObjectKind, Object)],
        shared_object_refs: &[ObjectRef],
    ) -> Result<(), SuiError> {
        debug!("Validating shared object sequence numbers from consensus...");

        // Internal consistency check
        debug_assert!(
            !shared_object_refs.is_empty(),
            "we just checked that there are share objects yet none found?"
        );

        let shared_locks: HashMap<_, _> = self
            .epoch_store()
            .get_shared_locks(transaction_digest)?
            .into_iter()
            .collect();

        // Check whether the shared objects have already been assigned a sequence number by
        // the consensus. Bail if the transaction contains even one shared object that either:
        // (i) was not assigned a sequence number, or
        // (ii) has a different sequence number than the current one.

        let lock_errors: Vec<_> = shared_object_refs
            .iter()
            .filter_map(|(object_id, version, _)| {
                if !shared_locks.contains_key(object_id) {
                    Some(SuiError::SharedObjectLockNotSetError)
                } else if shared_locks[object_id] != *version {
                    Some(SuiError::UnexpectedSequenceNumber {
                        object_id: *object_id,
                        // This sequence number is the one attributed by consensus.
                        expected_sequence: shared_locks[object_id],
                        // This sequence number is the one we currently have in the database.
                        given_sequence: *version,
                    })
                } else {
                    None
                }
            })
            .collect();

        fp_ensure!(
            lock_errors.is_empty(),
            // NOTE: the error message here will say 'Error acquiring lock' but what it means is
            // 'error checking lock'.
            SuiError::TransactionInputObjectsErrors {
                errors: lock_errors
            }
        );

        Ok(())
    }

    #[instrument(level = "trace", skip_all)]
    pub(crate) async fn process_certificate(
        &self,
        tx_guard: CertTxGuard<'_>,
        certificate: &VerifiedCertificate,
    ) -> SuiResult<VerifiedTransactionInfoResponse> {
        // Any caller that verifies the signatures on the certificate will have already checked the
        // epoch. But paths that don't verify sigs (e.g. execution from checkpoint, reading from db)
        // present the possibility of an epoch mismatch.
        if certificate.epoch() != self.epoch() {
            tx_guard.release();
            return Err(SuiError::WrongEpoch {
                expected_epoch: self.epoch(),
                actual_epoch: certificate.epoch(),
            });
        }

        // first check to see if we have already executed and committed the tx
        // to the WAL
        let epoch_store = self.epoch_store();
        if let Some((inner_temporary_storage, signed_effects)) = epoch_store
            .wal()
            .get_execution_output(certificate.digest())?
        {
            return self
                .commit_cert_and_notify(
                    certificate,
                    inner_temporary_storage,
                    signed_effects,
                    tx_guard,
                )
                .await;
        }

        let digest = *certificate.digest();
        // The cert could have been processed by a concurrent attempt of the same cert, so check if
        // the effects have already been written.
        if let Some(info) = self.get_tx_info_already_executed(&digest).await? {
            tx_guard.release();
            return Ok(info);
        }

        // Errors originating from prepare_certificate may be transient (failure to read locks) or
        // non-transient (transaction input is invalid, move vm errors). However, all errors from
        // this function occur before we have written anything to the db, so we commit the tx
        // guard and rely on the client to retry the tx (if it was transient).
        let (inner_temporary_store, signed_effects) =
            match self.prepare_certificate(certificate).await {
                Err(e) => {
                    debug!(name = ?self.name, ?digest, "Error preparing transaction: {e}");
                    tx_guard.release();
                    return Err(e);
                }
                Ok(res) => res,
            };

        // Write tx output to WAL as first commit phase. In second phase
        // we write from WAL to permanent storage. The purpose of this scheme
        // is to allow for retrying phase 2 from phase 1 in the case where we
        // fail mid-write. We prefer this over making the write to permanent
        // storage atomic as this allows for sharding storage across nodes, which
        // would be more difficult in the alternative.
        epoch_store.wal().write_execution_output(
            &digest,
            (inner_temporary_store.clone(), signed_effects.clone()),
        )?;

        self.commit_cert_and_notify(certificate, inner_temporary_store, signed_effects, tx_guard)
            .await
    }

    async fn commit_cert_and_notify(
        &self,
        certificate: &VerifiedCertificate,
        inner_temporary_store: InnerTemporaryStore,
        signed_effects: SignedTransactionEffects,
        tx_guard: CertTxGuard<'_>,
    ) -> SuiResult<VerifiedTransactionInfoResponse> {
        let digest = *certificate.digest();
        let input_object_count = inner_temporary_store.objects.len();
        let shared_object_count = signed_effects.data().shared_objects.len();

        // If commit_certificate returns an error, tx_guard will be dropped and the certificate
        // will be persisted in the log for later recovery.
        let notifier_ticket = self.batch_notifier.ticket()?;
        let ticket_seq = notifier_ticket.seq();
        let output_keys: Vec<_> = inner_temporary_store
            .written
            .iter()
            .map(|(_, ((id, seq, _), _, _))| ObjectKey(*id, *seq))
            .collect();
        let res = self
            .commit_certificate(
                inner_temporary_store,
                certificate,
                &signed_effects,
                notifier_ticket,
            )
            .await;
        let seq = match res {
            Err(err) => {
                error!(?digest, seq=?ticket_seq, "commit_certificate failed: {}", err);
                // Check if we were able to sequence the tx at all
                match self.db().get_tx_sequence(*certificate.digest()).await {
                    Err(db_err) => {
                        // TODO: Add retries on failing to read from db because
                        // this still stalls the batch maker
                        error!(
                            ?digest,
                            seq=?ticket_seq,
                            "validator failed to read if db has locked the tx sequence: {}", db_err
                        );
                    }
                    Ok(None) => {
                        debug!(?digest, seq=?ticket_seq, "Closing the notifier ticket because we couldn't lock the tx sequence");
                        self.batch_notifier.notify(ticket_seq);
                    }
                    Ok(Some(tx_seq)) => {
                        if tx_seq < ticket_seq {
                            debug!(
                                ?digest,
                                ?tx_seq,
                                ?ticket_seq,
                                "Notifying during retry failure, current low watermark {:?}",
                                self.batch_notifier.low_watermark()
                            );
                            // Notify if we failed during a retry after sequencing
                            self.batch_notifier.notify(ticket_seq);
                        };
                    }
                }
                return Err(err);
            }
            Ok(seq) => {
                if seq < ticket_seq {
                    debug!(
                        ?digest,
                        ?seq,
                        ?ticket_seq,
                        "Notifying during retry, current low watermark {:?}",
                        self.batch_notifier.low_watermark()
                    );
                    self.batch_notifier.notify(seq);
                };
                seq
            }
        };

        // Notifies transaction manager about available input objects. This allows the transaction
        // manager to schedule ready transactions.
        //
        // REQUIRED: this must be called after commit_certificate() (above), to ensure
        // TransactionManager can receive the notifications for objects that it did not find
        // in the objects table.
        //
        // REQUIRED: this must be called before tx_guard.commit_tx() (below), to ensure
        // TransactionManager can get the notifications after the node crashes and restarts.
        self.transaction_manager
            .objects_committed(output_keys)
            .await;

        // commit_certificate finished, the tx is fully committed to the store.
        tx_guard.commit_tx();

        // index certificate
        let _ = self
            .post_process_one_tx(seq, &digest)
            .await
            .tap_err(|e| error!(tx_digest = ?digest, "tx post processing failed: {e}"));

        // Update metrics.
        self.metrics.total_effects.inc();
        self.metrics.total_certs.inc();

        if shared_object_count > 0 {
            self.metrics.shared_obj_tx.inc();
        }

        self.metrics
            .num_input_objs
            .observe(input_object_count as f64);
        self.metrics
            .num_shared_objects
            .observe(shared_object_count as f64);
        self.metrics
            .batch_size
            .observe(certificate.data().intent_message.value.kind.batch_size() as f64);

        Ok(VerifiedTransactionInfoResponse {
            signed_transaction: self.database.get_transaction(&digest)?,
            certified_transaction: Some(certificate.clone()),
            signed_effects: Some(signed_effects),
        })
    }

    /// prepare_certificate validates the transaction input, and executes the certificate,
    /// returning effects, output objects, events, etc.
    ///
    /// It reads state from the db (both owned and shared locks), but it has no side effects.
    ///
    /// It can be generally understood that a failure of prepare_certificate indicates a
    /// non-transient error, e.g. the transaction input is somehow invalid, the correct
    /// locks are not held, etc. However, this is not entirely true, as a transient db read error
    /// may also cause this function to fail.
    #[instrument(level = "trace", skip_all)]
    async fn prepare_certificate(
        &self,
        certificate: &VerifiedCertificate,
    ) -> SuiResult<(InnerTemporaryStore, SignedTransactionEffects)> {
        let _metrics_guard = self.metrics.prepare_certificate_latency.start_timer();
        let (gas_status, input_objects) =
            transaction_input_checker::check_certificate_input(&self.database, certificate).await?;

        let owned_object_refs = input_objects.filter_owned_objects();
        self.check_owned_locks(&owned_object_refs).await?;

        // At this point we need to check if any shared objects need locks,
        // and whether they have them.
        let shared_object_refs = input_objects.filter_shared_objects();
        if !shared_object_refs.is_empty()
            && !certificate
                .data()
                .intent_message
                .value
                .kind
                .is_change_epoch_tx()
        {
            // If the transaction contains shared objects, we need to ensure they have been scheduled
            // for processing by the consensus protocol.
            // There is no need to go through consensus for system transactions that can
            // only be executed at a time when consensus is turned off.
            // TODO: Add some assert here to make sure consensus is indeed off with
            // is_change_epoch_tx.
            self.check_shared_locks(certificate.digest(), &shared_object_refs)
                .await?;
        }

        debug!(
            num_inputs = input_objects.len(),
            "Read inputs for transaction from DB"
        );

        let transaction_dependencies = input_objects.transaction_dependencies();
        let temporary_store =
            TemporaryStore::new(self.database.clone(), input_objects, *certificate.digest());
        let (inner_temp_store, effects, _execution_error) =
            execution_engine::execute_transaction_to_effects::<execution_mode::Normal, _>(
                shared_object_refs,
                temporary_store,
                certificate.data().intent_message.value.clone(),
                *certificate.digest(),
                transaction_dependencies,
                &self.move_vm,
                &self._native_functions,
                gas_status,
                self.epoch(),
            );

        // TODO: Distribute gas charge and rebate, which can be retrieved from effects.
        let signed_effects =
            SignedTransactionEffects::new(self.epoch(), effects, &*self.secret, self.name);
        Ok((inner_temp_store, signed_effects))
    }

    /// Notifies TransactionManager about an executed certificate.
    pub async fn certificate_executed(&self, digest: &TransactionDigest) {
        self.transaction_manager.certificate_executed(digest).await
    }

    pub async fn dry_exec_transaction(
        &self,
        transaction: TransactionData,
        transaction_digest: TransactionDigest,
    ) -> Result<SuiTransactionEffects, anyhow::Error> {
        let (gas_status, input_objects) =
            transaction_input_checker::check_transaction_input(&self.database, &transaction)
                .await?;
        let shared_object_refs = input_objects.filter_shared_objects();

        let transaction_dependencies = input_objects.transaction_dependencies();
        let temporary_store =
            TemporaryStore::new(self.database.clone(), input_objects, transaction_digest);
        let (_inner_temp_store, effects, _execution_error) =
            execution_engine::execute_transaction_to_effects::<execution_mode::Normal, _>(
                shared_object_refs,
                temporary_store,
                transaction,
                transaction_digest,
                transaction_dependencies,
                &self.move_vm,
                &self._native_functions,
                gas_status,
                self.epoch(),
            );
        SuiTransactionEffects::try_from(effects, self.module_cache.as_ref())
    }

    pub fn is_tx_already_executed(&self, digest: &TransactionDigest) -> SuiResult<bool> {
        self.database.effects_exists(digest)
    }

    pub async fn get_tx_info_already_executed(
        &self,
        digest: &TransactionDigest,
    ) -> SuiResult<Option<VerifiedTransactionInfoResponse>> {
        if self.database.effects_exists(digest)? {
            debug!("Transaction {digest:?} already executed");
            Ok(Some(self.make_transaction_info(digest).await?))
        } else {
            Ok(None)
        }
    }

    #[instrument(level = "debug", skip_all, fields(seq = ?seq, tx_digest =? digest), err)]
    fn index_tx(
        &self,
        indexes: &IndexStore,
        seq: TxSequenceNumber,
        digest: &TransactionDigest,
        cert: &VerifiedCertificate,
        effects: &SignedTransactionEffects,
        timestamp_ms: u64,
    ) -> SuiResult {
        indexes.index_tx(
            cert.sender_address(),
            cert.data()
                .intent_message
                .value
                .input_objects()?
                .iter()
                .map(|o| o.object_id()),
            effects
                .data()
                .all_mutated()
                .map(|(obj_ref, owner, _kind)| (*obj_ref, *owner)),
            cert.data()
                .intent_message
                .value
                .move_calls()
                .iter()
                .map(|mc| (mc.package.0, mc.module.clone(), mc.function.clone())),
            seq,
            digest,
            timestamp_ms,
        )
    }

    #[instrument(level = "debug", skip_all, fields(seq=?seq, tx_digest=?digest), err)]
    async fn post_process_one_tx(
        &self,
        seq: TxSequenceNumber,
        digest: &TransactionDigest,
    ) -> SuiResult {
        if self.indexes.is_none()
            && self.transaction_streamer.is_none()
            && self.event_handler.is_none()
        {
            return Ok(());
        }

        // Load cert and effects.
        let info = self.make_transaction_info(digest).await?;
        let (cert, effects) = match info {
            VerifiedTransactionInfoResponse {
                certified_transaction: Some(cert),
                signed_effects: Some(effects),
                ..
            } => (cert, effects),
            _ => {
                return Err(SuiError::CertificateNotfound {
                    certificate_digest: *digest,
                })
            }
        };

        let timestamp_ms = Self::unixtime_now_ms();

        // Index tx
        if let Some(indexes) = &self.indexes {
            let _ = self
                .index_tx(indexes.as_ref(), seq, digest, &cert, &effects, timestamp_ms)
                .tap_ok(|_| self.metrics.post_processing_total_tx_indexed.inc())
                .tap_err(|e| warn!(tx_digest=?digest, "Post processing - Couldn't index tx: {e}"));
        }

        // Stream transaction
        if let Some(transaction_streamer) = &self.transaction_streamer {
            transaction_streamer
                .enqueue((cert.into(), effects.clone()))
                .await;
            self.metrics
                .post_processing_total_tx_added_to_streamer
                .inc();
        }

        // Emit events
        if let Some(event_handler) = &self.event_handler {
            event_handler
                .process_events(effects.data(), timestamp_ms, seq)
                .await
                .tap_ok(|_| self.metrics.post_processing_total_tx_had_event_processed.inc())
                .tap_err(|e| warn!(tx_digest=?digest, "Post processing - Couldn't process events for tx: {}", e))?;

            self.metrics
                .post_processing_total_events_emitted
                .inc_by(effects.data().events.len() as u64);
        }

        Ok(())
    }

    // TODO: This should persist the last successfully-processed sequence to disk, and upon
    // starting up, look for any sequences in the store since then and process them.
    #[instrument(level = "debug", skip_all)]
    pub async fn run_tx_post_processing_process(&self) -> SuiResult {
        let mut subscriber = self.subscribe_batch();
        let _guard = scoped_counter!(self.metrics, num_post_processing_tasks);
        debug!("subscribed to batch service");

        loop {
            match subscriber.recv().await {
                Ok(item) => match item {
                    UpdateItem::Batch(batch) => {
                        debug!(
                            batch_seq = ?batch.data().next_sequence_number,
                            "post process received batch"
                        );
                    }
                    UpdateItem::Transaction((seq, ExecutionDigests { .. })) => {
                        self.metrics.post_processing_latest_seq_seen.set(seq as i64);
                        self.metrics
                            .post_processing_total_tx_sent_to_post_processing
                            .inc();
                        /*
                         * TODO: we are temporarily not processing txes here because the batch
                         * system is flaky somehow. The metrics above are left alone so that we can
                         * continue debugging.
                        if let Err(e) = self.post_process_one_tx(seq, &digest).await {
                            warn!(?digest, "Couldn't process tx: {e}");
                        }
                        */
                    }
                },
                Err(RecvError::Closed) => {
                    // This shall not happen because the sender of batch notifier should not be closed.
                    error!("run_tx_post_processing_process receiver channel closed. If this happens there is a bug");
                    break;
                }
                // Today if post processing is too slow we will skip indexing some txes.
                // TODO: https://github.com/MystenLabs/sui/issues/5025
                // Automatically restart the task, which in combination with the todo above,
                // will process any skipped txes and then begin listening for new ones.
                Err(RecvError::Lagged(number_skipped)) => {
                    error!(
                        "run_tx_post_processing_process too slow, skipped {} txes",
                        number_skipped
                    );
                }
            }
        }

        Ok(())
    }

    pub fn unixtime_now_ms() -> u64 {
        let ts_ms = Utc::now().timestamp_millis();
        u64::try_from(ts_ms).expect("Travelling in time machine")
    }

    pub async fn handle_transaction_info_request(
        &self,
        request: TransactionInfoRequest,
    ) -> Result<VerifiedTransactionInfoResponse, SuiError> {
        self.make_transaction_info(&request.transaction_digest)
            .await
    }

    pub async fn handle_account_info_request(
        &self,
        request: AccountInfoRequest,
    ) -> Result<AccountInfoResponse, SuiError> {
        self.make_account_info(request.account)
    }

    pub async fn handle_object_info_request(
        &self,
        request: ObjectInfoRequest,
    ) -> Result<VerifiedObjectInfoResponse, SuiError> {
        let ref_and_digest = match request.request_kind {
            ObjectInfoRequestKind::PastObjectInfo(seq)
            | ObjectInfoRequestKind::PastObjectInfoDebug(seq, _) => {
                // Get the Transaction Digest that created the object
                self.get_parent_iterator(request.object_id, Some(seq))
                    .await?
                    .next()
            }
            ObjectInfoRequestKind::LatestObjectInfo(_) => {
                // Or get the latest object_reference and transaction entry.
                self.get_latest_parent_entry(request.object_id).await?
            }
        };

        let (requested_object_reference, parent_certificate) = match ref_and_digest {
            Some((object_ref, transaction_digest)) => (
                Some(object_ref),
                if transaction_digest == TransactionDigest::genesis() {
                    None
                } else {
                    // Get the cert from the transaction digest
                    Some(self.read_certificate(&transaction_digest).await?.ok_or(
                        SuiError::CertificateNotfound {
                            certificate_digest: transaction_digest,
                        },
                    )?)
                },
            ),
            None => (None, None),
        };

        // Return the latest version of the object and the current lock if any, if requested.
        let object_and_lock = match request.request_kind {
            ObjectInfoRequestKind::LatestObjectInfo(request_layout) => {
                match self.get_object(&request.object_id).await {
                    Ok(Some(object)) => {
                        let lock = if !object.is_address_owned() {
                            // Only address owned objects have locks.
                            None
                        } else {
                            self.get_transaction_lock(
                                &object.compute_object_reference(),
                                self.epoch(),
                            )
                            .await?
                        };
                        let layout = match request_layout {
                            Some(format) => {
                                object.get_layout(format, self.module_cache.as_ref())?
                            }
                            None => None,
                        };

                        Some(ObjectResponse {
                            object,
                            lock,
                            layout,
                        })
                    }
                    Err(e) => return Err(e),
                    _ => None,
                }
            }
            ObjectInfoRequestKind::PastObjectInfoDebug(seq, request_layout) => {
                match self.database.get_object_by_key(&request.object_id, seq) {
                    Ok(Some(object)) => {
                        let layout = match request_layout {
                            Some(format) => {
                                object.get_layout(format, self.module_cache.as_ref())?
                            }
                            None => None,
                        };

                        Some(ObjectResponse {
                            object,
                            lock: None,
                            layout,
                        })
                    }
                    Err(e) => return Err(e),
                    _ => None,
                }
            }
            ObjectInfoRequestKind::PastObjectInfo(_) => None,
        };

        Ok(ObjectInfoResponse {
            parent_certificate,
            requested_object_reference,
            object_and_lock,
        })
    }

    pub fn handle_checkpoint_request(
        &self,
        _request: &CheckpointRequest,
    ) -> Result<CheckpointResponse, SuiError> {
        Err(SuiError::UnsupportedFeatureError {
            error: "Re-enable this once we can serve them from checkpoint v2".to_string(),
        })
    }

    pub fn handle_committee_info_request(
        &self,
        request: &CommitteeInfoRequest,
    ) -> SuiResult<CommitteeInfoResponse> {
        let (epoch, committee) = match request.epoch {
            Some(epoch) => (epoch, self.committee_store.get_committee(&epoch)?),
            None => {
                let committee = self.committee_store.get_latest_committee();
                (committee.epoch, Some(committee))
            }
        };
        Ok(CommitteeInfoResponse {
            epoch,
            committee_info: committee.map(|c| c.voting_rights),
        })
    }

    // TODO: This function takes both committee and genesis as parameter.
    // Technically genesis already contains committee information. Could consider merging them.
    #[allow(clippy::disallowed_methods)] // allow unbounded_channel()
    pub async fn new(
        name: AuthorityName,
        secret: StableSyncAuthoritySigner,
        store: Arc<AuthorityStore>,
        node_sync_store: Arc<NodeSyncStore>,
        committee_store: Arc<CommitteeStore>,
        indexes: Option<Arc<IndexStore>>,
        event_store: Option<Arc<EventStoreType>>,
        transaction_streamer: Option<Arc<TransactionStreamer>>,
        prometheus_registry: &Registry,
    ) -> Arc<Self> {
        let (tx, _rx) = tokio::sync::broadcast::channel(BROADCAST_CAPACITY);
        let native_functions =
            sui_framework::natives::all_natives(MOVE_STDLIB_ADDRESS, SUI_FRAMEWORK_ADDRESS);
        let move_vm = Arc::new(
            adapter::new_move_vm(native_functions.clone())
                .expect("We defined natives to not fail here"),
        );
        let module_cache = Arc::new(SyncModuleCache::new(ResolverWrapper(store.clone())));
        let event_handler = event_store.map(|es| {
            let handler = EventHandler::new(es, module_cache.clone());
            handler.regular_cleanup_task();
            Arc::new(handler)
        });
        let metrics = Arc::new(AuthorityMetrics::new(prometheus_registry));
        let (tx_ready_certificates, rx_ready_certificates) = unbounded_channel();
        let transaction_manager = Arc::new(
            TransactionManager::new(store.clone(), tx_ready_certificates, metrics.clone()).await,
        );

        let state = Arc::new(AuthorityState {
            name,
            secret,
            _native_functions: native_functions,
            move_vm,
            database: store.clone(),
            node_sync_store,
            indexes,
            // `module_cache` uses a separate in-mem cache from `event_handler`
            // this is because they largely deal with different types of MoveStructs
            module_cache,
            event_handler,
            transaction_streamer,
            committee_store,
            transaction_manager: transaction_manager.clone(),
            batch_channels: tx,
            batch_notifier: Arc::new(
                authority_notifier::TransactionNotifier::new(store.clone(), prometheus_registry)
                    .expect("Notifier cannot start."),
            ),
            metrics,
        });

        prometheus_registry
            .register(Box::new(ModuleCacheGauge::new(&state.module_cache)))
            .unwrap();

        // Process tx recovery log first, so that the batch and checkpoint recovery (below)
        // don't observe partially-committed txes.
        state
            .process_tx_recovery_log(None)
            .await
            .expect("Could not fully process recovery log at startup!");

        state
            .init_batches_from_database()
            .expect("Init batches failed!");

        // Start a task to execute ready certificates.
        let authority_state = Arc::downgrade(&state);
        spawn_monitored_task!(execution_process(authority_state, rx_ready_certificates));

        state
    }

    // TODO: Technically genesis_committee can be derived from genesis.
    pub async fn new_for_testing(
        genesis_committee: Committee,
        key: &AuthorityKeyPair,
        store_base_path: Option<PathBuf>,
        genesis: Option<&Genesis>,
    ) -> Arc<Self> {
        let secret = Arc::pin(key.copy());
        let path = match store_base_path {
            Some(path) => path,
            None => {
                let dir = std::env::temp_dir();
                let path = dir.join(format!("DB_{:?}", nondeterministic!(ObjectID::random())));
                std::fs::create_dir(&path).unwrap();
                path
            }
        };
        let default_genesis = Genesis::get_default_genesis();
        let genesis = match genesis {
            Some(genesis) => genesis,
            None => &default_genesis,
        };

        // unwrap ok - for testing only.
        let store = Arc::new(
            AuthorityStore::open_with_committee(
                &path.join("store"),
                None,
                &genesis_committee,
                genesis,
            )
            .await
            .unwrap(),
        );

        let epochs = Arc::new(CommitteeStore::new(
            path.join("epochs"),
            &genesis_committee,
            None,
        ));

        let node_sync_store = Arc::new(NodeSyncStore::open_tables_read_write(
            path.join("node_sync_db"),
            None,
            None,
        ));

        // add the object_basics module
        AuthorityState::new(
            secret.public().into(),
            secret.clone(),
            store,
            node_sync_store,
            epochs,
            None,
            None,
            None,
            &Registry::new(),
        )
        .await
    }

    /// Adds certificates to the pending certificate store and transaction manager for ordered execution.
    /// Currently, only used in tests and deprecated callsites.
    pub async fn add_pending_certificates(&self, certs: Vec<VerifiedCertificate>) -> SuiResult<()> {
        self.node_sync_store
            .batch_store_certs(certs.iter().cloned())?;
        self.epoch_store().insert_pending_certificates(&certs)?;
        self.transaction_manager.enqueue(certs).await
    }

    // Continually pop in-progress txes from the WAL and try to drive them to completion.
    pub async fn process_tx_recovery_log(&self, limit: Option<usize>) -> SuiResult {
        let mut limit = limit.unwrap_or(usize::MAX);
        let epoch_store = self.epoch_store();
        while limit > 0 {
            limit -= 1;
            if let Some((cert, tx_guard)) = epoch_store.wal().read_one_recoverable_tx().await? {
                let digest = tx_guard.tx_id();
                debug!(?digest, "replaying failed cert from log");

                if tx_guard.retry_num() >= MAX_TX_RECOVERY_RETRY {
                    // This tx will be only partially executed, however the store will be in a safe
                    // state. We will simply never reach eventual consistency for this TX.
                    // TODO: Should we revert the tx entirely? I'm not sure the effort is
                    // warranted, since the only way this can happen is if we are repeatedly
                    // failing to write to the db, in which case a revert probably won't succeed
                    // either.
                    error!(
                        ?digest,
                        "Abandoning in-progress TX after {} retries.", MAX_TX_RECOVERY_RETRY
                    );
                    // prevent the tx from going back into the recovery list again.
                    tx_guard.release();
                    continue;
                }

                if let Err(e) = self.process_certificate(tx_guard, &cert.into()).await {
                    warn!(?digest, "Failed to process in-progress certificate: {e}");
                }
            } else {
                break;
            }
        }

        Ok(())
    }

    pub fn reconfigure(&self, new_committee: Committee) -> SuiResult {
        // TODO: We should move the committee into epoch db store, so that the operation below
        // can become atomic.
        fp_ensure!(
            self.epoch() + 1 == new_committee.epoch,
            SuiError::from("Invalid new epoch to sign and update")
        );

        self.committee_store.insert_new_committee(&new_committee)?;
        self.db().reopen_epoch_db(new_committee);
        Ok(())
    }

    pub fn db(&self) -> Arc<AuthorityStore> {
        self.database.clone()
    }

    pub fn epoch_store(&self) -> Guard<Arc<AuthorityPerEpochStore>> {
        self.database.epoch_store()
    }

    pub fn clone_committee(&self) -> Committee {
        self.epoch_store().committee().clone()
    }

    // This method can only be called from ConsensusAdapter::begin_reconfiguration
    pub fn close_user_certs(&self, lock_guard: parking_lot::RwLockWriteGuard<'_, ReconfigState>) {
        self.epoch_store().close_user_certs(lock_guard)
    }

    pub(crate) async fn get_object(
        &self,
        object_id: &ObjectID,
    ) -> Result<Option<Object>, SuiError> {
        self.database.get_object(object_id)
    }

    pub async fn get_framework_object_ref(&self) -> SuiResult<ObjectRef> {
        Ok(self
            .get_object(&SUI_FRAMEWORK_ADDRESS.into())
            .await?
            .expect("framework object should always exist")
            .compute_object_reference())
    }

    pub async fn get_sui_system_state_object(&self) -> SuiResult<SuiSystemState> {
        self.database.get_sui_system_state_object()
    }

    pub async fn get_object_read(&self, object_id: &ObjectID) -> Result<ObjectRead, SuiError> {
        match self.database.get_latest_parent_entry(*object_id)? {
            None => Ok(ObjectRead::NotExists(*object_id)),
            Some((obj_ref, _)) => {
                if obj_ref.2.is_alive() {
                    match self.database.get_object_by_key(object_id, obj_ref.1)? {
                        None => {
                            error!("Object with in parent_entry is missing from object store, datastore is inconsistent");
                            Err(SuiError::ObjectNotFound {
                                object_id: *object_id,
                                version: Some(obj_ref.1),
                            })
                        }
                        Some(object) => {
                            let layout = object.get_layout(
                                ObjectFormatOptions::default(),
                                self.module_cache.as_ref(),
                            )?;
                            Ok(ObjectRead::Exists(obj_ref, object, layout))
                        }
                    }
                } else {
                    Ok(ObjectRead::Deleted(obj_ref))
                }
            }
        }
    }

    /// This function aims to serve rpc reads on past objects and
    /// we don't expect it to be called for other purposes.
    /// Depending on the object pruning policies that will be enforced in the
    /// future there is no software-level guarantee/SLA to retrieve an object
    /// with an old version even if it exists/existed.
    pub async fn get_past_object_read(
        &self,
        object_id: &ObjectID,
        version: SequenceNumber,
    ) -> Result<PastObjectRead, SuiError> {
        // Firstly we see if the object ever exists by getting its latest data
        match self.database.get_latest_parent_entry(*object_id)? {
            None => Ok(PastObjectRead::ObjectNotExists(*object_id)),
            Some((obj_ref, _)) => {
                if version > obj_ref.1 {
                    return Ok(PastObjectRead::VersionTooHigh {
                        object_id: *object_id,
                        asked_version: version,
                        latest_version: obj_ref.1,
                    });
                }
                if version < obj_ref.1 {
                    // Read past objects
                    return Ok(match self.database.get_object_by_key(object_id, version)? {
                        None => PastObjectRead::VersionNotFound(*object_id, version),
                        Some(object) => {
                            let layout = object.get_layout(
                                ObjectFormatOptions::default(),
                                self.module_cache.as_ref(),
                            )?;
                            let obj_ref = object.compute_object_reference();
                            PastObjectRead::VersionFound(obj_ref, object, layout)
                        }
                    });
                }
                // version is equal to the latest seq number this node knows
                if obj_ref.2.is_alive() {
                    match self.database.get_object_by_key(object_id, obj_ref.1)? {
                        None => {
                            error!("Object with in parent_entry is missing from object store, datastore is inconsistent");
                            Err(SuiError::ObjectNotFound {
                                object_id: *object_id,
                                version: Some(obj_ref.1),
                            })
                        }
                        Some(object) => {
                            let layout = object.get_layout(
                                ObjectFormatOptions::default(),
                                self.module_cache.as_ref(),
                            )?;
                            Ok(PastObjectRead::VersionFound(obj_ref, object, layout))
                        }
                    }
                } else {
                    Ok(PastObjectRead::ObjectDeleted(obj_ref))
                }
            }
        }
    }

    pub fn get_owner_objects(&self, owner: Owner) -> SuiResult<Vec<ObjectInfo>> {
        self.database.get_owner_objects(owner)
    }

    pub fn get_owner_objects_iterator(
        &self,
        owner: Owner,
    ) -> SuiResult<impl Iterator<Item = ObjectInfo> + '_> {
        self.database.get_owner_objects_iterator(owner)
    }

    pub fn get_total_transaction_number(&self) -> Result<u64, anyhow::Error> {
        QueryHelpers::get_total_transaction_number(&self.database)
    }

    pub fn get_transactions_in_range(
        &self,
        start: TxSequenceNumber,
        end: TxSequenceNumber,
    ) -> Result<Vec<(TxSequenceNumber, TransactionDigest)>, anyhow::Error> {
        QueryHelpers::get_transactions_in_range(&self.database, start, end)
    }

    pub fn get_recent_transactions(
        &self,
        count: u64,
    ) -> Result<Vec<(TxSequenceNumber, TransactionDigest)>, anyhow::Error> {
        QueryHelpers::get_recent_transactions(&self.database, count)
    }

    pub async fn get_transaction(
        &self,
        digest: TransactionDigest,
    ) -> Result<(VerifiedCertificate, TransactionEffects), anyhow::Error> {
        QueryHelpers::get_transaction(&self.database, &digest)
    }

    fn get_indexes(&self) -> SuiResult<Arc<IndexStore>> {
        match &self.indexes {
            Some(i) => Ok(i.clone()),
            None => Err(SuiError::UnsupportedFeatureError {
                error: "extended object indexing is not enabled on this server".into(),
            }),
        }
    }

    pub fn get_transactions(
        &self,
        query: TransactionQuery,
        cursor: Option<TransactionDigest>,
        limit: Option<usize>,
        reverse: bool,
    ) -> Result<Vec<TransactionDigest>, anyhow::Error> {
        // Lookup TransactionDigest sequence number,
        // also default cursor to 0 or the current sequence number depends on ordering.
        let cursor = if let Some(cursor) = cursor {
            self.get_indexes()?
                .get_transaction_seq(&cursor)?
                .ok_or_else(|| anyhow!("Transaction [{cursor:?}] not found."))?
        } else if reverse {
            TxSequenceNumber::MAX
        } else {
            TxSequenceNumber::MIN
        };

        Ok(match query {
            TransactionQuery::MoveFunction {
                package,
                module,
                function,
            } => self.get_indexes()?.get_transactions_by_move_function(
                package, module, function, cursor, limit, reverse,
            )?,
            TransactionQuery::InputObject(object_id) => self
                .get_indexes()?
                .get_transactions_by_input_object(object_id, cursor, limit, reverse)?,
            TransactionQuery::MutatedObject(object_id) => self
                .get_indexes()?
                .get_transactions_by_mutated_object(object_id, cursor, limit, reverse)?,
            TransactionQuery::FromAddress(address) => self
                .get_indexes()?
                .get_transactions_from_addr(address, cursor, limit, reverse)?,
            TransactionQuery::ToAddress(address) => self
                .get_indexes()?
                .get_transactions_to_addr(address, cursor, limit, reverse)?,
            TransactionQuery::All => {
                let iter = self.database.perpetual_tables.executed_sequence.iter();
                if reverse {
                    let iter = iter
                        .skip_prior_to(&cursor)?
                        .reverse()
                        .map(|(_, digest)| digest.transaction);
                    if let Some(limit) = limit {
                        iter.take(limit).collect()
                    } else {
                        iter.collect()
                    }
                } else {
                    let iter = iter.skip_to(&cursor)?.map(|(_, digest)| digest.transaction);
                    if let Some(limit) = limit {
                        iter.take(limit).collect()
                    } else {
                        iter.collect()
                    }
                }
            }
        })
    }

    pub async fn get_timestamp_ms(
        &self,
        digest: &TransactionDigest,
    ) -> Result<Option<u64>, anyhow::Error> {
        Ok(self.get_indexes()?.get_timestamp_ms(digest)?)
    }

    /// Returns a full handle to the event store, including inserts... so be careful!
    fn get_event_store(&self) -> Option<Arc<EventStoreType>> {
        self.event_handler
            .as_ref()
            .map(|handler| handler.event_store.clone())
    }

    pub async fn get_events(
        &self,
        query: EventQuery,
        cursor: Option<EventID>,
        limit: usize,
        descending: bool,
    ) -> Result<Vec<(EventID, SuiEventEnvelope)>, anyhow::Error> {
        let es = self.get_event_store().ok_or(SuiError::NoEventStore)?;
        let cursor = cursor.unwrap_or(if descending {
            // Database only support up to i64::MAX
            (i64::MAX, i64::MAX).into()
        } else {
            (0, 0).into()
        });

        let stored_events = match query {
            EventQuery::All => es.all_events(cursor, limit, descending).await?,
            EventQuery::Transaction(digest) => {
                es.events_by_transaction(digest, cursor, limit, descending)
                    .await?
            }
            EventQuery::MoveModule { package, module } => {
                let module_id = ModuleId::new(
                    AccountAddress::from(package),
                    Identifier::from_str(&module)?,
                );
                es.events_by_module_id(&module_id, cursor, limit, descending)
                    .await?
            }
            EventQuery::MoveEvent(struct_name) => {
                es.events_by_move_event_struct_name(&struct_name, cursor, limit, descending)
                    .await?
            }
            EventQuery::Sender(sender) => {
                es.events_by_sender(&sender, cursor, limit, descending)
                    .await?
            }
            EventQuery::Recipient(recipient) => {
                es.events_by_recipient(&recipient, cursor, limit, descending)
                    .await?
            }
            EventQuery::Object(object) => {
                es.events_by_object(&object, cursor, limit, descending)
                    .await?
            }
            EventQuery::TimeRange {
                start_time,
                end_time,
            } => {
                es.event_iterator(start_time, end_time, cursor, limit, descending)
                    .await?
            }
            EventQuery::EventType(event_type) => {
                es.events_by_type(event_type, cursor, limit, descending)
                    .await?
            }
        };
        let mut events = StoredEvent::into_event_envelopes(stored_events)?;
        // populate parsed json event
        for event in &mut events {
            if let SuiEvent::MoveEvent {
                type_, fields, bcs, ..
            } = &mut event.1.event
            {
                let struct_tag = parse_struct_tag(type_)?;
                let event =
                    Event::move_event_to_move_struct(&struct_tag, bcs, &*self.module_cache)?;
                let (_, event) = type_and_fields_from_move_struct(&struct_tag, event);
                *fields = Some(event)
            }
        }
        Ok(events)
    }

    pub async fn insert_genesis_object(&self, object: Object) {
        self.database
            .insert_genesis_object(object)
            .await
            .expect("Cannot insert genesis object")
    }

    pub async fn insert_genesis_objects_bulk_unsafe(&self, objects: &[&Object]) {
        self.database
            .bulk_object_insert(objects)
            .await
            .expect("Cannot bulk insert genesis objects")
    }

    /// Make an information response for a transaction
    pub async fn make_transaction_info(
        &self,
        transaction_digest: &TransactionDigest,
    ) -> Result<VerifiedTransactionInfoResponse, SuiError> {
        let mut info = self
            .database
            .get_signed_transaction_info(transaction_digest)?;
        // If the transaction was executed in previous epochs, the validator will
        // re-sign the effects with new current epoch so that a client is always able to
        // obtain an effects certificate at the current epoch.
        //
        // Why is this necessary? Consider the following case:
        // - assume there are 4 validators
        // - Quorum driver gets 2 signed effects before reconfig halt
        // - The tx makes it into final checkpoint.
        // - 2 validators go away and are replaced in the new epoch.
        // - The new epoch begins.
        // - The quorum driver cannot complete the partial effects cert from the previous epoch,
        //   because it may not be able to reach either of the 2 former validators.
        // - But, if the 2 validators that stayed are willing to re-sign the effects in the new
        //   epoch, the QD can make a new effects cert and return it to the client.
        //
        // This is a considered a short-term workaround. Eventually, Quorum Driver should be able
        // to return either an effects certificate, -or- a proof of inclusion in a checkpoint. In
        // the case above, the Quorum Driver would return a proof of inclusion in the final
        // checkpoint, and this code would no longer be necessary.
        //
        // Alternatively, some of the confusion around re-signing could be resolved if
        // CertifiedTransactionEffects included both the epoch in which the transaction became
        // final, as well as the epoch at which the effects were certified. In this case, there
        // would be nothing terribly odd about the validators from epoch N certifying that a
        // given TX became final in epoch N - 1. The confusion currently arises from the fact that
        // the epoch field in AuthoritySignInfo is overloaded both to identify the provenance of
        // the authority's signature, as well as to identify in which epoch the transaction was
        // executed.
        if let Some(effects) = info.signed_effects.take() {
            let cur_epoch = self.epoch();
            let new_effects = if effects.epoch() < cur_epoch {
                debug!(
                    effects_epoch=?effects.epoch(),
                    ?cur_epoch,
                    "Re-signing the effects with the current epoch"
                );
                SignedTransactionEffects::new(
                    cur_epoch,
                    effects.into_data(),
                    &*self.secret,
                    self.name,
                )
            } else {
                effects
            };
            info.signed_effects = Some(new_effects);
        }
        Ok(info)
    }

    fn make_account_info(&self, account: SuiAddress) -> Result<AccountInfoResponse, SuiError> {
        self.database
            .get_owner_objects(Owner::AddressOwner(account))
            .map(|object_ids| AccountInfoResponse {
                object_ids: object_ids.into_iter().map(|id| id.into()).collect(),
                owner: account,
            })
    }

    // Helper function to manage transaction_locks

    /// Set the transaction lock to a specific transaction
    #[instrument(level = "trace", skip_all)]
    pub async fn set_transaction_lock(
        &self,
        mutable_input_objects: &[ObjectRef],
        signed_transaction: VerifiedSignedTransaction,
    ) -> Result<(), SuiError> {
        self.database
            .lock_and_write_transaction(self.epoch(), mutable_input_objects, signed_transaction)
            .await
    }

    /// Update state and signals that a new transactions has been processed
    /// to the batch maker service.
    #[instrument(level = "trace", skip_all)]
    pub(crate) async fn commit_certificate(
        &self,
        inner_temporary_store: InnerTemporaryStore,
        certificate: &VerifiedCertificate,
        signed_effects: &SignedTransactionEffects,
        notifier_ticket: TransactionNotifierTicket,
    ) -> SuiResult<TxSequenceNumber> {
        let _metrics_guard = self.metrics.commit_certificate_latency.start_timer();

        let seq = notifier_ticket.seq();

        let digest = certificate.digest();
        let effects_digest = &signed_effects.digest();
        let seq = self
            .database
            .update_state(
                inner_temporary_store,
                certificate,
                seq,
                signed_effects,
                effects_digest,
            )
            .await
            .tap_ok(|_| {
                debug!(?digest, ?effects_digest, ?self.name, "commit_certificate finished");
            })?;
        // todo - ideally move this metric in NotifyRead once we have metrics in AuthorityStore
        self.metrics
            .pending_notify_read
            .set(self.database.effects_notify_read.num_pending() as i64);
        // We only notify i.e. update low watermark once database changes are committed
        notifier_ticket.notify();
        Ok(seq)
    }

    /// Returns true if certificate is a shared-object cert but has not been sequenced.
    fn is_cert_awaiting_sequencing(&self, certificate: &CertifiedTransaction) -> SuiResult<bool> {
        // always an error to call this on fullnode.
        assert!(!self.is_fullnode());

        if !certificate.contains_shared_object() {
            Ok(false)
        } else {
            self.database
                .consensus_message_processed(&ConsensusTransactionKey::Certificate(
                    *certificate.digest(),
                ))
                .map(|r| !r)
        }
    }

    /// Check whether certificate was processed by consensus.
    /// For shared lock certificates, if this function returns true means shared locks for this certificate are set
    pub fn consensus_message_processed(
        &self,
        certificate: &CertifiedTransaction,
    ) -> SuiResult<bool> {
        self.database
            .consensus_message_processed(&ConsensusTransactionKey::Certificate(
                *certificate.digest(),
            ))
    }

    /// Get a read reference to an object/seq lock
    pub async fn get_transaction_lock(
        &self,
        object_ref: &ObjectRef,
        epoch_id: EpochId,
    ) -> Result<Option<VerifiedSignedTransaction>, SuiError> {
        self.database
            .get_object_locking_transaction(object_ref, epoch_id)
            .await
    }

    // Helper functions to manage certificates

    /// Read from the DB of certificates
    pub async fn read_certificate(
        &self,
        digest: &TransactionDigest,
    ) -> Result<Option<VerifiedCertificate>, SuiError> {
        self.database.read_certificate(digest)
    }

    pub async fn parent(&self, object_ref: &ObjectRef) -> Option<TransactionDigest> {
        self.database
            .parent(object_ref)
            .expect("TODO: propagate the error")
    }

    pub async fn get_objects(
        &self,
        _objects: &[ObjectID],
    ) -> Result<Vec<Option<Object>>, SuiError> {
        self.database.get_objects(_objects)
    }

    /// Returns all parents (object_ref and transaction digests) that match an object_id, at
    /// any object version, or optionally at a specific version.
    pub async fn get_parent_iterator(
        &self,
        object_id: ObjectID,
        seq: Option<SequenceNumber>,
    ) -> Result<impl Iterator<Item = (ObjectRef, TransactionDigest)> + '_, SuiError> {
        {
            self.database.get_parent_iterator(object_id, seq)
        }
    }

    pub async fn get_latest_parent_entry(
        &self,
        object_id: ObjectID,
    ) -> Result<Option<(ObjectRef, TransactionDigest)>, SuiError> {
        self.database.get_latest_parent_entry(object_id)
    }

    /// Verifies transaction signatures and other data
    /// Important: This function can potentially be called in parallel and you can not rely on order of transactions to perform verification
    /// If this function return an error, transaction is skipped and is not passed to handle_consensus_transaction
    /// This function returns unit error and is responsible for emitting log messages for internal errors
    pub(crate) fn verify_consensus_transaction(
        &self,
        transaction: SequencedConsensusTransaction,
    ) -> Result<VerifiedSequencedConsensusTransaction, ()> {
        let _scope = monitored_scope("VerifyConsensusTransaction");
        if self
            .database
            .consensus_message_processed(&transaction.transaction.key())
            .expect("Storage error")
        {
            debug!(
                consensus_index=?transaction.consensus_index.index.transaction_index,
                tracking_id=?transaction.transaction.tracking_id,
                "handle_consensus_transaction UserTransaction [skip]",
            );
            self.metrics.skipped_consensus_txns.inc();
            return Err(());
        }
        // Signatures are verified as part of narwhal payload verification in SuiTxValidator
        match &transaction.transaction.kind {
            ConsensusTransactionKind::UserTransaction(_certificate) => {}
            ConsensusTransactionKind::CheckpointSignature(data) => {
                if transaction.sender_authority() != data.summary.auth_signature.authority {
                    warn!("CheckpointSignature authority {} does not match narwhal certificate source {}", data.summary.auth_signature.authority, transaction.certificate.origin() );
                    return Err(());
                }
            }
            ConsensusTransactionKind::EndOfPublish(authority) => {
                if &transaction.sender_authority() != authority {
                    warn!(
                        "EndOfPublish authority {} does not match narwhal certificate source {}",
                        authority,
                        transaction.certificate.origin()
                    );
                    return Err(());
                }
            }
        }
        Ok(VerifiedSequencedConsensusTransaction(transaction))
    }

    /// The transaction passed here went through verification in verify_consensus_transaction.
    /// This method is called in the exact sequence message are ordered in consensus.
    /// Errors returned by this call are treated as critical errors and cause node to panic.
    pub(crate) async fn handle_consensus_transaction<C: CheckpointServiceNotify>(
        &self,
        transaction: VerifiedSequencedConsensusTransaction,
        checkpoint_service: &Arc<C>,
    ) -> SuiResult {
        if let Some(certificate) = self
            .process_consensus_transaction(transaction, checkpoint_service)
            .await?
        {
            // The certificate has already been inserted into the pending_certificates table by
            // process_consensus_transaction() above.
            self.transaction_manager.enqueue(vec![certificate]).await?;
        }
        Ok(())
    }

    /// Depending on the type of the VerifiedSequencedConsensusTransaction wrapper,
    /// - Verify and initialize the state to execute the certificate.
    ///   Returns a VerifiedCertificate only if this succeeds.
    /// - Or update the state for checkpoint or epoch change protocol. Returns None.
    pub(crate) async fn process_consensus_transaction<C: CheckpointServiceNotify>(
        &self,
        transaction: VerifiedSequencedConsensusTransaction,
        checkpoint_service: &Arc<C>,
    ) -> SuiResult<Option<VerifiedCertificate>> {
        let _scope = monitored_scope("HandleConsensusTransaction");
        let VerifiedSequencedConsensusTransaction(SequencedConsensusTransaction {
            certificate: consensus_output,
            consensus_index,
            transaction,
        }) = transaction;
        let tracking_id = transaction.get_tracking_id();
        match &transaction.kind {
            ConsensusTransactionKind::UserTransaction(certificate) => {
                let authority = (&consensus_output.header.author).into();
                if self.database.sent_end_of_publish(&authority)? {
                    // This can not happen with valid authority
                    // With some edge cases narwhal might sometimes resend previously seen certificate after EndOfPublish
                    // However this certificate will be filtered out before this line by `consensus_message_processed` call in `verify_consensus_transaction`
                    // If we see some new certificate here it means authority is byzantine and sent certificate after EndOfPublish (or we have some bug in ConsensusAdapter)
                    warn!("[Byzantine authority] Authority {:?} sent a new, previously unseen certificate {:?} after it sent EndOfPublish message to consensus", authority.concise(), certificate.digest());
                    return Ok(None);
                }
                // Safe because signatures are verified when VerifiedSequencedConsensusTransaction
                // is constructed.
                let certificate = VerifiedCertificate::new_unchecked(*certificate.clone());

                debug!(
                    ?tracking_id,
                    tx_digest = ?certificate.digest(),
                    "handle_consensus_transaction UserTransaction",
                );

                if !self
                    .epoch_store()
                    .get_reconfig_state_read_lock_guard()
                    .should_accept_consensus_certs()
                {
                    debug!("Ignoring consensus certificate for transaction {:?} because of end of epoch",
                    certificate.digest());
                    return Ok(None);
                }

                if certificate.contains_shared_object() {
                    self.database
                        .record_shared_object_cert_from_consensus(
                            &transaction,
                            &certificate,
                            consensus_index,
                        )
                        .await?;
                } else {
                    self.database
                        .record_owned_object_cert_from_consensus(
                            &transaction,
                            &certificate,
                            consensus_index,
                        )
                        .await?;
                }

                Ok(Some(certificate))
            }
            ConsensusTransactionKind::CheckpointSignature(info) => {
                checkpoint_service.notify_checkpoint_signature(info)?;
                self.database
                    .record_consensus_transaction_processed(&transaction, consensus_index)
                    .await?;
                Ok(None)
            }
            ConsensusTransactionKind::EndOfPublish(authority) => {
                debug!("Received EndOfPublish from {:?}", authority.concise());
                self.database
                    .record_end_of_publish(*authority, &transaction, consensus_index)
                    .await?;
                Ok(None)
            }
        }
    }

    pub(crate) fn handle_commit_boundary<C: CheckpointServiceNotify>(
        &self,
        committed_dag: &Arc<CommittedSubDag>,
        checkpoint_service: &Arc<C>,
    ) -> SuiResult {
        let round = committed_dag.round();
        debug!("Commit boundary at {}", round);
        // This exchange is restart safe because of following:
        //
        // We try to read last checkpoint content and send it to the checkpoint service
        // CheckpointService::notify_checkpoint is idempotent in case you send same last checkpoint multiple times
        //
        // Only after CheckpointService::notify_checkpoint stores checkpoint in it's store we update checkpoint boundary
        if let Some((index, roots)) = self.database.last_checkpoint(round)? {
            let final_checkpoint_round = self.database.final_epoch_checkpoint()?;
            let final_checkpoint = match final_checkpoint_round.map(|r| r.cmp(&round)) {
                Some(CmpOrdering::Less) => {
                    debug!(
                        "Not forming checkpoint for round {} above final checkpoint round {:?}",
                        round, final_checkpoint_round
                    );
                    return Ok(());
                }
                Some(CmpOrdering::Equal) => true,
                Some(CmpOrdering::Greater) => false,
                None => false,
            };
            checkpoint_service.notify_checkpoint(index, roots, final_checkpoint)?;
        }
        self.database.record_checkpoint_boundary(round)
    }

    pub async fn create_advance_epoch_tx_cert(
        &self,
        next_epoch: EpochId,
        gas_cost_summary: &GasCostSummary,
        timeout: Duration,
    ) -> anyhow::Result<VerifiedCertificate> {
        debug!(
            ?next_epoch,
            computation_cost=?gas_cost_summary.computation_cost,
            storage_cost=?gas_cost_summary.storage_cost,
            storage_rebase=?gas_cost_summary.storage_rebate,
            "Creating advance epoch transaction"
        );
        let tx = VerifiedTransaction::new_change_epoch(
            next_epoch,
            gas_cost_summary.storage_cost,
            gas_cost_summary.computation_cost,
            gas_cost_summary.storage_rebate,
        );
        // If we fail to sign the transaction locally for whatever reason, it's not recoverable.
        self.handle_transaction_impl(tx.clone()).await?;
        debug!(?next_epoch, "Successfully signed advance epoch transaction");

        // If constructing network fails, it's not recoverable.
        let net = AuthorityAggregator::new_from_system_state(
            &self.database,
            &self.committee_store,
            &Registry::new(),
        )?;

        let result = tokio::time::timeout(timeout, async {
            loop {
                // We may have already executed this transaction somewhere else.
                // If so, no need to try to get it from the network.
                if let Ok(Some(VerifiedTransactionInfoResponse {
                    certified_transaction: Some(cert),
                    ..
                })) = self.get_tx_info_already_executed(tx.digest()).await
                {
                    return cert;
                }
                match net.process_transaction(tx.clone()).await {
                    Ok(cert) => {
                        return cert;
                    }
                    Err(err) => {
                        debug!("Did not create advance epoch transaction cert: {:?}", err);
                    }
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await;
        match result {
            Ok(cert) => {
                debug!(
                    "Successfully created advance epoch transaction cert: {:?}",
                    cert
                );
                Ok(cert)
            }
            Err(err) => {
                error!("Failed to create advance epoch transaction cert. Giving up");
                Err(err.into())
            }
        }
    }
}
