// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use jsonrpsee_core::server::access_control::AccessControlBuilder;
use jsonrpsee_core::server::rpc_module::RpcModule;

use crate::http_server::{HttpServerBuilder, HttpServerHandle};
use crate::ws_server::{WsServerBuilder, WsServerHandle};

use jsonrpsee::types::Params;
use jsonrpsee_core::middleware::{Headers, HttpMiddleware, MethodKind, WsMiddleware};
use prometheus::{
    register_histogram_vec_with_registry, register_int_counter_vec_with_registry, HistogramVec,
    IntCounterVec,
};
use std::env;
use std::net::SocketAddr;
use std::time::Instant;
use sui_open_rpc::{Module, Project};
use tracing::info;

pub use jsonrpsee::http_server;
pub use jsonrpsee::ws_server;

pub mod api;
pub mod bcs_api;
pub mod event_api;
pub mod gateway_api;
pub mod quorum_driver_api;
pub mod read_api;

pub enum ServerBuilder<M = ()> {
    HttpBuilder(HttpServerBuilder<M>),
    WsBuilder(WsServerBuilder<M>),
}

pub enum ServerHandle {
    HttpHandler(HttpServerHandle),
    WsHandle(WsServerHandle),
}

#[derive(Clone)]
pub enum ApiMetrics {
    JsonRpcMetrics(JsonRpcMetrics),
    WebsocketMetrics(WebsocketMetrics),
}

impl ServerHandle {
    pub fn into_http_server_handle(self) -> Option<HttpServerHandle> {
        match self {
            ServerHandle::HttpHandler(handle) => Some(handle),
            _ => None,
        }
    }

    pub fn into_ws_server_handle(self) -> Option<WsServerHandle> {
        match self {
            ServerHandle::WsHandle(handle) => Some(handle),
            _ => None,
        }
    }
}

pub struct JsonRpcServerBuilder {
    module: RpcModule<()>,
    server_builder: ServerBuilder<ApiMetrics>,
    rpc_doc: Project,
}

pub fn sui_rpc_doc() -> Project {
    Project::new(
        "Sui JSON-RPC",
        "Sui JSON-RPC API for interaction with the Sui network gateway.",
        "Mysten Labs",
        "https://mystenlabs.com",
        "build@mystenlabs.com",
        "Apache-2.0",
        "https://raw.githubusercontent.com/MystenLabs/sui/main/LICENSE",
    )
}

impl JsonRpcServerBuilder {
    pub fn new(
        use_websocket: bool,
        prometheus_registry: &prometheus::Registry,
    ) -> anyhow::Result<Self> {
        let acl = match env::var("ACCESS_CONTROL_ALLOW_ORIGIN") {
            Ok(value) => {
                let owned_list: Vec<String> = value
                    .split(',')
                    .into_iter()
                    .map(|s| s.into())
                    .collect::<Vec<_>>();
                AccessControlBuilder::default().set_allowed_origins(&owned_list)?
            }
            _ => AccessControlBuilder::default(),
        }
        .build();
        info!(?acl);

        let server_builder = if use_websocket {
            ServerBuilder::WsBuilder(
                WsServerBuilder::default()
                    .set_access_control(acl)
                    .set_middleware(ApiMetrics::WebsocketMetrics(WebsocketMetrics {})),
            )
        } else {
            ServerBuilder::HttpBuilder(
                HttpServerBuilder::default()
                    .set_access_control(acl)
                    .set_middleware(ApiMetrics::JsonRpcMetrics(JsonRpcMetrics::new(
                        prometheus_registry,
                    ))),
            )
        };

        let module = RpcModule::new(());

        Ok(Self {
            module,
            server_builder,
            rpc_doc: sui_rpc_doc(),
        })
    }

    pub fn register_module<T: SuiRpcModule>(&mut self, module: T) -> Result<(), anyhow::Error> {
        self.rpc_doc.add_module(T::rpc_doc_module());
        self.module.merge(module.rpc()).map_err(Into::into)
    }

    pub async fn start(
        mut self,
        listen_address: SocketAddr,
    ) -> Result<ServerHandle, anyhow::Error> {
        self.module
            .register_method("rpc.discover", move |_, _| Ok(self.rpc_doc.clone()))?;
        let methods_names = self.module.method_names().collect::<Vec<_>>();
        let (handle, addr, server_name) = match self.server_builder {
            ServerBuilder::HttpBuilder(http_builder) => {
                let server = http_builder.build(listen_address).await?;
                let addr = server.local_addr()?;
                let handle = server.start(self.module)?;
                (ServerHandle::HttpHandler(handle), addr, "JSON-RPC")
            }
            ServerBuilder::WsBuilder(ws_builder) => {
                let server = ws_builder.build(listen_address).await?;
                let addr = server.local_addr()?;
                let handle = server.start(self.module)?;
                (ServerHandle::WsHandle(handle), addr, "Websocket")
            }
        };
        info!(local_addr =? addr, "Sui {server_name} server listening on {addr}");
        info!("Available {server_name} methods : {:?}", methods_names);

        Ok(handle)
    }
}

#[derive(Clone)]
pub struct JsonRpcMetrics {
    /// Counter of requests, route is a label (ie separate timeseries per route)
    requests_by_route: IntCounterVec,
    /// Request latency, route is a label
    req_latency_by_route: HistogramVec,
    /// Failed requests by route
    errors_by_route: IntCounterVec,
}

impl JsonRpcMetrics {
    pub fn new(registry: &prometheus::Registry) -> Self {
        Self {
            requests_by_route: register_int_counter_vec_with_registry!(
                "rpc_requests_by_route",
                "Number of requests by route",
                &["route"],
                registry,
            )
            .unwrap(),
            req_latency_by_route: register_histogram_vec_with_registry!(
                "req_latency_by_route",
                "Latency of a request by route",
                &["route"],
                registry,
            )
            .unwrap(),
            errors_by_route: register_int_counter_vec_with_registry!(
                "errors_by_route",
                "Number of errors by route",
                &["route"],
                registry,
            )
            .unwrap(),
        }
    }
}

// TODO: add metrics middleware for ws server
#[derive(Clone)]
pub struct WebsocketMetrics {}

impl HttpMiddleware for ApiMetrics {
    type Instant = Instant;

    fn on_request(&self, _remote_addr: SocketAddr, _headers: &Headers) -> Instant {
        Instant::now()
    }

    fn on_call(&self, _method_name: &str, _params: Params, _kind: MethodKind) {}

    fn on_result(&self, name: &str, success: bool, started_at: Instant) {
        if let ApiMetrics::JsonRpcMetrics(JsonRpcMetrics {
            requests_by_route,
            req_latency_by_route,
            errors_by_route,
        }) = self
        {
            requests_by_route.with_label_values(&[name]).inc();
            let req_latency_secs = (Instant::now() - started_at).as_secs_f64();
            req_latency_by_route
                .with_label_values(&[name])
                .observe(req_latency_secs);
            if !success {
                errors_by_route.with_label_values(&[name]).inc();
            }
        }
    }

    fn on_response(&self, _result: &str, _started_at: Self::Instant) {}
}

impl WsMiddleware for ApiMetrics {
    type Instant = Instant;

    fn on_connect(&self, _remote_addr: SocketAddr, _headers: &Headers) {}

    fn on_request(&self) -> Self::Instant {
        Instant::now()
    }

    fn on_call(&self, _method_name: &str, _params: Params, _kind: MethodKind) {}

    fn on_result(&self, _method_name: &str, _success: bool, _started_at: Self::Instant) {}

    fn on_response(&self, _result: &str, _started_at: Self::Instant) {}

    fn on_disconnect(&self, _remote_addr: SocketAddr) {}
}

pub trait SuiRpcModule
where
    Self: Sized,
{
    fn rpc(self) -> RpcModule<Self>;
    fn rpc_doc_module() -> Module;
}
