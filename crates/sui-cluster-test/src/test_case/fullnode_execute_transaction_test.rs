// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::{TestCaseImpl, TestContext};
use async_trait::async_trait;
use sui_json_rpc_types::SuiExecutionStatus;
use sui_sdk::SuiClient;
use sui_types::{base_types::TransactionDigest, messages::ExecuteTransactionRequestType};
use tracing::info;

pub struct FullNodeExecuteTransactionTest;

impl FullNodeExecuteTransactionTest {
    async fn verify_transaction(fullnode: &SuiClient, tx_digest: TransactionDigest) {
        fullnode
            .read_api()
            .get_transaction(tx_digest)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "Failed get transaction {:?} from fullnode: {:?}",
                    tx_digest, e
                )
            });
    }
}

#[async_trait]
impl TestCaseImpl for FullNodeExecuteTransactionTest {
    fn name(&self) -> &'static str {
        "FullNodeExecuteTransaction"
    }

    fn description(&self) -> &'static str {
        "Test executing transaction via Fullnode Quorum Driver"
    }

    async fn run(&self, ctx: &mut TestContext) -> Result<(), anyhow::Error> {
        let txn_count = 4;
        ctx.get_sui_from_faucet(Some(txn_count)).await;

        let mut txns = ctx.make_transactions(txn_count).await;
        assert!(
            txns.len() >= txn_count,
            "Expect at least {} txns, but only got {}. Do we generate enough gas objects during genesis?",
            txn_count,
            txns.len(),
        );

        let fullnode = ctx.get_fullnode_client();

        info!("Test execution with WaitForEffectsCert");
        let txn = txns.swap_remove(0);
        let txn_digest = *txn.digest();

        let response = fullnode
            .quorum_driver()
            .execute_transaction(txn, Some(ExecuteTransactionRequestType::WaitForEffectsCert))
            .await?;

        assert!(!response.confirmed_local_execution);
        assert_eq!(txn_digest, response.tx_digest);
        response.tx_cert.unwrap();
        let effects = response.effects.unwrap();
        if !matches!(effects.status, SuiExecutionStatus::Success { .. }) {
            panic!(
                "Failed to execute transfer tranasction {:?}: {:?}",
                txn_digest, effects.status
            )
        }
        // Verify fullnode observes the txn
        ctx.let_fullnode_sync(vec![txn_digest], 5).await;
        Self::verify_transaction(fullnode, txn_digest).await;

        info!("Test execution with WaitForLocalExecution");
        let txn = txns.swap_remove(0);
        let txn_digest = *txn.digest();

        let response = fullnode
            .quorum_driver()
            .execute_transaction(
                txn,
                Some(ExecuteTransactionRequestType::WaitForLocalExecution),
            )
            .await?;
        response.tx_cert.unwrap();
        assert!(response.confirmed_local_execution);
        assert_eq!(txn_digest, response.tx_digest);
        let effects = response.effects.unwrap();
        if !matches!(effects.status, SuiExecutionStatus::Success { .. }) {
            panic!(
                "Failed to execute transfer tranasction {:?}: {:?}",
                txn_digest, effects.status
            )
        }
        // Unlike in other execution modes, there's no need to wait for the node to sync
        Self::verify_transaction(fullnode, txn_digest).await;

        Ok(())
    }
}
