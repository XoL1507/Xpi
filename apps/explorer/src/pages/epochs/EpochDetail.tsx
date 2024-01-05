// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { useFeature, useGrowthBook } from '@growthbook/growthbook-react';
import { Navigate } from 'react-router-dom';

import { validatorsTableData } from '../validators/Validators';
import { getMockEpochData, mockCheckpointsTable } from './mocks';
import { EpochStats } from './stats/EpochStats';

import { SuiAmount } from '~/components/transaction-card/TxCardUtils';
import { useGetSystemObject } from '~/hooks/useGetObject';
import { useGetValidatorsEvents } from '~/hooks/useGetValidatorsEvents';
import { EpochProgress } from '~/pages/epochs/stats/EpochProgress';
import { Banner } from '~/ui/Banner';
import { Card } from '~/ui/Card';
import { LoadingSpinner } from '~/ui/LoadingSpinner';
import { RingChart } from '~/ui/RingChart';
import { Stats } from '~/ui/Stats';
import { TableCard } from '~/ui/TableCard';
import { Tab, TabGroup, TabList, TabPanel, TabPanels } from '~/ui/Tabs';
import { GROWTHBOOK_FEATURES } from '~/utils/growthbook';

function EpochDetail() {
    const { endTimestamp, gasCostSummary, totalRewards, storageFundEarnings } =
        getMockEpochData();

    const { data, isError, isLoading } = useGetSystemObject();

    const { data: validatorEvents, isLoading: validatorsEventsLoading } =
        useGetValidatorsEvents({
            limit: data?.activeValidators.length || 0,
            order: 'descending',
        });

    if (isError)
        return (
            <Banner variant="error" fullWidth>
                There was an issue retrieving data for the current epoch
            </Banner>
        );

    if (isLoading || validatorsEventsLoading) return <LoadingSpinner />;
    if (!data || !validatorEvents) return null;

    const validatorsTable = validatorsTableData(data, validatorEvents.data);

    return (
        <div className="flex flex-col space-y-16">
            <div className="grid grid-flow-row gap-4 sm:gap-2 md:flex md:gap-6">
                <EpochProgress
                    epoch={data.epoch}
                    inProgress
                    start={data.epochStartTimestampMs ?? 0}
                    end={endTimestamp}
                />
                <EpochStats label="Activity">
                    <Stats label="Gas Revenue" tooltip="Gas Revenue">
                        <SuiAmount amount={gasCostSummary?.gasRevenue} />
                    </Stats>
                    <Stats label="Storage Revenue" tooltip="Storage Revenue">
                        <SuiAmount amount={gasCostSummary?.storageRevenue} />
                    </Stats>
                    <Stats label="Stake Rewards" tooltip="Stake Rewards">
                        <SuiAmount amount={gasCostSummary?.stakeRewards} />
                    </Stats>
                </EpochStats>
                <EpochStats label="Rewards">
                    <Stats label="Stake Subsidies" tooltip="Stake Subsidies">
                        <SuiAmount
                            amount={data.stakeSubsidyCurrentDistributionAmount}
                        />
                    </Stats>
                    <Stats label="Total Rewards" tooltip="Total Rewards">
                        <SuiAmount amount={totalRewards} />
                    </Stats>

                    <Stats
                        label="Storage Fund Earnings"
                        tooltip="Storage Fund Earnings"
                    >
                        <SuiAmount amount={storageFundEarnings} />
                    </Stats>
                </EpochStats>
                <Card spacing="lg">
                    <RingChart
                        title="Validators in Next Epoch"
                        suffix="validators"
                        data={[
                            {
                                value: data.activeValidators.length,
                                label: 'Active',
                                color: '#589AEA',
                            },
                            {
                                value: data.pendingActiveValidatorsSize,
                                label: 'New',
                                color: '#6FBCF0',
                            },
                            {
                                value: data.atRiskValidators.length,
                                label: 'At Risk',
                                color: '#FF794B',
                            },
                        ]}
                    />
                </Card>
            </div>

            <TabGroup size="lg">
                <TabList>
                    <Tab>Checkpoints</Tab>
                    <Tab>Participating Validators</Tab>
                </TabList>
                <TabPanels className="mt-4">
                    <TabPanel>
                        <TableCard
                            data={mockCheckpointsTable.data}
                            columns={mockCheckpointsTable.columns}
                        />
                    </TabPanel>
                    <TabPanel>
                        {validatorsTable ? (
                            <TableCard
                                data={validatorsTable?.data!}
                                sortTable
                                defaultSorting={[{ id: 'stake', desc: false }]}
                                columns={validatorsTable?.columns!}
                            />
                        ) : null}
                    </TabPanel>
                </TabPanels>
            </TabGroup>
        </div>
    );
}

export default function EpochDetailFeatureFlagged() {
    const gb = useGrowthBook();
    const { on: epochsEnabled } = useFeature(
        GROWTHBOOK_FEATURES.EPOCHS_CHECKPOINTS
    );
    if (!gb?.ready) return <LoadingSpinner />;
    if (epochsEnabled) return <EpochDetail />;
    return <Navigate to="/" />;
}
