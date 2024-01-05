// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { SUI_TYPE_ARG } from '@mysten/sui.js';
import { Link } from 'react-router-dom';

import { CoinItem } from './CoinItem';
import { useActiveAddress } from '_app/hooks/useActiveAddress';
import { Text } from '_app/shared/text';
import Loading from '_components/loading';
import { useGetAllBalances } from '_hooks';

export function ActiveCoinsCard({
    activeCoinType = SUI_TYPE_ARG,
    showActiveCoin = true,
}: {
    activeCoinType: string;
    showActiveCoin?: boolean;
}) {
    const selectedAddress = useActiveAddress();
    const { data: coins, isLoading } = useGetAllBalances(selectedAddress!);

    const activeCoin = coins?.find(
        ({ coinType }) => coinType === activeCoinType
    );

    return (
        <Loading loading={isLoading}>
            <div className="flex w-full">
                {showActiveCoin ? (
                    activeCoin && (
                        <Link
                            to={`/send/select?${new URLSearchParams({
                                type: activeCoin.coinType,
                            }).toString()}`}
                            className="bg-gray-40 rounded-2lg py-2.5 px-3 no-underline flex gap-2 items-center w-full"
                        >
                            <CoinItem
                                coinType={activeCoin.coinType}
                                balance={BigInt(activeCoin.totalBalance)}
                                isActive
                            />
                        </Link>
                    )
                ) : (
                    <div className="flex flex-col w-full">
                        <Text
                            variant="caption"
                            color="steel-darker"
                            weight="semibold"
                        >
                            My Coins
                        </Text>
                        <div className="flex flex-col justify-between items-center mt-2">
                            {coins?.map(({ coinType, totalBalance }) => (
                                <Link
                                    to={`/send?${new URLSearchParams({
                                        type: coinType,
                                    }).toString()}`}
                                    key={coinType}
                                    className="py-3.75 px-1.5 no-underline flex gap-2 items-center w-full hover:bg-sui/10 group border-t border-solid border-transparent border-t-gray-45"
                                >
                                    <CoinItem
                                        coinType={coinType}
                                        balance={BigInt(totalBalance)}
                                    />
                                </Link>
                            ))}
                        </div>
                    </div>
                )}
            </div>
        </Loading>
    );
}
