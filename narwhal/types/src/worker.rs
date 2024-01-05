// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{collections::HashSet, sync::Arc};

use crate::{Batch, BatchDigest, Round};

use crypto::NetworkPublicKey;
use mysten_common::notify_once::NotifyOnce;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::oneshot::Sender;

#[cfg(test)]
#[path = "tests/batch_serde.rs"]
mod batch_serde;

/// Used by worker to inform its payload fetcher about a batch that needs to be
/// fetched.
#[derive(Debug)]
pub struct WorkerFetchBatchMessage {
    pub digest: BatchDigest,
    // workers who should have the batch available for fetching.
    pub fetch_candidates: HashSet<NetworkPublicKey>,
    // if true batch contents need to be validated else just the digest.
    pub validate: bool,
    // if true an immediate request for the batch should be sent.
    pub fetch_now: bool,
    // Maximum age of the batch.
    pub max_age: Round,
    pub notify_sender: Sender<Arc<NotifyOnce>>,
}

/// Used by workers to send a new batch.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkerBatchMessage {
    pub batch: Batch,
}

/// Used by primary to ask worker for the request.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestBatchRequest {
    pub batch: BatchDigest,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestBatchResponse {
    pub batch: Option<Batch>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestBatchesRequest {
    pub batches: Vec<BatchDigest>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestBatchesResponse {
    pub batches: Vec<Option<Batch>>,
}

pub type TxResponse = tokio::sync::oneshot::Sender<BatchDigest>;
pub type PrimaryResponse = Option<tokio::sync::oneshot::Sender<()>>;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DigestError {
    #[error("Invalid argument: invalid byte at {0}")]
    InvalidArgumentError(usize),
    #[error("Invalid length")]
    InvalidLengthError,
}
