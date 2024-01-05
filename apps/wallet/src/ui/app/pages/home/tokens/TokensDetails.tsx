// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { getSendOrSwapUrl } from '_app/helpers/getSendOrSwapUrl';
import { getUSDCurrency, useSuiBalanceInUSDC } from '_app/hooks/useDeepbook';
import { useSortedCoinsByCategories } from '_app/hooks/useSortedCoinsByCategories';
import { LargeButton } from '_app/shared/LargeButton';
import { Text } from '_app/shared/text';
import { ButtonOrLink } from '_app/shared/utils/ButtonOrLink';
import Alert from '_components/alert';
import { CoinIcon } from '_components/coin-icon';
import Loading from '_components/loading';
import { filterAndSortTokenBalances } from '_helpers';
import { useAppSelector, useCoinsReFetchingConfig } from '_hooks';
import { ampli } from '_src/shared/analytics/ampli';
import { API_ENV } from '_src/shared/api-env';
import { FEATURES } from '_src/shared/experimentation/features';
import { AccountsList } from '_src/ui/app/components/accounts/AccountsList';
import { UnlockAccountButton } from '_src/ui/app/components/accounts/UnlockAccountButton';
import { useActiveAccount } from '_src/ui/app/hooks/useActiveAccount';
import { usePinnedCoinTypes } from '_src/ui/app/hooks/usePinnedCoinTypes';
import FaucetRequestButton from '_src/ui/app/shared/faucet/FaucetRequestButton';
import PageTitle from '_src/ui/app/shared/PageTitle';
import { useFeature } from '@growthbook/growthbook-react';
import { useAppsBackend, useFormatCoin, useResolveSuiNSName } from '@mysten/core';
import { useAllBalances, useBalance } from '@mysten/dapp-kit';
import { Info12, Pin16, Unpin16 } from '@mysten/icons';
import { type CoinBalance as CoinBalanceType } from '@mysten/sui.js/client';
import { Coin } from '@mysten/sui.js/framework';
import { formatAddress, MIST_PER_SUI, SUI_DECIMALS, SUI_TYPE_ARG } from '@mysten/sui.js/utils';
import { useQuery } from '@tanstack/react-query';
import BigNumber from 'bignumber.js';
import clsx from 'classnames';
import { useEffect, useMemo, useState } from 'react';

import Interstitial, { type InterstitialConfig } from '../interstitial';
import { useOnrampProviders } from '../onramp/useOnrampProviders';
import { CoinBalance } from './coin-balance';
import { PortfolioName } from './PortfolioName';
import { TokenIconLink } from './TokenIconLink';
import { TokenLink } from './TokenLink';
import { TokenList } from './TokenList';
import SvgSuiTokensStack from './TokensStackIcon';

type TokenDetailsProps = {
	coinType?: string;
};

function PinButton({ unpin, onClick }: { unpin?: boolean; onClick: () => void }) {
	return (
		<button
			type="button"
			className="border-none bg-transparent text-transparent group-hover/coin:text-steel hover:!text-hero cursor-pointer"
			aria-label={unpin ? 'Unpin Coin' : 'Pin Coin'}
			onClick={(e) => {
				e.preventDefault();
				e.stopPropagation();
				onClick();
			}}
		>
			{unpin ? <Unpin16 /> : <Pin16 />}
		</button>
	);
}

function TokenRowSuiToUSDC({ suiBalance }: { suiBalance: BigInt }) {
	const { rawValue } = useSuiBalanceInUSDC(
		new BigNumber(suiBalance.toString()).shiftedBy(-1 * SUI_DECIMALS),
	);

	return (
		<Text variant="subtitle" color="steel-dark" weight="medium">
			{getUSDCurrency(rawValue)}
		</Text>
	);
}

function TokenRowButton({
	coinBalance,
	page,
	copy,
}: {
	coinBalance: CoinBalanceType;
	page: 'send' | 'swap';
	copy: string;
}) {
	return (
		<ButtonOrLink
			to={getSendOrSwapUrl(page, coinBalance.coinType)}
			onClick={
				page === 'send'
					? () =>
							ampli.selectedCoin({
								coinType: coinBalance.coinType,
								totalBalance: Number(BigInt(coinBalance.totalBalance) / MIST_PER_SUI),
							})
					: undefined
			}
			key={coinBalance.coinType}
			className="no-underline text-subtitle font-medium text-steel hover:font-semibold hover:text-hero"
		>
			{copy}
		</ButtonOrLink>
	);
}

export function TokenRow({
	coinBalance,
	renderActions,
	onClick,
	as: Tag = 'div',
	borderBottom,
}: {
	coinBalance: CoinBalanceType;
	renderActions?: boolean;
	as?: 'div' | 'button';
	onClick?: () => void;
	borderBottom?: boolean;
}) {
	const coinType = coinBalance.coinType;
	const balance = BigInt(coinBalance.totalBalance);
	const [formatted, symbol] = useFormatCoin(balance, coinType);
	const isButton = Tag === 'button';
	const isSui = coinType === SUI_TYPE_ARG;

	return (
		<Tag
			className={clsx(
				'flex py-3 pl-1.5 pr-2 rounded hover:bg-sui/10 items-center border-solid border-t-transparent border-x-transparent',
				isButton && 'bg-transparent hover:cursor-pointer',
				borderBottom && 'border-b border-gray-45',
				!borderBottom && 'border-b-transparent',
			)}
			onClick={isButton ? onClick : undefined}
		>
			<div className="flex gap-2.5">
				<CoinIcon coinType={coinType} size="md" />

				<div className="flex flex-col gap-1 items-start">
					<Text variant="body" color="gray-90" weight="semibold">
						{symbol}
					</Text>

					{renderActions ? (
						<div className="flex gap-2.5 items-center">
							<TokenRowButton coinBalance={coinBalance} page="send" copy="Send" />
							<TokenRowButton coinBalance={coinBalance} page="swap" copy="Swap" />
						</div>
					) : (
						<div className="flex gap-1 items-center">
							<Text variant="subtitleSmall" weight="semibold" color="gray-90">
								{symbol}
							</Text>
							<Text variant="subtitleSmall" weight="medium" color="steel-dark">
								{formatAddress(coinType)}
							</Text>
						</div>
					)}
				</div>
			</div>

			<div className="ml-auto flex flex-col items-end gap-1.5">
				{formatted !== '0' && (
					<Text variant="body" color="gray-90" weight="medium">
						{formatted} {symbol}
					</Text>
				)}

				{isSui && <TokenRowSuiToUSDC suiBalance={balance} />}
			</div>
		</Tag>
	);
}

export function MyTokens({
	coinBalances,
	isLoading,
	isFetched,
}: {
	coinBalances: CoinBalanceType[];
	isLoading: boolean;
	isFetched: boolean;
}) {
	const isDefiWalletEnabled = useFeature<boolean>(FEATURES.WALLET_DEFI).value;
	const apiEnv = useAppSelector(({ app }) => app.apiEnv);

	const [_, { pinCoinType, unpinCoinType }] = usePinnedCoinTypes();

	const { recognized, pinned, unrecognized } = useSortedCoinsByCategories(coinBalances);

	// Avoid perpetual loading state when fetching and retry keeps failing; add isFetched check.
	const isFirstTimeLoading = isLoading && !isFetched;

	return (
		<Loading loading={isFirstTimeLoading}>
			{recognized.length > 0 && (
				<TokenList title="My Coins" defaultOpen>
					{recognized.map((coinBalance) =>
						isDefiWalletEnabled ? (
							<TokenRow renderActions key={coinBalance.coinType} coinBalance={coinBalance} />
						) : (
							<TokenLink key={coinBalance.coinType} coinBalance={coinBalance} />
						),
					)}
				</TokenList>
			)}

			{pinned.length > 0 && (
				<TokenList title="Pinned Coins" defaultOpen>
					{pinned.map((coinBalance) => (
						<TokenLink
							key={coinBalance.coinType}
							coinBalance={coinBalance}
							centerAction={
								<PinButton
									unpin
									onClick={() => {
										ampli.unpinnedCoin({ coinType: coinBalance.coinType });
										unpinCoinType(coinBalance.coinType);
									}}
								/>
							}
						/>
					))}
				</TokenList>
			)}

			{unrecognized.length > 0 && (
				<TokenList
					title={
						unrecognized.length === 1
							? `${unrecognized.length} Unrecognized Coin`
							: `${unrecognized.length} Unrecognized Coins`
					}
					defaultOpen={apiEnv !== API_ENV.mainnet}
				>
					{unrecognized.map((coinBalance) => (
						<TokenLink
							key={coinBalance.coinType}
							coinBalance={coinBalance}
							subtitle="Send"
							centerAction={
								<PinButton
									onClick={() => {
										ampli.pinnedCoin({ coinType: coinBalance.coinType });
										pinCoinType(coinBalance.coinType);
									}}
								/>
							}
						/>
					))}
				</TokenList>
			)}
		</Loading>
	);
}

function TokenDetails({ coinType }: TokenDetailsProps) {
	const isDefiWalletEnabled = useFeature<boolean>(FEATURES.WALLET_DEFI).value;
	const [interstitialDismissed, setInterstitialDismissed] = useState<boolean>(false);
	const activeCoinType = coinType || SUI_TYPE_ARG;
	const activeAccount = useActiveAccount();
	const activeAccountAddress = activeAccount?.address;
	const { data: domainName } = useResolveSuiNSName(activeAccountAddress);
	const { staleTime, refetchInterval } = useCoinsReFetchingConfig();
	const {
		data: coinBalance,
		isError,
		isLoading,
		isFetched,
	} = useBalance(
		{ coinType: activeCoinType, owner: activeAccountAddress! },
		{ enabled: !!activeAccountAddress, refetchInterval, staleTime },
	);

	const { apiEnv } = useAppSelector((state) => state.app);
	const { request } = useAppsBackend();
	const { data } = useQuery({
		queryKey: ['apps-backend', 'monitor-network'],
		queryFn: () =>
			request<{ degraded: boolean }>('monitor-network', {
				project: 'WALLET',
			}),
		// Keep cached for 2 minutes:
		staleTime: 2 * 60 * 1000,
		retry: false,
		enabled: apiEnv === API_ENV.mainnet,
	});

	const {
		data: coinBalances,
		isLoading: coinBalancesLoading,
		isFetched: coinBalancesFetched,
	} = useAllBalances(
		{ owner: activeAccountAddress! },
		{
			enabled: !!activeAccountAddress,
			staleTime,
			refetchInterval,
			select: filterAndSortTokenBalances,
		},
	);

	const walletInterstitialConfig = useFeature<InterstitialConfig>(
		FEATURES.WALLET_INTERSTITIAL_CONFIG,
	).value;

	const { providers } = useOnrampProviders();

	const tokenBalance = BigInt(coinBalance?.totalBalance ?? 0);

	const { rawValue: rawBalanceInUSD } = useSuiBalanceInUSDC(
		new BigNumber(tokenBalance.toString()).shiftedBy(-1 * SUI_DECIMALS),
	);
	const walletBalanceInUSD = getUSDCurrency(rawBalanceInUSD);

	const coinSymbol = useMemo(() => Coin.getCoinSymbol(activeCoinType), [activeCoinType]);
	// Avoid perpetual loading state when fetching and retry keeps failing add isFetched check
	const isFirstTimeLoading = isLoading && !isFetched;

	useEffect(() => {
		const dismissed =
			walletInterstitialConfig?.dismissKey &&
			localStorage.getItem(walletInterstitialConfig.dismissKey);
		setInterstitialDismissed(dismissed === 'true');
	}, [walletInterstitialConfig?.dismissKey]);

	if (walletInterstitialConfig?.enabled && !interstitialDismissed) {
		return (
			<Interstitial
				{...walletInterstitialConfig}
				onClose={() => {
					setInterstitialDismissed(true);
				}}
			/>
		);
	}
	const accountHasSui = coinBalances?.some(({ coinType }) => coinType === SUI_TYPE_ARG);
	if (!activeAccountAddress) {
		return null;
	}
	return (
		<>
			{apiEnv === API_ENV.mainnet && data?.degraded && (
				<div className="rounded-2xl bg-warning-light border border-solid border-warning-dark/20 text-warning-dark flex items-center py-2 px-3 mb-4">
					<Info12 className="shrink-0" />
					<div className="ml-2">
						<Text variant="pBodySmall" weight="medium">
							We're sorry that the app is running slower than usual. We're working to fix the issue
							and appreciate your patience.
						</Text>
					</div>
				</div>
			)}

			<Loading loading={isFirstTimeLoading}>
				{coinType && <PageTitle title={coinSymbol} back="/tokens" />}

				<div
					className="flex flex-col h-full flex-1 flex-grow items-center gap-8"
					data-testid="coin-page"
				>
					<AccountsList />
					<div className="flex flex-col w-full">
						<PortfolioName
							name={activeAccount.nickname ?? domainName ?? formatAddress(activeAccountAddress)}
						/>
						{activeAccount.isLocked ? null : (
							<>
								<div
									data-testid="coin-balance"
									className="bg-hero/5 rounded-2xl py-5 px-4 flex flex-col w-full gap-3 items-center mt-4"
								>
									{accountHasSui ? (
										<div className="flex flex-col gap-1 items-center">
											<CoinBalance amount={tokenBalance} type={activeCoinType} />
											{walletBalanceInUSD && isDefiWalletEnabled && (
												<Text variant="caption" weight="medium" color="hero-darkest">
													{walletBalanceInUSD}
												</Text>
											)}
										</div>
									) : (
										<div className="flex flex-col gap-5">
											<div className="flex flex-col flex-nowrap justify-center items-center text-center px-2.5">
												<SvgSuiTokensStack className="h-14 w-14 text-steel" />
												<div className="flex flex-col gap-2 justify-center">
													<Text variant="pBodySmall" color="gray-80" weight="normal">
														To conduct transactions on the Sui network, you need SUI in your wallet.
													</Text>
												</div>
											</div>
											<FaucetRequestButton />
										</div>
									)}
									{isError ? (
										<Alert>
											<div>
												<strong>Error updating balance</strong>
											</div>
										</Alert>
									) : null}
									<div className="grid grid-cols-3 gap-3 w-full">
										<LargeButton
											center
											to="/onramp"
											disabled={(coinType && coinType !== SUI_TYPE_ARG) || !providers?.length}
										>
											Buy
										</LargeButton>

										<LargeButton
											center
											data-testid="send-coin-button"
											to={`/send${
												coinBalance?.coinType
													? `?${new URLSearchParams({
															type: coinBalance.coinType,
													  }).toString()}`
													: ''
											}`}
											disabled={!tokenBalance}
										>
											Send
										</LargeButton>

										<LargeButton
											center
											disabled={!isDefiWalletEnabled}
											to={getSendOrSwapUrl('swap', coinBalance?.coinType ?? '')}
										>
											Swap
										</LargeButton>
									</div>
									<div className="w-full">
										{activeCoinType === SUI_TYPE_ARG ? (
											<TokenIconLink
												disabled={!tokenBalance}
												accountAddress={activeAccountAddress}
											/>
										) : null}
									</div>
								</div>
							</>
						)}
					</div>
					{activeAccount.isLocked ? (
						<UnlockAccountButton account={activeAccount} />
					) : (
						<MyTokens
							coinBalances={coinBalances ?? []}
							isLoading={coinBalancesLoading}
							isFetched={coinBalancesFetched}
						/>
					)}
				</div>
			</Loading>
		</>
	);
}

export default TokenDetails;
