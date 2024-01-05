// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { SentryHttpTransport } from '@mysten/core';

import { SuiClient, SuiHTTPTransport } from '@mysten/sui.js/client';
import { type WalletSigner } from './WalletSigner';
import { BackgroundServiceSigner } from './background-client/BackgroundServiceSigner';
import { queryClient } from './helpers/queryClient';
import { type SerializedUIAccount } from '_src/background/accounts/Account';
import { AccountType, type SerializedAccount } from '_src/background/keyring/Account';
import { API_ENV } from '_src/shared/api-env';

import type { BackgroundClient } from './background-client';
import type { SignerWithProvider } from '@mysten/sui.js';

type EnvInfo = {
	name: string;
	env: API_ENV;
};

export const API_ENV_TO_INFO: Record<API_ENV, EnvInfo> = {
	[API_ENV.local]: { name: 'Local', env: API_ENV.local },
	[API_ENV.devNet]: { name: 'Devnet', env: API_ENV.devNet },
	[API_ENV.customRPC]: { name: 'Custom RPC', env: API_ENV.customRPC },
	[API_ENV.testNet]: { name: 'Testnet', env: API_ENV.testNet },
	[API_ENV.mainnet]: { name: 'Mainnet', env: API_ENV.mainnet },
};

export const ENV_TO_API: Record<API_ENV, string | null> = {
	[API_ENV.customRPC]: null,
	[API_ENV.local]: process.env.API_ENDPOINT_LOCAL_FULLNODE || '',
	[API_ENV.devNet]: process.env.API_ENDPOINT_DEV_NET_FULLNODE || '',
	[API_ENV.testNet]: process.env.API_ENDPOINT_TEST_NET_FULLNODE || '',
	[API_ENV.mainnet]: process.env.API_ENDPOINT_MAINNET_FULLNODE || '',
};

function getDefaultApiEnv() {
	const apiEnv = process.env.API_ENV;
	if (apiEnv && !Object.keys(API_ENV).includes(apiEnv)) {
		throw new Error(`Unknown environment variable API_ENV, ${apiEnv}`);
	}
	return apiEnv ? API_ENV[apiEnv as keyof typeof API_ENV] : API_ENV.devNet;
}

function getDefaultAPI(env: API_ENV) {
	const apiEndpoint = ENV_TO_API[env];
	if (!apiEndpoint || apiEndpoint === '') {
		throw new Error(`API endpoint not found for API_ENV ${env}`);
	}
	return apiEndpoint;
}

export const DEFAULT_API_ENV = getDefaultApiEnv();
const SENTRY_MONITORED_ENVS = [API_ENV.mainnet];

type NetworkTypes = keyof typeof API_ENV;

export const generateActiveNetworkList = (): NetworkTypes[] => {
	return Object.values(API_ENV);
};

export default class ApiProvider {
	private _apiFullNodeProvider?: SuiClient;
	private _signerByAddress: Map<string, SignerWithProvider> = new Map();

	public setNewJsonRpcProvider(apiEnv: API_ENV = DEFAULT_API_ENV, customRPC?: string | null) {
		const connection = customRPC ? customRPC : getDefaultAPI(apiEnv);
		this._apiFullNodeProvider = new SuiClient({
			transport:
				!customRPC && SENTRY_MONITORED_ENVS.includes(apiEnv)
					? new SentryHttpTransport(connection)
					: new SuiHTTPTransport({ url: connection }),
		});

		this._signerByAddress.clear();

		// We also clear the query client whenever set set a new API provider:
		queryClient.resetQueries();
		queryClient.clear();
	}

	public get instance() {
		if (!this._apiFullNodeProvider) {
			this.setNewJsonRpcProvider();
		}
		return {
			// eslint-disable-next-line @typescript-eslint/no-non-null-assertion
			fullNode: this._apiFullNodeProvider!,
		};
	}

	public getSignerInstance(
		// TODO: Remove SerializedAccount
		account: SerializedAccount | SerializedUIAccount,
		backgroundClient: BackgroundClient,
	): SignerWithProvider {
		if (!this._apiFullNodeProvider) {
			this.setNewJsonRpcProvider();
		}
		if (
			[AccountType.DERIVED, AccountType.IMPORTED, 'mnemonic-derived', 'imported'].includes(
				account.type,
			)
		) {
			return this.getBackgroundSignerInstance(account, backgroundClient);
		}
		if ([AccountType.LEDGER, 'ledger'].includes(account.type)) {
			// Ideally, Ledger transactions would be signed in the background
			// and exist as an asynchronous keypair; however, this isn't possible
			// because you can't connect to a Ledger device from the background
			// script. Similarly, the signer instance can't be retrieved from
			// here because ApiProvider is a global and results in very buggy
			// behavior due to the reactive nature of managing Ledger connections
			// and displaying relevant UI updates. Refactoring ApiProvider to
			// not be a global instance would help out here, but that is also
			// a non-trivial task because we need access to ApiProvider in the
			// background script as well.
			throw new Error("Signing with Ledger via ApiProvider isn't supported");
		}
		throw new Error('Encountered unknown account type');
	}

	public getBackgroundSignerInstance(
		account: SerializedAccount | SerializedUIAccount,
		backgroundClient: BackgroundClient,
	): WalletSigner {
		const key = 'id' in account ? account.id : account.address;
		if (!this._signerByAddress.has(key)) {
			this._signerByAddress.set(
				key,
				new BackgroundServiceSigner(account, backgroundClient, this._apiFullNodeProvider!),
			);
		}
		return this._signerByAddress.get(key)!;
	}
}
