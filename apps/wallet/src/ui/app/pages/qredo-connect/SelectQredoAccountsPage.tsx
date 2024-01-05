// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { ArrowRight16 } from '@mysten/icons';
import { useEffect, useState } from 'react';
import { toast } from 'react-hot-toast';
import { useParams, useLocation, Navigate, useNavigate } from 'react-router-dom';

import { useBackgroundClient } from '../../hooks/useBackgroundClient';
import { Button } from '../../shared/ButtonUI';
import { SelectQredoAccountsSummaryCard } from './components/SelectQredoAccountsSummaryCard';
import { useQredoUIPendingRequest } from './hooks';
import { PasswordInputDialog } from '_components/menu/content/PasswordInputDialog';
import Overlay from '_components/overlay';
import { type Wallet } from '_src/shared/qredo-api';

export function SelectQredoAccountsPage() {
	const { id } = useParams();
	const { state } = useLocation();
	const navigate = useNavigate();
	const backgroundService = useBackgroundClient();
	const qredoRequestReviewed = !!state?.reviewed;
	const { data: qredoPendingRequest, isLoading: isQredoRequestLoading } =
		useQredoUIPendingRequest(id);
	// do not call the api if user has not clicked continue in Qredo Connect Info page
	const fetchAccountsEnabled =
		!isQredoRequestLoading && (!qredoPendingRequest || qredoRequestReviewed);

	const [selectedAccounts, setSelectedAccounts] = useState<Wallet[]>([]);
	const [showPassword, setShowPassword] = useState(false);
	const shouldCloseWindow = (!isQredoRequestLoading && !qredoPendingRequest) || !id;
	useEffect(() => {
		if (shouldCloseWindow) {
			window.close();
		}
	}, [shouldCloseWindow]);
	if (qredoPendingRequest && !qredoRequestReviewed) {
		return <Navigate to="../" replace relative="path" />;
	}
	if (shouldCloseWindow) {
		return null;
	}
	return (
		<>
			{showPassword ? (
				<div className="flex flex-1 pb-4">
					<PasswordInputDialog
						title="Import Accounts"
						continueLabel="Import"
						onBackClicked={() => setShowPassword(false)}
						onPasswordVerified={async (password) => {
							await backgroundService.acceptQredoConnection({
								qredoID: id,
								accounts: selectedAccounts,
								password,
							});
							toast.success(`Qredo account${selectedAccounts.length > 1 ? 's' : ''} added`);
							navigate('/tokens?menu=/accounts');
						}}
						background
						spacing
					/>
				</div>
			) : (
				<Overlay
					showModal
					title="Import Accounts"
					closeOverlay={() => {
						navigate(-1);
					}}
				>
					<div className="flex flex-col flex-1 flex-nowrap align-top overflow-x-hidden overflow-y-auto gap-3">
						<div className="flex flex-1 overflow-hidden">
							<SelectQredoAccountsSummaryCard
								fetchAccountsEnabled={fetchAccountsEnabled}
								qredoID={id}
								selectedAccounts={selectedAccounts}
								onChange={setSelectedAccounts}
							/>
						</div>
						<div>
							<Button
								size="tall"
								variant="primary"
								text="Continue"
								after={<ArrowRight16 />}
								disabled={!selectedAccounts?.length}
								onClick={() => {
									setShowPassword(true);
								}}
							/>
						</div>
					</div>
				</Overlay>
			)}
		</>
	);
}
