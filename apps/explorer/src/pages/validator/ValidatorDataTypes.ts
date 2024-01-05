// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//TODO pull from the SDK

export const VALIDATORS_OBJECT_ID = '0x5';
export const VALDIATOR_NAME = /^[A-Z-_.\s0-9]+$/i;

export interface Validators {
    dataType: string;
    type: string;
    has_public_transfer: boolean;
    fields: ValidatorsFields;
}

export interface ValidatorsFields {
    chain_id: number;
    epoch: string;
    id: ID;
    parameters: Parameters;
    reference_gas_price: string;
    stake_subsidy: StakeSubsidy;
    storage_fund: string;
    sui_supply: Supply;
    validator_report_records: ValidatorReportRecords;
    validators: ValidatorsClass;
}

export interface ID {
    id: string;
}

export interface Parameters {
    type: string;
    fields: ParametersFields;
}

export interface ParametersFields {
    max_validator_candidate_count: string;
    min_validator_stake: string;
    storage_gas_price: string;
}

export interface StakeSubsidy {
    type: string;
    fields: StakeSubsidyFields;
}

export interface StakeSubsidyFields {
    balance: string;
    current_epoch_amount: string;
    epoch_counter: string;
}

export interface Supply {
    type: string;
    fields: SuiSupplyFields;
}

export interface SuiSupplyFields {
    value: string;
}

export interface ValidatorReportRecords {
    type: string;
    fields: ValidatorReportRecordsFields;
}

export interface ValidatorReportRecordsFields {
    contents: any[];
}

export interface ValidatorsClass {
    type: string;
    fields: ValidatorsFieldsClass;
}

export interface ValidatorsFieldsClass {
    active_validators: ActiveValidator[];
    next_epoch_validators: NextEpochValidator[];
    pending_delegation_switches: ValidatorReportRecords;
    pending_removals: number[];
    pending_validators: number[];
    quorum_stake_threshold: string;
    total_delegation_stake: string;
    total_validator_stake: string;
}

export interface ActiveValidator {
    type: string;
    fields: ActiveValidatorFields;
}

export interface ActiveValidatorFields {
    commission_rate: string;
    delegation_staking_pool: DelegationStakingPool;
    gas_price: string;
    metadata: NextEpochValidator;
    pending_stake: string;
    pending_withdraw: string;
    stake_amount: string;
}

export interface DelegationStakingPool {
    type: string;
    fields: DelegationStakingPoolFields;
}

export interface DelegationStakingPoolFields {
    delegation_token_supply: Supply;
    pending_delegations: Pending;
    pending_withdraws: Pending;
    rewards_pool: string;
    starting_epoch: string;
    sui_balance: string;
    validator_address: string;
}

export interface Pending {
    type: string;
    fields: PendingDelegationsFields;
}

export interface PendingDelegationsFields {
    contents: Contents;
}

export interface Contents {
    type: string;
    fields: ContentsFields;
}

export interface ContentsFields {
    id: ID;
    size: string;
}

export interface NextEpochValidator {
    type: string;
    fields: NextEpochValidatorFields;
}

export interface NextEpochValidatorFields {
    consensus_address: number[];
    name: number[];
    net_address: number[];
    network_pubkey_bytes: number[];
    next_epoch_commission_rate: string;
    next_epoch_delegation: string;
    next_epoch_gas_price: string;
    next_epoch_stake: string;
    proof_of_possession: number[];
    pubkey_bytes: number[];
    sui_address: string;
    worker_address: number[];
    worker_pubkey_bytes: number[];
}
