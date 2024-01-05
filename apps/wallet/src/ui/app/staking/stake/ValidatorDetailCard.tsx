// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
import { isSuiObject, isSuiMoveObject, SUI_TYPE_ARG } from '@mysten/sui.js';
import { useMemo } from 'react';

import { getName, STATE_OBJECT } from '../usePendingDelegation';
import { Card } from '_app/shared/card';
import CoinBalance from '_app/shared/coin-balance';
import { ImageIcon } from '_app/shared/image-icon';
import Alert from '_components/alert';
import LoadingIndicator from '_components/loading/LoadingIndicator';
import { useGetObject, useAppSelector } from '_hooks';
import { GAS_TYPE_ARG } from '_redux/slices/sui-objects/Coin';
import { Text } from '_src/ui/app/shared/text';
import { IconTooltip } from '_src/ui/app/shared/tooltip';

import type { ValidatorState } from '../ValidatorDataTypes';

export function ValidateDetailFormCard({
    validatorAddress,
    unstake,
}: {
    validatorAddress: string;
    unstake?: boolean;
}) {
    const accountAddress = useAppSelector(({ account }) => account.address);
    const { data, isLoading, isError } = useGetObject(STATE_OBJECT);

    const validatorsData =
        data && isSuiObject(data.details) && isSuiMoveObject(data.details.data)
            ? (data.details.data.fields as ValidatorState)
            : null;

    const validatorByAddress = useMemo(() => {
        if (!validatorsData) return null;
        return (
            validatorsData.validators.fields.active_validators.find(
                (av) =>
                    av.fields.metadata.fields.sui_address === validatorAddress
            ) || null
        );
    }, [validatorAddress, validatorsData]);

    const validatorData = useMemo(() => {
        if (!validatorByAddress || !validatorsData) return null;

        const {
            sui_balance,
            starting_epoch,
            pending_delegations,
            delegation_token_supply,
        } = validatorByAddress.fields.delegation_staking_pool.fields;

        const num_epochs_participated = validatorsData.epoch - starting_epoch;
        const { name: rawName, sui_address } =
            validatorByAddress.fields.metadata.fields;

        const APY = Math.pow(
            1 +
                (sui_balance - delegation_token_supply.fields.value) /
                    delegation_token_supply.fields.value,
            365 / num_epochs_participated - 1
        );
        const pending_delegationsByAddress = pending_delegations
            ? pending_delegations.filter(
                  (d) => d.fields.delegator === accountAddress
              )
            : [];

        return {
            name: getName(rawName),
            apy: APY > 0 ? APY : 'N/A',
            logo: null,
            address: sui_address,
            totalStaked: pending_delegations.reduce(
                (acc, fields) =>
                    (acc += BigInt(fields.fields.sui_amount || 0n)),
                0n
            ),
            pendingDelegationAmount: pending_delegationsByAddress.reduce(
                (acc, fields) =>
                    (acc += BigInt(fields.fields.sui_amount || 0n)),
                0n
            ),
        };
    }, [accountAddress, validatorByAddress, validatorsData]);

    if (isLoading) {
        return (
            <div className="p-2 w-full flex justify-center item-center h-full">
                <LoadingIndicator />
            </div>
        );
    }

    if (isError) {
        return (
            <div className="p-2">
                <Alert mode="warning">
                    <div className="mb-1 font-semibold">
                        Something went wrong
                    </div>
                </Alert>
            </div>
        );
    }

    return (
        <div className="w-full">
            {validatorData && (
                <Card
                    header={
                        <div className="flex py-2.5 gap-2 items-center">
                            <ImageIcon
                                src={validatorData.logo}
                                alt={validatorData.name}
                                size="small"
                                variant="circle"
                            />
                            <Text variant="body" weight="semibold">
                                {validatorData.name}
                            </Text>
                        </div>
                    }
                    footer={
                        <>
                            <Text
                                variant="body"
                                weight="medium"
                                color="steel-darker"
                            >
                                Your Staked SUI
                            </Text>

                            <CoinBalance
                                balance={validatorData.pendingDelegationAmount}
                                className="text-body steel-darker"
                                type={SUI_TYPE_ARG}
                                diffSymbol
                            />
                        </>
                    }
                >
                    <div className="divide-x flex divide-solid divide-gray-45 divide-y-0 flex-col gap-3.5 mb-3.5">
                        <div className="flex gap-2 items-center justify-between ">
                            <div className="flex gap-1 items-baseline text-steel">
                                <Text
                                    variant="body"
                                    weight="medium"
                                    color="steel-darker"
                                >
                                    Staking APY
                                </Text>
                                <IconTooltip tip="Annual Percentage Yield" />
                            </div>

                            <Text
                                variant="body"
                                weight="semibold"
                                color="gray-90"
                            >
                                {validatorData.apy}{' '}
                                {typeof validatorData.apy !== 'string' && '%'}
                            </Text>
                        </div>
                        {!unstake && (
                            <div className="flex gap-2 items-center justify-between">
                                <div className="flex gap-1 items-baseline text-steel">
                                    <Text
                                        variant="body"
                                        weight="medium"
                                        color="steel-darker"
                                    >
                                        Total Staked
                                    </Text>
                                </div>

                                <CoinBalance
                                    balance={validatorData.totalStaked}
                                    className="text-body font-medium steel-darker"
                                    type={GAS_TYPE_ARG}
                                    diffSymbol
                                />
                            </div>
                        )}
                    </div>
                </Card>
            )}
        </div>
    );
}
