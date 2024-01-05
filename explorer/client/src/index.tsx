// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import * as Sentry from '@sentry/react';
import { BrowserTracing } from '@sentry/tracing';
import React from 'react';
import ReactDOM from 'react-dom';
import { BrowserRouter as Router } from 'react-router-dom';

import App from './app/App';
import {
    CURRENT_ENV,
    IS_DEVNET_ENV,
    IS_STAGING_ENV,
    IS_TESTNET_ENV,
} from './utils/envUtil';
import reportWebVitals from './utils/reportWebVitals';

import './index.css';

if (IS_STAGING_ENV || IS_DEVNET_ENV || IS_TESTNET_ENV) {
    Sentry.init({
        dsn: 'https://e4251274d1b141d7ba272103fa0f8d83@o1314142.ingest.sentry.io/6564988',
        integrations: [new BrowserTracing()],

        // Set tracesSampleRate to 1.0 to capture 100%
        // of transactions for performance monitoring.
        // TODO: adjust this to a lower value once the Explorer
        // has more traffic
        tracesSampleRate: 1.0,
        environment: CURRENT_ENV,
    });
}

ReactDOM.render(
    <React.StrictMode>
        <Router>
            <App />
        </Router>
    </React.StrictMode>,
    document.getElementById('root')
);

reportWebVitals();
