// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::checkpoints::CheckpointStore;
use crate::checkpoints::ConsensusSender;
use bytes::Bytes;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use multiaddr::Multiaddr;
use narwhal_types::TransactionProto;
use narwhal_types::TransactionsClient;
use parking_lot::Mutex;
use prometheus::register_int_gauge_with_registry;
use prometheus::IntCounter;
use prometheus::IntGauge;
use prometheus::Registry;
use prometheus::{register_histogram_with_registry, register_int_counter_with_registry, Histogram};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use std::collections::VecDeque;
use std::future::Future;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{Hash, Hasher},
};
use sui_metrics::monitored_future;
use sui_metrics::spawn_monitored_task;
use sui_types::base_types::TransactionDigest;
use sui_types::committee::Committee;
use sui_types::messages_checkpoint::CheckpointSequenceNumber;
use sui_types::messages_checkpoint::SignedCheckpointFragmentMessage;
use sui_types::{
    error::{SuiError, SuiResult},
    messages::{ConsensusTransaction, VerifiedCertificate},
};

use tap::prelude::*;
use tokio::time::Instant;

use sui_types::base_types::AuthorityName;
use tokio::{
    sync::{
        mpsc::{Receiver, Sender},
        oneshot,
    },
    task::JoinHandle,
    time::{timeout, Duration},
};
use tracing::debug;
use tracing::error;

#[cfg(test)]
#[path = "unit_tests/consensus_tests.rs"]
pub mod consensus_tests;

/// A serialized consensus transaction.
type SerializedConsensusTransaction = Vec<u8>;

/// The digest of a consensus transactions.
type ConsensusTransactionDigest = u64;

/// Channel to notify the caller when the Sui certificate has been sequenced.
type TxSequencedNotifier = oneshot::Sender<SuiResult<()>>;
type TxSequencedNotifierClose = oneshot::Sender<()>;

const SEQUENCING_CERTIFICATE_LATENCY_SEC_BUCKETS: &[f64] = &[
    0.1, 0.25, 0.5, 1., 2.5, 5., 7.5, 10., 12.5, 15., 20., 25., 30., 60., 90., 120., 180., 300.,
    600.,
];

pub struct ConsensusAdapterMetrics {
    // Certificate sequencing metrics
    pub sequencing_certificate_attempt: IntCounter,
    pub sequencing_certificate_success: IntCounter,
    pub sequencing_certificate_timeouts: IntCounter,
    pub sequencing_certificate_failures: IntCounter,
    pub sequencing_certificate_inflight: IntGauge,
    pub sequencing_acknowledge_latency: Histogram,

    // Fragment sequencing metrics
    pub sequencing_fragment_attempt: IntCounter,
    pub sequencing_fragment_success: IntCounter,
    pub sequencing_fragment_timeouts: IntCounter,
    pub sequencing_fragment_control_delay: IntGauge,
}

pub type OptArcConsensusAdapterMetrics = Option<Arc<ConsensusAdapterMetrics>>;

impl ConsensusAdapterMetrics {
    pub fn new(registry: &Registry) -> OptArcConsensusAdapterMetrics {
        Some(Arc::new(ConsensusAdapterMetrics {
            sequencing_certificate_attempt: register_int_counter_with_registry!(
                "sequencing_certificate_attempt",
                "Counts the number of certificates the validator attempts to sequence.",
                registry,
            )
            .unwrap(),
            sequencing_certificate_success: register_int_counter_with_registry!(
                "sequencing_certificate_success",
                "Counts the number of successfully sequenced certificates.",
                registry,
            )
            .unwrap(),
            sequencing_certificate_timeouts: register_int_counter_with_registry!(
                "sequencing_certificate_timeouts",
                "Counts the number of sequenced certificates that timed out.",
                registry,
            )
            .unwrap(),
            sequencing_certificate_failures: register_int_counter_with_registry!(
                "sequencing_certificate_failures",
                "Counts the number of sequenced certificates that failed other than by timeout.",
                registry,
            )
            .unwrap(),
            sequencing_certificate_inflight: register_int_gauge_with_registry!(
                "sequencing_certificate_inflight",
                "The inflight requests to sequence certificates.",
                registry,
            )
            .unwrap(),
            sequencing_acknowledge_latency: register_histogram_with_registry!(
                "sequencing_acknowledge_latency",
                "The latency for acknowledgement from sequencing engine .",
                SEQUENCING_CERTIFICATE_LATENCY_SEC_BUCKETS.to_vec(),
                registry,
            )
            .unwrap(),
            sequencing_fragment_attempt: register_int_counter_with_registry!(
                "sequencing_fragment_attempt",
                "Counts the number of sequenced fragments submitted.",
                registry,
            )
            .unwrap(),
            sequencing_fragment_success: register_int_counter_with_registry!(
                "sequencing_fragment_success",
                "Counts the number of successfully sequenced fragments.",
                registry,
            )
            .unwrap(),
            sequencing_fragment_timeouts: register_int_counter_with_registry!(
                "sequencing_fragment_timeouts",
                "Counts the number of sequenced fragments that timed out.",
                registry,
            )
            .unwrap(),
            sequencing_fragment_control_delay: register_int_gauge_with_registry!(
                "sequencing_fragment_control_delay",
                "The estimated latency of sequencing fragments.",
                registry,
            )
            .unwrap(),
        }))
    }

    pub fn new_test() -> OptArcConsensusAdapterMetrics {
        None
    }
}

/// Message to notify the consensus listener that a new transaction has been sent to consensus
/// or that the caller timed out on a specific transaction.
#[derive(Debug)]
pub enum ConsensusListenerMessage {
    New(
        SerializedConsensusTransaction,
        (TxSequencedNotifier, TxSequencedNotifierClose),
    ),
    Processed(Vec<u8>),
}

pub struct ConsensusWaiter {
    // This channel is used to signal the result if the transaction gets
    // sequenced and observed at the output of consensus.
    signal_back: oneshot::Receiver<SuiResult<()>>,
    // We use this channel as a signaling mechanism, to detect if the ConsensusWaiter
    // struct is dropped, and to clean up the ConsensusListener structures to prevent
    // memory leaks.
    signal_close: oneshot::Receiver<()>,
}

impl ConsensusWaiter {
    pub fn new() -> (
        ConsensusWaiter,
        (TxSequencedNotifier, TxSequencedNotifierClose),
    ) {
        let (notif, signal_back) = oneshot::channel();
        let (close, signal_close) = oneshot::channel();
        (
            ConsensusWaiter {
                signal_back,
                signal_close,
            },
            (notif, close),
        )
    }

    pub fn close(&mut self) {
        self.signal_close.close();
    }

    pub async fn wait_for_result(self) -> SuiResult<()> {
        self.signal_back
            .await
            .map_err(|e| SuiError::FailedToHearBackFromConsensus(e.to_string()))?
    }
}

/// Submit Sui certificates to the consensus.
pub struct ConsensusAdapter {
    /// The network client connecting to the consensus node of this authority.
    consensus_client: Box<dyn SubmitToConsensus>,
    /// The Sui committee information.
    committee: Committee,
    /// A channel to notify the consensus listener to take action for a transactions.
    _tx_consensus_listener: Sender<ConsensusListenerMessage>,
    /// Retries sending a transaction to consensus after this timeout.
    timeout: Duration,
    /// Number of submitted transactions still inflight at this node.
    num_inflight_transactions: AtomicU64,
    /// A structure to register metrics
    opt_metrics: OptArcConsensusAdapterMetrics,
}

#[async_trait::async_trait]
pub trait SubmitToConsensus: Sync + Send + 'static {
    async fn submit_to_consensus(&self, transaction: &ConsensusTransaction) -> SuiResult;
}

#[async_trait::async_trait]
impl SubmitToConsensus for TransactionsClient<sui_network::tonic::transport::Channel> {
    async fn submit_to_consensus(&self, transaction: &ConsensusTransaction) -> SuiResult {
        let serialized =
            bincode::serialize(transaction).expect("Serializing consensus transaction cannot fail");
        let bytes = Bytes::from(serialized.clone());
        self.clone()
            .submit_transaction(TransactionProto { transaction: bytes })
            .await
            .map_err(|e| SuiError::ConsensusConnectionBroken(format!("{:?}", e)))
            .tap_err(|r| {
                error!("Submit transaction failed with: {:?}", r);
            })?;
        Ok(())
    }
}

impl ConsensusAdapter {
    /// Make a new Consensus adapter instance.
    pub fn new(
        consensus_address: Multiaddr,
        committee: Committee,
        tx_consensus_listener: Sender<ConsensusListenerMessage>,
        timeout: Duration,
        opt_metrics: OptArcConsensusAdapterMetrics,
    ) -> Self {
        let consensus_client = Box::new(TransactionsClient::new(
            mysten_network::client::connect_lazy(&consensus_address)
                .expect("Failed to connect to consensus"),
        ));
        let num_inflight_transactions = Default::default();
        Self {
            consensus_client,
            committee,
            _tx_consensus_listener: tx_consensus_listener,
            timeout,
            num_inflight_transactions,
            opt_metrics,
        }
    }

    #[cfg(test)]
    pub fn new_test(
        consensus_client: Box<dyn SubmitToConsensus>,
        committee: Committee,
        tx_consensus_listener: Sender<ConsensusListenerMessage>,
        timeout: Duration,
        opt_metrics: OptArcConsensusAdapterMetrics,
    ) -> Self {
        let num_inflight_transactions = Default::default();
        Self {
            consensus_client,
            committee,
            _tx_consensus_listener: tx_consensus_listener,
            timeout,
            num_inflight_transactions,
            opt_metrics,
        }
    }

    pub fn num_inflight_transactions(&self) -> u64 {
        self.num_inflight_transactions.load(Ordering::Relaxed)
    }

    /// Check if this authority should submit the transaction to consensus.
    fn should_submit(
        committee: &Committee,
        ourselves: &AuthorityName,
        tx_digest: &TransactionDigest,
    ) -> bool {
        // the 32 is as requirement of the deault StdRng::from_seed choice
        let digest_bytes: [u8; 32] = tx_digest.to_bytes()[..32].try_into().unwrap();

        // permute the validators deterministically, based on the digest
        let mut rng = StdRng::from_seed(digest_bytes);
        let mut validators = committee.voting_rights.clone();
        validators.shuffle(&mut rng);

        // the last (f+1) elements by weight are the submitters for this transaction
        let mut total_weight = 0u64;
        let mut found = false;
        while total_weight < committee.validity_threshold() {
            if let Some((name, weight)) = validators.pop() {
                total_weight += weight;
                if name == *ourselves {
                    found = true;
                    break;
                }
            } else {
                unreachable!(
                    "We should cross the validity threshold before running out of validators"
                );
            }
        }
        // Are we one of the submitters?
        found

        // TODO [issue #1647]: Right now every transaction is submitted to (f+1) authorities.
        // We should bring this number down to one, and make sure the mapping to submitters is
        // refreshed frequently enough to make sure this is Byzantine-resistant
    }

    /// Submit a transaction to consensus, wait for its processing, and notify the caller.
    // Use .inspect when its stable.
    #[allow(clippy::option_map_unit_fn)]
    pub async fn submit(
        &self,
        authority: &AuthorityName,
        certificate: &VerifiedCertificate,
        processed_waiter: impl Future<Output = SuiResult<()>>,
    ) -> SuiResult {
        // Serialize the certificate in a way that is understandable to consensus (i.e., using
        // bincode) and it certificate to consensus.
        let transaction =
            ConsensusTransaction::new_certificate_message(authority, certificate.clone().into());
        let tracking_id = transaction.get_tracking_id();
        let tx_digest = certificate.digest();
        debug!(
            ?tracking_id,
            ?tx_digest,
            "Certified transaction consensus message created"
        );

        // Check if this authority submits the transaction to consensus.
        let should_submit = Self::should_submit(&self.committee, authority, tx_digest);
        let _inflight_guard = if should_submit {
            // Timer to record latency of acknowledgements from consensus
            let _timer = self
                .opt_metrics
                .as_ref()
                .map(|m| m.sequencing_acknowledge_latency.start_timer());

            // todo - we need stronger guarantees for checkpoints here (issue #5763)
            // todo - for owned objects this can also be done async
            self.consensus_client
                .submit_to_consensus(&transaction)
                .await?;

            Some(InflightDropGuard::acquire(self))
        } else {
            None
        };

        // We do not wait unless its a share object transaction being sequenced.
        if !certificate.contains_shared_object() {
            // We only record for shared object transactions
            return Ok(());
        };

        // Now consensus guarantees delivery after submit_transaction() if primary/workers are live
        match timeout(self.timeout, processed_waiter).await {
            Ok(Ok(())) => {
                // Increment the attempted certificate sequencing success
                self.opt_metrics.as_ref().map(|metrics| {
                    metrics.sequencing_certificate_success.inc();
                });
                Ok(())
            }
            Ok(Err(e)) => {
                // Increment the attempted certificate sequencing failure
                self.opt_metrics.as_ref().map(|metrics| {
                    metrics.sequencing_certificate_failures.inc();
                });
                Err(e)
            }
            Err(e) => {
                // Increment the attempted certificate sequencing timeout
                self.opt_metrics.as_ref().map(|metrics| {
                    metrics.sequencing_certificate_timeouts.inc();
                });

                // We drop the waiter which will signal to the conensus listener task to clean up
                // the channels.
                Err(SuiError::FailedToHearBackFromConsensus(e.to_string()))
            }
        }
    }
}

/// Tracks number of inflight consensus requests and relevant metrics
struct InflightDropGuard<'a> {
    adapter: &'a ConsensusAdapter,
}

impl<'a> InflightDropGuard<'a> {
    pub fn acquire(adapter: &'a ConsensusAdapter) -> Self {
        let inflight = adapter
            .num_inflight_transactions
            .fetch_add(1, Ordering::SeqCst);
        if let Some(metrics) = adapter.opt_metrics.as_ref() {
            metrics.sequencing_certificate_attempt.inc();
            metrics.sequencing_certificate_inflight.set(inflight as i64);
        }
        Self { adapter }
    }
}

impl<'a> Drop for InflightDropGuard<'a> {
    fn drop(&mut self) {
        let inflight = self
            .adapter
            .num_inflight_transactions
            .fetch_sub(1, Ordering::SeqCst);
        // Store the latest latency
        if let Some(metrics) = self.adapter.opt_metrics.as_ref() {
            metrics.sequencing_certificate_inflight.set(inflight as i64);
        }
    }
}

/// This module interfaces the consensus with Sui. It receives certificates input to consensus and
/// notify the called when they are sequenced.
pub struct ConsensusListener {
    /// Receive messages input to the consensus.
    rx_consensus_listener: Receiver<ConsensusListenerMessage>,
    /// Keep a map of all consensus inputs that are currently being sequenced.
    /// Maximum size of the pending notifiers is bounded by the maximum pending transactions of the node.
    pending: HashMap<ConsensusTransactionDigest, Vec<(u64, TxSequencedNotifier)>>,
}

impl ConsensusListener {
    /// Spawn a new consensus adapter in a dedicated tokio task.
    pub fn spawn(rx_consensus_listener: Receiver<ConsensusListenerMessage>) -> JoinHandle<()> {
        spawn_monitored_task!(Self {
            rx_consensus_listener,
            pending: HashMap::new(),
        }
        .run())
    }

    /// Hash serialized consensus transactions. We do not need specific cryptographic properties except
    /// only collision resistance.
    pub fn hash_serialized_transaction(
        serialized: &SerializedConsensusTransaction,
    ) -> ConsensusTransactionDigest {
        let mut hasher = DefaultHasher::new();
        let len = serialized.len();
        if len > 8 {
            // The first 8 bytes are the tracking id, and we don't want to hash that so that
            // certificates submitted by different validators are considered the same message.
            (serialized[8..]).hash(&mut hasher);
        } else {
            // If somehow the length is <= 8 (which is invalid), we just don't care and hash
            // the whole thing.
            serialized.hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Main loop receiving messages input to consensus and notifying the caller once the inputs
    /// are sequenced (or if an error happened).
    async fn run(mut self) {
        let mut closed_notifications = FuturesUnordered::new();
        let mut id_counter: u64 = 0;

        loop {
            tokio::select! {
                // A new transaction has been sent to consensus or is no longer needed.
                Some(message) = self.rx_consensus_listener.recv() => {
                    match message {
                        // Keep track of this certificates so we can notify the user later.
                        ConsensusListenerMessage::New(transaction, (replier, mut closer)) => {
                            let digest = Self::hash_serialized_transaction(&transaction);
                            let id = id_counter;
                            id_counter += 1;

                            let list = self.pending.entry(digest).or_insert_with(Vec::new);
                            list.push((id, replier));

                            // Register with the close notification.
                            closed_notifications.push(monitored_future!(async move {
                                // Wait for the channel to close
                                closer.closed().await;
                                // Return he digest concerned
                                (digest, id)
                            }));
                        },
                        ConsensusListenerMessage::Processed(serialized) => {
                            let digest = Self::hash_serialized_transaction(&serialized);
                            if let Some(repliers) = self.pending.remove(&digest) {
                                for (_, replier) in repliers {
                                    if replier.send(Ok(())).is_err() {
                                        debug!("No replier to listen to consensus output {digest}");
                                    }
                                }
                            }
                        }
                    }
                },

                Some((digest, id)) = closed_notifications.next() => {
                    let should_delete = if let Some(list) = self.pending.get_mut(&digest) {
                        // First clean up the list
                        list.retain(|(item_id, _)| *item_id != id);
                        // if the resuting list is empty we should delete the entry.
                        list.is_empty()
                    } else { false };

                    // Secondly we determine if we need to delete the entry
                    if should_delete {
                        self.pending.remove(&digest);
                    }

                }

            }
        }
    }
}

/// Send checkpoint fragments through consensus.
pub struct CheckpointSender {
    tx_checkpoint_consensus_adapter: Sender<SignedCheckpointFragmentMessage>,
}

impl CheckpointSender {
    pub fn new(tx_checkpoint_consensus_adapter: Sender<SignedCheckpointFragmentMessage>) -> Self {
        Self {
            tx_checkpoint_consensus_adapter,
        }
    }
}

impl ConsensusSender for CheckpointSender {
    fn send_to_consensus(&self, fragment_messages: SignedCheckpointFragmentMessage) -> SuiResult {
        self.tx_checkpoint_consensus_adapter
            .try_send(fragment_messages)
            .map_err(|e| SuiError::from(&e.to_string()[..]))
    }
}

fn weighted_average_half(old_average: u64, new_value: u64) -> u64 {
    (500 * old_average + 500 * new_value) / 1000
}

/// Reliably submit checkpoints fragments to consensus.
pub struct CheckpointConsensusAdapter {
    /// The network client connecting to the consensus node of this authority.
    consensus_client: TransactionsClient<sui_network::tonic::transport::Channel>,
    /// Channel to request to be notified when a given consensus transaction is sequenced.
    _tx_consensus_listener: Sender<ConsensusListenerMessage>,
    /// Receive new checkpoint fragments to sequence.
    rx_checkpoint_consensus_adapter: Receiver<SignedCheckpointFragmentMessage>,
    /// A pointer to the checkpoints local store.
    checkpoint_db: Arc<Mutex<CheckpointStore>>,
    /// The initial delay to wait before re-attempting a connection with consensus (in ms).
    retry_delay: Duration,
    /// The maximum number of checkpoint fragment pending sequencing.
    max_pending_transactions: usize,
    /// Keep all checkpoint fragment waiting to be sequenced.
    buffer: VecDeque<(SerializedConsensusTransaction, CheckpointSequenceNumber)>,

    /// A structure to register metrics
    opt_metrics: OptArcConsensusAdapterMetrics,
}

impl CheckpointConsensusAdapter {
    /// Create a new `CheckpointConsensusAdapter`.
    pub fn new(
        consensus_address: Multiaddr,
        tx_consensus_listener: Sender<ConsensusListenerMessage>,
        rx_checkpoint_consensus_adapter: Receiver<SignedCheckpointFragmentMessage>,
        checkpoint_db: Arc<Mutex<CheckpointStore>>,
        retry_delay: Duration,
        max_pending_transactions: usize,
        opt_metrics: OptArcConsensusAdapterMetrics,
    ) -> Self {
        // Create a new network client.
        let connection = mysten_network::client::connect_lazy(&consensus_address)
            .expect("Failed to connect to consensus");
        let consensus_client = TransactionsClient::new(connection);

        // Create the new instance.
        Self {
            consensus_client,
            _tx_consensus_listener: tx_consensus_listener,
            rx_checkpoint_consensus_adapter,
            checkpoint_db,
            retry_delay,
            max_pending_transactions,
            buffer: VecDeque::with_capacity(max_pending_transactions),
            opt_metrics,
        }
    }

    /// Spawn a `CheckpointConsensusAdapter` in a dedicated tokio task.
    pub fn spawn(mut self) -> JoinHandle<()> {
        spawn_monitored_task!(self.run())
    }

    /// Submit a transaction to consensus.
    // Use .inspect when its stable.
    #[allow(clippy::option_map_unit_fn)]
    async fn submit(&self, serialized: SerializedConsensusTransaction) -> SuiResult {
        let transaction = Bytes::from(serialized);
        let proto_transaction = TransactionProto { transaction };

        // Increment the attempted fragment sequencing failure
        self.opt_metrics.as_ref().map(|metrics| {
            metrics.sequencing_fragment_attempt.inc();
        });

        self.consensus_client
            .clone()
            .submit_transaction(proto_transaction)
            .await
            .map_err(|e| SuiError::ConsensusConnectionBroken(format!("{:?}", e)))
            .map(|_| ())
    }

    /// Wait for a transaction to be sequenced by consensus (or to timeout).
    async fn waiter<T>(
        receiver: ConsensusWaiter,
        retry_delay: Duration,
        deliver: T,
    ) -> (SuiResult<()>, u64, T) {
        let now = Instant::now();
        let outcome = match timeout(retry_delay, receiver.wait_for_result()).await {
            Ok(reply) => reply,
            Err(e) => Err(SuiError::FailedToHearBackFromConsensus(e.to_string())),
        };
        let conensus_latency = now.elapsed().as_millis() as u64;
        (outcome, conensus_latency, deliver)
    }

    /// Main loop receiving checkpoint fragments to reliably submit to consensus.
    // Use .inspect when its stable.
    #[allow(clippy::option_map_unit_fn)]
    async fn run(&mut self) {
        let mut waiting = FuturesUnordered::new();

        // Fragment sequencing latency estimation
        let mut latency_estimate = self.retry_delay.as_millis() as u64;
        let max_latency = latency_estimate * 100;

        // Continuously listen to checkpoint fragments and re-attempt sequencing if needed.
        loop {
            // Try to submit all pending checkpoint fragments to consensus.
            while let Some((serialized, sequence_number)) = self.buffer.pop_back() {
                match self.submit(serialized.clone()).await {
                    Ok(_) => {
                        // Notify the consensus listener that we wish to be notified once our
                        // consensus transaction is sequenced.
                        let (waiter, signals) = ConsensusWaiter::new();

                        let consensus_input =
                            ConsensusListenerMessage::New(serialized.clone(), signals);

                        // Add the receiver to the waiter. So we can retransmit if the
                        // connection fails.
                        let deliver = (serialized, sequence_number);
                        let timeout_delay =
                            Duration::from_millis(latency_estimate) + self.retry_delay;
                        let future =
                            monitored_future!(Self::waiter(waiter, timeout_delay, deliver));
                        waiting.push(future);

                        // Finally sent to consensus, after registering to avoid a race condition
                        self._tx_consensus_listener
                            .send(consensus_input)
                            .await
                            .expect("Failed to notify consensus listener");
                    }
                    Err(e) => {
                        error!("Checkpoint fragment submit failed: {:?}", e);
                        self.buffer.push_back((serialized, sequence_number));
                        break;
                    }
                }
            }

            // Process new events.
            tokio::select! {
                // Listen to new checkpoint fragments.
                Some(fragment) = self.rx_checkpoint_consensus_adapter.recv() => {
                    let sequence_number = fragment.message.proposer_sequence_number();

                    // Cleanup the buffer.
                    if self.buffer.len() >= self.max_pending_transactions {
                        // Drop the earliest fragments. They are not needed for liveness.
                        if let Some(proposal) = &self.checkpoint_db.lock().get_locals().current_proposal {
                            let current_sequence_number = proposal.sequence_number();
                            self.buffer.retain(|(_, s)| s >= current_sequence_number);
                        }
                    }

                    // Add the fragment to the buffer.
                    let (cp_seq, proposer, other) = fragment.message.message_key();
                    let transaction = ConsensusTransaction::new_checkpoint_message(fragment);
                    let tracking_id = transaction.get_tracking_id();
                    let serialized = bincode::serialize(&transaction).expect("Serialize consensus transaction cannot fail");
                    debug!(
                        ?tracking_id,
                        ?cp_seq,
                        size=?serialized.len(),
                        "Checkpoint fragment consensus message created. Proposer: {}, Other: {}",
                        proposer,
                        other,
                    );
                    self.buffer.push_front((serialized, sequence_number));
                },

                // Listen to checkpoint fragments who failed to be sequenced and need retries.
                Some((outcome, latency_ms, identifier)) = waiting.next() => {

                    // Update the latency estimate using a weigted average
                    // But also cap it upwards by max_latency
                    latency_estimate = max_latency.min(weighted_average_half(latency_estimate, latency_ms));

                    // Record the latest consensus latency estimate for fragments
                    self.opt_metrics.as_ref().map(|metrics| {
                        metrics.sequencing_fragment_control_delay.set(latency_estimate as i64);
                    });

                   if let Err(error) = outcome {
                       tracing::warn!("Failed to sequence checkpoint fragment, and re-submitting fragment: {error}");
                       let (serialized_transaction, checkpoint_sequence_number) = identifier;

                            // Increment the attempted fragment sequencing failure
                            self.opt_metrics.as_ref().map(|metrics| {
                                metrics.sequencing_fragment_timeouts.inc();
                            });

                       self.buffer.push_back((serialized_transaction, checkpoint_sequence_number));
                   } else {
                            // Increment the attempted fragment sequencing success
                            self.opt_metrics.as_ref().map(|metrics| {
                                metrics.sequencing_fragment_success.inc();
                            });
                   }
                },
            }
        }
    }
}

#[cfg(test)]
mod adapter_tests {
    use super::ConsensusAdapter;
    use fastcrypto::traits::KeyPair;
    use rand::{rngs::StdRng, Rng, RngCore, SeedableRng};
    use sui_types::{
        base_types::{TransactionDigest, TRANSACTION_DIGEST_LENGTH},
        committee::Committee,
        crypto::{get_key_pair_from_rng, AuthorityKeyPair, AuthorityPublicKeyBytes},
    };

    #[test]
    fn should_submit_selects_valid_submitters() {
        // grab a random committee and a random stake distribution
        let mut rng = StdRng::from_seed([0; 32]);
        const COMMITTEE_SIZE: usize = 10; // 3 * 3 + 1;
        let authorities = (0..COMMITTEE_SIZE)
            .map(|_k| {
                (
                    AuthorityPublicKeyBytes::from(
                        get_key_pair_from_rng::<AuthorityKeyPair, _>(&mut rng)
                            .1
                            .public(),
                    ),
                    rng.gen_range(0u64..10u64),
                )
            })
            .collect::<Vec<_>>();
        let committee = Committee::new(0, authorities.iter().cloned().collect()).unwrap();

        // generate random transaction digests, and account for validator selection
        const NUM_TEST_TRANSACTIONS: usize = 1000;

        for _tx_idx in 0..NUM_TEST_TRANSACTIONS {
            let mut tx_digest_bytes = [0u8; TRANSACTION_DIGEST_LENGTH];
            rng.fill_bytes(&mut tx_digest_bytes);
            let tx_digest = TransactionDigest::new(tx_digest_bytes);

            let total_stake_this_committee = authorities.iter().map(|(_name, stake)| stake).sum();
            // collect the stake of authorities which will be selected to submit the transaction
            let mut submitters_total_stake = 0u64;
            for (name, stake) in authorities.iter() {
                if ConsensusAdapter::should_submit(&committee, name, &tx_digest) {
                    submitters_total_stake += stake;
                }
            }
            assert!(submitters_total_stake >= committee.validity_threshold());
            assert!(submitters_total_stake < total_stake_this_committee);
        }
    }
}
