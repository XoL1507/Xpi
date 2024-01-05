// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import type { SuiClient } from '@mysten/sui.js/client';
import type { UseQueryOptions } from '@tanstack/react-query';
import { useQuery } from '@tanstack/react-query';

import { useSuiClientContext } from './useSuiClient.js';

export type SuiRpcMethodName = {
	[K in keyof SuiClient]: SuiClient[K] extends ((input: any) => Promise<any>) | (() => Promise<any>)
		? K
		: never;
}[keyof SuiClient];

export type SuiRpcMethods = {
	[K in SuiRpcMethodName]: SuiClient[K] extends (input: infer P) => Promise<infer R>
		? {
				name: K;
				result: R;
				params: P;
		  }
		: SuiClient[K] extends () => Promise<infer R>
		? {
				name: K;
				result: R;
				params: undefined;
		  }
		: never;
};

export type UseSuiClientQueryOptions<T extends keyof SuiRpcMethods> = Omit<
	UseQueryOptions<SuiRpcMethods[T]['result'], Error, SuiRpcMethods[T]['result'], unknown[]>,
	'queryFn'
>;

export function useSuiClientQuery<T extends keyof SuiRpcMethods>(
	{
		method,
		params,
	}: {
		method: T;
		params: SuiRpcMethods[T]['params'];
	},
	{ queryKey, enabled = !!params, ...options }: UseSuiClientQueryOptions<T> = {},
) {
	const suiContext = useSuiClientContext();

	return useQuery({
		...options,
		queryKey: [suiContext.network, method, params],
		enabled,
		queryFn: async () => {
			return await suiContext.client[method](params as never);
		},
	});
}
