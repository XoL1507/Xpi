// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { LedgerLogo17 as LedgerLogo } from '@mysten/icons';
import { useState, type ReactNode, useEffect, useRef, useCallback } from 'react';
import toast from 'react-hot-toast';
import { useNavigate, useSearchParams } from 'react-router-dom';
import Browser from 'webextension-polyfill';
import { useAccountsFormContext } from '../../components/accounts/AccountsFormContext';
import { ZkLoginButtons } from '../../components/accounts/ZkLoginButtons';
import { ConnectLedgerModal } from '../../components/ledger/ConnectLedgerModal';
import { getLedgerConnectionErrorMessage } from '../../helpers/errorMessages';
import { useAppSelector } from '../../hooks';
import { useCountAccountsByType } from '../../hooks/useCountAccountByType';
import { useCreateAccountsMutation } from '../../hooks/useCreateAccountMutation';
import { AppType } from '../../redux/slices/app/AppType';
import { Button } from '_app/shared/ButtonUI';
import { Text } from '_app/shared/text';
import Overlay from '_components/overlay';
import { type ZkProvider, zkProviderDataMap } from '_src/background/accounts/zk/providers';
import { ampli } from '_src/shared/analytics/ampli';

async function openTabWithSearchParam(searchParam: string, searchParamValue: string) {
	const currentURL = new URL(window.location.href);
	const [currentHash, currentHashSearch] = currentURL.hash.split('?');
	const urlSearchParams = new URLSearchParams(currentHashSearch);
	urlSearchParams.set(searchParam, searchParamValue);
	currentURL.hash = `${currentHash}?${urlSearchParams.toString()}`;
	currentURL.searchParams.delete('type');
	await Browser.tabs.create({
		url: currentURL.href,
	});
}

export function AddAccountPage() {
	const [searchParams, setSearchParams] = useSearchParams();
	const navigate = useNavigate();
	const sourceFlow = searchParams.get('sourceFlow') || 'Unknown';
	const showSocialSignInOptions = sourceFlow !== 'Onboarding';
	const forceShowLedger =
		searchParams.has('showLedger') && searchParams.get('showLedger') !== 'false';
	const [, setAccountsFormValues] = useAccountsFormContext();
	const isPopup = useAppSelector((state) => state.app.appType === AppType.popup);
	const [isConnectLedgerModalOpen, setConnectLedgerModalOpen] = useState(forceShowLedger);
	const createAccountsMutation = useCreateAccountsMutation();
	const createZkLoginAccount = useCallback(
		async (provider: ZkProvider) => {
			await setAccountsFormValues({ type: 'zk', provider });
			await createAccountsMutation.mutateAsync(
				{
					type: 'zk',
				},
				{
					onSuccess: () => {
						navigate('/tokens');
					},
					onError: (error) => {
						toast.error((error as Error)?.message || 'Failed to create account. (Unknown error)');
					},
				},
			);
		},
		[setAccountsFormValues, createAccountsMutation, navigate],
	);
	const [forcedZkLoginProvider, setForcedZkLoginProvider] = useState<ZkProvider | null>(null);
	const forceZkLoginWithProviderRef = useRef(searchParams.get('forceZkLoginProvider'));
	const forcedLoginHandledRef = useRef(false);
	const { data: accountsTotalByType, isLoading: isAccountsCountLoading } = useCountAccountsByType();
	useEffect(() => {
		if (isAccountsCountLoading) {
			return;
		}
		const zkLoginProvider = forceZkLoginWithProviderRef.current as ZkProvider;
		if (zkLoginProvider && zkProviderDataMap[zkLoginProvider] && !forcedLoginHandledRef.current) {
			const totalProviderAccounts = accountsTotalByType?.zk?.extra?.[zkLoginProvider] || 0;
			if (totalProviderAccounts === 0) {
				setForcedZkLoginProvider(zkLoginProvider);
				createZkLoginAccount(zkLoginProvider).finally(() => setForcedZkLoginProvider(null));
			}
			const newURLSearchParams = new URLSearchParams(searchParams.toString());
			newURLSearchParams.delete('forceZkLoginProvider');
			setSearchParams(newURLSearchParams.toString());
			forcedLoginHandledRef.current = true;
		}
	}, [
		setSearchParams,
		accountsTotalByType,
		searchParams,
		createZkLoginAccount,
		isAccountsCountLoading,
	]);
	return (
		<Overlay showModal title="Add Account" closeOverlay={() => navigate('/')}>
			<div className="w-full flex flex-col gap-8">
				<div className="flex flex-col gap-3">
					{showSocialSignInOptions && (
						<ZkLoginButtons
							layout="column"
							showLabel
							sourceFlow={sourceFlow}
							forcedZkLoginProvider={forcedZkLoginProvider}
							onButtonClick={async (provider) => {
								if (isPopup) {
									await openTabWithSearchParam('forceZkLoginProvider', provider);
									window.close();
									return;
								} else {
									return createZkLoginAccount(provider);
								}
							}}
						/>
					)}
					<Button
						variant="outline"
						size="tall"
						text="Set up Ledger"
						before={<LedgerLogo className="text-gray-90 w-4 h-4" />}
						onClick={async () => {
							ampli.openedConnectLedgerFlow({ sourceFlow });
							if (isPopup) {
								await openTabWithSearchParam('showLedger', 'true');
								window.close();
							} else {
								setConnectLedgerModalOpen(true);
							}
						}}
						disabled={createAccountsMutation.isLoading}
					/>
				</div>
				<Section title="Create New">
					<Button
						variant="outline"
						size="tall"
						text="Create a new Passphrase Account"
						to="/accounts/protect-account?accountType=new-mnemonic"
						onClick={() => {
							setAccountsFormValues({ type: 'new-mnemonic' });
							ampli.clickedCreateNewAccount({ sourceFlow });
						}}
						disabled={createAccountsMutation.isLoading}
					/>
				</Section>
				<Section title="Import Existing Accounts">
					<Button
						variant="outline"
						size="tall"
						text="Import Passphrase"
						to="/accounts/import-passphrase"
						onClick={() => {
							ampli.clickedImportPassphrase({ sourceFlow });
						}}
						disabled={createAccountsMutation.isLoading}
					/>
					<Button
						variant="outline"
						size="tall"
						text="Import Private Key"
						to="/accounts/import-private-key"
						onClick={() => {
							ampli.clickedImportPrivateKey({ sourceFlow });
						}}
						disabled={createAccountsMutation.isLoading}
					/>
				</Section>
			</div>
			{isConnectLedgerModalOpen && (
				<ConnectLedgerModal
					onClose={() => {
						setConnectLedgerModalOpen(false);
					}}
					onError={(error) => {
						setConnectLedgerModalOpen(false);
						toast.error(getLedgerConnectionErrorMessage(error) || 'Something went wrong.');
					}}
					onConfirm={() => {
						ampli.connectedHardwareWallet({ hardwareWalletType: 'Ledger' });
						navigate('/accounts/import-ledger-accounts');
					}}
				/>
			)}
		</Overlay>
	);
}

type SectionProps = {
	title: string;
	children: ReactNode;
};

function Section({ title, children }: SectionProps) {
	return (
		<section className="flex flex-col gap-3">
			<div className="flex items-center gap-2">
				<div className="grow border-0 border-t border-solid border-gray-40"></div>
				<Text variant="caption" weight="semibold" color="steel">
					{title}
				</Text>
				<div className="grow border-0 border-t border-solid border-gray-40"></div>
			</div>
			{children}
		</section>
	);
}
