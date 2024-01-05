// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::ops::Neg;
use std::sync::Arc;

use async_trait::async_trait;
use move_core_types::language_storage::TypeTag;
use tokio::sync::RwLock;

use sui_core::authority::AuthorityState;
use sui_json_rpc_types::BalanceChange;
use sui_types::base_types::{ObjectID, ObjectRef, SequenceNumber};
use sui_types::coin::Coin;
use sui_types::error::SuiError;
use sui_types::gas_coin::GAS;
use sui_types::messages::TransactionEffectsAPI;
use sui_types::messages::{ExecutionStatus, TransactionEffects};
use sui_types::object::{Object, Owner};
use sui_types::storage::WriteKind;

pub async fn get_balance_changes_from_effect<P: ObjectProvider<Error = E>, E>(
    object_provider: &P,
    effects: &TransactionEffects,
) -> Result<Vec<BalanceChange>, E> {
    let (_, gas_owner) = effects.gas_object();

    // Only charge gas when tx fails, skip all object parsing
    if effects.status() != &ExecutionStatus::Success {
        return Ok(vec![BalanceChange {
            owner: *gas_owner,
            coin_type: GAS::type_tag(),
            amount: effects.gas_cost_summary().net_gas_usage().neg() as i128,
        }]);
    }

    let all_mutated: Vec<(&ObjectRef, &Owner, WriteKind)> = effects.all_changed_objects();
    let all_mutated = all_mutated
        .iter()
        .map(|((id, version, _), _, _)| (*id, *version))
        .collect::<Vec<_>>();

    get_balance_changes(
        object_provider,
        effects.modified_at_versions(),
        &all_mutated,
    )
    .await
}

pub async fn get_balance_changes<P: ObjectProvider<Error = E>, E>(
    object_provider: &P,
    modified_at_version: &[(ObjectID, SequenceNumber)],
    all_mutated: &[(ObjectID, SequenceNumber)],
) -> Result<Vec<BalanceChange>, E> {
    // 1. subtract all input coins
    let balances = fetch_coins(object_provider, modified_at_version)
        .await?
        .into_iter()
        .fold(
            BTreeMap::<_, i128>::new(),
            |mut acc, (owner, type_, amount)| {
                *acc.entry((owner, type_)).or_default() -= amount as i128;
                acc
            },
        );
    // 2. add all mutated coins
    let balances = fetch_coins(object_provider, all_mutated)
        .await?
        .into_iter()
        .fold(balances, |mut acc, (owner, type_, amount)| {
            *acc.entry((owner, type_)).or_default() += amount as i128;
            acc
        });

    Ok(balances
        .into_iter()
        .filter_map(|((owner, coin_type), amount)| {
            if amount == 0 {
                return None;
            }
            Some(BalanceChange {
                owner,
                coin_type,
                amount,
            })
        })
        .collect())
}

async fn fetch_coins<P: ObjectProvider<Error = E>, E>(
    object_provider: &P,
    objects: &[(ObjectID, SequenceNumber)],
) -> Result<Vec<(Owner, TypeTag, u64)>, E> {
    let mut all_mutated_coins = vec![];
    for (id, version) in objects {
        // TODO: use multi get object
        if let Ok(o) = object_provider.get_object(id, version).await {
            if let Some(type_) = o.type_() {
                if type_.is_coin() {
                    let [coin_type]: [TypeTag; 1] =
                        type_.clone().into_type_params().try_into().unwrap();
                    all_mutated_coins.push((
                        o.owner,
                        coin_type,
                        // we know this is a coin, safe to unwrap
                        Coin::extract_balance_if_coin(&o).unwrap().unwrap(),
                    ))
                }
            }
        }
    }
    Ok(all_mutated_coins)
}

#[async_trait]
pub trait ObjectProvider {
    type Error;
    async fn get_object(
        &self,
        id: &ObjectID,
        version: &SequenceNumber,
    ) -> Result<Object, Self::Error>;
    async fn find_object_lt_or_eq_version(
        &self,
        id: &ObjectID,
        version: &SequenceNumber,
    ) -> Result<Option<Object>, Self::Error>;
}

#[async_trait]
impl ObjectProvider for Arc<AuthorityState> {
    type Error = SuiError;
    async fn get_object(
        &self,
        id: &ObjectID,
        version: &SequenceNumber,
    ) -> Result<Object, Self::Error> {
        Ok(self
            .get_past_object_read(id, *version)
            .await?
            .into_object()?)
    }

    async fn find_object_lt_or_eq_version(
        &self,
        id: &ObjectID,
        version: &SequenceNumber,
    ) -> Result<Option<Object>, Self::Error> {
        Ok(self.database.find_object_lt_or_eq_version(*id, *version))
    }
}

pub struct ObjectProviderCache<P> {
    object_cache: RwLock<BTreeMap<(ObjectID, SequenceNumber), Object>>,
    last_version_cache: RwLock<BTreeMap<(ObjectID, SequenceNumber), SequenceNumber>>,
    provider: P,
}

impl<P> ObjectProviderCache<P> {
    pub fn new(provider: P) -> Self {
        Self {
            object_cache: Default::default(),
            last_version_cache: Default::default(),
            provider,
        }
    }

    pub fn new_with_cache(
        provider: P,
        written_objects: BTreeMap<ObjectID, (ObjectRef, Object, WriteKind)>,
    ) -> Self {
        let mut object_cache = BTreeMap::new();
        let mut last_version_cache = BTreeMap::new();

        for (object_id, (object_ref, object, _)) in written_objects {
            let key = (object_id, object_ref.1);
            object_cache.insert(key, object.clone());

            match last_version_cache.get_mut(&key) {
                Some(existing_seq_number) => {
                    if object_ref.1 > *existing_seq_number {
                        *existing_seq_number = object_ref.1
                    }
                }
                None => {
                    last_version_cache.insert(key, object_ref.1);
                }
            }
        }

        Self {
            object_cache: RwLock::new(object_cache),
            last_version_cache: RwLock::new(last_version_cache),
            provider,
        }
    }
}

#[async_trait]
impl<P, E> ObjectProvider for ObjectProviderCache<P>
where
    P: ObjectProvider<Error = E> + Sync + Send,
    E: Sync + Send,
{
    type Error = P::Error;

    async fn get_object(
        &self,
        id: &ObjectID,
        version: &SequenceNumber,
    ) -> Result<Object, Self::Error> {
        if let Some(o) = self.object_cache.read().await.get(&(*id, *version)) {
            return Ok(o.clone());
        }
        let o = self.provider.get_object(id, version).await?;
        self.object_cache
            .write()
            .await
            .insert((*id, *version), o.clone());
        Ok(o)
    }

    async fn find_object_lt_or_eq_version(
        &self,
        id: &ObjectID,
        version: &SequenceNumber,
    ) -> Result<Option<Object>, Self::Error> {
        if let Some(version) = self.last_version_cache.read().await.get(&(*id, *version)) {
            return Ok(self.get_object(id, version).await.ok());
        }
        if let Some(o) = self
            .provider
            .find_object_lt_or_eq_version(id, version)
            .await?
        {
            self.object_cache
                .write()
                .await
                .insert((*id, o.version()), o.clone());
            self.last_version_cache
                .write()
                .await
                .insert((*id, *version), o.version());
            Ok(Some(o))
        } else {
            Ok(None)
        }
    }
}
