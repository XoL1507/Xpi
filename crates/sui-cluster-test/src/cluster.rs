// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
use super::config::{ClusterTestOpt, Env};
use async_trait::async_trait;
use clap::*;
use std::net::SocketAddr;
use sui::client_commands::WalletContext;
use sui::config::SuiClientConfig;
use sui_config::genesis_config::GenesisConfig;
use sui_config::Config;
use sui_config::SUI_KEYSTORE_FILENAME;
use sui_sdk::crypto::KeystoreType;
use sui_sdk::ClientType;
use sui_swarm::memory::Node;
use sui_swarm::memory::Swarm;
use sui_types::base_types::SuiAddress;
use sui_types::crypto::KeypairTraits;
use sui_types::crypto::SuiKeyPair;
use sui_types::crypto::{get_key_pair, AccountKeyPair};
use test_utils::network::{start_rpc_test_network_with_fullnode, TestNetwork};
use tracing::info;

const DEVNET_FAUCET_ADDR: &str = "https://faucet.devnet.sui.io:443";
const STAGING_FAUCET_ADDR: &str = "https://faucet.staging.sui.io:443";
const CONTINUOUS_FAUCET_ADDR: &str = "https://faucet.ci.sui.io:443";
const DEVNET_GATEWAY_ADDR: &str = "https://gateway.devnet.sui.io:443";
const STAGING_GATEWAY_ADDR: &str = "https://gateway.staging.sui.io:443";
const CONTINUOUS_GATEWAY_ADDR: &str = "https://gateway.ci.sui.io:443";
const DEVNET_FULLNODE_ADDR: &str = "https://fullnode.devnet.sui.io:443";
const STAGING_FULLNODE_ADDR: &str = "https://fullnode.staging.sui.io:443";
const CONTINUOUS_FULLNODE_ADDR: &str = "https://fullnode.ci.sui.io:443";

pub struct ClusterFactory;

impl ClusterFactory {
    pub async fn start(
        options: &ClusterTestOpt,
    ) -> Result<Box<dyn Cluster + Sync + Send>, anyhow::Error> {
        Ok(match &options.env {
            Env::NewLocal => Box::new(LocalNewCluster::start(options).await?),
            _ => Box::new(RemoteRunningCluster::start(options).await?),
        })
    }
}

/// Cluster Abstraction
#[async_trait]
pub trait Cluster {
    async fn start(options: &ClusterTestOpt) -> Result<Self, anyhow::Error>
    where
        Self: Sized;

    fn rpc_url(&self) -> &str;
    fn fullnode_url(&self) -> &str;
    fn websocket_url(&self) -> Option<&str>;
    fn user_key(&self) -> AccountKeyPair;

    /// Returns faucet url in a remote cluster.
    fn remote_faucet_url(&self) -> Option<&str>;

    /// Returns faucet key in a local cluster.
    fn local_faucet_key(&self) -> Option<&AccountKeyPair>;
}

/// Represents an up and running cluster deployed remotely.
pub struct RemoteRunningCluster {
    rpc_url: String,
    faucet_url: String,
    fullnode_url: String,
}

#[async_trait]
impl Cluster for RemoteRunningCluster {
    async fn start(options: &ClusterTestOpt) -> Result<Self, anyhow::Error> {
        let (rpc_url, faucet_url, fullnode_url) = match options.env {
            Env::DevNet => (
                String::from(DEVNET_GATEWAY_ADDR),
                String::from(DEVNET_FAUCET_ADDR),
                String::from(DEVNET_FULLNODE_ADDR),
            ),
            Env::Staging => (
                String::from(STAGING_GATEWAY_ADDR),
                String::from(STAGING_FAUCET_ADDR),
                String::from(STAGING_FULLNODE_ADDR),
            ),
            Env::Continuous => (
                String::from(CONTINUOUS_GATEWAY_ADDR),
                String::from(CONTINUOUS_FAUCET_ADDR),
                String::from(CONTINUOUS_FULLNODE_ADDR),
            ),
            Env::CustomRemote => (
                options
                    .gateway_address
                    .clone()
                    .expect("Expect 'gateway_address' for Env::Custom"),
                options
                    .faucet_address
                    .clone()
                    .expect("Expect 'faucet_address' for Env::Custom"),
                options
                    .fullnode_address
                    .clone()
                    .expect("Expect 'fullnode_address' for Env::Custom"),
            ),
            Env::NewLocal => unreachable!("NewLocal shouldn't use RemoteRunningCluster"),
        };

        // TODO: test connectivity before proceeding?

        Ok(Self {
            rpc_url,
            faucet_url,
            fullnode_url,
        })
    }
    fn rpc_url(&self) -> &str {
        &self.rpc_url
    }
    fn fullnode_url(&self) -> &str {
        &self.fullnode_url
    }
    fn websocket_url(&self) -> Option<&str> {
        None
    }
    fn user_key(&self) -> AccountKeyPair {
        get_key_pair().1
    }
    fn remote_faucet_url(&self) -> Option<&str> {
        Some(&self.faucet_url)
    }
    fn local_faucet_key(&self) -> Option<&AccountKeyPair> {
        None
    }
}

/// Represents a local Cluster which starts per cluster test run.
pub struct LocalNewCluster {
    test_network: TestNetwork,
    fullnode_url: String,
    faucet_key: AccountKeyPair,
    websocket_url: Option<String>,
}

impl LocalNewCluster {
    #[allow(unused)]
    pub fn swarm(&self) -> &Swarm {
        &self.test_network.network
    }
}

#[async_trait]
impl Cluster for LocalNewCluster {
    async fn start(options: &ClusterTestOpt) -> Result<Self, anyhow::Error> {
        // Let the faucet account hold 1000 gas objects on genesis
        let genesis_config = GenesisConfig::custom_genesis(4, 1, 1000);

        let gateway_port = options.gateway_address.as_ref().map(|addr| {
            addr.parse::<SocketAddr>()
                .expect("Unable to parse gateway address")
                .port()
        });

        let fullnode_port = options.fullnode_address.as_ref().map(|addr| {
            addr.parse::<SocketAddr>()
                .expect("Unable to parse fullnode address")
                .port()
        });

        let websocket_port = options.websocket_address.as_ref().map(|addr| {
            addr.parse::<SocketAddr>()
                .expect("Unable to parse fullnode address")
                .port()
        });

        let mut test_network = start_rpc_test_network_with_fullnode(
            Some(genesis_config),
            1,
            gateway_port,
            fullnode_port,
            websocket_port,
        )
        .await
        .unwrap_or_else(|e| panic!("Failed to start a local network, e: {e}"));

        // Use the wealthy account for faucet
        let faucet_key = test_network
            .network
            .config_mut()
            .account_keys
            .swap_remove(0);
        let faucet_address = SuiAddress::from(faucet_key.public());
        info!(?faucet_address, "faucet_address");

        let fullnode: &Node = test_network
            .network
            .fullnodes()
            .next()
            .expect("Expect one fullnode");
        let fullnode_url = format!("http://{}", fullnode.json_rpc_address());

        // Let nodes connect to one another
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // TODO: test connectivity before proceeding?
        Ok(Self {
            test_network,
            fullnode_url,
            faucet_key,
            websocket_url: options.websocket_address.clone(),
        })
    }

    fn rpc_url(&self) -> &str {
        &self.test_network.rpc_url
    }

    fn fullnode_url(&self) -> &str {
        &self.fullnode_url
    }

    fn websocket_url(&self) -> Option<&str> {
        self.websocket_url.as_deref()
    }

    fn user_key(&self) -> AccountKeyPair {
        get_key_pair().1
    }

    fn remote_faucet_url(&self) -> Option<&str> {
        None
    }

    fn local_faucet_key(&self) -> Option<&AccountKeyPair> {
        Some(&self.faucet_key)
    }
}

// Make linter happy
#[async_trait]
impl Cluster for Box<dyn Cluster + Send + Sync> {
    async fn start(_options: &ClusterTestOpt) -> Result<Self, anyhow::Error> {
        unreachable!(
            "If we already have a boxed Cluster trait object we wouldn't have to call this function"
        );
    }
    fn rpc_url(&self) -> &str {
        (**self).rpc_url()
    }

    fn fullnode_url(&self) -> &str {
        (**self).fullnode_url()
    }

    fn websocket_url(&self) -> Option<&str> {
        (**self).websocket_url()
    }

    fn user_key(&self) -> AccountKeyPair {
        (**self).user_key()
    }

    fn remote_faucet_url(&self) -> Option<&str> {
        (**self).remote_faucet_url()
    }

    fn local_faucet_key(&self) -> Option<&AccountKeyPair> {
        (**self).local_faucet_key()
    }
}

pub async fn new_wallet_context_from_cluster(
    cluster: &(dyn Cluster + Sync + Send),
    key_pair: AccountKeyPair,
) -> WalletContext {
    let temp_dir = tempfile::tempdir().unwrap();
    let wallet_config_path = temp_dir.path().join("client.yaml");
    let rpc_url = cluster.rpc_url();
    info!("Use gateway: {}", &rpc_url);
    let keystore_path = temp_dir.path().join(SUI_KEYSTORE_FILENAME);
    let keystore = KeystoreType::File(keystore_path);
    let address: SuiAddress = key_pair.public().into();
    keystore
        .init()
        .unwrap()
        .add_key(SuiKeyPair::Ed25519SuiKeyPair(key_pair))
        .unwrap();
    SuiClientConfig {
        keystore,
        client_type: ClientType::RPC(rpc_url.into(), None),
        active_address: Some(address),
    }
    .persisted(&wallet_config_path)
    .save()
    .unwrap();

    info!(
        "Initialize wallet from config path: {:?}",
        wallet_config_path
    );

    WalletContext::new(&wallet_config_path)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "Failed to init wallet context from path {:?}, error: {e}",
                wallet_config_path
            )
        })
}
