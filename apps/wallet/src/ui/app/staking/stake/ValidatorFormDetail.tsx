// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
import { is, SuiObject, type ValidatorsFields } from '@mysten/sui.js';
import { useMemo } from 'react';

import { calculateAPY } from '../calculateAPY';
import { StakeAmount } from '../home/StakeAmount';
import { useGetDelegatedStake } from '../useGetDelegatedStake';
import { STATE_OBJECT } from '../usePendingDelegation';
import { ValidatorLogo } from '../validators/ValidatorLogo';
import { Card } from '_app/shared/card';
import Alert from '_components/alert';
import LoadingIndicator from '_components/loading/LoadingIndicator';
import { useGetObject, useAppSelector } from '_hooks';
import { Text } from '_src/ui/app/shared/text';
import { IconTooltip } from '_src/ui/app/shared/tooltip';

type ValidatorFormDetailProps = {
    validatorAddress: string;
    unstake?: boolean;
    stakedId?: string | null;
};

export function ValidatorFormDetail({
    validatorAddress,
    unstake,
    stakedId,
}: ValidatorFormDetailProps) {
    const accountAddress = useAppSelector(({ account }) => account.address);
    const {
        data: validatetors,
        isLoading: loadingValidators,
        isError: errorValidators,
    } = useGetObject(STATE_OBJECT);

    const {
        data: allDelegation,
        isLoading,
        isError,
        error,
    } = useGetDelegatedStake(accountAddress || '');

    const validatorsData =
        validatetors &&
        is(validatetors.details, SuiObject) &&
        validatetors.details.data.dataType === 'moveObject'
            ? (validatetors.details.data.fields as ValidatorsFields)
            : null;

    const delegationData = useMemo(() => {
        if (!allDelegation) return null;

        return allDelegation.find(
            ({ staked_sui }) => staked_sui.id.id === stakedId
        );
    }, [allDelegation, stakedId]);

    const totalSuiStake = delegationData?.staked_sui.principal.value || 0n;

    const validatorData = useMemo(() => {
        if (!validatorsData) return null;
        return validatorsData.validators.fields.active_validators.find(
            (av) => av.fields.metadata.fields.sui_address === validatorAddress
        );
    }, [validatorAddress, validatorsData]);

    const totalValidatorStake = validatorData?.fields.stake_amount || 0;

    const totalStake = useMemo(() => {
        if (!allDelegation) return 0n;
        let totalActiveStake = 0n;
        allDelegation.forEach((event) => {
            if (event.staked_sui.validator_address === validatorAddress) {
                totalActiveStake += BigInt(event.staked_sui.principal.value);
            }
        });
        return totalActiveStake;
    }, [allDelegation, validatorAddress]);

    const apy = useMemo(() => {
        if (!validatorData || !validatorsData) return 0;
        return calculateAPY(validatorData, +validatorsData.epoch);
    }, [validatorData, validatorsData]);

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
                        {error?.message ?? 'Error loading validator data'}
                    </div>
                </Alert>
            </div>
        );
    }

    return (
        <div className="w-full">
            {validatorData && (
                <Card
                    titleDivider
                    header={
                        <div className="flex py-2.5 gap-2 items-center">
                            <ValidatorLogo
                                validatorAddress={validatorAddress}
                                iconSize="sm"
                                size="body"
                            />
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

                            <StakeAmount balance={totalStake} variant="body" />
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
                                {apy > 0 ? `${apy}%` : '--'}
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
                                <StakeAmount
                                    balance={
                                        stakedId
                                            ? totalSuiStake
                                            : totalValidatorStake
                                    }
                                    variant="body"
                                />
                            </div>
                        )}
                    </div>
                </Card>
            )}
        </div>
    );
}
