// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
use crate::api::EventReadApiServer;
use crate::api::EventStreamingApiServer;
use crate::SuiRpcModule;
use async_trait::async_trait;
use futures::{StreamExt, TryStream};
use jsonrpsee::core::RpcResult;
use jsonrpsee::types::SubscriptionResult;
use jsonrpsee_core::error::SubscriptionClosed;
use jsonrpsee_core::server::rpc_module::RpcModule;
use jsonrpsee_core::server::rpc_module::SubscriptionSink;
use serde::Serialize;
use std::fmt::Display;
use std::sync::Arc;
use sui_core::authority::AuthorityState;
use sui_core::event_handler::EventHandler;
use sui_json_rpc_types::{SuiEvent, SuiEventEnvelope, SuiEventFilter};
use sui_open_rpc::Module;
use sui_types::base_types::{ObjectID, SuiAddress, TransactionDigest};
use tracing::warn;

pub struct EventStreamingApiImpl {
    state: Arc<AuthorityState>,
    event_handler: Arc<EventHandler>,
}

impl EventStreamingApiImpl {
    pub fn new(state: Arc<AuthorityState>, event_handler: Arc<EventHandler>) -> Self {
        Self {
            state,
            event_handler,
        }
    }
}

#[async_trait]
impl EventStreamingApiServer for EventStreamingApiImpl {
    fn subscribe_event(
        &self,
        mut sink: SubscriptionSink,
        filter: SuiEventFilter,
    ) -> SubscriptionResult {
        let filter = match filter.try_into() {
            Ok(filter) => filter,
            Err(e) => {
                let e = jsonrpsee_core::Error::from(e);
                warn!(error = ?e, "Rejecting subscription request.");
                return Ok(sink.reject(e)?);
            }
        };

        let state = self.state.clone();
        let stream = self.event_handler.subscribe(filter);
        let stream = stream.map(move |e| {
            let event = SuiEvent::try_from(e.event, state.module_cache.as_ref());
            event.map(|event| SuiEventEnvelope {
                timestamp: e.timestamp,
                tx_digest: e.tx_digest,
                event,
            })
        });
        spawn_subscript(sink, stream);

        Ok(())
    }
}

fn spawn_subscript<S, T, E>(mut sink: SubscriptionSink, rx: S)
where
    S: TryStream<Ok = T, Error = E> + Unpin + Send + 'static,
    T: Serialize,
    E: Display,
{
    tokio::spawn(async move {
        match sink.pipe_from_try_stream(rx).await {
            SubscriptionClosed::Success => {
                sink.close(SubscriptionClosed::Success);
            }
            SubscriptionClosed::RemotePeerAborted => (),
            SubscriptionClosed::Failed(err) => {
                warn!(error = ?err, "Event subscription closed.");
                sink.close(err);
            }
        };
    });
}

impl SuiRpcModule for EventStreamingApiImpl {
    fn rpc(self) -> RpcModule<Self> {
        self.into_rpc()
    }

    fn rpc_doc_module() -> Module {
        crate::api::EventStreamingApiOpenRpc::module_doc()
    }
}

#[allow(unused)]
pub struct EventReadApiImpl {
    state: Arc<AuthorityState>,
    event_handler: Arc<EventHandler>,
}

impl EventReadApiImpl {
    pub fn new(state: Arc<AuthorityState>, event_handler: Arc<EventHandler>) -> Self {
        Self {
            state,
            event_handler,
        }
    }
}

#[allow(unused)]
#[async_trait]
impl EventReadApiServer for EventReadApiImpl {
    async fn get_events_by_transaction(
        &self,
        digest: TransactionDigest,
    ) -> RpcResult<Vec<SuiEventEnvelope>> {
        Ok(vec![])
    }

    async fn get_events_by_module(
        &self,
        package: ObjectID,
        module: String,
        count: u64,
        start_time: u64,
        end_time: u64,
    ) -> RpcResult<Vec<SuiEventEnvelope>> {
        Ok(vec![])
    }

    async fn get_events_by_event_type(
        &self,
        event_type: String,
        count: u64,
        start_time: u64,
        end_time: u64,
    ) -> RpcResult<Vec<SuiEventEnvelope>> {
        Ok(vec![])
    }

    async fn get_events_by_sender(
        &self,
        sender: SuiAddress,
        count: u64,
        start_time: u64,
        end_time: u64,
    ) -> RpcResult<Vec<SuiEventEnvelope>> {
        Ok(vec![])
    }

    async fn get_events_by_object(
        &self,
        object: ObjectID,
        count: u64,
        start_time: u64,
        end_time: u64,
    ) -> RpcResult<Vec<SuiEventEnvelope>> {
        Ok(vec![])
    }

    async fn get_events_by_owner(
        &self,
        owner: SuiAddress,
        count: u64,
        start_time: u64,
        end_time: u64,
    ) -> RpcResult<Vec<SuiEventEnvelope>> {
        Ok(vec![])
    }
}

impl SuiRpcModule for EventReadApiImpl {
    fn rpc(self) -> RpcModule<Self> {
        self.into_rpc()
    }

    fn rpc_doc_module() -> Module {
        crate::api::EventReadApiOpenRpc::module_doc()
    }
}
