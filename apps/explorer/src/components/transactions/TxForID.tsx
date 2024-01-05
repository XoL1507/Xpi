// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { useRpcClient } from '@mysten/core';
import {
    type SuiTransactionResponse,
    type GetTxnDigestsResponse,
} from '@mysten/sui.js';
import { useState, useEffect, useContext } from 'react';

import { NetworkContext } from '../../context';
import { DefaultRpcClient as rpc } from '../../utils/api/DefaultRpcClient';
import PaginationLogic from '../pagination/PaginationLogic';
import { genTableDataFromTxData, getDataOnTxDigests } from './TxCardUtils';

import { Banner } from '~/ui/Banner';
import { TableCard } from '~/ui/TableCard';

const ITEMS_PER_PAGE = 20;

const DATATYPE_DEFAULT = {
    loadState: 'pending',
};

type categoryType = 'address' | 'object';

const getTx = async (
    id: string,
    network: string,
    category: categoryType
): Promise<GetTxnDigestsResponse> =>
    category === 'address'
        ? rpc(network).queryTransactionsForAddressDeprecated(id, true)
        : rpc(network).queryTransactionsForObjectDeprecated(id, true);

const viewFn = (results: any) => <TxForIDView showData={results} />;

function TxForIDView({
    showData,
}: {
    showData: SuiTransactionResponse[] | undefined;
}) {
    if (!showData || showData.length === 0) return null;

    const tableData = genTableDataFromTxData(showData);

    return (
        <div data-testid="tx">
            <TableCard data={tableData.data} columns={tableData.columns} />
        </div>
    );
}

function TxForID({ id, category }: { id: string; category: categoryType }) {
    const [showData, setData] = useState<{
        data?: SuiTransactionResponse[];
        loadState: string;
    }>(DATATYPE_DEFAULT);
    const [network] = useContext(NetworkContext);
    const rpc = useRpcClient();
    useEffect(() => {
        getTx(id, network, category).then(
            (transactions) => {
                //If the API method does not exist, the transactions will be undefined
                if (!transactions?.[0]) {
                    setData({
                        loadState: 'loaded',
                    });
                } else {
                    getDataOnTxDigests(rpc, transactions)
                        .then((data) => {
                            setData({
                                data,
                                loadState: 'loaded',
                            });
                        })
                        .catch((error) => {
                            console.log(error);
                            setData({ ...DATATYPE_DEFAULT, loadState: 'fail' });
                        });
                }
            },
            () => {}
        );
    }, [id, network, rpc, category]);

    if (showData.loadState === 'pending') {
        return <div>Loading ...</div>;
    }

    if (showData.loadState === 'loaded') {
        const data = showData.data;
        if (!data) return null;
        return (
            <PaginationLogic
                results={data}
                viewComponentFn={viewFn}
                itemsPerPage={ITEMS_PER_PAGE}
                canVaryItemsPerPage
            />
        );
    }

    return (
        <Banner variant="error" fullWidth>
            Transactions could not be extracted on the following specified ID:{' '}
            {id}
        </Banner>
    );
}

export default TxForID;
