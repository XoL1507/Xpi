// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import {
    GrowthBookProvider,
    useFeatureIsOn,
} from '@growthbook/growthbook-react';
import { RpcClientContext } from '@mysten/core';
import { WalletKitProvider } from '@mysten/wallet-kit';
import * as Sentry from '@sentry/react';
import { QueryClientProvider } from '@tanstack/react-query';
import { ReactQueryDevtools } from '@tanstack/react-query-devtools';
import { useEffect, useMemo } from 'react';
import { Toaster } from 'react-hot-toast';
import { Outlet, ScrollRestoration } from 'react-router-dom';

import { usePageView } from '../../hooks/usePageView';
import Footer from '../footer/Footer';
import Header from '../header/Header';

import { useNetwork } from '~/hooks/useNetwork';
import { Banner } from '~/ui/Banner';
import { DefaultRpcClient } from '~/utils/api/DefaultRpcClient';
import { growthbook } from '~/utils/growthbook';
import { queryClient } from '~/utils/queryClient';

export function Layout() {
    const [network] = useNetwork();
    const jsonRpcProvider = useMemo(() => DefaultRpcClient(network), [network]);
    const networkOutage = useFeatureIsOn('explorer-network-outage');

    useEffect(() => {
        growthbook.setAttributes({
            network,
            environment: import.meta.env.VITE_VERCEL_ENV,
        });
    }, [network]);

    useEffect(() => {
        Sentry.setContext('network', {
            network,
        });
    }, [network]);

    usePageView();

    return (
        <GrowthBookProvider growthbook={growthbook}>
            <ScrollRestoration />
            <WalletKitProvider
                /*autoConnect={false}*/
                enableUnsafeBurner={import.meta.env.DEV}
            >
                <QueryClientProvider client={queryClient}>
                    <RpcClientContext.Provider value={jsonRpcProvider}>
                        <div className="w-full">
                            <Header />
                            <main className="relative z-10 min-h-screen bg-offwhite">
                                <section className="mx-auto max-w-[1440px] px-5 py-10 lg:px-10 2xl:px-0">
                                    {networkOutage && (
                                        <div className="pb-2.5">
                                            <Banner
                                                variant="warning"
                                                border
                                                fullWidth
                                            >
                                                We&rsquo;re sorry that the
                                                explorer is running slower than
                                                usual. We&rsquo;re working to
                                                fix the issue and appreciate
                                                your patience.
                                            </Banner>
                                        </div>
                                    )}
                                    <Outlet />
                                </section>
                            </main>
                            <Footer />
                        </div>

                        <Toaster
                            position="bottom-center"
                            gutter={8}
                            containerStyle={{
                                top: 40,
                                left: 40,
                                bottom: 40,
                                right: 40,
                            }}
                            toastOptions={{
                                duration: 4000,
                                success: {
                                    icon: null,
                                    className:
                                        '!bg-success-light !text-success-dark',
                                },
                                error: {
                                    icon: null,
                                    className:
                                        '!bg-issue-light !text-issue-dark',
                                },
                            }}
                        />
                        <ReactQueryDevtools />
                    </RpcClientContext.Provider>
                </QueryClientProvider>
            </WalletKitProvider>
        </GrowthBookProvider>
    );
}
