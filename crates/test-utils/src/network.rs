// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use futures::future::join_all;
use std::net::SocketAddr;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use jsonrpsee::http_client::{HttpClient, HttpClientBuilder};
use jsonrpsee::ws_client::WsClient;
use jsonrpsee::ws_client::WsClientBuilder;
use rand::{distributions::*, rngs::OsRng, seq::SliceRandom};
use sui_config::node::DBCheckpointConfig;
use sui_config::{Config, SUI_CLIENT_CONFIG, SUI_NETWORK_CONFIG};
use sui_config::{NodeConfig, PersistedConfig, SUI_KEYSTORE_FILENAME};
use sui_json_rpc_types::SuiTransactionBlockResponse;
use sui_keys::keystore::{AccountKeystore, FileBasedKeystore, Keystore};
use sui_node::SuiNodeHandle;
use sui_protocol_config::{ProtocolVersion, SupportedProtocolVersions};
use sui_sdk::sui_client_config::{SuiClientConfig, SuiEnv};
use sui_sdk::wallet_context::WalletContext;
use sui_sdk::{SuiClient, SuiClientBuilder};
use sui_swarm::memory::{Swarm, SwarmBuilder};
use sui_swarm_config::genesis_config::{
    AccountConfig, GenesisConfig, ValidatorGenesisConfig, DEFAULT_GAS_AMOUNT,
};
use sui_swarm_config::network_config_builder::{
    ProtocolVersionsConfig, SupportedProtocolVersionsCallback,
};
use sui_swarm_config::node_config_builder::{FullnodeConfigBuilder, ValidatorConfigBuilder};
use sui_test_transaction_builder::TestTransactionBuilder;
use sui_types::base_types::{AuthorityName, ObjectID, ObjectRef, SuiAddress};
use sui_types::committee::EpochId;
use sui_types::crypto::KeypairTraits;
use sui_types::crypto::SuiKeyPair;
use sui_types::governance::MIN_VALIDATOR_JOINING_STAKE_MIST;
use sui_types::object::Object;
use sui_types::sui_system_state::epoch_start_sui_system_state::EpochStartSystemStateTrait;
use sui_types::sui_system_state::SuiSystemState;
use sui_types::sui_system_state::SuiSystemStateTrait;
use sui_types::transaction::{TransactionData, VerifiedTransaction};
use tokio::time::{timeout, Instant};
use tokio::{task::JoinHandle, time::sleep};
use tracing::info;

const NUM_VALIDATOR: usize = 4;

pub struct FullNodeHandle {
    pub sui_node: SuiNodeHandle,
    pub sui_client: SuiClient,
    pub rpc_client: HttpClient,
    pub rpc_url: String,
    pub ws_client: WsClient,
    pub ws_url: String,
}

impl FullNodeHandle {
    pub async fn new(sui_node: SuiNodeHandle, json_rpc_address: SocketAddr) -> Self {
        let rpc_url = format!("http://{}", json_rpc_address);
        let rpc_client = HttpClientBuilder::default().build(&rpc_url).unwrap();

        let ws_url = format!("ws://{}", json_rpc_address);
        let ws_client = WsClientBuilder::default().build(&ws_url).await.unwrap();
        let sui_client = SuiClientBuilder::default()
            .ws_url(&ws_url)
            .build(&rpc_url)
            .await
            .unwrap();

        Self {
            sui_node,
            sui_client,
            rpc_client,
            rpc_url,
            ws_client,
            ws_url,
        }
    }
}

pub struct TestCluster {
    pub swarm: Swarm,
    pub wallet: WalletContext,
    pub fullnode_handle: FullNodeHandle,
}

impl TestCluster {
    pub fn rpc_client(&self) -> &HttpClient {
        &self.fullnode_handle.rpc_client
    }

    pub fn sui_client(&self) -> &SuiClient {
        &self.fullnode_handle.sui_client
    }

    pub fn rpc_url(&self) -> &str {
        &self.fullnode_handle.rpc_url
    }

    pub fn wallet_mut(&mut self) -> &mut WalletContext {
        &mut self.wallet
    }

    pub fn get_addresses(&self) -> Vec<SuiAddress> {
        self.wallet.get_addresses()
    }

    // Helper function to get the 0th address in WalletContext
    pub fn get_address_0(&self) -> SuiAddress {
        self.get_addresses()[0]
    }

    // Helper function to get the 1st address in WalletContext
    pub fn get_address_1(&self) -> SuiAddress {
        self.get_addresses()[1]
    }

    // Helper function to get the 2nd address in WalletContext
    pub fn get_address_2(&self) -> SuiAddress {
        self.get_addresses()[2]
    }

    pub fn fullnode_config_builder(&self) -> FullnodeConfigBuilder {
        self.swarm.get_fullnode_config_builder()
    }

    /// Convenience method to start a new fullnode in the test cluster.
    pub async fn spawn_new_fullnode(&mut self) -> FullNodeHandle {
        self.start_fullnode_from_config(
            self.fullnode_config_builder()
                .build(&mut OsRng, self.swarm.config()),
        )
        .await
    }

    pub async fn start_fullnode_from_config(&mut self, config: NodeConfig) -> FullNodeHandle {
        let json_rpc_address = config.json_rpc_address;
        let node = self.swarm.spawn_new_node(config).await;
        FullNodeHandle::new(node, json_rpc_address).await
    }

    pub fn all_node_handles(&self) -> Vec<SuiNodeHandle> {
        self.swarm
            .all_nodes()
            .map(|n| n.get_node_handle().unwrap())
            .collect()
    }

    pub fn get_validator_pubkeys(&self) -> Vec<AuthorityName> {
        self.swarm.validator_nodes().map(|v| v.name()).collect()
    }

    pub fn stop_node(&self, name: &AuthorityName) {
        self.swarm.node(name).unwrap().stop();
    }

    pub async fn stop_all_validators(&self) {
        info!("Stopping all validators in the cluster");
        self.swarm.active_validators().for_each(|v| v.stop());
        tokio::time::sleep(Duration::from_secs(3)).await;
    }

    pub async fn start_all_validators(&self) {
        info!("Starting all validators in the cluster");
        for v in self.swarm.validator_nodes() {
            if v.is_running() {
                continue;
            }
            v.start().await.unwrap();
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }

    pub async fn start_node(&self, name: &AuthorityName) {
        let node = self.swarm.node(name).unwrap();
        if node.is_running() {
            return;
        }
        node.start().await.unwrap();
    }

    pub async fn spawn_new_validator(
        &mut self,
        genesis_config: ValidatorGenesisConfig,
    ) -> SuiNodeHandle {
        let node_config = ValidatorConfigBuilder::new()
            .build(genesis_config, self.swarm.config().genesis.clone());
        self.swarm.spawn_new_node(node_config).await
    }

    pub fn random_node_restarter(self: &Arc<Self>) -> RandomNodeRestarter {
        RandomNodeRestarter::new(self.clone())
    }

    pub async fn get_reference_gas_price(&self) -> u64 {
        self.sui_client()
            .governance_api()
            .get_reference_gas_price()
            .await
            .expect("failed to get reference gas price")
    }

    pub async fn get_object_from_fullnode_store(&self, object_id: &ObjectID) -> Option<Object> {
        self.fullnode_handle
            .sui_node
            .with_async(|node| async { node.state().get_object(object_id).await.unwrap() })
            .await
    }

    pub async fn get_object_or_tombstone_from_fullnode_store(
        &self,
        object_id: ObjectID,
    ) -> ObjectRef {
        self.fullnode_handle
            .sui_node
            .state()
            .db()
            .get_object_or_tombstone(object_id)
            .unwrap()
            .unwrap()
    }

    /// To detect whether the network has reached such state, we use the fullnode as the
    /// source of truth, since a fullnode only does epoch transition when the network has
    /// done so.
    /// If target_epoch is specified, wait until the cluster reaches that epoch.
    /// If target_epoch is None, wait until the cluster reaches the next epoch.
    /// Note that this function does not guarantee that every node is at the target epoch.
    pub async fn wait_for_epoch(&self, target_epoch: Option<EpochId>) -> SuiSystemState {
        self.wait_for_epoch_with_timeout(target_epoch, Duration::from_secs(60))
            .await
    }

    pub async fn wait_for_epoch_with_timeout(
        &self,
        target_epoch: Option<EpochId>,
        timeout_dur: Duration,
    ) -> SuiSystemState {
        let mut epoch_rx = self
            .fullnode_handle
            .sui_node
            .with(|node| node.subscribe_to_epoch_change());
        timeout(timeout_dur, async move {
            while let Ok(system_state) = epoch_rx.recv().await {
                info!("received epoch {}", system_state.epoch());
                match target_epoch {
                    Some(target_epoch) if system_state.epoch() >= target_epoch => {
                        return system_state;
                    }
                    None => {
                        return system_state;
                    }
                    _ => (),
                }
            }
            unreachable!("Broken reconfig channel");
        })
        .await
        .expect("Timed out waiting for cluster to target epoch")
    }

    /// Ask 2f+1 validators to close epoch actively, and wait for the entire network to reach the next
    /// epoch. This requires waiting for both the fullnode and all validators to reach the next epoch.
    pub async fn trigger_reconfiguration(&self) {
        info!("Starting reconfiguration");
        let start = Instant::now();

        // Close epoch on 2f+1 validators.
        let cur_committee = self
            .fullnode_handle
            .sui_node
            .with(|node| node.state().clone_committee_for_testing());
        let mut cur_stake = 0;
        for node in self.swarm.active_validators() {
            node.get_node_handle()
                .unwrap()
                .with_async(|node| async {
                    node.close_epoch_for_testing().await.unwrap();
                    cur_stake += cur_committee.weight(&node.state().name);
                })
                .await;
            if cur_stake >= cur_committee.quorum_threshold() {
                break;
            }
        }
        info!("close_epoch complete after {:?}", start.elapsed());

        self.wait_for_epoch(Some(cur_committee.epoch + 1)).await;
        self.wait_for_epoch_all_nodes(cur_committee.epoch + 1).await;

        info!("reconfiguration complete after {:?}", start.elapsed());
    }

    pub async fn wait_for_epoch_all_nodes(&self, target_epoch: EpochId) {
        let handles: Vec<_> = self
            .swarm
            .all_nodes()
            .map(|node| node.get_node_handle().unwrap())
            .collect();
        let tasks: Vec<_> = handles
            .iter()
            .map(|handle| {
                handle.with_async(|node| async {
                    let mut retries = 0;
                    loop {
                        let epoch = node.state().epoch_store_for_testing().epoch();
                        if epoch == target_epoch {
                            break;
                        }
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        retries += 1;
                        if retries % 5 == 0 {
                            tracing::warn!(validator=?node.state().name.concise(), "Waiting for {:?} seconds to reach epoch {:?}. Currently at epoch {:?}", retries, target_epoch, epoch);
                        }
                    }
                })
            })
            .collect();

        timeout(Duration::from_secs(40), join_all(tasks))
            .await
            .expect("timed out waiting for reconfiguration to complete");
    }

    /// Upgrade the network protocol version, by restarting every validator with a new
    /// supported versions.
    /// Note that we don't restart the fullnode here, and it is assumed that the fulnode supports
    /// the entire version range.
    pub async fn update_validator_supported_versions(
        &mut self,
        new_supported_versions: SupportedProtocolVersions,
    ) {
        for authority in self.get_validator_pubkeys() {
            self.stop_node(&authority);
            tokio::time::sleep(Duration::from_millis(1000)).await;
            self.swarm
                .node_mut(&authority)
                .unwrap()
                .config
                .supported_protocol_versions = Some(new_supported_versions);
            self.start_node(&authority).await;
            info!("Restarted validator {}", authority);
        }
    }

    /// Wait for all nodes in the network to upgrade to `protocol_version`.
    pub async fn wait_for_all_nodes_upgrade_to(&self, protocol_version: u64) {
        for h in self.all_node_handles() {
            h.with_async(|node| async {
                while node
                    .state()
                    .epoch_store_for_testing()
                    .epoch_start_state()
                    .protocol_version()
                    .as_u64()
                    != protocol_version
                {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            })
            .await;
        }
    }

    pub async fn test_transaction_builder(&self) -> TestTransactionBuilder {
        let (sender, gas) = self.wallet.get_one_gas_object().await.unwrap().unwrap();
        let rgp = self.get_reference_gas_price().await;
        TestTransactionBuilder::new(sender, gas, rgp)
    }

    pub async fn sign_and_execute_transaction(
        &self,
        tx: &TransactionData,
    ) -> SuiTransactionBlockResponse {
        let signed_tx = self.wallet.sign_transaction(tx);
        self.execute_transaction(signed_tx).await
    }

    /// Execute a transaction on the network and wait for it to be executed on the rpc fullnode.
    /// Also expects the effects status to be ExecutionStatus::Success.
    pub async fn execute_transaction(
        &self,
        tx: VerifiedTransaction,
    ) -> SuiTransactionBlockResponse {
        self.wallet.execute_transaction_must_succeed(tx).await
    }

    #[cfg(msim)]
    pub fn set_safe_mode_expected(&self, value: bool) {
        for n in self.all_node_handles() {
            n.with(|node| node.set_safe_mode_expected(value));
        }
    }
}

pub struct RandomNodeRestarter {
    test_cluster: Arc<TestCluster>,

    // How frequently should we kill nodes
    kill_interval: Uniform<Duration>,
    // How long should we wait before restarting them.
    restart_delay: Uniform<Duration>,

    task_handle: Mutex<Option<JoinHandle<()>>>,
}

impl RandomNodeRestarter {
    fn new(test_cluster: Arc<TestCluster>) -> Self {
        Self {
            test_cluster,
            kill_interval: Uniform::new(Duration::from_secs(10), Duration::from_secs(11)),
            restart_delay: Uniform::new(Duration::from_secs(1), Duration::from_secs(2)),
            task_handle: Default::default(),
        }
    }

    pub fn with_kill_interval_secs(mut self, a: u64, b: u64) -> Self {
        self.kill_interval = Uniform::new(Duration::from_secs(a), Duration::from_secs(b));
        self
    }

    pub fn with_restart_delay_secs(mut self, a: u64, b: u64) -> Self {
        self.restart_delay = Uniform::new(Duration::from_secs(a), Duration::from_secs(b));
        self
    }

    pub fn run(&self) {
        let test_cluster = self.test_cluster.clone();
        let kill_interval = self.kill_interval;
        let restart_delay = self.restart_delay;
        let validators = self.test_cluster.get_validator_pubkeys();
        let mut task_handle = self.task_handle.lock().unwrap();
        assert!(task_handle.is_none());
        task_handle.replace(tokio::task::spawn(async move {
            loop {
                let delay = kill_interval.sample(&mut OsRng);
                info!("Sleeping {delay:?} before killing a validator");
                sleep(delay).await;

                let validator = validators.choose(&mut OsRng).unwrap();
                info!("Killing validator {:?}", validator.concise());
                test_cluster.stop_node(validator);

                let delay = restart_delay.sample(&mut OsRng);
                info!("Sleeping {delay:?} before restarting");
                sleep(delay).await;
                info!("Starting validator {:?}", validator.concise());
                test_cluster.start_node(validator).await;
            }
        }));
    }
}

impl Drop for RandomNodeRestarter {
    fn drop(&mut self) {
        if let Some(handle) = self.task_handle.lock().unwrap().take() {
            handle.abort();
        }
    }
}

pub struct TestClusterBuilder {
    genesis_config: Option<GenesisConfig>,
    additional_objects: Vec<Object>,
    num_validators: Option<usize>,
    fullnode_rpc_port: Option<u16>,
    enable_fullnode_events: bool,
    validator_supported_protocol_versions_config: ProtocolVersionsConfig,
    // Default to validator_supported_protocol_versions_config, but can be overridden.
    fullnode_supported_protocol_versions_config: Option<ProtocolVersionsConfig>,
    db_checkpoint_config_validators: DBCheckpointConfig,
    db_checkpoint_config_fullnodes: DBCheckpointConfig,
    num_unpruned_validators: Option<usize>,
}

impl TestClusterBuilder {
    pub fn new() -> Self {
        TestClusterBuilder {
            genesis_config: None,
            additional_objects: vec![],
            fullnode_rpc_port: None,
            num_validators: None,
            enable_fullnode_events: false,
            validator_supported_protocol_versions_config: ProtocolVersionsConfig::Default,
            fullnode_supported_protocol_versions_config: None,
            db_checkpoint_config_validators: DBCheckpointConfig::default(),
            db_checkpoint_config_fullnodes: DBCheckpointConfig::default(),
            num_unpruned_validators: None,
        }
    }

    pub fn with_fullnode_rpc_port(mut self, rpc_port: u16) -> Self {
        self.fullnode_rpc_port = Some(rpc_port);
        self
    }

    pub fn set_genesis_config(mut self, genesis_config: GenesisConfig) -> Self {
        assert!(self.genesis_config.is_none());
        self.genesis_config = Some(genesis_config);
        self
    }

    pub fn with_objects<I: IntoIterator<Item = Object>>(mut self, objects: I) -> Self {
        self.additional_objects.extend(objects);
        self
    }

    pub fn with_num_validators(mut self, num: usize) -> Self {
        self.num_validators = Some(num);
        self
    }

    pub fn enable_fullnode_events(mut self) -> Self {
        self.enable_fullnode_events = true;
        self
    }

    pub fn with_enable_db_checkpoints_validators(mut self) -> Self {
        self.db_checkpoint_config_validators = DBCheckpointConfig {
            perform_db_checkpoints_at_epoch_end: true,
            checkpoint_path: None,
            object_store_config: None,
            perform_index_db_checkpoints_at_epoch_end: None,
            prune_and_compact_before_upload: None,
        };
        self
    }

    pub fn with_enable_db_checkpoints_fullnodes(mut self) -> Self {
        self.db_checkpoint_config_fullnodes = DBCheckpointConfig {
            perform_db_checkpoints_at_epoch_end: true,
            checkpoint_path: None,
            object_store_config: None,
            perform_index_db_checkpoints_at_epoch_end: None,
            prune_and_compact_before_upload: Some(true),
        };
        self
    }

    pub fn with_epoch_duration_ms(mut self, epoch_duration_ms: u64) -> Self {
        self.get_or_init_genesis_config()
            .parameters
            .epoch_duration_ms = epoch_duration_ms;
        self
    }

    pub fn with_stake_subsidy_start_epoch(mut self, stake_subsidy_start_epoch: u64) -> Self {
        self.get_or_init_genesis_config()
            .parameters
            .stake_subsidy_start_epoch = stake_subsidy_start_epoch;
        self
    }

    pub fn with_supported_protocol_versions(mut self, c: SupportedProtocolVersions) -> Self {
        self.validator_supported_protocol_versions_config = ProtocolVersionsConfig::Global(c);
        self
    }

    pub fn with_fullnode_supported_protocol_versions_config(
        mut self,
        c: SupportedProtocolVersions,
    ) -> Self {
        self.fullnode_supported_protocol_versions_config = Some(ProtocolVersionsConfig::Global(c));
        self
    }

    pub fn with_protocol_version(mut self, v: ProtocolVersion) -> Self {
        self.get_or_init_genesis_config()
            .parameters
            .protocol_version = v;
        self
    }

    pub fn with_supported_protocol_version_callback(
        mut self,
        func: SupportedProtocolVersionsCallback,
    ) -> Self {
        self.validator_supported_protocol_versions_config =
            ProtocolVersionsConfig::PerValidator(func);
        self
    }

    pub fn with_validator_candidates(
        mut self,
        addresses: impl IntoIterator<Item = SuiAddress>,
    ) -> Self {
        self.get_or_init_genesis_config()
            .accounts
            .extend(addresses.into_iter().map(|address| AccountConfig {
                address: Some(address),
                gas_amounts: vec![DEFAULT_GAS_AMOUNT, MIN_VALIDATOR_JOINING_STAKE_MIST],
            }));
        self
    }

    pub fn with_num_unpruned_validators(mut self, n: usize) -> Self {
        self.num_unpruned_validators = Some(n);
        self
    }

    pub fn with_accounts(mut self, accounts: Vec<AccountConfig>) -> Self {
        self.get_or_init_genesis_config().accounts = accounts;
        self
    }

    pub async fn build(mut self) -> TestCluster {
        let swarm = self.start_swarm().await.unwrap();
        let working_dir = swarm.dir();

        let mut wallet_conf: SuiClientConfig =
            PersistedConfig::read(&working_dir.join(SUI_CLIENT_CONFIG)).unwrap();

        let fullnode = swarm.fullnodes().next().unwrap();
        let json_rpc_address = fullnode.config.json_rpc_address;
        let fullnode_handle =
            FullNodeHandle::new(fullnode.get_node_handle().unwrap(), json_rpc_address).await;

        wallet_conf.envs.push(SuiEnv {
            alias: "localnet".to_string(),
            rpc: fullnode_handle.rpc_url.clone(),
            ws: Some(fullnode_handle.ws_url.clone()),
        });
        wallet_conf.active_env = Some("localnet".to_string());

        wallet_conf
            .persisted(&working_dir.join(SUI_CLIENT_CONFIG))
            .save()
            .unwrap();

        let wallet_conf = swarm.dir().join(SUI_CLIENT_CONFIG);
        let wallet = WalletContext::new(&wallet_conf, None, None).await.unwrap();

        TestCluster {
            swarm,
            wallet,
            fullnode_handle,
        }
    }

    /// Start a Swarm and set up WalletConfig
    async fn start_swarm(&mut self) -> Result<Swarm, anyhow::Error> {
        let mut builder: SwarmBuilder = Swarm::builder()
            .committee_size(
                NonZeroUsize::new(self.num_validators.unwrap_or(NUM_VALIDATOR)).unwrap(),
            )
            .with_objects(self.additional_objects.clone())
            .with_db_checkpoint_config(self.db_checkpoint_config_validators.clone())
            .with_supported_protocol_versions_config(
                self.validator_supported_protocol_versions_config.clone(),
            )
            .with_fullnode_count(1)
            .with_fullnode_supported_protocol_versions_config(
                self.fullnode_supported_protocol_versions_config
                    .clone()
                    .unwrap_or(self.validator_supported_protocol_versions_config.clone()),
            )
            .with_db_checkpoint_config(self.db_checkpoint_config_fullnodes.clone());

        if let Some(genesis_config) = self.genesis_config.take() {
            builder = builder.with_genesis_config(genesis_config);
        }
        if let Some(fullnode_rpc_port) = self.fullnode_rpc_port {
            builder = builder.with_fullnode_rpc_port(fullnode_rpc_port);
        }
        if let Some(num_unpruned_validators) = self.num_unpruned_validators {
            builder = builder.with_num_unpruned_validators(num_unpruned_validators);
        }

        let mut swarm = builder.build();
        swarm.launch().await?;

        let dir = swarm.dir();

        let network_path = dir.join(SUI_NETWORK_CONFIG);
        let wallet_path = dir.join(SUI_CLIENT_CONFIG);
        let keystore_path = dir.join(SUI_KEYSTORE_FILENAME);

        swarm.config().save(&network_path)?;
        let mut keystore = Keystore::from(FileBasedKeystore::new(&keystore_path)?);
        for key in &swarm.config().account_keys {
            keystore.add_key(SuiKeyPair::Ed25519(key.copy()))?;
        }

        let active_address = keystore.addresses().first().cloned();

        // Create wallet config with stated authorities port
        SuiClientConfig {
            keystore: Keystore::from(FileBasedKeystore::new(&keystore_path)?),
            envs: Default::default(),
            active_address,
            active_env: Default::default(),
        }
        .save(wallet_path)?;

        // Return network handle
        Ok(swarm)
    }

    fn get_or_init_genesis_config(&mut self) -> &mut GenesisConfig {
        if self.genesis_config.is_none() {
            self.genesis_config = Some(GenesisConfig::for_local_testing());
        }
        self.genesis_config.as_mut().unwrap()
    }
}

impl Default for TestClusterBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// TODO: Merge the following functions with the ones inside TestCluster.
pub async fn wait_for_node_transition_to_epoch(node: &SuiNodeHandle, expected_epoch: EpochId) {
    node.with_async(|node| async move {
        let mut rx = node.subscribe_to_epoch_change();
        let epoch = node.current_epoch_for_testing();
        if epoch != expected_epoch {
            let system_state = rx.recv().await.unwrap();
            assert_eq!(system_state.epoch(), expected_epoch);
        }
    })
    .await
}

pub async fn wait_for_nodes_transition_to_epoch<'a>(
    nodes: impl Iterator<Item = &'a SuiNodeHandle>,
    expected_epoch: EpochId,
) {
    let handles: Vec<_> = nodes
        .map(|handle| wait_for_node_transition_to_epoch(handle, expected_epoch))
        .collect();
    join_all(handles).await;
}
