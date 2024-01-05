// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use jsonrpsee::core::RpcResult;
use jsonrpsee_proc_macros::rpc;

use sui_json_rpc_types::{
    CheckpointedObjectID, EpochInfo, EpochPage, MoveCallMetrics, NetworkMetrics, QueryObjectsPage,
    SuiObjectResponseQuery,
};
use sui_open_rpc_macros::open_rpc;
use sui_types::base_types::EpochId;

#[open_rpc(namespace = "suix", tag = "Extended API")]
#[rpc(server, client, namespace = "suix")]
pub trait ExtendedApi {
    /// Return a list of epoch info
    #[method(name = "getEpochs")]
    async fn get_epochs(
        &self,
        /// optional paging cursor
        cursor: Option<EpochId>,
        /// maximum number of items per page
        limit: Option<usize>,
        /// flag to return results in descending order
        descending_order: Option<bool>,
    ) -> RpcResult<EpochPage>;

    /// Return current epoch info
    #[method(name = "getCurrentEpoch")]
    async fn get_current_epoch(&self) -> RpcResult<EpochInfo>;

    /// Return the list of queried objects. Note that this is an enhanced full node only api.
    #[method(name = "queryObjects")]
    async fn query_objects(
        &self,
        /// the objects query criteria.
        query: SuiObjectResponseQuery,
        /// An optional paging cursor. If provided, the query will start from the next item after the specified cursor. Default to start from the first item if not specified.
        cursor: Option<CheckpointedObjectID>,
        /// Max number of items returned per page, default to [QUERY_MAX_RESULT_LIMIT_OBJECTS] if not specified.
        limit: Option<usize>,
    ) -> RpcResult<QueryObjectsPage>;

    /// Return Network metrics
    #[method(name = "getNetworkMetrics")]
    async fn get_network_metrics(&self) -> RpcResult<NetworkMetrics>;

    /// Return Network metrics
    #[method(name = "getMoveCallMetrics")]
    async fn get_move_call_metrics(&self) -> RpcResult<MoveCallMetrics>;
}
