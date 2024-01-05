// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { useFeatureIsOn } from '@growthbook/growthbook-react';
import cl from 'classnames';
import { memo } from 'react';

import Loading from '_components/loading';
import { useAppSelector, useFullscreenGuard } from '_hooks';
import { getNavIsVisible } from '_redux/slices/app';

import type { ReactNode } from 'react';

import st from './Layout.module.scss';

export type PageLayoutProps = {
	forceFullscreen?: boolean;
	children: ReactNode | ReactNode[];
	className?: string;
};

function PageLayout({ forceFullscreen = false, children, className }: PageLayoutProps) {
	const guardLoading = useFullscreenGuard(forceFullscreen);
	const isNavVisible = useAppSelector(getNavIsVisible);
	const isSocialSignInEnabled = useFeatureIsOn('enoki-social-sign-in');

	return (
		<Loading loading={guardLoading}>
			<div
				className={cl('w-popup-width h-popup-height', st.container, className, {
					[st.navHidden]: !isNavVisible,
					'h-popup-height': !isSocialSignInEnabled,
					'h-popup-height-v2': isSocialSignInEnabled,
				})}
			>
				{children}
				<div id="overlay-portal-container"></div>
				<div id="toaster-portal-container"></div>
			</div>
		</Loading>
	);
}

export default memo(PageLayout);
