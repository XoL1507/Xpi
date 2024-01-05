// Copyright (c) 2021, Facebook, Inc. and its affiliates
// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crypto::NetworkPublicKey;
use futures::{stream::FuturesUnordered, StreamExt};
use network::{P2pNetwork, ReliableNetwork};
use tokio::{sync::watch, task::JoinHandle};
use types::{
    metered_channel::Receiver, ReconfigureNotification, WorkerOthersBatchMessage,
    WorkerOurBatchMessage,
};

/// The maximum number of digests kept in memory waiting to be sent to the primary.
pub const MAX_PENDING_DIGESTS: usize = 10_000;

// Send batches' digests to the primary.
pub struct PrimaryConnector {
    /// The public key of this authority.
    primary_name: NetworkPublicKey,
    /// Receive reconfiguration updates.
    rx_reconfigure: watch::Receiver<ReconfigureNotification>,
    /// Input channels to receive the messages to send to the primary.
    rx_our_batch: Receiver<WorkerOurBatchMessage>,
    rx_others_batch: Receiver<WorkerOthersBatchMessage>,
    /// A network sender to send the batches' digests to the primary.
    primary_client: P2pNetwork,
}

impl PrimaryConnector {
    #[must_use]
    pub fn spawn(
        primary_name: NetworkPublicKey,
        rx_reconfigure: watch::Receiver<ReconfigureNotification>,
        rx_our_batch: Receiver<WorkerOurBatchMessage>,
        rx_others_batch: Receiver<WorkerOthersBatchMessage>,
        primary_client: P2pNetwork,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            Self {
                primary_name,
                rx_reconfigure,
                rx_our_batch,
                rx_others_batch,
                primary_client,
            }
            .run()
            .await;
        })
    }

    async fn run(&mut self) {
        let mut futures = FuturesUnordered::new();
        loop {
            tokio::select! {
                // Send the batches through the network.
                Some(batch) = self.rx_our_batch.recv() => {
                    if futures.len() >= MAX_PENDING_DIGESTS {
                        tracing::warn!("Primary unreachable: dropping {batch:?}");
                        continue;
                    }

                    let handle = self.primary_client.send(self.primary_name.to_owned(), &batch).await;
                    futures.push(handle);
                },
                Some(batch) = self.rx_others_batch.recv() => {
                    if futures.len() >= MAX_PENDING_DIGESTS {
                        tracing::warn!("Primary unreachable: dropping {batch:?}");
                        continue;
                    }

                    let handle = self.primary_client.send(self.primary_name.to_owned(), &batch).await;
                    futures.push(handle);
                },

                // Trigger reconfigure.
                result = self.rx_reconfigure.changed() => {
                    result.expect("Committee channel dropped");
                    // TODO: Move logic to handle epoch & committee changes to wherever anemo
                    // network is managed after worker-to-worker interface is migrated.
                    if self.rx_reconfigure.borrow().clone() == ReconfigureNotification::Shutdown {
                        return
                    }
                }

                Some(_result) = futures.next() => ()
            }
        }
    }
}
