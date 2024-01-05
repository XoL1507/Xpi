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
use std::{
    collections::{BTreeMap, HashMap},
    ops::Deref,
    sync::Arc,
    time::Duration,
};
use sui_storage::{follower_store::FollowerStore, node_sync_store::NodeSyncStore};
use sui_types::{base_types::AuthorityName, error::SuiResult};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::error;

use crate::{
    authority::AuthorityState, authority_aggregator::AuthorityAggregator,
    authority_client::AuthorityAPI,
};
use tokio::time::Instant;

pub mod gossip;
use gossip::{gossip_process, node_sync_process};

pub mod checkpoint_driver;
use checkpoint_driver::checkpoint_process;

pub mod execution_driver;

use self::{checkpoint_driver::CheckpointProcessControl, execution_driver::execution_process};

// TODO: Make these into a proper config
const MAX_RETRIES_RECORDED: u32 = 10;
const DELAY_FOR_1_RETRY_MS: u64 = 2_000;
const EXPONENTIAL_DELAY_BASIS: u64 = 2;
pub const MAX_RETRY_DELAY_MS: u64 = 30_000;

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
    pub follower_store: Arc<FollowerStore>,
    // The network interfaces to other authorities
    pub net: ArcSwap<AuthorityAggregator<A>>,
    // Network health
    pub health: Arc<Mutex<HashMap<AuthorityName, AuthorityHealth>>>,
}

impl<A> ActiveAuthority<A> {
    pub fn new(
        authority: Arc<AuthorityState>,
        follower_store: Arc<FollowerStore>,
        authority_clients: BTreeMap<AuthorityName, A>,
    ) -> SuiResult<Self> {
        let committee = authority.clone_committee();

        Ok(ActiveAuthority {
            health: Arc::new(Mutex::new(
                committee
                    .voting_rights
                    .iter()
                    .map(|(name, _)| (*name, AuthorityHealth::default()))
                    .collect(),
            )),
            state: authority,
            follower_store,
            net: ArcSwap::from(Arc::new(AuthorityAggregator::new(
                committee,
                authority_clients,
            ))),
        })
    }

    pub fn new_with_ephemeral_follower_store(
        authority: Arc<AuthorityState>,
        authority_clients: BTreeMap<AuthorityName, A>,
    ) -> SuiResult<Self> {
        let working_dir = tempfile::tempdir().unwrap();
        let follower_store = Arc::new(FollowerStore::open(&working_dir).expect("cannot open db"));
        Self::new(authority, follower_store, authority_clients)
    }

    /// Returns the amount of time we should wait to be able to contact at least
    /// 2/3 of the nodes in the committee according to the `no_contact_before`
    /// instant stored in the authority health records. A network needs 2/3 stake
    /// live nodes, so before that we are unlikely to be able to make process
    /// even if we have a few connections.
    pub async fn minimum_wait_for_majority_honest_available(&self) -> Instant {
        let lock = self.health.lock().await;
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
            follower_store: self.follower_store.clone(),
            net: ArcSwap::from(self.net.load().clone()),
            health: self.health.clone(),
        }
    }
}

impl<A> ActiveAuthority<A>
where
    A: AuthorityAPI + Send + Sync + 'static + Clone,
{
    pub async fn spawn_checkpoint_process(self) {
        self.spawn_checkpoint_process_with_config(Some(CheckpointProcessControl::default()))
            .await
    }

    /// Spawn all active tasks.
    pub async fn spawn_checkpoint_process_with_config(
        self,
        checkpoint_process_control: Option<CheckpointProcessControl>,
    ) {
        let active = Arc::new(self);

        // Spawn task to take care of checkpointing
        let checkpoint_locals = active; // .clone();
        let _checkpoint_join = tokio::task::spawn(async move {
            if let Some(checkpoint) = checkpoint_process_control {
                checkpoint_process(&checkpoint_locals, &checkpoint).await;
            }
        });

        if let Err(err) = _checkpoint_join.await {
            error!("Join checkpoint task end error: {:?}", err);
        }
    }

    /// Spawn gossip process
    pub async fn spawn_gossip_process(self, degree: usize) -> JoinHandle<()> {
        let active = Arc::new(self);

        // Number of tasks at most "degree" and no more than committee - 1
        // (validators do not follow themselves for gossip)
        let committee = active.state.committee.load().deref().clone();
        let target_num_tasks = usize::min(committee.voting_rights.len() - 1, degree);

        tokio::task::spawn(async move {
            gossip_process(&active, target_num_tasks).await;
        })
    }

    pub async fn spawn_node_sync_process(
        self,
        node_sync_store: Arc<NodeSyncStore>,
    ) -> JoinHandle<()> {
        let active = Arc::new(self);
        let committee = active.state.committee.load().deref().clone();
        // nodes follow all validators to ensure they can eventually determine
        // finality of certs. We need to follow 2f+1 _honest_ validators to
        // eventually find finality, therefore we must follow all validators.
        let target_num_tasks = committee.voting_rights.len();

        tokio::task::spawn(async move {
            node_sync_process(&active, target_num_tasks, node_sync_store).await;
        })
    }

    /// Spawn gossip process
    pub async fn spawn_execute_process(self) -> JoinHandle<()> {
        let active = Arc::new(self);

        let locals = active;
        tokio::task::spawn(async move {
            execution_process(&locals).await;
        })
    }
}
