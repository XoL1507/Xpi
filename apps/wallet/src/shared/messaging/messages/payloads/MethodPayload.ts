// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { type SuiAddress } from '@mysten/sui.js';

import { isBasePayload } from './BasePayload';

import type { Payload } from './Payload';

type MethodPayloads = {
    zkCreateAccount: {
        currentEpoch: number;
        accountPin?: string;
    };
    zkAccountCreated: {
        pin: string;
        email: string;
        address: string;
    };
    ensureZKAccountUnlocked: { currentEpoch: number; address: SuiAddress };
};

type Methods = keyof MethodPayloads;

export interface MethodPayload<M extends Methods> {
    type: 'method-payload';
    method: M;
    args: MethodPayloads[M];
}

export function isMethodPayload<M extends Methods>(
    payload: Payload,
    method: M
): payload is MethodPayload<M> {
    return (
        isBasePayload(payload) &&
        payload.type === 'method-payload' &&
        'method' in payload &&
        payload.method === method &&
        'args' in payload
    );
}
