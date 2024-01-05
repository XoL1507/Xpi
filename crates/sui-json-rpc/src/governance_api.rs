// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use jsonrpsee::core::RpcResult;
use std::collections::BTreeMap;
use std::sync::Arc;
use sui_json_rpc_types::SuiCommittee;
use sui_types::sui_system_state::sui_system_state_summary::SuiSystemStateSummary;

use crate::api::GovernanceReadApiServer;
use crate::error::Error;
use crate::SuiRpcModule;
use async_trait::async_trait;
use jsonrpsee::RpcModule;
use sui_core::authority::AuthorityState;
use sui_json_rpc_types::{DelegatedStake, Stake, StakeStatus};
use sui_open_rpc::Module;
use sui_types::base_types::{MoveObjectType, ObjectID, SuiAddress};
use sui_types::committee::EpochId;
use sui_types::governance::StakedSui;
use sui_types::sui_system_state::sui_system_state_inner_v1::{PoolTokenExchangeRate, ValidatorV1};
use sui_types::sui_system_state::SuiSystemState;
use sui_types::sui_system_state::SuiSystemStateTrait;

pub struct GovernanceReadApi {
    state: Arc<AuthorityState>,
}

impl GovernanceReadApi {
    pub fn new(state: Arc<AuthorityState>) -> Self {
        Self { state }
    }

    async fn get_staked_sui(&self, owner: SuiAddress) -> Result<Vec<StakedSui>, Error> {
        Ok(self
            .state
            .get_move_objects(owner, MoveObjectType::StakedSui)
            .await?)
    }

    async fn get_delegated_stakes(&self, owner: SuiAddress) -> Result<Vec<DelegatedStake>, Error> {
        let stakes = self.get_staked_sui(owner).await?;
        if stakes.is_empty() {
            return Ok(vec![]);
        }

        let pools = stakes
            .into_iter()
            .fold(BTreeMap::<_, Vec<_>>::new(), |mut pools, s| {
                pools
                    .entry((s.pool_id(), s.validator_address()))
                    .or_default()
                    .push(s);
                pools
            });

        let system_state: SuiSystemStateSummary =
            self.get_system_state()?.into_sui_system_state_summary();
        let mut delegated_stakes = vec![];
        for ((pool_id, validator_address), stakes) in pools {
            let rate_table = self
                .get_exchange_rate_table(&system_state, &pool_id)
                .await?;

            let current_rate = self
                .get_exchange_rate(rate_table, system_state.epoch)
                .await?;

            let mut delegations = vec![];
            for stake in stakes {
                // delegation will be active in next epoch
                let status = if system_state.epoch >= stake.request_epoch() {
                    let stake_rate = self
                        .get_exchange_rate(rate_table, stake.request_epoch())
                        .await?;
                    let estimated_reward = (((stake_rate.rate() / current_rate.rate()) - 1.0)
                        * stake.principal() as f64)
                        .round() as u64;
                    StakeStatus::Active { estimated_reward }
                } else {
                    StakeStatus::Pending
                };
                delegations.push(Stake {
                    staked_sui_id: stake.id(),
                    stake_request_epoch: stake.request_epoch(),
                    // TODO: this might change when we implement warm up period.
                    stake_active_epoch: stake.request_epoch() + 1,
                    principal: stake.principal(),
                    token_lock: stake.sui_token_lock(),
                    status,
                })
            }

            delegated_stakes.push(DelegatedStake {
                validator_address,
                staking_pool: pool_id,
                stakes: delegations,
            })
        }
        Ok(delegated_stakes)
    }

    fn get_system_state(&self) -> Result<SuiSystemState, Error> {
        Ok(self.state.database.get_sui_system_state_object()?)
    }

    async fn get_exchange_rate_table(
        &self,
        system_state: &SuiSystemStateSummary,
        pool_id: &ObjectID,
    ) -> Result<ObjectID, Error> {
        let active_rate = system_state.active_validators.iter().find_map(|v| {
            if &v.staking_pool_id == pool_id {
                Some(v.exchange_rates_id)
            } else {
                None
            }
        });

        if let Some(active_rate) = active_rate {
            return Ok(active_rate);
        } else {
            // try find from inactive pool
            let inactive_validators = system_state.inactive_pools_id;
            let inactive_validators = self
                .state
                .read_table_value::<ObjectID, ValidatorV1>(inactive_validators, pool_id)
                .await;
            if let Some(inactive_validators) = inactive_validators {
                return Ok(inactive_validators.staking_pool.exchange_rates.id);
            }
        }
        Err(Error::UnexpectedError(format!(
            "Cannot find exchange rate table for pool [{pool_id}]."
        )))
    }

    async fn get_exchange_rate(
        &self,
        table: ObjectID,
        epoch: EpochId,
    ) -> Result<PoolTokenExchangeRate, Error> {
        self.state
            .read_table_value(table, &epoch)
            .await
            .ok_or_else(|| {
                Error::UnexpectedError(format!(
                    "Cannot find exchange rate for epoch [{epoch}], from rate table object [{table}]."
                ))
            })
    }
}

#[async_trait]
impl GovernanceReadApiServer for GovernanceReadApi {
    async fn get_delegated_stakes(&self, owner: SuiAddress) -> RpcResult<Vec<DelegatedStake>> {
        Ok(self.get_delegated_stakes(owner).await?)
    }

    async fn get_committee_info(&self, epoch: Option<EpochId>) -> RpcResult<SuiCommittee> {
        Ok(self
            .state
            .committee_store()
            .get_or_latest_committee(epoch)
            .map(|committee| committee.into())
            .map_err(Error::from)?)
    }

    async fn get_latest_sui_system_state(&self) -> RpcResult<SuiSystemStateSummary> {
        Ok(self
            .state
            .database
            .get_sui_system_state_object()
            .map_err(Error::from)?
            .into_sui_system_state_summary())
    }

    async fn get_reference_gas_price(&self) -> RpcResult<u64> {
        let epoch_store = self.state.load_epoch_store_one_call_per_task();
        Ok(epoch_store.reference_gas_price())
    }
}

impl SuiRpcModule for GovernanceReadApi {
    fn rpc(self) -> RpcModule<Self> {
        self.into_rpc()
    }

    fn rpc_doc_module() -> Module {
        crate::api::GovernanceReadApiOpenRpc::module_doc()
    }
}
