// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import {
    getTransactionDigest,
    SUI_TYPE_ARG,
    normalizeSuiAddress,
    type SuiAddress,
    type SuiMoveObject,
} from '@mysten/sui.js';
import { useQueryClient, useMutation } from '@tanstack/react-query';
import { Formik } from 'formik';
import { useCallback, useMemo, useState } from 'react';
import { Navigate, useNavigate, useSearchParams } from 'react-router-dom';

import { useGetDelegatedStake } from '../useGetDelegatedStake';
import { STATE_OBJECT } from '../usePendingDelegation';
import { DelegationState, STATE_TO_COPY } from './../home/DelegationCard';
import StakeForm from './StakeForm';
import { ValidatorFormDetail } from './ValidatorFormDetail';
import { createValidationSchema } from './validation';
import BottomMenuLayout, {
    Content,
    Menu,
} from '_app/shared/bottom-menu-layout';
import Button from '_app/shared/button';
import { Collapse } from '_app/shared/collapse';
import Icon, { SuiIcons } from '_components/icon';
import Loading from '_components/loading';
import LoadingIndicator from '_components/loading/LoadingIndicator';
import { parseAmount } from '_helpers';
import {
    useSigner,
    useAppSelector,
    useCoinDecimals,
    useIndividualCoinMaxBalance,
} from '_hooks';
import {
    accountAggregateBalancesSelector,
    accountItemizedBalancesSelector,
    ownedObjects,
} from '_redux/slices/account';
import { Coin, GAS_TYPE_ARG } from '_redux/slices/sui-objects/Coin';
import { Text } from '_src/ui/app/shared/text';

import type { SerializedError } from '@reduxjs/toolkit';
import type { FormikHelpers } from 'formik';

const initialValues = {
    amount: '',
};

export type FormValues = typeof initialValues;

function StakingCard() {
    const coinType = GAS_TYPE_ARG;
    const [sendError, setSendError] = useState<string | null>(null);
    const accountAddress = useAppSelector(({ account }) => account.address);
    const balances = useAppSelector(accountItemizedBalancesSelector);
    const aggregateBalances = useAppSelector(accountAggregateBalancesSelector);
    const [searchParams] = useSearchParams();
    const validatorAddress = searchParams.get('address');
    const stakeIdParams = searchParams.get('staked');
    const unstake = searchParams.get('unstake') === 'true';
    const { data: allDelegation, isLoading } = useGetDelegatedStake(
        accountAddress || ''
    );
    const totalGasCoins = useMemo(
        () => balances[GAS_TYPE_ARG]?.length || 0,
        [balances]
    );

    const gasAggregateBalance = useMemo(
        () => aggregateBalances[GAS_TYPE_ARG] || BigInt(0),
        [aggregateBalances]
    );
    const totalTokenBalance = useMemo(() => {
        if (!allDelegation) return BigInt(0);
        // return only the total amount of tokens staked for a specific stakeId
        if (stakeIdParams) {
            const balance =
                allDelegation.find(
                    ({ staked_sui }) => staked_sui.id.id === stakeIdParams
                )?.staked_sui.principal.value || 0;
            return BigInt(balance);
        }
        // return aggregate delegation
        return allDelegation.reduce(
            (acc, { staked_sui }) => acc + BigInt(staked_sui.principal.value),
            0n
        );
    }, [allDelegation, stakeIdParams]);

    // For requesting withdrawal of tokens the coinBalance is the total amount of tokens staked for the specific stakeId
    const coinBalance = useMemo(
        () =>
            unstake
                ? totalTokenBalance
                : (coinType && aggregateBalances[coinType]) || BigInt(0),
        [unstake, totalTokenBalance, coinType, aggregateBalances]
    );

    const delegationData = useMemo(() => {
        if (!allDelegation) return null;

        return allDelegation.find(
            ({ staked_sui }) => staked_sui.id.id === stakeIdParams
        );
    }, [allDelegation, stakeIdParams]);

    const coinSymbol = useMemo(
        () => (coinType && Coin.getCoinSymbol(coinType)) || '',
        [coinType]
    );

    const [coinDecimals] = useCoinDecimals(coinType);
    const [gasDecimals] = useCoinDecimals(GAS_TYPE_ARG);
    const maxSuiSingleCoinBalance = useIndividualCoinMaxBalance(SUI_TYPE_ARG);

    const validationSchema = useMemo(
        () =>
            createValidationSchema(
                coinType || '',
                coinBalance,
                coinSymbol,
                gasAggregateBalance,
                totalGasCoins,
                coinDecimals,
                gasDecimals,
                maxSuiSingleCoinBalance
            ),
        [
            coinType,
            coinBalance,
            coinSymbol,
            gasAggregateBalance,
            totalGasCoins,
            coinDecimals,
            gasDecimals,
            maxSuiSingleCoinBalance,
        ]
    );

    const queryClient = useQueryClient();
    const delegationId = useMemo(() => {
        if (!delegationData || delegationData.delegation_status === 'Pending')
            return null;
        return delegationData.delegation_status.Active.id.id;
    }, [delegationData]);

    const navigate = useNavigate();
    const signer = useSigner();
    const allCoins = useAppSelector(ownedObjects);
    const stakeToken = useMutation({
        mutationFn: async ({
            tokenTypeArg,
            amount,
            validatorAddress,
        }: {
            tokenTypeArg: string;
            amount: bigint;
            validatorAddress: SuiAddress;
        }) => {
            if (!validatorAddress || !amount || !tokenTypeArg) {
                throw new Error('Failed, missing required field');
            }

            const coinType = Coin.getCoinTypeFromArg(tokenTypeArg);
            const coins: SuiMoveObject[] = allCoins
                .filter(
                    (anObj) =>
                        anObj.data.dataType === 'moveObject' &&
                        anObj.data.type === coinType
                )
                .map(({ data }) => data as SuiMoveObject);

            const response = Coin.stakeCoin(
                signer,
                coins,
                amount,
                validatorAddress
            );
            return response;
        },
    });
    const unStakeToken = useMutation({
        mutationFn: async ({
            principalWithdrawAmount,
            delegationId,
            stakeSuId,
        }: {
            principalWithdrawAmount: string;
            delegationId: string;
            stakeSuId: string;
        }) => {
            if (!principalWithdrawAmount || !delegationId || !stakeSuId) {
                throw new Error(
                    'Failed, missing required field (!principalWithdrawAmount | delegationId | stakeSuId).'
                );
            }

            const response = await Coin.unStakeCoin(
                signer,
                delegationId,
                stakeSuId,
                principalWithdrawAmount
            );
            return response;
        },
    });

    const onHandleSubmit = useCallback(
        async (
            { amount }: FormValues,
            { resetForm }: FormikHelpers<FormValues>
        ) => {
            if (coinType === null || validatorAddress === null) {
                return;
            }
            setSendError(null);
            try {
                const bigIntAmount = parseAmount(amount, coinDecimals);
                let response;
                let txDigest;
                if (unstake) {
                    // check for delegation data
                    if (
                        !delegationData ||
                        !stakeIdParams ||
                        delegationData.delegation_status === 'Pending'
                    ) {
                        return;
                    }
                    response = await unStakeToken.mutateAsync({
                        delegationId:
                            delegationData.delegation_status.Active.id.id,
                        stakeSuId: stakeIdParams,
                        principalWithdrawAmount: bigIntAmount.toString(),
                    });

                    txDigest = getTransactionDigest(response);
                } else {
                    response = await stakeToken.mutateAsync({
                        amount: bigIntAmount,
                        tokenTypeArg: coinType,
                        validatorAddress: validatorAddress,
                    });
                    txDigest = getTransactionDigest(response);
                }

                //  invalidate the react query for 0x5 and validator
                Promise.all([
                    queryClient.invalidateQueries({
                        queryKey: ['object', normalizeSuiAddress(STATE_OBJECT)],
                    }),
                    queryClient.invalidateQueries({
                        queryKey: ['validator'],
                    }),
                ]);
                resetForm();

                navigate(
                    `/receipt?${new URLSearchParams({
                        txdigest: txDigest,
                    }).toString()}`
                );
            } catch (e) {
                setSendError((e as SerializedError).message || null);
            }
        },
        [
            coinType,
            validatorAddress,
            coinDecimals,
            unstake,
            queryClient,
            navigate,
            delegationData,
            stakeIdParams,
            unStakeToken,
            stakeToken,
        ]
    );

    const handleOnClearSubmitError = useCallback(() => {
        setSendError(null);
    }, []);
    const loadingBalance = useAppSelector(
        ({ suiObjects }) => suiObjects.loading && !suiObjects.lastSync
    );

    if (!coinType || !validatorAddress) {
        return <Navigate to="/" replace={true} />;
    }

    return (
        <div className="flex flex-col flex-nowrap flex-grow w-full">
            <Loading
                loading={loadingBalance || isLoading}
                className="flex justify-center w-full h-full items-center "
            >
                <Formik
                    initialValues={initialValues}
                    validateOnMount
                    validationSchema={validationSchema}
                    onSubmit={onHandleSubmit}
                >
                    {({ isSubmitting, isValid, submitForm }) => (
                        <BottomMenuLayout>
                            <Content>
                                <ValidatorFormDetail
                                    validatorAddress={validatorAddress}
                                    unstake={unstake}
                                    stakedId={stakeIdParams}
                                />
                                <div className="flex flex-col justify-between items-center mb-2 mt-6 w-full">
                                    <Text
                                        variant="caption"
                                        color="gray-85"
                                        weight="semibold"
                                    >
                                        {unstake
                                            ? 'Enter the amount of SUI to unstake'
                                            : 'Enter the amount of SUI to stake'}
                                    </Text>
                                </div>
                                <StakeForm
                                    submitError={sendError}
                                    coinBalance={coinBalance}
                                    coinType={coinType}
                                    unstake={unstake}
                                    onClearSubmitError={
                                        handleOnClearSubmitError
                                    }
                                />

                                {stakeIdParams && (
                                    <div className="flex-1 mt-7.5">
                                        <Collapse
                                            title={
                                                STATE_TO_COPY[
                                                    delegationData?.delegation_status ===
                                                    'Pending'
                                                        ? DelegationState.WARM_UP
                                                        : DelegationState.EARNING
                                                ]
                                            }
                                            initialIsOpen
                                        >
                                            --
                                        </Collapse>
                                    </div>
                                )}
                            </Content>

                            <Menu
                                stuckClass="staked-cta"
                                className="w-full px-0 pb-0 mx-0"
                            >
                                <Button
                                    size="large"
                                    mode="neutral"
                                    href="/stake"
                                    disabled={isSubmitting}
                                    className="!text-steel-darker w-1/2"
                                >
                                    <Icon
                                        icon={SuiIcons.ArrowLeft}
                                        className="text-body text-gray-65 font-normal"
                                    />
                                    Back
                                </Button>
                                <Button
                                    size="large"
                                    mode="primary"
                                    onClick={submitForm}
                                    className=" w-1/2"
                                    disabled={
                                        !isValid ||
                                        isSubmitting ||
                                        (unstake && !delegationId)
                                    }
                                >
                                    {isSubmitting ? (
                                        <LoadingIndicator className="border-white" />
                                    ) : unstake ? (
                                        'Unstake Now'
                                    ) : (
                                        'Stake Now'
                                    )}
                                </Button>
                            </Menu>
                        </BottomMenuLayout>
                    )}
                </Formik>
            </Loading>
        </div>
    );
}

export default StakingCard;
