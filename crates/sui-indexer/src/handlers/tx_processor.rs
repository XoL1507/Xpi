// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use async_trait::async_trait;
use move_binary_format::CompiledModule;
use move_core_types::language_storage::ModuleId;
use mysten_metrics::monitored_scope;
use mysten_metrics::spawn_monitored_task;
use tokio::sync::watch;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use sui_types::object::Object;
use tokio::time::Duration;
use tokio::time::Instant;

use sui_json_rpc::get_balance_changes_from_effect;
use sui_json_rpc::get_object_changes;
use sui_json_rpc::ObjectProvider;
use sui_types::base_types::SequenceNumber;
use sui_types::digests::TransactionDigest;
use sui_types::effects::{TransactionEffects, TransactionEffectsAPI};
use sui_types::transaction::{TransactionData, TransactionDataAPI};
use tracing::debug;

use sui_types::base_types::ObjectID;
use sui_types::messages_checkpoint::CheckpointSequenceNumber;

use crate::errors::IndexerError;
use crate::metrics::IndexerMetrics;

use crate::types_v2::IndexedPackage;
use crate::types_v2::{IndexedObjectChange, IndexerResult};

pub struct InMemPackageCache {
    packages: HashMap<(ObjectID, String), (Arc<CompiledModule>, CheckpointSequenceNumber)>,
}

impl InMemPackageCache {
    pub fn start(
        commit_watcher: watch::Receiver<Option<CheckpointSequenceNumber>>,
    ) -> Arc<Mutex<Self>> {
        let cache = Arc::new(Mutex::new(Self {
            packages: HashMap::new(),
        }));
        let cache_clone = cache.clone();
        spawn_monitored_task!(Self::remove_committed(cache_clone, commit_watcher));
        cache
    }

    pub async fn remove_committed(
        cache: Arc<Mutex<Self>>,
        commit_watcher: watch::Receiver<Option<CheckpointSequenceNumber>>,
    ) {
        // GC every 10 minutes
        let mut interval = tokio::time::interval_at(Instant::now(), Duration::from_secs(600));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let _scope = monitored_scope("InMemObjectCache::remove_committed");
            let Some(committed_checkpoint) = *commit_watcher.borrow() else {
                continue;
            };
            debug!("About to GC packages older than: {committed_checkpoint}");

            let mut cache = cache.lock().unwrap();
            let mut to_remove = vec![];
            for (id, (_, checkpoint_seq)) in cache.packages.iter() {
                if *checkpoint_seq <= committed_checkpoint {
                    to_remove.push(id.clone());
                }
            }
            for id in to_remove {
                cache.packages.remove(&id);
            }
        }
    }

    pub fn insert_packages(
        &mut self,
        new_packages: &[IndexedPackage],
        checkpoint_seq: CheckpointSequenceNumber,
    ) {
        let new_packages = new_packages
            .iter()
            .flat_map(|p| {
                p.move_package
                    .serialized_module_map()
                    .iter()
                    .map(|(module_name, bytes)| {
                        let module = CompiledModule::deserialize_with_defaults(bytes).unwrap();
                        (
                            (p.package_id, module_name.clone()),
                            (Arc::new(module), checkpoint_seq),
                        )
                    })
            })
            .collect::<HashMap<_, _>>();
        self.packages.extend(new_packages);
    }

    pub fn get_module_by_id(&self, id: &ModuleId) -> Option<Arc<CompiledModule>> {
        let package_id = ObjectID::from(*id.address());
        let name = id.name().to_string();
        self.packages
            .get(&(package_id, name))
            .as_ref()
            .map(|(m, _)| m.clone())
    }
}

pub struct InMemObjectCache {
    id_map: HashMap<ObjectID, Arc<Object>>,
    seq_map: HashMap<(ObjectID, SequenceNumber), Arc<Object>>,
}

impl InMemObjectCache {
    pub fn new() -> Self {
        Self {
            id_map: HashMap::new(),
            seq_map: HashMap::new(),
        }
    }

    pub fn insert_object(&mut self, object: Object) {
        let obj = Arc::new(object);
        self.id_map.insert(obj.id(), obj.clone());
        self.seq_map.insert((obj.id(), obj.version()), obj);
    }

    pub fn get(&self, id: &ObjectID, version: Option<&SequenceNumber>) -> Option<&Object> {
        if let Some(version) = version {
            self.seq_map.get(&(*id, *version)).map(|o| o.as_ref())
        } else {
            self.id_map.get(id).map(|o| o.as_ref())
        }
    }
}

pub struct TxChangesProcessor {
    object_cache: InMemObjectCache,
    metrics: IndexerMetrics,
}

impl TxChangesProcessor {
    pub fn new(objects: &[&Object], metrics: IndexerMetrics) -> Self {
        let mut object_cache = InMemObjectCache::new();
        for obj in objects {
            object_cache.insert_object(<&Object>::clone(obj).clone());
        }
        Self {
            object_cache,
            metrics,
        }
    }

    pub(crate) async fn get_changes(
        &self,
        tx: &TransactionData,
        effects: &TransactionEffects,
        tx_digest: &TransactionDigest,
    ) -> IndexerResult<(
        Vec<sui_json_rpc_types::BalanceChange>,
        Vec<IndexedObjectChange>,
    )> {
        let _timer = self
            .metrics
            .indexing_tx_object_changes_latency
            .start_timer();
        let object_change: Vec<_> = get_object_changes(
            self,
            tx.sender(),
            effects.modified_at_versions(),
            effects.all_changed_objects(),
            effects.all_removed_objects(),
        )
        .await?
        .into_iter()
        .map(IndexedObjectChange::from)
        .collect();
        let balance_change = get_balance_changes_from_effect(
            self,
            effects,
            tx.input_objects().unwrap_or_else(|e| {
                panic!(
                    "Checkpointed tx {:?} has inavlid input objects: {e}",
                    tx_digest,
                )
            }),
            None,
        )
        .await?;
        Ok((balance_change, object_change))
    }
}

// Note: the implementation of `ObjectProvider` for `TxChangesProcessor`
// is NOT trivial. It needs to be a ObjectProvider to do
// `try_create_dynamic_field_info`. So the logic below is tailored towards that.
#[async_trait]
impl ObjectProvider for TxChangesProcessor {
    type Error = IndexerError;

    async fn get_object(
        &self,
        id: &ObjectID,
        version: &SequenceNumber,
    ) -> Result<Object, Self::Error> {
        let object = self
            .object_cache
            .get(id, Some(version))
            .as_ref()
            .map(|o| <&Object>::clone(o).clone());
        if let Some(o) = object {
            self.metrics.indexing_get_object_in_mem_hit.inc();
            return Ok(o);
        }

        panic!(
            "Object {} is not found in TxChangesProcessor as an ObjectProvider (fn get_object)",
            id
        );
    }

    async fn find_object_lt_or_eq_version(
        &self,
        id: &ObjectID,
        version: &SequenceNumber,
    ) -> Result<Option<Object>, Self::Error> {
        // First look up the exact version in object_cache.
        let object = self
            .object_cache
            .get(id, Some(version))
            .as_ref()
            .map(|o| <&Object>::clone(o).clone());
        if let Some(o) = object {
            self.metrics.indexing_get_object_in_mem_hit.inc();
            return Ok(Some(o));
        }

        // Second look up the latest version in object_cache. This may be
        // called when the object is deleted hence the version at deletion
        // is given.
        let object = self
            .object_cache
            .get(id, None)
            .as_ref()
            .map(|o| <&Object>::clone(o).clone());
        if let Some(o) = object {
            if o.version() > *version {
                panic!(
                    "Found a higher version {} for object {}, expected lt_or_eq {}",
                    o.version(),
                    id,
                    *version
                );
            }
            if o.version() <= *version {
                self.metrics.indexing_get_object_in_mem_hit.inc();
                return Ok(Some(o));
            }
        }

        panic!("Object {} is not found in TxChangesProcessor as an ObjectProvider (fn find_object_lt_or_eq_version)", id);
    }
}
