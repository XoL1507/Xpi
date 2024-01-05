// Copyright (c) 2021, Facebook, Inc. and its affiliates
// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
use config::{Committee, SharedWorkerCache, Stake, WorkerId};
use crypto::PublicKey;
use fastcrypto::hash::Hash;
use futures::stream::{futures_unordered::FuturesUnordered, StreamExt as _};
use network::{CancelOnDropHandler, P2pNetwork, ReliableNetwork};
use store::Store;
use sui_metrics::{monitored_future, spawn_monitored_task};
use tokio::{sync::watch, task::JoinHandle};
use types::{
    error::DagError,
    metered_channel::{Receiver, Sender},
    Batch, BatchDigest, ReconfigureNotification, WorkerBatchMessage, WorkerOurBatchMessage,
};

#[cfg(test)]
#[path = "tests/quorum_waiter_tests.rs"]
pub mod quorum_waiter_tests;

/// The QuorumWaiter waits for 2f authorities to acknowledge reception of a batch.
pub struct QuorumWaiter {
    /// The public key of this authority.
    name: PublicKey,
    /// The id of this worker.
    id: WorkerId,
    // The persistent storage.
    store: Store<BatchDigest, Batch>,
    /// The committee information.
    committee: Committee,
    /// The worker information cache.
    worker_cache: SharedWorkerCache,
    /// Receive reconfiguration updates.
    rx_reconfigure: watch::Receiver<ReconfigureNotification>,
    /// Input Channel to receive commands.
    rx_quorum_waiter: Receiver<Batch>,
    /// Channel to deliver batches for which we have enough acknowledgments.
    tx_our_batch: Sender<WorkerOurBatchMessage>,
    /// A network sender to broadcast the batches to the other workers.
    network: P2pNetwork,
}

impl QuorumWaiter {
    /// Spawn a new QuorumWaiter.
    #[must_use]
    pub fn spawn(
        name: PublicKey,
        id: WorkerId,
        store: Store<BatchDigest, Batch>,
        committee: Committee,
        worker_cache: SharedWorkerCache,
        rx_reconfigure: watch::Receiver<ReconfigureNotification>,
        rx_quorum_waiter: Receiver<Batch>,
        tx_our_batch: Sender<WorkerOurBatchMessage>,
        network: P2pNetwork,
    ) -> JoinHandle<()> {
        spawn_monitored_task!(async move {
            Self {
                name,
                id,
                store,
                committee,
                worker_cache,
                rx_reconfigure,
                rx_quorum_waiter,
                tx_our_batch,
                network,
            }
            .run()
            .await;
        })
    }

    /// Helper function. It waits for a future to complete and then delivers a value.
    async fn waiter(
        wait_for: CancelOnDropHandler<anemo::Result<anemo::Response<()>>>,
        deliver: Stake,
    ) -> Stake {
        let _ = wait_for.await;
        deliver
    }

    /// Main loop.
    async fn run(&mut self) {
        loop {
            tokio::select! {
                Some(batch) = self.rx_quorum_waiter.recv() => {
                    // Broadcast the batch to the other workers.
                    let workers: Vec<_> = self
                        .worker_cache
                        .load()
                        .others_workers(&self.name, &self.id)
                        .into_iter()
                        .map(|(name, info)| (name, info.name))
                        .collect();
                    let (primary_names, worker_names): (Vec<_>, _) = workers.into_iter().unzip();
                    let message = WorkerBatchMessage{batch: batch.clone()};
                    let handlers = self.network.broadcast(worker_names, &message).await;

                    // Collect all the handlers to receive acknowledgements.
                    let mut wait_for_quorum: FuturesUnordered<_> = primary_names
                        .into_iter()
                        .zip(handlers.into_iter())
                        .map(|(name, handler)| {
                            let stake = self.committee.stake(&name);
                            monitored_future!(Self::waiter(handler, stake))
                        })
                        .collect();

                    // Wait for the first 2f nodes to send back an Ack. Then we consider the batch
                    // delivered and we send its digest to the primary (that will include it into
                    // the dag). This should reduce the amount of synching.
                    let threshold = self.committee.quorum_threshold();
                    let mut total_stake = self.committee.stake(&self.name);
                    loop {
                        tokio::select! {
                            Some(stake) = wait_for_quorum.next() => {
                                total_stake += stake;
                                if total_stake >= threshold {
                                    let digest = batch.digest();
                                    let metadata = batch.metadata.clone();
                                    self.store.async_write(digest, batch).await;
                                    if self.tx_our_batch.send(WorkerOurBatchMessage{
                                        digest,
                                        worker_id: self.id,
                                        metadata
                                    }).await.is_err() {
                                        tracing::debug!("{}", DagError::ShuttingDown);
                                    }
                                    break;
                                }
                            }

                            result = self.rx_reconfigure.changed() => {
                                result.expect("Committee channel dropped");
                                let message = self.rx_reconfigure.borrow().clone();
                                match message {
                                    ReconfigureNotification::NewEpoch(new_committee)
                                        | ReconfigureNotification::UpdateCommittee(new_committee) => {
                                            self.committee = new_committee;
                                            tracing::debug!("Dropping batch: committee updated to {}", self.committee);
                                            break; // Don't wait for acknowledgements.
                                    },
                                    ReconfigureNotification::Shutdown => return
                                }
                            }
                        }
                    }
                },

                // Trigger reconfigure.
                result = self.rx_reconfigure.changed() => {
                    result.expect("Committee channel dropped");
                    let message = self.rx_reconfigure.borrow().clone();
                    match message {
                        ReconfigureNotification::NewEpoch(new_committee) => {
                            self.committee = new_committee;
                        },
                        ReconfigureNotification::UpdateCommittee(new_committee) => {
                            self.committee = new_committee;

                        },
                        ReconfigureNotification::Shutdown => return
                    }
                    tracing::debug!("Committee updated to {}", self.committee);
                }
            }
        }
    }
}
