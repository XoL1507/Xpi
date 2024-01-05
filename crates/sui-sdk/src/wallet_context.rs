// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::sui_client_config::SuiClientConfig;
use crate::SuiClient;
use anyhow::anyhow;
use colored::Colorize;
use shared_crypto::intent::Intent;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use sui_config::{Config, PersistedConfig};
use sui_json_rpc_types::{
    get_new_package_obj_from_response, SuiObjectData, SuiObjectDataFilter, SuiObjectDataOptions,
    SuiObjectResponse, SuiObjectResponseQuery, SuiTransactionBlockEffectsAPI,
    SuiTransactionBlockResponse, SuiTransactionBlockResponseOptions,
};
use sui_keys::keystore::AccountKeystore;
use sui_test_transaction_builder::TestTransactionBuilder;
use sui_types::base_types::{ObjectID, ObjectRef, SequenceNumber, SuiAddress, TransactionDigest};
use sui_types::crypto::{get_key_pair, AccountKeyPair};
use sui_types::gas_coin::GasCoin;
use sui_types::object::Owner;
use sui_types::transaction::{
    Transaction, TransactionData, TransactionDataAPI, VerifiedTransaction,
    TEST_ONLY_GAS_UNIT_FOR_TRANSFER,
};
use tokio::sync::RwLock;
use tracing::warn;

pub struct WalletContext {
    pub config: PersistedConfig<SuiClientConfig>,
    request_timeout: Option<std::time::Duration>,
    client: Arc<RwLock<Option<SuiClient>>>,
    max_concurrent_requests: Option<u64>,
}

impl WalletContext {
    pub async fn new(
        config_path: &Path,
        request_timeout: Option<std::time::Duration>,
        max_concurrent_requests: Option<u64>,
    ) -> clap::Result<Self, anyhow::Error> {
        let config: SuiClientConfig = PersistedConfig::read(config_path).map_err(|err| {
            anyhow!(
                "Cannot open wallet config file at {:?}. Err: {err}",
                config_path
            )
        })?;

        let config = config.persisted(config_path);
        let context = Self {
            config,
            request_timeout,
            client: Default::default(),
            max_concurrent_requests,
        };
        Ok(context)
    }

    pub fn get_addresses(&self) -> Vec<SuiAddress> {
        self.config.keystore.addresses()
    }

    pub async fn get_client(&self) -> clap::Result<SuiClient, anyhow::Error> {
        let read = self.client.read().await;

        Ok(if let Some(client) = read.as_ref() {
            client.clone()
        } else {
            drop(read);
            let client = self
                .config
                .get_active_env()?
                .create_rpc_client(self.request_timeout, self.max_concurrent_requests)
                .await?;
            if let Err(e) = client.check_api_version() {
                warn!("{e}");
                eprintln!("{}", format!("[warn] {e}").yellow().bold());
            }
            self.client.write().await.insert(client).clone()
        })
    }

    // TODO: Ger rid of mut
    pub fn active_address(&mut self) -> clap::Result<SuiAddress, anyhow::Error> {
        if self.config.keystore.addresses().is_empty() {
            return Err(anyhow!(
                "No managed addresses. Create new address with `new-address` command."
            ));
        }

        // Ok to unwrap because we checked that config addresses not empty
        // Set it if not exists
        self.config.active_address = Some(
            self.config
                .active_address
                .unwrap_or(*self.config.keystore.addresses().get(0).unwrap()),
        );

        Ok(self.config.active_address.unwrap())
    }

    /// Get the latest object reference given a object id
    pub async fn get_object_ref(
        &self,
        object_id: ObjectID,
    ) -> clap::Result<ObjectRef, anyhow::Error> {
        let client = self.get_client().await?;
        Ok(client
            .read_api()
            .get_object_with_options(object_id, SuiObjectDataOptions::new())
            .await?
            .into_object()?
            .object_ref())
    }

    /// Get all the gas objects (and conveniently, gas amounts) for the address
    pub async fn gas_objects(
        &self,
        address: SuiAddress,
    ) -> clap::Result<Vec<(u64, SuiObjectData)>, anyhow::Error> {
        let client = self.get_client().await?;

        let mut objects: Vec<SuiObjectResponse> = Vec::new();
        let mut cursor = None;
        loop {
            let response = client
                .read_api()
                .get_owned_objects(
                    address,
                    Some(SuiObjectResponseQuery::new(
                        Some(SuiObjectDataFilter::StructType(GasCoin::type_())),
                        Some(SuiObjectDataOptions::full_content()),
                    )),
                    cursor,
                    None,
                )
                .await?;

            objects.extend(response.data);

            if response.has_next_page {
                cursor = response.next_cursor;
            } else {
                break;
            }
        }

        // TODO: We should ideally fetch the objects from local cache
        let mut values_objects = Vec::new();

        for object in objects {
            let o = object.data;
            if let Some(o) = o {
                let gas_coin = GasCoin::try_from(&o)?;
                values_objects.push((gas_coin.value(), o.clone()));
            }
        }

        Ok(values_objects)
    }

    pub async fn get_object_owner(&self, id: &ObjectID) -> clap::Result<SuiAddress, anyhow::Error> {
        let client = self.get_client().await?;
        let object = client
            .read_api()
            .get_object_with_options(*id, SuiObjectDataOptions::new().with_owner())
            .await?
            .into_object()?;
        Ok(object
            .owner
            .ok_or_else(|| anyhow!("Owner field is None"))?
            .get_owner_address()?)
    }

    pub async fn try_get_object_owner(
        &self,
        id: &Option<ObjectID>,
    ) -> clap::Result<Option<SuiAddress>, anyhow::Error> {
        if let Some(id) = id {
            Ok(Some(self.get_object_owner(id).await?))
        } else {
            Ok(None)
        }
    }

    /// Find a gas object which fits the budget
    pub async fn gas_for_owner_budget(
        &self,
        address: SuiAddress,
        budget: u64,
        forbidden_gas_objects: BTreeSet<ObjectID>,
    ) -> clap::Result<(u64, SuiObjectData), anyhow::Error> {
        for o in self.gas_objects(address).await.unwrap() {
            if o.0 >= budget && !forbidden_gas_objects.contains(&o.1.object_id) {
                return Ok((o.0, o.1));
            }
        }
        Err(anyhow!(
            "No non-argument gas objects found with value >= budget {budget}"
        ))
    }

    pub async fn get_all_gas_objects_owned_by_address(
        &self,
        address: SuiAddress,
    ) -> anyhow::Result<Vec<ObjectRef>> {
        self.get_gas_objects_owned_by_address(address, None).await
    }

    pub async fn get_gas_objects_owned_by_address(
        &self,
        address: SuiAddress,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<ObjectRef>> {
        let client = self.get_client().await?;
        let results: Vec<_> = client
            .read_api()
            .get_owned_objects(
                address,
                Some(SuiObjectResponseQuery::new(
                    Some(SuiObjectDataFilter::StructType(GasCoin::type_())),
                    Some(SuiObjectDataOptions::full_content()),
                )),
                None,
                limit,
            )
            .await?
            .data
            .into_iter()
            .filter_map(|r| r.data.map(|o| o.object_ref()))
            .collect();
        Ok(results)
    }

    /// Given an address, return one gas object owned by this address.
    /// The actual implementation just returns the first one returned by the read api.
    pub async fn get_one_gas_object_owned_by_address(
        &self,
        address: SuiAddress,
    ) -> anyhow::Result<Option<ObjectRef>> {
        Ok(self
            .get_gas_objects_owned_by_address(address, Some(1))
            .await?
            .pop())
    }

    /// Returns one address and all gas objects owned by that address.
    pub async fn get_one_account(&self) -> anyhow::Result<(SuiAddress, Vec<ObjectRef>)> {
        let address = self.get_addresses().pop().unwrap();
        Ok((
            address,
            self.get_all_gas_objects_owned_by_address(address).await?,
        ))
    }

    /// Return a gas object owned by an arbitrary address managed by the wallet.
    pub async fn get_one_gas_object(&self) -> anyhow::Result<Option<(SuiAddress, ObjectRef)>> {
        for address in self.get_addresses() {
            if let Some(gas_object) = self.get_one_gas_object_owned_by_address(address).await? {
                return Ok(Some((address, gas_object)));
            }
        }
        Ok(None)
    }

    /// Returns all the account addresses managed by the wallet and their owned gas objects.
    pub async fn get_all_accounts_and_gas_objects(
        &self,
    ) -> anyhow::Result<Vec<(SuiAddress, Vec<ObjectRef>)>> {
        let mut result = vec![];
        for address in self.get_addresses() {
            let objects = self
                .gas_objects(address)
                .await?
                .into_iter()
                .map(|(_, o)| o.object_ref())
                .collect();
            result.push((address, objects));
        }
        Ok(result)
    }

    pub async fn get_reference_gas_price(&self) -> clap::Result<u64, anyhow::Error> {
        let client = self.get_client().await?;
        let gas_price = client.governance_api().get_reference_gas_price().await?;
        Ok(gas_price)
    }

    /// Sign a transaction with a key currently managed by the WalletContext
    pub fn sign_transaction(&self, data: &TransactionData) -> VerifiedTransaction {
        let sig = self
            .config
            .keystore
            .sign_secure(&data.sender(), data, Intent::sui_transaction())
            .unwrap();
        // TODO: To support sponsored transaction, we should also look at the gas owner.
        VerifiedTransaction::new_unchecked(Transaction::from_data(
            data.clone(),
            Intent::sui_transaction(),
            vec![sig],
        ))
    }

    /// Execute a transaction and wait for it to be locally executed on the fullnode.
    /// Also expects the effects status to be ExecutionStatus::Success.
    pub async fn execute_transaction_must_succeed(
        &self,
        tx: VerifiedTransaction,
    ) -> SuiTransactionBlockResponse {
        let response = self.execute_transaction_may_fail(tx).await.unwrap();
        assert!(response.status_ok().unwrap());
        response
    }

    /// Execute a transaction and wait for it to be locally executed on the fullnode.
    /// The transaction execution is not guaranteed to succeed and may fail. This is usually only
    /// needed in non-test environment or the caller is explicitly testing some failure behavior.
    pub async fn execute_transaction_may_fail(
        &self,
        tx: VerifiedTransaction,
    ) -> anyhow::Result<SuiTransactionBlockResponse> {
        let client = self.get_client().await?;
        Ok(client
            .quorum_driver_api()
            .execute_transaction_block(
                tx,
                SuiTransactionBlockResponseOptions::new()
                    .with_effects()
                    .with_events()
                    .with_input()
                    .with_events()
                    .with_object_changes()
                    .with_balance_changes(),
                Some(sui_types::quorum_driver_types::ExecuteTransactionRequestType::WaitForLocalExecution),
            )
            .await?)
    }

    /// A helper function to make Transactions with controlled accounts in WalletContext.
    /// Particularly, the wallet needs to own gas objects for transactions.
    /// However, if this function is called multiple times without any "sync" actions
    /// on gas object management, txns may fail and objects may be locked.
    ///
    /// The param is called `max_txn_num` because it does not always return the exact
    /// same amount of Transactions, for example when there are not enough gas objects
    /// controlled by the WalletContext. Caller should rely on the return value to
    /// check the count.
    pub async fn batch_make_transfer_transactions(
        &self,
        max_txn_num: usize,
    ) -> Vec<VerifiedTransaction> {
        let recipient = get_key_pair::<AccountKeyPair>().0;
        let accounts_and_objs = self.get_all_accounts_and_gas_objects().await.unwrap();
        let mut res = Vec::with_capacity(max_txn_num);

        let gas_price = self.get_reference_gas_price().await.unwrap();
        for (address, objs) in accounts_and_objs {
            for obj in objs {
                if res.len() >= max_txn_num {
                    return res;
                }
                let data = TransactionData::new_transfer_sui(
                    recipient,
                    address,
                    Some(2),
                    obj,
                    gas_price * TEST_ONLY_GAS_UNIT_FOR_TRANSFER,
                    gas_price,
                );
                let tx = self.sign_transaction(&data);
                res.push(tx);
            }
        }
        res
    }

    pub async fn make_transfer_sui_transaction(
        &self,
        recipient: Option<SuiAddress>,
        amount: Option<u64>,
    ) -> VerifiedTransaction {
        let (sender, gas_object) = self.get_one_gas_object().await.unwrap().unwrap();
        let gas_price = self.get_reference_gas_price().await.unwrap();
        self.sign_transaction(
            &TestTransactionBuilder::new(sender, gas_object, gas_price)
                .transfer_sui(amount, recipient.unwrap_or(sender))
                .build(),
        )
    }

    pub async fn make_staking_transaction(
        &self,
        validator_address: SuiAddress,
    ) -> VerifiedTransaction {
        let accounts_and_objs = self.get_all_accounts_and_gas_objects().await.unwrap();
        let sender = accounts_and_objs[0].0;
        let gas_object = accounts_and_objs[0].1[0];
        let stake_object = accounts_and_objs[0].1[1];
        let gas_price = self.get_reference_gas_price().await.unwrap();
        self.sign_transaction(
            &TestTransactionBuilder::new(sender, gas_object, gas_price)
                .call_staking(stake_object, validator_address)
                .build(),
        )
    }

    pub async fn make_publish_transaction(&self, path: PathBuf) -> VerifiedTransaction {
        let (sender, gas_object) = self.get_one_gas_object().await.unwrap().unwrap();
        let gas_price = self.get_reference_gas_price().await.unwrap();
        self.sign_transaction(
            &TestTransactionBuilder::new(sender, gas_object, gas_price)
                .publish(path)
                .build(),
        )
    }

    pub async fn make_publish_transaction_with_deps(&self, path: PathBuf) -> VerifiedTransaction {
        let (sender, gas_object) = self.get_one_gas_object().await.unwrap().unwrap();
        let gas_price = self.get_reference_gas_price().await.unwrap();
        self.sign_transaction(
            &TestTransactionBuilder::new(sender, gas_object, gas_price)
                .publish_with_deps(path)
                .build(),
        )
    }

    pub async fn publish_package(&self, path: PathBuf) -> ObjectRef {
        let (sender, gas_object) = self.get_one_gas_object().await.unwrap().unwrap();
        let gas_price = self.get_reference_gas_price().await.unwrap();
        let txn = self.sign_transaction(
            &TestTransactionBuilder::new(sender, gas_object, gas_price)
                .publish(path)
                .build(),
        );
        let resp = self.execute_transaction_must_succeed(txn).await;
        get_new_package_obj_from_response(&resp).unwrap()
    }

    /// Executes a transaction to publish the `basics` package and returns the package object ref.
    pub async fn publish_basics_package(&self) -> ObjectRef {
        let (sender, gas_object) = self.get_one_gas_object().await.unwrap().unwrap();
        let gas_price = self.get_reference_gas_price().await.unwrap();
        let txn = self.sign_transaction(
            &TestTransactionBuilder::new(sender, gas_object, gas_price)
                .publish_examples("basics")
                .build(),
        );
        let resp = self.execute_transaction_must_succeed(txn).await;
        get_new_package_obj_from_response(&resp).unwrap()
    }

    /// Executes a transaction to publish the `basics` package and another one to create a counter.
    /// Returns the package object ref and the counter object ref.
    pub async fn publish_basics_package_and_make_counter(&self) -> (ObjectRef, ObjectRef) {
        let package_ref = self.publish_basics_package().await;
        let (sender, gas_object) = self.get_one_gas_object().await.unwrap().unwrap();
        let gas_price = self.get_reference_gas_price().await.unwrap();
        let counter_creation_txn = self.sign_transaction(
            &TestTransactionBuilder::new(sender, gas_object, gas_price)
                .call_counter_create(package_ref.0)
                .build(),
        );
        let resp = self
            .execute_transaction_must_succeed(counter_creation_txn)
            .await;
        let counter_ref = resp
            .effects
            .unwrap()
            .created()
            .iter()
            .find(|obj_ref| matches!(obj_ref.owner, Owner::Shared { .. }))
            .unwrap()
            .reference
            .to_object_ref();
        (package_ref, counter_ref)
    }

    /// Executes a transaction to increment a counter object.
    /// Must be called after calling `publish_basics_package_and_make_counter`.
    pub async fn increment_counter(
        &self,
        sender: SuiAddress,
        gas_object_id: Option<ObjectID>,
        package_id: ObjectID,
        counter_id: ObjectID,
        initial_shared_version: SequenceNumber,
    ) -> SuiTransactionBlockResponse {
        let gas_object = if let Some(gas_object_id) = gas_object_id {
            self.get_object_ref(gas_object_id).await.unwrap()
        } else {
            self.get_one_gas_object_owned_by_address(sender)
                .await
                .unwrap()
                .unwrap()
        };
        let rgp = self.get_reference_gas_price().await.unwrap();
        let txn = self.sign_transaction(
            &TestTransactionBuilder::new(sender, gas_object, rgp)
                .call_counter_increment(package_id, counter_id, initial_shared_version)
                .build(),
        );
        self.execute_transaction_must_succeed(txn).await
    }

    /// Executes a transaction to publish the `nfts` package and returns the package id, id of the gas object used, and the digest of the transaction.
    pub async fn publish_nfts_package(&self) -> (ObjectID, ObjectID, TransactionDigest) {
        let (sender, gas_object) = self.get_one_gas_object().await.unwrap().unwrap();
        let gas_id = gas_object.0;
        let gas_price = self.get_reference_gas_price().await.unwrap();
        let txn = self.sign_transaction(
            &TestTransactionBuilder::new(sender, gas_object, gas_price)
                .publish_examples("nfts")
                .build(),
        );
        let resp = self.execute_transaction_must_succeed(txn).await;
        let package_id = get_new_package_obj_from_response(&resp).unwrap().0;
        (package_id, gas_id, resp.digest)
    }

    /// Pre-requisite: `publish_nfts_package` must be called before this function.
    /// Executes a transaction to create an NFT and returns the sender address, the object id of the NFT, and the digest of the transaction.
    pub async fn create_devnet_nft(
        &self,
        package_id: ObjectID,
    ) -> (SuiAddress, ObjectID, TransactionDigest) {
        let (sender, gas_object) = self.get_one_gas_object().await.unwrap().unwrap();
        let rgp = self.get_reference_gas_price().await.unwrap();

        let txn = self.sign_transaction(
            &TestTransactionBuilder::new(sender, gas_object, rgp)
                .call_nft_create(package_id)
                .build(),
        );
        let resp = self.execute_transaction_must_succeed(txn).await;

        let object_id = resp
            .effects
            .as_ref()
            .unwrap()
            .created()
            .first()
            .unwrap()
            .reference
            .object_id;

        (sender, object_id, resp.digest)
    }

    /// Executes a transaction to delete the given NFT.
    pub async fn delete_devnet_nft(
        &self,
        sender: SuiAddress,
        package_id: ObjectID,
        nft_to_delete: ObjectRef,
    ) -> SuiTransactionBlockResponse {
        let gas = self
            .get_one_gas_object_owned_by_address(sender)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("Expect {sender} to have at least one gas object"));
        let rgp = self.get_reference_gas_price().await.unwrap();
        let txn = self.sign_transaction(
            &TestTransactionBuilder::new(sender, gas, rgp)
                .call_nft_delete(package_id, nft_to_delete)
                .build(),
        );
        self.execute_transaction_must_succeed(txn).await
    }
}
