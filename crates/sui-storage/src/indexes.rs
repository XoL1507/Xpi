// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! IndexStore supports creation of various ancillary indexes of state in SuiDataStore.
//! The main user of this data is the explorer.

use rocksdb::Options;
use serde::{de::DeserializeOwned, Serialize};

use crate::default_db_options;
use std::path::Path;
use sui_types::base_types::{ObjectID, SuiAddress, TransactionDigest};
use sui_types::batch::TxSequenceNumber;
use sui_types::error::SuiResult;

use sui_types::base_types::ObjectRef;
use sui_types::object::Owner;

use typed_store::rocks::DBMap;
use typed_store::{reopen, traits::Map};

pub struct IndexStore {
    /// Index from sui address to transactions initiated by that address.
    transactions_from_addr: DBMap<(SuiAddress, TxSequenceNumber), TransactionDigest>,

    /// Index from sui address to transactions that were sent to that address.
    transactions_to_addr: DBMap<(SuiAddress, TxSequenceNumber), TransactionDigest>,

    /// Index from object id to transactions that used that object id as input.
    transactions_by_input_object_id: DBMap<(ObjectID, TxSequenceNumber), TransactionDigest>,

    /// Index from object id to transactions that modified/created that object id.
    transactions_by_mutated_object_id: DBMap<(ObjectID, TxSequenceNumber), TransactionDigest>,

    /// This is a map between the transaction digest and its timestamp (UTC timestamp in
    /// **milliseconds** since epoch 1/1/1970). A transaction digest is subjectively time stamped
    /// on a node according to the local machine time, so it varies across nodes.
    /// The timestamping happens when the node sees a txn certificate for the first time.
    timestamps: DBMap<TransactionDigest, u64>,
}

impl IndexStore {
    pub fn open<P: AsRef<Path>>(path: P, db_options: Option<Options>) -> Self {
        let (options, point_lookup) = default_db_options(db_options, Some(1_000_000));

        let db = {
            let path = &path;
            let db_options = Some(options.clone());
            let opt_cfs: &[(&str, &rocksdb::Options)] = &[
                ("transactions_from_addr", &options),
                ("transactions_to_addr", &options),
                ("transactions_by_input_object_id", &options),
                ("transactions_by_mutated_object_id", &options),
                ("timestamps", &point_lookup),
            ];
            typed_store::rocks::open_cf_opts(path, db_options, opt_cfs)
        }
        .expect("Cannot open DB.");

        let (
            transactions_from_addr,
            transactions_to_addr,
            transactions_by_input_object_id,
            transactions_by_mutated_object_id,
            timestamps,
        ) = reopen!(
            &db,
            "transactions_from_addr"; <(SuiAddress, TxSequenceNumber), TransactionDigest>,
            "transactions_to_addr"; <(SuiAddress, TxSequenceNumber), TransactionDigest>,
            "transactions_by_input_object_id"; <(ObjectID, TxSequenceNumber), TransactionDigest>,
            "transactions_by_mutated_object_id"; <(ObjectID, TxSequenceNumber), TransactionDigest>,
            "timestamps";<TransactionDigest, u64>
        );

        Self {
            transactions_from_addr,
            transactions_to_addr,
            transactions_by_input_object_id,
            transactions_by_mutated_object_id,
            timestamps,
        }
    }

    pub fn index_tx<'a>(
        &self,
        sender: SuiAddress,
        active_inputs: impl Iterator<Item = ObjectID>,
        mutated_objects: impl Iterator<Item = &'a (ObjectRef, Owner)> + Clone,
        sequence: TxSequenceNumber,
        digest: &TransactionDigest,
        timestamp_ms: u64,
    ) -> SuiResult {
        let batch = self.transactions_from_addr.batch();

        let batch = batch.insert_batch(
            &self.transactions_from_addr,
            std::iter::once(((sender, sequence), *digest)),
        )?;

        let batch = batch.insert_batch(
            &self.transactions_by_input_object_id,
            active_inputs.map(|id| ((id, sequence), *digest)),
        )?;

        let batch = batch.insert_batch(
            &self.transactions_by_mutated_object_id,
            mutated_objects
                .clone()
                .map(|(obj_ref, _)| ((obj_ref.0, sequence), *digest)),
        )?;

        let batch = batch.insert_batch(
            &self.transactions_to_addr,
            mutated_objects.filter_map(|(_, owner)| {
                owner
                    .get_owner_address()
                    .ok()
                    .map(|addr| ((addr, sequence), digest))
            }),
        )?;

        let batch =
            batch.insert_batch(&self.timestamps, std::iter::once((*digest, timestamp_ms)))?;

        batch.write()?;

        Ok(())
    }

    /// Returns unix timestamp for a transaction if it exists
    pub fn get_timestamp_ms(
        &self,
        transaction_digest: &TransactionDigest,
    ) -> SuiResult<Option<u64>> {
        let ts = self.timestamps.get(transaction_digest)?;
        Ok(ts)
    }

    fn get_transactions_by_object<
        KeyT: Clone + Serialize + DeserializeOwned + std::cmp::PartialEq,
    >(
        index: &DBMap<(KeyT, TxSequenceNumber), TransactionDigest>,
        object_id: KeyT,
    ) -> SuiResult<Vec<(TxSequenceNumber, TransactionDigest)>> {
        Ok(index
            .iter()
            .skip_to(&(object_id.clone(), TxSequenceNumber::MIN))?
            .take_while(|((id, _), _)| *id == object_id)
            .map(|((_, seq), digest)| (seq, digest))
            .collect())
    }

    pub fn get_transactions_by_input_object(
        &self,
        input_object: ObjectID,
    ) -> SuiResult<Vec<(TxSequenceNumber, TransactionDigest)>> {
        Self::get_transactions_by_object(&self.transactions_by_input_object_id, input_object)
    }

    pub fn get_transactions_by_mutated_object(
        &self,
        mutated_object: ObjectID,
    ) -> SuiResult<Vec<(TxSequenceNumber, TransactionDigest)>> {
        Self::get_transactions_by_object(&self.transactions_by_mutated_object_id, mutated_object)
    }

    pub fn get_transactions_from_addr(
        &self,
        addr: SuiAddress,
    ) -> SuiResult<Vec<(TxSequenceNumber, TransactionDigest)>> {
        Self::get_transactions_by_object(&self.transactions_from_addr, addr)
    }

    pub fn get_transactions_to_addr(
        &self,
        addr: SuiAddress,
    ) -> SuiResult<Vec<(TxSequenceNumber, TransactionDigest)>> {
        Self::get_transactions_by_object(&self.transactions_to_addr, addr)
    }
}
