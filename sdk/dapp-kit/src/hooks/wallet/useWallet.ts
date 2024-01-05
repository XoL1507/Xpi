// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { useMemo } from 'react';
import { useWalletContext } from './useWalletContext.js';

/**
 * Hook for retrieving wallet and account information.
 */
export function useWallet() {
	const { wallets, currentWallet, accounts, currentAccount, connectionStatus } = useWalletContext();
	return useMemo(
		() => ({ wallets, currentWallet, accounts, currentAccount, connectionStatus }),
		[accounts, currentAccount, currentWallet, wallets, connectionStatus],
	);
}
