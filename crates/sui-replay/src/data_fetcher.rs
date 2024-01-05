// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::types::LocalExecError;
use crate::types::EPOCH_CHANGE_STRUCT_TAG;
use async_trait::async_trait;
use futures::future::join_all;
use move_core_types::parser::parse_struct_tag;
use rand::Rng;
use std::collections::BTreeMap;
use std::str::FromStr;
use sui_core::authority::NodeStateDump;
use sui_json_rpc::api::QUERY_MAX_RESULT_LIMIT;
use sui_json_rpc_types::EventFilter;
use sui_json_rpc_types::SuiEvent;
use sui_json_rpc_types::SuiGetPastObjectRequest;
use sui_json_rpc_types::SuiObjectData;
use sui_json_rpc_types::SuiObjectDataOptions;
use sui_json_rpc_types::SuiObjectResponse;
use sui_json_rpc_types::SuiPastObjectResponse;
use sui_json_rpc_types::SuiTransactionBlockResponse;
use sui_json_rpc_types::SuiTransactionBlockResponseOptions;
use sui_sdk::SuiClient;
use sui_types::base_types::{ObjectID, SequenceNumber, VersionNumber};
use sui_types::digests::TransactionDigest;
use sui_types::messages::SenderSignedData;
use sui_types::messages::TransactionDataAPI;
use sui_types::messages::TransactionKind;
use sui_types::object::Object;
use tracing::error;
/// This trait defines the interfaces for fetching data from some local or remote store
#[async_trait]
pub(crate) trait DataFetcher {
    #![allow(implied_bounds_entailment)]
    /// Fetch the specified versions of objects
    async fn multi_get_versioned(
        &self,
        objects: &[(ObjectID, SequenceNumber)],
    ) -> Result<Vec<Object>, LocalExecError>;

    /// Fetch the latest versions of objects
    async fn multi_get_latest(&self, objects: &[ObjectID]) -> Result<Vec<Object>, LocalExecError>;

    /// Fetch the TXs for this checkpoint
    async fn get_checkpoint_txs(&self, id: u64) -> Result<Vec<TransactionDigest>, LocalExecError>;

    /// Fetch the transaction info for a given transaction digest
    async fn get_transaction(
        &self,
        tx_digest: &TransactionDigest,
    ) -> Result<SuiTransactionBlockResponse, LocalExecError>;

    async fn get_loaded_child_objects(
        &self,
        tx_digest: &TransactionDigest,
    ) -> Result<Vec<(ObjectID, SequenceNumber)>, LocalExecError>;

    async fn get_latest_checkpoint_sequence_number(&self) -> Result<u64, LocalExecError>;

    async fn fetch_random_tx(
        &self,
        // TODO: add more params
        checkpoint_id_start: Option<u64>,
        checkpoint_id_end: Option<u64>,
    ) -> Result<TransactionDigest, LocalExecError>;

    async fn get_epoch_start_timestamp_and_rgp(
        &self,
        epoch_id: u64,
        is_testnet: bool,
    ) -> Result<(u64, u64), LocalExecError>;

    async fn get_epoch_change_events(&self, reverse: bool)
        -> Result<Vec<SuiEvent>, LocalExecError>;
}

#[derive(Clone)]
pub enum Fetchers {
    Remote(RemoteFetcher),
    NodeStateDump(NodeStateDumpFetcher),
}

impl Fetchers {
    pub fn as_remote(&self) -> &RemoteFetcher {
        match self {
            Fetchers::Remote(q) => q,
            Fetchers::NodeStateDump(_) => panic!("not a remote fetcher"),
        }
    }

    pub fn as_node_state_dump(&self) -> &NodeStateDumpFetcher {
        match self {
            Fetchers::Remote(_) => panic!("not a node state dump fetcher"),
            Fetchers::NodeStateDump(q) => q,
        }
    }
}

#[async_trait]
impl DataFetcher for Fetchers {
    #![allow(implied_bounds_entailment)]
    async fn multi_get_versioned(
        &self,
        objects: &[(ObjectID, SequenceNumber)],
    ) -> Result<Vec<Object>, LocalExecError> {
        match self {
            Fetchers::Remote(q) => q.multi_get_versioned(objects).await,
            Fetchers::NodeStateDump(q) => q.multi_get_versioned(objects).await,
        }
    }

    async fn multi_get_latest(&self, objects: &[ObjectID]) -> Result<Vec<Object>, LocalExecError> {
        match self {
            Fetchers::Remote(q) => q.multi_get_latest(objects).await,
            Fetchers::NodeStateDump(q) => q.multi_get_latest(objects).await,
        }
    }

    async fn get_checkpoint_txs(&self, id: u64) -> Result<Vec<TransactionDigest>, LocalExecError> {
        match self {
            Fetchers::Remote(q) => q.get_checkpoint_txs(id).await,
            Fetchers::NodeStateDump(q) => q.get_checkpoint_txs(id).await,
        }
    }

    async fn get_transaction(
        &self,
        tx_digest: &TransactionDigest,
    ) -> Result<SuiTransactionBlockResponse, LocalExecError> {
        match self {
            Fetchers::Remote(q) => q.get_transaction(tx_digest).await,
            Fetchers::NodeStateDump(q) => q.get_transaction(tx_digest).await,
        }
    }

    async fn get_loaded_child_objects(
        &self,
        tx_digest: &TransactionDigest,
    ) -> Result<Vec<(ObjectID, SequenceNumber)>, LocalExecError> {
        match self {
            Fetchers::Remote(q) => q.get_loaded_child_objects(tx_digest).await,
            Fetchers::NodeStateDump(q) => q.get_loaded_child_objects(tx_digest).await,
        }
    }

    async fn get_latest_checkpoint_sequence_number(&self) -> Result<u64, LocalExecError> {
        match self {
            Fetchers::Remote(q) => q.get_latest_checkpoint_sequence_number().await,
            Fetchers::NodeStateDump(q) => q.get_latest_checkpoint_sequence_number().await,
        }
    }

    async fn fetch_random_tx(
        &self,
        checkpoint_id_start: Option<u64>,
        checkpoint_id_end: Option<u64>,
    ) -> Result<TransactionDigest, LocalExecError> {
        match self {
            Fetchers::Remote(q) => {
                q.fetch_random_tx(checkpoint_id_start, checkpoint_id_end)
                    .await
            }
            Fetchers::NodeStateDump(q) => {
                q.fetch_random_tx(checkpoint_id_start, checkpoint_id_end)
                    .await
            }
        }
    }

    async fn get_epoch_start_timestamp_and_rgp(
        &self,
        epoch_id: u64,
        is_testnet: bool,
    ) -> Result<(u64, u64), LocalExecError> {
        match self {
            Fetchers::Remote(q) => {
                q.get_epoch_start_timestamp_and_rgp(epoch_id, is_testnet)
                    .await
            }
            Fetchers::NodeStateDump(q) => {
                q.get_epoch_start_timestamp_and_rgp(epoch_id, is_testnet)
                    .await
            }
        }
    }

    async fn get_epoch_change_events(
        &self,
        reverse: bool,
    ) -> Result<Vec<SuiEvent>, LocalExecError> {
        match self {
            Fetchers::Remote(q) => q.get_epoch_change_events(reverse).await,
            Fetchers::NodeStateDump(q) => q.get_epoch_change_events(reverse).await,
        }
    }
}

#[derive(Clone)]
pub struct RemoteFetcher {
    /// This is used to download items not in store
    pub rpc_client: SuiClient,
}

#[async_trait]
impl DataFetcher for RemoteFetcher {
    #![allow(implied_bounds_entailment)]
    async fn multi_get_versioned(
        &self,
        objects: &[(ObjectID, VersionNumber)],
    ) -> Result<Vec<Object>, LocalExecError> {
        let options = SuiObjectDataOptions::bcs_lossless();

        let objs: Vec<_> = objects
            .iter()
            .map(|(object_id, version)| SuiGetPastObjectRequest {
                object_id: *object_id,
                version: *version,
            })
            .collect();

        let objectsx = objs.chunks(*QUERY_MAX_RESULT_LIMIT).map(|q| {
            self.rpc_client
                .read_api()
                .try_multi_get_parsed_past_object(q.to_vec(), options.clone())
        });

        join_all(objectsx)
            .await
            .into_iter()
            .collect::<Result<Vec<Vec<_>>, _>>()
            .map_err(LocalExecError::from)?
            .iter()
            .flatten()
            .map(|q| convert_past_obj_response(q.clone()))
            .collect::<Result<Vec<_>, _>>()
    }

    async fn multi_get_latest(&self, objects: &[ObjectID]) -> Result<Vec<Object>, LocalExecError> {
        let options = SuiObjectDataOptions::bcs_lossless();

        let objectsx = objects.chunks(*QUERY_MAX_RESULT_LIMIT).map(|q| {
            self.rpc_client
                .read_api()
                .multi_get_object_with_options(q.to_vec(), options.clone())
        });

        join_all(objectsx)
            .await
            .into_iter()
            .collect::<Result<Vec<Vec<_>>, _>>()
            .map_err(LocalExecError::from)?
            .iter()
            .flatten()
            .map(obj_from_sui_obj_response)
            .collect::<Result<Vec<_>, _>>()
    }

    async fn get_checkpoint_txs(&self, id: u64) -> Result<Vec<TransactionDigest>, LocalExecError> {
        Ok(self
            .rpc_client
            .read_api()
            .get_checkpoint(id.into())
            .await
            .map_err(|q| LocalExecError::SuiRpcError { err: q.to_string() })?
            .transactions)
    }

    async fn get_transaction(
        &self,
        tx_digest: &TransactionDigest,
    ) -> Result<SuiTransactionBlockResponse, LocalExecError> {
        let tx_fetch_opts = SuiTransactionBlockResponseOptions::full_content();

        self.rpc_client
            .read_api()
            .get_transaction_with_options(*tx_digest, tx_fetch_opts)
            .await
            .map_err(LocalExecError::from)
    }

    async fn get_loaded_child_objects(
        &self,
        tx_digest: &TransactionDigest,
    ) -> Result<Vec<(ObjectID, SequenceNumber)>, LocalExecError> {
        let loaded_child_objs = match self
            .rpc_client
            .read_api()
            .get_loaded_child_objects(*tx_digest)
            .await
        {
            Ok(objs) => objs,
            Err(e) => {
                error!("Error getting dynamic fields loaded objects: {}. This RPC server might not support this feature yet", e);
                return Err(LocalExecError::UnableToGetDynamicFieldLoadedObjects {
                    rpc_err: e.to_string(),
                });
            }
        };

        // Fetch the refs
        Ok(loaded_child_objs
            .loaded_child_objects
            .iter()
            .map(|obj| (obj.object_id(), obj.sequence_number()))
            .collect::<Vec<_>>())
    }

    async fn get_latest_checkpoint_sequence_number(&self) -> Result<u64, LocalExecError> {
        self.rpc_client
            .read_api()
            .get_latest_checkpoint_sequence_number()
            .await
            .map_err(LocalExecError::from)
    }

    async fn fetch_random_tx(
        &self,
        // TODO: add more params
        checkpoint_id_start: Option<u64>,
        checkpoint_id_end: Option<u64>,
    ) -> Result<TransactionDigest, LocalExecError> {
        let checkpoint_id_end =
            checkpoint_id_end.unwrap_or(self.get_latest_checkpoint_sequence_number().await?);
        let checkpoint_id_start = checkpoint_id_start.unwrap_or(1);
        let checkpoint_id = rand::thread_rng().gen_range(checkpoint_id_start..=checkpoint_id_end);

        let txs = self.get_checkpoint_txs(checkpoint_id).await?;
        let tx_idx = rand::thread_rng().gen_range(0..txs.len());

        Ok(txs[tx_idx])
    }

    /// Very testnet specific
    /// This function is testnet specific and will be extended for mainnet later
    async fn get_epoch_start_timestamp_and_rgp(
        &self,
        epoch_id: u64,
        is_testnet: bool,
    ) -> Result<(u64, u64), LocalExecError> {
        // Hack for testnet: for epoch in range [3, 742), we have no data, but no user TX was executed, so return dummy
        if (is_testnet) && (2 < epoch_id) && (epoch_id < 742) {
            return Ok((0, 1));
        }

        let event = self
            .get_epoch_change_events(true)
            .await?
            .into_iter()
            .find(|ev| match extract_epoch_and_version(ev.clone()) {
                Ok((epoch, _)) => epoch == epoch_id,
                Err(_) => false,
            })
            .ok_or(LocalExecError::EventNotFound { epoch: epoch_id })?;

        let reference_gas_price = if let serde_json::Value::Object(w) = event.parsed_json {
            u64::from_str(&w["reference_gas_price"].to_string().replace('\"', "")).unwrap()
        } else {
            return Err(LocalExecError::UnexpectedEventFormat {
                event: event.clone(),
            });
        };

        let epoch_change_tx = event.id.tx_digest;

        // Fetch full transaction content
        let tx_info = self.get_transaction(&epoch_change_tx).await?;

        let orig_tx: SenderSignedData = bcs::from_bytes(&tx_info.raw_transaction).unwrap();
        let tx_kind_orig = orig_tx.transaction_data().kind();

        if let TransactionKind::ChangeEpoch(change) = tx_kind_orig {
            return Ok((change.epoch_start_timestamp_ms, reference_gas_price));
        }
        Err(LocalExecError::InvalidEpochChangeTx { epoch: epoch_id })
    }

    async fn get_epoch_change_events(
        &self,
        reverse: bool,
    ) -> Result<Vec<SuiEvent>, LocalExecError> {
        let struct_tag_str = EPOCH_CHANGE_STRUCT_TAG.to_string();
        let struct_tag = parse_struct_tag(&struct_tag_str)?;

        // TODO: Should probably limit/page this but okay for now?
        Ok(self
            .rpc_client
            .event_api()
            .query_events(EventFilter::MoveEventType(struct_tag), None, None, reverse)
            .await
            .map_err(|e| LocalExecError::UnableToQuerySystemEvents {
                rpc_err: e.to_string(),
            })?
            .data
            .into_iter()
            .collect())
    }
}

fn convert_past_obj_response(resp: SuiPastObjectResponse) -> Result<Object, LocalExecError> {
    match resp {
        SuiPastObjectResponse::VersionFound(o) => obj_from_sui_obj_data(&o),
        SuiPastObjectResponse::ObjectDeleted(r) => Err(LocalExecError::ObjectDeleted {
            id: r.object_id,
            version: r.version,
            digest: r.digest,
        }),
        SuiPastObjectResponse::ObjectNotExists(id) => Err(LocalExecError::ObjectNotExist { id }),
        SuiPastObjectResponse::VersionNotFound(id, version) => {
            Err(LocalExecError::ObjectVersionNotFound { id, version })
        }
        SuiPastObjectResponse::VersionTooHigh {
            object_id,
            asked_version,
            latest_version,
        } => Err(LocalExecError::ObjectVersionTooHigh {
            id: object_id,
            asked_version,
            latest_version,
        }),
    }
}

fn obj_from_sui_obj_response(o: &SuiObjectResponse) -> Result<Object, LocalExecError> {
    let o = o.object().map_err(LocalExecError::from)?.clone();
    obj_from_sui_obj_data(&o)
}

fn obj_from_sui_obj_data(o: &SuiObjectData) -> Result<Object, LocalExecError> {
    match TryInto::<Object>::try_into(o.clone()) {
        Ok(obj) => Ok(obj),
        Err(e) => Err(e.into()),
    }
}

pub fn extract_epoch_and_version(ev: SuiEvent) -> Result<(u64, u64), LocalExecError> {
    if let serde_json::Value::Object(w) = ev.parsed_json {
        let epoch = u64::from_str(&w["epoch"].to_string().replace('\"', "")).unwrap();
        let version = u64::from_str(&w["protocol_version"].to_string().replace('\"', "")).unwrap();
        return Ok((epoch, version));
    }

    Err(LocalExecError::UnexpectedEventFormat { event: ev })
}

#[derive(Debug, Clone)]
pub struct NodeStateDumpFetcher {
    pub node_state_dump: NodeStateDump,
    pub object_ref_pool: BTreeMap<(ObjectID, SequenceNumber), Object>,
    pub latest_object_version_pool: BTreeMap<ObjectID, Object>,
}

impl From<NodeStateDump> for NodeStateDumpFetcher {
    fn from(node_state_dump: NodeStateDump) -> Self {
        let mut object_ref_pool = BTreeMap::new();
        let mut latest_object_version_pool: BTreeMap<ObjectID, Object> = BTreeMap::new();

        node_state_dump
            .all_objects()
            .iter()
            .for_each(|current_obj| {
                // Dense storage
                object_ref_pool.insert(
                    (current_obj.id(), current_obj.version()),
                    current_obj.clone(),
                );

                // Only most recent
                if let Some(last_seen_obj) = latest_object_version_pool.get(&current_obj.id()) {
                    if current_obj.version() <= last_seen_obj.version() {
                        return;
                    }
                };
                latest_object_version_pool.insert(current_obj.id(), current_obj.clone());
            });
        Self {
            node_state_dump,
            object_ref_pool,
            latest_object_version_pool,
        }
    }
}

#[async_trait]
impl DataFetcher for NodeStateDumpFetcher {
    async fn multi_get_versioned(
        &self,
        objects: &[(ObjectID, SequenceNumber)],
    ) -> Result<Vec<Object>, LocalExecError> {
        let mut resp = vec![];
        objects.iter().try_for_each(|(id, version)| {
            if let Some(obj) = self.object_ref_pool.get(&(*id, *version)) {
                resp.push(obj.clone());
                return Ok(());
            }
            Err(LocalExecError::ObjectVersionNotFound {
                id: *id,
                version: *version,
            })
        })?;
        Ok(resp)
    }

    async fn multi_get_latest(&self, objects: &[ObjectID]) -> Result<Vec<Object>, LocalExecError> {
        let mut resp = vec![];
        objects.iter().try_for_each(|id| {
            if let Some(obj) = self.latest_object_version_pool.get(id) {
                resp.push(obj.clone());
                return Ok(());
            }
            Err(LocalExecError::ObjectNotExist { id: *id })
        })?;
        Ok(resp)
    }

    async fn get_checkpoint_txs(&self, _id: u64) -> Result<Vec<TransactionDigest>, LocalExecError> {
        unimplemented!("get_checkpoint_txs for state dump is not implemented")
    }

    async fn get_transaction(
        &self,
        _tx_digest: &TransactionDigest,
    ) -> Result<SuiTransactionBlockResponse, LocalExecError> {
        unimplemented!("get_transaction for state dump is not implemented")
    }

    async fn get_loaded_child_objects(
        &self,
        _tx_digest: &TransactionDigest,
    ) -> Result<Vec<(ObjectID, SequenceNumber)>, LocalExecError> {
        Ok(self
            .node_state_dump
            .loaded_child_objects
            .iter()
            .map(|q| q.compute_object_reference())
            .map(|w| (w.0, w.1))
            .collect())
    }

    async fn get_latest_checkpoint_sequence_number(&self) -> Result<u64, LocalExecError> {
        unimplemented!("get_latest_checkpoint_sequence_number for state dump is not implemented")
    }

    async fn fetch_random_tx(
        &self,
        // TODO: add more params
        _checkpoint_id_start: Option<u64>,
        _checkpoint_id_end: Option<u64>,
    ) -> Result<TransactionDigest, LocalExecError> {
        unimplemented!("fetch_random_tx for state dump is not implemented")
    }

    async fn get_epoch_start_timestamp_and_rgp(
        &self,
        _epoch_id: u64,
        _is_testnet: bool,
    ) -> Result<(u64, u64), LocalExecError> {
        Ok((
            self.node_state_dump.epoch_start_timestamp_ms,
            self.node_state_dump.reference_gas_price,
        ))
    }

    async fn get_epoch_change_events(
        &self,
        _reverse: bool,
    ) -> Result<Vec<SuiEvent>, LocalExecError> {
        unimplemented!("get_epoch_change_events for state dump is not implemented")
    }
}
