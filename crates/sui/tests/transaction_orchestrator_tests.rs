// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use prometheus::Registry;
use sui_core::authority_client::NetworkAuthorityClient;
use sui_core::transaction_orchestrator::TransactiondOrchestrator;
use sui_node::SuiNode;
use sui_types::base_types::TransactionDigest;
use sui_types::error::SuiResult;
use sui_types::messages::{
    ExecuteTransactionRequest, ExecuteTransactionRequestType, ExecuteTransactionResponse,
    QuorumDriverRequest, QuorumDriverRequestType, VerifiedTransaction,
};
use test_utils::messages::make_transactions_with_wallet_context;
use test_utils::network::TestClusterBuilder;
use test_utils::transaction::{wait_for_all_txes, wait_for_tx};

#[tokio::test]
async fn test_blocking_execution() -> Result<(), anyhow::Error> {
    let mut test_cluster = TestClusterBuilder::new().build().await?;
    let context = &mut test_cluster.wallet;
    let node = &test_cluster.fullnode_handle.sui_node;

    let active = node.active();

    // Disable node sync process
    active.cancel_node_sync_process_for_tests().await;

    let net = active.agg_aggregator();
    let temp_dir = tempfile::tempdir().unwrap();
    let orchestrator =
        TransactiondOrchestrator::new(net, node.state(), temp_dir.path(), &Registry::new());

    let txn_count = 4;
    let mut txns = make_transactions_with_wallet_context(context, txn_count).await;
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

    // Wait for data sync to catch up
    wait_for_tx(digest, node.state().clone()).await;

    // Transaction Orchestrator proactivcely executes txn locally
    let txn = txns.swap_remove(0);
    let digest = *txn.digest();

    let res = execute_with_orchestrator(
        &orchestrator,
        txn,
        ExecuteTransactionRequestType::WaitForLocalExecution,
    )
    .await
    .unwrap_or_else(|e| panic!("Failed to execute transaction {:?}: {:?}", digest, e));

    if let ExecuteTransactionResponse::EffectsCert(result) = res {
        let (_, _, executed_locally) = *result;
        assert!(executed_locally);
    };

    assert!(node.state().get_transaction(digest).await.is_ok());

    Ok(())
}

#[tokio::test]
async fn test_non_blocking_execution() -> Result<(), anyhow::Error> {
    let mut test_cluster = TestClusterBuilder::new().build().await?;
    let context = &mut test_cluster.wallet;
    let node = &test_cluster.fullnode_handle.sui_node;

    let active = node.active();

    // Disable node sync process
    active.cancel_node_sync_process_for_tests().await;

    let net = active.agg_aggregator();
    let temp_dir = tempfile::tempdir().unwrap();
    let orchestrator =
        TransactiondOrchestrator::new(net, node.state(), temp_dir.path(), &Registry::new());

    let txn_count = 4;
    let mut txns = make_transactions_with_wallet_context(context, txn_count).await;
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
    .await
    .unwrap_or_else(|e| panic!("Failed to execute transaction {:?}: {:?}", digest1, e));

    let txn = txns.swap_remove(0);
    let digest2 = *txn.digest();
    execute_with_orchestrator(
        &orchestrator,
        txn,
        ExecuteTransactionRequestType::WaitForTxCert,
    )
    .await
    .unwrap_or_else(|e| panic!("Failed to execute transaction {:?}: {:?}", digest2, e));

    let txn = txns.swap_remove(0);
    let digest3 = *txn.digest();
    execute_with_orchestrator(
        &orchestrator,
        txn,
        ExecuteTransactionRequestType::WaitForEffectsCert,
    )
    .await
    .unwrap_or_else(|e| panic!("Failed to execute transaction {:?}: {:?}", digest3, e));

    let digests = vec![digest1, digest2, digest3];
    wait_for_all_txes(digests.clone(), node.state().clone()).await;
    node_knows_txes(node, &digests).await;

    Ok(())
}

#[tokio::test]
async fn test_fullnode_wal_log() -> Result<(), anyhow::Error> {
    telemetry_subscribers::init_for_testing();
    let mut test_cluster = TestClusterBuilder::new().build().await?;

    let node = &test_cluster.fullnode_handle.sui_node;

    let active = node.active();
    let net = active.agg_aggregator();
    let temp_dir = tempfile::tempdir().unwrap();
    let orchestrator =
        TransactiondOrchestrator::new(net, node.state(), temp_dir.path(), &Registry::new());

    let txn_count = 2;
    let context = &mut test_cluster.wallet;
    let mut txns = make_transactions_with_wallet_context(context, txn_count).await;
    assert!(
        txns.len() >= txn_count,
        "Expect at least {} txns. Do we generate enough gas objects during genesis?",
        txn_count,
    );
    // As a comparison, we first verify a tx can go through
    let txn = txns.swap_remove(0);
    let digest = *txn.digest();
    execute_with_orchestrator(
        &orchestrator,
        txn,
        ExecuteTransactionRequestType::WaitForLocalExecution,
    )
    .await
    .unwrap_or_else(|e| panic!("Failed to execute transaction {:?}: {:?}", digest, e));

    let validator_addresses = test_cluster.get_validator_addresses();
    assert_eq!(validator_addresses.len(), 4);

    // Stop 2 validators and we lose quorum
    test_cluster.stop_validator(validator_addresses[0]);
    test_cluster.stop_validator(validator_addresses[1]);

    let txn = txns.swap_remove(0);
    // Expect tx to fail
    execute_with_orchestrator(
        &orchestrator,
        txn.clone(),
        ExecuteTransactionRequestType::WaitForLocalExecution,
    )
    .await
    .unwrap_err();

    // Because the tx did not go through, we expect to see it in the WAL log
    let pending_txes = orchestrator.load_all_pending_transactions();
    assert_eq!(pending_txes, vec![txn.clone()]);

    // Bring up 1 validator, we obtain quorum again and tx should succeed
    test_cluster.start_validator(validator_addresses[0]).await;
    execute_with_orchestrator(
        &orchestrator,
        txn,
        ExecuteTransactionRequestType::WaitForLocalExecution,
    )
    .await
    .unwrap();

    // TODO: wal erasing is done in the loop handling effects, so may have some delay.
    // However, once the refactoring is completed the wal removal will be done before
    // response is returned and we will not need the sleep.
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    // The tx should be erased in wal log.
    let pending_txes = orchestrator.load_all_pending_transactions();
    assert!(pending_txes.is_empty());

    Ok(())
}

async fn node_knows_txes(node: &SuiNode, digests: &Vec<TransactionDigest>) {
    for digest in digests {
        assert!(node.state().get_transaction(*digest).await.is_ok());
    }
}

async fn execute_with_orchestrator(
    orchestrator: &TransactiondOrchestrator<NetworkAuthorityClient>,
    txn: VerifiedTransaction,
    request_type: ExecuteTransactionRequestType,
) -> SuiResult<ExecuteTransactionResponse> {
    orchestrator
        .execute_transaction(ExecuteTransactionRequest {
            transaction: txn.into(),
            request_type,
        })
        .await
}
