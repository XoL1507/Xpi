// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use sui_json_rpc_types::SuiExecuteTransactionResponse;
use sui_types::base_types::TransactionDigest;
use sui_types::messages::ExecuteTransactionRequestType;
use thiserror::Error;

pub type SuiRpcResult<T = ()> = Result<T, RpcError>;

#[derive(Error, Debug)]
pub enum RpcError {
    #[error(transparent)]
    RpcError(#[from] jsonrpsee::core::Error),
    #[error("Subscription error : {0}")]
    Subscription(String),
    #[error("Invalid response type {0:?} for request type: {1:?}")]
    InvalidTransactionResponse(SuiExecuteTransactionResponse, ExecuteTransactionRequestType),
    #[error("Encountered error when confirming tx status for {0:?}, err: {1:?}")]
    TransactionConfirmationError(TransactionDigest, jsonrpsee::core::Error),
    #[error("Failed to confirm tx status for {0:?} within {1} seconds.")]
    FailToConfirmTransactionStatus(TransactionDigest, u64),
    #[error("Data error: {0}")]
    DataError(String),
    #[error("Client/Server api version mismatch, client api version : {client_version}, server api version : {server_version}")]
    ServerVersionMismatch {
        client_version: String,
        server_version: String,
    },
}
