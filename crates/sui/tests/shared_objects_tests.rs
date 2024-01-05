// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;
use sui_core::authority_client::AuthorityAPI;
use sui_core::gateway_state::{GatewayAPI, GatewayState};
use sui_types::messages::{
    CallArg, ExecutionStatus, ObjectArg, ObjectInfoRequest, ObjectInfoRequestKind,
};
use sui_types::object::OBJECT_START_VERSION;
use test_utils::authority::{get_client, test_authority_aggregator};
use test_utils::transaction::{
    publish_counter_package, submit_shared_object_transaction, submit_single_owner_transaction,
};
use test_utils::{
    authority::{spawn_test_authorities, test_authority_configs},
    messages::{move_transaction, test_shared_object_transactions},
    objects::{test_gas_objects, test_shared_object},
};

/// Send a simple shared object transaction to Sui and ensures the client gets back a response.
#[tokio::test]
async fn shared_object_transaction() {
    let mut objects = test_gas_objects();
    objects.push(test_shared_object());

    // Get the authority configs and spawn them. Note that it is important to not drop
    // the handles (or the authorities will stop).
    let configs = test_authority_configs();
    let _handles = spawn_test_authorities(objects, &configs).await;

    // Make a test shared object certificate.
    let transaction = test_shared_object_transactions().pop().unwrap();

    // Submit the transaction. Note that this transaction is random and we do not expect
    // it to be successfully executed by the Move execution engine.
    tokio::task::yield_now().await;
    let _effects = submit_shared_object_transaction(transaction, &configs.validator_set()[0..1])
        .await
        .unwrap();
}

/// Same as `shared_object_transaction` but every authorities submit the transaction.
#[tokio::test]
async fn many_shared_object_transactions() {
    let mut objects = test_gas_objects();
    objects.push(test_shared_object());

    // Get the authority configs and spawn them. Note that it is important to not drop
    // the handles (or the authorities will stop).
    let configs = test_authority_configs();
    let _handles = spawn_test_authorities(objects, &configs).await;

    // Make a test shared object certificate.
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    let transaction = test_shared_object_transactions().pop().unwrap();

    // Submit the transaction. Note that this transaction is random and we do not expect
    // it to be successfully executed by the Move execution engine.
    tokio::task::yield_now().await;
    let _effects = submit_shared_object_transaction(transaction, configs.validator_set())
        .await
        .unwrap();
}

/// End-to-end shared transaction test for a Sui validator. It does not test the client, wallet,
/// or gateway but tests the end-to-end flow from Sui to consensus.
#[tokio::test]
async fn call_shared_object_contract() {
    let mut gas_objects = test_gas_objects();

    // Get the authority configs and spawn them. Note that it is important to not drop
    // the handles (or the authorities will stop).
    let configs = test_authority_configs();
    let _handles = spawn_test_authorities(gas_objects.clone(), &configs).await;
    // Publish the move package to all authorities and get the new package ref.
    tokio::task::yield_now().await;
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    let package_ref =
        publish_counter_package(gas_objects.pop().unwrap(), configs.validator_set()).await;

    // Make a transaction to create a counter.
    tokio::task::yield_now().await;
    let transaction = move_transaction(
        gas_objects.pop().unwrap(),
        "counter",
        "create",
        package_ref,
        /* arguments */ Vec::default(),
    );
    let effects = submit_single_owner_transaction(transaction, configs.validator_set()).await;
    assert!(matches!(effects.status, ExecutionStatus::Success { .. }));
    let ((counter_id, _, _), _) = effects.created[0];

    // Ensure the value of the counter is `0`.
    tokio::task::yield_now().await;
    let transaction = move_transaction(
        gas_objects.pop().unwrap(),
        "counter",
        "assert_value",
        package_ref,
        vec![
            CallArg::Object(ObjectArg::SharedObject(counter_id)),
            CallArg::Pure(0u64.to_le_bytes().to_vec()),
        ],
    );
    let effects = submit_shared_object_transaction(transaction, &configs.validator_set()[0..1])
        .await
        .unwrap();
    assert!(matches!(effects.status, ExecutionStatus::Success { .. }));

    // Make a transaction to increment the counter.
    tokio::task::yield_now().await;
    let transaction = move_transaction(
        gas_objects.pop().unwrap(),
        "counter",
        "increment",
        package_ref,
        vec![CallArg::Object(ObjectArg::SharedObject(counter_id))],
    );
    let effects = submit_shared_object_transaction(transaction, &configs.validator_set()[0..1])
        .await
        .unwrap();
    assert!(matches!(effects.status, ExecutionStatus::Success { .. }));

    // Ensure the value of the counter is `1`.
    tokio::task::yield_now().await;
    let transaction = move_transaction(
        gas_objects.pop().unwrap(),
        "counter",
        "assert_value",
        package_ref,
        vec![
            CallArg::Object(ObjectArg::SharedObject(counter_id)),
            CallArg::Pure(1u64.to_le_bytes().to_vec()),
        ],
    );
    let effects = submit_shared_object_transaction(transaction, &configs.validator_set()[0..1])
        .await
        .unwrap();
    assert!(matches!(effects.status, ExecutionStatus::Success { .. }));
}

/// Same test as `call_shared_object_contract` but the clients submits many times the same
/// transaction (one copy per authority).
#[tokio::test]
async fn shared_object_flood() {
    telemetry_subscribers::init_for_testing();
    let mut gas_objects = test_gas_objects();

    // Get the authority configs and spawn them. Note that it is important to not drop
    // the handles (or the authorities will stop).
    let configs = test_authority_configs();
    let _handles = spawn_test_authorities(gas_objects.clone(), &configs).await;

    // Publish the move package to all authorities and get the new package ref.
    tokio::task::yield_now().await;
    let package_ref =
        publish_counter_package(gas_objects.pop().unwrap(), configs.validator_set()).await;

    // Make a transaction to create a counter.
    tokio::task::yield_now().await;
    let transaction = move_transaction(
        gas_objects.pop().unwrap(),
        "counter",
        "create",
        package_ref,
        /* arguments */ Vec::default(),
    );
    let effects = submit_single_owner_transaction(transaction, configs.validator_set()).await;
    assert!(matches!(effects.status, ExecutionStatus::Success { .. }));
    let ((counter_id, _, _), _) = effects.created[0];

    // Ensure the value of the counter is `0`.
    tokio::task::yield_now().await;
    let transaction = move_transaction(
        gas_objects.pop().unwrap(),
        "counter",
        "assert_value",
        package_ref,
        vec![
            CallArg::Object(ObjectArg::SharedObject(counter_id)),
            CallArg::Pure(0u64.to_le_bytes().to_vec()),
        ],
    );
    let effects = submit_shared_object_transaction(transaction, configs.validator_set())
        .await
        .unwrap();
    assert!(matches!(effects.status, ExecutionStatus::Success { .. }));

    // Make a transaction to increment the counter.
    tokio::task::yield_now().await;
    let transaction = move_transaction(
        gas_objects.pop().unwrap(),
        "counter",
        "increment",
        package_ref,
        vec![CallArg::Object(ObjectArg::SharedObject(counter_id))],
    );
    let effects = submit_shared_object_transaction(transaction, configs.validator_set())
        .await
        .unwrap();
    assert!(matches!(effects.status, ExecutionStatus::Success { .. }));

    // Ensure the value of the counter is `1`.
    tokio::task::yield_now().await;
    let transaction = move_transaction(
        gas_objects.pop().unwrap(),
        "counter",
        "assert_value",
        package_ref,
        vec![
            CallArg::Object(ObjectArg::SharedObject(counter_id)),
            CallArg::Pure(1u64.to_le_bytes().to_vec()),
        ],
    );
    let effects = submit_shared_object_transaction(transaction, configs.validator_set())
        .await
        .unwrap();
    assert!(matches!(effects.status, ExecutionStatus::Success { .. }));
}

#[tokio::test]
async fn shared_object_sync() {
    let mut gas_objects = test_gas_objects();

    // Get the authority configs and spawn them. Note that it is important to not drop
    // the handles (or the authorities will stop).
    let configs = test_authority_configs();
    let _handles = spawn_test_authorities(gas_objects.clone(), &configs).await;

    // Publish the move package to all authorities and get the new package ref.
    tokio::task::yield_now().await;
    let package_ref =
        publish_counter_package(gas_objects.pop().unwrap(), configs.validator_set()).await;

    // Send a transaction to create a counter, but only to one authority.
    tokio::task::yield_now().await;
    let create_counter_transaction = move_transaction(
        gas_objects.pop().unwrap(),
        "counter",
        "create",
        package_ref,
        /* arguments */ Vec::default(),
    );
    let effects = submit_single_owner_transaction(
        create_counter_transaction.clone(),
        &configs.validator_set()[0..1],
    )
    .await;
    assert!(matches!(effects.status, ExecutionStatus::Success { .. }));
    let ((counter_id, _, _), _) = effects.created[0];

    // Check that the counter object only exist in the first validator, but not the rest.
    get_client(&configs.validator_set()[0])
        .handle_object_info_request(ObjectInfoRequest {
            object_id: counter_id,
            request_kind: ObjectInfoRequestKind::LatestObjectInfo(None),
        })
        .await
        .unwrap()
        .object()
        .unwrap();
    for config in configs.validator_set().iter().skip(1) {
        assert!(get_client(config)
            .handle_object_info_request(ObjectInfoRequest {
                object_id: counter_id,
                request_kind: ObjectInfoRequestKind::LatestObjectInfo(None),
            })
            .await
            .unwrap()
            .object()
            .is_none());
    }

    // Make a transaction to increment the counter.
    tokio::task::yield_now().await;
    let increment_counter_transaction = move_transaction(
        gas_objects.pop().unwrap(),
        "counter",
        "increment",
        package_ref,
        vec![CallArg::Object(ObjectArg::SharedObject(counter_id))],
    );

    // Let's submit the transaction to the first authority (the only one up-to-date).
    let effects = submit_shared_object_transaction(
        increment_counter_transaction.clone(),
        &configs.validator_set()[0..1],
    )
    .await
    .unwrap();
    assert!(matches!(effects.status, ExecutionStatus::Success { .. }));

    // Let's submit the transaction to the out-of-date authorities.
    // Right now grpc doesn't send back the error message in a recoverable way.
    // Ideally we expect Err(SuiError::SharedObjectLockingFailure(_)).
    let _err = submit_shared_object_transaction(
        increment_counter_transaction.clone(),
        &configs.validator_set()[1..],
    )
    .await
    .unwrap_err();

    // Now send the missing certificates to the outdated authorities. We also re-send
    // the transaction to the first authority who should simply ignore it.
    tokio::task::yield_now().await;
    let effects =
        submit_single_owner_transaction(create_counter_transaction, configs.validator_set()).await;
    assert!(matches!(effects.status, ExecutionStatus::Success { .. }));

    // Now we can try again with the shared-object transaction who failed before.
    tokio::task::yield_now().await;
    let effects = submit_shared_object_transaction(
        increment_counter_transaction,
        &configs.validator_set()[1..],
    )
    .await
    .unwrap();
    assert!(matches!(effects.status, ExecutionStatus::Success { .. }));
}

/// Send a simple shared object transaction to Sui and ensures the client gets back a response.
#[tokio::test]
async fn replay_shared_object_transaction() {
    let mut gas_objects = test_gas_objects();

    // Get the authority configs and spawn them. Note that it is important to not drop
    // the handles (or the authorities will stop).
    let configs = test_authority_configs();
    let _handles = spawn_test_authorities(gas_objects.clone(), &configs).await;

    // Publish the move package to all authorities and get the new package ref.
    tokio::task::yield_now().await;
    let package_ref =
        publish_counter_package(gas_objects.pop().unwrap(), configs.validator_set()).await;

    // Send a transaction to create a counter (only to one authority) -- twice.
    tokio::task::yield_now().await;
    let create_counter_transaction = move_transaction(
        gas_objects.pop().unwrap(),
        "counter",
        "create",
        package_ref,
        /* arguments */ Vec::default(),
    );
    for _ in 0..2 {
        let effects = submit_single_owner_transaction(
            create_counter_transaction.clone(),
            &configs.validator_set()[0..1],
        )
        .await;
        assert!(matches!(effects.status, ExecutionStatus::Success { .. }));

        // Ensure the sequence number of the shared object did not change.
        let ((_, seq, _), _) = effects.created[0];
        assert_eq!(seq, OBJECT_START_VERSION);
    }
}

#[tokio::test]
//#[ignore] // cargo test gateway -p sui --test shared_objects_tests -- --nocapture
async fn shared_object_on_gateway() {
    let mut gas_objects = test_gas_objects();

    // Get the authority configs and spawn them. Note that it is important to not drop
    // the handles (or the authorities will stop).
    let configs = test_authority_configs();
    let _handles = spawn_test_authorities(gas_objects.clone(), &configs).await;
    let clients = test_authority_aggregator(&configs);
    let path = tempfile::tempdir().unwrap().into_path();
    let gateway = Arc::new(GatewayState::new_with_authorities(path, clients).unwrap());

    // Publish the move package to all authorities and get the new package ref.
    tokio::task::yield_now().await;
    let package_ref =
        publish_counter_package(gas_objects.pop().unwrap(), configs.validator_set()).await;

    // Send a transaction to create a counter.
    tokio::task::yield_now().await;
    let create_counter_transaction = move_transaction(
        gas_objects.pop().unwrap(),
        "counter",
        "create",
        package_ref,
        /* arguments */ Vec::default(),
    );
    let resp = gateway
        .execute_transaction(create_counter_transaction)
        .await
        .unwrap();
    let effects = resp.to_effect_response().unwrap().effects;
    let shared_object_id = effects.created[0].reference.object_id;
    // We need to have one gas object left for the final value check.
    let last_gas_object = gas_objects.pop().unwrap();
    let increment_amount = gas_objects.len();

    // It may happen that no authorities manage to get their transaction sequenced by consensus
    // (we may be unlucky and consensus may drop all our transactions). It would have been nice
    // to only filter "timeout" errors, but the game way simply returns `anyhow::Error`, this
    // will be fixed by issue #1717. Note that the gateway has an internal retry mechanism but
    // it is not an infinite loop.
    loop {
        let futures: Vec<_> = gas_objects
            .iter()
            .cloned()
            .map(|gas_object| {
                let g = gateway.clone();
                let increment_counter_transaction = move_transaction(
                    gas_object,
                    "counter",
                    "increment",
                    package_ref,
                    /* arguments */
                    vec![CallArg::Object(ObjectArg::SharedObject(shared_object_id))],
                );
                async move { g.execute_transaction(increment_counter_transaction).await }
            })
            .collect();

        let replies: Vec<_> = futures::future::join_all(futures)
            .await
            .into_iter()
            .collect();
        assert_eq!(replies.len(), increment_amount);
        if replies.iter().all(|result| result.is_ok()) {
            break;
        }
    }

    let assert_value_transaction = move_transaction(
        last_gas_object,
        "counter",
        "assert_value",
        package_ref,
        vec![
            CallArg::Object(ObjectArg::SharedObject(shared_object_id)),
            CallArg::Pure((increment_amount as u64).to_le_bytes().to_vec()),
        ],
    );

    // Same problem may happen here (consensus may drop transactions).
    loop {
        let result = gateway
            .clone()
            .execute_transaction(assert_value_transaction.clone())
            .await;
        if let Ok(response) = result {
            let effects = response.to_effect_response().unwrap().effects;
            assert!(effects.status.is_ok());
            break;
        }
    }
}
