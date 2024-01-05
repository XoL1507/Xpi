// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { useGetDynamicFields, useGetObject } from '@mysten/core';
import { useNormalizedMoveStruct } from '@mysten/dapp-kit';
import { type SuiObjectResponse } from '@mysten/sui.js/client';
import { Heading } from '@mysten/ui';
import { type ReactNode, useState } from 'react';

import { DynamicFieldsCard } from '~/components/Object/DynamicFieldsCard';
import { ObjectFieldsCard } from '~/components/Object/ObjectFieldsCard';
import TransactionBlocksForAddress from '~/components/TransactionBlocksForAddress/TransactionBlocksForAddress';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '~/ui/Tabs';

function FieldsContainer({ children }: { children: ReactNode }) {
	return (
		<div className="mt-4 flex flex-col gap-5 rounded-xl border border-gray-45 bg-objectCard py-6 pl-6 pr-4">
			{children}
		</div>
	);
}

enum TABS_VALUES {
	FIELDS = 'fields',
	DYNAMIC_FIELDS = 'dynamicFields',
}

function useObjectFieldsCard(id: string) {
	const { data: suiObjectResponseData, isLoading, isError } = useGetObject(id);

	const objectType =
		suiObjectResponseData?.data?.type ??
		suiObjectResponseData?.data?.content?.dataType === 'package'
			? suiObjectResponseData.data.type
			: suiObjectResponseData?.data?.content?.type;

	const [packageId, moduleName, functionName] = objectType?.split('<')[0]?.split('::') || [];

	// Get the normalized struct for the object
	const {
		data: normalizedStructData,
		isLoading: loadingNormalizedStruct,
		isError: errorNormalizedMoveStruct,
	} = useNormalizedMoveStruct(
		{
			package: packageId,
			module: moduleName,
			struct: functionName,
		},
		{
			enabled: !!packageId && !!moduleName && !!functionName,
		},
	);

	return {
		loading: isLoading || loadingNormalizedStruct,
		error: isError || errorNormalizedMoveStruct,
		normalizedStructData,
		suiObjectResponseData,
		objectType,
	};
}

export function TokenView({ data }: { data: SuiObjectResponse }) {
	const objectId = data.data?.objectId!;

	const {
		normalizedStructData,
		suiObjectResponseData,
		objectType,
		loading: objectFieldsCardLoading,
		error: objectFieldsCardError,
	} = useObjectFieldsCard(objectId);

	const fieldsCount = normalizedStructData?.fields.length;

	const [activeTab, setActiveTab] = useState<string>(TABS_VALUES.FIELDS);

	const { data: dynamicFieldsData } = useGetDynamicFields(objectId);

	const renderDynamicFields = !!dynamicFieldsData?.pages?.[0].data.length;

	return (
		<div className="flex flex-col flex-nowrap gap-14">
			<Tabs size="lg" value={activeTab} onValueChange={setActiveTab}>
				<TabsList>
					<TabsTrigger value={TABS_VALUES.FIELDS}>
						<Heading variant="heading4/semibold">{fieldsCount} Fields</Heading>
					</TabsTrigger>

					{renderDynamicFields && (
						<TabsTrigger value={TABS_VALUES.DYNAMIC_FIELDS}>
							<Heading variant="heading4/semibold">Dynamic Fields</Heading>
						</TabsTrigger>
					)}
				</TabsList>

				<TabsContent value={TABS_VALUES.FIELDS}>
					<FieldsContainer>
						<ObjectFieldsCard
							objectType={objectType || ''}
							normalizedStructData={normalizedStructData}
							suiObjectResponseData={suiObjectResponseData}
							loading={objectFieldsCardLoading}
							error={objectFieldsCardError}
							id={objectId}
						/>
					</FieldsContainer>
				</TabsContent>
				{renderDynamicFields && (
					<TabsContent value={TABS_VALUES.DYNAMIC_FIELDS}>
						<FieldsContainer>
							<DynamicFieldsCard id={objectId} />
						</FieldsContainer>
					</TabsContent>
				)}
			</Tabs>

			<TransactionBlocksForAddress address={objectId} isObject />
		</div>
	);
}
