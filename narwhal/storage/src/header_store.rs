// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::NodeStorage;
use store::rocks::ReadWriteOptions;
use store::rocks::{open_cf, DBMap, MetricConf};
use store::{reopen, Map, TypedStoreError};
use types::{Header, HeaderDigest};

#[derive(Clone)]
pub struct HeaderStore {
    store: DBMap<HeaderDigest, Header>,
}

impl HeaderStore {
    pub fn new(header_store: DBMap<HeaderDigest, Header>) -> Self {
        Self {
            store: header_store,
        }
    }

    pub fn new_for_tests() -> Self {
        let rocksdb = open_cf(
            tempfile::tempdir().unwrap(),
            None,
            MetricConf::default(),
            &[NodeStorage::HEADERS_CF],
        )
        .expect("Cannot open database");
        let map = reopen!(&rocksdb, NodeStorage::HEADERS_CF;<HeaderDigest, Header>);
        Self::new(map)
    }

    pub fn read(&self, id: &HeaderDigest) -> Result<Option<Header>, TypedStoreError> {
        self.store.get(id)
    }

    pub fn write(&self, header: &Header) -> Result<(), TypedStoreError> {
        self.store.insert(&header.digest(), header)
    }

    pub fn remove_all(
        &self,
        keys: impl IntoIterator<Item = HeaderDigest>,
    ) -> Result<(), TypedStoreError> {
        self.store.multi_remove(keys)
    }
}
