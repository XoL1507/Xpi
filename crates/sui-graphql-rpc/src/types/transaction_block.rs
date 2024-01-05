// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::context_data::{
    context_ext::DataProviderContextExt, sui_sdk_data_provider::convert_to_epoch,
};

use super::{
    address::Address,
    base64::Base64,
    epoch::Epoch,
    gas::{GasEffects, GasInput},
    sui_address::SuiAddress,
    tx_digest::TransactionDigest,
};
use async_graphql::*;
use sui_json_rpc_types::{
    SuiExecutionStatus, SuiTransactionBlockDataAPI, SuiTransactionBlockEffects,
    SuiTransactionBlockEffectsAPI, SuiTransactionBlockResponse,
};

#[derive(SimpleObject, Clone, Eq, PartialEq)]
#[graphql(complex)]
pub(crate) struct TransactionBlock {
    pub digest: TransactionDigest,
    pub effects: Option<TransactionBlockEffects>,
    pub sender: Option<Address>,
    pub bcs: Option<Base64>,
    pub gas_input: Option<GasInput>,
}

impl From<SuiTransactionBlockResponse> for TransactionBlock {
    fn from(tx_block: sui_json_rpc_types::SuiTransactionBlockResponse) -> Self {
        let transaction = tx_block.transaction.as_ref();
        let sender = transaction.map(|tx| Address {
            address: SuiAddress::from_array(tx.data.sender().to_inner()),
        });
        let gas_input = transaction.map(|tx| GasInput::from(tx.data.gas_data()));

        Self {
            digest: TransactionDigest::from_array(tx_block.digest.into_inner()),
            effects: tx_block.effects.as_ref().map(TransactionBlockEffects::from),
            sender,
            bcs: Some(Base64::from(&tx_block.raw_transaction)),
            gas_input,
        }
    }
}

#[ComplexObject]
impl TransactionBlock {
    async fn expiration(&self, ctx: &Context<'_>) -> Result<Option<Epoch>> {
        if self.effects.is_none() {
            return Ok(None);
        }
        let gcs = self.effects.as_ref().unwrap().gas_effects.gcs;
        let data_provider = ctx.data_provider();
        let system_state = data_provider.get_latest_sui_system_state().await?;
        let protocol_configs = data_provider.fetch_protocol_config(None).await?;
        let epoch = convert_to_epoch(gcs, &system_state, &protocol_configs)?;
        Ok(Some(epoch))
    }
}

#[derive(Clone, Eq, PartialEq, SimpleObject)]
#[graphql(complex)]
pub(crate) struct TransactionBlockEffects {
    pub digest: TransactionDigest,
    #[graphql(skip)]
    pub gas_effects: GasEffects,
    pub status: ExecutionStatus,
    pub errors: Option<String>,
    // pub transaction_block: TransactionBlock,
    // pub dependencies: Vec<TransactionBlock>,
    // pub lamport_version: Option<u64>,
    // pub object_reads: Vec<Object>,
    // pub object_changes: Vec<ObjectChange>,
    // pub balance_changes: Vec<BalanceChange>,
    // pub epoch: Epoch
    // pub checkpoint: Checkpoint
}

impl From<&SuiTransactionBlockEffects> for TransactionBlockEffects {
    fn from(tx_effects: &SuiTransactionBlockEffects) -> Self {
        let (status, errors) = match tx_effects.status() {
            SuiExecutionStatus::Success => (ExecutionStatus::Success, None),
            SuiExecutionStatus::Failure { error } => {
                (ExecutionStatus::Failure, Some(error.clone()))
            }
        };

        Self {
            digest: TransactionDigest::from_array(tx_effects.transaction_digest().into_inner()),
            gas_effects: GasEffects::from((tx_effects.gas_cost_summary(), tx_effects.gas_object())),
            status,
            errors,
        }
    }
}

#[ComplexObject]
impl TransactionBlockEffects {
    async fn gas_effects(&self) -> Option<GasEffects> {
        Some(self.gas_effects)
    }

    async fn epoch(&self, ctx: &Context<'_>) -> Result<Option<Epoch>> {
        let data_provider = ctx.data_provider();
        let system_state = data_provider.get_latest_sui_system_state().await?;
        let protocol_configs = data_provider.fetch_protocol_config(None).await?;
        let epoch = convert_to_epoch(self.gas_effects.gcs, &system_state, &protocol_configs)?;
        Ok(Some(epoch))
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub(crate) enum TransactionBlockKindInput {
    ProgrammableTx,
    SystemTx,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum ExecutionStatus {
    Success,
    Failure,
}

#[derive(InputObject)]
pub(crate) struct TransactionBlockFilter {
    package: Option<SuiAddress>,
    module: Option<String>,
    function: Option<String>,

    kind: Option<TransactionBlockKindInput>,
    checkpoint: Option<u64>,

    sign_address: Option<SuiAddress>,
    sent_address: Option<SuiAddress>,
    recv_address: Option<SuiAddress>,
    paid_address: Option<SuiAddress>,

    input_object: Option<SuiAddress>,
    changed_object: Option<SuiAddress>,
}
