// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import type { MutationKey } from '@tanstack/react-query';

export const walletMutationKeys = {
	all: { baseScope: 'wallet' },
	connectWallet: formMutationKeyFn('connect-wallet'),
	disconnectWallet: formMutationKeyFn('disconnect-wallet'),
	switchAccount: formMutationKeyFn('switch-account'),
	signPersonalMessage: formMutationKeyFn('sign-personal-message'),
	signTransactionBlock: formMutationKeyFn('sign-transaction-block'),
	signAndExecuteTransactionBlock: formMutationKeyFn('sign-and-execute-transaction-block'),
};

function formMutationKeyFn(baseEntity: string) {
	return function mutationKeyFn(additionalKeys: MutationKey = []) {
		return [{ ...walletMutationKeys.all, baseEntity }, ...additionalKeys];
	};
}
