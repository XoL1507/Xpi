// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { formatAddress } from '@mysten/sui.js';
import { cva, cx } from 'class-variance-authority';

import { Heading } from '_app/shared/heading';
import Loading from '_components/loading';
import { NftImage, type NftImageProps } from '_components/nft-display/NftImage';
import { useGetNFTMeta, useFileExtensionType } from '_hooks';

import type { VariantProps } from 'class-variance-authority';

const nftDisplayCardStyles = cva('flex flex-nowrap items-center h-full', {
    variants: {
        animateHover: {
            true: 'group',
        },
        wideView: {
            true: 'bg-gray-40 p-2.5 rounded-lg gap-2.5 flex-row-reverse justify-between',
            false: 'flex-col',
        },
    },
    defaultVariants: {
        wideView: false,
    },
});

export interface NFTsProps extends VariantProps<typeof nftDisplayCardStyles> {
    objectId: string;
    showLabel?: boolean;
    size: NftImageProps['size'];
    borderRadius?: NftImageProps['borderRadius'];
}

export function NFTDisplayCard({
    objectId,
    showLabel,
    size,
    wideView,
    animateHover,
    borderRadius = 'md',
}: NFTsProps) {
    const { data: nftMeta, isLoading } = useGetNFTMeta(objectId);
    const nftName = nftMeta?.name || formatAddress(objectId);
    const nftImageUrl = nftMeta?.imageUrl || '';
    const fileExtensionType = useFileExtensionType(nftImageUrl);
    return (
        <div className={nftDisplayCardStyles({ animateHover, wideView })}>
            <Loading loading={isLoading}>
                <NftImage
                    name={nftName}
                    src={nftImageUrl}
                    title={nftMeta?.description || ''}
                    animateHover={true}
                    showLabel={!wideView}
                    borderRadius={borderRadius}
                    size={size}
                />
                {wideView && (
                    <div className="flex flex-col gap-1 flex-1 min-w-0 ml-1">
                        <Heading variant="heading6" color="gray-90" truncate>
                            {nftName}
                        </Heading>
                        <div className="text-gray-75 text-body font-medium">
                            {nftImageUrl ? (
                                `${fileExtensionType.name} ${fileExtensionType.type}`
                            ) : (
                                <span className="uppercase font-normal text-bodySmall">
                                    NO MEDIA
                                </span>
                            )}
                        </div>
                    </div>
                )}
                {showLabel && !wideView && (
                    <div
                        className={cx(
                            'flex-1 mt-2 text-steel-dark truncate overflow-hidden max-w-full',
                            animateHover
                                ? 'group-hover:text-black duration-200 ease-ease-in-out-cubic'
                                : ''
                        )}
                    >
                        {nftName}
                    </div>
                )}
            </Loading>
        </div>
    );
}
