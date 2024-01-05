// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { Link } from 'react-router-dom';

import PageTitle from '_app/shared/page-title';
import Alert from '_components/alert';
import { ErrorBoundary } from '_components/error-boundary';
import Loading from '_components/loading';
import NFTdisplay from '_components/nft-display';
import { useAppSelector, useObjectsState } from '_hooks';
import { accountNftsSelector } from '_redux/slices/account';

function NftsPage() {
    const nfts = useAppSelector(accountNftsSelector);
    const { error, loading, showError } = useObjectsState();
    const isNftsFound = nfts.length > 0;
    return (
        <div className="flex flex-col flex-nowrap items-center gap-4 flex-1">
            <Loading loading={loading}>
                <PageTitle title="NFTs" className="justify-center" />
                {showError && error ? (
                    <Alert>
                        <strong>Sync error (data might be outdated).</strong>{' '}
                        <small>{error.message}</small>
                    </Alert>
                ) : null}
                {isNftsFound ? (
                    <div className="grid grid-cols-2 gap-x-3.5 gap-y-4">
                        {nfts.map((nft) => (
                            <Link
                                to={`/nft-details?${new URLSearchParams({
                                    objectId: nft.reference.objectId,
                                }).toString()}`}
                                key={nft.reference.objectId}
                                className="no-underline"
                            >
                                <ErrorBoundary>
                                    <NFTdisplay
                                        nftobj={nft}
                                        size="md"
                                        showlabel
                                        animateHover
                                        borderRadius="xl"
                                    />
                                </ErrorBoundary>
                            </Link>
                        ))}
                    </div>
                ) : (
                    <div className="text-steel-darker font-semibold text-caption flex-1 self-center flex items-center">
                        No NFTs found
                    </div>
                )}
            </Loading>
        </div>
    );
}

export default NftsPage;
