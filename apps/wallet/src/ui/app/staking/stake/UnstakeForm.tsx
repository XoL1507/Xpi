// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { useFormatCoin } from '@mysten/core';
import { SUI_TYPE_ARG } from '@mysten/sui.js';
import { Form, useFormikContext } from 'formik';
import { useEffect } from 'react';

import LoadingIndicator from '../../components/loading/LoadingIndicator';
import { useGasBudgetInMist } from '../../hooks/useGasBudgetInMist';
import { Heading } from '../../shared/heading';
import { useGetTimeBeforeEpochNumber } from '../useGetTimeBeforeEpochNumber';
import { Card } from '_app/shared/card';
import { Text } from '_app/shared/text';
import { DEFAULT_GAS_BUDGET_FOR_STAKE } from '_redux/slices/sui-objects/Coin';
import { CountDownTimer } from '_src/ui/app/shared/countdown-timer';

import type { FormValues } from './StakingCard';

export type StakeFromProps = {
    coinBalance: bigint;
    coinType: string;
    stakingReward?: number;
    epoch: number;
};

export function UnStakeForm({
    coinBalance,
    coinType,
    stakingReward,
    epoch,
}: StakeFromProps) {
    const { setFieldValue } = useFormikContext<FormValues>();
    const { gasBudget, isLoading } = useGasBudgetInMist(
        DEFAULT_GAS_BUDGET_FOR_STAKE
    );
    const [gasBudgetFormatted, symbol] = useFormatCoin(gasBudget, SUI_TYPE_ARG);
    const [rewards, rewardSymbol] = useFormatCoin(stakingReward, SUI_TYPE_ARG);
    const [totalSui] = useFormatCoin(
        BigInt(stakingReward || 0) + coinBalance,
        SUI_TYPE_ARG
    );
    const [tokenBalance] = useFormatCoin(coinBalance, coinType);

    const { data: currentEpochEndTime } = useGetTimeBeforeEpochNumber(
        epoch + 1 || 0
    );

    useEffect(() => {
        setFieldValue(
            'gasBudget',
            isLoading ? '' : (gasBudget || 0).toString(),
            true
        );
    }, [setFieldValue, gasBudget, isLoading]);

    return (
        <Form
            className="flex flex-1 flex-col flex-nowrap"
            autoComplete="off"
            noValidate
        >
            <Card
                titleDivider
                header={
                    <div className="px-4 py-3 w-full flex bg-white justify-between">
                        <Text
                            variant="body"
                            weight="medium"
                            color="steel-darker"
                        >
                            Current Epoch Ends
                        </Text>
                        <div className="flex gap-0.5 ml-auto">
                            {currentEpochEndTime > 0 ? (
                                <CountDownTimer
                                    timestamp={currentEpochEndTime}
                                    variant="body"
                                    color="steel-dark"
                                    weight="medium"
                                    endLabel="--"
                                />
                            ) : (
                                <Text
                                    variant="body"
                                    weight="medium"
                                    color="steel-dark"
                                >
                                    Epoch #{epoch}
                                </Text>
                            )}
                        </div>
                    </div>
                }
                footer={
                    <div className="flex gap-0.5 justify-between w-full">
                        <Text variant="p2" weight="medium" color="steel-darker">
                            Total unstaked SUI
                        </Text>
                        <div className="flex gap-0.5 ml-auto">
                            <Heading
                                variant="heading4"
                                weight="semibold"
                                color="steel-darker"
                                leading="none"
                            >
                                {totalSui}
                            </Heading>
                            <Text
                                variant="bodySmall"
                                weight="medium"
                                color="steel-dark"
                            >
                                {symbol}
                            </Text>
                        </div>
                    </div>
                }
            >
                <div className="pb-3.75 flex flex-col  w-full gap-2">
                    <div className="flex gap-0.5 justify-between w-full">
                        <Text
                            variant="body"
                            weight="medium"
                            color="steel-darker"
                        >
                            Your Stake
                        </Text>
                        <Text
                            variant="body"
                            weight="medium"
                            color="steel-darker"
                        >
                            {tokenBalance} {symbol}
                        </Text>
                    </div>
                    <div className="flex gap-0.5 justify-between w-full">
                        <Text
                            variant="body"
                            weight="medium"
                            color="steel-darker"
                        >
                            Staking Rewards Earned
                        </Text>
                        <Text
                            variant="body"
                            weight="medium"
                            color="steel-darker"
                        >
                            {rewards} {rewardSymbol}
                        </Text>
                    </div>
                </div>
            </Card>
            <div className="mt-4">
                <Card variant="gray">
                    <div className=" w-full flex justify-between">
                        <Text
                            variant="body"
                            weight="medium"
                            color="steel-darker"
                        >
                            Gas Fees
                        </Text>

                        <Text variant="body" weight="medium" color="steel-dark">
                            {isLoading ? (
                                <LoadingIndicator />
                            ) : (
                                `${gasBudgetFormatted} ${symbol}`
                            )}
                        </Text>
                    </div>
                </Card>
            </div>
        </Form>
    );
}
