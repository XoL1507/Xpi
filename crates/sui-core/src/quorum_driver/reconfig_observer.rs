// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use async_trait::async_trait;
use std::sync::Arc;
use sui_types::committee::Committee;
use tracing::{info, warn};

use crate::{
    authority::AuthorityStore,
    authority_aggregator::{AuthAggMetrics, AuthorityAggregator},
    authority_client::NetworkAuthorityClient,
    epoch::committee_store::CommitteeStore,
    safe_client::SafeClientMetricsBase,
};

use super::QuorumDriver;

#[async_trait]
pub trait ReconfigObserver<A> {
    async fn run(&mut self, quorum_driver: Arc<QuorumDriver<A>>);
}

pub struct OnsiteReconfigObserver {
    reconfig_rx: tokio::sync::broadcast::Receiver<Committee>,
    authority_store: Arc<AuthorityStore>,
    committee_store: Arc<CommitteeStore>,
    safe_client_metrics_base: SafeClientMetricsBase,
    auth_agg_metrics: AuthAggMetrics,
}

impl OnsiteReconfigObserver {
    pub fn new(
        reconfig_rx: tokio::sync::broadcast::Receiver<Committee>,
        authority_store: Arc<AuthorityStore>,
        committee_store: Arc<CommitteeStore>,
        safe_client_metrics_base: SafeClientMetricsBase,
        auth_agg_metrics: AuthAggMetrics,
    ) -> Self {
        Self {
            reconfig_rx,
            authority_store,
            committee_store,
            safe_client_metrics_base,
            auth_agg_metrics,
        }
    }

    async fn create_authority_aggregator_from_system_state(
        &self,
    ) -> AuthorityAggregator<NetworkAuthorityClient> {
        AuthorityAggregator::new_from_system_state(
            &self.authority_store,
            &self.committee_store,
            self.safe_client_metrics_base.clone(),
            self.auth_agg_metrics.clone(),
        )
        // TODO: we should tolerate when <= f validators give invalid addresses
        // GH issue: https://github.com/MystenLabs/sui/issues/7019
        .unwrap_or_else(|e| {
            warn!("panicked? {:?}", e);
            panic!(
                "Failed to create AuthorityAggregator from System State: {:?}",
                e
            )
        })
    }
}

#[async_trait]
impl ReconfigObserver<NetworkAuthorityClient> for OnsiteReconfigObserver {
    async fn run(&mut self, quorum_driver: Arc<QuorumDriver<NetworkAuthorityClient>>) {
        // A tiny optimization: when the a very stale node just starts, the
        // channel may fill up committees quickly. Here we skip directly to
        // the last known committee by looking at SuiSystemState.
        let authority_agg = self.create_authority_aggregator_from_system_state().await;
        if authority_agg.committee.epoch > quorum_driver.current_epoch() {
            quorum_driver
                .update_validators(Arc::new(authority_agg))
                .await;
        }
        loop {
            match self.reconfig_rx.recv().await {
                Ok(committee) => {
                    info!("Got reconfig message: {}", committee);
                    if committee.epoch > quorum_driver.current_epoch() {
                        let authority_agg =
                            self.create_authority_aggregator_from_system_state().await;
                        quorum_driver
                            .update_validators(Arc::new(authority_agg))
                            .await;
                    } else {
                        // This should only happen when the node just starts
                        warn!("Ignored non-newer from reconfig channel: {}", committee);
                    }
                }
                // Neither closed channel nor lagged shall happen
                Err(other_err) => panic!(
                    "Got unexpected error from reconfig broadcast channel: {:?}",
                    other_err
                ),
            }
        }
    }
}
