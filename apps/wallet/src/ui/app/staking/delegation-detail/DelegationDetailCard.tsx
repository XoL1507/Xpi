// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import {
	useGetValidatorsApy,
	useGetSystemState,
	useCoinMetadata,
	useGetCoinBalance,
} from '@mysten/core';
import { ArrowLeft16, StakeAdd16, StakeRemove16 } from '@mysten/icons';
import { SUI_TYPE_ARG } from '@mysten/sui.js';
import BigNumber from 'bignumber.js';
import { useMemo } from 'react';

import { useActiveAddress } from '../../hooks/useActiveAddress';
import { Heading } from '../../shared/heading';
import { getDelegationDataByStakeId } from '../getDelegationByStakeId';
import { StakeAmount } from '../home/StakeAmount';
import { useGetDelegatedStake } from '../useGetDelegatedStake';
import { Button } from '_app/shared/ButtonUI';
import BottomMenuLayout, { Content } from '_app/shared/bottom-menu-layout';
import { Card } from '_app/shared/card';
import { CardItem } from '_app/shared/card/CardItem';
import { Text } from '_app/shared/text';
import { IconTooltip } from '_app/shared/tooltip';
import Alert from '_components/alert';
import LoadingIndicator from '_components/loading/LoadingIndicator';
import { useAppSelector, useCoinsReFetchingConfig } from '_hooks';
import { API_ENV } from '_src/shared/api-env';
import { MIN_NUMBER_SUI_TO_STAKE } from '_src/shared/constants';
import FaucetRequestButton from '_src/ui/app/shared/faucet/FaucetRequestButton';

type DelegationDetailCardProps = {
	validatorAddress: string;
	stakedId: string;
};

export function DelegationDetailCard({ validatorAddress, stakedId }: DelegationDetailCardProps) {
	const {
		data: system,
		isLoading: loadingValidators,
		isError: errorValidators,
	} = useGetSystemState();

	const accountAddress = useActiveAddress();

	const { data: allDelegation, isLoading, isError } = useGetDelegatedStake(accountAddress || '');

	const apiEnv = useAppSelector(({ app }) => app.apiEnv);
	const { staleTime, refetchInterval } = useCoinsReFetchingConfig();
	const { data: suiCoinBalance } = useGetCoinBalance(
		SUI_TYPE_ARG,
		accountAddress,
		refetchInterval,
		staleTime,
	);
	const { data: metadata } = useCoinMetadata(SUI_TYPE_ARG);
	// set minimum stake amount to 1 SUI
	const showRequestMoreSuiToken = useMemo(() => {
		if (!suiCoinBalance?.totalBalance || !metadata?.decimals || apiEnv === API_ENV.mainnet)
			return false;
		const currentBalance = new BigNumber(suiCoinBalance.totalBalance);
		const minStakeAmount = new BigNumber(MIN_NUMBER_SUI_TO_STAKE).shiftedBy(metadata.decimals);
		return currentBalance.lt(minStakeAmount.toString());
	}, [apiEnv, metadata?.decimals, suiCoinBalance?.totalBalance]);

	const { data: rollingAverageApys } = useGetValidatorsApy();

	const validatorData = useMemo(() => {
		if (!system) return null;
		return system.activeValidators.find((av) => av.suiAddress === validatorAddress);
	}, [validatorAddress, system]);

	const delegationData = useMemo(() => {
		return allDelegation ? getDelegationDataByStakeId(allDelegation, stakedId) : null;
	}, [allDelegation, stakedId]);

	const totalStake = BigInt(delegationData?.principal || 0n);

	const suiEarned = BigInt(delegationData?.estimatedReward || 0n);
	const { apy, isApyApproxZero } = rollingAverageApys?.[validatorAddress] ?? {
		apy: 0,
	};

	const delegationId = delegationData?.status === 'Active' && delegationData?.stakedSuiId;

	const stakeByValidatorAddress = `/stake/new?${new URLSearchParams({
		address: validatorAddress,
		staked: stakedId,
	}).toString()}`;

	// check if the validator is in the active validator list, if not, is inactive validator
	const hasInactiveValidatorDelegation = !system?.activeValidators?.find(
		({ stakingPoolId }) => stakingPoolId === validatorData?.stakingPoolId,
	);

	const commission = validatorData ? Number(validatorData.commissionRate) / 100 : 0;

	if (isLoading || loadingValidators) {
		return (
			<div className="p-2 w-full flex justify-center items-center h-full">
				<LoadingIndicator />
			</div>
		);
	}

	if (isError || errorValidators) {
		return (
			<div className="p-2">
				<Alert>
					<div className="mb-1 font-semibold">Something went wrong</div>
				</Alert>
			</div>
		);
	}

	return (
		<div className="flex flex-col flex-nowrap flex-grow h-full">
			<BottomMenuLayout>
				<Content>
					<div className="justify-center w-full flex flex-col items-center">
						{hasInactiveValidatorDelegation ? (
							<div className="mb-3">
								<Alert>
									Unstake SUI from this inactive validator and stake on an active validator to start
									earning rewards again.
								</Alert>
							</div>
						) : null}
						<div className="w-full flex">
							<Card
								header={
									<div className="grid grid-cols-2 divide-x divide-solid divide-gray-45 divide-y-0 w-full">
										<CardItem title="Your Stake">
											<StakeAmount balance={totalStake} variant="heading5" />
										</CardItem>

										<CardItem title="Earned">
											<StakeAmount balance={suiEarned} variant="heading5" isEarnedRewards />
										</CardItem>
									</div>
								}
								padding="none"
							>
								<div className="divide-x flex divide-solid divide-gray-45 divide-y-0">
									<CardItem
										title={
											<div className="flex text-steel-darker gap-1 items-start">
												APY
												<div className="text-steel">
													<IconTooltip tip="Annual Percentage Yield" placement="top" />
												</div>
											</div>
										}
									>
										<div className="flex gap-0.5 items-baseline">
											<Heading variant="heading4" weight="semibold" color="gray-90" leading="none">
												{isApyApproxZero ? '~' : ''}
												{apy}
											</Heading>

											<Text variant="subtitleSmall" weight="medium" color="steel-dark">
												%
											</Text>
										</div>
									</CardItem>

									<CardItem
										title={
											<div className="flex text-steel-darker gap-1">
												Commission
												<div className="text-steel">
													<IconTooltip tip="Validator commission" placement="top" />
												</div>
											</div>
										}
									>
										<div className="flex gap-0.5 items-baseline">
											<Heading variant="heading4" weight="semibold" color="gray-90" leading="none">
												{commission}
											</Heading>

											<Text variant="subtitleSmall" weight="medium" color="steel-dark">
												%
											</Text>
										</div>
									</CardItem>
								</div>
							</Card>
						</div>
						<div className="flex gap-2.5 w-full my-3.75">
							{!hasInactiveValidatorDelegation ? (
								<Button
									size="tall"
									variant="outline"
									to={stakeByValidatorAddress}
									before={<StakeAdd16 />}
									text="Stake SUI"
									disabled={showRequestMoreSuiToken}
								/>
							) : null}

							{Boolean(totalStake) && delegationId && (
								<Button
									data-testid="unstake-button"
									size="tall"
									variant="outline"
									to={stakeByValidatorAddress + '&unstake=true'}
									text="Unstake SUI"
									before={<StakeRemove16 />}
								/>
							)}
						</div>
					</div>
				</Content>

				{/* show faucet request button on devnet or testnet whenever there is only one coin  */}
				{showRequestMoreSuiToken ? (
					<div className="flex flex-col gap-4 items-center">
						<div className="w-8/12 text-center">
							<Text variant="pSubtitle" weight="medium" color="steel-darker">
								You need a minimum of {MIN_NUMBER_SUI_TO_STAKE} SUI to continue staking.
							</Text>
						</div>
						<FaucetRequestButton size="tall" />
					</div>
				) : (
					<Button
						size="tall"
						variant="secondary"
						to="/stake"
						before={<ArrowLeft16 />}
						text="Back"
					/>
				)}
			</BottomMenuLayout>
		</div>
	);
}
