// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { useFormatCoin } from '@mysten/core';
import { CheckFill16 } from '@mysten/icons';
import { formatAddress, type SuiAddress, SUI_TYPE_ARG } from '@mysten/sui.js';
import cl from 'classnames';

import { useGetCoinBalance } from '../../hooks';
import { Text } from '_src/ui/app/shared/text';

type LedgerAccountRowProps = {
    isSelected: boolean;
    address: SuiAddress;
};

export function LedgerAccountRow({
    isSelected,
    address,
}: LedgerAccountRowProps) {
    const { data: coinBalance } = useGetCoinBalance(SUI_TYPE_ARG, address);
    const [totalAmount, totalAmountSymbol] = useFormatCoin(
        coinBalance?.totalBalance ?? 0,
        SUI_TYPE_ARG
    );

    return (
        <div className="flex items-center gap-3">
            <CheckFill16
                className={cl('w-4 h-4', {
                    'text-gray-50': !isSelected,
                    'text-success': isSelected,
                })}
            />
            <Text
                mono
                variant="bodySmall"
                weight="semibold"
                color={isSelected ? 'steel-darker' : 'steel-dark'}
            >
                {formatAddress(address)}
            </Text>
            <div className="ml-auto">
                <Text variant="bodySmall" color="steel" weight="semibold" mono>
                    {totalAmount} {totalAmountSymbol}
                </Text>
            </div>
        </div>
    );
}
