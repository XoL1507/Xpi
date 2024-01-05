// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::workload::{submit_transaction, Gas, Payload, Workload, WorkloadType};
use crate::workloads::workload::{get_latest, transfer_sui_for_testing, MAX_GAS_FOR_TESTING};
use async_trait::async_trait;
use futures::future::join_all;
use std::{path::PathBuf, sync::Arc};
use sui_core::{
    authority_aggregator::AuthorityAggregator, authority_client::NetworkAuthorityClient,
};
use sui_types::{
    base_types::{ObjectID, ObjectRef, SuiAddress},
    crypto::{get_key_pair, AccountKeyPair, EmptySignInfo},
    messages::TransactionEnvelope,
    object::Owner,
};
use test_utils::messages::{make_counter_create_transaction, make_counter_increment_transaction};
use test_utils::{
    messages::create_publish_move_package_transaction, transaction::parse_package_ref,
};

pub struct SharedCounterTestPayload {
    package_ref: ObjectRef,
    counter_id: ObjectID,
    gas: Gas,
    sender: SuiAddress,
    keypair: Arc<AccountKeyPair>,
}

impl Payload for SharedCounterTestPayload {
    fn make_new_payload(self: Box<Self>, _: ObjectRef, new_gas: ObjectRef) -> Box<dyn Payload> {
        Box::new(SharedCounterTestPayload {
            package_ref: self.package_ref,
            counter_id: self.counter_id,
            gas: (new_gas, self.gas.1),
            sender: self.sender,
            keypair: self.keypair.clone(),
        })
    }
    fn make_transaction(&self) -> TransactionEnvelope<EmptySignInfo> {
        make_counter_increment_transaction(
            self.gas.0,
            self.package_ref,
            self.counter_id,
            self.sender,
            &self.keypair,
        )
    }
    fn get_object_id(&self) -> ObjectID {
        self.counter_id
    }
    fn get_workload_type(&self) -> WorkloadType {
        WorkloadType::SharedCounter
    }
}

pub struct SharedCounterWorkload {
    pub test_gas: ObjectID,
    pub test_gas_owner: SuiAddress,
    pub test_gas_keypair: Arc<AccountKeyPair>,
    pub basics_package_ref: Option<ObjectRef>,
}

impl SharedCounterWorkload {
    pub fn new_boxed(
        gas: ObjectID,
        owner: SuiAddress,
        keypair: Arc<AccountKeyPair>,
        basics_package_ref: Option<ObjectRef>,
    ) -> Box<dyn Workload<dyn Payload>> {
        Box::<dyn Workload<dyn Payload>>::from(Box::new(SharedCounterWorkload {
            test_gas: gas,
            test_gas_owner: owner,
            test_gas_keypair: keypair,
            basics_package_ref,
        }))
    }
}

pub async fn publish_basics_package(
    gas: ObjectRef,
    aggregator: Arc<AuthorityAggregator<NetworkAuthorityClient>>,
    sender: SuiAddress,
    keypair: &AccountKeyPair,
) -> ObjectRef {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("../../sui_programmability/examples/basics");
    let transaction = create_publish_move_package_transaction(gas, path, sender, keypair);
    let effects = submit_transaction(transaction, aggregator).await.unwrap();
    parse_package_ref(&effects).unwrap()
}

#[async_trait]
impl Workload<dyn Payload> for SharedCounterWorkload {
    async fn init(&mut self, aggregator: Arc<AuthorityAggregator<NetworkAuthorityClient>>) {
        if self.basics_package_ref.is_some() {
            return;
        }
        // publish basics package
        let primary_gas = get_latest(self.test_gas, aggregator.clone()).await.unwrap();
        let primary_gas_ref = primary_gas.compute_object_reference();
        let mut publish_module_gas_ref = None;
        let (address, keypair) = get_key_pair();
        if let Some((_updated, minted)) = transfer_sui_for_testing(
            (primary_gas_ref, Owner::AddressOwner(self.test_gas_owner)),
            &self.test_gas_keypair,
            MAX_GAS_FOR_TESTING,
            address,
            aggregator.clone(),
        )
        .await
        {
            publish_module_gas_ref = Some((address, keypair, minted));
        }
        // Publish basics package
        eprintln!("Publishing basics package");
        let publish_module_gas = publish_module_gas_ref.unwrap();
        self.basics_package_ref = Some(
            publish_basics_package(
                publish_module_gas.2,
                aggregator,
                publish_module_gas.0,
                &publish_module_gas.1,
            )
            .await,
        )
    }
    async fn make_test_payloads(
        &self,
        count: u64,
        aggregator: Arc<AuthorityAggregator<NetworkAuthorityClient>>,
    ) -> Vec<Box<dyn Payload>> {
        // Read latest test gas object
        let primary_gas = get_latest(self.test_gas, aggregator.clone()).await.unwrap();
        let mut primary_gas_ref = primary_gas.compute_object_reference();
        // Make as many gas objects as the number of counters
        let mut counters_gas = vec![];
        for _ in 0..count {
            let (address, keypair) = get_key_pair();
            if let Some((updated, minted)) = transfer_sui_for_testing(
                (primary_gas_ref, Owner::AddressOwner(self.test_gas_owner)),
                &self.test_gas_keypair,
                MAX_GAS_FOR_TESTING,
                address,
                aggregator.clone(),
            )
            .await
            {
                primary_gas_ref = updated;
                counters_gas.push((address, keypair, minted));
            }
        }
        let agg = &aggregator;
        // create counters using gas objects we created above
        eprintln!("Creating shared counters, this may take a while..");
        let futures = counters_gas
            .into_iter()
            .map(|(sender, keypair, gas)| async move {
                let transaction = make_counter_create_transaction(
                    gas,
                    self.basics_package_ref.unwrap(),
                    sender,
                    &keypair,
                );
                if let Some(effects) = submit_transaction(transaction, agg.clone()).await {
                    Box::new(SharedCounterTestPayload {
                        package_ref: self.basics_package_ref.unwrap(),
                        counter_id: effects.created[0].0 .0,
                        gas: effects.gas_object,
                        sender,
                        keypair: Arc::new(keypair),
                    })
                } else {
                    panic!("Failed to create shared counter!");
                }
            });
        join_all(futures)
            .await
            .into_iter()
            .map(|b| Box::<dyn Payload>::from(b))
            .collect()
    }
}
