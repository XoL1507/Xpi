// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import {
    RawSigner,
    JsonRpcProvider,
    LocalTxnDataSerializer,
} from '@mysten/sui.js';

import { growthbook } from './experimentation/feature-gating';
import { FEATURES } from './experimentation/features';
import { queryClient } from './helpers/queryClient';

import type { Keypair } from '@mysten/sui.js';

export enum API_ENV {
    local = 'local',
    devNet = 'devNet',
    staging = 'staging',
}

type EnvInfo = {
    name: string;
    color: string;
};

type ApiEndpoints = {
    fullNode: string;
    faucet: string;
};
export const API_ENV_TO_INFO: Record<API_ENV, EnvInfo> = {
    [API_ENV.local]: { name: 'Local', color: '#9064ff' },
    [API_ENV.devNet]: { name: 'DevNet', color: '#29b6af' },
    [API_ENV.staging]: { name: 'Staging', color: '#ff4a8d' },
};

export const ENV_TO_API: Record<API_ENV, ApiEndpoints> = {
    [API_ENV.local]: {
        fullNode: process.env.API_ENDPOINT_LOCAL_FULLNODE || '',
        faucet: process.env.API_ENDPOINT_LOCAL_FAUCET || '',
    },
    [API_ENV.devNet]: {
        fullNode: process.env.API_ENDPOINT_DEV_NET_FULLNODE || '',
        faucet: process.env.API_ENDPOINT_DEV_NET_FAUCET || '',
    },
    [API_ENV.staging]: {
        fullNode: process.env.API_ENDPOINT_STAGING_FULLNODE || '',
        faucet: process.env.API_ENDPOINT_STAGING_FAUCET || '',
    },
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
    if (
        !apiEndpoint ||
        apiEndpoint.fullNode === '' ||
        apiEndpoint.faucet === ''
    ) {
        throw new Error(`API endpoint not found for API_ENV ${env}`);
    }
    return apiEndpoint;
}

export const DEFAULT_API_ENV = getDefaultApiEnv();

export default class ApiProvider {
    private _apiFullNodeProvider?: JsonRpcProvider;
    private _signer: RawSigner | null = null;

    public setNewJsonRpcProvider(apiEnv: API_ENV = DEFAULT_API_ENV) {
        // We also clear the query client whenever set set a new API provider:
        queryClient.clear();
        this._apiFullNodeProvider = new JsonRpcProvider(
            getDefaultAPI(apiEnv).fullNode
        );
        this._signer = null;
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

    public getSignerInstance(keypair: Keypair): RawSigner {
        if (!this._apiFullNodeProvider) {
            this.setNewJsonRpcProvider();
        }
        if (!this._signer) {
            this._signer = new RawSigner(
                keypair,
                this._apiFullNodeProvider,

                growthbook.isOn(FEATURES.USE_LOCAL_TXN_SERIALIZER)
                    ? // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
                      new LocalTxnDataSerializer(this._apiFullNodeProvider!)
                    : undefined
            );
        }
        return this._signer;
    }
}
