// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import clsx from 'clsx';

import { ReactComponent as InfoSvg } from './icons/info_10x10.svg';
import { Heading } from '~/ui/Heading';
import { Text } from '~/ui/Text';
import { Tooltip } from '~/ui/Tooltip';
import { ampli } from '~/utils/analytics/ampli';

import type { ReactNode } from 'react';

export type StatsProps = {
	size?: 'sm' | 'md';
	label: string;
	children?: ReactNode;
	tooltip?: string;
	unavailable?: boolean;
	postfix?: ReactNode;
	orientation?: 'horizontal' | 'vertical';
	color?: 'steel-darker' | 'hero';
};

export function Stats({
	label,
	children,
	tooltip,
	unavailable,
	postfix,
	size = 'md',
	orientation = 'vertical',
	color = 'steel-darker',
}: StatsProps) {
	return (
		<div
			className={clsx(
				'flex max-w-full flex-nowrap justify-between gap-1.5',
				orientation === 'horizontal' ? '' : 'flex-col',
			)}
		>
			<div className="flex items-center justify-start gap-1 overflow-hidden text-caption">
				<Text variant="caption/semibold" color={color} truncate>
					{label}
				</Text>
				{tooltip && (
					<Tooltip
						tip={unavailable ? 'Coming soon' : tooltip}
						onOpen={() => {
							ampli.activatedTooltip({ tooltipLabel: label });
						}}
					>
						<InfoSvg />
					</Tooltip>
				)}
			</div>
			<div className="flex items-baseline gap-0.5">
				<Heading
					variant={size === 'md' ? 'heading2/semibold' : 'heading3/semibold'}
					color={unavailable ? 'steel-darker' : color}
				>
					{unavailable || children == null ? '--' : children}
				</Heading>

				{postfix && (
					<Heading variant="heading4/medium" color={unavailable ? 'steel-darker' : color}>
						{postfix}
					</Heading>
				)}
			</div>
		</div>
	);
}
