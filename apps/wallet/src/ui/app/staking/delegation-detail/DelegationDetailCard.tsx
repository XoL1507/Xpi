// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { useFeature } from '@growthbook/growthbook-react';
import { useGetRollingAverageApys } from '@mysten/core';
import { ArrowLeft16, StakeAdd16, StakeRemove16 } from '@mysten/icons';
import { useMemo } from 'react';

import { useActiveAddress } from '../../hooks/useActiveAddress';
import { Heading } from '../../shared/heading';
import { getDelegationDataByStakeId } from '../getDelegationByStakeId';
import { StakeAmount } from '../home/StakeAmount';
import { useGetDelegatedStake } from '../useGetDelegatedStake';
import { useSystemState } from '../useSystemState';
import { Button } from '_app/shared/ButtonUI';
import BottomMenuLayout, { Content } from '_app/shared/bottom-menu-layout';
import { Card } from '_app/shared/card';
import { CardItem } from '_app/shared/card/CardItem';
import { Text } from '_app/shared/text';
import { IconTooltip } from '_app/shared/tooltip';
import Alert from '_components/alert';
import LoadingIndicator from '_components/loading/LoadingIndicator';
import { FEATURES } from '_src/shared/experimentation/features';

type DelegationDetailCardProps = {
    validatorAddress: string;
    stakedId: string;
};

export function DelegationDetailCard({
    validatorAddress,
    stakedId,
}: DelegationDetailCardProps) {
    const {
        data: system,
        isLoading: loadingValidators,
        isError: errorValidators,
    } = useSystemState();

    const accountAddress = useActiveAddress();

    const {
        data: allDelegation,
        isLoading,
        isError,
    } = useGetDelegatedStake(accountAddress || '');

    const { data: rollingAverageApys } = useGetRollingAverageApys(
        system?.activeValidators.length || null
    );

    const validatorData = useMemo(() => {
        if (!system) return null;
        return system.activeValidators.find(
            (av) => av.suiAddress === validatorAddress
        );
    }, [validatorAddress, system]);

    const delegationData = useMemo(() => {
        return allDelegation
            ? getDelegationDataByStakeId(allDelegation, stakedId)
            : null;
    }, [allDelegation, stakedId]);

    const totalStake = BigInt(delegationData?.principal || 0n);

    const suiEarned = BigInt(delegationData?.estimatedReward || 0n);

    const apy = rollingAverageApys?.[validatorAddress] || 0;

    const delegationId =
        delegationData?.status === 'Active' && delegationData?.stakedSuiId;

    const stakeByValidatorAddress = `/stake/new?${new URLSearchParams({
        address: validatorAddress,
        staked: stakedId,
    }).toString()}`;

    const commission = validatorData ? +validatorData.commissionRate / 100 : 0;
    const stakingEnabled = useFeature(FEATURES.STAKING_ENABLED).on;

    if (isLoading || loadingValidators) {
        return (
            <div className="p-2 w-full flex justify-center items-center h-full">
                <LoadingIndicator />
            </div>
        );
    }

    if (isError || errorValidators) {
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
        <div className="flex flex-col flex-nowrap flex-grow h-full">
            <BottomMenuLayout>
                <Content>
                    <div className="justify-center w-full flex flex-col items-center">
                        <div className="w-full flex">
                            <Card
                                header={
                                    <div className="grid grid-cols-2 divide-x divide-solid divide-gray-45 divide-y-0 w-full">
                                        <CardItem title="Your Stake">
                                            <StakeAmount
                                                balance={totalStake}
                                                variant="heading5"
                                            />
                                        </CardItem>

                                        <CardItem title="Earned">
                                            <StakeAmount
                                                balance={suiEarned}
                                                variant="heading5"
                                                isEarnedRewards
                                            />
                                        </CardItem>
                                    </div>
                                }
                                padding="none"
                            >
                                <div className="divide-x flex divide-solid divide-gray-45 divide-y-0">
                                    <CardItem
                                        title={
                                            <div className="flex text-steel-darker gap-1 items-start">
                                                APY
                                                <div className="text-steel">
                                                    <IconTooltip
                                                        tip="Annual Percentage Yield"
                                                        placement="top"
                                                    />
                                                </div>
                                            </div>
                                        }
                                    >
                                        <div className="flex gap-0.5 items-baseline">
                                            <Heading
                                                variant="heading4"
                                                weight="semibold"
                                                color="gray-90"
                                                leading="none"
                                            >
                                                {apy}
                                            </Heading>

                                            <Text
                                                variant="subtitleSmall"
                                                weight="medium"
                                                color="steel-dark"
                                            >
                                                %
                                            </Text>
                                        </div>
                                    </CardItem>

                                    <CardItem
                                        title={
                                            <div className="flex text-steel-darker gap-1">
                                                Commission
                                                <div className="text-steel">
                                                    <IconTooltip
                                                        tip="Validator commission"
                                                        placement="top"
                                                    />
                                                </div>
                                            </div>
                                        }
                                    >
                                        <div className="flex gap-0.5 items-baseline">
                                            <Heading
                                                variant="heading4"
                                                weight="semibold"
                                                color="gray-90"
                                                leading="none"
                                            >
                                                {commission}
                                            </Heading>

                                            <Text
                                                variant="subtitleSmall"
                                                weight="medium"
                                                color="steel-dark"
                                            >
                                                %
                                            </Text>
                                        </div>
                                    </CardItem>
                                </div>
                            </Card>
                        </div>
                        <div className="flex gap-2.5 w-full my-3.75">
                            <Button
                                size="tall"
                                variant="outline"
                                to={stakeByValidatorAddress}
                                before={<StakeAdd16 />}
                                text="Stake SUI"
                                disabled={!stakingEnabled}
                            />

                            {Boolean(totalStake) && delegationId && (
                                <Button
                                    size="tall"
                                    variant="outline"
                                    to={
                                        stakeByValidatorAddress +
                                        '&unstake=true'
                                    }
                                    text="Unstake SUI"
                                    before={<StakeRemove16 />}
                                />
                            )}
                        </div>
                    </div>
                </Content>
                <Button
                    size="tall"
                    variant="secondary"
                    to="/stake"
                    before={<ArrowLeft16 />}
                    text="Back"
                />
            </BottomMenuLayout>
        </div>
    );
}
