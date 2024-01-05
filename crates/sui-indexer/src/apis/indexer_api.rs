// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use async_trait::async_trait;
use futures::future::join_all;
use jsonrpsee::core::RpcResult;
use jsonrpsee::http_client::HttpClient;
use jsonrpsee::types::SubscriptionResult;
use jsonrpsee::{RpcModule, SubscriptionSink};

use sui_core::event_handler::EventHandler;
use sui_json_rpc::api::IndexerApiServer;
use sui_json_rpc::api::{validate_limit, IndexerApiClient, QUERY_MAX_RESULT_LIMIT};
use sui_json_rpc::indexer_api::spawn_subscription;
use sui_json_rpc::SuiRpcModule;
use sui_json_rpc_types::{
    CheckpointedObjectID, DynamicFieldPage, EventFilter, EventPage, ObjectsPage, Page,
    SuiObjectDataFilter, SuiObjectResponse, SuiObjectResponseQuery,
    SuiTransactionBlockResponseQuery, TransactionBlocksPage,
};
use sui_open_rpc::Module;
use sui_types::base_types::{ObjectID, SuiAddress};
use sui_types::digests::TransactionDigest;
use sui_types::dynamic_field::DynamicFieldName;
use sui_types::event::EventID;
use sui_types::query::TransactionFilter;

use crate::errors::IndexerError;
use crate::store::IndexerStore;

pub(crate) struct IndexerApi<S> {
    state: S,
    fullnode: HttpClient,
    event_handler: Arc<EventHandler>,
    migrated_methods: Vec<String>,
}

impl<S: IndexerStore> IndexerApi<S> {
    pub fn new(
        state: S,
        fullnode_client: HttpClient,
        event_handler: Arc<EventHandler>,
        migrated_methods: Vec<String>,
    ) -> Self {
        Self {
            state,
            fullnode: fullnode_client,
            event_handler,
            migrated_methods,
        }
    }

    pub fn query_events_internal(
        &self,
        query: EventFilter,
        cursor: Option<EventID>,
        limit: Option<usize>,
        descending_order: Option<bool>,
    ) -> Result<EventPage, IndexerError> {
        self.state
            .get_events(query, cursor, limit, descending_order.unwrap_or_default())
    }

    async fn query_transaction_blocks_internal(
        &self,
        query: SuiTransactionBlockResponseQuery,
        cursor: Option<TransactionDigest>,
        limit: Option<usize>,
        descending_order: Option<bool>,
    ) -> Result<TransactionBlocksPage, IndexerError> {
        let limit = validate_limit(limit, QUERY_MAX_RESULT_LIMIT)?;
        let is_descending = descending_order.unwrap_or_default();
        let cursor_str = cursor.map(|digest| digest.to_string());
        let mut tx_vec_from_db = match query.filter {
            None => {
                let indexer_seq_number = self
                    .state
                    .get_transaction_sequence_by_digest(cursor_str, is_descending)?;
                self.state
                    .get_all_transaction_page(indexer_seq_number, limit + 1, is_descending)
            }
            Some(TransactionFilter::Checkpoint(checkpoint_id)) => {
                let indexer_seq_number = self
                    .state
                    .get_transaction_sequence_by_digest(cursor_str, is_descending)?;
                self.state.get_transaction_page_by_checkpoint(
                    checkpoint_id as i64,
                    indexer_seq_number,
                    limit + 1,
                    is_descending,
                )
            }
            Some(TransactionFilter::MoveFunction {
                package,
                module,
                function,
            }) => {
                let move_call_seq_number = self
                    .state
                    .get_move_call_sequence_by_digest(cursor_str, is_descending)?;
                self.state.get_transaction_page_by_move_call(
                    package.to_string(),
                    module,
                    function,
                    move_call_seq_number,
                    limit + 1,
                    is_descending,
                )
            }
            Some(TransactionFilter::InputObject(input_obj_id)) => {
                let input_obj_seq = self
                    .state
                    .get_input_object_sequence_by_digest(cursor_str, is_descending)?;
                self.state.get_transaction_page_by_input_object(
                    input_obj_id.to_string(),
                    /* version */ None,
                    input_obj_seq,
                    limit + 1,
                    is_descending,
                )
            }
            Some(TransactionFilter::ChangedObject(mutated_obj_id)) => {
                let indexer_seq_number = self
                    .state
                    .get_transaction_sequence_by_digest(cursor_str, is_descending)?;
                self.state.get_transaction_page_by_mutated_object(
                    mutated_obj_id.to_string(),
                    indexer_seq_number,
                    limit + 1,
                    is_descending,
                )
            }
            // NOTE: more efficient to run this query over transactions table
            Some(TransactionFilter::FromAddress(sender_address)) => {
                let indexer_seq_number = self
                    .state
                    .get_transaction_sequence_by_digest(cursor_str, is_descending)?;
                self.state.get_transaction_page_by_sender_address(
                    sender_address.to_string(),
                    indexer_seq_number,
                    limit + 1,
                    is_descending,
                )
            }
            Some(TransactionFilter::ToAddress(recipient_address)) => {
                let recipient_seq_number = self
                    .state
                    .get_recipient_sequence_by_digest(cursor_str, is_descending)?;
                self.state.get_transaction_page_by_sender_recipient_address(
                    /* from */ None,
                    recipient_address.to_string(),
                    recipient_seq_number,
                    limit + 1,
                    is_descending,
                )
            }
            Some(TransactionFilter::FromAndToAddress { from, to }) => {
                let recipient_seq_number = self
                    .state
                    .get_recipient_sequence_by_digest(cursor_str, is_descending)?;
                self.state.get_transaction_page_by_sender_recipient_address(
                    Some(from.to_string()),
                    to.to_string(),
                    recipient_seq_number,
                    limit + 1,
                    is_descending,
                )
            }
            Some(TransactionFilter::TransactionKind(tx_kind_name)) => {
                let indexer_seq_number = self
                    .state
                    .get_transaction_sequence_by_digest(cursor_str, is_descending)?;
                self.state.get_transaction_page_by_transaction_kind(
                    tx_kind_name,
                    indexer_seq_number,
                    limit + 1,
                    is_descending,
                )
            }
        }?;

        let has_next_page = tx_vec_from_db.len() > limit;
        tx_vec_from_db.truncate(limit);
        let next_cursor = tx_vec_from_db
            .last()
            .cloned()
            .map(|tx| {
                let digest = tx.transaction_digest;
                let tx_digest: Result<TransactionDigest, _> = digest.parse();
                tx_digest.map_err(|e| {
                    IndexerError::SerdeError(format!(
                        "Failed to deserialize transaction digest: {:?} with error {:?}",
                        digest, e
                    ))
                })
            })
            .transpose()?
            .map_or(cursor, Some);

        let tx_resp_futures = tx_vec_from_db.into_iter().map(|tx| {
            self.state
                .compose_sui_transaction_block_response(tx, query.options.as_ref())
        });
        let sui_tx_resp_vec = join_all(tx_resp_futures)
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Page {
            data: sui_tx_resp_vec,
            next_cursor,
            has_next_page,
        })
    }

    async fn get_owned_objects_interal(
        &self,
        address: SuiAddress,
        query: Option<SuiObjectResponseQuery>,
        cursor: Option<CheckpointedObjectID>,
        limit: Option<usize>,
    ) -> RpcResult<ObjectsPage> {
        let address = SuiObjectDataFilter::AddressOwner(address);
        let (filter, options) = match query {
            Some(SuiObjectResponseQuery {
                filter: Some(filter),
                options,
            }) => (address.and(filter), options),
            Some(SuiObjectResponseQuery { filter: _, options }) => (address, options),
            None => (address, None),
        };

        let at_checkpoint = if let Some(CheckpointedObjectID {
            at_checkpoint: Some(at_checkpoint),
            ..
        }) = cursor
        {
            at_checkpoint
        } else {
            self.state.get_latest_checkpoint_sequence_number()? as u64
        };
        let object_cursor = cursor.as_ref().map(|c| c.object_id);

        let limit = validate_limit(limit, QUERY_MAX_RESULT_LIMIT)?;
        let options = options.unwrap_or_default();
        let mut objects =
            self.state
                .query_objects(filter, at_checkpoint, object_cursor, limit + 1)?;

        let has_next_page = objects.len() > limit;
        objects.truncate(limit);
        let next_cursor = objects.last().and_then(|o| {
            o.object().ok().map(|o| CheckpointedObjectID {
                object_id: o.id(),
                at_checkpoint: Some(at_checkpoint),
            })
        });

        let objects = objects
            .into_iter()
            .map(|o| (o, options.clone()).try_into())
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Page {
            data: objects,
            next_cursor,
            has_next_page,
        })
    }
}

#[async_trait]
impl<S> IndexerApiServer for IndexerApi<S>
where
    S: IndexerStore + Sync + Send + 'static,
{
    async fn get_owned_objects(
        &self,
        address: SuiAddress,
        query: Option<SuiObjectResponseQuery>,
        cursor: Option<CheckpointedObjectID>,
        limit: Option<usize>,
    ) -> RpcResult<ObjectsPage> {
        if !self
            .migrated_methods
            .contains(&"get_owned_objects".to_string())
        {
            return self
                .fullnode
                .get_owned_objects(address, query, cursor, limit)
                .await;
        }
        Ok(self
            .get_owned_objects_interal(address, query, cursor, limit)
            .await?)
    }

    async fn query_transaction_blocks(
        &self,
        query: SuiTransactionBlockResponseQuery,
        cursor: Option<TransactionDigest>,
        limit: Option<usize>,
        descending_order: Option<bool>,
    ) -> RpcResult<TransactionBlocksPage> {
        if !self
            .migrated_methods
            .contains(&"query_transaction_blocks".to_string())
        {
            return self
                .fullnode
                .query_transaction_blocks(query, cursor, limit, descending_order)
                .await;
        }
        Ok(self
            .query_transaction_blocks_internal(query, cursor, limit, descending_order)
            .await?)
    }

    async fn query_events(
        &self,
        query: EventFilter,
        // exclusive cursor if `Some`, otherwise start from the beginning
        cursor: Option<EventID>,
        limit: Option<usize>,
        descending_order: Option<bool>,
    ) -> RpcResult<EventPage> {
        if self.migrated_methods.contains(&"query_events".to_string()) {
            return self
                .fullnode
                .query_events(query, cursor, limit, descending_order)
                .await;
        }
        Ok(self.query_events_internal(query, cursor, limit, descending_order)?)
    }

    async fn get_dynamic_fields(
        &self,
        parent_object_id: ObjectID,
        cursor: Option<ObjectID>,
        limit: Option<usize>,
    ) -> RpcResult<DynamicFieldPage> {
        self.fullnode
            .get_dynamic_fields(parent_object_id, cursor, limit)
            .await
    }

    async fn get_dynamic_field_object(
        &self,
        parent_object_id: ObjectID,
        name: DynamicFieldName,
    ) -> RpcResult<SuiObjectResponse> {
        self.fullnode
            .get_dynamic_field_object(parent_object_id, name)
            .await
    }

    fn subscribe_event(&self, sink: SubscriptionSink, filter: EventFilter) -> SubscriptionResult {
        spawn_subscription(sink, self.event_handler.subscribe(filter));
        Ok(())
    }
}

impl<S> SuiRpcModule for IndexerApi<S>
where
    S: IndexerStore + Sync + Send + 'static,
{
    fn rpc(self) -> RpcModule<Self> {
        self.into_rpc()
    }

    fn rpc_doc_module() -> Module {
        sui_json_rpc::api::IndexerApiOpenRpc::module_doc()
    }
}
