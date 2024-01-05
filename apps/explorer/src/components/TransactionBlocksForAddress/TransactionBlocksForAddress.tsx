// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import {
    type TransactionFilter,
    type PaginatedTransactionResponse,
} from '@mysten/sui.js';
import { type InfiniteData } from '@tanstack/react-query';
import { useEffect, useReducer, useState } from 'react';

import { genTableDataFromTxData } from '../transactions/TxCardUtils';

import {
    DEFAULT_TRANSACTIONS_LIMIT,
    useGetTransactionBlocksForAddress,
} from '~/hooks/useGetTransactionBlocksForAddress';
import { Heading } from '~/ui/Heading';
import { Pagination } from '~/ui/Pagination';
import { PlaceholderTable } from '~/ui/PlaceholderTable';
import { RadioGroup, RadioOption } from '~/ui/Radio';
import { TableCard } from '~/ui/TableCard';

type TransactionBlocksForAddressProps = {
    address: string;
    isObject?: boolean;
};

enum PAGE_ACTIONS {
    NEXT,
    PREV,
    FIRST,
}

enum FILTER_VALUES {
    UNFILTERED = 'Unfiltered',
    INPUT = 'InputObject',
    CHANGED = 'ChangedObject',
}

type TransactionBlocksForAddressActionType = {
    type: PAGE_ACTIONS;
    filterValue: FILTER_VALUES;
};

type PageStateByFilterMap = {
    Unfiltered: number;
    InputObject: number;
    ChangedObject: number;
};

const FILTER_OPTIONS = [
    { label: 'Input Objects', value: 'InputObject' },
    { label: 'Updated Objects', value: 'ChangedObject' },
];

const reducer = (
    state: PageStateByFilterMap,
    action: TransactionBlocksForAddressActionType
) => {
    switch (action.type) {
        case PAGE_ACTIONS.NEXT:
            return {
                ...state,
                [action.filterValue]: state[action.filterValue] + 1,
            };
        case PAGE_ACTIONS.PREV:
            return {
                ...state,
                [action.filterValue]: state[action.filterValue] - 1,
            };
        case PAGE_ACTIONS.FIRST:
            return {
                ...state,
                [action.filterValue]: 0,
            };
        default:
            return { ...state };
    }
};

function TransactionBlocksForAddress({
    address,
    isObject = false,
}: TransactionBlocksForAddressProps) {
    const [filterValue, setFilterValue] = useState(FILTER_VALUES.UNFILTERED);
    const [currentPageState, dispatch] = useReducer(reducer, {
        Unfiltered: 0,
        InputObject: 0,
        ChangedObject: 0,
    });

    useEffect(() => {
        if (isObject) {
            setFilterValue(FILTER_VALUES.CHANGED);
        }
    }, [isObject]);

    const {
        data,
        isLoading,
        isFetching,
        isFetchingNextPage,
        fetchNextPage,
        hasNextPage,
    } = useGetTransactionBlocksForAddress(
        address,
        filterValue !== FILTER_VALUES.UNFILTERED
            ? ({
                  [filterValue]: address,
              } as TransactionFilter)
            : undefined
    );

    const generateTableCard = (
        currentPage: number,
        data?: InfiniteData<PaginatedTransactionResponse>
    ) => {
        if (!data) {
            return;
        }
        const cardData = genTableDataFromTxData(data?.pages[currentPage].data);
        return <TableCard data={cardData.data} columns={cardData.columns} />;
    };

    return (
        <div data-testid="tx">
            <div className="flex items-center justify-between border-b border-gray-45 pb-5">
                <Heading color="gray-90" variant="heading4/semibold">
                    Transaction Blocks
                </Heading>

                {isObject && (
                    <RadioGroup
                        className="flex"
                        ariaLabel="transaction filter"
                        value={filterValue}
                        onChange={setFilterValue}
                    >
                        {FILTER_OPTIONS.map((filter) => (
                            <RadioOption
                                key={filter.value}
                                value={filter.value}
                                label={filter.label}
                            />
                        ))}
                    </RadioGroup>
                )}
            </div>

            <div className="flex flex-col space-y-5 pt-5 text-left xl:pr-10">
                {isLoading || isFetching || isFetchingNextPage ? (
                    <PlaceholderTable
                        rowCount={DEFAULT_TRANSACTIONS_LIMIT}
                        rowHeight="16px"
                        colHeadings={[
                            'Digest',
                            'Sender',
                            'Txns',
                            'Gas',
                            'Time',
                        ]}
                        colWidths={['30%', '30%', '10%', '20%', '10%']}
                    />
                ) : (
                    <div>
                        {generateTableCard(currentPageState[filterValue], data)}
                    </div>
                )}

                {(hasNextPage || (data && data?.pages.length > 1)) && (
                    <Pagination
                        onNext={() => {
                            if (isLoading || isFetching) {
                                return;
                            }

                            // Make sure we are at the end before fetching another page
                            if (
                                data &&
                                currentPageState[filterValue] ===
                                    data?.pages.length - 1 &&
                                !isLoading &&
                                !isFetching
                            ) {
                                fetchNextPage();
                            }
                            dispatch({
                                type: PAGE_ACTIONS.NEXT,

                                filterValue,
                            });
                            // setCurrentPage(currentPage + 1);
                        }}
                        hasNext={Boolean(hasNextPage)}
                        hasPrev={currentPageState[filterValue] !== 0}
                        onPrev={() =>
                            dispatch({
                                type: PAGE_ACTIONS.PREV,

                                filterValue,
                            })
                        }
                        onFirst={() =>
                            dispatch({
                                type: PAGE_ACTIONS.FIRST,
                                filterValue,
                            })
                        }
                    />
                )}
            </div>
        </div>
    );
}

export default TransactionBlocksForAddress;
