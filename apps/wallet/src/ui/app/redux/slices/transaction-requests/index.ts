// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import {
    fromB64,
    type SignerWithProvider,
    Transaction,
    type SignedMessage,
    type SignedTransaction,
} from '@mysten/sui.js';
import {
    createAsyncThunk,
    createEntityAdapter,
    createSlice,
} from '@reduxjs/toolkit';

import {
    AccountType,
    type SerializedAccount,
} from '_src/background/keyring/Account';
import { type LedgerSigner } from '_src/ui/app/LedgerSigner';

import type { SuiTransactionResponse } from '@mysten/sui.js';
import type { PayloadAction } from '@reduxjs/toolkit';
import type { ApprovalRequest } from '_payloads/transactions/ApprovalRequest';
import type { RootState } from '_redux/RootReducer';
import type { AppThunkConfig } from '_store/thunk-extras';

const txRequestsAdapter = createEntityAdapter<ApprovalRequest>({
    sortComparer: (a, b) => {
        const aDate = new Date(a.createdDate);
        const bDate = new Date(b.createdDate);
        return aDate.getTime() - bDate.getTime();
    },
});

export const respondToTransactionRequest = createAsyncThunk<
    {
        txRequestID: string;
        approved: boolean;
        txResponse: SuiTransactionResponse | null;
    },
    {
        txRequestID: string;
        approved: boolean;
        accountForTransaction: SerializedAccount;
        initializeLedgerSignerInstance: (
            derivationPath: string
        ) => Promise<LedgerSigner>;
    },
    AppThunkConfig
>(
    'respond-to-transaction-request',
    async (
        {
            txRequestID,
            approved,
            accountForTransaction,
            initializeLedgerSignerInstance,
        },
        { extra: { background, api }, getState }
    ) => {
        const state = getState();
        const txRequest = txRequestsSelectors.selectById(state, txRequestID);
        if (!txRequest) {
            throw new Error(`TransactionRequest ${txRequestID} not found`);
        }
        let txSigned: SignedTransaction | undefined = undefined;
        let txResult: SuiTransactionResponse | SignedMessage | undefined =
            undefined;
        let txResultError: string | undefined;
        if (approved) {
            let signer: SignerWithProvider | undefined;
            if (accountForTransaction.type === AccountType.LEDGER) {
                signer = await initializeLedgerSignerInstance(
                    accountForTransaction.derivationPath
                );
            } else {
                signer = api.getSignerInstance(
                    accountForTransaction,
                    background
                );
            }

            try {
                if (txRequest.tx.type === 'sign-message') {
                    txResult = await signer.signMessage({
                        message: fromB64(txRequest.tx.message),
                    });
                } else if (txRequest.tx.type === 'transaction') {
                    const tx = Transaction.from(txRequest.tx.data);
                    if (txRequest.tx.justSign) {
                        // Just a signing request, do not submit
                        txSigned = await signer.signTransaction({
                            transaction: tx,
                        });
                    } else {
                        txResult = await signer.signAndExecuteTransaction({
                            transaction: tx,
                            options: txRequest.tx.options,
                            requestType: txRequest.tx.requestType,
                        });
                    }
                } else {
                    throw new Error(
                        // eslint-disable-next-line @typescript-eslint/no-explicit-any
                        `Unexpected type: ${(txRequest.tx as any).type}`
                    );
                }
            } catch (e) {
                txResultError = (e as Error).message;
            }
        }
        background.sendTransactionRequestResponse(
            txRequestID,
            approved,
            txResult,
            txResultError,
            txSigned
        );
        return { txRequestID, approved: approved, txResponse: null };
    }
);

const slice = createSlice({
    name: 'transaction-requests',
    initialState: txRequestsAdapter.getInitialState({
        initialized: false,
    }),
    reducers: {
        setTransactionRequests: (
            state,
            { payload }: PayloadAction<ApprovalRequest[]>
        ) => {
            // eslint-disable-next-line @typescript-eslint/ban-ts-comment
            // @ts-ignore
            txRequestsAdapter.setAll(state, payload);
            state.initialized = true;
        },
    },
    extraReducers: (build) => {
        build.addCase(
            respondToTransactionRequest.fulfilled,
            (state, { payload }) => {
                const { txRequestID, approved: allowed, txResponse } = payload;
                txRequestsAdapter.updateOne(state, {
                    id: txRequestID,
                    changes: {
                        approved: allowed,
                        txResult: txResponse || undefined,
                    },
                });
            }
        );
    },
});

export default slice.reducer;

export const { setTransactionRequests } = slice.actions;

export const txRequestsSelectors = txRequestsAdapter.getSelectors(
    (state: RootState) => state.transactionRequests
);
