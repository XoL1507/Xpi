// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use anyhow::anyhow;
use std::sync::Arc;

use async_trait::async_trait;
use jsonrpsee::core::RpcResult;
use jsonrpsee::RpcModule;
use move_core_types::language_storage::{StructTag, TypeTag};
use tracing::debug;

use sui_core::authority::AuthorityState;
use sui_json_rpc_types::{Balance, Coin as SuiCoin};
use sui_json_rpc_types::{CoinPage, SuiCoinMetadata};
use sui_open_rpc::Module;
use sui_types::balance::Supply;
use sui_types::base_types::{ObjectID, ObjectRef, SuiAddress};
use sui_types::coin::{CoinMetadata, TreasuryCap};
use sui_types::error::SuiError;
use sui_types::gas_coin::GAS;
use sui_types::messages::TransactionEffectsAPI;
use sui_types::object::{Object, Owner};
use sui_types::parse_sui_struct_tag;

use crate::api::{cap_page_limit, CoinReadApiServer, JsonRpcMetrics};
use crate::error::Error;
use crate::SuiRpcModule;

pub struct CoinReadApi {
    state: Arc<AuthorityState>,
    pub metrics: Arc<JsonRpcMetrics>,
}

impl CoinReadApi {
    pub fn new(state: Arc<AuthorityState>, metrics: Arc<JsonRpcMetrics>) -> Self {
        Self { state, metrics }
    }

    fn get_coins_iterator(
        &self,
        owner: SuiAddress,
        cursor: (String, ObjectID),
        limit: Option<usize>,
        one_coin_type_only: bool,
    ) -> anyhow::Result<CoinPage> {
        let limit = cap_page_limit(limit);
        self.metrics.get_coins_limit.report(limit as u64);
        let coins = self
            .state
            .get_owned_coins_iterator_with_cursor(owner, cursor, limit + 1, one_coin_type_only)?
            .map(|(coin_type, obj_id, coin)| (coin_type, obj_id, coin));

        let mut data = coins
            .map(|(coin_type, coin_object_id, coin)| SuiCoin {
                coin_type,
                coin_object_id,
                version: coin.version,
                digest: coin.digest,
                balance: coin.balance,
                locked_until_epoch: None,
                previous_transaction: coin.previous_transaction,
            })
            .collect::<Vec<_>>();

        let has_next_page = data.len() > limit;
        data.truncate(limit);

        self.metrics.get_coins_result_size.report(data.len() as u64);
        self.metrics
            .get_coins_result_size_total
            .inc_by(data.len() as u64);
        let next_cursor = data.last().map(|coin| coin.coin_object_id);
        Ok(CoinPage {
            data,
            next_cursor,
            has_next_page,
        })
    }

    async fn find_package_object(
        &self,
        package_id: &ObjectID,
        object_struct_tag: StructTag,
    ) -> Result<Object, Error> {
        let publish_txn_digest = self
            .state
            .get_object_read(package_id)?
            .into_object()?
            .previous_transaction;
        let (_, effect) = self
            .state
            .get_executed_transaction_and_effects(publish_txn_digest)
            .await?;
        let created: &[(ObjectRef, Owner)] = effect.created();

        let object_id = async {
            for ((id, version, _), _) in created {
                if let Ok(past_object) = self.state.get_past_object_read(id, *version) {
                    if let Ok(object) = past_object.into_object() {
                        if matches!(object.type_(), Some(type_) if type_.is(&object_struct_tag)) {
                            return Ok(*id);
                        }
                    }
                }
            }
            Err(anyhow!(
                "Cannot find object [{}] from [{}] package event.",
                object_struct_tag,
                package_id
            ))
        }
        .await?;
        Ok(self.state.get_object_read(&object_id)?.into_object()?)
    }
}

impl SuiRpcModule for CoinReadApi {
    fn rpc(self) -> RpcModule<Self> {
        self.into_rpc()
    }

    fn rpc_doc_module() -> Module {
        crate::api::CoinReadApiOpenRpc::module_doc()
    }
}

#[async_trait]
impl CoinReadApiServer for CoinReadApi {
    async fn get_coins(
        &self,
        owner: SuiAddress,
        coin_type: Option<String>,
        // exclusive cursor if `Some`, otherwise start from the beginning
        cursor: Option<ObjectID>,
        limit: Option<usize>,
    ) -> RpcResult<CoinPage> {
        let coin_type_tag = TypeTag::Struct(Box::new(match coin_type {
            Some(c) => parse_sui_struct_tag(&c)?,
            None => GAS::type_(),
        }));

        let cursor = match cursor {
            Some(c) => (coin_type_tag.to_string(), c),
            // If cursor is not specified, we need to start from the beginning of the coin type, which is the minimal possible ObjectID.
            None => (coin_type_tag.to_string(), ObjectID::ZERO),
        };

        let coins = self.get_coins_iterator(
            owner, cursor, limit, true, // only care about one type of coin
        )?;

        Ok(coins)
    }

    async fn get_all_coins(
        &self,
        owner: SuiAddress,
        // exclusive cursor if `Some`, otherwise start from the beginning
        cursor: Option<ObjectID>,
        limit: Option<usize>,
    ) -> RpcResult<CoinPage> {
        let cursor = match cursor {
            Some(object_id) => {
                let obj = self
                    .state
                    .get_object(&object_id)
                    .await
                    .map_err(Error::from)?;
                match obj {
                    Some(obj) => {
                        let coin_type = obj.coin_type_maybe();
                        if coin_type.is_none() {
                            Err(anyhow!(
                                "Invalid Cursor {:?}, Object is not a coin",
                                object_id
                            ))
                        } else {
                            Ok((coin_type.unwrap().to_string(), object_id))
                        }
                    }
                    None => Err(anyhow!("Invalid Cursor {:?}, Object not found", object_id)),
                }
            }
            None => {
                // If cursor is None, start from the beginning
                Ok((String::from_utf8([0u8].to_vec()).unwrap(), ObjectID::ZERO))
            }
        }?;

        let coins = self.get_coins_iterator(
            owner, cursor, limit, false, // return all types of coins
        )?;

        Ok(coins)
    }

    async fn get_balance(
        &self,
        owner: SuiAddress,
        coin_type: Option<String>,
    ) -> RpcResult<Balance> {
        let coin_type = TypeTag::Struct(Box::new(match coin_type {
            Some(c) => parse_sui_struct_tag(&c)?,
            None => GAS::type_(),
        }));
        let balance = self
            .state
            .indexes
            .as_ref()
            .ok_or(Error::SuiError(SuiError::IndexStoreNotAvailable))?
            .get_balance(owner, coin_type.clone())
            .await
            .map_err(|e: SuiError| {
                debug!(?owner, "Failed to get balance with error: {:?}", e);
                Error::from(e)
            })?;
        Ok(Balance {
            coin_type: coin_type.to_string(),
            coin_object_count: balance.num_coins as usize,
            total_balance: balance.balance as u128,
            // note: LockedCoin is deprecated
            locked_balance: Default::default(),
        })
    }

    async fn get_all_balances(&self, owner: SuiAddress) -> RpcResult<Vec<Balance>> {
        let all_balance = self
            .state
            .indexes
            .as_ref()
            .ok_or(Error::SuiError(SuiError::IndexStoreNotAvailable))?
            .get_all_balance(owner)
            .await
            .map_err(|e: SuiError| {
                debug!(?owner, "Failed to get all balance with error: {:?}", e);
                Error::from(e)
            })?;
        Ok(all_balance
            .iter()
            .map(|(coin_type, balance)| {
                Balance {
                    coin_type: coin_type.to_string(),
                    coin_object_count: balance.num_coins as usize,
                    total_balance: balance.balance as u128,
                    // note: LockedCoin is deprecated
                    locked_balance: Default::default(),
                }
            })
            .collect())
    }

    async fn get_coin_metadata(&self, coin_type: String) -> RpcResult<SuiCoinMetadata> {
        let coin_struct = parse_sui_struct_tag(&coin_type)?;

        let metadata_object = self
            .find_package_object(
                &coin_struct.address.into(),
                CoinMetadata::type_(coin_struct),
            )
            .await?;
        let metadata_object_id = metadata_object.id();
        Ok(metadata_object.try_into().map_err(|e: SuiError| {
            debug!(
                ?metadata_object_id,
                "Failed to convert object to CoinMetadata: {:?}", e
            );
            Error::from(e)
        })?)
    }

    async fn get_total_supply(&self, coin_type: String) -> RpcResult<Supply> {
        let coin_struct = parse_sui_struct_tag(&coin_type)?;

        Ok(if GAS::is_gas(&coin_struct) {
            Supply { value: 0 }
        } else {
            let treasury_cap_object = self
                .find_package_object(&coin_struct.address.into(), TreasuryCap::type_(coin_struct))
                .await?;

            let treasury_cap = TreasuryCap::from_bcs_bytes(
                treasury_cap_object.data.try_as_move().unwrap().contents(),
            )
            .map_err(Error::from)?;
            treasury_cap.total_supply
        })
    }
}
