// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use jsonrpsee::http_client::{HttpClient, HttpClientBuilder};

use crate::QuorumDriverApiClient;
use crate::RpcFullNodeReadApiClient;
use crate::RpcReadApiClient;
use crate::RpcTransactionBuilderClient;

pub struct SuiRpcClient {
    client: HttpClient,
}

impl SuiRpcClient {
    pub fn new(server_url: &str) -> Result<Self, anyhow::Error> {
        let client = HttpClientBuilder::default().build(server_url)?;
        Ok(Self { client })
    }

    pub fn read_api(&self) -> &impl RpcReadApiClient {
        &self.client
    }
    pub fn quorum_driver(&self) -> &impl QuorumDriverApiClient {
        &self.client
    }
    pub fn full_node_read_api(&self) -> &impl RpcFullNodeReadApiClient {
        &self.client
    }
    pub fn transaction_builder(&self) -> &impl RpcTransactionBuilderClient {
        &self.client
    }
}
