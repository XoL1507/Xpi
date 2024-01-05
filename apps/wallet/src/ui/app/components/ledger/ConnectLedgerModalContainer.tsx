// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import toast from 'react-hot-toast';
import { useNavigate } from 'react-router-dom';

import { getLedgerConnectionErrorMessage } from '../../helpers/errorMessages';
import { useNextMenuUrl } from '../menu/hooks';
import { ConnectLedgerModal } from './ConnectLedgerModal';

export function ConnectLedgerModalContainer() {
	const navigate = useNavigate();
	const accountsUrl = useNextMenuUrl(true, '/accounts');
	const importLedgerAccountsUrl = useNextMenuUrl(true, '/import-ledger-accounts');

	return (
		<ConnectLedgerModal
			onClose={() => {
				navigate(accountsUrl);
			}}
			onError={(error) => {
				navigate(accountsUrl);
				toast.error(getLedgerConnectionErrorMessage(error) || 'Something went wrong.');
			}}
			onConfirm={() => {
				navigate(importLedgerAccountsUrl);
			}}
		/>
	);
}
