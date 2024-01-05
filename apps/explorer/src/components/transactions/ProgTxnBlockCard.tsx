// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import clsx from 'clsx';
import { type ReactNode } from 'react';

import { ExpandableList, ExpandableListControl, ExpandableListItems } from '~/ui/ExpandableList';
import { CollapsibleCard } from '~/ui/collapsible/CollapsibleCard';

interface ProgrammableTxnBlockCardProps {
	items: ReactNode[];
	itemsLabel: string;
	defaultItemsToShow: number;
	noExpandableList?: boolean;
	count?: number;
	initialClose?: boolean;
}

export function ProgrammableTxnBlockCard({
	items,
	itemsLabel,
	defaultItemsToShow,
	noExpandableList,
	count,
	initialClose,
}: ProgrammableTxnBlockCardProps) {
	if (!items?.length) {
		return null;
	}

	const cardTitle = count ? `${count} ${itemsLabel}` : itemsLabel;

	return (
		<CollapsibleCard collapsible initialClose={initialClose} title={cardTitle}>
			<ExpandableList items={items} defaultItemsToShow={defaultItemsToShow} itemsLabel={itemsLabel}>
				<div
					className={clsx(
						'flex flex-col gap-6 overflow-y-auto',
						!noExpandableList && 'max-h-[300px]',
					)}
				>
					{noExpandableList ? <>{items}</> : <ExpandableListItems />}
				</div>

				{items.length > defaultItemsToShow && (
					<div className="mt-6">
						<ExpandableListControl />
					</div>
				)}
			</ExpandableList>
		</CollapsibleCard>
	);
}
