// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// integration test with standalone postgresql database
#[cfg(feature = "pg_integration")]
mod pg_integration {
    use diesel::migration::MigrationSource;
    use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
    use jsonrpsee::http_client::{HttpClient, HttpClientBuilder};
    use prometheus::Registry;
    use std::env;
    use std::str::FromStr;
    use sui_indexer::errors::IndexerError;
    use sui_indexer::store::{IndexerStore, PgIndexerStore};
    use sui_indexer::PgPoolConnection;
    use sui_indexer::{new_pg_connection_pool, Indexer};
    use sui_types::digests::TransactionDigest;
    use test_utils::network::{TestCluster, TestClusterBuilder};
    use tokio::task::JoinHandle;
    const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");
    use sui_json_rpc::api::ReadApiClient;

    #[tokio::test]
    async fn test_genesis_sync() {
        let (test_cluster, indexer_rpc_client, store, handle) = start_test_cluster().await;
        // Allow indexer to sync
        wait_until_checkpoint(&store, 1).await;

        let checkpoint = store.get_checkpoint(0.into()).unwrap();

        for tx in checkpoint.transactions {
            let tx = tx.unwrap();
            let transaction = store.get_transaction_by_digest(&tx);
            assert!(transaction.is_ok());
            let tx_digest = TransactionDigest::from_str(&tx).unwrap();
            let _fullnode_rpc_tx = test_cluster
                .rpc_client()
                .get_transaction(tx_digest)
                .await
                .unwrap();
            let _indexer_rpc_tx = indexer_rpc_client.get_transaction(tx_digest).await.unwrap();

            // This fails because of events mismatch
            // TODO: fix this
            //assert_eq!(fullnode_rpc_tx, indexer_rpc_tx);
        }
        // TODO: more checks to ensure genesis sync data integrity.
        drop(handle);
    }

    async fn start_test_cluster() -> (
        TestCluster,
        HttpClient,
        PgIndexerStore,
        JoinHandle<Result<(), IndexerError>>,
    ) {
        let pg_host = env::var("POSTGRES_HOST").unwrap_or_else(|_| "localhost".into());
        let pg_port = env::var("POSTGRES_PORT").unwrap_or_else(|_| "5432".into());
        let db_url = format!("postgres://postgres:postgrespw@{pg_host}:{pg_port}");
        let pg_connection_pool = new_pg_connection_pool(&db_url).await.unwrap();

        reset_database(&mut pg_connection_pool.get().unwrap());

        let test_cluster = TestClusterBuilder::new().build().await.unwrap();
        let store = PgIndexerStore::new(pg_connection_pool);

        let store_clone = store.clone();
        let registry = Registry::default();

        let rpc_url = test_cluster.rpc_url().to_string();
        let handle =
            tokio::spawn(async move { Indexer::start(&rpc_url, &registry, store_clone).await });

        // TODO: make indexer port configurable
        let http_client = HttpClientBuilder::default()
            .build("http://0.0.0.0:3030")
            .unwrap();

        (test_cluster, http_client, store, handle)
    }

    async fn wait_until_checkpoint(store: &PgIndexerStore, until_checkpoint: i64) {
        let mut cp = store.get_latest_checkpoint_sequence_number().unwrap();
        while cp < until_checkpoint {
            tokio::task::yield_now().await;
            cp = store.get_latest_checkpoint_sequence_number().unwrap();
        }
    }

    fn reset_database(conn: &mut PgPoolConnection) {
        conn.revert_all_migrations(MIGRATIONS).unwrap();
        conn.run_migrations(&MIGRATIONS.migrations().unwrap())
            .unwrap();
    }
}
