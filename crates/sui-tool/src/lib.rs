// Copyright (c) 2021, Facebook, Inc. and its affiliates
// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;
use futures::future::join_all;
use futures::stream::StreamExt;
use itertools::Itertools;
use multiaddr::Multiaddr;
use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::PathBuf;
use std::sync::Arc;
use sui_config::{genesis::Genesis, ValidatorInfo};
use sui_core::authority_client::{
    AuthorityAPI, NetworkAuthorityClient, NetworkAuthorityClientMetrics,
};
use sui_network::default_mysten_network_config;
use sui_types::message_envelope::Message;
use sui_types::object::ObjectFormatOptions;
use sui_types::{base_types::*, batch::*, messages::*, object::Owner};
use tokio::time::Instant;

use anyhow::anyhow;

pub mod commands;
pub mod db_tool;

fn make_clients(
    genesis: PathBuf,
) -> Result<BTreeMap<AuthorityName, (ValidatorInfo, NetworkAuthorityClient)>> {
    let net_config = default_mysten_network_config();

    let genesis = Genesis::load(genesis)?;

    let mut authority_clients = BTreeMap::new();

    for validator in genesis.into_validator_set() {
        let channel = net_config
            .connect_lazy(validator.network_address())
            .map_err(|err| anyhow!(err.to_string()))?;
        let client = NetworkAuthorityClient::new(
            channel,
            Arc::new(NetworkAuthorityClientMetrics::new_for_tests()),
        );
        let public_key_bytes = validator.protocol_key();
        authority_clients.insert(public_key_bytes, (validator, client));
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

pub struct GrouppedObjectOutput(pub ObjectData);

#[allow(clippy::format_in_format_args)]
impl std::fmt::Display for GrouppedObjectOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let responses = self
            .0
            .responses
            .iter()
            .flat_map(|(name, multiaddr, resp)| {
                resp.iter().map(|(seq_num, r, timespent)| {
                    (*name, multiaddr.clone(), seq_num, r, timespent)
                })
            })
            .sorted_by(|a, b| Ord::cmp(&b.2, &a.2))
            .group_by(|(_, _, seq_num, _r, _ts)| **seq_num);
        for (seq_num, group) in &responses {
            writeln!(f, "seq num: {}", seq_num.opt_debug("latest-seq-num"))?;
            let cur_version_resp = group.group_by(|(_, _, _, r, _)| match r {
                Ok(result) => {
                    let parent_tx_digest = result.parent_certificate.as_ref().map(|tx| tx.digest());
                    let obj_digest = result
                        .requested_object_reference
                        .as_ref()
                        .map(|(_, _, digest)| digest);
                    let lock = result
                        .object_and_lock
                        .as_ref()
                        .map(|obj_n_lock| obj_n_lock.lock.as_ref().map(|lock| *lock.digest()));
                    let owner = result
                        .object_and_lock
                        .as_ref()
                        .map(|obj_n_lock| obj_n_lock.object.owner);
                    Some((parent_tx_digest, obj_digest, lock, owner))
                }
                Err(_) => None,
            });
            for (result, group) in &cur_version_resp {
                match result {
                    Some((parent_tx_digest, obj_digest, lock, owner)) => {
                        let objref = obj_digest.opt_debug("objref-not-available");
                        let parent_cert = parent_tx_digest.opt_debug("<genesis>");
                        let owner = owner.opt_display("no-owner-available");
                        let lock = lock.opt_debug("no-known-lock");
                        writeln!(f, "obj ref: {objref}")?;
                        writeln!(f, "parent cert: {parent_cert}")?;
                        writeln!(f, "owner: {owner}")?;
                        writeln!(f, "lock: {lock}")?;
                        for (i, (name, multiaddr, _, _, timespent)) in group.enumerate() {
                            writeln!(
                                f,
                                "        {:<4} {:<66} {:<56} (using {:.3} seconds)",
                                i,
                                name,
                                format!("{}", multiaddr),
                                timespent
                            )?;
                        }
                    }
                    None => {
                        writeln!(f, " error")?;
                        for (i, (name, multiaddr, _, resp, timespent)) in group.enumerate() {
                            writeln!(
                                f,
                                "        {:<4} {:<66} {:<56} (using {:.3} seconds) {:?}",
                                i,
                                name,
                                format!("{}", multiaddr),
                                timespent,
                                resp
                            )?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

struct ConciseObjectOutput(ObjectData);

impl ConciseObjectOutput {
    fn header() -> String {
        format!(
            "{:<66} {:<8} {:<66} {:<45} {}",
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
                    "{:<66} {:<8}",
                    format!("{:?}", name),
                    version.map(|s| s.value()).opt_debug("-")
                )?;
                match resp {
                    Err(_) => writeln!(
                        f,
                        "{:<66} {:<45} {:<51}",
                        "object-fetch-failed", "no-cert-available", "no-owner-available"
                    )?,
                    Ok(resp) => {
                        let objref = resp
                            .requested_object_reference
                            .map(|(_, _, digest)| digest)
                            .opt_debug("objref-not-available");
                        let cert = resp
                            .parent_certificate
                            .as_ref()
                            .map(|c| *c.digest())
                            .opt_debug("<genesis>");
                        let owner = resp
                            .object_and_lock
                            .as_ref()
                            .map(|o| OwnerOutput(o.object.owner))
                            .opt_display("no-owner-available");
                        write!(f, " {:<66} {:<45} {:<51}", objref, cert, owner)?;
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
            writeln!(f, "validator: {:?}, addr: {:?}", name, multiaddr)?;

            for (version, resp, timespent) in versions {
                writeln!(
                    f,
                    "-- version: {} (using {:.3} seconds)",
                    version.opt_debug("<version not available>"),
                    timespent,
                )?;

                match resp {
                    Err(e) => writeln!(f, "Error fetching object: {}", e)?,
                    Ok(resp) => {
                        let objref = resp.requested_object_reference.opt_debug("<no object>");
                        writeln!(f, "  -- ref: {}", objref)?;

                        write!(f, "  -- cert:")?;
                        match &resp.parent_certificate {
                            None => writeln!(f, " <genesis>")?,
                            Some(cert) => {
                                let cert = format!("{}", cert);
                                let cert = textwrap::indent(&cert, "     | ");
                                write!(f, "\n{}", cert)?;
                            }
                        }

                        if let Some(ObjectResponse {
                            lock,
                            object,
                            layout,
                        }) = &resp.object_and_lock
                        {
                            if object.is_package() {
                                writeln!(f, "  -- object: <Move Package>")?;
                            } else if let Some(layout) = layout {
                                writeln!(
                                    f,
                                    "  -- object: Move Object: {}",
                                    object
                                        .data
                                        .try_as_move()
                                        .unwrap()
                                        .to_move_struct(layout)
                                        .unwrap()
                                )?;
                            }
                            writeln!(f, "  -- owner: {}", object.owner)?;
                            writeln!(f, "  -- locked by: {}", lock.opt_debug("<not locked>"))?;
                        }
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
    genesis: PathBuf,
    history: bool,
) -> Result<ObjectData> {
    let clients = make_clients(genesis)?;

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
            .map(|(name, (v, client))| async {
                let object_versions = get_object_impl(client, obj_id, version, history).await;
                (*name, v.network_address().clone(), object_versions)
            }),
    )
    .await;

    Ok(ObjectData {
        requested_id: obj_id,
        responses,
    })
}

pub async fn get_transaction(tx_digest: TransactionDigest, genesis: PathBuf) -> Result<String> {
    let clients = make_clients(genesis)?;
    let timer = Instant::now();
    let responses = join_all(clients.iter().map(|(name, (v, client))| async {
        let result = client
            .handle_transaction_info_request(TransactionInfoRequest {
                transaction_digest: tx_digest,
            })
            .await;
        (
            *name,
            v.network_address().clone(),
            result,
            timer.elapsed().as_secs_f64(),
        )
    }))
    .await;

    let responses = responses
        .iter()
        .sorted_by(|(_, _, resp_a, _), (_, _, resp_b, _)| {
            let sort_key_a = resp_a
                .as_ref()
                .map(|ok_result| {
                    (ok_result.signed_effects)
                        .as_ref()
                        .map(|effects| *effects.digest())
                })
                .ok();
            let sort_key_b = resp_b
                .as_ref()
                .map(|ok_result| {
                    (ok_result.signed_effects)
                        .as_ref()
                        .map(|effects| *effects.digest())
                })
                .ok();
            Ord::cmp(&sort_key_a, &sort_key_b)
        })
        .group_by(|(_name, _addr, resp, _ts)| {
            resp.as_ref().map(|ok_result| {
                (ok_result.signed_effects)
                    .as_ref()
                    .map(|effects| (effects.data(), effects.data().digest()))
            })
        });
    let mut s = String::new();
    for (i, (st, group)) in (&responses).into_iter().enumerate() {
        match st {
            Ok(Some((effects, effect_digest))) => {
                writeln!(
                    &mut s,
                    "#{:<2} tx_digest: {:<68?} effects_digest: {:?}",
                    i, tx_digest, effect_digest
                )?;
                writeln!(&mut s, "{:#?}", effects)?;
            }
            other => {
                writeln!(&mut s, "#{:<2} {:#?}", i, other)?;
            }
        }
        for (j, res) in group.enumerate() {
            writeln!(
                &mut s,
                "        {:<4} {:<66} {:<56} (using {:.3} seconds)",
                j,
                res.0,
                format!("{}", res.1),
                res.3
            )?;
        }
        writeln!(&mut s)?;
    }
    Ok(s)
}

async fn get_object_impl(
    client: &NetworkAuthorityClient,
    id: ObjectID,
    start_version: Option<u64>,
    full_history: bool,
) -> Vec<(Option<SequenceNumber>, Result<ObjectInfoResponse>, f64)> {
    let mut ret = Vec::new();
    let mut version = start_version;

    loop {
        let start = Instant::now();
        let resp = client
            .handle_object_info_request(ObjectInfoRequest {
                object_id: id,
                request_kind: match version {
                    None => ObjectInfoRequestKind::LatestObjectInfo(Some(
                        ObjectFormatOptions::default(),
                    )),
                    Some(v) => ObjectInfoRequestKind::PastObjectInfoDebug(
                        SequenceNumber::from_u64(v),
                        Some(ObjectFormatOptions::default()),
                    ),
                },
            })
            .await
            .map_err(anyhow::Error::from);
        let elapsed = start.elapsed().as_secs_f64();

        let resp_version = resp
            .as_ref()
            .ok()
            .and_then(|r| r.requested_object_reference)
            .map(|(_, v, _)| v.value());
        ret.push((resp_version.map(SequenceNumber::from), resp, elapsed));

        version = match (version, resp_version) {
            (Some(v), _) | (None, Some(v)) => {
                if v == 1 || !full_history {
                    break;
                } else {
                    Some(v - 1)
                }
            }
            _ => break,
        };
    }

    ret
}

async fn handle_batch(client: &dyn AuthorityAPI, req: &BatchInfoRequest) {
    let mut streamx = Box::pin(client.handle_batch_stream(req.clone()).await.unwrap());

    while let Some(item) = streamx.next().await {
        match item {
            Ok(BatchInfoResponseItem(UpdateItem::Batch(signed_batch))) => {
                println!("batch: {:?}", signed_batch);
            }

            Ok(BatchInfoResponseItem(UpdateItem::Transaction((seq, digests)))) => {
                println!("item: {:?}, {:?}", seq, digests);
            }

            // Return any errors.
            Err(err) => {
                println!("error: {}", err);
            }
        }
    }
}
