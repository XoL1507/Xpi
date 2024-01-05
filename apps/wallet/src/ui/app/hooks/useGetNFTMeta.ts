// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { getObjectFields, is, SuiObjectData } from '@mysten/sui.js';
import { useMemo } from 'react';

import { useGetObject } from './useGetObject';

export type NFTMetadata = {
    name: string | null;
    description: string | null;
    url: string;
};

export function useGetNFTMeta(objectID: string) {
    const resp = useGetObject(objectID);

    const nftMeta = useMemo(() => {
        if (!resp.data) return null;

        const { data } = resp.data || {};
        if (!is(data, SuiObjectData) || !resp) return null;
        const fields = getObjectFields(resp.data);
        if (!fields?.url) return null;
        return {
            description:
                typeof fields.description === 'string'
                    ? fields.description
                    : null,
            name: typeof fields.name === 'string' ? fields.name : null,
            url: fields.url,
        };
    }, [resp]);

    return {
        ...resp,
        data: nftMeta,
    };
}
