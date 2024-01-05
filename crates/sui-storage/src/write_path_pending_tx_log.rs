// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! WritePathPendingTransactionLog is used in the transaction write path (e.g. in
//! TranasctionOrchestrator) for transaction submission processing. It helps to achieve:
//! 1. At one time, a transaction is only processed once.
//! 2. When Fullnode crashes and restarts, the pending transaction will be loaded and retried.

use crate::mutex_table::MutexTable;
use crate::write_ahead_log::{MUTEX_TABLE_SHARD_SIZE, MUTEX_TABLE_SIZE};
use std::path::PathBuf;
use sui_types::base_types::TransactionDigest;
use sui_types::error::{SuiError, SuiResult};
use sui_types::messages::{TrustedTransction, VerifiedTransaction};
use typed_store::traits::TypedStoreDebug;
use typed_store::{rocks::DBMap, traits::Map};
use typed_store_derive::DBMapUtils;

pub type IsFirstRecord = bool;

#[derive(DBMapUtils)]
struct WritePathPendingTransactionTable {
    logs: DBMap<TransactionDigest, TrustedTransction>,
}

pub struct WritePathPendingTransactionLog {
    pending_transactions: WritePathPendingTransactionTable,
    mutex_table: MutexTable<TransactionDigest>,
}

impl WritePathPendingTransactionLog {
    pub fn new(path: PathBuf) -> Self {
        let pending_transactions =
            WritePathPendingTransactionTable::open_tables_read_write(path, None, None);
        Self {
            pending_transactions,
            mutex_table: MutexTable::new(MUTEX_TABLE_SIZE, MUTEX_TABLE_SHARD_SIZE),
        }
    }

    // Returns whether the table currently has this transaction in record.
    // If not, write the transaction and return true; otherwise return false.
    // Because the record will be cleanded up when the transaction finishes,
    // even when it returns true, the callsite of this function should check
    // the transaction status before doing anything, to avoid duplicates.
    pub async fn write_pending_transaction_maybe(
        &self,
        tx: &VerifiedTransaction,
    ) -> SuiResult<IsFirstRecord> {
        let tx_digest = tx.digest();
        let _guard = self.mutex_table.acquire_lock(*tx_digest).await;
        if self.pending_transactions.logs.contains_key(tx_digest)? {
            Ok(false)
        } else {
            self.pending_transactions
                .logs
                .insert(tx_digest, tx.serializable_ref())?;
            Ok(true)
        }
    }

    // This function does not need to be behind a lock because:
    // 1. there is supposed to be only one callsite; Even when there are
    //    several, the deletion is idempotent.
    // 2. it does not race with the insert (`write_pending_transaction_maybe`)
    //    in a way that we care.
    //    2.a. for one transaction, `finish_transaction` shouldn't predate
    //        `write_pending_transaction_maybe`.
    //    2.b  for concurrent requests of one transaction, a call to this
    //        function may happen in bewteen hence making the second request
    //        thinks it is the first record. It's preventable by checking this
    //        transaction again after the call of `write_pending_transaction_maybe`.
    pub fn finish_transaction(&self, tx: &TransactionDigest) -> SuiResult {
        let write_batch = self.pending_transactions.logs.batch();
        let write_batch =
            write_batch.delete_batch(&self.pending_transactions.logs, std::iter::once(tx))?;
        write_batch.write().map_err(SuiError::from)
    }

    pub fn load_all_pending_transactions(&self) -> Vec<VerifiedTransaction> {
        self.pending_transactions
            .logs
            .iter()
            .map(|(_tx_digest, tx)| VerifiedTransaction::from(tx))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow;
    use std::collections::HashSet;
    use sui_types::utils::create_fake_transaction;

    #[tokio::test]
    async fn test_pending_tx_log_basic() -> anyhow::Result<()> {
        let temp_dir = tempfile::tempdir().unwrap();
        let pending_txes = WritePathPendingTransactionLog::new(temp_dir.path().to_path_buf());
        let tx = create_fake_transaction();
        let tx_digest = *tx.digest();
        assert!(pending_txes
            .write_pending_transaction_maybe(&tx)
            .await
            .unwrap());
        // The second write will return false
        assert!(!pending_txes
            .write_pending_transaction_maybe(&tx)
            .await
            .unwrap());

        let loaded_txes = pending_txes.load_all_pending_transactions();
        assert_eq!(vec![tx], loaded_txes);

        pending_txes.finish_transaction(&tx_digest).unwrap();
        let loaded_txes = pending_txes.load_all_pending_transactions();
        assert!(loaded_txes.is_empty());

        // It's ok to finish an already finished transaction
        pending_txes.finish_transaction(&tx_digest).unwrap();

        // Test writing and finishing more transactions
        let txes: Vec<_> = (0..10).map(|_| create_fake_transaction()).collect();
        for tx in txes.iter().take(10) {
            assert!(pending_txes
                .write_pending_transaction_maybe(tx)
                .await
                .unwrap());
        }
        let loaded_tx_digests: HashSet<_> = pending_txes
            .load_all_pending_transactions()
            .iter()
            .map(|t| *t.digest())
            .collect();
        assert_eq!(
            txes.iter().map(|t| *t.digest()).collect::<HashSet<_>>(),
            loaded_tx_digests
        );

        for tx in txes.iter().take(5) {
            pending_txes.finish_transaction(tx.digest()).unwrap();
        }
        let loaded_tx_digests: HashSet<_> = pending_txes
            .load_all_pending_transactions()
            .iter()
            .map(|t| *t.digest())
            .collect();
        assert_eq!(
            txes.iter()
                .skip(5)
                .map(|t| *t.digest())
                .collect::<HashSet<_>>(),
            loaded_tx_digests
        );

        Ok(())
    }
}
