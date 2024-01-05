// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use async_trait::async_trait;

use sui_json_rpc_types::{
    Checkpoint as RpcCheckpoint, CheckpointId, EpochInfo, EventFilter, EventPage, MoveCallMetrics,
    NetworkMetrics, SuiObjectData, SuiObjectDataFilter, SuiTransactionResponseOptions,
};
use sui_types::base_types::{EpochId, ObjectID, SequenceNumber};
use sui_types::digests::CheckpointDigest;
use sui_types::error::SuiError;
use sui_types::event::EventID;
use sui_types::messages_checkpoint::CheckpointSequenceNumber;
use sui_types::object::ObjectRead;
use sui_types::storage::ObjectStore;

use crate::errors::IndexerError;
use crate::models::addresses::Address;
use crate::models::checkpoints::Checkpoint;
use crate::models::epoch::DBEpochInfo;
use crate::models::events::Event;
use crate::models::objects::{DeletedObject, Object, ObjectStatus};
use crate::models::packages::Package;
use crate::models::system_state::{DBSystemStateSummary, DBValidatorSummary};
use crate::models::transaction_index::{InputObject, MoveCall, Recipient};
use crate::models::transactions::Transaction;
use crate::types::SuiTransactionFullResponse;

#[async_trait]
pub trait IndexerStore {
    type ModuleCache;

    fn get_latest_checkpoint_sequence_number(&self) -> Result<i64, IndexerError>;
    fn get_checkpoint(&self, id: CheckpointId) -> Result<RpcCheckpoint, IndexerError>;
    fn get_checkpoint_sequence_number(
        &self,
        digest: CheckpointDigest,
    ) -> Result<CheckpointSequenceNumber, IndexerError>;

    fn get_event(&self, id: EventID) -> Result<Event, IndexerError>;
    fn get_events(
        &self,
        query: EventFilter,
        cursor: Option<EventID>,
        limit: Option<usize>,
        descending_order: bool,
    ) -> Result<EventPage, IndexerError>;

    fn get_object(
        &self,
        object_id: ObjectID,
        version: Option<SequenceNumber>,
    ) -> Result<ObjectRead, IndexerError>;

    fn query_objects(
        &self,
        filter: SuiObjectDataFilter,
        at_checkpoint: CheckpointSequenceNumber,
        cursor: Option<ObjectID>,
        limit: usize,
    ) -> Result<Vec<ObjectRead>, IndexerError>;

    fn get_total_transaction_number_from_checkpoints(&self) -> Result<i64, IndexerError>;

    // TODO: combine all get_transaction* methods
    fn get_transaction_by_digest(&self, tx_digest: &str) -> Result<Transaction, IndexerError>;
    fn multi_get_transactions_by_digests(
        &self,
        tx_digests: &[String],
    ) -> Result<Vec<Transaction>, IndexerError>;

    async fn compose_full_transaction_response(
        &self,
        tx: Transaction,
        options: Option<SuiTransactionResponseOptions>,
    ) -> Result<SuiTransactionFullResponse, IndexerError>;

    fn get_all_transaction_digest_page(
        &self,
        start_sequence: Option<i64>,
        limit: usize,
        is_descending: bool,
    ) -> Result<Vec<String>, IndexerError>;

    fn get_transaction_digest_page_by_checkpoint(
        &self,
        checkpoint_sequence_number: i64,
        start_sequence: Option<i64>,
        limit: usize,
        is_descending: bool,
    ) -> Result<Vec<String>, IndexerError>;

    fn get_transaction_digest_page_by_transaction_kind(
        &self,
        kind: String,
        start_sequence: Option<i64>,
        limit: usize,
        is_descending: bool,
    ) -> Result<Vec<String>, IndexerError>;

    fn get_transaction_digest_page_by_mutated_object(
        &self,
        object_id: String,
        start_sequence: Option<i64>,
        limit: usize,
        is_descending: bool,
    ) -> Result<Vec<String>, IndexerError>;

    fn get_transaction_digest_page_by_sender_address(
        &self,
        sender_address: String,
        start_sequence: Option<i64>,
        limit: usize,
        is_descending: bool,
    ) -> Result<Vec<String>, IndexerError>;

    fn get_transaction_digest_page_by_sender_recipient_address(
        &self,
        sender_address: Option<String>,
        recipient_address: String,
        start_sequence: Option<i64>,
        limit: usize,
        is_descending: bool,
    ) -> Result<Vec<String>, IndexerError>;

    fn get_transaction_digest_page_by_input_object(
        &self,
        object_id: String,
        version: Option<i64>,
        start_sequence: Option<i64>,
        limit: usize,
        is_descending: bool,
    ) -> Result<Vec<String>, IndexerError>;

    fn get_transaction_digest_page_by_move_call(
        &self,
        package: String,
        module: Option<String>,
        function: Option<String>,
        start_sequence: Option<i64>,
        limit: usize,
        is_descending: bool,
    ) -> Result<Vec<String>, IndexerError>;

    fn get_transaction_sequence_by_digest(
        &self,
        tx_digest: Option<String>,
        is_descending: bool,
    ) -> Result<Option<i64>, IndexerError>;

    fn get_move_call_sequence_by_digest(
        &self,
        tx_digest: Option<String>,
        is_descending: bool,
    ) -> Result<Option<i64>, IndexerError>;

    fn get_input_object_sequence_by_digest(
        &self,
        tx_digest: Option<String>,
        is_descending: bool,
    ) -> Result<Option<i64>, IndexerError>;

    fn get_recipient_sequence_by_digest(
        &self,
        tx_digest: Option<String>,
        is_descending: bool,
    ) -> Result<Option<i64>, IndexerError>;

    fn read_transactions(
        &self,
        last_processed_id: i64,
        limit: usize,
    ) -> Result<Vec<Transaction>, IndexerError>;

    fn get_network_metrics(&self) -> Result<NetworkMetrics, IndexerError>;
    fn get_move_call_metrics(&self) -> Result<MoveCallMetrics, IndexerError>;

    fn persist_checkpoint(&self, data: &TemporaryCheckpointStore) -> Result<usize, IndexerError>;
    fn persist_epoch(&self, data: &TemporaryEpochStore) -> Result<(), IndexerError>;

    fn get_epochs(
        &self,
        cursor: Option<EpochId>,
        limit: usize,
        descending_order: Option<bool>,
    ) -> Result<Vec<EpochInfo>, IndexerError>;

    fn get_current_epoch(&self) -> Result<EpochInfo, IndexerError>;

    fn module_cache(&self) -> &Self::ModuleCache;
}

#[derive(Clone, Debug)]
pub struct CheckpointData {
    pub checkpoint: RpcCheckpoint,
    pub transactions: Vec<SuiTransactionFullResponse>,
    pub changed_objects: Vec<(ObjectStatus, SuiObjectData)>,
}

impl ObjectStore for CheckpointData {
    fn get_object(
        &self,
        object_id: &ObjectID,
    ) -> Result<Option<sui_types::object::Object>, SuiError> {
        Ok(self
            .changed_objects
            .iter()
            .find_map(|(status, o)| match status {
                ObjectStatus::Created | ObjectStatus::Mutated if &o.object_id == object_id => {
                    o.clone().try_into().ok()
                }
                _ => None,
            }))
    }
}

// Per checkpoint indexing
pub struct TemporaryCheckpointStore {
    pub checkpoint: Checkpoint,
    pub transactions: Vec<Transaction>,
    pub events: Vec<Event>,
    pub objects_changes: Vec<TransactionObjectChanges>,
    pub addresses: Vec<Address>,
    pub packages: Vec<Package>,
    pub input_objects: Vec<InputObject>,
    pub move_calls: Vec<MoveCall>,
    pub recipients: Vec<Recipient>,
}

#[derive(Debug)]
pub struct TransactionObjectChanges {
    pub mutated_objects: Vec<Object>,
    pub deleted_objects: Vec<DeletedObject>,
}

// Per epoch indexing
pub struct TemporaryEpochStore {
    pub last_epoch: Option<DBEpochInfo>,
    pub new_epoch: DBEpochInfo,
    pub system_state: DBSystemStateSummary,
    pub validators: Vec<DBValidatorSummary>,
}
