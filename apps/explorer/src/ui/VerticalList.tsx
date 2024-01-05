// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import clsx from 'clsx';
import { type ReactNode } from 'react';

export interface ListItemProps {
    active?: boolean;
    children: ReactNode;
    onClick?(): void;
}

export function ListItem({ active, children, onClick }: ListItemProps) {
    return (
        <li className="list-none">
            <button
                type="button"
                className={clsx(
                    'block w-full cursor-pointer rounded-md border py-2 px-2.5 text-left text-body',
                    active
                        ? 'border-solid border-gray-50 bg-gray-45 font-semibold text-gray-90 shadow-sm'
                        : 'border-transparent bg-white font-medium text-gray-80'
                )}
                onClick={onClick}
            >
                {children}
            </button>
        </li>
    );
}

export interface VerticalListProps {
    children: ReactNode;
}

export function VerticalList({ children }: VerticalListProps) {
    return <ul className="m-0 flex flex-col gap-1 p-0">{children}</ul>;
}
