// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::config::{ConnectionConfig, ServiceConfig};
use crate::context_data::data_provider::DataProvider;
use crate::context_data::db_data_provider::PgManager;
use crate::context_data::sui_sdk_data_provider::{lru_cache_data_loader, sui_sdk_client_v0};
use crate::extensions::feature_gate::FeatureGate;
use crate::extensions::limits_info::LimitsInfo;
use crate::extensions::logger::Logger;
use crate::extensions::timeout::Timeout;
use crate::server::builder::ServerBuilder;

use std::default::Default;

pub async fn start_example_server(conn: ConnectionConfig, service_config: ServiceConfig) {
    let _guard = telemetry_subscribers::TelemetryConfig::new()
        .with_env()
        .init();

    let sui_sdk_client_v0 = sui_sdk_client_v0(&conn.rpc_url).await;
    let data_provider: Box<dyn DataProvider> = Box::new(sui_sdk_client_v0.clone());
    let data_loader = lru_cache_data_loader(&sui_sdk_client_v0).await;

    let pg_conn_pool = PgManager::new();

    let builder = ServerBuilder::new(conn.port, conn.host);
    println!("Launch GraphiQL IDE at: http://{}", builder.address());

    builder
        .max_query_depth(service_config.limits.max_query_depth)
        .max_query_nodes(service_config.limits.max_query_nodes)
        .context_data(data_provider)
        .context_data(data_loader)
        .context_data(service_config)
        .context_data(pg_conn_pool)
        .extension(FeatureGate)
        .extension(LimitsInfo)
        .extension(Logger::default())
        .extension(Timeout::default())
        .build()
        .run()
        .await;
}
