// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { fromB64 } from '@mysten/bcs';
import { blake2b } from '@noble/hashes/blake2b';
import { bytesToHex } from '@noble/hashes/utils';
import { PublicKey } from '../../cryptography/publickey.js';
import type { PublicKeyInitData } from '../../cryptography/publickey.js';
import { SIGNATURE_SCHEME_TO_FLAG } from '../../cryptography/signature.js';
import { SUI_ADDRESS_LENGTH, normalizeSuiAddress } from '../../utils/sui-types.js';

const SECP256R1_PUBLIC_KEY_SIZE = 33;

/**
 * A Secp256r1 public key
 */
export class Secp256r1PublicKey extends PublicKey {
	static SIZE = SECP256R1_PUBLIC_KEY_SIZE;
	private data: Uint8Array;

	/**
	 * Create a new Secp256r1PublicKey object
	 * @param value secp256r1 public key as buffer or base-64 encoded string
	 */
	constructor(value: PublicKeyInitData) {
		super();

		if (typeof value === 'string') {
			this.data = fromB64(value);
		} else if (value instanceof Uint8Array) {
			this.data = value;
		} else {
			this.data = Uint8Array.from(value);
		}

		if (this.data.length !== SECP256R1_PUBLIC_KEY_SIZE) {
			throw new Error(
				`Invalid public key input. Expected ${SECP256R1_PUBLIC_KEY_SIZE} bytes, got ${this.data.length}`,
			);
		}
	}

	/**
	 * Checks if two Secp256r1 public keys are equal
	 */
	override equals(publicKey: Secp256r1PublicKey): boolean {
		return super.equals(publicKey);
	}

	/**
	 * Return the byte array representation of the Secp256r1 public key
	 */
	toBytes(): Uint8Array {
		return this.data;
	}

	/**
	 * Return the Sui address associated with this Secp256r1 public key
	 */
	toSuiAddress(): string {
		let tmp = new Uint8Array(SECP256R1_PUBLIC_KEY_SIZE + 1);
		tmp.set([SIGNATURE_SCHEME_TO_FLAG['Secp256r1']]);
		tmp.set(this.toBytes(), 1);
		// Each hex char represents half a byte, hence hex address doubles the length
		return normalizeSuiAddress(
			bytesToHex(blake2b(tmp, { dkLen: 32 })).slice(0, SUI_ADDRESS_LENGTH * 2),
		);
	}

	/**
	 * Return the Sui address associated with this Secp256r1 public key
	 */
	flag(): number {
		return SIGNATURE_SCHEME_TO_FLAG['Secp256r1'];
	}
}
