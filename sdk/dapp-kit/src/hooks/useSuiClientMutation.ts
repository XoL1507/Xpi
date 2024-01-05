// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import type { UseMutationOptions } from '@tanstack/react-query';
import { useMutation } from '@tanstack/react-query';
import { useSuiClientContext } from './useSuiClient.js';
import type { SuiRpcMethods } from './useSuiClientQuery.js';

export type UseSuiClientMutationOptions<T extends keyof SuiRpcMethods> = Omit<
	UseMutationOptions<SuiRpcMethods[T]['result'], unknown, SuiRpcMethods[T]['result'], unknown[]>,
	'mutationFn'
>;

export function useSuiClientMutation<T extends keyof SuiRpcMethods>(
	{
		method,
		params,
	}: {
		method: T;
		params: SuiRpcMethods[T]['params'];
	},
	options: UseSuiClientMutationOptions<T> = {},
) {
	const suiContext = useSuiClientContext();

	return useMutation({
		...options,
		mutationFn: async () => {
			return await suiContext.client[method](params as never);
		},
	});
}
