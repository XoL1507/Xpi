// Copyright (c) 2021, Facebook, Inc. and its affiliates
// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use sui_test_transaction_builder::TestTransactionBuilder;
use sui_types::error::{SuiError, SuiResult};
use sui_types::transaction::VerifiedTransaction;
use sui_types::utils::{get_zklogin_user_address, make_zklogin_tx, sign_zklogin_tx};
use test_utils::authority::{spawn_test_authorities, test_authority_configs_with_objects};
use test_utils::network::TestClusterBuilder;

use sui_core::{authority_aggregator::AuthorityAggregatorBuilder, authority_client::AuthorityAPI};
use sui_macros::sim_test;
use sui_types::object::generate_test_gas_objects;

async fn do_zklogin_test() -> SuiResult {
    let gas_objects = generate_test_gas_objects();

    // Get the authority configs and spawn them. Note that it is important to not drop
    // the handles (or the authorities will stop).
    let (config, _) = test_authority_configs_with_objects(gas_objects);
    let _handles = spawn_test_authorities(&config).await;

    let (_, tx, _) = make_zklogin_tx();

    let (_agg, clients) = AuthorityAggregatorBuilder::from_network_config(&config)
        .build()
        .unwrap();

    clients
        .values()
        .next()
        .unwrap()
        .handle_transaction(tx)
        .await
        .map(|_| ())
}

#[sim_test]
async fn test_zklogin_feature_deny() {
    use sui_protocol_config::ProtocolConfig;

    let _guard = ProtocolConfig::apply_overrides_for_testing(|_, mut config| {
        config.set_zklogin_auth(false);
        config
    });

    let err = do_zklogin_test().await.unwrap_err();

    assert!(matches!(err, SuiError::UnsupportedFeatureError { .. }));
}

#[sim_test]
async fn test_zklogin_feature_allow() {
    use sui_protocol_config::ProtocolConfig;

    let _guard = ProtocolConfig::apply_overrides_for_testing(|_, mut config| {
        config.set_zklogin_auth(true);
        config
    });

    let err = do_zklogin_test().await.unwrap_err();

    // we didn't make a real transaction with a valid object, but we verify that we pass the
    // feature gate.
    assert!(matches!(err, SuiError::UserInputError { .. }));
}

#[sim_test]
async fn zklogin_end_to_end_test() {
    let mut test_cluster = TestClusterBuilder::new().build().await.unwrap();
    let rgp = test_cluster.get_reference_gas_price().await;
    let sender = test_cluster.get_address_0();

    let context = &mut test_cluster.wallet;
    let accounts_and_objs = context.get_all_accounts_and_gas_objects().await.unwrap();
    let gas_object = accounts_and_objs[0].1[0];
    let object_to_send = accounts_and_objs[0].1[1];

    let zklogin_addr = get_zklogin_user_address();

    // first send an object to the zklogin address.
    let txn = context.sign_transaction(
        &TestTransactionBuilder::new(sender, gas_object, rgp)
            .transfer(object_to_send, zklogin_addr)
            .build(),
    );

    context.execute_transaction_must_succeed(txn).await;

    // now send it back
    let gas_object = context
        .get_gas_objects_owned_by_address(zklogin_addr, None)
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();

    let txn = TestTransactionBuilder::new(zklogin_addr, gas_object, rgp)
        .transfer_sui(None, sender)
        .build();

    let (_, signed_txn, _) = sign_zklogin_tx(txn);

    context
        .execute_transaction_must_succeed(VerifiedTransaction::new_unchecked(signed_txn))
        .await;

    assert!(context
        .get_gas_objects_owned_by_address(zklogin_addr, None)
        .await
        .unwrap()
        .is_empty());
}
