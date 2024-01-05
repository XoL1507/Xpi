// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use async_trait::async_trait;
use fastcrypto::encoding::Base64;
use jsonrpsee::core::RpcResult;
use jsonrpsee::http_client::HttpClient;
use jsonrpsee::RpcModule;

use sui_json_rpc::api::{WriteApiClient, WriteApiServer};
use sui_json_rpc::SuiRpcModule;
use sui_json_rpc_types::{
    BigInt, DevInspectResults, DryRunTransactionBlockResponse, SuiTransactionBlockEffectsAPI,
    SuiTransactionBlockResponse, SuiTransactionBlockResponseOptions,
};
use sui_open_rpc::Module;
use sui_types::base_types::{EpochId, SuiAddress};
use sui_types::messages::ExecuteTransactionRequestType;

use crate::handlers::checkpoint_handler::{
    fetch_changed_objects, get_deleted_db_objects, get_object_changes, to_changed_db_objects,
};
use crate::models::transactions::Transaction;
use crate::store::{IndexerStore, TransactionObjectChanges};
use crate::types::{
    FastPathTransactionBlockResponse, SuiTransactionBlockResponseWithOptions,
    TemporaryTransactionBlockResponseStore,
};

pub(crate) struct WriteApi<S> {
    fullnode: HttpClient,
    state: S,
}

impl<S: IndexerStore> WriteApi<S> {
    pub fn new(state: S, fullnode_client: HttpClient) -> Self {
        Self {
            state,
            fullnode: fullnode_client,
        }
    }
}

#[async_trait]
impl<S> WriteApiServer for WriteApi<S>
where
    S: IndexerStore + Sync + Send + 'static,
{
    async fn execute_transaction_block(
        &self,
        tx_bytes: Base64,
        signatures: Vec<Base64>,
        options: Option<SuiTransactionBlockResponseOptions>,
        request_type: Option<ExecuteTransactionRequestType>,
    ) -> RpcResult<SuiTransactionBlockResponse> {
        let fast_path_options = SuiTransactionBlockResponseOptions::full_content();
        let sui_transaction_response = self
            .fullnode
            .execute_transaction_block(tx_bytes, signatures, Some(fast_path_options), request_type)
            .await?;
        let fast_path_resp: FastPathTransactionBlockResponse =
            sui_transaction_response.clone().try_into()?;
        let effects = &fast_path_resp.effects;
        let epoch = <u64>::from(effects.executed_epoch());

        let object_changes = get_object_changes(effects);
        let changed_objects = fetch_changed_objects(self.fullnode.clone(), object_changes).await?;
        let changed_db_objects =
            to_changed_db_objects(changed_objects, epoch, /* checkpoint */ None);
        let deleted_db_objects = get_deleted_db_objects(effects, epoch, /* checkpoint */ None);
        let tx_object_changes = TransactionObjectChanges {
            changed_objects: changed_db_objects,
            deleted_objects: deleted_db_objects,
        };

        let transaction_store: TemporaryTransactionBlockResponseStore = fast_path_resp.into();
        let transaction: Transaction = transaction_store.try_into()?;
        self.state
            .persist_fast_path(transaction, tx_object_changes)
            .await?;

        Ok(SuiTransactionBlockResponseWithOptions {
            response: sui_transaction_response,
            options: options.unwrap_or_default(),
        }
        .into())
    }

    async fn dev_inspect_transaction_block(
        &self,
        sender_address: SuiAddress,
        tx_bytes: Base64,
        gas_price: Option<BigInt>,
        epoch: Option<EpochId>,
    ) -> RpcResult<DevInspectResults> {
        self.fullnode
            .dev_inspect_transaction_block(sender_address, tx_bytes, gas_price, epoch)
            .await
    }

    async fn dry_run_transaction_block(
        &self,
        tx_bytes: Base64,
    ) -> RpcResult<DryRunTransactionBlockResponse> {
        self.fullnode.dry_run_transaction_block(tx_bytes).await
    }
}

impl<S> SuiRpcModule for WriteApi<S>
where
    S: IndexerStore + Sync + Send + 'static,
{
    fn rpc(self) -> RpcModule<Self> {
        self.into_rpc()
    }

    fn rpc_doc_module() -> Module {
        sui_json_rpc::api::WriteApiOpenRpc::module_doc()
    }
}
