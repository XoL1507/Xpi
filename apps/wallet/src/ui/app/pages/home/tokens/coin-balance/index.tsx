// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { useFormatCoin } from '@mysten/core';
import cl from 'classnames';
import { memo, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';

import { CoinItem } from '_components/active-coins-card/CoinItem';

import st from './CoinBalance.module.scss';

export type CoinProps = {
	type: string;
	balance: bigint;
	hideStake?: boolean;
	mode?: 'row-item' | 'standalone';
};

function CoinBalance({ type, balance, mode = 'row-item' }: CoinProps) {
	const [formatted, symbol] = useFormatCoin(balance, type);
	const navigate = useNavigate();

	// TODO: use a different logic to differentiate between view types
	const coinDetail = useCallback(() => {
		if (mode !== 'row-item') return;

		navigate(`/send?type=${encodeURIComponent(type)}`);
	}, [mode, navigate, type]);

	return (
		<div
			className={cl(st.container, st[mode], mode === 'row-item' && st.coinBalanceBtn)}
			onClick={coinDetail}
			role="button"
		>
			{mode === 'row-item' ? <CoinItem coinType={type} balance={balance} /> : null}
			<div className={cl(st.valueContainer, st[mode])}>
				<span className={cl(st.value, st[mode])}>{formatted}</span>
				<span className={cl(st.symbol, st[mode])}>{symbol}</span>
			</div>
		</div>
	);
}

export default memo(CoinBalance);
