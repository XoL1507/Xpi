// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::certificate_deny_config::CertificateDenyConfig;
use crate::genesis;
use crate::p2p::P2pConfig;
use crate::transaction_deny_config::TransactionDenyConfig;
use crate::Config;
use anyhow::Result;
use narwhal_config::Parameters as ConsensusParameters;
use once_cell::sync::OnceCell;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use std::usize;
use sui_keys::keypair_file::{read_authority_keypair_from_file, read_keypair_from_file};
use sui_protocol_config::SupportedProtocolVersions;
use sui_storage::object_store::ObjectStoreConfig;
use sui_types::base_types::{ObjectID, SuiAddress};
use sui_types::crypto::AuthorityPublicKeyBytes;
use sui_types::crypto::KeypairTraits;
use sui_types::crypto::NetworkKeyPair;
use sui_types::crypto::SuiKeyPair;
use sui_types::crypto::{get_key_pair_from_rng, AccountKeyPair, AuthorityKeyPair};
use sui_types::multiaddr::Multiaddr;

// Default max number of concurrent requests served
pub const DEFAULT_GRPC_CONCURRENCY_LIMIT: usize = 20000000000;

/// Default gas price of 100 Mist
pub const DEFAULT_VALIDATOR_GAS_PRICE: u64 = sui_types::transaction::DEFAULT_VALIDATOR_GAS_PRICE;

/// Default commission rate of 2%
pub const DEFAULT_COMMISSION_RATE: u64 = 200;

#[serde_as]
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct NodeConfig {
    #[serde(default = "default_authority_key_pair")]
    pub protocol_key_pair: AuthorityKeyPairWithPath,
    #[serde(default = "default_key_pair")]
    pub worker_key_pair: KeyPairWithPath,
    #[serde(default = "default_key_pair")]
    pub account_key_pair: KeyPairWithPath,
    #[serde(default = "default_key_pair")]
    pub network_key_pair: KeyPairWithPath,

    pub db_path: PathBuf,
    #[serde(default = "default_grpc_address")]
    pub network_address: Multiaddr,
    #[serde(default = "default_json_rpc_address")]
    pub json_rpc_address: SocketAddr,

    #[serde(default = "default_metrics_address")]
    pub metrics_address: SocketAddr,
    #[serde(default = "default_admin_interface_port")]
    pub admin_interface_port: u16,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub consensus_config: Option<ConsensusConfig>,

    // TODO: Remove this as it's no longer used.
    #[serde(default)]
    pub enable_event_processing: bool,

    #[serde(default = "default_enable_index_processing")]
    pub enable_index_processing: bool,

    #[serde(default)]
    pub grpc_load_shed: Option<bool>,

    #[serde(default = "default_concurrency_limit")]
    pub grpc_concurrency_limit: Option<usize>,

    #[serde(default)]
    pub p2p_config: P2pConfig,

    pub genesis: Genesis,

    #[serde(default = "default_authority_store_pruning_config")]
    pub authority_store_pruning_config: AuthorityStorePruningConfig,

    /// Size of the broadcast channel used for notifying other systems of end of epoch.
    ///
    /// If unspecified, this will default to `128`.
    #[serde(default = "default_end_of_epoch_broadcast_channel_capacity")]
    pub end_of_epoch_broadcast_channel_capacity: usize,

    #[serde(default)]
    pub checkpoint_executor_config: CheckpointExecutorConfig,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<MetricsConfig>,

    /// In a `sui-node` binary, this is set to SupportedProtocolVersions::SYSTEM_DEFAULT
    /// in sui-node/src/main.rs. It is present in the config so that it can be changed by tests in
    /// order to test protocol upgrades.
    #[serde(skip)]
    pub supported_protocol_versions: Option<SupportedProtocolVersions>,

    #[serde(default)]
    pub db_checkpoint_config: DBCheckpointConfig,

    #[serde(default)]
    pub indirect_objects_threshold: usize,

    #[serde(default)]
    pub expensive_safety_check_config: ExpensiveSafetyCheckConfig,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_service_package_address: Option<SuiAddress>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_service_registry_id: Option<ObjectID>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_service_reverse_registry_id: Option<ObjectID>,

    #[serde(default)]
    pub transaction_deny_config: TransactionDenyConfig,

    #[serde(default)]
    pub certificate_deny_config: CertificateDenyConfig,

    #[serde(default)]
    pub state_debug_dump_config: StateDebugDumpConfig,

    #[serde(default)]
    pub state_archive_config: StateArchiveConfig,
}

fn default_authority_store_pruning_config() -> AuthorityStorePruningConfig {
    AuthorityStorePruningConfig::default()
}

pub fn default_enable_index_processing() -> bool {
    true
}

fn default_grpc_address() -> Multiaddr {
    "/ip4/0.0.0.0/tcp/8080".parse().unwrap()
}
fn default_authority_key_pair() -> AuthorityKeyPairWithPath {
    AuthorityKeyPairWithPath::new(get_key_pair_from_rng::<AuthorityKeyPair, _>(&mut OsRng).1)
}

fn default_key_pair() -> KeyPairWithPath {
    KeyPairWithPath::new(
        get_key_pair_from_rng::<AccountKeyPair, _>(&mut OsRng)
            .1
            .into(),
    )
}

fn default_metrics_address() -> SocketAddr {
    use std::net::{IpAddr, Ipv4Addr};
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 9184)
}

pub fn default_admin_interface_port() -> u16 {
    1337
}

pub fn default_json_rpc_address() -> SocketAddr {
    use std::net::{IpAddr, Ipv4Addr};
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 9000)
}

pub fn default_websocket_address() -> Option<SocketAddr> {
    use std::net::{IpAddr, Ipv4Addr};
    Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 9001))
}

pub fn default_concurrency_limit() -> Option<usize> {
    Some(DEFAULT_GRPC_CONCURRENCY_LIMIT)
}

pub fn default_end_of_epoch_broadcast_channel_capacity() -> usize {
    128
}

pub fn bool_true() -> bool {
    true
}

impl Config for NodeConfig {}

impl NodeConfig {
    pub fn protocol_key_pair(&self) -> &AuthorityKeyPair {
        self.protocol_key_pair.authority_keypair()
    }

    pub fn worker_key_pair(&self) -> &NetworkKeyPair {
        match self.worker_key_pair.keypair() {
            SuiKeyPair::Ed25519(kp) => kp,
            other => panic!(
                "Invalid keypair type: {:?}, only Ed25519 is allowed for worker key",
                other
            ),
        }
    }

    pub fn network_key_pair(&self) -> &NetworkKeyPair {
        match self.network_key_pair.keypair() {
            SuiKeyPair::Ed25519(kp) => kp,
            other => panic!(
                "Invalid keypair type: {:?}, only Ed25519 is allowed for network key",
                other
            ),
        }
    }

    pub fn protocol_public_key(&self) -> AuthorityPublicKeyBytes {
        self.protocol_key_pair().public().into()
    }

    pub fn db_path(&self) -> PathBuf {
        self.db_path.join("live")
    }

    pub fn db_checkpoint_path(&self) -> PathBuf {
        self.db_path.join("db_checkpoints")
    }

    pub fn archive_path(&self) -> PathBuf {
        self.db_path.join("archive")
    }

    pub fn network_address(&self) -> &Multiaddr {
        &self.network_address
    }

    pub fn consensus_config(&self) -> Option<&ConsensusConfig> {
        self.consensus_config.as_ref()
    }

    pub fn genesis(&self) -> Result<&genesis::Genesis> {
        self.genesis.genesis()
    }

    pub fn sui_address(&self) -> SuiAddress {
        (&self.account_key_pair.keypair().public()).into()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct ConsensusConfig {
    pub address: Multiaddr,
    pub db_path: PathBuf,

    /// Optional alternative address preferentially used by a primary to talk to its own worker.
    /// For example, this could be used to connect to co-located workers over a private LAN address.
    pub internal_worker_address: Option<Multiaddr>,

    /// Maximum number of pending transactions to submit to consensus, including those
    /// in submission wait.
    /// Assuming 10_000 txn tps * 10 sec consensus latency = 100_000 inflight consensus txns,
    /// Default to 100_000.
    pub max_pending_transactions: Option<usize>,

    /// When defined caps the calculated submission position to the max_submit_position. Even if the
    /// is elected to submit from a higher position than this, it will "reset" to the max_submit_position.
    pub max_submit_position: Option<usize>,

    /// The submit delay step to consensus defined in milliseconds. When provided it will
    /// override the current back off logic otherwise the default backoff logic will be applied based
    /// on consensus latency estimates.
    pub submit_delay_step_override_millis: Option<u64>,

    pub narwhal_config: ConsensusParameters,
}

impl ConsensusConfig {
    pub fn address(&self) -> &Multiaddr {
        &self.address
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn max_pending_transactions(&self) -> usize {
        self.max_pending_transactions.unwrap_or(100_000)
    }

    pub fn submit_delay_step_override(&self) -> Option<Duration> {
        self.submit_delay_step_override_millis
            .map(Duration::from_millis)
    }

    pub fn narwhal_config(&self) -> &ConsensusParameters {
        &self.narwhal_config
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct CheckpointExecutorConfig {
    /// Upper bound on the number of checkpoints that can be concurrently executed
    ///
    /// If unspecified, this will default to `200`
    #[serde(default = "default_checkpoint_execution_max_concurrency")]
    pub checkpoint_execution_max_concurrency: usize,

    /// Number of seconds to wait for effects of a batch of transactions
    /// before logging a warning. Note that we will continue to retry
    /// indefinitely
    ///
    /// If unspecified, this will default to `10`.
    #[serde(default = "default_local_execution_timeout_sec")]
    pub local_execution_timeout_sec: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct ExpensiveSafetyCheckConfig {
    /// If enabled, at epoch boundary, we will check that the storage
    /// fund balance is always identical to the sum of the storage
    /// rebate of all live objects, and that the total SUI in the network remains
    /// the same.
    #[serde(default)]
    enable_epoch_sui_conservation_check: bool,

    /// If enabled, we will check that the total SUI in all input objects of a tx
    /// (both the Move part and the storage rebate) matches the total SUI in all
    /// output objects of the tx + gas fees
    #[serde(default)]
    enable_deep_per_tx_sui_conservation_check: bool,

    /// Disable epoch SUI conservation check even when we are running in debug mode.
    #[serde(default)]
    force_disable_epoch_sui_conservation_check: bool,

    /// If enabled, at epoch boundary, we will check that the accumulated
    /// live object state matches the end of epoch root state digest.
    #[serde(default)]
    enable_state_consistency_check: bool,

    /// Disable state consistency check even when we are running in debug mode.
    #[serde(default)]
    force_disable_state_consistency_check: bool,

    /// If enabled, we run the Move VM in paranoid mode, which provides protection
    /// against some (but not all) potential bugs in the bytecode verifier
    #[serde(default)]
    enable_move_vm_paranoid_checks: bool,
    // TODO: Add more expensive checks here
}

impl ExpensiveSafetyCheckConfig {
    pub fn new_enable_all() -> Self {
        Self {
            enable_epoch_sui_conservation_check: true,
            enable_deep_per_tx_sui_conservation_check: true,
            force_disable_epoch_sui_conservation_check: false,
            enable_state_consistency_check: true,
            force_disable_state_consistency_check: false,
            enable_move_vm_paranoid_checks: true,
        }
    }

    pub fn new_disable_all() -> Self {
        Self {
            enable_epoch_sui_conservation_check: false,
            enable_deep_per_tx_sui_conservation_check: false,
            force_disable_epoch_sui_conservation_check: true,
            enable_state_consistency_check: false,
            force_disable_state_consistency_check: true,
            enable_move_vm_paranoid_checks: false,
        }
    }

    pub fn enable_paranoid_checks(&mut self) {
        self.enable_move_vm_paranoid_checks = true
    }

    pub fn force_disable_epoch_sui_conservation_check(&mut self) {
        self.force_disable_epoch_sui_conservation_check = true;
    }

    pub fn enable_epoch_sui_conservation_check(&self) -> bool {
        (self.enable_epoch_sui_conservation_check || cfg!(debug_assertions))
            && !self.force_disable_epoch_sui_conservation_check
    }

    pub fn force_disable_state_consistency_check(&mut self) {
        self.force_disable_state_consistency_check = true;
    }

    pub fn enable_state_consistency_check(&self) -> bool {
        (self.enable_state_consistency_check || cfg!(debug_assertions))
            && !self.force_disable_state_consistency_check
    }

    pub fn enable_move_vm_paranoid_checks(&self) -> bool {
        self.enable_move_vm_paranoid_checks
    }

    pub fn enable_deep_per_tx_sui_conservation_check(&self) -> bool {
        self.enable_deep_per_tx_sui_conservation_check || cfg!(debug_assertions)
    }
}

fn default_checkpoint_execution_max_concurrency() -> usize {
    200
}

fn default_local_execution_timeout_sec() -> u64 {
    30
}

impl Default for CheckpointExecutorConfig {
    fn default() -> Self {
        Self {
            checkpoint_execution_max_concurrency: default_checkpoint_execution_max_concurrency(),
            local_execution_timeout_sec: default_local_execution_timeout_sec(),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct AuthorityStorePruningConfig {
    /// number of the latest epoch dbs to retain
    pub num_latest_epoch_dbs_to_retain: usize,
    /// time interval used by the pruner to determine whether there are any epoch DBs to remove
    pub epoch_db_pruning_period_secs: u64,
    /// number of epochs to keep the latest version of objects for.
    /// Note that a zero value corresponds to an aggressive pruner.
    /// This mode is experimental and needs to be used with caution.
    /// Use `u64::MAX` to disable the pruner for the objects.
    pub num_epochs_to_retain: u64,
    /// pruner's runtime interval used for aggressive mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pruning_run_delay_seconds: Option<u64>,
    /// maximum number of checkpoints in the pruning batch. Can be adjusted to increase performance
    pub max_checkpoints_in_batch: usize,
    /// maximum number of transaction in the pruning batch
    pub max_transactions_in_batch: usize,
    /// enables periodic background compaction for old SST files whose last modified time is
    /// older than `periodic_compaction_threshold_days` days.
    /// That ensures that all sst files eventually go through the compaction process
    #[serde(skip_serializing_if = "Option::is_none")]
    pub periodic_compaction_threshold_days: Option<usize>,
    /// number of epochs to keep the latest version of transactions and effects for
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_epochs_to_retain_for_checkpoints: Option<u64>,
}

impl Default for AuthorityStorePruningConfig {
    fn default() -> Self {
        // TODO: Remove this after aggressive pruning is enabled by default
        let num_epochs_to_retain = if cfg!(msim) { 0 } else { 2 };
        let pruning_run_delay_seconds = if cfg!(msim) { Some(5) } else { None };
        Self {
            num_latest_epoch_dbs_to_retain: usize::MAX,
            epoch_db_pruning_period_secs: u64::MAX,
            num_epochs_to_retain,
            pruning_run_delay_seconds,
            max_checkpoints_in_batch: 10,
            max_transactions_in_batch: 1000,
            periodic_compaction_threshold_days: None,
            num_epochs_to_retain_for_checkpoints: None,
        }
    }
}

impl AuthorityStorePruningConfig {
    pub fn validator_config() -> Self {
        // TODO: Remove this after aggressive pruning is enabled by default
        let num_epochs_to_retain = if cfg!(msim) { 0 } else { 2 };
        let pruning_run_delay_seconds = if cfg!(msim) { Some(2) } else { None };
        let num_epochs_to_retain_for_checkpoints = if cfg!(msim) { Some(1) } else { None };
        Self {
            num_latest_epoch_dbs_to_retain: 3,
            epoch_db_pruning_period_secs: 60 * 60,
            num_epochs_to_retain,
            pruning_run_delay_seconds,
            max_checkpoints_in_batch: 10,
            max_transactions_in_batch: 1000,
            periodic_compaction_threshold_days: None,
            num_epochs_to_retain_for_checkpoints,
        }
    }
    pub fn fullnode_config() -> Self {
        // TODO: Remove this after aggressive pruning is enabled by default
        let num_epochs_to_retain = if cfg!(msim) { 0 } else { 2 };
        let pruning_run_delay_seconds = if cfg!(msim) { Some(2) } else { None };
        let num_epochs_to_retain_for_checkpoints = if cfg!(msim) { Some(1) } else { None };
        Self {
            num_latest_epoch_dbs_to_retain: 3,
            epoch_db_pruning_period_secs: 60 * 60,
            num_epochs_to_retain,
            pruning_run_delay_seconds,
            max_checkpoints_in_batch: 10,
            max_transactions_in_batch: 1000,
            periodic_compaction_threshold_days: None,
            num_epochs_to_retain_for_checkpoints,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct MetricsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push_interval_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push_url: Option<String>,
}

#[derive(Default, Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct DBCheckpointConfig {
    #[serde(default)]
    pub perform_db_checkpoints_at_epoch_end: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_store_config: Option<ObjectStoreConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub perform_index_db_checkpoints_at_epoch_end: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prune_and_compact_before_upload: Option<bool>,
}

#[derive(Default, Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct StateArchiveConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_store_config: Option<ObjectStoreConfig>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Eq)]
pub struct Genesis {
    #[serde(flatten)]
    location: GenesisLocation,

    #[serde(skip)]
    genesis: once_cell::sync::OnceCell<genesis::Genesis>,
}

impl Genesis {
    pub fn new(genesis: genesis::Genesis) -> Self {
        Self {
            location: GenesisLocation::InPlace { genesis },
            genesis: Default::default(),
        }
    }

    pub fn new_from_file<P: Into<PathBuf>>(path: P) -> Self {
        Self {
            location: GenesisLocation::File {
                genesis_file_location: path.into(),
            },
            genesis: Default::default(),
        }
    }

    pub fn genesis(&self) -> Result<&genesis::Genesis> {
        match &self.location {
            GenesisLocation::InPlace { genesis } => Ok(genesis),
            GenesisLocation::File {
                genesis_file_location,
            } => self
                .genesis
                .get_or_try_init(|| genesis::Genesis::load(genesis_file_location)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Eq)]
#[serde(untagged)]
enum GenesisLocation {
    InPlace {
        genesis: genesis::Genesis,
    },
    File {
        #[serde(rename = "genesis-file-location")]
        genesis_file_location: PathBuf,
    },
}

/// Wrapper struct for SuiKeyPair that can be deserialized from a file path. Used by network, worker, and account keypair.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct KeyPairWithPath {
    #[serde(flatten)]
    location: KeyPairLocation,

    #[serde(skip)]
    keypair: OnceCell<Arc<SuiKeyPair>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Eq)]
#[serde_as]
#[serde(untagged)]
enum KeyPairLocation {
    InPlace {
        #[serde_as(as = "Arc<KeyPairBase64>")]
        value: Arc<SuiKeyPair>,
    },
    File {
        #[serde(rename = "path")]
        path: PathBuf,
    },
}

impl KeyPairWithPath {
    pub fn new(kp: SuiKeyPair) -> Self {
        let cell: OnceCell<Arc<SuiKeyPair>> = OnceCell::new();
        let arc_kp = Arc::new(kp);
        // OK to unwrap panic because authority should not start without all keypairs loaded.
        cell.set(arc_kp.clone()).expect("Failed to set keypair");
        Self {
            location: KeyPairLocation::InPlace { value: arc_kp },
            keypair: cell,
        }
    }

    pub fn new_from_path(path: PathBuf) -> Self {
        let cell: OnceCell<Arc<SuiKeyPair>> = OnceCell::new();
        // OK to unwrap panic because authority should not start without all keypairs loaded.
        cell.set(Arc::new(read_keypair_from_file(&path).unwrap_or_else(
            |e| panic!("Invalid keypair file at path {:?}: {e}", &path),
        )))
        .expect("Failed to set keypair");
        Self {
            location: KeyPairLocation::File { path },
            keypair: cell,
        }
    }

    pub fn keypair(&self) -> &SuiKeyPair {
        self.keypair
            .get_or_init(|| match &self.location {
                KeyPairLocation::InPlace { value } => value.clone(),
                KeyPairLocation::File { path } => {
                    // OK to unwrap panic because authority should not start without all keypairs loaded.
                    Arc::new(
                        read_keypair_from_file(path).unwrap_or_else(|e| {
                            panic!("Invalid keypair file at path {:?}: {e}", path)
                        }),
                    )
                }
            })
            .as_ref()
    }
}

/// Wrapper struct for AuthorityKeyPair that can be deserialized from a file path.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct AuthorityKeyPairWithPath {
    #[serde(flatten)]
    location: AuthorityKeyPairLocation,

    #[serde(skip)]
    keypair: OnceCell<Arc<AuthorityKeyPair>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Eq)]
#[serde_as]
#[serde(untagged)]
enum AuthorityKeyPairLocation {
    InPlace { value: Arc<AuthorityKeyPair> },
    File { path: PathBuf },
}

impl AuthorityKeyPairWithPath {
    pub fn new(kp: AuthorityKeyPair) -> Self {
        let cell: OnceCell<Arc<AuthorityKeyPair>> = OnceCell::new();
        let arc_kp = Arc::new(kp);
        // OK to unwrap panic because authority should not start without all keypairs loaded.
        cell.set(arc_kp.clone())
            .expect("Failed to set authority keypair");
        Self {
            location: AuthorityKeyPairLocation::InPlace { value: arc_kp },
            keypair: cell,
        }
    }

    pub fn new_from_path(path: PathBuf) -> Self {
        let cell: OnceCell<Arc<AuthorityKeyPair>> = OnceCell::new();
        // OK to unwrap panic because authority should not start without all keypairs loaded.
        cell.set(Arc::new(
            read_authority_keypair_from_file(&path)
                .unwrap_or_else(|_| panic!("Invalid authority keypair file at path {:?}", &path)),
        ))
        .expect("Failed to set authority keypair");
        Self {
            location: AuthorityKeyPairLocation::File { path },
            keypair: cell,
        }
    }

    pub fn authority_keypair(&self) -> &AuthorityKeyPair {
        self.keypair
            .get_or_init(|| match &self.location {
                AuthorityKeyPairLocation::InPlace { value } => value.clone(),
                AuthorityKeyPairLocation::File { path } => {
                    // OK to unwrap panic because authority should not start without all keypairs loaded.
                    Arc::new(
                        read_authority_keypair_from_file(path).unwrap_or_else(|_| {
                            panic!("Invalid authority keypair file {:?}", &path)
                        }),
                    )
                }
            })
            .as_ref()
    }
}

/// Configurations which determine how we dump state debug info.
/// Debug info is dumped when a node forks.
#[derive(Clone, Debug, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct StateDebugDumpConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dump_file_directory: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fastcrypto::traits::KeyPair;
    use rand::{rngs::StdRng, SeedableRng};
    use sui_keys::keypair_file::{write_authority_keypair_to_file, write_keypair_to_file};
    use sui_types::crypto::{get_key_pair_from_rng, AuthorityKeyPair, NetworkKeyPair, SuiKeyPair};

    use super::Genesis;
    use crate::NodeConfig;

    #[test]
    fn serialize_genesis_from_file() {
        let g = Genesis::new_from_file("path/to/file");

        let s = serde_yaml::to_string(&g).unwrap();
        assert_eq!("---\ngenesis-file-location: path/to/file\n", s);
        let loaded_genesis: Genesis = serde_yaml::from_str(&s).unwrap();
        assert_eq!(g, loaded_genesis);
    }

    #[test]
    fn fullnode_template() {
        const TEMPLATE: &str = include_str!("../data/fullnode-template.yaml");

        let _template: NodeConfig = serde_yaml::from_str(TEMPLATE).unwrap();
    }

    #[test]
    fn load_key_pairs_to_node_config() {
        let protocol_key_pair: AuthorityKeyPair =
            get_key_pair_from_rng(&mut StdRng::from_seed([0; 32])).1;
        let worker_key_pair: NetworkKeyPair =
            get_key_pair_from_rng(&mut StdRng::from_seed([0; 32])).1;
        let network_key_pair: NetworkKeyPair =
            get_key_pair_from_rng(&mut StdRng::from_seed([0; 32])).1;

        write_authority_keypair_to_file(&protocol_key_pair, PathBuf::from("protocol.key")).unwrap();
        write_keypair_to_file(
            &SuiKeyPair::Ed25519(worker_key_pair.copy()),
            PathBuf::from("worker.key"),
        )
        .unwrap();
        write_keypair_to_file(
            &SuiKeyPair::Ed25519(network_key_pair.copy()),
            PathBuf::from("network.key"),
        )
        .unwrap();

        const TEMPLATE: &str = include_str!("../data/fullnode-template-with-path.yaml");
        let template: NodeConfig = serde_yaml::from_str(TEMPLATE).unwrap();
        assert_eq!(
            template.protocol_key_pair().public(),
            protocol_key_pair.public()
        );
        assert_eq!(
            template.network_key_pair().public(),
            network_key_pair.public()
        );
        assert_eq!(
            template.worker_key_pair().public(),
            worker_key_pair.public()
        );
    }
}
