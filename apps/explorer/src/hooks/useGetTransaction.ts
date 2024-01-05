// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { useRpcClient } from '@mysten/core';
import { useQuery } from '@tanstack/react-query';

export function useGetTransaction(transactionId: string) {
    const rpc = useRpcClient();
    return useQuery(
        ['transactions-by-id', transactionId],
        async () =>
            rpc.getTransaction({
                digest: transactionId,
                options: {
                    showInput: true,
                    showEffects: true,
                    showEvents: true,
                    showObjectChanges: true,
                    showBalanceChanges: true,
                },
            }),
        { enabled: !!transactionId }
    );
}
