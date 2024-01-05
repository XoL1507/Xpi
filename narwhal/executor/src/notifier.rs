// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
use crate::{ExecutionIndices, ExecutionState, ExecutorMetrics};
use consensus::ConsensusOutput;
use fastcrypto::hash::Hash;
use std::sync::Arc;
use sui_metrics::spawn_monitored_task;
use tokio::task::JoinHandle;
use tracing::{debug, info};

use types::{metered_channel, Batch, Timestamp};

#[derive(Clone, Debug)]
pub struct BatchIndex {
    pub consensus_output: Arc<ConsensusOutput>,
    pub next_certificate_index: u64,
    pub batch_index: u64,
}

pub struct Notifier<State: ExecutionState> {
    rx_notifier: metered_channel::Receiver<(BatchIndex, Batch)>,
    callback: State,
    metrics: Arc<ExecutorMetrics>,
}

impl<State: ExecutionState + Send + Sync + 'static> Notifier<State> {
    pub fn spawn(
        rx_notifier: metered_channel::Receiver<(BatchIndex, Batch)>,
        callback: State,
        metrics: Arc<ExecutorMetrics>,
    ) -> JoinHandle<()> {
        let notifier = Notifier {
            rx_notifier,
            callback,
            metrics,
        };
        spawn_monitored_task!(notifier.run())
    }

    async fn run(mut self) {
        while let Some((index, batch)) = self.rx_notifier.recv().await {
            debug!(
                "Notifier processes batch {}, num of transactions: {}",
                batch.digest(),
                batch.transactions.len()
            );
            self.metrics.notifier_processed_batches.inc();
            let mut bytes = 0usize;
            for (transaction_index, transaction) in batch.transactions.into_iter().enumerate() {
                let execution_indices = ExecutionIndices {
                    next_certificate_index: index.next_certificate_index,
                    next_batch_index: index.batch_index + 1,
                    next_transaction_index: transaction_index as u64 + 1,
                };
                bytes += transaction.len();
                self.callback
                    .handle_consensus_transaction(
                        &index.consensus_output,
                        execution_indices,
                        transaction,
                    )
                    .await;
            }
            // this is temporary
            // we will get explicit signal from consensus on where is commit boundary in the future
            info!("index.batch_index: {}, len: {}", index.batch_index, index.consensus_output.certificate.header.payload.len());
            if index.batch_index + 1
                == index.consensus_output.certificate.header.payload.len() as u64
            {
                info!("notify_commit_boundary: {}, len: {}", index.batch_index, index.consensus_output.certificate.header.payload.len());
                self.callback
                    .notify_commit_boundary(&index.consensus_output)
                    .await;
            }
            self.metrics
                .batch_execution_latency
                .observe(batch.metadata.created_at.elapsed().as_secs_f64());
            self.metrics.notifier_processed_bytes.inc_by(bytes as u64);
        }
    }
}
