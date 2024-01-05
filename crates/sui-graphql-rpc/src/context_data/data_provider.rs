// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::types::balance::Balance;
use crate::types::checkpoint::Checkpoint;
use crate::types::object::ObjectFilter;
use crate::types::protocol_config::ProtocolConfigs;
use crate::types::{object::Object, sui_address::SuiAddress};
use async_graphql::connection::Connection;
use async_graphql::*;
use async_trait::async_trait;
use sui_json_rpc_types::SuiObjectDataOptions;
use sui_sdk::types::base_types::ObjectID;
use sui_sdk::types::sui_system_state::sui_system_state_summary::SuiSystemStateSummary;

#[async_trait]
pub(crate) trait DataProvider: Send + Sync {
    async fn fetch_obj(&self, address: SuiAddress, version: Option<u64>) -> Result<Option<Object>>;

    async fn fetch_owned_objs(
        &self,
        owner: &SuiAddress,
        first: Option<u64>,
        after: Option<String>,
        last: Option<u64>,
        before: Option<String>,
        _filter: Option<ObjectFilter>,
    ) -> Result<Connection<String, Object>>;

    async fn get_object_with_options(
        &self,
        object_id: ObjectID,
        options: SuiObjectDataOptions,
    ) -> Result<Option<Object>>;

    async fn multi_get_object_with_options(
        &self,
        object_ids: Vec<ObjectID>,
        options: SuiObjectDataOptions,
    ) -> Result<Vec<Object>>;

    async fn fetch_balance(&self, address: &SuiAddress, type_: Option<String>) -> Result<Balance>;

    async fn fetch_balance_connection(
        &self,
        address: &SuiAddress,
        first: Option<u64>,
        after: Option<String>,
        last: Option<u64>,
        before: Option<String>,
    ) -> Result<Connection<String, Balance>>;

    async fn fetch_checkpoint_connection(
        &self,
        first: Option<u64>,
        after: Option<String>,
        last: Option<u64>,
        before: Option<String>,
    ) -> Result<Connection<String, Checkpoint>>;

    async fn fetch_chain_id(&self) -> Result<String>;

    async fn fetch_protocol_config(&self, version: Option<u64>) -> Result<ProtocolConfigs>;

    async fn get_latest_sui_system_state(&self) -> Result<SuiSystemStateSummary>;
}
