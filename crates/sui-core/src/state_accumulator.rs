// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use mysten_metrics::monitored_scope;
use serde::Serialize;
use sui_types::base_types::{ObjectID, ObjectRef, SequenceNumber};
use sui_types::committee::EpochId;
use sui_types::digests::ObjectDigest;
use sui_types::storage::ObjectKey;
use tracing::debug;
use typed_store::Map;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use fastcrypto::hash::MultisetHash;
use sui_types::accumulator::Accumulator;
use sui_types::error::SuiResult;
use sui_types::messages::{TransactionEffects, TransactionEffectsAPI};
use sui_types::messages_checkpoint::{CheckpointSequenceNumber, ECMHLiveObjectSetDigest};
use typed_store::rocks::TypedStoreError;

use crate::authority::authority_per_epoch_store::AuthorityPerEpochStore;
use crate::authority::AuthorityStore;

pub struct StateAccumulator {
    authority_store: Arc<AuthorityStore>,
}

/// Serializable representation of the ObjectRef of an
/// object that has been wrapped
#[derive(Serialize)]
struct WrappedObject {
    id: ObjectID,
    wrapped_at: SequenceNumber,
    digest: ObjectDigest,
}

impl WrappedObject {
    fn new(id: ObjectID, wrapped_at: SequenceNumber) -> Self {
        Self {
            id,
            wrapped_at,
            digest: ObjectDigest::OBJECT_DIGEST_WRAPPED,
        }
    }
}

impl StateAccumulator {
    pub fn new(authority_store: Arc<AuthorityStore>) -> Self {
        Self { authority_store }
    }

    /// Accumulates the effects of a single checkpoint and persists the accumulator.
    /// This function is idempotent.
    pub fn accumulate_checkpoint(
        &self,
        effects: Vec<TransactionEffects>,
        checkpoint_seq_num: CheckpointSequenceNumber,
        epoch_store: Arc<AuthorityPerEpochStore>,
    ) -> SuiResult<Accumulator> {
        let _scope = monitored_scope("AccumulateCheckpoint");
        if let Some(acc) = epoch_store.get_state_hash_for_checkpoint(&checkpoint_seq_num)? {
            return Ok(acc);
        }

        let acc = self.accumulate_effects(effects);

        epoch_store.insert_state_hash_for_checkpoint(&checkpoint_seq_num, &acc)?;
        debug!("Accumulated checkpoint {}", checkpoint_seq_num);

        epoch_store
            .checkpoint_state_notify_read
            .notify(&checkpoint_seq_num, &acc);

        Ok(acc)
    }

    /// Accumulates given effects and returns the accumulator without side effects.
    pub fn accumulate_effects(&self, effects: Vec<TransactionEffects>) -> Accumulator {
        // prefetch all objects that may need to be looked up

        // get all unwrapped ObjectRefs
        let all_unwrapped = effects
            .iter()
            .flat_map(|fx| {
                fx.unwrapped()
                    .iter()
                    .map(|(oref, _owner)| *oref)
                    .chain(fx.unwrapped_then_deleted().iter().cloned())
                    .collect::<Vec<ObjectRef>>()
            })
            .collect::<Vec<ObjectRef>>();

        let all_wrapped: Vec<ObjectRef> = all_unwrapped
            .iter()
            .filter_map(|(id, seq_num, _digest)| {
                self.authority_store
                    .get_object_ref_prior_to_key(id, *seq_num)
                    .expect("read cannot fail")
            })
            .collect();

        let modified_at_version_keys: Vec<ObjectKey> = effects
            .iter()
            .flat_map(|fx| {
                fx.modified_at_versions()
                    .iter()
                    .map(|(id, seq_num)| ObjectKey(*id, *seq_num))
            })
            .collect();

        let all_modified_at: HashMap<ObjectKey, ObjectRef> = self
            .authority_store
            .multi_get_object_by_key(&modified_at_version_keys.clone())
            .expect("Failed to get modified_at_versions object from object table")
            .into_iter()
            .zip(modified_at_version_keys)
            // it's safe to filter missing objects here, as they could be unwrapped objects
            // that did not exist previously. If they should be there, but are not, they will
            // be caught in accumulate_per_tx_effects.
            .filter_map(|(obj, key)| obj.map(|obj| (key, obj.compute_object_reference())))
            .collect();

        let lookup_cache: HashMap<ObjectKey, ObjectRef> = all_wrapped
            .iter()
            .map(|oref| (ObjectKey(oref.0, oref.1), *oref))
            .chain(all_modified_at)
            .collect();

        let effects_accumulators: Vec<Accumulator> = effects
            .into_iter()
            .map(|fx| self.accumulate_per_tx_effects(fx, &lookup_cache))
            .collect();

        let mut accumulator = Accumulator::default();
        for acc in effects_accumulators {
            accumulator.union(&acc);
        }

        accumulator
    }

    /// Separately accumulates the effects of a single transaction. This is used to
    /// ensure that we are correctly considering incremental tx side effects, where
    /// they might otherwise interfere across transactions and lead to bugs.
    fn accumulate_per_tx_effects(
        &self,
        effects: TransactionEffects,
        lookup_cache: &HashMap<ObjectKey, ObjectRef>,
    ) -> Accumulator {
        let mut acc = Accumulator::default();

        // process insertions to the set
        acc.insert_all(
            effects
                .created()
                .iter()
                .map(|(oref, _)| oref.2)
                .chain(effects.unwrapped().iter().map(|(oref, _)| oref.2))
                .chain(effects.mutated().iter().map(|(oref, _)| oref.2))
                .collect::<Vec<ObjectDigest>>(),
        );

        // insert wrapped tombstones. We use a custom struct in order to contain the tombstone
        // against the object id and sequence number, as the tombstone by itself is not unique.
        acc.insert_all(
            effects
                .wrapped()
                .iter()
                .map(|oref| {
                    bcs::to_bytes(&WrappedObject::new(oref.0, oref.1))
                        .unwrap()
                        .to_vec()
                })
                .collect::<Vec<Vec<u8>>>(),
        );

        let all_unwrapped = effects
            .unwrapped()
            .iter()
            .map(|(oref, _owner)| (oref.0, oref.1))
            .chain(
                effects
                    .unwrapped_then_deleted()
                    .iter()
                    .map(|oref| (oref.0, oref.1)),
            )
            .collect::<Vec<(ObjectID, SequenceNumber)>>();

        let unwrapped_ids: HashSet<ObjectID> =
            all_unwrapped.iter().map(|(id, _)| id).cloned().collect();

        // Collect modified_at_versions to remove from the accumulator.
        // Filter all unwrapped objects (from unwrapped or unwrapped_then_deleted effects)
        // as these were inserted into the accumulator as a WrappedObject. Will handle these
        // separately.
        let modified_at_digests: Vec<ObjectDigest> = effects
            .modified_at_versions()
            .iter()
            .filter_map(|(id, seq_num)| {
                if unwrapped_ids.contains(id) {
                    None
                } else {
                    let digest = lookup_cache
                        .get(&ObjectKey(*id, *seq_num))
                        .unwrap_or_else(|| {
                            panic!(
                                "id {} seq_num {} expected to exist in accumulator lookup cache",
                                id, seq_num
                            )
                        })
                        .2;
                    Some(digest)
                }
            })
            .collect();

        acc.remove_all(modified_at_digests);

        // Process unwrapped and unwrapped_then_deleted effects, which need to be
        // removed as WrappedObject using the last sequence number it was tombstoned
        // against. Since this happened in a past transaction, and the child object may
        // have been modified since (and hence its sequence number incremented), we
        // seek the version prior to the unwrapped version from the objects table directly.
        // If the tombstone is not found, then assume this is a newly created wrapped object hence
        // we don't expect to find it in the table.
        let wrapped_objects_to_remove: Vec<WrappedObject> = all_unwrapped
            .iter()
            .filter_map(|(id, seq_num)| {
                let objref = lookup_cache.get(&ObjectKey(*id, *seq_num));

                objref.map(|(id, version, digest)| {
                    assert!(digest.is_wrapped(), "{:?}", id);
                    WrappedObject::new(*id, *version)
                })
            })
            .collect();

        acc.remove_all(
            wrapped_objects_to_remove
                .iter()
                .map(|wrapped| bcs::to_bytes(wrapped).unwrap().to_vec())
                .collect::<Vec<Vec<u8>>>(),
        );

        acc
    }

    /// Unions all checkpoint accumulators at the end of the epoch to generate the
    /// root state hash and persists it to db. This function is idempotent. Can be called on
    /// non-consecutive epochs, e.g. to accumulate epoch 3 after having last
    /// accumulated epoch 1.
    pub async fn accumulate_epoch(
        &self,
        epoch: &EpochId,
        last_checkpoint_of_epoch: CheckpointSequenceNumber,
        epoch_store: Arc<AuthorityPerEpochStore>,
    ) -> Result<Accumulator, TypedStoreError> {
        if let Some((_checkpoint, acc)) = self
            .authority_store
            .perpetual_tables
            .root_state_hash_by_epoch
            .get(epoch)?
        {
            return Ok(acc);
        }

        // Get the next checkpoint to accumulate (first checkpoint of the epoch)
        // by adding 1 to the highest checkpoint of the previous epoch
        let (_, (next_to_accumulate, mut root_state_hash)) = self
            .authority_store
            .perpetual_tables
            .root_state_hash_by_epoch
            .iter()
            .skip_to_last()
            .next()
            .map(|(epoch, (highest, hash))| {
                (
                    epoch,
                    (
                        highest.checked_add(1).expect("Overflowed u64 for epoch ID"),
                        hash,
                    ),
                )
            })
            .unwrap_or((0, (0, Accumulator::default())));

        debug!(
            "Accumulating epoch {} from checkpoint {} to checkpoint {} (inclusive)",
            epoch, next_to_accumulate, last_checkpoint_of_epoch
        );

        let (checkpoints, mut accumulators) = epoch_store
            .get_accumulators_in_checkpoint_range(next_to_accumulate, last_checkpoint_of_epoch)?
            .into_iter()
            .unzip::<_, _, Vec<_>, Vec<_>>();

        let remaining_checkpoints: Vec<_> = (next_to_accumulate..=last_checkpoint_of_epoch)
            .filter(|seq_num| !checkpoints.contains(seq_num))
            .collect();

        if !remaining_checkpoints.is_empty() {
            debug!(
                "Awaiting accumulation of checkpoints {:?} for epoch {} accumulation",
                remaining_checkpoints, epoch
            );
        }

        let mut remaining_accumulators = epoch_store
            .notify_read_checkpoint_state_digests(remaining_checkpoints)
            .await
            .expect("Failed to notify read checkpoint state digests");

        accumulators.append(&mut remaining_accumulators);

        assert!(accumulators.len() == (last_checkpoint_of_epoch - next_to_accumulate + 1) as usize);

        for acc in accumulators {
            root_state_hash.union(&acc);
        }

        self.authority_store
            .perpetual_tables
            .root_state_hash_by_epoch
            .insert(epoch, &(last_checkpoint_of_epoch, root_state_hash.clone()))?;

        self.authority_store
            .root_state_notify_read
            .notify(epoch, &(last_checkpoint_of_epoch, root_state_hash.clone()));

        Ok(root_state_hash)
    }

    /// Returns the result of accumulatng the live object set, without side effects
    pub fn accumulate_live_object_set(&self) -> Accumulator {
        let mut acc = Accumulator::default();
        for oref in self.authority_store.iter_live_object_set() {
            if oref.2 == ObjectDigest::OBJECT_DIGEST_WRAPPED {
                acc.insert(
                    bcs::to_bytes(&WrappedObject::new(oref.0, oref.1))
                        .expect("Failed to serialize WrappedObject"),
                );
            } else {
                acc.insert(oref.2);
            }
        }
        acc
    }

    pub fn digest_live_object_set(&self) -> ECMHLiveObjectSetDigest {
        let acc = self.accumulate_live_object_set();
        acc.digest().into()
    }

    pub async fn digest_epoch(
        &self,
        epoch: &EpochId,
        last_checkpoint_of_epoch: CheckpointSequenceNumber,
        epoch_store: Arc<AuthorityPerEpochStore>,
    ) -> Result<ECMHLiveObjectSetDigest, TypedStoreError> {
        Ok(self
            .accumulate_epoch(epoch, last_checkpoint_of_epoch, epoch_store)
            .await?
            .digest()
            .into())
    }
}
