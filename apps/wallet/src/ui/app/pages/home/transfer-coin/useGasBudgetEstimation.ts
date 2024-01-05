// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { Coin, SUI_TYPE_ARG } from '@mysten/sui.js';
import { useQuery } from '@tanstack/react-query';
import { useMemo } from 'react';

import { useAppSelector, useIndividualCoinMaxBalance, useSigner } from '_hooks';
import {
    accountAggregateBalancesSelector,
    accountCoinsSelector,
} from '_redux/slices/account';

export function useGasBudgetEstimation(
    coinTypeArg: string | null,
    amountToSend: bigint
) {
    const signer = useSigner();
    const suiCoinMaxBalance = useIndividualCoinMaxBalance(SUI_TYPE_ARG);
    const allCoins = useAppSelector(accountCoinsSelector);
    const balancePerType = useAppSelector(accountAggregateBalancesSelector);
    const balance = useMemo(
        () => (coinTypeArg && balancePerType[coinTypeArg]) || BigInt(0),
        [coinTypeArg, balancePerType]
    );
    const enabled = amountToSend > 0 && !!coinTypeArg;
    const estimationResult = useQuery({
        queryKey: [
            'gas-estimation',
            'nft-transfer',
            coinTypeArg,
            suiCoinMaxBalance.toString(),
            amountToSend.toString(),
            balance.toString(),
        ],
        queryFn: async () => {
            return await Coin.getGasCostEstimationAndSuggestedBudget(
                signer,
                allCoins,
                coinTypeArg || '',
                amountToSend,
                await signer.getAddress(), // any address will have the same gas cost
                suiCoinMaxBalance
            );
        },
        enabled,
    });
    return [
        estimationResult.data?.suggestedGasBudget ?? null,
        estimationResult.data?.gasCostEstimation ?? null,
        estimationResult.isLoading,
        suiCoinMaxBalance,
        estimationResult.data?.insufficientGas ?? false,
    ] as const;
}
