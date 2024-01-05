// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::{authority_store::ObjectKey, *};
use rocksdb::Options;
use serde::{Deserialize, Serialize};
use std::path::Path;
use sui_storage::default_db_options;
use sui_types::base_types::{ExecutionDigests, SequenceNumber};
use sui_types::batch::{SignedBatch, TxSequenceNumber};
use sui_types::messages::TrustedCertificate;
use typed_store::rocks::{DBMap, DBOptions};
use typed_store::traits::TypedStoreDebug;

use typed_store_derive::DBMapUtils;

/// AuthorityPerpetualTables contains data that must be preserved from one epoch to the next.
#[derive(DBMapUtils)]
pub struct AuthorityPerpetualTables<S> {
    /// This is a map between the object (ID, version) and the latest state of the object, namely the
    /// state that is needed to process new transactions.
    ///
    /// Note that while this map can store all versions of an object, we will eventually
    /// prune old object versions from the db.
    ///
    /// IMPORTANT: object versions must *only* be pruned if they appear as inputs in some
    /// TransactionEffects. Simply pruning all objects but the most recent is an error!
    /// This is because there can be partially executed transactions whose effects have not yet
    /// been written out, and which must be retried. But, they cannot be retried unless their input
    /// objects are still accessible!
    #[default_options_override_fn = "objects_table_default_config"]
    pub(crate) objects: DBMap<ObjectKey, Object>,

    /// This is a an index of object references to currently existing objects, indexed by the
    /// composite key of the SuiAddress of their owner and the object ID of the object.
    /// This composite index allows an efficient iterator to list all objected currently owned
    /// by a specific user, and their object reference.
    pub(crate) owner_index: DBMap<(Owner, ObjectID), ObjectInfo>,

    /// This is a map between the transaction digest and the corresponding certificate for all
    /// certificates that have been successfully processed by this authority. This set of certificates
    /// along with the genesis allows the reconstruction of all other state, and a full sync to this
    /// authority.
    #[default_options_override_fn = "certificates_table_default_config"]
    pub(crate) certificates: DBMap<TransactionDigest, TrustedCertificate>,

    /// The map between the object ref of objects processed at all versions and the transaction
    /// digest of the certificate that lead to the creation of this version of the object.
    ///
    /// When an object is deleted we include an entry into this table for its next version and
    /// a digest of ObjectDigest::deleted(), along with a link to the transaction that deleted it.
    pub(crate) parent_sync: DBMap<ObjectRef, TransactionDigest>,

    /// A map between the transaction digest of a certificate that was successfully processed
    /// (ie in `certificates`) and the effects its execution has on the authority state. This
    /// structure is used to ensure we do not double process a certificate, and that we can return
    /// the same response for any call after the first (ie. make certificate processing idempotent).
    #[default_options_override_fn = "effects_table_default_config"]
    pub(crate) executed_effects: DBMap<TransactionDigest, TransactionEffectsEnvelope<S>>,

    pub(crate) effects: DBMap<TransactionEffectsDigest, TransactionEffects>,

    // Tables used for authority batch structure
    // TODO: executed_sequence and batches both conceptually belong in AuthorityEpochTables,
    // but we currently require that effects and executed_sequence are written atomically.
    // See https://github.com/MystenLabs/sui/pull/4395 for the reason why.
    //
    // This can be addressed when we do the WAL rework. Something similar to the following flow
    // would be required:
    // 1. First execute the tx and store the outputs in an intermediate location.
    // 2. Note that execution has finished (e.g. in the WAL.)
    // 3. Write intermediate outputs to their permanent locations.
    // 4. Mark the tx as finished in the WAL.
    // 5. Crucially: If step 3 is interrupted, we must restart at step 3 based solely on the fact
    //    that the WAL indicates the tx is not written yet. This fixes the root cause of the issue,
    //    which is that we currently exit early if effects have been written.
    /// A sequence on all executed certificates and effects.
    pub executed_sequence: DBMap<TxSequenceNumber, ExecutionDigests>,

    /// A sequence of batches indexing into the sequence of executed transactions.
    pub batches: DBMap<TxSequenceNumber, SignedBatch>,
}

impl<S> AuthorityPerpetualTables<S>
where
    S: std::fmt::Debug + Serialize + for<'de> Deserialize<'de>,
{
    pub fn path(parent_path: &Path) -> PathBuf {
        parent_path.join("perpetual")
    }

    pub fn open(parent_path: &Path, db_options: Option<Options>) -> Self {
        Self::open_tables_read_write(Self::path(parent_path), db_options, None)
    }

    pub fn open_readonly(parent_path: &Path) -> AuthorityPerpetualTablesReadOnly<S> {
        Self::get_read_only_handle(Self::path(parent_path), None, None)
    }

    /// Read an object and return it, or Err(ObjectNotFound) if the object was not found.
    pub fn get_object(&self, object_id: &ObjectID) -> Result<Option<Object>, SuiError> {
        let obj_entry = self
            .objects
            .iter()
            .skip_prior_to(&ObjectKey::max_for_id(object_id))?
            .next();

        let obj = match obj_entry {
            Some((ObjectKey(obj_id, _), obj)) if obj_id == *object_id => obj,
            _ => return Ok(None),
        };

        // Note that the two reads in this function are (obviously) not atomic, and the
        // object may be deleted after we have read it. Hence we check get_latest_parent_entry
        // last, so that the write to self.parent_sync gets the last word.
        //
        // TODO: verify this race is ok.
        //
        // I believe it is - Even if the reads were atomic, calls to this function would still
        // race with object deletions (the object could be deleted between when the function is
        // called and when the first read takes place, which would be indistinguishable to the
        // caller with the case in which the object is deleted in between the two reads).
        let parent_entry = self.get_latest_parent_entry(*object_id)?;

        match parent_entry {
            None => {
                error!(
                    ?object_id,
                    "Object is missing parent_sync entry, data store is inconsistent"
                );
                Ok(None)
            }
            Some((obj_ref, _)) if obj_ref.2.is_alive() => Ok(Some(obj)),
            _ => Ok(None),
        }
    }

    pub fn get_latest_parent_entry(
        &self,
        object_id: ObjectID,
    ) -> Result<Option<(ObjectRef, TransactionDigest)>, SuiError> {
        let mut iterator = self
            .parent_sync
            .iter()
            // Make the max possible entry for this object ID.
            .skip_prior_to(&(object_id, SequenceNumber::MAX, ObjectDigest::MAX))?;

        Ok(iterator.next().and_then(|(obj_ref, tx_digest)| {
            if obj_ref.0 == object_id {
                Some((obj_ref, tx_digest))
            } else {
                None
            }
        }))
    }

    pub fn get_sui_system_state_object(&self) -> SuiResult<SuiSystemState> {
        let sui_system_object = self
            .get_object(&SUI_SYSTEM_STATE_OBJECT_ID)?
            .expect("Sui System State object must always exist");
        let move_object = sui_system_object
            .data
            .try_as_move()
            .expect("Sui System State object must be a Move object");
        let result = bcs::from_bytes::<SuiSystemState>(move_object.contents())
            .expect("Sui System State object deserialization cannot fail");
        Ok(result)
    }

    pub fn get_epoch(&self) -> SuiResult<EpochId> {
        Ok(self.get_sui_system_state_object()?.epoch)
    }

    pub fn get_committee(&self) -> SuiResult<Committee> {
        Ok(self
            .get_sui_system_state_object()?
            .get_current_epoch_committee()
            .committee)
    }

    pub fn database_is_empty(&self) -> SuiResult<bool> {
        Ok(self
            .objects
            .iter()
            .skip_to(&ObjectKey::ZERO)?
            .next()
            .is_none())
    }
}

// These functions are used to initialize the DB tables
fn objects_table_default_config() -> DBOptions {
    default_db_options(None, None).1
}
fn certificates_table_default_config() -> DBOptions {
    default_db_options(None, None).1
}
fn effects_table_default_config() -> DBOptions {
    default_db_options(None, None).1
}
