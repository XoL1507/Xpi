// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { yupResolver } from '@hookform/resolvers/yup';
import { useForm, type SubmitHandler } from 'react-hook-form';
import { useNavigate } from 'react-router-dom';
import * as Yup from 'yup';
import { privateKeyValidation } from '../../helpers/validation/privateKeyValidation';
import { Form } from '../../shared/forms/Form';
import { TextAreaField } from '../../shared/forms/TextAreaField';
import { Button } from '_app/shared/ButtonUI';

const formSchema = Yup.object({
	privateKey: privateKeyValidation,
});

type FormValues = Yup.InferType<typeof formSchema>;

type ImportPrivateKeyFormProps = {
	onSubmit: SubmitHandler<FormValues>;
};

export function ImportPrivateKeyForm({ onSubmit }: ImportPrivateKeyFormProps) {
	const form = useForm({
		mode: 'onTouched',
		resolver: yupResolver(formSchema),
	});
	const {
		register,
		formState: { isSubmitting, isValid },
	} = form;
	const navigate = useNavigate();

	return (
		<Form className="flex flex-col h-full" form={form} onSubmit={onSubmit}>
			<TextAreaField label="Enter Private Key" rows={4} {...register('privateKey')} />
			<div className="flex gap-2.5 mt-auto">
				<Button variant="outline" size="tall" text="Cancel" onClick={() => navigate(-1)} />
				<Button
					type="submit"
					disabled={isSubmitting || !isValid}
					variant="primary"
					size="tall"
					loading={isSubmitting}
					text="Add Account"
				/>
			</div>
		</Form>
	);
}
