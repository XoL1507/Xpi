// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
#![recursion_limit = "256"]

use std::env;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::Result;
use backoff::future::retry;
use backoff::ExponentialBackoff;
use clap::Parser;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::r2d2::ConnectionManager;
use diesel_async::pooled_connection::deadpool::{Object, Pool};
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::AsyncPgConnection;
use futures::future::BoxFuture;
use futures::FutureExt;
use jsonrpsee::http_client::{HeaderMap, HeaderValue, HttpClient, HttpClientBuilder};
use prometheus::Registry;
use rustls::client::{ServerCertVerified, ServerCertVerifier};
use rustls::{Certificate, Error, ServerName};
use tracing::{info, warn};
use url::Url;

use apis::{
    CoinReadApi, ExtendedApi, GovernanceReadApi, IndexerApi, ReadApi, TransactionBuilderApi,
    WriteApi,
};
use errors::IndexerError;
use handlers::checkpoint_handler::CheckpointHandler;
use mysten_metrics::spawn_monitored_task;
use store::IndexerStore;
use sui_core::event_handler::EventHandler;
use sui_json_rpc::{JsonRpcServerBuilder, ServerHandle, CLIENT_SDK_TYPE_HEADER};
use sui_sdk::{SuiClient, SuiClientBuilder};

use crate::apis::MoveUtilsApi;

pub mod apis;
pub mod errors;
mod handlers;
pub mod metrics;
pub mod models;
pub mod processors;
pub mod schema;
pub mod store;
pub mod test_utils;
pub mod types;
pub mod utils;

pub type PgConnectionPool = diesel::r2d2::Pool<ConnectionManager<PgConnection>>;
pub type PgPoolConnection = diesel::r2d2::PooledConnection<ConnectionManager<PgConnection>>;

pub type AsyncPgConnectionPool = Pool<AsyncPgConnection>;

/// Returns all endpoints for which we have implemented on the indexer,
/// some of them are not validated yet.
/// NOTE: we only use this for integration testing
const IMPLEMENTED_METHODS: [&str; 9] = [
    // read apis
    "get_checkpoint",
    "get_latest_checkpoint_sequence_number",
    "get_object",
    "get_owned_objects",
    "get_total_transaction_blocks",
    "get_transaction_block",
    "multi_get_transaction_blocks",
    // indexer apis
    "query_events",
    "query_transaction_blocks",
];

#[derive(Parser, Clone, Debug)]
#[clap(
    name = "Sui indexer",
    about = "An off-fullnode service serving data from Sui protocol",
    rename_all = "kebab-case"
)]
pub struct IndexerConfig {
    #[clap(long)]
    pub db_url: String,
    #[clap(long)]
    pub rpc_client_url: String,
    #[clap(long, default_value = "0.0.0.0", global = true)]
    pub client_metric_host: String,
    #[clap(long, default_value = "9184", global = true)]
    pub client_metric_port: u16,
    #[clap(long, default_value = "0.0.0.0", global = true)]
    pub rpc_server_url: String,
    #[clap(long, default_value = "9000", global = true)]
    pub rpc_server_port: u16,
    #[clap(long, multiple_occurrences = false, multiple_values = true)]
    pub migrated_methods: Vec<String>,
    #[clap(long)]
    pub reset_db: bool,
    // NOTE: experimental only, do not use in production.
    #[clap(long)]
    pub skip_db_commit: bool,
}

impl IndexerConfig {
    /// returns connection url without the db name
    pub fn base_connection_url(&self) -> String {
        let url = Url::parse(&self.db_url).expect("Failed to parse URL");
        format!(
            "{}://{}:{}@{}:{}/",
            url.scheme(),
            url.username(),
            url.password().unwrap_or_default(),
            url.host_str().unwrap_or_default(),
            url.port().unwrap_or_default()
        )
    }

    pub fn all_implemented_methods() -> Vec<String> {
        IMPLEMENTED_METHODS.iter().map(|&s| s.to_string()).collect()
    }
}

impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            db_url: "postgres://postgres:postgres@localhost:5432/sui_indexer".to_string(),
            rpc_client_url: "http://127.0.0.1:9000".to_string(),
            client_metric_host: "0.0.0.0".to_string(),
            client_metric_port: 9184,
            rpc_server_url: "0.0.0.0".to_string(),
            rpc_server_port: 9000,
            migrated_methods: vec![],
            reset_db: false,
            skip_db_commit: false,
        }
    }
}

pub struct Indexer;

impl Indexer {
    pub async fn start<S: IndexerStore + Sync + Send + Clone + 'static>(
        config: &IndexerConfig,
        registry: &Registry,
        store: S,
    ) -> Result<(), IndexerError> {
        let event_handler = Arc::new(EventHandler::default());
        let handle = build_json_rpc_server(registry, store.clone(), event_handler.clone(), config)
            .await
            .expect("Json rpc server should not run into errors upon start.");
        // let JSON RPC server run forever.
        spawn_monitored_task!(handle.stopped());
        info!(
            "Sui indexer of version {:?} started...",
            env!("CARGO_PKG_VERSION")
        );

        backoff::future::retry(ExponentialBackoff::default(), || async {
            let event_handler_clone = event_handler.clone();
            let http_client = get_http_client(config.rpc_client_url.as_str())?;
            let cp = CheckpointHandler::new(
                store.clone(),
                http_client,
                event_handler_clone,
                registry,
                config,
            );
            cp.spawn()
                .await
                .expect("Indexer main should not run into errors.");
            Ok(())
        })
        .await
    }
}

// TODO(gegaowp): this is only used in validation now, will remove in a separate PR
// together with the validation codes.
pub async fn new_rpc_client(http_url: &str) -> Result<SuiClient, IndexerError> {
    info!("Getting new RPC client...");
    SuiClientBuilder::default()
        .build(http_url)
        .await
        .map_err(|e| {
            warn!("Failed to get new RPC client with error: {:?}", e);
            IndexerError::HttpClientInitError(format!(
                "Failed to initialize fullnode RPC client with error: {:?}",
                e
            ))
        })
}

fn get_http_client(rpc_client_url: &str) -> Result<HttpClient, IndexerError> {
    let mut headers = HeaderMap::new();
    headers.insert(CLIENT_SDK_TYPE_HEADER, HeaderValue::from_static("indexer"));

    HttpClientBuilder::default()
        .max_request_body_size(2 << 30)
        .max_concurrent_requests(usize::MAX)
        .set_headers(headers.clone())
        .build(rpc_client_url)
        .map_err(|e| {
            warn!("Failed to get new Http client with error: {:?}", e);
            IndexerError::HttpClientInitError(format!(
                "Failed to initialize fullnode RPC client with error: {:?}",
                e
            ))
        })
}

fn establish_connection(url: &str) -> BoxFuture<ConnectionResult<AsyncPgConnection>> {
    async {
        let mut config = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_no_client_auth();

        // TODO: we might want to set a proper SSL cert for DB and indexer
        struct AcceptAllVerifier;
        impl ServerCertVerifier for AcceptAllVerifier {
            fn verify_server_cert(
                &self,
                _end_entity: &Certificate,
                _intermediates: &[Certificate],
                _server_name: &ServerName,
                _scts: &mut dyn Iterator<Item = &[u8]>,
                _ocsp_response: &[u8],
                _now: SystemTime,
            ) -> std::result::Result<ServerCertVerified, Error> {
                Ok(ServerCertVerified::assertion())
            }
        }
        config
            .dangerous()
            .set_certificate_verifier(Arc::new(AcceptAllVerifier));

        let connector = tokio_postgres_rustls::MakeRustlsConnect::new(config);
        let (client, connection) = tokio_postgres::connect(url, connector)
            .await
            .map_err(|e| ConnectionError::BadConnection(e.to_string()))?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("connection error: {}", e);
            }
        });
        AsyncPgConnection::try_from(client).await
    }
    .boxed()
}

pub async fn new_pg_connection_pool(
    db_url: &str,
) -> Result<(PgConnectionPool, AsyncPgConnectionPool), IndexerError> {
    let manager = ConnectionManager::<PgConnection>::new(db_url);
    // default connection pool max size is 10
    let blocking_cp = diesel::r2d2::Pool::builder().build(manager).map_err(|e| {
        IndexerError::PgConnectionPoolInitError(format!(
            "Failed to initialize connection pool with error: {:?}",
            e
        ))
    })?;

    let manager = AsyncDieselConnectionManager::<AsyncPgConnection>::new_with_setup(
        db_url,
        establish_connection,
    );
    // Our vultr instances allow up to 197 concurrent connections,
    // setting the default pool size to 187 for async connections and 10 for blocking connections.
    let connection_size = env::var("DB_CONNECTION_SIZE")
        .unwrap_or_else(|_| "187".to_string())
        .parse::<usize>()
        .unwrap_or(187);
    info!("Creating connection pool with size: {connection_size}");
    let async_pool = Pool::builder(manager)
        .max_size(connection_size)
        .build()
        .map_err(|e| {
            IndexerError::PgConnectionPoolInitError(format!(
                "Failed to initialize async connection pool with error: {:?}",
                e
            ))
        })?;
    Ok((blocking_cp, async_pool))
}

pub fn get_pg_pool_connection(pool: &PgConnectionPool) -> Result<PgPoolConnection, IndexerError> {
    backoff::retry(ExponentialBackoff::default(), || {
        let pool_conn = pool.get()?;
        Ok(pool_conn)
    })
    .map_err(|e| {
        IndexerError::PgPoolConnectionError(format!(
            "Failed to get connection from PG connection pool with error: {:?}",
            e
        ))
    })
}

pub async fn get_async_pg_pool_connection(
    pool: &AsyncPgConnectionPool,
) -> Result<Object<AsyncPgConnection>, IndexerError> {
    retry(ExponentialBackoff::default(), || async {
        pool.get().await.map_err(backoff::Error::Permanent)
    })
    .await
    .map_err(|e| {
        IndexerError::PgPoolConnectionError(format!(
            "Failed to get async connection from PG connection pool with error: {:?}",
            e
        ))
    })
}

pub async fn build_json_rpc_server<S: IndexerStore + Sync + Send + 'static + Clone>(
    prometheus_registry: &Registry,
    state: S,
    event_handler: Arc<EventHandler>,
    config: &IndexerConfig,
) -> Result<ServerHandle, IndexerError> {
    let mut builder = JsonRpcServerBuilder::new(env!("CARGO_PKG_VERSION"), prometheus_registry);
    let http_client = get_http_client(config.rpc_client_url.as_str())?;

    builder.register_module(ReadApi::new(
        state.clone(),
        http_client.clone(),
        config.migrated_methods.clone(),
    ))?;
    builder.register_module(CoinReadApi::new(http_client.clone()))?;
    builder.register_module(TransactionBuilderApi::new(http_client.clone()))?;
    builder.register_module(GovernanceReadApi::new(http_client.clone()))?;
    builder.register_module(IndexerApi::new(
        state.clone(),
        http_client.clone(),
        event_handler,
        config.migrated_methods.clone(),
    ))?;
    builder.register_module(WriteApi::new(state.clone(), http_client.clone()))?;
    builder.register_module(ExtendedApi::new(state.clone()))?;
    builder.register_module(MoveUtilsApi::new(http_client))?;
    let default_socket_addr = SocketAddr::new(
        // unwrap() here is safe b/c the address is a static config.
        IpAddr::V4(Ipv4Addr::from_str(config.rpc_server_url.as_str()).unwrap()),
        config.rpc_server_port,
    );
    Ok(builder.start(default_socket_addr).await?)
}
