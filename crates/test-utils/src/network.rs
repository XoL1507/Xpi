// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use jsonrpsee_http_client::{HttpClient, HttpClientBuilder};
use std::net::SocketAddr;
use std::num::NonZeroUsize;
use std::path::Path;
use sui::{
    client_commands::{SuiClientCommands, WalletContext},
    config::SuiClientConfig,
};
use sui_config::gateway::GatewayConfig;
use sui_config::genesis_config::GenesisConfig;
use sui_config::{Config, SUI_CLIENT_CONFIG, SUI_GATEWAY_CONFIG, SUI_NETWORK_CONFIG};
use sui_config::{PersistedConfig, SUI_KEYSTORE_FILENAME};
use sui_core::gateway_state::GatewayState;
use sui_json_rpc::api::RpcGatewayApiServer;
use sui_json_rpc::api::RpcReadApiServer;
use sui_json_rpc::api::RpcTransactionBuilderServer;
use sui_json_rpc::api::WalletSyncApiServer;
use sui_json_rpc::gateway_api::{
    GatewayReadApiImpl, GatewayWalletSyncApiImpl, RpcGatewayImpl, TransactionBuilderImpl,
};
use sui_json_rpc::http_server::{HttpServerBuilder, HttpServerHandle, RpcModule};
use sui_sdk::crypto::KeystoreType;
use sui_sdk::{ClientType, SuiClient};
use sui_swarm::memory::{Swarm, SwarmBuilder};
use sui_types::base_types::SuiAddress;
use sui_types::crypto::KeypairTraits;
use sui_types::crypto::SuiKeyPair::Ed25519SuiKeyPair;
const NUM_VALIDAOTR: usize = 4;

pub async fn start_test_network(
    genesis_config: Option<GenesisConfig>,
) -> Result<Swarm, anyhow::Error> {
    start_test_network_with_fullnodes(genesis_config, 0).await
}

pub async fn start_test_network_with_fullnodes(
    genesis_config: Option<GenesisConfig>,
    fullnode_count: usize,
) -> Result<Swarm, anyhow::Error> {
    let mut builder: SwarmBuilder = Swarm::builder()
        .committee_size(NonZeroUsize::new(NUM_VALIDAOTR).unwrap())
        .with_fullnode_count(fullnode_count);
    if let Some(genesis_config) = genesis_config {
        builder = builder.initial_accounts_config(genesis_config);
    }

    let mut swarm = builder.build();
    swarm.launch().await?;

    let dir = swarm.dir();

    let network_path = dir.join(SUI_NETWORK_CONFIG);
    let wallet_path = dir.join(SUI_CLIENT_CONFIG);
    let keystore_path = dir.join(SUI_KEYSTORE_FILENAME);
    let db_folder_path = dir.join("client_db");
    let gateway_path = dir.join(SUI_GATEWAY_CONFIG);

    swarm.config().save(&network_path)?;
    let mut keystore = KeystoreType::File(keystore_path.clone()).init()?;
    for key in &swarm.config().account_keys {
        keystore.add_key(Ed25519SuiKeyPair(key.copy()))?;
    }

    let validators = swarm.config().validator_set().to_owned();
    let active_address = keystore.addresses().first().cloned();

    GatewayConfig {
        db_folder_path: db_folder_path.clone(),
        validator_set: validators.clone(),
        ..Default::default()
    }
    .save(gateway_path)?;

    // Create wallet config with stated authorities port
    SuiClientConfig {
        keystore: KeystoreType::File(keystore_path),
        gateway: ClientType::Embedded(GatewayConfig {
            db_folder_path,
            validator_set: validators,
            ..Default::default()
        }),
        active_address,
    }
    .save(&wallet_path)?;

    // Return network handle
    Ok(swarm)
}

// TODO make a buidler for this...
pub async fn setup_network_and_wallet() -> Result<(Swarm, WalletContext, SuiAddress), anyhow::Error>
{
    let swarm = start_test_network(None).await?;

    // Create Wallet context.
    let wallet_conf = swarm.dir().join(SUI_CLIENT_CONFIG);
    let mut context = WalletContext::new(&wallet_conf).await?;
    let address = context.keystore.addresses().first().cloned().unwrap();

    // Sync client to retrieve objects from the network.
    SuiClientCommands::SyncClientState {
        address: Some(address),
    }
    .execute(&mut context)
    .await?;
    Ok((swarm, context, address))
}

async fn start_rpc_gateway(
    config_path: &Path,
    port: Option<u16>,
) -> Result<(SocketAddr, HttpServerHandle), anyhow::Error> {
    let server_url = format!("127.0.0.1:{}", port.unwrap_or(0));
    let server = HttpServerBuilder::default().build(server_url).await?;
    let addr = server.local_addr()?;

    let config = PersistedConfig::read(config_path)?;
    let client = GatewayState::create_client(&config, None)?;
    let mut module = RpcModule::new(());
    module.merge(RpcGatewayImpl::new(client.clone()).into_rpc())?;
    module.merge(GatewayReadApiImpl::new(client.clone()).into_rpc())?;
    module.merge(TransactionBuilderImpl::new(client.clone()).into_rpc())?;
    module.merge(GatewayWalletSyncApiImpl::new(client.clone()).into_rpc())?;

    let handle = server.start(module)?;
    Ok((addr, handle))
}

pub async fn start_rpc_test_network(
    genesis_config: Option<GenesisConfig>,
) -> Result<TestNetwork, anyhow::Error> {
    start_rpc_test_network_with_fullnode(genesis_config, 0, None).await
}

pub async fn start_rpc_test_network_with_fullnode(
    genesis_config: Option<GenesisConfig>,
    fullnode_count: usize,
    gateway_port: Option<u16>,
) -> Result<TestNetwork, anyhow::Error> {
    let network = start_test_network_with_fullnodes(genesis_config, fullnode_count).await?;
    let working_dir = network.dir();
    let (server_addr, rpc_server_handle) =
        start_rpc_gateway(&working_dir.join(SUI_GATEWAY_CONFIG), gateway_port).await?;
    let mut wallet_conf: SuiClientConfig =
        PersistedConfig::read(&working_dir.join(SUI_CLIENT_CONFIG))?;
    let rpc_url = format!("http://{}", server_addr);
    let accounts = wallet_conf.keystore.init()?.addresses();
    wallet_conf.gateway = ClientType::RPC(rpc_url.clone());
    wallet_conf
        .persisted(&working_dir.join(SUI_CLIENT_CONFIG))
        .save()?;

    let http_client = HttpClientBuilder::default().build(rpc_url.clone())?;
    let gateway_client = SuiClient::new_http_client(&rpc_url)?;
    Ok(TestNetwork {
        network,
        _rpc_server: rpc_server_handle,
        accounts,
        http_client,
        gateway_client,
        rpc_url,
    })
}

pub struct TestNetwork {
    pub network: Swarm,
    _rpc_server: HttpServerHandle,
    pub accounts: Vec<SuiAddress>,
    pub http_client: HttpClient,
    pub gateway_client: SuiClient,
    pub rpc_url: String,
}
