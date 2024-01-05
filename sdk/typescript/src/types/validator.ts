// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import {
  array,
  boolean,
  literal,
  number,
  object,
  string,
  union,
  Infer,
  nullable,
  tuple,
  optional,
} from 'superstruct';
import { SuiAddress } from './common';
import { AuthorityName } from './transactions';

/* -------------- Types for the SuiSystemState Rust definition -------------- */

export type DelegatedStake = Infer<typeof DelegatedStake>;
export type CommitteeInfo = Infer<typeof CommitteeInfo>;

// Staking

export const Balance = object({
  value: number(),
});

export const StakedSui = object({
  id: object({
    id: string(),
  }),
  pool_id: string(),
  validator_address: string(),
  delegation_request_epoch: number(),
  principal: Balance,
  sui_token_lock: union([number(), literal(null)]),
});

export const ActiveFields = object({
  id: object({
    id: string(),
  }),
  staked_sui_id: SuiAddress,
  principal_sui_amount: number(),
  pool_tokens: Balance,
});

export const ActiveDelegationStatus = object({
  Active: ActiveFields,
});

export const DelegatedStake = object({
  staked_sui: StakedSui,
  delegation_status: union([literal('Pending'), ActiveDelegationStatus]),
});

export const ParametersFields = object({
  max_validator_count: string(),
  min_validator_stake: string(),
  storage_gas_price: optional(string()),
});

export const Parameters = object({
  type: string(),
  fields: ParametersFields,
});

export const StakeSubsidyFields = object({
  balance: object({ value: number() }),
  current_epoch_amount: number(),
  epoch_counter: number(),
});

export const StakeSubsidy = object({
  type: string(),
  fields: StakeSubsidyFields,
});

export const SuiSupplyFields = object({
  value: number(),
});

export const ContentsFields = object({
  id: string(),
  size: number(),
  head: object({ vec: array() }),
  tail: object({ vec: array() }),
});

export const ContentsFieldsWithdraw = object({
  id: string(),
  size: number(),
});

export const Contents = object({
  type: string(),
  fields: ContentsFields,
});

export const DelegationStakingPoolFields = object({
  exchange_rates: object({
    id: string(),
    size: number(),
  }),
  id: string(),
  pending_delegation: number(),
  pending_pool_token_withdraw: number(),
  pending_total_sui_withdraw: number(),
  pool_token_balance: number(),
  rewards_pool: object({ value: number() }),
  activation_epoch: object({ vec: array(number()) }),
  deactivation_epoch: object({ vec: array() }),
  sui_balance: number(),
});

export const DelegationStakingPool = object({
  type: string(),
  fields: DelegationStakingPoolFields,
});

export const CommitteeInfo = object({
  epoch: number(),
  /** Array of (validator public key, stake unit) tuple */
  validators: optional(array(tuple([AuthorityName, number()]))),
});

export const SuiValidatorSummary = object({
  sui_address: SuiAddress,
  protocol_pubkey_bytes: array(number()),
  network_pubkey_bytes: array(number()),
  worker_pubkey_bytes: array(number()),
  proof_of_possession_bytes: array(number()),
  name: string(),
  description: string(),
  image_url: string(),
  project_url: string(),
  p2p_address: array(number()),
  net_address: array(number()),
  primary_address: array(number()),
  worker_address: array(number()),
  next_epoch_protocol_pubkey_bytes: nullable(array(number())),
  next_epoch_proof_of_possession: nullable(array(number())),
  next_epoch_network_pubkey_bytes: nullable(array(number())),
  next_epoch_worker_pubkey_bytes: nullable(array(number())),
  next_epoch_net_address: nullable(array(number())),
  next_epoch_p2p_address: nullable(array(number())),
  next_epoch_primary_address: nullable(array(number())),
  next_epoch_worker_address: nullable(array(number())),
  voting_power: number(),
  gas_price: number(),
  commission_rate: number(),
  next_epoch_stake: number(),
  next_epoch_gas_price: number(),
  next_epoch_commission_rate: number(),
  staking_pool_id: string(),
  staking_pool_activation_epoch: nullable(number()),
  staking_pool_deactivation_epoch: nullable(number()),
  staking_pool_sui_balance: number(),
  rewards_pool: number(),
  pool_token_balance: number(),
  pending_delegation: number(),
  pending_pool_token_withdraw: number(),
  pending_total_sui_withdraw: number(),
  exchange_rates_id: string(),
  exchange_rates_size: number(),
});

export type SuiValidatorSummary = Infer<typeof SuiValidatorSummary>;

export const SuiSystemStateSummary = object({
  epoch: number(),
  protocol_version: number(),
  storage_fund: number(),
  reference_gas_price: number(),
  safe_mode: boolean(),
  epoch_start_timestamp_ms: number(),
  min_validator_stake: number(),
  max_validator_count: number(),
  governance_start_epoch: number(),
  stake_subsidy_epoch_counter: number(),
  stake_subsidy_balance: number(),
  stake_subsidy_current_epoch_amount: number(),
  total_stake: number(),
  active_validators: array(SuiValidatorSummary),
  pending_active_validators_id: string(),
  pending_active_validators_size: number(),
  pending_removals: array(number()),
  staking_pool_mappings_id: string(),
  staking_pool_mappings_size: number(),
  inactive_pools_id: string(),
  inactive_pools_size: number(),
  validator_candidates_id: string(),
  validator_candidates_size: number(),
  validator_report_records: array(tuple([SuiAddress, array(SuiAddress)])),
});

export type SuiSystemStateSummary = Infer<typeof SuiSystemStateSummary>;
