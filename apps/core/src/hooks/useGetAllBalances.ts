// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { useRpcClient } from '../api/RpcClientContext';
import { type SuiAddress, CoinBalance } from '@mysten/sui.js';
import { useQuery } from '@tanstack/react-query';

export function useGetAllBalances<TResult = CoinBalance[]>(
	address?: SuiAddress | null,
	refetchInterval?: number,
	staleTime?: number,
	select?: (data: CoinBalance[]) => TResult,
) {
	const rpc = useRpcClient();
	return useQuery({
		queryKey: ['get-all-balance', address],
		queryFn: () => rpc.getAllBalances({ owner: address! }),
		enabled: !!address,
		refetchInterval,
		staleTime,
		select,
	});
}
