// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
use prometheus::{
    default_registry, register_histogram_with_registry, register_int_counter_vec_with_registry,
    register_int_counter_with_registry, register_int_gauge_vec_with_registry,
    register_int_gauge_with_registry, Histogram, IntCounter, IntCounterVec, IntGauge, IntGaugeVec,
    Registry,
};

#[derive(Clone, Debug)]
pub struct ConsensusMetrics {
    /// The number of rounds for which the Dag holds certificates (for Tusk or Bullshark)
    pub consensus_dag_rounds: IntGaugeVec,
    /// The last committed round from consensus
    pub last_committed_round: IntGaugeVec,
    /// The number of elements from the vertices secondary index (external consensus)
    pub external_consensus_dag_vertices_elements: IntGaugeVec,
    /// The number of elements in the dag (external consensus)
    pub external_consensus_dag_size: IntGaugeVec,
    /// The number of times the consensus state was restored from the consensus store
    /// following a node restart
    pub recovered_consensus_state: IntCounter,
    /// The number of certificates from consensus that were restored and sent to the executor
    /// following a node restart
    pub recovered_consensus_output: IntCounter,
    /// The approximate size in memory (including heap allocations) of the Dag.
    pub dag_size_bytes: IntGauge,
    /// The latency between two successful commit rounds
    pub commit_rounds_latency: Histogram,
    /// The number of certificates committed per commit round
    pub committed_certificates: Histogram,
    /// When a certificate is received on an odd round, we check
    /// about the previous (even) round leader. We do have three possible cases which
    /// are tagged as values of the label "outcome":
    /// * not_found: the leader certificate has not been found at all
    /// * not_enough_support: when the leader certificate has been found but there was not enough support
    /// * elected: when the leader certificate has been found and had enough support
    pub leader_election: IntCounterVec,
}

impl ConsensusMetrics {
    pub fn new(registry: &Registry) -> Self {
        Self {
            consensus_dag_rounds: register_int_gauge_vec_with_registry!(
                "consensus_dag_rounds",
                "The number of rounds for which the consensus Dag holds certificates",
                &[],
                registry
            ).unwrap(),
            last_committed_round: register_int_gauge_vec_with_registry!(
                "last_committed_round",
                "The most recent round that has been committed from consensus",
                &[],
                registry
            ).unwrap(),
            external_consensus_dag_vertices_elements: register_int_gauge_vec_with_registry!(
                "external_consensus_dag_vertices_elements",
                "The number of elements in the vertices secondary index in the inner dag structure (external consensus)",
                &[],
                registry
            ).unwrap(),
            external_consensus_dag_size: register_int_gauge_vec_with_registry!(
                "external_consensus_dag_size",
                "The number of elements in the inner dag (external consensus)",
                &[],
                registry
            ).unwrap(),
            recovered_consensus_state: register_int_counter_with_registry!(
                "recovered_consensus_state",
                "The number of times the consensus state was restored from the consensus store following a node restart",
                registry
            ).unwrap(),
            recovered_consensus_output: register_int_counter_with_registry!(
                "recovered_consensus_output", 
                "The number of certificates from consensus that were restored and sent to the executor following a node restart",
                registry
            ).unwrap(),
            dag_size_bytes: register_int_gauge_with_registry!(
                "dag_size_bytes",
                "The approximate size in memory (including heap allocations) of the dag",
                registry
            ).unwrap(),
            commit_rounds_latency: register_histogram_with_registry!(
                "consensus_commit_rounds_latency",
                "The latency between two successful commit rounds (when we have successful leader election)",
                // buckets in milliseconds
                vec![
                    100.0, 200.0, 500.0, 1_000.0, 2_000.0, 3_000.0, 4_000.0, 5_000.0, 8_000.0,
                    10_000.0, 15_000.0, 20_000.0, 30_000.0, 50_000.0, 100_000.0, 200_000.0
                ],
                registry
            ).unwrap(),
            committed_certificates: register_histogram_with_registry!(
                "committed_certificates",
                "The number of certificates committed on a commit round",
                // buckets in number of certificates
                vec![
                    0.0, 5.0, 10.0, 20.0, 30.0, 40.0, 50.0, 100.0, 150.0,
                    200.0, 300.0, 400.0, 500.0, 1_000.0, 2_000.0, 5_000.0
                ],
                registry
            ).unwrap(),
            leader_election: register_int_counter_vec_with_registry!(
                "leader_election",
                "The outcome of a leader election round",
                &["outcome"],
                registry
            ).unwrap(),
        }
    }
}

impl Default for ConsensusMetrics {
    fn default() -> Self {
        Self::new(default_registry())
    }
}

#[derive(Clone, Debug)]
pub struct ChannelMetrics {
    /// occupancy of the channel from the `Consensus` to `SubscriberHandler`.
    /// See also:
    /// * tx_committed_certificates in primary, where the committed certificates
    /// from `Consensus` are sent to `primary::Core`
    /// * tx_new_certificates where the newly created certificates are sent
    /// from `primary::Core` to `Consensus`
    pub tx_sequence: IntGauge,
}

impl ChannelMetrics {
    pub fn new(registry: &Registry) -> Self {
        Self {
            tx_sequence: register_int_gauge_with_registry!(
                "tx_sequence",
                "occupancy of the channel from the `Consensus` to `SubscriberHandler`",
                registry
            )
            .unwrap(),
        }
    }
}

impl Default for ChannelMetrics {
    fn default() -> Self {
        Self::new(default_registry())
    }
}
