// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
import clsx from 'clsx';
import { lazy, Suspense } from 'react';

import { ErrorBoundary } from '../../components/error-boundary/ErrorBoundary';
import { TopValidatorsCard } from '../../components/top-validators-card/TopValidatorsCard';

import { Activity } from '~/components/Activity';
import { GasPriceCard } from '~/components/GasPriceCard';
import { CurrentEpoch, NetworkTPS, OnTheNetwork } from '~/components/HomeMetrics';
import { SuiTokenCard } from '~/components/SuiTokenCard';
import { TopPackagesCard } from '~/components/top-packages/TopPackagesCard';
import { useNetwork } from '~/context';
import { Card } from '~/ui/Card';
import { Tab, TabGroup, TabList, TabPanel, TabPanels } from '~/ui/Tabs';
import { Network } from '~/utils/api/DefaultRpcClient';

const ValidatorMap = lazy(() => import('../../components/validator-map'));

const TRANSACTIONS_LIMIT = 25;

function Home() {
	const [network] = useNetwork();
	const isSuiTokenCardEnabled = network === Network.MAINNET;

	return (
		<div
			data-testid="home-page"
			className={clsx('home-page-grid-container', {
				'home-page-grid-container-with-sui-token': isSuiTokenCardEnabled,
			})}
		>
			<div style={{ gridArea: 'tps' }}>
				<NetworkTPS />
			</div>
			<div style={{ gridArea: 'epoch' }}>
				<CurrentEpoch />
			</div>
			{isSuiTokenCardEnabled && (
				<div style={{ gridArea: 'sui-token' }}>
					<SuiTokenCard />
				</div>
			)}

			<div style={{ gridArea: 'gas-price' }}>
				<GasPriceCard useLargeSpacing={!isSuiTokenCardEnabled} />
			</div>

			<div style={{ gridArea: 'network' }} className="overflow-hidden">
				<OnTheNetwork />
			</div>

			<div style={{ gridArea: 'node-map' }} className="h-[360px] xl:h-auto">
				<ErrorBoundary>
					<Suspense fallback={<Card height="full" />}>
						<ValidatorMap minHeight="100%" />
					</Suspense>
				</ErrorBoundary>
			</div>
			<div style={{ gridArea: 'activity' }} className="mt-5">
				<ErrorBoundary>
					<Activity initialLimit={TRANSACTIONS_LIMIT} disablePagination />
				</ErrorBoundary>
			</div>
			<div data-testid="validators-table" style={{ gridArea: 'validators' }} className="mt-5">
				<TabGroup size="lg">
					<TabList>
						<Tab>Validators</Tab>
					</TabList>
					<TabPanels>
						<TabPanel>
							<ErrorBoundary>
								<TopValidatorsCard limit={10} showIcon />
							</ErrorBoundary>
						</TabPanel>
					</TabPanels>
				</TabGroup>
			</div>

			<div style={{ gridArea: 'packages' }}>
				<TopPackagesCard />
			</div>
		</div>
	);
}

export default Home;
