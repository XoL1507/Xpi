// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { useGetObject } from '@mysten/core';
import { useParams } from 'react-router-dom';

import { ErrorBoundary } from '../../components/error-boundary/ErrorBoundary';
import { translate, type DataType } from './ObjectResultType';
import PkgView from './views/PkgView';
import { TokenView } from './views/TokenView';

import { Banner } from '~/ui/Banner';
import { LoadingSpinner } from '~/ui/LoadingSpinner';
import { PageHeader } from '~/ui/PageHeader';

const PACKAGE_TYPE_NAME = 'Move Package';

function Fail({ objID }: { objID: string | undefined }) {
	return (
		<Banner variant="error" spacing="lg" fullWidth>
			Data could not be extracted on the following specified object ID: {objID}
		</Banner>
	);
}

export function ObjectResult() {
	const { id: objID } = useParams();
	const { data, isLoading, isError, isFetched } = useGetObject(objID!);

	if (isLoading) {
		return (
			<div className="flex w-full items-center justify-center">
				<LoadingSpinner text="Loading data" />
			</div>
		);
	}

	if (isError) {
		return <Fail objID={objID} />;
	}

	// TODO: Handle status better NotExists, Deleted, Other
	if (data.error || (isFetched && !data)) {
		return <Fail objID={objID} />;
	}

	const resp = translate(data);
	const isPackage = resp.objType === PACKAGE_TYPE_NAME;

	return (
		<div className="mb-10">
			<PageHeader type={isPackage ? 'Package' : 'Object'} title={resp.id} />

			<ErrorBoundary>
				<div className="mt-10">
					{isPackage ? <PkgView data={resp} /> : <TokenView data={data} />}
				</div>
			</ErrorBoundary>
		</div>
	);
}

export type { DataType };
