// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { SuiMainnet, SuiTestnet, SuiDevnet, SuiLocal, SuiCustomRpc } from '@mysten/icons';

import { API_ENV } from '_src/shared/api-env';

type LogoProps = {
	networkName?: API_ENV;
};

const networkLogos = {
	[API_ENV.mainnet]: SuiMainnet,
	[API_ENV.devNet]: SuiDevnet,
	[API_ENV.testNet]: SuiTestnet,
	[API_ENV.local]: SuiLocal,
	[API_ENV.customRPC]: SuiCustomRpc,
};

const Logo = ({ networkName }: LogoProps) => {
	const LogoComponent = networkName ? networkLogos[networkName] : networkLogos[API_ENV.mainnet];

	return <LogoComponent className="h-7 w-walletLogo" />;
};

export default Logo;
