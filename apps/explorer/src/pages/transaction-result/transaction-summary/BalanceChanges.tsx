// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import {
    type BalanceChangeSummary,
    CoinFormat,
    useFormatCoin,
} from '@mysten/core';

import { CoinsStack } from '~/ui/CoinsStack';
import { Heading } from '~/ui/Heading';
import { AddressLink } from '~/ui/InternalLink';
import { Text } from '~/ui/Text';
import {
    TransactionBlockCard,
    TransactionBlockCardSection,
} from '~/ui/TransactionBlockCard';

interface BalanceChangesProps {
    changes: BalanceChangeSummary[] | null;
}

function BalanceChangeEntry({ change }: { change: BalanceChangeSummary }) {
    const { amount, coinType, recipient } = change;

    const [formatted, symbol] = useFormatCoin(
        amount,
        coinType,
        CoinFormat.FULL
    );

    const isPositive = BigInt(amount) > 0n;

    if (!change) {
        return null;
    }

    return (
        <div className="flex flex-col gap-2 py-3 first:pt-0 only:pb-0 only:pt-0">
            <div className="flex flex-col gap-2">
                <div className="flex justify-between">
                    <Text variant="pBody/medium" color="steel-dark">
                        Amount
                    </Text>
                    <div className="flex">
                        <Text
                            variant="pBody/medium"
                            color={isPositive ? 'success-dark' : 'issue-dark'}
                        >
                            {isPositive ? '+' : ''}
                            {formatted} {symbol}
                        </Text>
                    </div>
                </div>

                {recipient && (
                    <div className="flex justify-between">
                        <Text variant="pBody/medium" color="steel-dark">
                            Recipient
                        </Text>
                        {recipient}
                    </div>
                )}
            </div>
        </div>
    );
}

export function BalanceChanges({ changes }: BalanceChangesProps) {
    if (!changes?.length) return null;

    const owner = changes[0]?.owner ?? '';

    const coinTypesSet = new Set(changes.map((change) => change.coinType));

    return (
        <TransactionBlockCard
            title={
                <div className="flex w-full items-center justify-between">
                    <Heading variant="heading6/semibold" color="steel-darker">
                        Balance Changes
                    </Heading>

                    <CoinsStack coinTypes={Array.from(coinTypesSet)} />
                </div>
            }
            shadow
            size="sm"
            footer={
                owner ? (
                    <div className="flex justify-between">
                        <Text variant="pBody/medium" color="steel-dark">
                            Owner
                        </Text>
                        <Text variant="pBody/medium" color="hero-dark">
                            <AddressLink address={owner} />
                        </Text>
                    </div>
                ) : null
            }
        >
            <div className="flex flex-col gap-4">
                {changes.map((change, index) => (
                    <TransactionBlockCardSection key={index}>
                        <BalanceChangeEntry change={change} />
                    </TransactionBlockCardSection>
                ))}
            </div>
        </TransactionBlockCard>
    );
}
