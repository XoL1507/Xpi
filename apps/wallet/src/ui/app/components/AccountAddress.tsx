// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { Copy12 } from '@mysten/icons';
import { formatAddress, type SuiAddress } from '@mysten/sui.js';

import { useActiveAddress } from '../hooks/useActiveAddress';
import { useCopyToClipboard } from '../hooks/useCopyToClipboard';
import { Text } from '../shared/text';

type AccountAddressProps = {
    copyable?: boolean;
    address?: SuiAddress;
};

export function AccountAddress({ copyable, address }: AccountAddressProps) {
    const activeAddress = useActiveAddress();
    const addressToShow = address || activeAddress;
    const copyCallback = useCopyToClipboard(addressToShow || '', {
        copySuccessMessage: 'Address copied',
    });

    return addressToShow ? (
        <div className="flex flex-nowrap flex-row items-center gap-1">
            <Text variant="bodySmall" weight="medium" color="sui-dark" mono>
                {formatAddress(addressToShow)}
            </Text>
            {copyable ? (
                <Copy12
                    className="text-steel cursor-pointer"
                    onClick={copyCallback}
                />
            ) : null}
        </div>
    ) : null;
}
