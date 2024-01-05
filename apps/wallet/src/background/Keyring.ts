// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { Ed25519Keypair } from '@mysten/sui.js';
import mitt from 'mitt';
import Browser from 'webextension-polyfill';

import { createMessage } from '_messages';
import { isKeyringPayload } from '_payloads/keyring';
import { encrypt, decrypt } from '_shared/cryptography/keystore';
import { generateMnemonic } from '_shared/utils/bip39';

import type { Keypair } from '@mysten/sui.js';
import type { Message } from '_messages';
import type { ErrorPayload } from '_payloads';
import type { KeyringPayload } from '_payloads/keyring';
import type { Connection } from '_src/background/connections/Connection';

type KeyringEvents = {
    lockedStatusUpdate: boolean;
};

const STORAGE_KEY = 'vault';

class Keyring {
    #events = mitt<KeyringEvents>();
    #locked = true;
    #keypair: Keypair | null = null;
    #mnemonic: string | null = null;

    // Creates a new mnemonic and saves it to storage encrypted
    // if importedMnemonic is provided it uses that one instead
    public async createMnemonic(password: string, importedMnemonic?: string) {
        if (await this.isWalletInitialized()) {
            throw new Error(
                'Mnemonic already exists, creating a new one will override it. Clear the existing one first.'
            );
        }
        const encryptedMnemonic = await encrypt(
            password,
            Buffer.from(importedMnemonic || generateMnemonic(), 'utf8')
        );
        await this.storeEncryptedMnemonic(encryptedMnemonic);
    }

    public lock() {
        this.#keypair = null;
        this.#mnemonic = null;
        this.#locked = true;
        this.notifyLockedStatusUpdate(this.#locked);
    }

    public async unlock(password: string) {
        this.#mnemonic = await this.decryptMnemonic(password);
        this.#keypair = Ed25519Keypair.deriveKeypair(this.#mnemonic);
        this.#locked = false;
        this.notifyLockedStatusUpdate(this.#locked);
    }

    public async clearMnemonic() {
        await this.storeEncryptedMnemonic(null);
        this.lock();
    }

    public async isWalletInitialized() {
        return !!(await this.loadMnemonic());
    }

    public get isLocked() {
        return this.#locked;
    }

    public get keypair() {
        return this.#keypair;
    }

    public mnemonic() {
        return this.#mnemonic;
    }

    // sui address always prefixed with 0x
    public get address() {
        if (this.#keypair) {
            let address = this.#keypair.getPublicKey().toSuiAddress();
            if (!address.startsWith('0x')) {
                address = `0x${address}`;
            }
            return address;
        }
        return null;
    }

    public on = this.#events.on;

    public off = this.#events.off;

    public async handleUiMessage(msg: Message, uiConnection: Connection) {
        const { id, payload } = msg;
        try {
            if (
                isKeyringPayload<'createMnemonic'>(payload, 'createMnemonic') &&
                payload.args !== undefined
            ) {
                const { password, importedMnemonic } = payload.args;
                await this.createMnemonic(password, importedMnemonic);
                await this.unlock(password);
                if (!this.#mnemonic) {
                    throw new Error('Error created mnemonic is empty');
                }
                uiConnection.send(
                    createMessage<KeyringPayload<'createMnemonic'>>(
                        {
                            type: 'keyring',
                            method: 'createMnemonic',
                            return: { mnemonic: this.#mnemonic },
                        },
                        id
                    )
                );
            } else if (
                isKeyringPayload<'getMnemonic'>(payload, 'getMnemonic')
            ) {
                if (this.#locked) {
                    throw new Error('Keyring is locked. Unlock it first.');
                }
                if (!this.#mnemonic) {
                    throw new Error('Error mnemonic is empty');
                }
                uiConnection.send(
                    createMessage<KeyringPayload<'getMnemonic'>>(
                        {
                            type: 'keyring',
                            method: 'getMnemonic',
                            return: this.#mnemonic,
                        },
                        id
                    )
                );
            } else if (
                isKeyringPayload<'unlock'>(payload, 'unlock') &&
                payload.args
            ) {
                await this.unlock(payload.args.password);
                uiConnection.send(createMessage({ type: 'done' }, id));
            } else if (
                isKeyringPayload<'walletStatusUpdate'>(
                    payload,
                    'walletStatusUpdate'
                )
            ) {
                uiConnection.send(
                    createMessage<KeyringPayload<'walletStatusUpdate'>>(
                        {
                            type: 'keyring',
                            method: 'walletStatusUpdate',
                            return: {
                                isLocked: this.isLocked,
                                isInitialized: await this.isWalletInitialized(),
                                mnemonic: this.#mnemonic || undefined,
                            },
                        },
                        id
                    )
                );
            } else if (isKeyringPayload<'lock'>(payload, 'lock')) {
                this.lock();
                uiConnection.send(createMessage({ type: 'done' }, id));
            }
        } catch (e) {
            uiConnection.send(
                createMessage<ErrorPayload>(
                    { code: -1, error: true, message: (e as Error).message },
                    id
                )
            );
        }
    }

    // pass null to delete it
    private async storeEncryptedMnemonic(encryptedMnemonic: string | null) {
        await Browser.storage.local.set({ [STORAGE_KEY]: encryptedMnemonic });
    }

    private async loadMnemonic() {
        const storedMnemonic = await Browser.storage.local.get({
            [STORAGE_KEY]: null,
        });
        return storedMnemonic[STORAGE_KEY];
    }

    private async decryptMnemonic(password: string) {
        const encryptedMnemonic = await this.loadMnemonic();
        if (!encryptedMnemonic) {
            throw new Error(
                'Mnemonic is not initialized. Create a new one first.'
            );
        }
        return Buffer.from(await decrypt(password, encryptedMnemonic)).toString(
            'utf8'
        );
    }

    private notifyLockedStatusUpdate(isLocked: boolean) {
        this.#events.emit('lockedStatusUpdate', isLocked);
    }
}

export default new Keyring();
