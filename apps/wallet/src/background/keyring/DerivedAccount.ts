// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { type Keypair } from '@mysten/sui.js/cryptography';

import { AccountType, type Account } from './Account';
import { AccountKeypair } from './AccountKeypair';

/**
 * @deprecated
 */
export type SerializedDerivedAccount = {
	type: AccountType.DERIVED;
	address: string;
	derivationPath: string;
};

/**
 * @deprecated
 */
export class DerivedAccount implements Account {
	readonly accountKeypair: AccountKeypair;
	readonly type: AccountType;
	readonly address: string;
	readonly derivationPath: string;

	constructor({ derivationPath, keypair }: { derivationPath: string; keypair: Keypair }) {
		this.type = AccountType.DERIVED;
		this.derivationPath = derivationPath;
		this.accountKeypair = new AccountKeypair(keypair);
		this.address = this.accountKeypair.publicKey.toSuiAddress();
	}

	toJSON(): SerializedDerivedAccount {
		return {
			type: AccountType.DERIVED,
			address: this.address,
			derivationPath: this.derivationPath,
		};
	}

	getPublicKey() {
		return this.accountKeypair.publicKey.toBase64();
	}
}
