// Copyright (c) 2021, Facebook, Inc. and its affiliates
// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;
use fastcrypto::traits::ToFromBytes;
use futures::future::join_all;
use itertools::Itertools;
use std::collections::BTreeMap;
use std::fmt::Write;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use std::{fs, io};
use sui_config::{genesis::Genesis, NodeConfig};
use sui_core::authority_client::{AuthorityAPI, NetworkAuthorityClient};
use sui_network::default_mysten_network_config;
use sui_sdk::SuiClientBuilder;
use sui_types::crypto::AuthorityPublicKeyBytes;
use sui_types::multiaddr::Multiaddr;
use sui_types::object::ObjectFormatOptions;
use sui_types::{base_types::*, object::Owner};
use tokio::time::Instant;

use anyhow::anyhow;
use eyre::ContextCompat;
use indicatif::{ProgressBar, ProgressStyle};
use prometheus::Registry;
use sui_archival::reader::ArchiveReader;
use sui_core::authority::authority_store_tables::AuthorityPerpetualTables;
use sui_core::authority::AuthorityStore;
use sui_core::checkpoints::CheckpointStore;
use sui_core::epoch::committee_store::CommitteeStore;
use sui_core::storage::RocksDbStore;
use sui_storage::object_store::ObjectStoreConfig;
use sui_types::messages_grpc::{
    ObjectInfoRequest, ObjectInfoRequestKind, ObjectInfoResponse, TransactionInfoRequest,
    TransactionStatus,
};
use tracing::info;
use typed_store::rocks::MetricConf;

pub mod commands;
pub mod db_tool;

// This functions requires at least one of genesis or fullnode_rpc to be `Some`.
async fn make_clients(
    genesis: Option<PathBuf>,
    fullnode_rpc: Option<String>,
) -> Result<BTreeMap<AuthorityName, (Multiaddr, NetworkAuthorityClient)>> {
    let mut net_config = default_mysten_network_config();
    net_config.connect_timeout = Some(Duration::from_secs(5));
    let mut authority_clients = BTreeMap::new();

    if let Some(fullnode_rpc) = fullnode_rpc {
        let sui_client = SuiClientBuilder::default().build(fullnode_rpc).await?;
        let active_validators = sui_client
            .governance_api()
            .get_latest_sui_system_state()
            .await?
            .active_validators;

        for validator in active_validators {
            let net_addr = Multiaddr::try_from(validator.net_address).unwrap();
            let channel = net_config
                .connect_lazy(&net_addr)
                .map_err(|err| anyhow!(err.to_string()))?;
            let client = NetworkAuthorityClient::new(channel);
            let public_key_bytes =
                AuthorityPublicKeyBytes::from_bytes(&validator.protocol_pubkey_bytes)?;
            authority_clients.insert(public_key_bytes, (net_addr.clone(), client));
        }
    } else {
        if genesis.is_none() {
            return Err(anyhow!("Either genesis or fullnode_rpc must be specified"));
        }
        let genesis = Genesis::load(genesis.unwrap())?;
        for validator in genesis.validator_set_for_tooling() {
            let metadata = validator.verified_metadata();
            let channel = net_config
                .connect_lazy(&metadata.net_address)
                .map_err(|err| anyhow!(err.to_string()))?;
            let client = NetworkAuthorityClient::new(channel);
            let public_key_bytes = metadata.sui_pubkey_bytes();
            authority_clients.insert(public_key_bytes, (metadata.net_address.clone(), client));
        }
    }

    Ok(authority_clients)
}

type ObjectVersionResponses = Vec<(Option<SequenceNumber>, Result<ObjectInfoResponse>, f64)>;
pub struct ObjectData {
    requested_id: ObjectID,
    responses: Vec<(AuthorityName, Multiaddr, ObjectVersionResponses)>,
}

trait OptionDebug<T> {
    fn opt_debug(&self, def_str: &str) -> String;
}
trait OptionDisplay<T> {
    fn opt_display(&self, def_str: &str) -> String;
}

impl<T> OptionDebug<T> for Option<T>
where
    T: std::fmt::Debug,
{
    fn opt_debug(&self, def_str: &str) -> String {
        match self {
            None => def_str.to_string(),
            Some(t) => format!("{:?}", t),
        }
    }
}

impl<T> OptionDisplay<T> for Option<T>
where
    T: std::fmt::Display,
{
    fn opt_display(&self, def_str: &str) -> String {
        match self {
            None => def_str.to_string(),
            Some(t) => format!("{}", t),
        }
    }
}

struct OwnerOutput(Owner);

// grep/awk-friendly output for Owner
impl std::fmt::Display for OwnerOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Owner::AddressOwner(address) => {
                write!(f, "address({})", address)
            }
            Owner::ObjectOwner(address) => {
                write!(f, "object({})", address)
            }
            Owner::Immutable => {
                write!(f, "immutable")
            }
            Owner::Shared { .. } => {
                write!(f, "shared")
            }
        }
    }
}

pub struct GroupedObjectOutput(pub ObjectData);

#[allow(clippy::format_in_format_args)]
impl std::fmt::Display for GroupedObjectOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let responses = self
            .0
            .responses
            .iter()
            .flat_map(|(name, multiaddr, resp)| {
                resp.iter().map(|(seq_num, r, timespent)| {
                    (
                        *name,
                        multiaddr.clone(),
                        seq_num,
                        r,
                        timespent,
                        r.as_ref().err(),
                    )
                })
            })
            .sorted_by(|a, b| {
                Ord::cmp(&b.2, &a.2)
                    .then_with(|| Ord::cmp(&format!("{:?}", &b.5), &format!("{:?}", &a.5)))
            })
            .group_by(|(_, _, seq_num, _r, _ts, _)| **seq_num);
        for (seq_num, group) in &responses {
            writeln!(f, "seq num: {}", seq_num.opt_debug("latest-seq-num"))?;
            let cur_version_resp = group.group_by(|(_, _, _, r, _, _)| match r {
                Ok(result) => {
                    let parent_tx_digest = result.object.previous_transaction;
                    let obj_digest = result.object.compute_object_reference().2;
                    let lock = result
                        .lock_for_debugging
                        .as_ref()
                        .map(|lock| *lock.digest());
                    let owner = result.object.owner;
                    Some((parent_tx_digest, obj_digest, lock, owner))
                }
                Err(_) => None,
            });
            for (result, group) in &cur_version_resp {
                match result {
                    Some((parent_tx_digest, obj_digest, lock, owner)) => {
                        let lock = lock.opt_debug("no-known-lock");
                        writeln!(f, "obj ref: {obj_digest}")?;
                        writeln!(f, "parent tx: {parent_tx_digest}")?;
                        writeln!(f, "owner: {owner}")?;
                        writeln!(f, "lock: {lock}")?;
                        for (i, (name, multiaddr, _, _, timespent, _)) in group.enumerate() {
                            writeln!(
                                f,
                                "        {:<4} {:<20} {:<56} ({:.3}s)",
                                i,
                                name.concise(),
                                format!("{}", multiaddr),
                                timespent
                            )?;
                        }
                    }
                    None => {
                        writeln!(f, "ERROR")?;
                        for (i, (name, multiaddr, _, resp, timespent, _)) in group.enumerate() {
                            writeln!(
                                f,
                                "        {:<4} {:<20} {:<56} ({:.3}s) {:?}",
                                i,
                                name.concise(),
                                format!("{}", multiaddr),
                                timespent,
                                resp
                            )?;
                        }
                    }
                };
                writeln!(f, "{:<100}\n", "-".repeat(100))?;
            }
        }
        Ok(())
    }
}

struct ConciseObjectOutput(ObjectData);

impl ConciseObjectOutput {
    fn header() -> String {
        format!(
            "{:<20} {:<8} {:<66} {:<45} {}",
            "validator", "version", "digest", "parent_cert", "owner"
        )
    }
}

impl std::fmt::Display for ConciseObjectOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (name, _multi_addr, versions) in &self.0.responses {
            for (version, resp, _time_elapsed) in versions {
                write!(
                    f,
                    "{:<20} {:<8}",
                    format!("{:?}", name.concise()),
                    version.map(|s| s.value()).opt_debug("-")
                )?;
                match resp {
                    Err(_) => writeln!(
                        f,
                        "{:<66} {:<45} {:<51}",
                        "object-fetch-failed", "no-cert-available", "no-owner-available"
                    )?,
                    Ok(resp) => {
                        let obj_digest = resp.object.compute_object_reference().2;
                        let parent = resp.object.previous_transaction;
                        let owner = resp.object.owner;
                        write!(f, " {:<66} {:<45} {:<51}", obj_digest, parent, owner)?;
                    }
                }
                writeln!(f)?;
            }
        }
        Ok(())
    }
}

struct VerboseObjectOutput(ObjectData);

impl std::fmt::Display for VerboseObjectOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Object: {}", self.0.requested_id)?;

        for (name, multiaddr, versions) in &self.0.responses {
            writeln!(f, "validator: {:?}, addr: {:?}", name.concise(), multiaddr)?;

            for (version, resp, timespent) in versions {
                writeln!(
                    f,
                    "-- version: {} ({:.3}s)",
                    version.opt_debug("<version not available>"),
                    timespent,
                )?;

                match resp {
                    Err(e) => writeln!(f, "Error fetching object: {}", e)?,
                    Ok(resp) => {
                        writeln!(
                            f,
                            "  -- object digest: {}",
                            resp.object.compute_object_reference().2
                        )?;
                        if resp.object.is_package() {
                            writeln!(f, "  -- object: <Move Package>")?;
                        } else if let Some(layout) = &resp.layout {
                            writeln!(
                                f,
                                "  -- object: Move Object: {}",
                                resp.object
                                    .data
                                    .try_as_move()
                                    .unwrap()
                                    .to_move_struct(layout)
                                    .unwrap()
                            )?;
                        }
                        writeln!(f, "  -- owner: {}", resp.object.owner)?;
                        writeln!(
                            f,
                            "  -- locked by: {}",
                            resp.lock_for_debugging.opt_debug("<not locked>")
                        )?;
                    }
                }
            }
        }
        Ok(())
    }
}

pub async fn get_object(
    obj_id: ObjectID,
    version: Option<u64>,
    validator: Option<AuthorityName>,
    genesis: Option<PathBuf>,
    fullnode_rpc: Option<String>,
) -> Result<ObjectData> {
    let clients = make_clients(genesis, fullnode_rpc).await?;

    let responses = join_all(
        clients
            .iter()
            .filter(|(name, _)| {
                if let Some(v) = validator {
                    v == **name
                } else {
                    true
                }
            })
            .map(|(name, (address, client))| async {
                let object_versions = get_object_impl(client, obj_id, version).await;
                (*name, address.clone(), object_versions)
            }),
    )
    .await;

    Ok(ObjectData {
        requested_id: obj_id,
        responses,
    })
}

pub async fn get_transaction_block(
    tx_digest: TransactionDigest,
    genesis: Option<PathBuf>,
    show_input_tx: bool,
    fullnode_rpc: Option<String>,
) -> Result<String> {
    let clients = make_clients(genesis, fullnode_rpc).await?;
    let timer = Instant::now();
    let responses = join_all(clients.iter().map(|(name, (address, client))| async {
        let result = client
            .handle_transaction_info_request(TransactionInfoRequest {
                transaction_digest: tx_digest,
            })
            .await;
        (
            *name,
            address.clone(),
            result,
            timer.elapsed().as_secs_f64(),
        )
    }))
    .await;

    // Grab one validator that return Some(TransactionInfoResponse)
    let validator_aware_of_tx = responses.iter().find(|r| r.2.is_ok());

    let responses = responses
        .iter()
        .map(|r| {
            let key =
                r.2.as_ref()
                    .map(|ok_result| match &ok_result.status {
                        TransactionStatus::Signed(_) => None,
                        TransactionStatus::Executed(_, effects, _) => Some(effects.digest()),
                    })
                    .ok();
            let err = r.2.as_ref().err();
            (key, err, r)
        })
        .sorted_by(|(k1, err1, _), (k2, err2, _)| {
            Ord::cmp(k1, k2).then_with(|| Ord::cmp(err1, err2))
        })
        .group_by(|(_, _err, r)| {
            r.2.as_ref().map(|ok_result| match &ok_result.status {
                TransactionStatus::Signed(_) => None,
                TransactionStatus::Executed(_, effects, _) => Some((
                    ok_result.transaction.transaction_data(),
                    effects.data(),
                    effects.digest(),
                )),
            })
        });
    let mut s = String::new();
    for (i, (key, group)) in responses.into_iter().enumerate() {
        match key {
            Ok(Some((tx, effects, effects_digest))) => {
                writeln!(
                    &mut s,
                    "#{:<2} tx_digest: {:<68?} effects_digest: {:?}",
                    i, tx_digest, effects_digest,
                )?;
                writeln!(&mut s, "{:#?}", effects)?;
                if show_input_tx {
                    writeln!(&mut s, "{:#?}", tx)?;
                }
            }
            Ok(None) => {
                writeln!(
                    &mut s,
                    "#{:<2} tx_digest: {:<68?} Signed but not executed",
                    i, tx_digest
                )?;
                if show_input_tx {
                    // In this case, we expect at least one validator knows about this tx
                    let validator_aware_of_tx = validator_aware_of_tx.unwrap();
                    let client = &clients.get(&validator_aware_of_tx.0).unwrap().1;
                    let tx_info = client.handle_transaction_info_request(TransactionInfoRequest {
                        transaction_digest: tx_digest,
                    }).await.unwrap_or_else(|e| panic!("Validator {:?} should have known about tx_digest: {:?}, got error: {:?}", validator_aware_of_tx.0, tx_digest, e));
                    writeln!(&mut s, "{:#?}", tx_info)?;
                }
            }
            other => {
                writeln!(&mut s, "#{:<2} {:#?}", i, other)?;
            }
        }
        for (j, (_, _, res)) in group.enumerate() {
            writeln!(
                &mut s,
                "        {:<4} {:<20} {:<56} ({:.3}s)",
                j,
                res.0.concise(),
                format!("{}", res.1),
                res.3
            )?;
        }
        writeln!(&mut s, "{:<100}\n", "-".repeat(100))?;
    }
    Ok(s)
}

// Keep the return type a vector in case we need support for lamport versions in the near future
async fn get_object_impl(
    client: &NetworkAuthorityClient,
    id: ObjectID,
    version: Option<u64>,
) -> Vec<(Option<SequenceNumber>, Result<ObjectInfoResponse>, f64)> {
    let mut ret = Vec::new();

    let start = Instant::now();
    let resp = client
        .handle_object_info_request(ObjectInfoRequest {
            object_id: id,
            object_format_options: Some(ObjectFormatOptions::default()),
            request_kind: match version {
                None => ObjectInfoRequestKind::LatestObjectInfo,
                Some(v) => ObjectInfoRequestKind::PastObjectInfoDebug(SequenceNumber::from_u64(v)),
            },
        })
        .await
        .map_err(anyhow::Error::from);
    let elapsed = start.elapsed().as_secs_f64();

    let resp_version = resp.as_ref().ok().map(|r| r.object.version().value());
    ret.push((resp_version.map(SequenceNumber::from), resp, elapsed));

    ret
}

pub(crate) fn make_anemo_config() -> anemo_cli::Config {
    use narwhal_types::*;
    use sui_network::discovery::*;
    use sui_network::state_sync::*;

    // TODO: implement `ServiceInfo` generation in anemo-build and use here.
    anemo_cli::Config::new()
        // Narwhal primary-to-primary
        .add_service(
            "PrimaryToPrimary",
            anemo_cli::ServiceInfo::new()
                .add_method(
                    "SendCertificate",
                    anemo_cli::ron_method!(
                        PrimaryToPrimaryClient,
                        send_certificate,
                        SendCertificateRequest
                    ),
                )
                .add_method(
                    "RequestVote",
                    anemo_cli::ron_method!(
                        PrimaryToPrimaryClient,
                        request_vote,
                        RequestVoteRequest
                    ),
                )
                .add_method(
                    "GetPayloadAvailability",
                    anemo_cli::ron_method!(
                        PrimaryToPrimaryClient,
                        get_payload_availability,
                        PayloadAvailabilityRequest
                    ),
                )
                .add_method(
                    "GetCertificates",
                    anemo_cli::ron_method!(
                        PrimaryToPrimaryClient,
                        get_certificates,
                        GetCertificatesRequest
                    ),
                )
                .add_method(
                    "FetchCertificates",
                    anemo_cli::ron_method!(
                        PrimaryToPrimaryClient,
                        fetch_certificates,
                        FetchCertificatesRequest
                    ),
                ),
        )
        // Narwhal worker-to-worker
        .add_service(
            "WorkerToWorker",
            anemo_cli::ServiceInfo::new()
                .add_method(
                    "ReportBatch",
                    anemo_cli::ron_method!(WorkerToWorkerClient, report_batch, WorkerBatchMessage),
                )
                .add_method(
                    "RequestBatch",
                    anemo_cli::ron_method!(
                        WorkerToWorkerClient,
                        request_batch,
                        RequestBatchRequest
                    ),
                ),
        )
        // Sui discovery
        .add_service(
            "Discovery",
            anemo_cli::ServiceInfo::new().add_method(
                "GetKnownPeers",
                anemo_cli::ron_method!(DiscoveryClient, get_known_peers, ()),
            ),
        )
        // Sui state sync
        .add_service(
            "StateSync",
            anemo_cli::ServiceInfo::new()
                .add_method(
                    "PushCheckpointSummary",
                    anemo_cli::ron_method!(
                        StateSyncClient,
                        push_checkpoint_summary,
                        sui_types::messages_checkpoint::CertifiedCheckpointSummary
                    ),
                )
                .add_method(
                    "GetCheckpointSummary",
                    anemo_cli::ron_method!(
                        StateSyncClient,
                        get_checkpoint_summary,
                        GetCheckpointSummaryRequest
                    ),
                )
                .add_method(
                    "GetCheckpointContents",
                    anemo_cli::ron_method!(
                        StateSyncClient,
                        get_checkpoint_contents,
                        sui_types::messages_checkpoint::CheckpointContentsDigest
                    ),
                )
                .add_method(
                    "GetCheckpointAvailability",
                    anemo_cli::ron_method!(StateSyncClient, get_checkpoint_availability, ()),
                ),
        )
}

fn copy_dir_all(
    src: impl AsRef<Path>,
    dst: impl AsRef<Path>,
    skip: Vec<PathBuf>,
) -> io::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if skip.contains(&entry.path()) {
            continue;
        }
        if ty.is_dir() {
            copy_dir_all(
                entry.path(),
                dst.as_ref().join(entry.file_name()),
                skip.clone(),
            )?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}

pub async fn restore_from_db_checkpoint(
    config: &NodeConfig,
    db_checkpoint_path: &Path,
) -> Result<(), anyhow::Error> {
    copy_dir_all(db_checkpoint_path, config.db_path(), vec![])?;
    Ok(())
}

pub async fn state_sync_from_archive(
    path: &Path,
    genesis: &Path,
    remote_store_config: ObjectStoreConfig,
    concurrency: usize,
) -> Result<()> {
    let genesis = Genesis::load(genesis).unwrap();
    let genesis_committee = genesis.committee()?;

    let checkpoint_store = Arc::new(CheckpointStore::open_tables_read_write(
        path.join("checkpoints"),
        MetricConf::default(),
        None,
        None,
    ));
    // Only insert the genesis checkpoint if the DB is empty and doesn't have it already
    if checkpoint_store
        .get_checkpoint_by_digest(genesis.checkpoint().digest())
        .unwrap()
        .is_none()
    {
        checkpoint_store.insert_checkpoint_contents(genesis.checkpoint_contents().clone())?;
        checkpoint_store.insert_verified_checkpoint(&genesis.checkpoint())?;
        checkpoint_store.update_highest_synced_checkpoint(&genesis.checkpoint())?;
    }

    let perpetual_db = Arc::new(AuthorityPerpetualTables::open(&path.join("store"), None));

    let committee_store = Arc::new(CommitteeStore::new(
        path.join("epochs"),
        &genesis_committee,
        None,
    ));

    let store = AuthorityStore::open(
        perpetual_db,
        &genesis,
        &committee_store,
        usize::MAX,
        false,
        &Registry::default(),
    )
    .await?;

    let latest_checkpoint = checkpoint_store
        .get_highest_synced_checkpoint()?
        .map(|c| c.sequence_number)
        .unwrap_or(0);
    let state_sync_store = RocksDbStore::new(store, committee_store, checkpoint_store.clone());
    let mut archive_reader =
        ArchiveReader::new(remote_store_config, NonZeroUsize::new(concurrency).unwrap())?;
    archive_reader.sync_manifest_once().await?;
    let latest_checkpoint_in_archive = archive_reader.latest_available_checkpoint().await?;
    info!(
        "Latest available checkpoint in archive store: {}",
        latest_checkpoint_in_archive
    );
    info!("Highest synced checkpoint in db: {latest_checkpoint}");
    if latest_checkpoint_in_archive <= latest_checkpoint {
        return Ok(());
    }
    let progress_bar = ProgressBar::new(latest_checkpoint_in_archive).with_style(
        ProgressStyle::with_template("[{elapsed_precise}] {wide_bar} {pos}/{len}({msg})").unwrap(),
    );
    let counter = Arc::new(AtomicU64::new(0));
    let cloned_progress_bar = progress_bar.clone();
    let cloned_checkpoint_store = checkpoint_store.clone();
    let cloned_counter = counter.clone();
    let instant = Instant::now();
    tokio::spawn(async move {
        loop {
            let curr_latest_checkpoint = cloned_checkpoint_store
                .get_highest_synced_checkpoint()
                .unwrap()
                .map(|c| c.sequence_number)
                .unwrap_or(0);
            let total_checkpoints_per_sec = (curr_latest_checkpoint - latest_checkpoint) as f64
                / instant.elapsed().as_secs_f64();
            let total_txns_per_sec =
                cloned_counter.load(Ordering::Relaxed) as f64 / instant.elapsed().as_secs_f64();
            cloned_progress_bar.set_position(curr_latest_checkpoint);
            cloned_progress_bar.set_message(format!(
                "checkpoints/s: {}, txns/s: {}",
                total_checkpoints_per_sec, total_txns_per_sec
            ));
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
    let start = latest_checkpoint
        .checked_add(1)
        .context("Checkpoint overflow")
        .map_err(|_| anyhow!("Failed to increment checkpoint"))?;
    info!("Starting syncing checkpoints from checkpoint seq num: {start}");
    archive_reader
        .read(state_sync_store, start..u64::MAX, counter)
        .await?;
    let end = checkpoint_store
        .get_highest_synced_checkpoint()?
        .map(|c| c.sequence_number)
        .unwrap_or(0);
    progress_bar.finish_and_clear();
    info!("Highest synced checkpoint after sync: {end}");
    Ok(())
}
