// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import nacl from 'tweetnacl';
import { Base64DataBuffer } from '../serialization/base64';
import { Keypair } from './keypair';
import { Ed25519PublicKey } from './ed25519-publickey';
import { SignatureScheme } from './publickey';

/**
 * Ed25519 Keypair data
 */
export interface Ed25519KeypairData {
  publicKey: Uint8Array;
  secretKey: Uint8Array;
}

/**
 * An Ed25519 Keypair used for signing transactions.
 */
export class Ed25519Keypair implements Keypair {
  private keypair: Ed25519KeypairData;

  /**
   * Create a new Ed25519 keypair instance.
   * Generate random keypair if no {@link Ed25519Keypair} is provided.
   *
   * @param keypair Ed25519 keypair
   */
  constructor(keypair?: Ed25519KeypairData) {
    if (keypair) {
      this.keypair = keypair;
    } else {
      this.keypair = nacl.sign.keyPair();
    }
  }

  /**
   * Get the key scheme of the keypair ED25519
   */
  getKeyScheme(): SignatureScheme {
    return 'ED25519';
  }

  /**
   * Generate a new random Ed25519 keypair
   */
  static generate(): Ed25519Keypair {
    return new Ed25519Keypair(nacl.sign.keyPair());
  }

  /**
   * Create a Ed25519 keypair from a raw secret key byte array.
   *
   * This method should only be used to recreate a keypair from a previously
   * generated secret key.
   *
   * @throws error if the provided secret key is invalid and validation is not skipped.
   *
   * @param secretKey secret key byte array
   * @param options: skip secret key validation
   */
  static fromSecretKey(
    secretKey: Uint8Array,
    options?: { skipValidation?: boolean }
  ): Ed25519Keypair {
    const keypair = nacl.sign.keyPair.fromSecretKey(secretKey);
    if (!options || !options.skipValidation) {
      const encoder = new TextEncoder();
      const signData = encoder.encode('sui validation');
      const signature = nacl.sign.detached(signData, keypair.secretKey);
      if (!nacl.sign.detached.verify(signData, signature, keypair.publicKey)) {
        throw new Error('provided secretKey is invalid');
      }
    }
    return new Ed25519Keypair(keypair);
  }

  /**
   * Generate a Ed25519 keypair from a 32 byte seed.
   *
   * @param seed seed byte array
   */
  static fromSeed(seed: Uint8Array): Ed25519Keypair {
    return new Ed25519Keypair(nacl.sign.keyPair.fromSeed(seed));
  }

  /**
   * The public key for this Ed25519 keypair
   */
  getPublicKey(): Ed25519PublicKey {
    return new Ed25519PublicKey(this.keypair.publicKey);
  }

  /**
   * Return the signature for the provided data using Ed25519.
   */
  signData(data: Base64DataBuffer): Base64DataBuffer {
    return new Base64DataBuffer(
      nacl.sign.detached(data.getData(), this.keypair.secretKey)
    );
  }
}
