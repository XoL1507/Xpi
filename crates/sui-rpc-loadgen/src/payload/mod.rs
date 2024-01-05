// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

mod rpc_command_processor;

use anyhow::Result;
use async_trait::async_trait;
use core::default::Default;
use std::str::FromStr;
use std::time::Duration;

use sui_types::messages_checkpoint::CheckpointSequenceNumber;

use crate::load_test::LoadTestConfig;
pub use rpc_command_processor::RpcCommandProcessor;
use sui_types::base_types::{ObjectID, SuiAddress};

#[derive(Default, Clone)]
pub struct SignerInfo {
    pub encoded_keypair: String,
    // TODO(chris): we should be able to derive this from the keypair?
    pub signer_address: SuiAddress,
    /// Different thread should use different gas_payment to avoid equivocation
    pub gas_payment: Option<ObjectID>,
    pub gas_budget: Option<u64>,
}

impl SignerInfo {
    pub fn new(encoded_keypair: String, signer_address: SuiAddress) -> Self {
        Self {
            encoded_keypair,
            signer_address,
            gas_payment: None,
            gas_budget: None,
        }
    }
}

#[derive(Clone, Default)]
pub struct Payload {
    pub commands: Vec<Command>,
    pub signer_info: Option<SignerInfo>,
}

#[derive(Default, Clone)]
pub struct Command {
    pub data: CommandData,
    /// 0 means the command will be run once. Default to be 0
    pub repeat_n_times: usize,
    /// how long to wait between the start of two subsequent repeats
    /// If the previous command takes longer than `repeat_interval` to finish, the next command
    /// will run as soon as the previous command finishes
    /// Default to be 0
    pub repeat_interval: Duration,
}

impl Command {
    pub fn new_dry_run() -> Self {
        Self {
            data: CommandData::DryRun(DryRun {}),
            ..Default::default()
        }
    }

    pub fn new_pay_sui() -> Self {
        Self {
            data: CommandData::PaySui(PaySui {}),
            ..Default::default()
        }
    }

    pub fn new_get_checkpoints(
        start: CheckpointSequenceNumber,
        end: Option<CheckpointSequenceNumber>,
        verify_transactions: bool,
        verify_objects: bool,
    ) -> Self {
        Self {
            data: CommandData::GetCheckpoints(GetCheckpoints {
                start,
                end,
                verify_transactions,
                verify_objects,
            }),
            ..Default::default()
        }
    }

    pub fn new_query_transactions(
        from_address: Option<String>,
        to_address: Option<String>,
    ) -> Self {
        let query_transactions = QueryTransactions {
            from_address: from_address.map(|addr| SuiAddress::from_str(&addr).unwrap()),
            to_address: to_address.map(|addr| SuiAddress::from_str(&addr).unwrap()),
        };
        Self {
            data: CommandData::QueryTransactions(query_transactions),
            ..Default::default()
        }
    }

    pub fn with_repeat_n_times(mut self, num: usize) -> Self {
        self.repeat_n_times = num;
        self
    }

    pub fn with_repeat_interval(mut self, duration: Duration) -> Self {
        self.repeat_interval = duration;
        self
    }
}

#[derive(Clone)]
#[allow(dead_code)]
pub enum CommandData {
    DryRun(DryRun),
    GetCheckpoints(GetCheckpoints),
    PaySui(PaySui),
    QueryTransactions(QueryTransactions),
}

impl Default for CommandData {
    fn default() -> Self {
        CommandData::DryRun(DryRun {})
    }
}

#[derive(Clone)]
pub struct DryRun {}

#[derive(Clone, Default)]
pub struct GetCheckpoints {
    /// Default to start from 0
    pub start: CheckpointSequenceNumber,
    /// If None, use `getLatestCheckpointSequenceNumber`
    pub end: Option<CheckpointSequenceNumber>,
    pub verify_transactions: bool,
    pub verify_objects: bool,
}

#[derive(Clone)]
pub struct PaySui {}

#[derive(Clone, Default)]
pub struct QueryTransactions {
    pub from_address: Option<SuiAddress>,
    pub to_address: Option<SuiAddress>,
}

#[async_trait]
pub trait Processor {
    /// process commands in order
    async fn apply(&self, payload: &Payload) -> Result<()>;

    /// prepare payload for each thread according to LoadTestConfig
    async fn prepare(&self, config: &LoadTestConfig) -> Result<Vec<Payload>>;
}

/// all payload should implement this trait
#[async_trait]
pub trait ProcessPayload<'a, T> {
    async fn process(&'a self, op: T, signer_info: &Option<SignerInfo>) -> Result<()>;
}
