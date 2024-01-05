// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { createRoot } from 'react-dom/client';
import { IntlProvider } from 'react-intl';
import { Provider } from 'react-redux';
import { HashRouter } from 'react-router-dom';

import App from './app';
import { initAppType, initNetworkFromStorage } from '_redux/slices/app';
import { getFromLocationSearch } from '_redux/slices/app/AppType';
import store from '_store';
import { thunkExtras } from '_store/thunk-extras';

import './styles/global.scss';
import '_font-icons/output/sui-icons.scss';
import 'bootstrap-icons/font/bootstrap-icons.scss';

async function init() {
    if (process.env.NODE_ENV === 'development') {
        Object.defineProperty(window, 'store', { value: store });
    }
    await thunkExtras.featureGating.init();
    store.dispatch(initAppType(getFromLocationSearch(window.location.search)));
    await store.dispatch(initNetworkFromStorage()).unwrap();
    await thunkExtras.background.init(store.dispatch);
}

function renderApp() {
    const rootDom = document.getElementById('root');
    if (!rootDom) {
        throw new Error('Root element not found');
    }
    const root = createRoot(rootDom);
    root.render(
        <HashRouter>
            <Provider store={store}>
                <IntlProvider locale={navigator.language}>
                    <App />
                </IntlProvider>
            </Provider>
        </HashRouter>
    );
}

(async () => {
    await init();
    renderApp();
})();
