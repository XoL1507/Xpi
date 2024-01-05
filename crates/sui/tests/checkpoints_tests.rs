// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use futures::stream::StreamExt;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use sui_config::NetworkConfig;
use sui_core::{
    authority::AuthorityState, authority_aggregator::AuthorityAggregator,
    authority_client::NetworkAuthorityClient,
};
use sui_macros::sim_test;
use sui_node::SuiNodeHandle;
use sui_sdk::crypto::{InMemKeystore, Keystore};
use sui_types::{
    base_types::{ExecutionDigests, TransactionDigest},
    messages::{
        CallArg, CheckpointStreamRequest, CheckpointStreamResponseItem, ExecutionStatus, ObjectArg,
        Transaction,
    },
    messages_checkpoint::AuthenticatedCheckpoint,
};
use test_utils::transaction::{publish_counter_package, submit_shared_object_transaction};
use test_utils::{
    authority::{
        spawn_checkpoint_processes, spawn_test_authorities, test_authority_aggregator,
        test_authority_configs,
    },
    messages::{make_transactions_with_pre_genesis_objects, move_transaction},
    objects::test_gas_objects,
};
use tokio::time::{sleep, Duration};
use typed_store::traits::Map;

/// Helper function determining whether the checkpoint store of an authority contains the input
/// transactions' digests.
fn transactions_in_checkpoint(authority: &AuthorityState) -> HashSet<TransactionDigest> {
    let checkpoints_store = authority.checkpoints();

    // Get all transactions in the first 10 checkpoints.
    (0..10)
        .filter_map(|checkpoint_sequence| {
            checkpoints_store
                .lock()
                .tables
                .checkpoint_contents
                .get(&checkpoint_sequence)
                .unwrap()
        })
        .flat_map(|x| x.iter().map(|tx| tx.transaction).collect::<HashSet<_>>())
        .collect::<HashSet<_>>()
}

async fn execute_transactions(
    aggregator: &AuthorityAggregator<NetworkAuthorityClient>,
    transactions: &[Transaction],
) {
    for transaction in transactions {
        let (_, effects) = aggregator
            .clone()
            .execute_transaction(transaction)
            .await
            .unwrap();

        // If this check fails the transactions will not be included in the checkpoint.
        assert!(matches!(
            effects.effects.status,
            ExecutionStatus::Success { .. }
        ));

        // Add some delay between transactions
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

async fn wait_for_advance_to_next_checkpoint(
    handles: &[SuiNodeHandle],
    transaction_digests: &HashSet<TransactionDigest>,
) {
    // Wait for the transactions to be executed and end up in a checkpoint.
    let mut cnt = 0;
    loop {
        // Ensure all submitted transactions are in the checkpoint.
        let ok = handles
            .iter()
            .map(|handle| handle.with(|authority| transactions_in_checkpoint(&authority.state())))
            .all(|digests| digests.is_superset(transaction_digests));

        match ok {
            true => break,
            false => sleep(Duration::from_secs(1)).await,
        }
        cnt += 1;
        assert!(cnt <= 60);
    }

    // Ensure all authorities moved to the next checkpoint sequence number.
    let ok = handles
        .iter()
        .map(|handle| {
            handle.with(|authority| {
                authority
                    .state()
                    .checkpoints()
                    .lock()
                    .get_locals()
                    .next_checkpoint
            })
        })
        .all(|sequence| sequence >= 1);
    assert!(ok);
}

fn make_aggregator(
    configs: &NetworkConfig,
    handles: &[SuiNodeHandle],
) -> AuthorityAggregator<NetworkAuthorityClient> {
    let committee_store = handles[0].with(|h| h.state().committee_store().clone());
    test_authority_aggregator(configs, committee_store)
}

#[sim_test]
async fn sequence_fragments() {
    // Spawn a quorum of authorities.
    let configs = test_authority_configs();
    let mut handles = spawn_test_authorities(vec![], &configs).await;
    let committee = handles[0].with(|h| h.state().clone_committee());

    // Get checkpoint proposals.
    let t1 = ExecutionDigests::random();
    let t2 = ExecutionDigests::random();
    let t3 = ExecutionDigests::random();
    let transactions = [(1, t1), (2, t2), (3, t3)];
    let next_sequence_number = (transactions.len() + 1) as u64;

    let mut proposals: Vec<_> = handles
        .iter_mut()
        .map(|handle| {
            handle.with(|node| {
                let checkpoints_store = node.state().checkpoints();
                checkpoints_store
                    .lock()
                    .handle_internal_batch(next_sequence_number, &transactions)
                    .unwrap();
                let proposal = checkpoints_store
                    .lock()
                    .set_proposal(committee.epoch)
                    .unwrap();
                proposal
            })
        })
        .collect();

    // Ensure the are no fragments in the checkpoint store at this time.
    for handle in &handles {
        let status = handle.with(|node| {
            node.state()
                .checkpoints()
                .lock()
                .tables
                .fragments
                .iter()
                .skip_to_last()
                .next()
        });
        assert!(status.is_none());
    }

    // Make a checkpoint fragment and sequence it.
    let p1 = proposals.pop().unwrap();
    let p2 = proposals.pop().unwrap();
    let fragment = p1.fragment_with(&p2);

    for handle in handles.iter_mut() {
        let _response = handle.with(|node| {
            node.state()
                .checkpoints()
                .lock()
                .submit_local_fragment_to_consensus(&fragment, &committee)
        });
    }

    // Wait until all validators sequence and process the fragment.
    loop {
        let ok = handles.iter().all(|handle| {
            handle.with(|node| {
                node.state()
                    .checkpoints()
                    .lock()
                    .tables
                    .fragments
                    .iter()
                    .next()
                    .is_some()
            })
        });
        if ok {
            break;
        }
        sleep(Duration::from_millis(10)).await;
    }
}

#[sim_test]
async fn end_to_end() {
    telemetry_subscribers::init_for_testing();
    // Make a few test transactions.
    let total_transactions = 3;
    let keys = Keystore::from(InMemKeystore::new(total_transactions));

    let (transactions, input_objects) = make_transactions_with_pre_genesis_objects(keys);
    let transaction_digests: HashSet<_> = transactions.iter().map(|x| *x.digest()).collect();

    // Spawn a quorum of authorities.
    let configs = test_authority_configs();
    let handles = spawn_test_authorities(input_objects, &configs).await;

    // Make an authority's aggregator.
    let aggregator = make_aggregator(&configs, &handles);

    // Follow checkpoint notifications
    let client = aggregator.clone_client(aggregator.committee.sample());
    let checkpoint_stream = Arc::new(Mutex::new(Vec::new()));
    let checkpoint_stream_inner = checkpoint_stream.clone();

    tokio::task::spawn(async move {
        let mut stream = client
            .handle_checkpoint_stream(CheckpointStreamRequest::new())
            .await
            .unwrap();

        while let Some(next) = stream.next().await {
            match next.unwrap() {
                CheckpointStreamResponseItem {
                    first_available_sequence,
                    checkpoint: AuthenticatedCheckpoint::Certified(c),
                } => {
                    assert_eq!(first_available_sequence, 0);
                    checkpoint_stream_inner.lock().unwrap().push(c);
                }
                _ => {
                    panic!("only certified checkpoints should be streamed for now")
                }
            }
        }
    });

    spawn_checkpoint_processes(&configs, &handles).await;

    execute_transactions(&aggregator, &transactions).await;

    wait_for_advance_to_next_checkpoint(&handles, &transaction_digests).await;

    tokio::time::sleep(Duration::from_secs(3)).await;

    let checkpoint_stream = checkpoint_stream.lock().unwrap();
    assert!(checkpoint_stream.len() >= 2);
    assert_eq!(checkpoint_stream[0].summary.sequence_number, 0);
    assert_eq!(checkpoint_stream[1].summary.sequence_number, 1);
}

#[sim_test]
async fn end_to_end_with_one_byzantine() {
    telemetry_subscribers::init_for_testing();
    // Make a few test transactions.
    let total_transactions = 3;
    let keystore = Keystore::from(InMemKeystore::new(total_transactions));
    let (transactions, input_objects) = make_transactions_with_pre_genesis_objects(keystore);
    let transaction_digests: HashSet<_> = transactions.iter().map(|x| *x.digest()).collect();

    // Spawn a quorum of authorities.
    let configs = test_authority_configs();
    let handles = spawn_test_authorities(input_objects, &configs).await;
    let (_first, rest) = handles[..].split_at(1);

    // Make an authority's aggregator.
    let aggregator = make_aggregator(&configs, &handles);

    // one authority does not participate in checkpointing
    spawn_checkpoint_processes(&configs, rest).await;

    execute_transactions(&aggregator, &transactions).await;

    // the honest majority still makes progress
    wait_for_advance_to_next_checkpoint(rest, &transaction_digests).await;
}

#[sim_test]
async fn checkpoint_with_shared_objects() {
    telemetry_subscribers::init_for_testing();

    // Get some gas objects to submit shared-objects transactions.
    let mut gas_objects = test_gas_objects();

    // Make a few test transactions.
    let total_transactions = 3;
    let keystore = Keystore::from(InMemKeystore::new(total_transactions));
    let (transactions, input_objects) = make_transactions_with_pre_genesis_objects(keystore);

    // Spawn a quorum of authorities.
    let configs = test_authority_configs();
    let initialization_objects = input_objects.into_iter().chain(gas_objects.iter().cloned());
    let handles = spawn_test_authorities(initialization_objects, &configs).await;

    // Make an authority's aggregator.
    let aggregator = make_aggregator(&configs, &handles);

    spawn_checkpoint_processes(&configs, &handles).await;

    // Publish the move package to all authorities and get the new package ref.
    let gas = gas_objects.pop().unwrap();
    let package_ref = publish_counter_package(gas, configs.validator_set()).await;

    // Make a transaction to create a counter.
    let create_counter_transaction = move_transaction(
        gas_objects.pop().unwrap(),
        "counter",
        "create",
        package_ref,
        /* arguments */ Vec::default(),
    );

    let (_, effects) = aggregator
        .execute_transaction(&create_counter_transaction)
        .await
        .unwrap();
    assert!(matches!(
        effects.effects.status,
        ExecutionStatus::Success { .. }
    ));
    let ((counter_id, _, _), _) = effects.effects.created[0];

    // We can finally make a valid shared-object transaction (incrementing the counter).
    let increment_counter_transaction = move_transaction(
        gas_objects.pop().unwrap(),
        "counter",
        "increment",
        package_ref,
        vec![CallArg::Object(ObjectArg::SharedObject(counter_id))],
    );
    let effects = submit_shared_object_transaction(
        increment_counter_transaction.clone(),
        configs.validator_set(),
    )
    .await
    .unwrap();
    assert!(effects.status.is_ok());

    // Now send a few single-writer transactions.
    execute_transactions(&aggregator, &transactions).await;

    // Record the transactions digests we expect to see in the checkpoint. Note that there is also
    // an extra transaction to register the move module that we don't consider here.
    let mut transaction_digests: HashSet<_> = transactions.iter().map(|x| *x.digest()).collect();
    transaction_digests.insert(*create_counter_transaction.digest());
    transaction_digests.insert(*increment_counter_transaction.digest());

    wait_for_advance_to_next_checkpoint(&handles, &transaction_digests).await;
}
