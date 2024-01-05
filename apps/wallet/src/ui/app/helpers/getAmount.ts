// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import {
    getPaySuiTransaction,
    getPayTransaction,
    getTransferSuiTransaction,
    getTransferObjectTransaction,
    getTransactionKindName,
    SUI_TYPE_ARG,
    is,
    CoinBalanceChangeEvent,
} from '@mysten/sui.js';

import type { SuiTransactionKind, TransactionEffects } from '@mysten/sui.js';

const getCoinType = (
    txEffects: TransactionEffects,
    address: string
): string | null => {
    const events = txEffects?.events || [];
    const coinEvent = events.find((event) => {
        if (
            'coinBalanceChange' in event &&
            is(event.coinBalanceChange, CoinBalanceChangeEvent) &&
            ['Receive', 'Pay'].includes(event.coinBalanceChange.changeType) &&
            event.coinBalanceChange.transactionModule !== 'gas'
        ) {
            const { owner, sender } = event.coinBalanceChange;
            const { AddressOwner } = owner as { AddressOwner: string };
            return AddressOwner === address || address === sender;
        }
        return false;
    }) as { coinBalanceChange: CoinBalanceChangeEvent } | undefined;

    return coinEvent?.coinBalanceChange.coinType || null;
};

type FormattedBalance = {
    amount?: number | null;
    coinType?: string | null;
    recipientAddress: string;
}[];

export function getAmount(
    txnData: SuiTransactionKind,
    txnEffect: TransactionEffects
): FormattedBalance | null {
    const txKindName = getTransactionKindName(txnData);
    if (txKindName === 'TransferObject') {
        const txn = getTransferObjectTransaction(txnData);
        return txn?.recipient
            ? [
                  {
                      recipientAddress: txn?.recipient,
                  },
              ]
            : null;
    }

    if (txKindName === 'TransferSui') {
        const txn = getTransferSuiTransaction(txnData);
        return txn?.recipient
            ? [
                  {
                      recipientAddress: txn.recipient,
                      amount: txn?.amount,
                      coinType:
                          txnEffect && getCoinType(txnEffect, txn.recipient),
                  },
              ]
            : null;
    }

    const paySuiData =
        getPaySuiTransaction(txnData) ?? getPayTransaction(txnData);

    const amountByRecipient = paySuiData?.recipients.reduce(
        (acc, recipient, index) => {
            const coinType =
                txKindName === 'PaySui'
                    ? SUI_TYPE_ARG
                    : getCoinType(txnEffect, recipient);
            return {
                ...acc,
                [recipient]: {
                    amount:
                        paySuiData.amounts[index] +
                        (recipient in acc ? acc[recipient].amount : 0),
                    coinType,
                    recipientAddress: recipient,
                },
            };
        },
        {} as {
            [key: string]: {
                amount: number;
                coinType: string | null;
                recipientAddress: string;
            };
        }
    );

    return amountByRecipient ? Object.values(amountByRecipient) : null;
}
