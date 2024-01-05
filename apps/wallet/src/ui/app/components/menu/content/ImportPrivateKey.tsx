// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { ArrowRight16 } from '@mysten/icons';
import { type ExportedKeypair } from '@mysten/sui.js/cryptography';
import { toB64 } from '@mysten/sui.js/utils';
import { hexToBytes } from '@noble/hashes/utils';
import { useMutation } from '@tanstack/react-query';
import { ErrorMessage, Field, Form, Formik } from 'formik';
import { useState } from 'react';
import { toast } from 'react-hot-toast';
import { useNavigate } from 'react-router-dom';
import { object } from 'yup';

import { MenuLayout } from './MenuLayout';
import { PasswordInputDialog } from '../../PasswordInputDialog';
import Alert from '../../alert';
import { useNextMenuUrl } from '../hooks';
import { ampli } from '_src/shared/analytics/ampli';
import { deprecatedPrivateKeyValidation } from '_src/ui/app/helpers/validation/privateKeyValidation';
import { useBackgroundClient } from '_src/ui/app/hooks/useBackgroundClient';
import { Button } from '_src/ui/app/shared/ButtonUI';
import FieldLabel from '_src/ui/app/shared/field-label';

const validation = object({
	privateKey: deprecatedPrivateKeyValidation,
});

export function ImportPrivateKey() {
	const accountsUrl = useNextMenuUrl(true, `/accounts`);
	const backgroundClient = useBackgroundClient();
	const navigate = useNavigate();
	const [showPasswordDialog, setShowPasswordDialog] = useState(false);
	const [privateKey, setPrivateKey] = useState('');
	const importMutation = useMutation({
		mutationFn: async (password: string) => {
			const keyPair: ExportedKeypair = {
				schema: 'ED25519',
				privateKey: toB64(hexToBytes(privateKey)),
			};
			await backgroundClient.importPrivateKey(password, keyPair);
		},
		onSuccess: () => {
			ampli.addedAccounts({
				accountType: 'Imported',
				numberOfAccounts: 1,
			});
			toast.success('Account imported');
			navigate(accountsUrl);
		},
		onError: () => setShowPasswordDialog(false),
	});
	return showPasswordDialog ? (
		<div className="absolute inset-0 pb-8 px-2.5 flex flex-col z-10">
			<PasswordInputDialog
				title="Import Account"
				continueLabel="Import"
				onBackClicked={() => setShowPasswordDialog(false)}
				showBackButton
				onPasswordVerified={async (password) => {
					await importMutation.mutateAsync(password);
				}}
				background
				spacing
			/>
		</div>
	) : (
		<MenuLayout title="Import Existing Account" back={accountsUrl}>
			<Formik
				initialValues={{ privateKey }}
				onSubmit={async ({ privateKey: privateKeyInput }) => {
					setPrivateKey(validation.cast({ privateKey: privateKeyInput }).privateKey);
					setShowPasswordDialog(true);
				}}
				validationSchema={validation}
				validateOnMount
				enableReinitialize
			>
				{({ isSubmitting, isValid }) => (
					<Form className="flex flex-col gap-3 pt-2.5">
						<FieldLabel txt="Enter Private Key">
							<Field
								name="privateKey"
								className="shadow-button text-steel-dark font-medium text-pBody resize-none rounded-xl border border-solid border-steel p-3"
								component={'textarea'}
								rows="3"
								spellCheck="false"
								autoComplete="off"
								autoFocus
							/>
							<ErrorMessage render={(error) => <Alert>{error}</Alert>} name="privateKey" />
						</FieldLabel>
						<Button
							type="submit"
							size="tall"
							variant="primary"
							text="Continue"
							after={<ArrowRight16 />}
							disabled={!isValid}
							loading={isSubmitting}
						/>
					</Form>
				)}
			</Formik>
		</MenuLayout>
	);
}
