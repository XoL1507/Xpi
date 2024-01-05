// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { Text } from '_app/shared/text';

import type { ReactNode } from 'react';

export type CardItemProps = {
    title: ReactNode;
    value: ReactNode;
};

export function CardItem({ title, value }: CardItemProps) {
    return (
        <div
            className={
                'flex flex-col flex-nowrap max-w-full p-3.5 gap-1.5 flex-1 justify-center items-center'
            }
        >
            <Text variant="captionSmall" weight="semibold" color="steel-darker">
                {title}
            </Text>

            <div className="overflow-x-hidden text-ellipsis">{value}</div>
        </div>
    );
}
