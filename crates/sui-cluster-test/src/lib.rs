// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
use async_trait::async_trait;
use clap::*;
use cluster::{Cluster, ClusterFactory};
use config::{ClusterTestOpt, Env};
use std::sync::Arc;
use sui::client_commands::WalletContext;
use sui_core::gateway_state::GatewayClient;

use crate::faucet::{Faucet, FaucetFactory};
use sui_json_rpc_types::TransactionResponse;
use sui_types::gas_coin::GasCoin;
use sui_types::{
    base_types::SuiAddress,
    messages::{Transaction, TransactionData},
};
use tokio::time::{sleep, Duration};
use tracing::{error, info};
use wallet_client::WalletClient;

pub mod cluster;
pub mod config;
pub mod faucet;
pub mod helper;
pub mod test_case;
pub mod wallet_client;

#[allow(unused)]
pub struct TestContext {
    /// The cluster env
    env: Env,
    /// Cluster handle that allows access to various components in a cluster
    cluster: Arc<dyn Cluster + Sync + Send>,
    /// Client that provides wallet context and gateway access
    /// Once we sunset gateway, we will spin off fullnode client,
    client: WalletClient,
    /// Facuet client that provides faucet access to a test
    faucet: Arc<dyn Faucet + Sync + Send>,
}

impl TestContext {
    async fn get_sui_from_faucet(&self, minimum_coins: Option<usize>) -> Vec<GasCoin> {
        self.faucet
            .request_sui_coins(self.get_context(), minimum_coins)
            .await
            .unwrap_or_else(|e| panic!("Failed to get test SUI coins from faucet, {e}"))
    }

    fn get_context(&self) -> &WalletClient {
        &self.client
    }

    fn get_gateway(&self) -> &GatewayClient {
        self.client.get_gateway()
    }

    fn get_fullnode(&self) -> &GatewayClient {
        self.client.get_fullnode()
    }

    fn get_wallet(&self) -> &WalletContext {
        self.client.get_wallet()
    }

    fn get_wallet_mut(&mut self) -> &mut WalletContext {
        self.client.get_wallet_mut()
    }

    fn get_wallet_address(&self) -> SuiAddress {
        self.client.get_wallet_address()
    }

    async fn sign_and_execute(&self, txn_data: TransactionData, desc: &str) -> TransactionResponse {
        let signature = self.get_context().sign(&txn_data, desc);
        self.get_gateway()
            .execute_transaction(Transaction::new(txn_data, signature))
            .await
            .unwrap_or_else(|e| panic!("Failed to execute transaction for {}. {}", desc, e))
    }

    pub async fn setup(options: ClusterTestOpt) -> Result<Self, anyhow::Error> {
        let cluster = ClusterFactory::start(&options).await?;
        let faucet_url = cluster.faucet_url().map(String::from);

        Ok(Self {
            env: options.env.clone(),
            cluster: cluster.clone(),
            client: WalletClient::new_from_cluster(cluster),
            faucet: FaucetFactory::create(&options, faucet_url),
        })
    }

    // TODO: figure out a more efficient way to test a local cluster
    // potentially by allowing retries with exponential back-off
    pub async fn let_fullnode_sync(&self) {
        let duration = Duration::from_secs(if let Env::NewLocal = self.env { 8 } else { 3 });
        sleep(duration).await;
    }
}

pub struct TestCase<'a> {
    test_case: Box<dyn TestCaseImpl + 'a>,
}

impl<'a> TestCase<'a> {
    pub fn new(test_case: impl TestCaseImpl + 'a) -> Self {
        TestCase {
            test_case: (Box::new(test_case)),
        }
    }

    pub async fn run(self, ctx: &mut TestContext) -> bool {
        let test_name = self.test_case.name();
        info!("Running test {}.", test_name);

        // TODO: unwind panic and fail gracefully?

        match self.test_case.run(ctx).await {
            Ok(()) => {
                info!("Test {test_name} succeeded.");
                true
            }
            Err(e) => {
                error!("Test {test_name} failed with error: {e}.");
                false
            }
        }
    }
}

#[async_trait]
pub trait TestCaseImpl {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    async fn run(&self, ctx: &mut TestContext) -> Result<(), anyhow::Error>;
}
