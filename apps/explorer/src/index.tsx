// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import '@fontsource/inter/variable.css';
import '@fontsource/red-hat-mono/variable.css';
import { GrowthBookProvider } from '@growthbook/growthbook-react';
import React from 'react';
import ReactDOM from 'react-dom/client';
import { RouterProvider } from 'react-router-dom';

import { router } from './pages';
import { growthbook } from './utils/growthbook';
import './utils/sentry';
import { reportWebVitals } from './utils/vitals';

import './index.css';

// Start loading features as early as we can:
growthbook.loadFeatures();

ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
        <GrowthBookProvider growthbook={growthbook}>
            <RouterProvider router={router} />
        </GrowthBookProvider>
    </React.StrictMode>
);

reportWebVitals();
