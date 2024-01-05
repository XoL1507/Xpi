// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import {
    SUI_CHAINS,
    ReadonlyWalletAccount,
    type SuiSignAndExecuteTransactionFeature,
    type SuiSignAndExecuteTransactionMethod,
    type ConnectFeature,
    type ConnectMethod,
    type Wallet,
    type EventsFeature,
    type EventsOnMethod,
    type EventsListeners,
} from '@mysten/wallet-standard';
import mitt, { type Emitter } from 'mitt';
import { filter, map, type Observable } from 'rxjs';

import { mapToPromise } from './utils';
import { createMessage } from '_messages';
import { WindowMessageStream } from '_messaging/WindowMessageStream';
import { type Payload } from '_payloads';
import {
    type AcquirePermissionsRequest,
    type AcquirePermissionsResponse,
    type HasPermissionsRequest,
    type HasPermissionsResponse,
    ALL_PERMISSION_TYPES,
} from '_payloads/permissions';

import type { GetAccount } from '_payloads/account/GetAccount';
import type { GetAccountResponse } from '_payloads/account/GetAccountResponse';
import type {
    ExecuteTransactionRequest,
    ExecuteTransactionResponse,
} from '_payloads/transactions';

type WalletEventsMap = {
    [E in keyof EventsListeners]: Parameters<EventsListeners[E]>[0];
};

// NOTE: Because this runs in a content script, we can't fetch the manifest.
const name = process.env.APP_NAME || 'Sui Wallet';

export class SuiWallet implements Wallet {
    readonly #events: Emitter<WalletEventsMap>;
    readonly #version = '1.0.0' as const;
    readonly #name = name;
    #account: ReadonlyWalletAccount | null;
    #messagesStream: WindowMessageStream;

    get version() {
        return this.#version;
    }

    get name() {
        return this.#name;
    }

    get icon() {
        return 'data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iNzIiIGhlaWdodD0iNzIiIHZpZXdCb3g9IjAgMCA3MiA3MiIgZmlsbD0ibm9uZSIgeG1sbnM9Imh0dHA6Ly93d3cudzMub3JnLzIwMDAvc3ZnIj4KPHJlY3Qgd2lkdGg9IjcyIiBoZWlnaHQ9IjcyIiByeD0iMTYiIGZpbGw9IiM2RkJDRjAiLz4KPHBhdGggZmlsbC1ydWxlPSJldmVub2RkIiBjbGlwLXJ1bGU9ImV2ZW5vZGQiIGQ9Ik0yMC40MjEzIDUyLjc4MzhDMjMuNjQ5NiA1OC4zNzYgMjkuNDMyMSA2MS43MTQyIDM1Ljg4ODggNjEuNzE0MkM0Mi4zNDU1IDYxLjcxNDIgNDguMTI3IDU4LjM3NiA1MS4zNTY0IDUyLjc4MzhDNTQuNTg0OCA0Ny4xOTI2IDU0LjU4NDggNDAuNTE2MyA1MS4zNTY0IDM0LjkyNEwzNy43NTI0IDExLjM2MTVDMzYuOTI0MSA5LjkyNzAxIDM0Ljg1MzUgOS45MjcwMSAzNC4wMjUzIDExLjM2MTVMMjAuNDIxMyAzNC45MjRDMTcuMTkyOSA0MC41MTUyIDE3LjE5MjkgNDcuMTkxNSAyMC40MjEzIDUyLjc4MzhaTTMyLjA1NjYgMjIuNTcxM0wzNC45NTcxIDE3LjU0NzRDMzUuMzcxMiAxNi44MzAxIDM2LjQwNjUgMTYuODMwMSAzNi44MjA2IDE3LjU0NzRMNDcuOTc5MSAzNi44NzQ4QzUwLjAyOTEgNDAuNDI1NCA1MC40MTM5IDQ0LjUzNSA0OS4xMzM1IDQ4LjI5NTRDNDkuMDAwMiA0Ny42ODE5IDQ4LjgxMzggNDcuMDU0MiA0OC41NjI2IDQ2LjQyMDFDNDcuMDIxMyA0Mi41MzA0IDQzLjUzNjMgMzkuNTI4OSAzOC4yMDIzIDM3LjQ5ODJDMzQuNTM1MSAzNi4xMDcxIDMyLjE5NDMgMzQuMDYxMyAzMS4yNDMxIDMxLjQxNzFDMzAuMDE4IDI4LjAwODkgMzEuMjk3NiAyNC4yOTI0IDMyLjA1NjYgMjIuNTcxM1pNMjcuMTEwNyAzMS4xMzc5TDIzLjc5ODYgMzYuODc0OEMyMS4yNzQ4IDQxLjI0NTkgMjEuMjc0OCA0Ni40NjQxIDIzLjc5ODYgNTAuODM1M0MyNi4zMjIzIDU1LjIwNjQgMzAuODQxMyA1Ny44MTUgMzUuODg4OCA1Ny44MTVDMzkuMjQxMyA1Ny44MTUgNDIuMzYxNSA1Ni42NjMzIDQ0LjgxODQgNTQuNjA4OEM0NS4xMzg4IDUzLjgwMjEgNDYuMTMxIDUwLjg0OTIgNDQuOTA1MiA0Ny44MDU4QzQzLjc3MyA0NC45OTU0IDQxLjA0ODIgNDIuNzUxOSAzNi44MDYxIDQxLjEzNkMzMi4wMTEgMzkuMzE3MSAyOC44OTU4IDM2LjQ3NzQgMjcuNTQ4NiAzMi42OTg0QzI3LjM2MzEgMzIuMTc4MSAyNy4yMTg5IDMxLjY1NjggMjcuMTEwNyAzMS4xMzc5WiIgZmlsbD0id2hpdGUiLz4KPC9zdmc+' as const;
    }

    get chains() {
        // TODO: Extract chain from wallet:
        return SUI_CHAINS;
    }

    get features(): ConnectFeature &
        EventsFeature &
        SuiSignAndExecuteTransactionFeature {
        return {
            'standard:connect': {
                version: '1.0.0',
                connect: this.#connect,
            },
            'standard:events': {
                version: '1.0.0',
                on: this.#on,
            },
            'sui:signAndExecuteTransaction': {
                version: '1.0.0',
                signAndExecuteTransaction: this.#signAndExecuteTransaction,
            },
        };
    }

    get accounts() {
        return this.#account ? [this.#account] : [];
    }

    constructor() {
        this.#events = mitt();
        this.#account = null;
        this.#messagesStream = new WindowMessageStream(
            'sui_in-page',
            'sui_content-script'
        );

        this.#connected();
    }

    #on: EventsOnMethod = (event, listener) => {
        this.#events.on(event, listener);
        return () => this.#events.off(event, listener);
    };

    #connected = async () => {
        if (!(await this.#hasPermissions(['viewAccount']))) {
            return;
        }
        const accounts = await mapToPromise(
            this.#send<GetAccount, GetAccountResponse>({
                type: 'get-account',
            }),
            (response) => response.accounts
        );

        const [address] = accounts;

        if (address) {
            const account = this.#account;
            if (!account || account.address !== address) {
                this.#account = new ReadonlyWalletAccount({
                    address,
                    // TODO: Expose public key instead of address:
                    publicKey: new Uint8Array(),
                    chains: SUI_CHAINS,
                    features: ['sui:signAndExecuteTransaction'],
                });
                this.#events.emit('change', { accounts: this.accounts });
            }
        }
    };

    #connect: ConnectMethod = async (input) => {
        if (!input?.silent) {
            await mapToPromise(
                this.#send<
                    AcquirePermissionsRequest,
                    AcquirePermissionsResponse
                >({
                    type: 'acquire-permissions-request',
                    permissions: ALL_PERMISSION_TYPES,
                }),
                (response) => response.result
            );
        }

        await this.#connected();

        return { accounts: this.accounts };
    };

    #signAndExecuteTransaction: SuiSignAndExecuteTransactionMethod = async (
        input
    ) => {
        return mapToPromise(
            this.#send<ExecuteTransactionRequest, ExecuteTransactionResponse>({
                type: 'execute-transaction-request',
                transaction: {
                    type: 'v2',
                    data: input.transaction,
                },
            }),
            (response) => response.result
        );
    };

    #hasPermissions(permissions: HasPermissionsRequest['permissions']) {
        return mapToPromise(
            this.#send<HasPermissionsRequest, HasPermissionsResponse>({
                type: 'has-permissions-request',
                permissions: permissions,
            }),
            ({ result }) => result
        );
    }

    #send<
        RequestPayload extends Payload,
        ResponsePayload extends Payload | void = void
    >(
        payload: RequestPayload,
        responseForID?: string
    ): Observable<ResponsePayload> {
        const msg = createMessage(payload, responseForID);
        this.#messagesStream.send(msg);
        return this.#messagesStream.messages.pipe(
            filter(({ id }) => id === msg.id),
            map((msg) => msg.payload as ResponsePayload)
        );
    }
}
