// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
use std::{marker::PhantomData, sync::Arc};

use bincode::Options;
use rocksdb::Direction;

use crate::metrics::{DBMetrics, SamplingInterval};

use super::{be_fix_int_ser, errors::TypedStoreError, RocksDBRawIter};
use serde::{de::DeserializeOwned, Serialize};

/// An iterator over all key-value pairs in a data map.
pub struct Iter<'a, K, V> {
    db_iter: RocksDBRawIter<'a>,
    _phantom: PhantomData<(K, V)>,
    direction: Direction,
    cf: String,
    db_metrics: Arc<DBMetrics>,
    iter_bytes_sample_interval: SamplingInterval,
}

impl<'a, K: DeserializeOwned, V: DeserializeOwned> Iter<'a, K, V> {
    pub(super) fn new(
        db_iter: RocksDBRawIter<'a>,
        cf: String,
        db_metrics: &Arc<DBMetrics>,
        iter_bytes_sample_interval: &SamplingInterval,
    ) -> Self {
        Self {
            db_iter,
            _phantom: PhantomData,
            direction: Direction::Forward,
            cf,
            db_metrics: db_metrics.clone(),
            iter_bytes_sample_interval: iter_bytes_sample_interval.clone(),
        }
    }
}

impl<'a, K: DeserializeOwned, V: DeserializeOwned> Iterator for Iter<'a, K, V> {
    type Item = (K, V);

    fn next(&mut self) -> Option<Self::Item> {
        if self.db_iter.valid() {
            let config = bincode::DefaultOptions::new()
                .with_big_endian()
                .with_fixint_encoding();
            let raw_key = self
                .db_iter
                .key()
                .expect("Valid iterator failed to get key");
            let raw_value = self
                .db_iter
                .value()
                .expect("Valid iterator failed to get value");
            let key = config.deserialize(raw_key).ok();
            let value = bincode::deserialize(raw_value).ok();
            if self.iter_bytes_sample_interval.sample() {
                let total_bytes_read = (raw_key.len() + raw_value.len()) as f64;
                self.db_metrics
                    .op_metrics
                    .rocksdb_iter_bytes
                    .with_label_values(&[&self.cf])
                    .observe(total_bytes_read);
            }
            match self.direction {
                Direction::Forward => self.db_iter.next(),
                Direction::Reverse => self.db_iter.prev(),
            }

            key.and_then(|k| value.map(|v| (k, v)))
        } else {
            None
        }
    }
}

impl<'a, K: Serialize, V> Iter<'a, K, V> {
    /// Skips all the elements that are smaller than the given key,
    /// and either lands on the key or the first one greater than
    /// the key.
    pub fn skip_to(mut self, key: &K) -> Result<Self, TypedStoreError> {
        self.db_iter.seek(be_fix_int_ser(key)?);
        Ok(self)
    }

    /// Moves the iterator the element given or
    /// the one prior to it if it does not exist. If there is
    /// no element prior to it, it returns an empty iterator.
    pub fn skip_prior_to(mut self, key: &K) -> Result<Self, TypedStoreError> {
        self.db_iter.seek_for_prev(be_fix_int_ser(key)?);
        Ok(self)
    }

    /// Seeks to the last key in the database (at this column family).
    pub fn skip_to_last(mut self) -> Self {
        self.db_iter.seek_to_last();
        self
    }

    /// Will make the direction of the iteration reverse and will
    /// create a new `RevIter` to consume. Every call to `next` method
    /// will give the next element from the end.
    pub fn reverse(mut self) -> RevIter<'a, K, V> {
        self.direction = Direction::Reverse;
        RevIter::new(self)
    }
}

/// An iterator with a reverted direction to the original. The `RevIter`
/// is hosting an iteration which is consuming in the opposing direction.
/// It's not possible to do further manipulation (ex re-reverse) to the
/// iterator.
pub struct RevIter<'a, K, V> {
    iter: Iter<'a, K, V>,
}

impl<'a, K, V> RevIter<'a, K, V> {
    fn new(iter: Iter<'a, K, V>) -> Self {
        Self { iter }
    }
}

impl<'a, K: DeserializeOwned, V: DeserializeOwned> Iterator for RevIter<'a, K, V> {
    type Item = (K, V);

    /// Will give the next item backwards
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}
