// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::errors::IndexerError;
use crate::store::IndexerStore;
use crate::types::{SuiTransactionFullResponse, SuiTransactionFullResponseWithOptions};
use async_trait::async_trait;
use futures::future::join_all;
use jsonrpsee::core::RpcResult;
use jsonrpsee::http_client::HttpClient;
use jsonrpsee::RpcModule;
use std::collections::BTreeMap;
use sui_json_rpc::api::{validate_limit, ReadApiClient, ReadApiServer, QUERY_MAX_RESULT_LIMIT};
use sui_json_rpc::SuiRpcModule;
use sui_json_rpc_types::{
    BigInt, Checkpoint, CheckpointId, CheckpointPage, DynamicFieldPage, MoveFunctionArgType,
    ObjectsPage, Page, SuiCheckpointSequenceNumber, SuiGetPastObjectRequest,
    SuiMoveNormalizedFunction, SuiMoveNormalizedModule, SuiMoveNormalizedStruct,
    SuiObjectDataOptions, SuiObjectResponse, SuiObjectResponseQuery, SuiPastObjectResponse,
    SuiTransactionResponse, SuiTransactionResponseOptions, SuiTransactionResponseQuery,
    TransactionsPage,
};
use sui_open_rpc::Module;
use sui_types::base_types::{ObjectID, SequenceNumber, SuiAddress, TxSequenceNumber};
use sui_types::digests::TransactionDigest;
use sui_types::dynamic_field::DynamicFieldName;
use sui_types::query::TransactionFilter;

pub(crate) struct ReadApi<S> {
    fullnode: HttpClient,
    state: S,
    migrated_methods: Vec<String>,
}

impl<S: IndexerStore> ReadApi<S> {
    pub fn new(state: S, fullnode_client: HttpClient, migrated_methods: Vec<String>) -> Self {
        Self {
            state,
            fullnode: fullnode_client,
            migrated_methods,
        }
    }

    fn get_total_transaction_number_internal(&self) -> Result<u64, IndexerError> {
        self.state
            .get_total_transaction_number_from_checkpoints()
            .map(|n| n as u64)
    }

    async fn get_transaction_with_options_internal(
        &self,
        digest: &TransactionDigest,
        options: Option<SuiTransactionResponseOptions>,
    ) -> Result<SuiTransactionResponse, IndexerError> {
        let tx = self
            .state
            .get_transaction_by_digest(&digest.base58_encode())?;
        let tx_full_resp: SuiTransactionFullResponse = self
            .state
            .compose_full_transaction_response(tx, options.clone())
            .await?;

        let sui_transaction_response = SuiTransactionFullResponseWithOptions {
            response: tx_full_resp,
            options: options.unwrap_or_default(),
        }
        .into();
        Ok(sui_transaction_response)
    }

    async fn multi_get_transactions_with_options_internal(
        &self,
        digests: &[TransactionDigest],
        options: Option<SuiTransactionResponseOptions>,
    ) -> Result<Vec<SuiTransactionResponse>, IndexerError> {
        let digest_strs = digests
            .iter()
            .map(|digest| digest.base58_encode())
            .collect::<Vec<_>>();
        let tx_vec = self.state.multi_get_transactions_by_digests(&digest_strs)?;
        let ordered_tx_vec = digest_strs
            .iter()
            .filter_map(|digest| {
                tx_vec
                    .iter()
                    .find(|tx| tx.transaction_digest == *digest)
                    .cloned()
            })
            .collect::<Vec<_>>();
        if ordered_tx_vec.len() != tx_vec.len() {
            return Err(IndexerError::PostgresReadError(
                "Transaction count changed after reorder, this should never happen.".to_string(),
            ));
        }
        let tx_full_resp_futures = ordered_tx_vec.into_iter().map(|tx| {
            self.state
                .compose_full_transaction_response(tx, options.clone())
        });
        let tx_full_resp_vec: Vec<SuiTransactionFullResponse> = join_all(tx_full_resp_futures)
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;

        let tx_resp_vec: Vec<SuiTransactionResponse> =
            tx_full_resp_vec.into_iter().map(|tx| tx.into()).collect();
        Ok(tx_resp_vec)
    }

    fn query_transactions_internal(
        &self,
        query: SuiTransactionResponseQuery,
        cursor: Option<TransactionDigest>,
        limit: Option<usize>,
        descending_order: Option<bool>,
    ) -> Result<TransactionsPage, IndexerError> {
        let limit = validate_limit(limit, QUERY_MAX_RESULT_LIMIT)?;
        let is_descending = descending_order.unwrap_or_default();
        let cursor_str = cursor.map(|digest| digest.to_string());

        let opts = query.options.unwrap_or_default();
        if !opts.only_digest() {
            // TODO(chris): implement this as a separate PR
            return Err(IndexerError::NotImplementedError(
                "options has not been implemented on indexer for queryTransactions".to_string(),
            ));
        }

        let digests_from_db = match query.filter {
            None => {
                let indexer_seq_number = self
                    .state
                    .get_transaction_sequence_by_digest(cursor_str, is_descending)?;
                self.state.get_all_transaction_digest_page(
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
                self.state.get_transaction_digest_page_by_move_call(
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
                self.state.get_transaction_digest_page_by_input_object(
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
                self.state.get_transaction_digest_page_by_mutated_object(
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
                self.state.get_transaction_digest_page_by_sender_address(
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
                self.state
                    .get_transaction_digest_page_by_sender_recipient_address(
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
                self.state
                    .get_transaction_digest_page_by_sender_recipient_address(
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
                self.state.get_transaction_digest_page_by_transaction_kind(
                    tx_kind_name,
                    indexer_seq_number,
                    limit + 1,
                    is_descending,
                )
            }
        }?;

        // digests here are of size (limit + 1), where the last one is the cursor for the next page
        let mut tx_digests = digests_from_db
            .iter()
            .map(|digest| {
                let tx_digest: Result<TransactionDigest, _> = digest.clone().parse();
                tx_digest.map_err(|e| {
                    IndexerError::SerdeError(format!(
                        "Failed to deserialize transaction digest: {:?} with error {:?}",
                        digest, e
                    ))
                })
            })
            .collect::<Result<Vec<TransactionDigest>, IndexerError>>()?;

        let has_next_page = tx_digests.len() > limit;
        tx_digests.truncate(limit);
        let next_cursor = tx_digests.last().cloned().map_or(cursor, Some);

        Ok(Page {
            data: tx_digests
                .into_iter()
                .map(SuiTransactionResponse::new)
                .collect(),
            next_cursor,
            has_next_page,
        })
    }

    fn get_object_with_options_internal(
        &self,
        object_id: ObjectID,
        options: Option<SuiObjectDataOptions>,
    ) -> Result<SuiObjectResponse, IndexerError> {
        let read = self.state.get_object(object_id, None)?;
        Ok((read, options.unwrap_or_default()).try_into()?)
    }

    fn get_latest_checkpoint_sequence_number_internal(&self) -> Result<u64, IndexerError> {
        self.state
            .get_latest_checkpoint_sequence_number()
            .map(|n| n as u64)
    }
}

#[async_trait]
impl<S> ReadApiServer for ReadApi<S>
where
    S: IndexerStore + Sync + Send + 'static,
{
    async fn get_owned_objects(
        &self,
        address: SuiAddress,
        query: Option<SuiObjectResponseQuery>,
        cursor: Option<ObjectID>,
        limit: Option<usize>,
        at_checkpoint: Option<CheckpointId>,
    ) -> RpcResult<ObjectsPage> {
        self.fullnode
            .get_owned_objects(address, query, cursor, limit, at_checkpoint)
            .await
    }

    async fn get_object_with_options(
        &self,
        object_id: ObjectID,
        options: Option<SuiObjectDataOptions>,
    ) -> RpcResult<SuiObjectResponse> {
        if !self
            .migrated_methods
            .contains(&"get_object_with_options".into())
        {
            return self
                .fullnode
                .get_object_with_options(object_id, options)
                .await;
        }

        Ok(self.get_object_with_options_internal(object_id, options)?)
    }

    async fn multi_get_object_with_options(
        &self,
        object_ids: Vec<ObjectID>,
        options: Option<SuiObjectDataOptions>,
    ) -> RpcResult<Vec<SuiObjectResponse>> {
        return self
            .fullnode
            .multi_get_object_with_options(object_ids, options)
            .await;
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

    async fn get_total_transaction_number(&self) -> RpcResult<BigInt> {
        if !self
            .migrated_methods
            .contains(&"get_total_transaction_number".to_string())
        {
            return self.fullnode.get_total_transaction_number().await;
        }
        Ok(self.get_total_transaction_number_internal()?.into())
    }

    async fn query_transactions(
        &self,
        query: SuiTransactionResponseQuery,
        cursor: Option<TransactionDigest>,
        limit: Option<usize>,
        descending_order: Option<bool>,
    ) -> RpcResult<TransactionsPage> {
        if !self
            .migrated_methods
            .contains(&"query_transactions".to_string())
        {
            return self
                .fullnode
                .query_transactions(query, cursor, limit, descending_order)
                .await;
        }
        Ok(self.query_transactions_internal(query, cursor, limit, descending_order)?)
    }

    async fn get_transactions_in_range_deprecated(
        &self,
        start: TxSequenceNumber,
        end: TxSequenceNumber,
    ) -> RpcResult<Vec<TransactionDigest>> {
        self.fullnode
            .get_transactions_in_range_deprecated(start, end)
            .await
    }

    async fn get_transaction_with_options(
        &self,
        digest: TransactionDigest,
        options: Option<SuiTransactionResponseOptions>,
    ) -> RpcResult<SuiTransactionResponse> {
        if !self
            .migrated_methods
            .contains(&"get_transaction".to_string())
        {
            return self
                .fullnode
                .get_transaction_with_options(digest, options)
                .await;
        }
        Ok(self
            .get_transaction_with_options_internal(&digest, options)
            .await?)
    }

    async fn multi_get_transactions_with_options(
        &self,
        digests: Vec<TransactionDigest>,
        options: Option<SuiTransactionResponseOptions>,
    ) -> RpcResult<Vec<SuiTransactionResponse>> {
        if !self
            .migrated_methods
            .contains(&"multi_get_transactions_with_options".to_string())
        {
            return self
                .fullnode
                .multi_get_transactions_with_options(digests, options)
                .await;
        }
        Ok(self
            .multi_get_transactions_with_options_internal(&digests, options)
            .await?)
    }

    async fn get_normalized_move_modules_by_package(
        &self,
        package: ObjectID,
    ) -> RpcResult<BTreeMap<String, SuiMoveNormalizedModule>> {
        self.fullnode
            .get_normalized_move_modules_by_package(package)
            .await
    }

    async fn get_normalized_move_module(
        &self,
        package: ObjectID,
        module_name: String,
    ) -> RpcResult<SuiMoveNormalizedModule> {
        self.fullnode
            .get_normalized_move_module(package, module_name)
            .await
    }

    async fn get_normalized_move_struct(
        &self,
        package: ObjectID,
        module_name: String,
        struct_name: String,
    ) -> RpcResult<SuiMoveNormalizedStruct> {
        self.fullnode
            .get_normalized_move_struct(package, module_name, struct_name)
            .await
    }

    async fn get_normalized_move_function(
        &self,
        package: ObjectID,
        module_name: String,
        function_name: String,
    ) -> RpcResult<SuiMoveNormalizedFunction> {
        self.fullnode
            .get_normalized_move_function(package, module_name, function_name)
            .await
    }

    async fn get_move_function_arg_types(
        &self,
        package: ObjectID,
        module: String,
        function: String,
    ) -> RpcResult<Vec<MoveFunctionArgType>> {
        self.fullnode
            .get_move_function_arg_types(package, module, function)
            .await
    }

    async fn try_get_past_object(
        &self,
        object_id: ObjectID,
        version: SequenceNumber,
        options: Option<SuiObjectDataOptions>,
    ) -> RpcResult<SuiPastObjectResponse> {
        self.fullnode
            .try_get_past_object(object_id, version, options)
            .await
    }

    async fn try_multi_get_past_objects(
        &self,
        past_objects: Vec<SuiGetPastObjectRequest>,
        options: Option<SuiObjectDataOptions>,
    ) -> RpcResult<Vec<SuiPastObjectResponse>> {
        self.fullnode
            .try_multi_get_past_objects(past_objects, options)
            .await
    }

    async fn get_latest_checkpoint_sequence_number(
        &self,
    ) -> RpcResult<SuiCheckpointSequenceNumber> {
        if !self
            .migrated_methods
            .contains(&"get_latest_checkpoint_sequence_number".to_string())
        {
            return self.fullnode.get_latest_checkpoint_sequence_number().await;
        }
        Ok(self
            .get_latest_checkpoint_sequence_number_internal()?
            .into())
    }

    async fn get_checkpoint(&self, id: CheckpointId) -> RpcResult<Checkpoint> {
        if !self
            .migrated_methods
            .contains(&"get_checkpoint".to_string())
        {
            return self.fullnode.get_checkpoint(id).await;
        }
        Ok(self.state.get_checkpoint(id)?)
    }

    async fn get_checkpoints(
        &self,
        cursor: Option<SuiCheckpointSequenceNumber>,
        limit: Option<usize>,
        descending_order: bool,
    ) -> RpcResult<CheckpointPage> {
        return self
            .fullnode
            .get_checkpoints(cursor, limit, descending_order)
            .await;
    }
}

impl<S> SuiRpcModule for ReadApi<S>
where
    S: IndexerStore + Sync + Send + 'static,
{
    fn rpc(self) -> RpcModule<Self> {
        self.into_rpc()
    }

    fn rpc_doc_module() -> Module {
        sui_json_rpc::api::ReadApiOpenRpc::module_doc()
    }
}
