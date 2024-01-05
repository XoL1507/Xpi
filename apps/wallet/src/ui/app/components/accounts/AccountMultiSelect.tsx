// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { type SerializedUIAccount } from '_src/background/accounts/Account';
import * as ToggleGroup from '@radix-ui/react-toggle-group';
import { useState } from 'react';

import { Button } from '../../shared/ButtonUI';
import { AccountMultiSelectItem } from './AccountMultiSelectItem';

type AccountMultiSelectProps = {
	accounts: SerializedUIAccount[];
	selectedAccountIDs: string[];
	onChange: (value: string[]) => void;
};

export function AccountMultiSelect({
	accounts,
	selectedAccountIDs,
	onChange,
}: AccountMultiSelectProps) {
	return (
		<ToggleGroup.Root
			value={selectedAccountIDs}
			onValueChange={onChange}
			type="multiple"
			className="flex flex-col gap-3"
		>
			{accounts.map((account) => (
				<AccountMultiSelectItem
					key={account.id}
					account={account}
					state={
						account.isLocked
							? 'disabled'
							: selectedAccountIDs.includes(account.id)
							? 'selected'
							: undefined
					}
				/>
			))}
		</ToggleGroup.Root>
	);
}

export function AccountMultiSelectWithControls({
	selectedAccountIDs: selectedAccountsFromProps,
	accounts,
	onChange: onChangeFromProps,
}: AccountMultiSelectProps) {
	const [selectedAccountIds, setSelectedAccountsIds] = useState(selectedAccountsFromProps);
	const onChange = (value: string[]) => {
		setSelectedAccountsIds(value);
		onChangeFromProps(value);
	};
	return (
		<div className="flex flex-col gap-3 [&>button]:border-none">
			<AccountMultiSelect
				selectedAccountIDs={selectedAccountIds}
				accounts={accounts}
				onChange={onChange}
			/>

			<Button
				onClick={() => {
					if (selectedAccountIds.length < accounts.length) {
						// select all accounts if not all are selected
						onChange(accounts.map((account) => account.id));
					} else {
						// deselect all accounts
						onChange([]);
					}
				}}
				variant="outline"
				size="xs"
				text={
					selectedAccountIds.length < accounts.length
						? 'Select All Accounts'
						: 'Deselect All Accounts'
				}
			/>
		</div>
	);
}
