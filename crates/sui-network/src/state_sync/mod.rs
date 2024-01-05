// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Peer-to-peer data synchronization of checkpoints.
//!
//! This StateSync module is responsible for the synchronization and dissemination of checkpoints
//! and the transactions, and their effects, contained within. This module is *not* responsible for
//! the execution of the transactions included in a checkpoint, that process is left to another
//! component in the system.
//!
//! # High-level Overview of StateSync
//!
//! StateSync discovers new checkpoints via a few different sources:
//! 1. If this node is a Validator, checkpoints will be produced via consensus at which point
//!    consensus can notify state-sync of the new checkpoint via [Handle::send_checkpoint].
//! 2. A peer notifies us of the latest checkpoint which they have synchronized. State-Sync will
//!    also periodically query its peers to discover what their latest checkpoint is.
//!
//! We keep track of two different watermarks:
//! * highest_verified_checkpoint - This is the highest checkpoint header that we've locally
//!   verified. This indicated that we have in our persistent store (and have verified) all
//!   checkpoint headers up to and including this value.
//! * highest_synced_checkpoint - This is the highest checkpoint that we've fully synchronized,
//!   meaning we've downloaded and have in our persistent stores all of the transactions, and their
//!   effects (but not the objects), for all checkpoints up to and including this point. This is
//!   the watermark that is shared with other peers, either via notification or when they query for
//!   our latest checkpoint, and is intended to be used as a guarantee of data availability.
//!
//! The `PeerHeights` struct is used to track the highest_synced_checkpoint watermark for all of
//! our peers.
//!
//! When a new checkpoint is discovered, and we've determined that it is higher than our
//! highest_verified_checkpoint, then StateSync will kick off a task to synchronize and verify all
//! checkpoints between our highest_synced_checkpoint and the newly discovered checkpoint. This
//! process is done by querying one of our peers for the checkpoints we're missing (using the
//! `PeerHeights` struct as a way to intelligently select which peers have the data available for
//! us to query) at which point we will locally verify the signatures on the checkpoint header with
//! the appropriate committee (based on the epoch). As checkpoints are verified, the
//! highest_synced_checkpoint watermark will be ratcheted up.
//!
//! Once we've ratcheted up our highest_verified_checkpoint, and if it is higher than
//! highest_synced_checkpoint, StateSync will then kick off a task to synchronize the contents of
//! all of the checkpoints from highest_synced_checkpoint..=highest_verified_checkpoint. After the
//! contents of each checkpoint is fully downloaded, StateSync will update our
//! highest_synced_checkpoint watermark and send out a notification on a broadcast channel
//! indicating that a new checkpoint has been fully downloaded. Notifications on this broadcast
//! channel will always be made in order. StateSync will also send out a notification to its peers
//! of the newly synchronized checkpoint so that it can help other peers synchronize.

use anemo::{types::PeerEvent, PeerId, Request, Response, Result};

use futures::{FutureExt, StreamExt};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, SystemTime},
};
use sui_config::p2p::StateSyncConfig;
use sui_types::messages_checkpoint::CertifiedCheckpointSummary;
use sui_types::{
    digests::CheckpointDigest,
    message_envelope::Message,
    messages::{CertifiedTransaction, TransactionEffects},
    messages_checkpoint::{
        CertifiedCheckpointSummary as Checkpoint, CheckpointContents, CheckpointSequenceNumber,
        VerifiedCheckpoint,
    },
    storage::ReadStore,
    storage::WriteStore,
};
use tap::{Pipe, TapFallible, TapOptional};
use tokio::{
    sync::{broadcast, mpsc},
    task::{AbortHandle, JoinSet},
};
use tracing::{debug, info, trace, warn};

mod generated {
    include!(concat!(env!("OUT_DIR"), "/sui.StateSync.rs"));
}
mod builder;
mod metrics;
mod server;
pub mod test_utils;
#[cfg(test)]
mod tests;

pub use builder::{Builder, UnstartedStateSync};
pub use generated::{
    state_sync_client::StateSyncClient,
    state_sync_server::{StateSync, StateSyncServer},
};
pub use server::GetCheckpointSummaryRequest;

use self::metrics::Metrics;

/// A handle to the StateSync subsystem.
///
/// This handle can be cloned and shared. Once all copies of a StateSync system's Handle have been
/// dropped, the StateSync system will be gracefully shutdown.
#[derive(Clone, Debug)]
pub struct Handle {
    sender: mpsc::Sender<StateSyncMessage>,
    checkpoint_event_sender: broadcast::Sender<VerifiedCheckpoint>,
}

impl Handle {
    /// Send a newly minted checkpoint from Consensus to StateSync so that it can be disseminated
    /// to other nodes on the network.
    ///
    /// # Invariant
    ///
    /// Consensus must only notify StateSync of new checkpoints that have been fully committed to
    /// persistent storage. This includes CheckpointContents and all Transactions and
    /// TransactionEffects included therein.
    pub async fn send_checkpoint(&self, checkpoint: VerifiedCheckpoint) {
        self.sender
            .send(StateSyncMessage::VerifiedCheckpoint(Box::new(checkpoint)))
            .await
            .unwrap()
    }

    /// Subscribe to the stream of checkpoints that have been fully synchronized and downloaded.
    pub fn subscribe_to_synced_checkpoints(&self) -> broadcast::Receiver<VerifiedCheckpoint> {
        self.checkpoint_event_sender.subscribe()
    }
}

struct PeerHeights {
    /// Table used to track the highest checkpoint for each of our peers.
    peers: HashMap<PeerId, PeerStateSyncInfo>,
    unprocessed_checkpoints: HashMap<CheckpointDigest, Checkpoint>,
    sequence_number_to_digest: HashMap<CheckpointSequenceNumber, CheckpointDigest>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct PeerStateSyncInfo {
    /// The digest of the Peer's genesis checkpoint.
    genesis_checkpoint_digest: CheckpointDigest,
    /// Indicates if this Peer is on the same chain as us.
    on_same_chain_as_us: bool,
    /// Highest checkpoint sequence number we know of for this Peer.
    height: CheckpointSequenceNumber,
}

impl PeerHeights {
    pub fn highest_known_checkpoint(&self) -> Option<&Checkpoint> {
        self.highest_known_checkpoint_sequence_number()
            .and_then(|s| self.sequence_number_to_digest.get(&s))
            .and_then(|digest| self.unprocessed_checkpoints.get(digest))
    }

    pub fn highest_known_checkpoint_sequence_number(&self) -> Option<CheckpointSequenceNumber> {
        self.peers
            .values()
            .filter_map(|info| info.on_same_chain_as_us.then_some(info.height))
            .max()
    }

    pub fn peers_on_same_chain(&self) -> impl Iterator<Item = (&PeerId, &PeerStateSyncInfo)> {
        self.peers
            .iter()
            .filter(|(_peer_id, info)| info.on_same_chain_as_us)
    }

    // Returns a bool that indicates if the update was done successfully.
    //
    // This will return false if the given peer doesn't have an entry or is not on the same chain
    // as us
    pub fn update_peer_info(&mut self, peer_id: PeerId, checkpoint: Checkpoint) -> bool {
        let info = match self.peers.get_mut(&peer_id) {
            Some(info) if info.on_same_chain_as_us => info,
            _ => return false,
        };

        info.height = std::cmp::max(checkpoint.sequence_number(), info.height);
        self.insert_checkpoint(checkpoint);

        true
    }

    pub fn insert_peer_info(&mut self, peer_id: PeerId, info: PeerStateSyncInfo) {
        use std::collections::hash_map::Entry;

        match self.peers.entry(peer_id) {
            Entry::Occupied(mut entry) => {
                // If there's already an entry and the genesis checkpoint digests match then update
                // the maximum height. Otherwise we'll use the more recent one
                let entry = entry.get_mut();
                if entry.genesis_checkpoint_digest == info.genesis_checkpoint_digest {
                    entry.height = std::cmp::max(entry.height, info.height);
                } else {
                    *entry = info;
                }
            }
            Entry::Vacant(entry) => {
                entry.insert(info);
            }
        }
    }

    pub fn mark_peer_as_not_on_same_chain(&mut self, peer_id: PeerId) {
        if let Some(info) = self.peers.get_mut(&peer_id) {
            info.on_same_chain_as_us = false;
        }
    }

    pub fn cleanup_old_checkpoints(&mut self, sequence_number: CheckpointSequenceNumber) {
        self.unprocessed_checkpoints
            .retain(|_digest, checkpoint| checkpoint.sequence_number() > sequence_number);
        self.sequence_number_to_digest
            .retain(|&s, _digest| s > sequence_number);
    }

    pub fn insert_checkpoint(&mut self, checkpoint: Checkpoint) {
        let digest = checkpoint.digest();
        let sequence_number = checkpoint.sequence_number();
        self.unprocessed_checkpoints.insert(digest, checkpoint);
        self.sequence_number_to_digest
            .insert(sequence_number, digest);
    }

    pub fn remove_checkpoint(&mut self, digest: &CheckpointDigest) {
        if let Some(checkpoint) = self.unprocessed_checkpoints.remove(digest) {
            self.sequence_number_to_digest
                .remove(&checkpoint.sequence_number());
        }
    }

    /*

    pub fn get_checkpoint_by_sequence_number(
        &self,
        sequence_number: CheckpointSequenceNumber,
    ) -> Option<&Checkpoint> {
        self.sequence_number_to_digest
            .get(&sequence_number)
            .and_then(|digest| self.get_checkpoint_by_digest(digest))
    }

    pub fn get_checkpoint_by_digest(&self, digest: &CheckpointDigest) -> Option<&Checkpoint> {
        self.unprocessed_checkpoints.get(digest)
    }

    */
}

#[derive(Clone, Debug)]
enum StateSyncMessage {
    StartSyncJob,
    // Validators will send this to the StateSyncEventLoop in order to kick off notifying our peers
    // of the new checkpoint.
    VerifiedCheckpoint(Box<VerifiedCheckpoint>),
    // Notification that the checkpoint content sync task will send to the event loop in the event
    // it was able to successfully sync a checkpoint's contents. If multiple checkpoints were
    // synced at the same time, only the highest checkpoint is sent.
    SyncedCheckpoint(Box<VerifiedCheckpoint>),
}

struct StateSyncEventLoop<S> {
    config: StateSyncConfig,

    mailbox: mpsc::Receiver<StateSyncMessage>,
    /// Weak reference to our own mailbox
    weak_sender: mpsc::WeakSender<StateSyncMessage>,

    tasks: JoinSet<()>,
    sync_checkpoint_summaries_task: Option<AbortHandle>,
    sync_checkpoint_contents_task: Option<AbortHandle>,

    store: S,
    peer_heights: Arc<RwLock<PeerHeights>>,
    checkpoint_event_sender: broadcast::Sender<VerifiedCheckpoint>,
    network: anemo::Network,
    metrics: Metrics,
}

impl<S> StateSyncEventLoop<S>
where
    S: WriteStore + Clone + Send + Sync + 'static,
    <S as ReadStore>::Error: std::error::Error,
{
    // Note: A great deal of care is taken to ensure that all event handlers are non-asynchronous
    // and that the only "await" points are from the select macro picking which event to handle.
    // This ensures that the event loop is able to process events at a high speed and reduce the
    // chance for building up a backlog of events to process.
    pub async fn start(mut self) {
        info!("State-Synchronizer started");

        let mut interval = tokio::time::interval(self.config.interval_period());
        let mut peer_events = {
            let (subscriber, peers) = self.network.subscribe().unwrap();
            for peer_id in peers {
                self.spawn_get_latest_from_peer(peer_id);
            }
            subscriber
        };

        // Initialize checkpoint watermark metrics
        self.metrics.set_highest_verified_checkpoint(
            self.store
                .get_highest_verified_checkpoint()
                .expect("store operation should not fail")
                .sequence_number(),
        );
        self.metrics.set_highest_synced_checkpoint(
            self.store
                .get_highest_synced_checkpoint()
                .expect("store operation should not fail")
                .sequence_number(),
        );

        loop {
            tokio::select! {
                now = interval.tick() => {
                    self.handle_tick(now.into_std());
                },
                maybe_message = self.mailbox.recv() => {
                    // Once all handles to our mailbox have been dropped this
                    // will yield `None` and we can terminate the event loop
                    if let Some(message) = maybe_message {
                        self.handle_message(message);
                    } else {
                        break;
                    }
                },
                peer_event = peer_events.recv() => {
                    self.handle_peer_event(peer_event);
                },
                Some(task_result) = self.tasks.join_next() => {
                    task_result.unwrap();

                    if matches!(&self.sync_checkpoint_contents_task, Some(t) if t.is_finished()) {
                        self.sync_checkpoint_contents_task = None;
                    }

                    if matches!(&self.sync_checkpoint_summaries_task, Some(t) if t.is_finished()) {
                        self.sync_checkpoint_summaries_task = None;
                    }
                },
            }

            self.maybe_start_checkpoint_summary_sync_task();
            // self.maybe_start_checkpoint_contents_sync_task();
        }

        info!("State-Synchronizer ended");
    }

    fn handle_message(&mut self, message: StateSyncMessage) {
        debug!("Received message: {:?}", message);
        match message {
            StateSyncMessage::StartSyncJob => self.maybe_start_checkpoint_summary_sync_task(),
            StateSyncMessage::VerifiedCheckpoint(checkpoint) => {
                self.handle_checkpoint_from_consensus(checkpoint)
            }
            // After we've successfully synced a checkpoint we can notify our peers
            StateSyncMessage::SyncedCheckpoint(checkpoint) => {
                self.spawn_notify_peers_of_checkpoint(*checkpoint)
            }
        }
    }

    // Handle a checkpoint that we received from consensus
    fn handle_checkpoint_from_consensus(&mut self, checkpoint: Box<VerifiedCheckpoint>) {
        let (next_sequence_number, previous_digest) = {
            let latest_checkpoint = self
                .store
                .get_highest_verified_checkpoint()
                .expect("store operation should not fail");

            // If this is an older checkpoint, just ignore it
            if latest_checkpoint.sequence_number() >= checkpoint.sequence_number() {
                return;
            }

            let next_sequence_number = latest_checkpoint.sequence_number().saturating_add(1);
            let previous_digest = latest_checkpoint.digest();
            (next_sequence_number, previous_digest)
        };

        // If this is exactly the next checkpoint then insert it and then notify our peers
        if checkpoint.sequence_number() == next_sequence_number
            && checkpoint.previous_digest() == Some(previous_digest)
        {
            let checkpoint = *checkpoint;

            // Check invariant that consensus must only send state-sync fully synced checkpoints
            #[cfg(debug_assertions)]
            {
                let contents = self
                    .store
                    .get_checkpoint_contents(&checkpoint.content_digest())
                    .expect("store operation should not fail")
                    .unwrap();
                for digests in contents.into_inner() {
                    debug_assert!(self
                        .store
                        .get_transaction(&digests.transaction)
                        .expect("store operation should not fail")
                        .is_some());
                    debug_assert!(self
                        .store
                        .get_transaction_effects(&digests.effects)
                        .expect("store operation should not fail")
                        .is_some());
                }
            }

            self.store
                .insert_checkpoint(checkpoint.clone())
                .expect("store operation should not fail");
            self.store
                .update_highest_synced_checkpoint(&checkpoint)
                .expect("store operation should not fail");
            self.metrics
                .set_highest_verified_checkpoint(checkpoint.sequence_number());
            self.metrics
                .set_highest_synced_checkpoint(checkpoint.sequence_number());

            // We don't care if no one is listening as this is a broadcast channel
            let _ = self.checkpoint_event_sender.send(checkpoint.clone());

            self.spawn_notify_peers_of_checkpoint(checkpoint);
        } else {
            // Ensure that if consensus sends us a checkpoint that we expect to be the next one,
            // that it isn't on a fork
            if checkpoint.sequence_number() == next_sequence_number {
                assert_eq!(checkpoint.previous_digest(), Some(previous_digest));
            }

            debug!("consensus sent too new of a checkpoint");

            // See if the missing checkpoints are already in our store and quickly update our
            // watermarks
            let mut checkpoints_from_storage =
                (next_sequence_number..=checkpoint.sequence_number()).map(|n| {
                    self.store
                        .get_checkpoint_by_sequence_number(n)
                        .expect("store operation should not fail")
                });
            while let Some(Some(checkpoint)) = checkpoints_from_storage.next() {
                self.store
                    .insert_checkpoint(checkpoint.clone())
                    .expect("store operation should not fail");
                self.store
                    .update_highest_synced_checkpoint(&checkpoint)
                    .expect("store operation should not fail");
                self.metrics
                    .set_highest_verified_checkpoint(checkpoint.sequence_number());
                self.metrics
                    .set_highest_synced_checkpoint(checkpoint.sequence_number());

                // We don't care if no one is listening as this is a broadcast channel
                let _ = self.checkpoint_event_sender.send(checkpoint.clone());
            }
        }
    }

    fn handle_peer_event(
        &mut self,
        peer_event: Result<PeerEvent, tokio::sync::broadcast::error::RecvError>,
    ) {
        use tokio::sync::broadcast::error::RecvError;

        match peer_event {
            Ok(PeerEvent::NewPeer(peer_id)) => {
                self.spawn_get_latest_from_peer(peer_id);
            }
            Ok(PeerEvent::LostPeer(peer_id, _)) => {
                self.peer_heights.write().unwrap().peers.remove(&peer_id);
            }

            Err(RecvError::Closed) => {
                panic!("PeerEvent channel shouldn't be able to be closed");
            }

            Err(RecvError::Lagged(_)) => {
                trace!("State-Sync fell behind processing PeerEvents");
            }
        }
    }

    fn spawn_get_latest_from_peer(&mut self, peer_id: PeerId) {
        if let Some(peer) = self.network.peer(peer_id) {
            let genesis_checkpoint_digest = self
                .store
                .get_checkpoint_by_sequence_number(0)
                .expect("store operation should not fail")
                .expect("store should contain genesis checkpoint")
                .digest();
            let task = get_latest_from_peer(
                genesis_checkpoint_digest,
                peer,
                self.peer_heights.clone(),
                self.config.timeout(),
            );
            self.tasks.spawn(task);
        }
    }

    fn handle_tick(&mut self, _now: std::time::Instant) {
        let task = query_peers_for_their_latest_checkpoint(
            self.network.clone(),
            self.peer_heights.clone(),
            self.weak_sender.clone(),
            self.config.timeout(),
        );
        self.tasks.spawn(task);
    }

    fn maybe_start_checkpoint_summary_sync_task(&mut self) {
        // Only run one sync task at a time
        if self.sync_checkpoint_summaries_task.is_some() {
            return;
        }

        let highest_synced_checkpoint = self
            .store
            .get_highest_synced_checkpoint()
            .expect("store operation should not fail");

        let highest_known_checkpoint = self
            .peer_heights
            .read()
            .unwrap()
            .highest_known_checkpoint()
            .cloned();

        if Some(highest_synced_checkpoint.sequence_number())
            < highest_known_checkpoint
                .as_ref()
                .map(|x| x.sequence_number())
        {
            // start sync job
            let task = sync_to_checkpoint(
                self.network.clone(),
                self.store.clone(),
                self.peer_heights.clone(),
                self.metrics.clone(),
                self.config.checkpoint_header_download_concurrency(),
                self.config.transaction_download_concurrency(),
                self.checkpoint_event_sender.clone(),
                self.config.timeout(),
                // The if condition should ensure that this is Some
                highest_known_checkpoint.unwrap(),
                self.weak_sender.clone(),
            )
            .map(|result| match result {
                Ok(()) => {}
                Err(e) => {
                    debug!("error syncing checkpoint {e}");
                }
            });
            let task_handle = self.tasks.spawn(task);
            self.sync_checkpoint_summaries_task = Some(task_handle);
        }
    }

    fn spawn_notify_peers_of_checkpoint(&mut self, checkpoint: VerifiedCheckpoint) {
        let task = notify_peers_of_checkpoint(
            self.network.clone(),
            self.peer_heights.clone(),
            checkpoint,
            self.config.timeout(),
        );
        self.tasks.spawn(task);
    }
}

async fn notify_peers_of_checkpoint(
    network: anemo::Network,
    peer_heights: Arc<RwLock<PeerHeights>>,
    checkpoint: VerifiedCheckpoint,
    timeout: Duration,
) {
    let futs = peer_heights
        .read()
        .unwrap()
        .peers_on_same_chain()
        // Filter out any peers who we know already have a checkpoint higher than this one
        .filter_map(|(peer_id, info)| {
            (checkpoint.sequence_number() > info.height).then_some(peer_id)
        })
        // Filter out any peers who we aren't connected with
        .flat_map(|peer_id| network.peer(*peer_id))
        .map(StateSyncClient::new)
        .map(|mut client| {
            let request = Request::new(checkpoint.inner().clone()).with_timeout(timeout);
            async move { client.push_checkpoint_summary(request).await }
        })
        .collect::<Vec<_>>();
    futures::future::join_all(futs).await;
}

async fn get_latest_from_peer(
    our_genesis_checkpoint_digest: CheckpointDigest,
    peer: anemo::Peer,
    peer_heights: Arc<RwLock<PeerHeights>>,
    timeout: Duration,
) {
    let peer_id = peer.peer_id();
    let mut client = StateSyncClient::new(peer);

    let info = {
        let maybe_info = peer_heights.read().unwrap().peers.get(&peer_id).copied();

        if let Some(info) = maybe_info {
            info
        } else {
            // TODO do we want to create a new API just for querying a node's chainid?
            //
            // We need to query this node's genesis checkpoint to see if they're on the same chain
            // as us
            let request = Request::new(GetCheckpointSummaryRequest::BySequenceNumber(0))
                .with_timeout(timeout);
            let response = client
                .get_checkpoint_summary(request)
                .await
                .map(Response::into_inner);

            let info = match response {
                Ok(Some(checkpoint)) => {
                    let digest = checkpoint.digest();
                    PeerStateSyncInfo {
                        genesis_checkpoint_digest: digest,
                        on_same_chain_as_us: our_genesis_checkpoint_digest == digest,
                        height: checkpoint.sequence_number(),
                    }
                }
                Ok(None) => PeerStateSyncInfo {
                    genesis_checkpoint_digest: CheckpointDigest::default(),
                    on_same_chain_as_us: false,
                    height: CheckpointSequenceNumber::default(),
                },
                Err(status) => {
                    trace!("get_latest_checkpoint_summary request failed: {status:?}");
                    return;
                }
            };
            peer_heights
                .write()
                .unwrap()
                .insert_peer_info(peer_id, info);
            info
        }
    };

    // Bail early if this node isn't on the same chain as us
    if !info.on_same_chain_as_us {
        return;
    }

    let checkpoint = {
        let request = Request::new(GetCheckpointSummaryRequest::Latest).with_timeout(timeout);
        let response = client
            .get_checkpoint_summary(request)
            .await
            .map(Response::into_inner);
        match response {
            Ok(Some(checkpoint)) => checkpoint,
            Ok(None) => return,
            Err(status) => {
                trace!("get_latest_checkpoint_summary request failed: {status:?}");
                return;
            }
        }
    };

    peer_heights
        .write()
        .unwrap()
        .update_peer_info(peer_id, checkpoint);
}

async fn query_peers_for_their_latest_checkpoint(
    network: anemo::Network,
    peer_heights: Arc<RwLock<PeerHeights>>,
    sender: mpsc::WeakSender<StateSyncMessage>,
    timeout: Duration,
) {
    let peer_heights = &peer_heights;
    let futs = peer_heights
        .read()
        .unwrap()
        .peers_on_same_chain()
        // Filter out any peers who we aren't connected with
        .flat_map(|(peer_id, _info)| network.peer(*peer_id))
        .map(|peer| {
            let peer_id = peer.peer_id();
            let mut client = StateSyncClient::new(peer);

            async move {
                let request =
                    Request::new(GetCheckpointSummaryRequest::Latest).with_timeout(timeout);
                let response = client
                    .get_checkpoint_summary(request)
                    .await
                    .map(Response::into_inner);
                match response {
                    Ok(Some(checkpoint)) => peer_heights
                        .write()
                        .unwrap()
                        .update_peer_info(peer_id, checkpoint.clone())
                        .then_some(checkpoint),
                    Ok(None) => None,
                    Err(status) => {
                        trace!("get_latest_checkpoint_summary request failed: {status:?}");
                        None
                    }
                }
            }
        })
        .collect::<Vec<_>>();

    let checkpoints = futures::future::join_all(futs).await.into_iter().flatten();

    let highest_checkpoint = checkpoints.max_by_key(|checkpoint| checkpoint.sequence_number());

    let our_highest_checkpoint = peer_heights
        .read()
        .unwrap()
        .highest_known_checkpoint()
        .cloned();

    let _new_checkpoint = match (highest_checkpoint, our_highest_checkpoint) {
        (Some(theirs), None) => theirs,
        (Some(theirs), Some(ours)) if theirs.sequence_number() > ours.sequence_number() => theirs,
        _ => return,
    };

    if let Some(sender) = sender.upgrade() {
        let _ = sender.send(StateSyncMessage::StartSyncJob).await;
    }
}

fn flag_bad_checkpoint_peer(
    maybe_peer_id: Option<PeerId>,
    checkpoint: &Checkpoint,
    peer_heights: Arc<RwLock<PeerHeights>>,
) -> Result<()> {
    let mut peer_heights = peer_heights.write().unwrap();
    // Remove the checkpoint from our temporary store so that we can try querying
    // another peer for a different one
    peer_heights.remove_checkpoint(&checkpoint.digest());

    // Mark peer as not on the same chain as us
    if let Some(peer_id) = maybe_peer_id {
        peer_heights.mark_peer_as_not_on_same_chain(peer_id);
    }

    warn!("Checkpoint failed verification (certificate).");
    Err(anyhow::anyhow!("unable to verify checkpoint {checkpoint}"))
}

async fn sync_to_checkpoint<S>(
    network: anemo::Network,
    store: S,
    peer_heights: Arc<RwLock<PeerHeights>>,
    metrics: Metrics,
    checkpoint_header_download_concurrency: usize,
    transaction_download_concurrency: usize,
    checkpoint_event_sender: broadcast::Sender<VerifiedCheckpoint>,
    timeout: Duration,
    checkpoint: Checkpoint,
    sender: mpsc::WeakSender<StateSyncMessage>,
) -> Result<()>
where
    S: WriteStore,
    <S as ReadStore>::Error: std::error::Error,
{
    metrics.set_highest_known_checkpoint(checkpoint.sequence_number());

    let mut current = store
        .get_highest_synced_checkpoint()
        .expect("store operation should not fail");
    if current.sequence_number() >= checkpoint.sequence_number() {
        return Err(anyhow::anyhow!(
            "target checkpoint {} is older than highest verified checkpoint {}",
            checkpoint.sequence_number(),
            current.sequence_number(),
        ));
    }

    // Define an epoch within which we are doing sync. After that we stop this
    // sync and start again a sync for the next epoch. If the last verified checkpoint
    // is at an epoch boundary we are in the next epoch, otherwise we are in the same
    // epoch.
    let sync_epoch = current
        .next_epoch_committee()
        .map(|_| current.epoch() + 1)
        .unwrap_or_else(|| current.epoch());

    // get a list of peers that can help
    let peers = peer_heights
        .read()
        .unwrap()
        .peers_on_same_chain()
        // Filter out any peers who can't help
        .filter(|(_peer_id, info)| info.height > current.sequence_number())
        .map(|(&peer_id, &info)| (peer_id, info))
        .collect::<Vec<_>>();

    let request_stream = (current.sequence_number().saturating_add(1)
        ..=checkpoint.sequence_number())
        .map(|next| {
            download_full_checkpoint(
                next,
                peers.clone(),
                &network,
                transaction_download_concurrency,
                timeout,
            )
        })
        .pipe(futures::stream::iter)
        .buffered(checkpoint_header_download_concurrency);

    const MAX_CHECKPOINT_CHUNK: usize = 200; // This is an internal param, no need to expose as config
    let mut chucks_stream = request_stream
        .ready_chunks(checkpoint_header_download_concurrency.min(MAX_CHECKPOINT_CHUNK));

    while let Some(mut vec_checkpoints) = chucks_stream.next().await {
        let checkpoint_bundle: Vec<_> = vec_checkpoints
            .clone()
            .into_iter()
            .take_while(
                |item| {
                    item.0.is_some() // We stop at the first checkpoint we failed to download.
                && item.0.as_ref().unwrap().0.epoch() == sync_epoch
                }, // We also stop after the epoch change checkpoint
            )
            .map(|item| {
                let (opt_items, _next, _opt_peer) = item;
                let (checkpoint, contents, transaction_effects) = opt_items.unwrap(); // Safe due to take_while check

                // Change all transaction to be certified.
                let verified: Vec<_> = transaction_effects
                    .into_iter()
                    .map(|(transaction, effects)| {
                        (
                            // Optimistically set to verified.
                            sui_types::messages::VerifiedCertificate::new_unchecked(transaction),
                            effects,
                        )
                    })
                    .collect();
                (
                    VerifiedCheckpoint::new_unchecked(checkpoint),
                    contents,
                    verified,
                )
            })
            .collect();

        // Truncate to first failed download or end of epoch
        vec_checkpoints.truncate(checkpoint_bundle.len());

        // Check that the last item has a correct certificate
        if let Some((last_checkpoint, _, maybe_peer_id)) = vec_checkpoints.last() {
            let (checkpoint, _, _) = last_checkpoint.as_ref().unwrap(); // Safe due to last() above

            let Some(committee) = store
            .get_committee(checkpoint.epoch())
            .expect("store operation should not fail") else {
                warn!("Cannot find committee for epoch: {}", checkpoint.epoch());
                return Ok(())
            };

            // Check the signature / certificate on the last checkpoint
            if checkpoint.verify(&committee, None).is_err() {
                // The checkpoint was wrong so we flag the peer that was the source of it.
                debug!("Checkpoint failed verification (certificate verification).");
                return flag_bad_checkpoint_peer(*maybe_peer_id, checkpoint, peer_heights);
            }

            // Now check the hash chain backwards
            for index in (0..checkpoint_bundle.len() - 1).rev() {
                let prev_checkpoint_digest = checkpoint_bundle[index + 1].0.previous_digest();
                // Safe to unwrap here because we have authenticated higher checkpoint and it cannot be zero
                // so as a result the digest will exist.
                if checkpoint_bundle[index].0.digest() != prev_checkpoint_digest.unwrap() {
                    let checkpoint = checkpoint_bundle[index].0.inner();
                    let peer_id = vec_checkpoints[index].2;

                    warn!("Checkpoint failed verification (hash chain).");
                    return flag_bad_checkpoint_peer(peer_id, checkpoint, peer_heights);
                }
            }
        }

        let vec_size = vec_checkpoints.len();
        let mut verified_checkpoints = Vec::with_capacity(vec_size);
        for ((_, next, maybe_peer_id), (checkpoint, _, _)) in
            vec_checkpoints.iter().zip(checkpoint_bundle.iter())
        {
            if current.sequence_number().saturating_add(1) != *next {
                // We must have experienced some failures so lets restart
                return Ok(());
            }

            // Verify the checkpoint
            if verify_checkpoint_not_certificate(&current, checkpoint.inner()).is_err() {
                warn!("Checkpoint failed verification (other checks).");
                return flag_bad_checkpoint_peer(*maybe_peer_id, checkpoint.inner(), peer_heights);
            }

            debug!(sequence_number = ?checkpoint.summary.sequence_number, "verified checkpoint summary");
            SystemTime::now()
                .duration_since(checkpoint.summary.timestamp())
                .map(|latency| metrics.report_checkpoint_summary_age(latency))
                .tap_err(|err| warn!("unable to compute checkpoint age: {}", err))
                .ok();

            // We don't care if no one is listening as this is a broadcast channel
            current = checkpoint.clone();
            verified_checkpoints.push(checkpoint.clone());
        }

        store
            .insert_full_bundle(checkpoint_bundle)
            .expect("store operation should not fail");

        metrics.set_highest_verified_checkpoint(current.sequence_number());
        metrics.set_highest_synced_checkpoint(current.sequence_number());

        // Notify event loop to notify our peers that we've synced to a new checkpoint height
        if let Some(sender) = sender.upgrade() {
            let message = StateSyncMessage::SyncedCheckpoint(Box::new(current.clone()));
            let _ = sender.send(message).await;
        }

        // Send all checkpooints to the channel for execution notification
        verified_checkpoints
            .into_iter()
            .for_each(|verified_checkpoint| {
                let _ = checkpoint_event_sender.send(verified_checkpoint);
            });

        // Since we stop at the epoch checkpoint it is safe to check the last one
        // and return if it changes epoch, since we need the new epoch committee
        // to check the new epoch checkpoints. This is rare enough to not care.
        if current.summary().end_of_epoch_data.is_some() || vec_size == 0 {
            debug!("Sync interrupted possibly at epoch boundary.");
            return Ok(());
        }
    }

    peer_heights
        .write()
        .unwrap()
        .cleanup_old_checkpoints(current.sequence_number());

    Ok(())
}

fn download_full_checkpoint(
    next: u64,
    peers: Vec<(PeerId, PeerStateSyncInfo)>,
    network: &anemo::Network,
    transaction_download_concurrency: usize,
    timeout: Duration,
) -> impl core::future::Future<
    Output = (
        Option<(
            CertifiedCheckpointSummary,
            CheckpointContents,
            Vec<(CertifiedTransaction, TransactionEffects)>,
        )>,
        u64,
        Option<PeerId>,
    ),
> {
    let mut peers = peers
        .iter()
        // Filter out any peers who can't help with this particular checkpoint
        .filter(|(_peer_id, info)| info.height >= next)
        // Filter out any peers who we aren't connected with
        .flat_map(|(peer_id, _height)| network.peer(*peer_id).map(|c| (*peer_id, c)))
        .map(|(peer_id, client)| (peer_id, StateSyncClient::new(client)))
        .collect::<Vec<_>>();

    let mut rng = <rand::rngs::StdRng as rand::SeedableRng>::from_entropy();
    rand::seq::SliceRandom::shuffle(peers.as_mut_slice(), &mut rng);

    async move {
        let now = tokio::time::Instant::now();

        // Iterate through our selected peers trying each one in turn until we're able to
        // successfully get the target checkpoint
        // info!("Num peers: {}", peers.len());
        'outer_peer_loop: for (peer_id, mut peer) in peers {
            let request = Request::new(GetCheckpointSummaryRequest::BySequenceNumber(next))
                .with_timeout(timeout);

            if let Some(checkpoint) = peer
                .get_checkpoint_summary(request)
                .await
                .tap_err(|e| debug!("{e:?}"))
                .ok()
                .and_then(Response::into_inner)
                .tap_none(|| debug!("Peer unable to help sync checkpoint {}", peer_id))
            {
                // peer didn't give us a checkpoint with the height that we requested
                if checkpoint.sequence_number() != next {
                    continue 'outer_peer_loop;
                }

                // Now get the contents structure
                let request = Request::new(checkpoint.content_digest()).with_timeout(timeout);
                let contents = if let Some(contents) = peer
                    .get_checkpoint_contents(request)
                    .await
                    .tap_err(|e| debug!("{e:?}"))
                    .ok()
                    .and_then(Response::into_inner)
                    .tap_none(|| debug!("Peer unable to help sync contents {}", peer_id))
                {
                    if checkpoint.content_digest() != contents.digest() {
                        continue 'outer_peer_loop;
                    }

                    contents
                } else {
                    continue 'outer_peer_loop;
                };

                let num_txns = contents.size() as u64;

                // Sync transactions and effects
                let mut stream = contents
                    .iter()
                    .cloned()
                    .into_iter()
                    .map(|digests| {
                        let mut peer = peer.clone();
                        async move {
                            // Ask for all requests
                            let request = Request::new(digests).with_timeout(timeout);
                            if let Some((transaction, effects)) = peer
                                .get_transaction_and_effects(request)
                                .await
                                .tap_err(|e| debug!("{e:?}"))
                                .ok()
                                .and_then(Response::into_inner)
                                .tap_none(|| {
                                    debug!("Peer unable to help sync transaction {}", peer_id)
                                })
                            {
                                if !(transaction.digest() == &digests.transaction
                                    && effects.digest() == digests.effects
                                    && effects.transaction_digest == digests.transaction)
                                {
                                    return None;
                                }

                                Some((transaction, effects))
                            } else {
                                None
                            }
                        }
                    })
                    .pipe(futures::stream::iter)
                    .buffer_unordered(transaction_download_concurrency);

                let mut transaction_effects = Vec::with_capacity(num_txns as usize);
                while let Some(result) = stream.next().await {
                    if let Some((transaction, effects)) = result {
                        transaction_effects.push((transaction, effects));
                    } else {
                        continue 'outer_peer_loop;
                    }
                }

                // Let go of the &stream reference
                drop(stream);

                trace!(
                    "Full Sync: {} L: {}\t{}ms",
                    checkpoint.sequence_number(),
                    num_txns,
                    now.elapsed().as_millis()
                );

                return (
                    Some((checkpoint, contents, transaction_effects)),
                    next,
                    Some(peer.inner().peer_id()),
                );
            }
        }

        debug!(
            "Failed to download checkpoint despite trying all peers: {}",
            next
        );
        (None, next, None)
    }
}

fn verify_checkpoint_not_certificate(
    current: &VerifiedCheckpoint,
    checkpoint: &Checkpoint,
) -> Result<(), ()> {
    assert_eq!(
        checkpoint.sequence_number(),
        current.sequence_number().saturating_add(1)
    );

    if Some(current.digest()) != checkpoint.previous_digest() {
        debug!(
            current_sequence_number = current.sequence_number(),
            current_digest =% current.digest(),
            checkpoint_sequence_number = checkpoint.sequence_number(),
            checkpoint_digest =% checkpoint.digest(),
            checkpoint_previous_digest =? checkpoint.previous_digest(),
            "checkpoint not on same chain"
        );
        return Err(());
    }

    let current_epoch = current.epoch();
    if checkpoint.epoch() != current_epoch && checkpoint.epoch() != current_epoch.saturating_add(1)
    {
        debug!(
            current_epoch = current_epoch,
            checkpoint_epoch = checkpoint.epoch(),
            "cannont verify checkpoint with too high of an epoch",
        );
        return Err(());
    }

    if checkpoint.epoch() == current_epoch.saturating_add(1)
        && current.next_epoch_committee().is_none()
    {
        debug!(
            "next checkpoint claims to be from the next epoch but the latest verified checkpoint does not indicate that it is the last checkpoint of an epoch"
        );
        return Err(());
    }

    Ok(())
}
