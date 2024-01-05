// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use anyhow::anyhow;
use async_trait::async_trait;
use fastcrypto::encoding::Base64;
use fastcrypto::traits::ToFromBytes;
use jsonrpsee::core::RpcResult;
use jsonrpsee::RpcModule;

use mysten_metrics::spawn_monitored_task;
use shared_crypto::intent::Intent;
use sui_core::authority::AuthorityState;
use sui_core::authority_client::NetworkAuthorityClient;
use sui_core::transaction_orchestrator::TransactiondOrchestrator;
use sui_json_rpc_types::{
    DevInspectResults, DryRunTransactionResponse, SuiTransactionEvents, SuiTransactionResponse,
    SuiTransactionResponseOptions,
};
use sui_open_rpc::Module;
use sui_types::base_types::{EpochId, SuiAddress};
use sui_types::messages::{
    ExecuteTransactionRequest, ExecuteTransactionRequestType, TransactionKind,
};
use sui_types::messages::{ExecuteTransactionResponse, Transaction};
use sui_types::messages::{TransactionData, TransactionDataAPI};
use sui_types::signature::GenericSignature;

use crate::api::WriteApiServer;
use crate::balance_changes::get_balance_change_from_effect;
use crate::error::Error;
use crate::read_api::get_transaction_data_and_digest;
use crate::{get_object_change_from_effect, ObjectProviderCache, SuiRpcModule};

pub struct TransactionExecutionApi {
    state: Arc<AuthorityState>,
    transaction_orchestrator: Arc<TransactiondOrchestrator<NetworkAuthorityClient>>,
}
impl TransactionExecutionApi {
    pub fn new(
        state: Arc<AuthorityState>,
        transaction_orchestrator: Arc<TransactiondOrchestrator<NetworkAuthorityClient>>,
    ) -> Self {
        Self {
            state,
            transaction_orchestrator,
        }
    }

    async fn execute_transaction(
        &self,
        tx_bytes: Base64,
        signatures: Vec<Base64>,
        opts: Option<SuiTransactionResponseOptions>,
        request_type: Option<ExecuteTransactionRequestType>,
    ) -> Result<SuiTransactionResponse, Error> {
        let opts = opts.unwrap_or_default();

        let request_type = match (request_type, opts.require_local_execution()) {
            (Some(ExecuteTransactionRequestType::WaitForEffectsCert), true) => {
                return Err(anyhow!(
                    "`request_type` must set to `None` or `WaitForLocalExecution`\
                         if effects is required in the response"
                )
                .into());
            }
            (t, _) => t.unwrap_or_else(|| opts.default_execution_request_type()),
        };
        let tx_data: TransactionData = bcs::from_bytes(&tx_bytes.to_vec()?)?;
        let sender = tx_data.sender();

        let mut sigs = Vec::new();
        for sig in signatures {
            sigs.push(GenericSignature::from_bytes(&sig.to_vec()?)?);
        }

        let txn = Transaction::from_generic_sig_data(tx_data, Intent::default(), sigs);
        let tx = txn.data().clone().try_into()?;
        let raw_transaction = opts.show_raw_input.then_some(bcs::to_bytes(txn.data())?);

        let digest = *txn.digest();

        let transaction_orchestrator = self.transaction_orchestrator.clone();
        let response = spawn_monitored_task!(transaction_orchestrator.execute_transaction(
            ExecuteTransactionRequest {
                transaction: txn,
                request_type,
            }
        ))
        .await??;

        match response {
            ExecuteTransactionResponse::EffectsCert(cert) => {
                let (effects, transaction_events, is_executed_locally) = *cert;
                let mut events: Option<SuiTransactionEvents> = None;
                if opts.show_events {
                    let module_cache = self
                        .state
                        .load_epoch_store_one_call_per_task()
                        .module_cache()
                        .clone();
                    events = Some(SuiTransactionEvents::try_from(
                        transaction_events,
                        digest,
                        None,
                        module_cache.as_ref(),
                    )?);
                }

                let object_cache = ObjectProviderCache::new(self.state.clone());
                let balance_changes = if opts.show_balance_changes {
                    Some(get_balance_change_from_effect(&object_cache, &effects.effects).await?)
                } else {
                    None
                };
                let object_changes = if opts.show_object_changes {
                    Some(
                        get_object_change_from_effect(&object_cache, sender, &effects.effects)
                            .await?,
                    )
                } else {
                    None
                };

                Ok(SuiTransactionResponse {
                    digest,
                    transaction: opts.show_input.then_some(tx),
                    raw_transaction,
                    effects: opts.show_effects.then_some(effects.effects.try_into()?),
                    events,
                    object_changes,
                    balance_changes,
                    timestamp_ms: None,
                    confirmed_local_execution: Some(is_executed_locally),
                    checkpoint: None,
                    errors: vec![],
                })
            }
        }
    }
}

#[async_trait]
impl WriteApiServer for TransactionExecutionApi {
    async fn execute_transaction(
        &self,
        tx_bytes: Base64,
        signatures: Vec<Base64>,
        opts: Option<SuiTransactionResponseOptions>,
        request_type: Option<ExecuteTransactionRequestType>,
    ) -> RpcResult<SuiTransactionResponse> {
        Ok(self
            .execute_transaction(tx_bytes, signatures, opts, request_type)
            .await?)
    }

    async fn dev_inspect_transaction(
        &self,
        sender_address: SuiAddress,
        tx_bytes: Base64,
        gas_price: Option<u64>,
        _epoch: Option<EpochId>,
    ) -> RpcResult<DevInspectResults> {
        let tx_kind: TransactionKind =
            bcs::from_bytes(&tx_bytes.to_vec().map_err(|e| anyhow!(e))?).map_err(|e| anyhow!(e))?;
        Ok(self
            .state
            .dev_inspect_transaction(sender_address, tx_kind, gas_price)
            .await?)
    }

    async fn dry_run_transaction(&self, tx_bytes: Base64) -> RpcResult<DryRunTransactionResponse> {
        let (txn_data, txn_digest) = get_transaction_data_and_digest(tx_bytes)?;
        Ok(self
            .state
            .dry_exec_transaction(txn_data, txn_digest)
            .await?)
    }
}

impl SuiRpcModule for TransactionExecutionApi {
    fn rpc(self) -> RpcModule<Self> {
        self.into_rpc()
    }

    fn rpc_doc_module() -> Module {
        crate::api::WriteApiOpenRpc::module_doc()
    }
}
