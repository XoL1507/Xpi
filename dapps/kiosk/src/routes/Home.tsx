// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { useWalletKit } from '@mysten/wallet-kit';
import { KioskData } from '../components/Kiosk/KioskData';
import { Loading } from '../components/Base/Loading';
import { useOwnedKiosk } from '../hooks/kiosk';
import { WalletNotConnected } from '../components/Base/WalletNotConnected';
import { KioskCreation } from '../components/Kiosk/KioskCreation';

function Home() {
	const { currentAccount } = useWalletKit();

	const { data: ownedKiosk, isLoading, refetch: refetchOwnedKiosk } = useOwnedKiosk();

	// Return loading state.
	if (isLoading) return <Loading />;

	// Return wallet not connected state.
	if (!currentAccount?.address) return <WalletNotConnected />;

	// if the account doesn't have a kiosk.
	if (!ownedKiosk?.kioskId) return <KioskCreation onCreate={refetchOwnedKiosk} />;

	// kiosk management screen.
	return (
		<div className="container">
			{ownedKiosk?.kioskCap && currentAccount?.address && <KioskData />}
		</div>
	);
}

export default Home;
