// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use prometheus::Registry;
use sui::client_commands::WalletContext;
use sui_core::authority_client::NetworkAuthorityClient;
use sui_core::transaction_orchestrator::TransactiondOrchestrator;
use sui_node::SuiNode;
use sui_types::base_types::{ObjectID, ObjectRef, SuiAddress, TransactionDigest};
use sui_types::messages::{
    ExecuteTransactionRequest, ExecuteTransactionRequestType, ExecuteTransactionResponse,
    QuorumDriverRequest, QuorumDriverRequestType, Transaction,
};
use test_utils::messages::{
    make_counter_increment_transaction_with_wallet_context, make_transactions_with_wallet_context,
};
use test_utils::network::setup_network_and_wallet;
use test_utils::transaction::{
    increment_counter, publish_basics_package_and_make_counter, wait_for_all_txes, wait_for_tx,
};

#[tokio::test]
async fn test_blocking_execution() -> Result<(), anyhow::Error> {
    let (swarm, mut context, _) = setup_network_and_wallet().await?;

    let config = swarm.config().generate_fullnode_config();
    let node = SuiNode::start(&config, Registry::new()).await?;
    let active = node.active();

    // Disable node sync process
    active.cancel_node_sync_process_for_tests().await;

    let net = active.agg_aggregator();
    let node_sync_state = active.node_sync_state.clone();
    let orchestrator = TransactiondOrchestrator::new(net, node_sync_state, &Registry::new());

    let txn_count = 4;
    let mut txns = make_transactions_with_wallet_context(&mut context, txn_count).await;
    assert!(
        txns.len() >= txn_count,
        "Expect at least {} txns. Do we generate enough gas objects during genesis?",
        txn_count,
    );

    // Quorum driver does not execute txn locally
    let txn = txns.swap_remove(0);
    let digest = *txn.digest();
    orchestrator
        .quorum_driver()
        .execute_transaction(QuorumDriverRequest {
            transaction: txn,
            request_type: QuorumDriverRequestType::WaitForEffectsCert,
        })
        .await
        .unwrap_or_else(|e| panic!("Failed to execute transaction {:?}: {:?}", digest, e));
    // Since node sync is turned off, this node does not know about this txn
    assert!(node.state().get_transaction(digest).await.is_err());

    // Transaction Orchestrator proactivcely executes txn locally
    let txn = txns.swap_remove(0);
    let digest = *txn.digest();

    let res = execute_with_orchestrator(
        &orchestrator,
        txn,
        ExecuteTransactionRequestType::WaitForLocalExecution,
    )
    .await;

    if let ExecuteTransactionResponse::EffectsCert(result) = res {
        let (_, _, executed_locally) = *result;
        assert!(executed_locally);
    };

    // This node knows about this txn even though node sync is toggled off.
    assert!(node.state().get_transaction(digest).await.is_ok());

    Ok(())
}

#[tokio::test]
async fn test_non_blocking_execution() -> Result<(), anyhow::Error> {
    let (swarm, mut context, _) = setup_network_and_wallet().await?;

    let config = swarm.config().generate_fullnode_config();
    let node = SuiNode::start(&config, Registry::new()).await?;
    let active = node.active();

    // Disable node sync process
    active.cancel_node_sync_process_for_tests().await;

    let net = active.agg_aggregator();
    let node_sync_state = active.node_sync_state.clone();
    let orchestrator = TransactiondOrchestrator::new(net, node_sync_state, &Registry::new());

    let txn_count = 4;
    let mut txns = make_transactions_with_wallet_context(&mut context, txn_count).await;
    assert!(
        txns.len() >= txn_count,
        "Expect at least {} txns. Do we generate enough gas objects during genesis?",
        txn_count,
    );

    // Test ImmediateReturn and WaitForTxCert eventually are executed too
    let txn = txns.swap_remove(0);
    let digest1 = *txn.digest();

    execute_with_orchestrator(
        &orchestrator,
        txn,
        ExecuteTransactionRequestType::ImmediateReturn,
    )
    .await;

    let txn = txns.swap_remove(0);
    let digest2 = *txn.digest();
    execute_with_orchestrator(
        &orchestrator,
        txn,
        ExecuteTransactionRequestType::WaitForTxCert,
    )
    .await;

    let txn = txns.swap_remove(0);
    let digest3 = *txn.digest();
    execute_with_orchestrator(
        &orchestrator,
        txn,
        ExecuteTransactionRequestType::WaitForEffectsCert,
    )
    .await;

    let digests = vec![digest1, digest2, digest3];
    wait_for_all_txes(digests.clone(), node.state().clone()).await;
    node_knows_txes(&node, &digests).await;

    Ok(())
}

#[tokio::test]
async fn test_local_execution_with_missing_parents() -> Result<(), anyhow::Error> {
    let (swarm, context, _) = setup_network_and_wallet().await?;

    let config = swarm.config().generate_fullnode_config();
    let node = SuiNode::start(&config, Registry::new()).await?;
    let active = node.active();

    // Disable node sync process
    active.cancel_node_sync_process_for_tests().await;

    let net = active.agg_aggregator();
    let node_sync_state = active.node_sync_state.clone();
    let orchestrator = TransactiondOrchestrator::new(net, node_sync_state, &Registry::new());

    let signer = context.keystore.addresses().get(0).cloned().unwrap();
    let (pkg_ref, counter_id) = publish_basics_package_and_make_counter(&context, signer).await;

    // Construct a dependency graph:
    // tx0.1 -> ... -> tx0.19 -------> tx1 -> tx2 ----> tx3.0 -> ... tx3.19 -> tx3

    let digests0 = increment(&context, &signer, counter_id, 20, pkg_ref).await;

    let tx1 =
        make_counter_increment_transaction_with_wallet_context(&context, signer, counter_id, None)
            .await;
    let digest1 = *tx1.digest();
    orchestrator
        .quorum_driver()
        .execute_transaction(QuorumDriverRequest {
            transaction: tx1,
            request_type: QuorumDriverRequestType::WaitForTxCert,
        })
        .await
        .unwrap_or_else(|e| panic!("Failed to execute transaction {:?}: {:?}", digest1, e));

    // The node does not know about these txns
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    node_does_not_know_txes(&node, &digests0).await;

    // WaitForLocalExecution synchronuously executes all previous txns
    let tx2 =
        make_counter_increment_transaction_with_wallet_context(&context, signer, counter_id, None)
            .await;
    let digest2 = *tx2.digest();
    let res = execute_with_orchestrator(
        &orchestrator,
        tx2,
        ExecuteTransactionRequestType::WaitForLocalExecution,
    )
    .await;

    if let ExecuteTransactionResponse::EffectsCert(result) = res {
        let (_, _, executed_locally) = *result;
        assert!(executed_locally);
    };
    // Now the node knows about all past txns
    node_knows_txes(&node, &digests0).await;
    node_knows_txes(&node, &vec![digest2]).await;

    // Do another round of counter incrementing
    let digests3 = increment(&context, &signer, counter_id, 20, pkg_ref).await;

    node_does_not_know_txes(&node, &digests3).await;

    let tx4 =
        make_counter_increment_transaction_with_wallet_context(&context, signer, counter_id, None)
            .await;
    let digest4 = *tx4.digest();
    // ImmediateReturn asynchronuously executes all previous txns
    execute_with_orchestrator(
        &orchestrator,
        tx4,
        ExecuteTransactionRequestType::ImmediateReturn,
    )
    .await;

    // Wait for the async execution to finish
    wait_for_tx(digest4, node.state().clone()).await;
    node_knows_txes(&node, &digests3).await;

    Ok(())
}

async fn increment(
    context: &WalletContext,
    signer: &SuiAddress,
    counter_id: ObjectID,
    delta: usize,
    pkg_ref: ObjectRef,
) -> Vec<TransactionDigest> {
    let mut digests = Vec::with_capacity(delta);
    for _ in 0..delta {
        let digest = increment_counter(context, *signer, None, pkg_ref, counter_id)
            .await
            .certificate
            .transaction_digest;
        digests.push(digest);
    }
    digests
}

async fn node_knows_txes(node: &SuiNode, digests: &Vec<TransactionDigest>) {
    for digest in digests {
        assert!(node.state().get_transaction(*digest).await.is_ok());
    }
}

async fn node_does_not_know_txes(node: &SuiNode, digests: &Vec<TransactionDigest>) {
    for digest in digests {
        assert!(node.state().get_transaction(*digest).await.is_err());
    }
}

async fn execute_with_orchestrator(
    orchestrator: &TransactiondOrchestrator<NetworkAuthorityClient>,
    txn: Transaction,
    request_type: ExecuteTransactionRequestType,
) -> ExecuteTransactionResponse {
    let digest = *txn.digest();
    orchestrator
        .execute_transaction(ExecuteTransactionRequest {
            transaction: txn,
            request_type,
        })
        .await
        .unwrap_or_else(|e| panic!("Failed to execute transaction {:?}: {:?}", digest, e))
}
