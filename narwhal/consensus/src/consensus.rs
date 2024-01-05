// Copyright (c) 2021, Facebook, Inc. and its affiliates
// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::mutable_key_type)]

use crate::{metrics::ConsensusMetrics, ConsensusOutput, SequenceNumber};
use config::Committee;
use crypto::PublicKey;
use fastcrypto::hash::Hash;
use std::{
    cmp::{max, Ordering},
    collections::HashMap,
    sync::Arc,
};
use storage::CertificateStore;
use tokio::{sync::watch, task::JoinHandle};
use tracing::{debug, info, instrument};
use types::{
    metered_channel, Certificate, CertificateDigest, ConsensusStore, ReconfigureNotification,
    Round, StoreResult,
};

/// The representation of the DAG in memory.
pub type Dag = HashMap<Round, HashMap<PublicKey, (CertificateDigest, Certificate)>>;

/// The state that needs to be persisted for crash-recovery.
pub struct ConsensusState {
    /// The last committed round.
    pub last_committed_round: Round,
    // Keeps the last committed round for each authority. This map is used to clean up the dag and
    // ensure we don't commit twice the same certificate.
    pub last_committed: HashMap<PublicKey, Round>,
    /// Keeps the latest committed certificate (and its parents) for every authority. Anything older
    /// must be regularly cleaned up through the function `update`.
    pub dag: Dag,
    /// Metrics handler
    pub metrics: Arc<ConsensusMetrics>,
}

impl ConsensusState {
    pub fn new(genesis: Vec<Certificate>, metrics: Arc<ConsensusMetrics>) -> Self {
        let genesis = genesis
            .into_iter()
            .map(|x| (x.origin(), (x.digest(), x)))
            .collect::<HashMap<_, _>>();

        Self {
            last_committed_round: 0,
            last_committed: genesis
                .iter()
                .map(|(x, (_, y))| (x.clone(), y.round()))
                .collect(),
            dag: [(0, genesis)]
                .iter()
                .cloned()
                .collect::<HashMap<_, HashMap<_, _>>>(),
            metrics,
        }
    }

    pub fn new_from_store(
        genesis: Vec<Certificate>,
        metrics: Arc<ConsensusMetrics>,
        recover_last_committed: HashMap<PublicKey, Round>,
        cert_store: CertificateStore,
        gc_depth: Round,
    ) -> Self {
        // We return a bool here which is always true to use as a "recovery token". This
        // allows us to ensure that the primary is spawned only after the
        // consensus is guaranteed to be in a state where it can process messages it
        // receives from the primary when the primary starts up. We do this by passing the
        // recovery token generated here and checking it before the primary spawn method.
        let last_committed_round = *recover_last_committed
            .iter()
            .max_by(|a, b| a.1.cmp(b.1))
            .map(|(_k, v)| v)
            .unwrap_or_else(|| &0);

        if last_committed_round == 0 {
            return Self::new(genesis, metrics);
        }
        metrics.recovered_consensus_state.inc();

        let dag = Self::construct_dag_from_cert_store(cert_store, last_committed_round, gc_depth);

        Self {
            last_committed_round,
            last_committed: recover_last_committed,
            dag,
            metrics,
        }
    }

    #[instrument(level = "info", skip_all)]
    pub fn construct_dag_from_cert_store(
        cert_store: CertificateStore,
        last_committed_round: Round,
        gc_depth: Round,
    ) -> Dag {
        let mut dag: Dag = HashMap::new();
        info!(
            "Recreating dag from last committed round: {}",
            last_committed_round
        );

        let min_round = last_committed_round.saturating_sub(gc_depth);
        // get all certificates at a round > min_round
        let cert_map = cert_store.after_round(min_round + 1).unwrap();

        let num_certs = cert_map.len();
        for (digest, cert) in cert_map.into_iter().map(|c| (c.digest(), c)) {
            let inner = dag.get_mut(&cert.header.round);
            match inner {
                Some(m) => {
                    m.insert(cert.header.author.clone(), (digest, cert.clone()));
                }
                None => {
                    dag.entry(cert.header.round)
                        .or_insert_with(HashMap::new)
                        .insert(cert.header.author.clone(), (digest, cert.clone()));
                }
            }
        }
        info!(
            "Dag was restored and contains {} certs for {} rounds",
            num_certs,
            dag.len()
        );

        dag
    }

    #[allow(clippy::result_unit_err)]
    pub fn try_insert(&mut self, certificate: Certificate) -> Result<(), ()> {
        let last_committed = self
            .last_committed
            .get(&certificate.origin())
            .cloned()
            .unwrap_or_default();
        if certificate.round() < last_committed {
            debug!(
                "Ignoring certificate {:?} as it is past last committed round for this origin {}",
                certificate, last_committed
            );
            Err(())
        } else {
            self.dag
                .entry(certificate.round())
                .or_insert_with(HashMap::new)
                .insert(certificate.origin(), (certificate.digest(), certificate));
            Ok(())
        }
    }

    /// Update and clean up internal state base on committed certificates.
    pub fn update(&mut self, certificate: &Certificate, gc_depth: Round) {
        self.last_committed
            .entry(certificate.origin())
            .and_modify(|r| *r = max(*r, certificate.round()))
            .or_insert_with(|| certificate.round());

        let last_committed_round = *Iterator::max(self.last_committed.values()).unwrap();
        self.last_committed_round = last_committed_round;

        self.metrics
            .last_committed_round
            .with_label_values(&[])
            .set(last_committed_round as i64);

        // We purge all certificates past the gc depth
        self.dag.retain(|r, _| r + gc_depth >= last_committed_round);
        for (name, round) in &self.last_committed {
            self.dag.retain(|r, authorities| {
                // We purge certificates for `name` prior to its latest commit
                if r < round {
                    authorities.remove(name);
                }
                !authorities.is_empty()
            });
        }
    }
}

/// Describe how to sequence input certificates.
pub trait ConsensusProtocol {
    fn process_certificate(
        &mut self,
        // The state of the consensus protocol.
        state: &mut ConsensusState,
        // The latest consensus index.
        consensus_index: SequenceNumber,
        // The new certificate.
        certificate: Certificate,
    ) -> StoreResult<Vec<ConsensusOutput>>;

    fn update_committee(&mut self, new_committee: Committee) -> StoreResult<()>;
}

pub struct Consensus<ConsensusProtocol> {
    /// The committee information.
    committee: Committee,

    /// Receive reconfiguration update.
    rx_reconfigure: watch::Receiver<ReconfigureNotification>,
    /// Receives new certificates from the primary. The primary should send us new certificates only
    /// if it already sent us its whole history.
    rx_new_certificates: metered_channel::Receiver<Certificate>,
    /// Outputs the sequence of ordered certificates to the primary (for cleanup and feedback).
    tx_committed_certificates: metered_channel::Sender<Certificate>,
    /// Outputs the sequence of ordered certificates to the application layer.
    tx_sequence: metered_channel::Sender<ConsensusOutput>,

    /// The (global) consensus index. We assign one index to each sequenced certificate. this is
    /// helpful for clients.
    consensus_index: SequenceNumber,

    /// The consensus protocol to run.
    protocol: ConsensusProtocol,

    /// Metrics handler
    metrics: Arc<ConsensusMetrics>,

    /// Inner state
    state: ConsensusState,
}

impl<Protocol> Consensus<Protocol>
where
    Protocol: ConsensusProtocol + Send + 'static,
{
    #[must_use]
    pub fn spawn(
        committee: Committee,
        store: Arc<ConsensusStore>,
        cert_store: CertificateStore,
        rx_reconfigure: watch::Receiver<ReconfigureNotification>,
        rx_new_certificates: metered_channel::Receiver<Certificate>,
        tx_committed_certificates: metered_channel::Sender<Certificate>,
        tx_sequence: metered_channel::Sender<ConsensusOutput>,
        protocol: Protocol,
        metrics: Arc<ConsensusMetrics>,
        gc_depth: Round,
    ) -> JoinHandle<()> {
        // The consensus state (everything else is immutable).
        let genesis = Certificate::genesis(&committee);
        let recovered_last_committed = store.read_last_committed();
        let state = ConsensusState::new_from_store(
            genesis,
            metrics.clone(),
            recovered_last_committed,
            cert_store,
            gc_depth,
        );
        let consensus_index = store
            .read_last_consensus_index()
            .expect("Failed to load consensus index from store");

        let s = Self {
            committee,
            rx_reconfigure,
            rx_new_certificates,
            tx_committed_certificates,
            tx_sequence,
            consensus_index,
            protocol,
            metrics,
            state,
        };

        tokio::spawn(s.run())
    }

    fn change_epoch(&mut self, new_committee: Committee) -> StoreResult<ConsensusState> {
        self.committee = new_committee.clone();
        self.protocol.update_committee(new_committee)?;

        self.consensus_index = 0;

        let genesis = Certificate::genesis(&self.committee);
        Ok(ConsensusState::new(genesis, self.metrics.clone()))
    }

    async fn run(self) {
        self.run_inner().await.expect("Failed to run consensus")
    }

    async fn run_inner(mut self) -> StoreResult<()> {
        // Listen to incoming certificates.
        loop {
            tokio::select! {
                Some(certificate) = self.rx_new_certificates.recv() => {
                    // If the core already moved to the next epoch we should pull the next
                    // committee as well.
                    match certificate.epoch().cmp(&self.committee.epoch()) {
                        Ordering::Greater => {
                            let message = self.rx_reconfigure.borrow_and_update().clone();
                            match message  {
                                ReconfigureNotification::NewEpoch(new_committee) => {
                                    self.state = self.change_epoch(new_committee)?;
                                },
                                ReconfigureNotification::UpdateCommittee(new_committee) => {
                                    self.committee = new_committee;
                                }
                                ReconfigureNotification::Shutdown => return Ok(()),
                            }
                            tracing::debug!("Committee updated to {}", self.committee);
                        }
                        Ordering::Less => {
                            // We already updated committee but the core is slow.
                            tracing::debug!("Already moved to the next epoch");
                            continue
                        },
                        Ordering::Equal => {
                            // Nothing to do, we can proceed.
                        }
                    }

                    // Process the certificate using the selected consensus protocol.
                    let sequence =
                        self.protocol
                            .process_certificate(&mut self.state, self.consensus_index, certificate)?;

                    // Update the consensus index.
                    self.consensus_index += sequence.len() as u64;

                    // Output the sequence in the right order.
                    for output in sequence {
                        let certificate = &output.certificate;
                        tracing::debug!("Commit in Sequence {:?}", output);

                        #[cfg(not(feature = "benchmark"))]
                        if output.consensus_index % 5_000 == 0 {
                            tracing::debug!("Committed {}", certificate.header);
                        }

                        #[cfg(feature = "benchmark")]
                        for digest in certificate.header.payload.keys() {
                            // NOTE: This log entry is used to compute performance.
                            tracing::info!("Committed {} -> {:?}", certificate.header, digest);
                        }

                        // Update DAG size metric periodically to limit computation cost.
                        // TODO: this should be triggered on collection when library support for
                        // closure metrics is available.
                        if output.consensus_index % 1_000 == 0 {
                            self.metrics
                                .dag_size_bytes
                                .set((mysten_util_mem::malloc_size(&self.state.dag) + std::mem::size_of::<Dag>()) as i64);
                        }

                        self.tx_committed_certificates
                            .send(certificate.clone())
                            .await
                            .expect("Failed to send certificate to primary");

                        if let Err(e) = self.tx_sequence.send(output).await {
                            tracing::warn!("Failed to output certificate: {e}");
                        }
                    }

                    self.metrics
                        .consensus_dag_rounds
                        .with_label_values(&[])
                        .set(self.state.dag.len() as i64);
                },

                // Check whether the committee changed.
                result = self.rx_reconfigure.changed() => {
                    result.expect("Committee channel dropped");
                    let message = self.rx_reconfigure.borrow().clone();
                    match message {
                        ReconfigureNotification::NewEpoch(new_committee) => {
                            self.state = self.change_epoch(new_committee)?;
                        },
                        ReconfigureNotification::UpdateCommittee(new_committee) => {
                            self.committee = new_committee;
                        }
                        ReconfigureNotification::Shutdown => return Ok(())
                    }
                    tracing::debug!("Committee updated to {}", self.committee);
                }
            }
        }
    }
}
