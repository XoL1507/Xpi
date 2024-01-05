// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
use anyhow::bail;
use async_trait::async_trait;
use embedded_reconfig_observer::EmbeddedReconfigObserver;
use fullnode_reconfig_observer::FullNodeReconfigObserver;
use futures::{stream::FuturesUnordered, StreamExt};
use prometheus::{IntCounterVec, Registry};
use roaring::RoaringBitmap;
use std::future::Future;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use sui_config::genesis::Genesis;
use sui_core::{
    authority_aggregator::{AuthorityAggregator, AuthorityAggregatorBuilder},
    authority_client::{
        make_authority_clients_with_timeout_config, AuthorityAPI, NetworkAuthorityClient,
    },
    quorum_driver::{
        QuorumDriver, QuorumDriverHandler, QuorumDriverHandlerBuilder, QuorumDriverMetrics,
    },
};
use sui_json_rpc_types::{
    SuiObjectDataOptions, SuiTransactionBlockEffects, SuiTransactionBlockEffectsAPI,
    SuiTransactionBlockResponseOptions,
};
use sui_network::{DEFAULT_CONNECT_TIMEOUT_SEC, DEFAULT_REQUEST_TIMEOUT_SEC};
use sui_sdk::{SuiClient, SuiClientBuilder};
use sui_types::error::SuiError;
use sui_types::messages::TransactionEvents;
use sui_types::sui_system_state::sui_system_state_summary::SuiSystemStateSummary;
use sui_types::{
    base_types::ObjectID,
    committee::{Committee, EpochId},
    crypto::{
        AggregateAuthenticator, AggregateAuthoritySignature, AuthorityQuorumSignInfo,
        AuthoritySignature,
    },
    message_envelope::Envelope,
    messages::{
        CertifiedTransaction, CertifiedTransactionEffects, HandleCertificateResponse,
        QuorumDriverResponse, Transaction, TransactionEffectsAPI, TransactionStatus,
    },
    object::Object,
};
use sui_types::{base_types::ObjectRef, crypto::AuthorityStrongQuorumSignInfo, object::Owner};
use sui_types::{
    base_types::{AuthorityName, SuiAddress},
    sui_system_state::SuiSystemStateTrait,
};
use tokio::{task::JoinSet, time::timeout};
use tracing::{error, info, warn};

pub mod bank;
pub mod benchmark_setup;
pub mod drivers;
pub mod embedded_reconfig_observer;
pub mod fullnode_reconfig_observer;
pub mod in_memory_wallet;
pub mod options;
pub mod system_state_observer;
pub mod util;
pub mod workloads;

#[derive(Debug)]
/// A wrapper on execution results to accommodate different types of
/// responses from LocalValidatorAggregatorProxy and FullNodeProxy
#[allow(clippy::large_enum_variant)]
pub enum ExecutionEffects {
    CertifiedTransactionEffects(CertifiedTransactionEffects, TransactionEvents),
    SuiTransactionBlockEffects(SuiTransactionBlockEffects),
}

impl ExecutionEffects {
    pub fn mutated(&self) -> Vec<(ObjectRef, Owner)> {
        match self {
            ExecutionEffects::CertifiedTransactionEffects(certified_effects, ..) => {
                certified_effects.data().mutated().to_vec()
            }
            ExecutionEffects::SuiTransactionBlockEffects(sui_tx_effects) => sui_tx_effects
                .mutated()
                .iter()
                .map(|refe| (refe.reference.to_object_ref(), refe.owner))
                .collect(),
        }
    }

    pub fn created(&self) -> Vec<(ObjectRef, Owner)> {
        match self {
            ExecutionEffects::CertifiedTransactionEffects(certified_effects, ..) => {
                certified_effects.data().created().to_vec()
            }
            ExecutionEffects::SuiTransactionBlockEffects(sui_tx_effects) => sui_tx_effects
                .created()
                .iter()
                .map(|refe| (refe.reference.to_object_ref(), refe.owner))
                .collect(),
        }
    }

    pub fn deleted(&self) -> Vec<ObjectRef> {
        match self {
            ExecutionEffects::CertifiedTransactionEffects(certified_effects, ..) => {
                certified_effects.data().deleted().to_vec()
            }
            ExecutionEffects::SuiTransactionBlockEffects(sui_tx_effects) => sui_tx_effects
                .deleted()
                .iter()
                .map(|refe| refe.to_object_ref())
                .collect(),
        }
    }

    pub fn quorum_sig(&self) -> Option<&AuthorityStrongQuorumSignInfo> {
        match self {
            ExecutionEffects::CertifiedTransactionEffects(certified_effects, ..) => {
                Some(certified_effects.auth_sig())
            }
            ExecutionEffects::SuiTransactionBlockEffects(_) => None,
        }
    }

    pub fn gas_object(&self) -> (ObjectRef, Owner) {
        match self {
            ExecutionEffects::CertifiedTransactionEffects(certified_effects, ..) => {
                *certified_effects.data().gas_object()
            }
            ExecutionEffects::SuiTransactionBlockEffects(sui_tx_effects) => {
                let refe = &sui_tx_effects.gas_object();
                (refe.reference.to_object_ref(), refe.owner)
            }
        }
    }

    pub fn sender(&self) -> SuiAddress {
        match self.gas_object().1 {
            Owner::AddressOwner(a) => a,
            Owner::ObjectOwner(_) | Owner::Shared { .. } | Owner::Immutable => unreachable!(), // owner of gas object is always an address
        }
    }

    pub fn is_ok(&self) -> bool {
        match self {
            ExecutionEffects::CertifiedTransactionEffects(certified_effects, ..) => {
                certified_effects.data().status().is_ok()
            }
            ExecutionEffects::SuiTransactionBlockEffects(sui_tx_effects) => {
                sui_tx_effects.status().is_ok()
            }
        }
    }
}

#[async_trait]
pub trait ValidatorProxy {
    async fn get_object(&self, object_id: ObjectID) -> Result<Object, anyhow::Error>;

    async fn get_latest_system_state_object(&self) -> Result<SuiSystemStateSummary, anyhow::Error>;

    async fn execute_transaction_block(&self, tx: Transaction) -> anyhow::Result<ExecutionEffects>;

    /// This function is similar to `execute_transaction` but does not check any validator's
    /// signature. It should only be used for benchmarks.
    async fn execute_bench_transaction(&self, tx: Transaction) -> anyhow::Result<ExecutionEffects>;

    fn clone_committee(&self) -> Committee;

    fn get_current_epoch(&self) -> EpochId;

    fn clone_new(&self) -> Box<dyn ValidatorProxy + Send + Sync>;

    async fn get_validators(&self) -> Result<Vec<SuiAddress>, anyhow::Error>;
}

// TODO: Eventually remove this proxy because we shouldn't rely on validators to read objects.
pub struct LocalValidatorAggregatorProxy {
    _qd_handler: QuorumDriverHandler<NetworkAuthorityClient>,
    // Stress client does not verify individual validator signatures since this is very expensive
    qd: Arc<QuorumDriver<NetworkAuthorityClient>>,
    committee: Committee,
    clients: BTreeMap<AuthorityName, NetworkAuthorityClient>,
    requests: Mutex<JoinSet<()>>,
}

impl LocalValidatorAggregatorProxy {
    pub async fn from_genesis(
        genesis: &Genesis,
        registry: &Registry,
        reconfig_fullnode_rpc_url: Option<&str>,
    ) -> Self {
        let (aggregator, _) = AuthorityAggregatorBuilder::from_genesis(genesis)
            .with_registry(registry)
            .build()
            .unwrap();

        let committee = genesis.committee_with_network();
        let clients = make_authority_clients_with_timeout_config(
            &committee,
            DEFAULT_CONNECT_TIMEOUT_SEC,
            DEFAULT_REQUEST_TIMEOUT_SEC,
        )
        .unwrap();

        Self::new_impl(
            aggregator,
            registry,
            reconfig_fullnode_rpc_url,
            clients,
            committee.committee,
        )
        .await
    }

    async fn new_impl(
        aggregator: AuthorityAggregator<NetworkAuthorityClient>,
        registry: &Registry,
        reconfig_fullnode_rpc_url: Option<&str>,
        clients: BTreeMap<AuthorityName, NetworkAuthorityClient>,
        committee: Committee,
    ) -> Self {
        let quorum_driver_metrics = Arc::new(QuorumDriverMetrics::new(registry));
        let qd_handler = (if let Some(reconfig_fullnode_rpc_url) = reconfig_fullnode_rpc_url {
            let qd_handler_builder = QuorumDriverHandlerBuilder::new(
                Arc::new(aggregator.clone()),
                quorum_driver_metrics,
            );
            info!(
                "Using FullNodeReconfigObserver: {:?}",
                reconfig_fullnode_rpc_url
            );
            let committee_store = aggregator.clone_committee_store();
            let reconfig_observer = Arc::new(
                FullNodeReconfigObserver::new(
                    reconfig_fullnode_rpc_url,
                    committee_store,
                    aggregator.safe_client_metrics_base.clone(),
                    aggregator.metrics.clone(),
                )
                .await,
            );
            qd_handler_builder.with_reconfig_observer(reconfig_observer)
        } else {
            info!("Using EmbeddedReconfigObserver");
            let observer = EmbeddedReconfigObserver::new();
            // Get the latest committee from config observer
            let new_agg = observer
                .get_committee(Arc::new(aggregator))
                .await
                .expect("Failed to get latest committee");
            let qd_handler_builder =
                QuorumDriverHandlerBuilder::new(new_agg, quorum_driver_metrics);
            qd_handler_builder.with_reconfig_observer(Arc::new(EmbeddedReconfigObserver::new()))
        })
        .start();

        let qd = qd_handler.clone_quorum_driver();
        Self {
            _qd_handler: qd_handler,
            qd,
            clients,
            committee,
            requests: Mutex::new(JoinSet::new()),
        }
    }
}

#[async_trait]
impl ValidatorProxy for LocalValidatorAggregatorProxy {
    async fn get_object(&self, object_id: ObjectID) -> Result<Object, anyhow::Error> {
        let auth_agg = self.qd.authority_aggregator().load();
        Ok(auth_agg
            .get_latest_object_version_for_testing(object_id)
            .await?)
    }

    async fn get_latest_system_state_object(&self) -> Result<SuiSystemStateSummary, anyhow::Error> {
        let auth_agg = self.qd.authority_aggregator().load();
        Ok(auth_agg
            .get_latest_system_state_object_for_testing()
            .await?
            .into_sui_system_state_summary())
    }

    async fn execute_transaction_block(&self, tx: Transaction) -> anyhow::Result<ExecutionEffects> {
        if std::env::var("BENCH_MODE").is_ok() {
            return self.execute_bench_transaction(tx).await;
        }
        let tx_digest = *tx.digest();
        let tx = tx.verify()?;
        let mut retry_cnt = 0;
        while retry_cnt < 3 {
            let ticket = self.qd.submit_transaction(tx.clone()).await?;
            // The ticket only times out when QuorumDriver exceeds the retry times
            match ticket.await {
                Ok(resp) => {
                    let QuorumDriverResponse {
                        effects_cert,
                        events,
                    } = resp;
                    return Ok(ExecutionEffects::CertifiedTransactionEffects(
                        effects_cert.into(),
                        events,
                    ));
                }
                Err(err) => {
                    error!(
                        ?tx_digest,
                        retry_cnt, "Transaction failed with err: {:?}", err
                    );
                    retry_cnt += 1;
                }
            }
        }
        bail!("Transaction {:?} failed for {retry_cnt} times", tx_digest);
    }

    async fn execute_bench_transaction(&self, tx: Transaction) -> anyhow::Result<ExecutionEffects> {
        // Store the epoch number; we read it from the votes and use it later to create the certificate.
        let mut epoch = 0;

        async fn retry<T, F: Future<Output = Result<T, SuiError>>>(
            gen: impl Fn() -> F,
            name: AuthorityName,
            metrics: IntCounterVec,
        ) -> T {
            loop {
                let fut = gen();
                match fut.await {
                    Ok(r) => return r,
                    Err(e) => {
                        metrics
                            .with_label_values(&[&name.concise().to_string(), e.as_ref()])
                            .inc();
                        warn!("RPC error from {} {:?}", name, e);
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                }
            }
        }

        // Send the transaction to all validators.
        let metrics = self.qd.authority_aggregator().load().metrics.clone();
        let mut futures = FuturesUnordered::new();
        for (name, client) in self.clients.iter() {
            let client = client.clone();
            let tx = tx.clone();
            let fut = retry(
                move || {
                    let client = client.clone();
                    let tx = tx.clone();
                    async move { client.handle_transaction(tx).await }
                },
                *name,
                metrics.process_tx_errors.clone(),
            );
            futures.push(fut);
        }

        // TODO: This following aggregation will not work well at epoch boundary.

        // Listen to the replies from the first 2f+1 votes.
        let mut total_stake = 0;
        let mut votes = Vec::new();
        let mut certificate = None;
        while let Some(response) = futures.next().await {
            match response.status {
                // If all goes well, the authority returns a vote.
                TransactionStatus::Signed(signature) => {
                    epoch = signature.epoch;
                    total_stake += self.committee.weight(&signature.authority);
                    votes.push(signature);
                }
                // The transaction may be submitted again in case the certificate's submission failed.
                TransactionStatus::Executed(cert, _effects, _) => {
                    tracing::warn!("Transaction already submitted: {tx:?}");
                    if let Some(cert) = cert {
                        certificate = Some(CertifiedTransaction::new_from_data_and_sig(
                            tx.data().clone(),
                            cert,
                        ));
                    }
                }
            }

            if total_stake >= self.committee.quorum_threshold() {
                break;
            }

            if certificate.is_some() {
                break;
            }
        }

        // Assemble a certificate from the validator's replies.
        let certified_transaction: CertifiedTransaction = match certificate {
            Some(x) => x,
            None => {
                let signatures: BTreeMap<_, _> = votes
                    .into_iter()
                    .map(|a| (a.authority, a.signature))
                    .collect();
                let mut signers_map = RoaringBitmap::new();
                for pk in signatures.keys() {
                    signers_map.insert(
                        self.committee
                            .authority_index(pk)
                            .ok_or(SuiError::UnknownSigner {
                                signer: Some(pk.concise().to_string()),
                                index: None,
                                committee: Box::new(self.committee.clone()),
                            })
                            .expect("Received signature from unknown validator"),
                    );
                }
                let sigs: Vec<AuthoritySignature> = signatures.into_values().collect();

                let quorum_signature = AuthorityQuorumSignInfo {
                    epoch,
                    // Note: This function simply aggregates signatures (it does not check that they
                    // are individually valid).
                    signature: AggregateAuthoritySignature::aggregate(&sigs)
                        .map_err(|e| SuiError::InvalidSignature {
                            error: e.to_string(),
                        })
                        .expect("Validator returned invalid signature"),
                    signers_map,
                };

                Envelope::new_from_data_and_sig(tx.into_data(), quorum_signature)
            }
        };

        // Send the certificate to all validators.
        let mut futures = FuturesUnordered::new();
        total_stake = 0;
        let mut transaction_effects = None;
        let mut transaction_events = None;
        for (name, client) in self.clients.iter() {
            let client = client.clone();
            let certificate = certified_transaction.clone();
            let name = *name;
            futures.push(retry(
                move || {
                    let client = client.clone();
                    let certificate = certificate.clone();
                    async move { client.handle_certificate(certificate).await }
                },
                name,
                metrics.process_cert_errors.clone(),
            ))
        }

        // Wait for the replies from a quorum of validators.
        while let Some(response) = futures.next().await {
            let HandleCertificateResponse {
                signed_effects,
                events,
            } = response;
            let author = signed_effects.auth_sig().authority;
            transaction_effects = Some(signed_effects.data().clone());
            transaction_events = Some(events);
            total_stake += self.committee.weight(&author);

            if total_stake >= self.committee.quorum_threshold() {
                break;
            }
        }

        // Abort if we failed to submit the certificate to enough validators. This typically
        // happens when the validators are overloaded and the requests timed out.
        if transaction_effects.is_none() || total_stake < self.committee.quorum_threshold() {
            bail!("Failed to submit certificate to quorum of validators");
        }

        // Wait for 10 more seconds on remaining requests asynchronously.
        {
            let mut requests = self.requests.lock().unwrap();
            requests.spawn(async move {
                let _ = timeout(Duration::from_secs(10), futures.collect::<Vec<_>>()).await;
            });
        }

        // Package the certificate and effects to return.
        let signed_material = certified_transaction.auth_sig().clone();
        let effects = ExecutionEffects::CertifiedTransactionEffects(
            Envelope::new_from_data_and_sig(transaction_effects.unwrap(), signed_material),
            transaction_events.unwrap(),
        );
        Ok(effects)
    }

    fn clone_committee(&self) -> Committee {
        self.qd.clone_committee()
    }

    fn get_current_epoch(&self) -> EpochId {
        self.qd.current_epoch()
    }

    fn clone_new(&self) -> Box<dyn ValidatorProxy + Send + Sync> {
        let qdh = self._qd_handler.clone_new();
        let qd = qdh.clone_quorum_driver();
        Box::new(Self {
            _qd_handler: qdh,
            qd,
            clients: self.clients.clone(),
            committee: self.committee.clone(),
            requests: Mutex::new(JoinSet::new()),
        })
    }

    async fn get_validators(&self) -> Result<Vec<SuiAddress>, anyhow::Error> {
        let system_state = self.get_latest_system_state_object().await?;
        Ok(system_state
            .active_validators
            .iter()
            .map(|v| v.sui_address)
            .collect())
    }
}

pub struct FullNodeProxy {
    sui_client: SuiClient,
    committee: Committee,
}

impl FullNodeProxy {
    pub async fn from_url(http_url: &str) -> Result<Self, anyhow::Error> {
        // Each request times out after 60s (default value)
        let sui_client = SuiClientBuilder::default()
            .max_concurrent_requests(500_000)
            .build(http_url)
            .await?;

        let resp = sui_client.read_api().get_committee_info(None).await?;
        let epoch = resp.epoch;
        let committee_vec = resp.validators;
        let committee_map =
            BTreeMap::from_iter(committee_vec.into_iter().map(|(name, stake)| (name, stake)));
        let committee =
            Committee::new_for_testing_with_normalized_voting_power(epoch, committee_map);

        Ok(Self {
            sui_client,
            committee,
        })
    }
}

#[async_trait]
impl ValidatorProxy for FullNodeProxy {
    async fn get_object(&self, object_id: ObjectID) -> Result<Object, anyhow::Error> {
        let response = self
            .sui_client
            .read_api()
            .get_object_with_options(object_id, SuiObjectDataOptions::bcs_lossless())
            .await?;

        if let Some(sui_object) = response.data {
            sui_object.try_into()
        } else if let Some(error) = response.error {
            bail!("Error getting object {:?}: {}", object_id, error)
        } else {
            bail!("Object {:?} not found and no error provided", object_id)
        }
    }

    async fn get_latest_system_state_object(&self) -> Result<SuiSystemStateSummary, anyhow::Error> {
        Ok(self
            .sui_client
            .governance_api()
            .get_latest_sui_system_state()
            .await?)
    }

    async fn execute_transaction_block(&self, tx: Transaction) -> anyhow::Result<ExecutionEffects> {
        let tx_digest = *tx.digest();
        let tx = tx.verify()?;
        let mut retry_cnt = 0;
        while retry_cnt < 10 {
            // Fullnode could time out after WAIT_FOR_FINALITY_TIMEOUT (30s) in TransactionOrchestrator
            // SuiClient times out after 60s
            match self
                .sui_client
                .quorum_driver()
                .execute_transaction_block(
                    tx.clone(),
                    SuiTransactionBlockResponseOptions::new().with_effects(),
                    None,
                )
                .await
            {
                Ok(resp) => {
                    let effects = ExecutionEffects::SuiTransactionBlockEffects(
                        resp.effects.expect("effects field should not be None"),
                    );
                    return Ok(effects);
                }
                Err(err) => {
                    error!(
                        ?tx_digest,
                        retry_cnt, "Transaction failed with err: {:?}", err
                    );
                    retry_cnt += 1;
                }
            }
        }
        bail!("Transaction {:?} failed for {retry_cnt} times", tx_digest);
    }

    async fn execute_bench_transaction(&self, tx: Transaction) -> anyhow::Result<ExecutionEffects> {
        self.execute_transaction_block(tx).await
    }

    fn clone_committee(&self) -> Committee {
        self.committee.clone()
    }

    fn get_current_epoch(&self) -> EpochId {
        self.committee.epoch
    }

    fn clone_new(&self) -> Box<dyn ValidatorProxy + Send + Sync> {
        Box::new(Self {
            sui_client: self.sui_client.clone(),
            committee: self.clone_committee(),
        })
    }

    async fn get_validators(&self) -> Result<Vec<SuiAddress>, anyhow::Error> {
        let validators = self
            .sui_client
            .governance_api()
            .get_latest_sui_system_state()
            .await?
            .active_validators;
        Ok(validators.into_iter().map(|v| v.sui_address).collect())
    }
}
