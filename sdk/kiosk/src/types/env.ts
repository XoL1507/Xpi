// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { SuiAddress } from '@mysten/sui.js';

/* A list of environments. */
export type Environment = 'mainnet' | 'testnet' | 'devnet' | 'custom';

/** A Parameter to support enivronments for rules.  */
export type RulesEnvironmentParam = { env: Environment; address?: SuiAddress };

/** A default Testnet Environment object  */
export const testnetEnvironment: RulesEnvironmentParam = { env: 'testnet' };

/** A default Mainnet Environment object  */
export const mainnetEnvironment: RulesEnvironmentParam = { env: 'mainnet' };

/** A helper to easily export a custom environment */
export const customEnvironment = (address: string): RulesEnvironmentParam => ({
	env: 'custom',
	address,
});
