// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

export type ResultType = {
    input: string;
    category: 'objects' | 'transactions' | 'addresses';
    result: object | null;
};
