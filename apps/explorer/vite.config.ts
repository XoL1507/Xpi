// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

/// <reference types="vitest" />
import { pathAlias } from '@mysten/core/vite.config';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';
import svgr from 'vite-plugin-svgr';
import { configDefaults } from 'vitest/config';

process.env.VITE_VERCEL_ENV = process.env.VERCEL_ENV || 'development';

// https://vitejs.dev/config/
export default defineConfig({
    plugins: [react(), svgr()],
    test: {
        // Omit end-to-end tests:
        exclude: [...configDefaults.exclude, 'tests/**'],
        css: true,
        globals: true,
        environment: 'happy-dom',
    },
    // NOTE: This allows the static and non-static sites to run at the same time without cache conflicts.
    // This should be removed when we remove static mode.
    cacheDir:
        process.env.VITE_NETWORK === 'STATIC'
            ? 'node_modules/.vite-static'
            : 'node_modules/.vite',
    build: {
        // Set the output directory to match what CRA uses:
        outDir: 'build',
        sourcemap: true,
    },
    resolve: {
        alias: {
            '~': new URL('./src', import.meta.url).pathname,
            ...pathAlias(import.meta.url),
        },
    },
});