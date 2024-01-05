// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::sync::Arc;

use move_bytecode_utils::module_cache::SyncModuleCache;
use serde_json::Value;
use sui_json_rpc_api::rpc_types::{SuiMoveStruct, SuiMoveValue};
use tokio_stream::Stream;
use tracing::{debug, error, trace};

use sui_storage::event_store::{EventStore, EventStoreType};
use sui_types::base_types::TransactionDigest;
use sui_types::{
    error::{SuiError, SuiResult},
    event::{Event, EventEnvelope},
    messages::TransactionEffects,
};

use crate::authority::{AuthorityStore, ResolverWrapper};
use crate::streamer::Streamer;
use sui_types::event_filter::EventFilter;

#[cfg(test)]
#[path = "unit_tests/event_handler_tests.rs"]
mod event_handler_tests;

pub const EVENT_DISPATCH_BUFFER_SIZE: usize = 1000;

pub struct EventHandler {
    module_cache: SyncModuleCache<ResolverWrapper<AuthorityStore>>,
    event_streamer: Streamer<EventEnvelope, EventFilter>,
    pub(crate) event_store: Arc<EventStoreType>,
}

impl EventHandler {
    pub fn new(validator_store: Arc<AuthorityStore>, event_store: Arc<EventStoreType>) -> Self {
        let streamer = Streamer::spawn(EVENT_DISPATCH_BUFFER_SIZE);
        Self {
            module_cache: SyncModuleCache::new(ResolverWrapper(validator_store)),
            event_streamer: streamer,
            event_store,
        }
    }

    pub async fn process_events(
        &self,
        effects: &TransactionEffects,
        timestamp_ms: u64,
        seq_num: u64,
        checkpoint_num: u64,
    ) -> SuiResult {
        let res: Result<Vec<_>, _> = effects
            .events
            .iter()
            .map(|e| self.create_envelope(e, effects.transaction_digest, seq_num, timestamp_ms))
            .collect();
        let envelopes = res?;

        // Ingest all envelopes together at once (for efficiency) into Event Store
        self.event_store
            .add_events(&envelopes, checkpoint_num)
            .await?;
        trace!(
            num_events = envelopes.len(),
            digest =? effects.transaction_digest,
            checkpoint_num, "Finished writing events to event store"
        );

        // serially dispatch event processing to honor events' orders.
        for envelope in envelopes {
            if let Err(e) = self.event_streamer.send(envelope).await {
                error!(error =? e, "Failed to send EventEnvelope to dispatch");
            }
        }

        Ok(())
    }

    fn create_envelope(
        &self,
        event: &Event,
        digest: TransactionDigest,
        seq_num: u64,
        timestamp_ms: u64,
    ) -> Result<EventEnvelope, SuiError> {
        let json_value = match event {
            Event::MoveEvent {
                type_, contents, ..
            } => {
                debug!(event =? event, "Process MoveEvent.");
                let move_struct =
                    Event::move_event_to_move_struct(type_, contents, &self.module_cache)?;
                // Convert into `SuiMoveStruct` which is a mirror of MoveStruct but with additional type supports, (e.g. ascii::String).
                let sui_move_struct = move_struct.into();
                Some(to_json_value(sui_move_struct).map_err(|e| {
                    SuiError::ObjectSerializationError {
                        error: e.to_string(),
                    }
                })?)
            }
            _ => None,
        };

        Ok(EventEnvelope::new(
            timestamp_ms,
            Some(digest),
            seq_num,
            event.clone(),
            json_value,
        ))
    }

    pub fn subscribe(&self, filter: EventFilter) -> impl Stream<Item = EventEnvelope> {
        self.event_streamer.subscribe(filter)
    }
}

fn to_json_value(move_struct: SuiMoveStruct) -> Result<Value, serde_json::Error> {
    // Unwrap MoveStructs
    let unwrapped = match move_struct {
        SuiMoveStruct::Runtime(values) => {
            let values = values
                .into_iter()
                .map(|value| match value {
                    SuiMoveValue::Struct(move_struct) => to_json_value(move_struct),
                    SuiMoveValue::Vector(values) => to_json_value(SuiMoveStruct::Runtime(values)),
                    _ => serde_json::to_value(&value),
                })
                .collect::<Result<Vec<_>, _>>()?;
            serde_json::to_value(&values)
        }
        // We only care about values here, assuming struct type information is known at the client side.
        SuiMoveStruct::WithTypes { type_: _, fields } | SuiMoveStruct::WithFields(fields) => {
            let fields = fields
                .into_iter()
                .map(|(key, value)| {
                    let value = match value {
                        SuiMoveValue::Struct(move_struct) => to_json_value(move_struct),
                        SuiMoveValue::Vector(values) => {
                            to_json_value(SuiMoveStruct::Runtime(values))
                        }
                        _ => serde_json::to_value(&value),
                    };
                    value.map(|value| (key, value))
                })
                .collect::<Result<BTreeMap<_, _>, _>>()?;
            serde_json::to_value(&fields)
        }
    }?;
    serde_json::to_value(&unwrapped)
}
