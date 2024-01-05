// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import type { PublicKey } from './publickey.js';
import type { SerializedSignature } from './signature.js';
import { toSerializedSignature } from './signature.js';
import type { SignatureScheme } from './signature.js';
import { IntentScope, messageWithIntent } from './intent.js';
import { blake2b } from '@noble/hashes/blake2b';

export const PRIVATE_KEY_SIZE = 32;
export const LEGACY_PRIVATE_KEY_SIZE = 64;

export type ExportedKeypair = {
	schema: SignatureScheme;
	privateKey: string;
};

interface SignedMessage {
	bytes: Uint8Array;
	signature: SerializedSignature;
}

/**
 * TODO: Document
 */
export abstract class BaseSigner {
	abstract sign(bytes: Uint8Array): Promise<Uint8Array>;

	async signWithIntent(bytes: Uint8Array, intent: IntentScope): Promise<SignedMessage> {
		const intentMessage = messageWithIntent(intent, bytes);
		const digest = blake2b(intentMessage, { dkLen: 32 });

		const signature = toSerializedSignature({
			signature: await this.sign(digest),
			signatureScheme: this.getKeyScheme(),
			pubKey: this.getPublicKey(),
		});

		return {
			signature,
			bytes,
		};
	}

	async signTransactionBlock(bytes: Uint8Array) {
		return this.signWithIntent(bytes, IntentScope.TransactionData);
	}

	async signMessage(bytes: Uint8Array) {
		return this.signWithIntent(bytes, IntentScope.PersonalMessage);
	}

	toSuiAddress(): string {
		return this.getPublicKey().toSuiAddress();
	}

	/**
	 * Return the signature for the data.
	 * Prefer the async verion {@link sign}, as this method will be deprecated in a future release.
	 */
	abstract signData(data: Uint8Array): Uint8Array;

	/**
	 * Get the key scheme of the keypair: Secp256k1 or ED25519
	 */
	abstract getKeyScheme(): SignatureScheme;

	/**
	 * The public key for this keypair
	 */
	abstract getPublicKey(): PublicKey;
}

/**
 * TODO: Document
 */
export abstract class Keypair extends BaseSigner {
	abstract export(): ExportedKeypair;
}
