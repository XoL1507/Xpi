// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { defineConfig } from 'cypress';

export default defineConfig({
    e2e: {
        baseUrl: 'http://localhost:8080',
    },
    component: {
        devServer: {
            framework: 'react',
            bundler: 'vite',
        },
    },
    defaultCommandTimeout: 10000,
});
