// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

/*
    Authorities have a passive component (in AuthorityState), but can also have active
    components to perform a number of functions such as:

    (1) Share transactions received with other authorities, to complete their execution
        in case clients fail before sharing a transaction with sufficient authorities.
    (2) Share certificates with other authorities in case clients fail before a
        certificate has its execution finalized.
    (3) Gossip executed certificates digests with other authorities through following
        each other and using push / pull to execute certificates.
    (4) Perform the active operations necessary to progress the periodic checkpointing
        protocol.

    This component manages the root of all these active processes. It spawns services
    and tasks that actively initiate network operations to progress all these
    processes.

    Some ground rules:
    - The logic here does nothing "privileged", namely any process that could not
      have been performed over the public authority interface by an untrusted
      client.
    - All logic here should be safe to the ActiveAuthority state being transient
      and multiple instances running in parallel per authority, or at untrusted
      clients. Or Authority state being stopped, without its state being saved
      (loss of store), and then restarted some time later.

*/

use arc_swap::ArcSwap;
use std::{collections::HashMap, ops::Deref, sync::Arc, time::Duration};
use sui_storage::{follower_store::FollowerStore, node_sync_store::NodeSyncStore};
use sui_types::{
    base_types::AuthorityName,
    error::{SuiError, SuiResult},
};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{debug, info};
use typed_store::traits::DBMapTableUtil;

use crate::{
    authority::AuthorityState,
    authority_aggregator::AuthorityAggregator,
    authority_client::AuthorityAPI,
    node_sync::{NodeSyncHandle, NodeSyncState},
};
use once_cell::sync::OnceCell;

use tokio::time::Instant;
pub mod gossip;
use gossip::{gossip_process, node_sync_process, GossipMetrics};

pub mod checkpoint_driver;
use crate::authority_active::checkpoint_driver::CheckpointMetrics;
use crate::epoch::reconfiguration::Reconfigurable;
use checkpoint_driver::{checkpoint_process, get_latest_checkpoint_from_all, sync_to_checkpoint};

pub mod execution_driver;

use self::{checkpoint_driver::CheckpointProcessControl, execution_driver::execution_process};

// TODO: Make these into a proper config
const MAX_RETRIES_RECORDED: u32 = 10;
const DELAY_FOR_1_RETRY_MS: u64 = 2_000;
const EXPONENTIAL_DELAY_BASIS: u64 = 2;
pub const MAX_RETRY_DELAY_MS: u64 = 30_000;

#[derive(Debug)]
pub struct AuthorityHealth {
    // Records the number of retries
    pub retries: u32,
    // The instant after which we should contact this
    // authority again.
    pub no_contact_before: Instant,
}

impl Default for AuthorityHealth {
    fn default() -> AuthorityHealth {
        AuthorityHealth {
            retries: 0,
            no_contact_before: Instant::now(),
        }
    }
}

impl AuthorityHealth {
    /// Sets the no contact instant to be larger than what
    /// is currently recorded.
    pub fn set_no_contact_for(&mut self, period: Duration) {
        let future_instant = Instant::now() + period;
        if self.no_contact_before < future_instant {
            self.no_contact_before = future_instant;
        }
    }

    // Reset the no contact to no delay
    pub fn reset_no_contact(&mut self) {
        self.no_contact_before = Instant::now();
    }

    pub fn can_initiate_contact_now(&self) -> bool {
        let now = Instant::now();
        self.no_contact_before <= now
    }
}

pub struct ActiveAuthority<A> {
    // The local authority state
    pub state: Arc<AuthorityState>,
    pub node_sync_state: Arc<NodeSyncState<A>>,
    node_sync_handle: OnceCell<NodeSyncHandle>,

    pub follower_store: Arc<FollowerStore>,
    // The network interfaces to other authorities
    pub net: ArcSwap<AuthorityAggregator<A>>,
    // Network health
    pub health: Arc<Mutex<HashMap<AuthorityName, AuthorityHealth>>>,
    // Gossip Metrics including gossip between validators and
    // node sync process between fullnode and validators
    pub gossip_metrics: GossipMetrics,
}

impl<A> ActiveAuthority<A> {
    pub fn new(
        authority: Arc<AuthorityState>,
        node_sync_store: Arc<NodeSyncStore>,
        follower_store: Arc<FollowerStore>,
        net: AuthorityAggregator<A>,
        gossip_metrics: GossipMetrics,
    ) -> SuiResult<Self> {
        let committee = authority.clone_committee();

        let net = Arc::new(net);

        let node_sync_state = Arc::new(NodeSyncState::new(
            authority.clone(),
            net.clone(),
            node_sync_store,
            gossip_metrics.clone(),
        ));

        Ok(ActiveAuthority {
            health: Arc::new(Mutex::new(
                committee
                    .names()
                    .map(|name| (*name, AuthorityHealth::default()))
                    .collect(),
            )),
            state: authority,
            node_sync_state,
            node_sync_handle: OnceCell::new(),
            follower_store,
            net: ArcSwap::from(net),
            gossip_metrics,
        })
    }

    fn net(&self) -> Arc<AuthorityAggregator<A>> {
        self.net.load().clone()
    }

    pub fn new_with_ephemeral_storage_for_test(
        authority: Arc<AuthorityState>,
        net: AuthorityAggregator<A>,
    ) -> SuiResult<Self> {
        let working_dir = tempfile::tempdir().unwrap();
        let follower_db_path = working_dir.path().join("follower_db");
        let sync_db_path = working_dir.path().join("node_sync_db");

        let follower_store = Arc::new(FollowerStore::open_tables_read_write(
            follower_db_path,
            None,
        ));
        let node_sync_store = Arc::new(NodeSyncStore::open_tables_read_write(sync_db_path, None));
        Self::new(
            authority,
            node_sync_store,
            follower_store,
            net,
            GossipMetrics::new_for_tests(),
        )
    }

    /// Returns the amount of time we should wait to be able to contact at least
    /// 2/3 of the nodes in the committee according to the `no_contact_before`
    /// instant stored in the authority health records. A network needs 2/3 stake
    /// live nodes, so before that we are unlikely to be able to make process
    /// even if we have a few connections.
    pub async fn minimum_wait_for_majority_honest_available(&self) -> Instant {
        let lock = self.health.lock().await;

        let health_overview: Vec<_> = lock
            .iter()
            .map(|(name, h)| {
                (
                    *name,
                    h.retries,
                    h.no_contact_before - tokio::time::Instant::now(),
                )
            })
            .collect();
        debug!(health_overview = ?health_overview, "Current validator health metrics");

        let (_, instant) = self.net.load().committee.robust_value(
            lock.iter().map(|(name, h)| (*name, h.no_contact_before)),
            // At least one honest node is at or above it.
            self.net.load().committee.quorum_threshold(),
        );
        instant
    }

    /// Adds one more retry to the retry counter up to MAX_RETRIES_RECORDED, and then increases
    /// the`no contact` value to DELAY_FOR_1_RETRY_MS * EXPONENTIAL_DELAY_BASIS ^ retries, up to
    /// a maximum delay of MAX_RETRY_DELAY_MS.
    pub async fn set_failure_backoff(&self, name: AuthorityName) {
        let mut lock = self.health.lock().await;
        let mut entry = lock.entry(name).or_default();
        entry.retries = u32::min(entry.retries + 1, MAX_RETRIES_RECORDED);
        let delay: u64 = u64::min(
            DELAY_FOR_1_RETRY_MS * u64::pow(EXPONENTIAL_DELAY_BASIS, entry.retries),
            MAX_RETRY_DELAY_MS,
        );
        entry.set_no_contact_for(Duration::from_millis(delay));
    }

    /// Resets retries to zero and sets no contact to zero delay.
    pub async fn set_success_backoff(&self, name: AuthorityName) {
        let mut lock = self.health.lock().await;
        let mut entry = lock.entry(name).or_default();
        entry.retries = 0;
        entry.reset_no_contact();
    }

    /// Checks given the current time if we should contact this authority, ie
    /// if we are past any `no contact` delay.
    pub async fn can_contact(&self, name: AuthorityName) -> bool {
        let mut lock = self.health.lock().await;
        let entry = lock.entry(name).or_default();
        entry.can_initiate_contact_now()
    }
}

impl<A> Clone for ActiveAuthority<A> {
    fn clone(&self) -> Self {
        ActiveAuthority {
            state: self.state.clone(),
            node_sync_state: self.node_sync_state.clone(),
            node_sync_handle: self.node_sync_handle.clone(),
            follower_store: self.follower_store.clone(),
            net: ArcSwap::from(self.net.load().clone()),
            health: self.health.clone(),
            gossip_metrics: self.gossip_metrics.clone(),
        }
    }
}

impl<A> ActiveAuthority<A>
where
    A: AuthorityAPI + Send + Sync + 'static + Clone,
{
    fn node_sync_handle(&self) -> NodeSyncHandle {
        let node_sync_state = self.node_sync_state.clone();
        self.node_sync_handle
            .get_or_init(|| NodeSyncHandle::new(node_sync_state, self.gossip_metrics.clone()))
            .clone()
    }

    pub async fn sync_to_latest_checkpoint(&self, metrics: &CheckpointMetrics) -> SuiResult {
        self.sync_to_latest_checkpoint_with_config(metrics, Default::default())
            .await
    }

    pub async fn sync_to_latest_checkpoint_with_config(
        &self,
        metrics: &CheckpointMetrics,
        checkpoint_process_control: CheckpointProcessControl,
    ) -> SuiResult {
        let checkpoint_store =
            self.state
                .checkpoints
                .clone()
                .ok_or(SuiError::UnsupportedFeatureError {
                    error: "Checkpoint not supported".to_owned(),
                })?;

        // TODO: fullnode should not get proposals
        // TODO: potentially move get_latest_proposal_and_checkpoint_from_all and
        // sync_to_checkpoint out of checkpoint_driver
        let checkpoint_summary = get_latest_checkpoint_from_all(
            self.net(),
            checkpoint_process_control.extra_time_after_quorum,
            checkpoint_process_control.timeout_until_quorum,
        )
        .await?;

        let checkpoint_summary = match checkpoint_summary {
            Some(c) => c,
            None => {
                info!(name = ?self.state.name, "no checkpoints found");
                return Ok(());
            }
        };

        sync_to_checkpoint(self, checkpoint_store, checkpoint_summary, metrics).await
    }

    /// Spawn gossip process
    pub async fn spawn_gossip_process(self: Arc<Self>, degree: usize) -> JoinHandle<()> {
        // Number of tasks at most "degree" and no more than committee - 1
        // (validators do not follow themselves for gossip)
        let committee = self.state.committee.load().deref().clone();
        let target_num_tasks = usize::min(committee.num_members() - 1, degree);

        tokio::task::spawn(async move {
            gossip_process(&self, target_num_tasks).await;
        })
    }

    pub async fn spawn_node_sync_process(self: Arc<Self>) -> JoinHandle<()> {
        let committee = self.state.committee.load().deref().clone();
        // nodes follow all validators to ensure they can eventually determine
        // finality of certs. We need to follow 2f+1 _honest_ validators to
        // eventually find finality, therefore we must follow all validators.
        let target_num_tasks = committee.num_members();

        tokio::task::spawn(async move {
            node_sync_process(&self, target_num_tasks).await;
        })
    }

    /// Spawn pending certificate execution process
    pub async fn spawn_execute_process(self: Arc<Self>) -> JoinHandle<()> {
        tokio::task::spawn(async move {
            execution_process(&self).await;
        })
    }
}

impl<A> ActiveAuthority<A>
where
    A: AuthorityAPI + Send + Sync + 'static + Clone + Reconfigurable,
{
    pub async fn spawn_checkpoint_process(
        self: Arc<Self>,
        metrics: CheckpointMetrics,
        enable_reconfig: bool,
    ) -> JoinHandle<()> {
        self.spawn_checkpoint_process_with_config(
            CheckpointProcessControl::default(),
            metrics,
            enable_reconfig,
        )
        .await
    }

    pub async fn spawn_checkpoint_process_with_config(
        self: Arc<Self>,
        checkpoint_process_control: CheckpointProcessControl,
        metrics: CheckpointMetrics,
        enable_reconfig: bool,
    ) -> JoinHandle<()> {
        // Spawn task to take care of checkpointing
        tokio::task::spawn(async move {
            checkpoint_process(&self, &checkpoint_process_control, metrics, enable_reconfig).await;
        })
    }
}
