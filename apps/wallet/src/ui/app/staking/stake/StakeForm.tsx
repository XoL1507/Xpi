// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { SUI_TYPE_ARG } from '@mysten/sui.js';
import { ErrorMessage, Field, Form, useFormikContext } from 'formik';
import { useEffect, useRef, memo, useCallback, useMemo } from 'react';

import { Content } from '_app/shared/bottom-menu-layout';
import { Card } from '_app/shared/card';
import { Text } from '_app/shared/text';
import Alert from '_components/alert';
import NumberInput from '_components/number-input';
import { useCoinDecimals, useFormatCoin } from '_hooks';
import { DEFAULT_GAS_BUDGET_FOR_STAKE } from '_redux/slices/sui-objects/Coin';

import type { FormValues } from './StakingCard';

import st from './StakeForm.module.scss';

export type StakeFromProps = {
    submitError: string | null;
    coinBalance: bigint;
    coinType: string;
    unstake: boolean;
    onClearSubmitError: () => void;
};

function StakeForm({
    submitError,
    coinBalance,
    unstake,
    coinType,
    onClearSubmitError,
}: StakeFromProps) {
    const {
        setFieldValue,
        values: { amount },
    } = useFormikContext<FormValues>();

    const onClearRef = useRef(onClearSubmitError);
    onClearRef.current = onClearSubmitError;
    useEffect(() => {
        onClearRef.current();
    }, [amount]);

    const [gasBudgetEstimation, symbol] = useFormatCoin(
        DEFAULT_GAS_BUDGET_FOR_STAKE,
        SUI_TYPE_ARG
    );

    const coinBalanceMinusGas =
        coinBalance -
        BigInt(coinType === SUI_TYPE_ARG ? DEFAULT_GAS_BUDGET_FOR_STAKE : 0);

    const maxTokenFormated = useFormatCoin(coinBalanceMinusGas, coinType);

    const maxToken = maxTokenFormated[0];
    const queryResult = maxTokenFormated[2];
    const [coinDecimals] = useCoinDecimals(coinType);

    const setMaxToken = useCallback(() => {
        if (!maxToken) return;
        setFieldValue('amount', maxToken);
    }, [maxToken, setFieldValue]);

    const calculateRemaining = useMemo(() => {
        if (!amount || !maxToken) return 0;
        return (+maxToken - +amount).toFixed(coinDecimals);
    }, [amount, maxToken, coinDecimals]);

    return (
        <Form
            className="flex flex-1 flex-col flex-nowrap"
            autoComplete="off"
            noValidate={true}
        >
            <Content>
                <Card
                    variant="blue"
                    titleDivider
                    header={
                        <div className="p-2.5 w-full flex bg-white">
                            <Field
                                component={NumberInput}
                                allowNegative={false}
                                name="amount"
                                className="w-full border-none text-hero-dark text-heading4 font-semibold placeholder:text-gray-70 placeholder:font-medium"
                                decimals
                            />
                            <button
                                className="bg-white border border-solid border-gray-60 hover:border-steel-dark rounded-2xl h-6 w-11 flex justify-center items-center cursor-pointer text-steel-darker hover:text-steel-darker text-bodySmall font-medium disabled:opacity-50 disabled:cursor-auto"
                                onClick={setMaxToken}
                                disabled={queryResult.isLoading}
                                type="button"
                            >
                                Max
                            </button>
                        </div>
                    }
                    footer={
                        <div className="py-px flex justify-between w-full">
                            <Text
                                variant="body"
                                weight="medium"
                                color="steel-darker"
                            >
                                Gas Fees
                            </Text>
                            <Text
                                variant="body"
                                weight="medium"
                                color="steel-darker"
                            >
                                {gasBudgetEstimation} {symbol}
                            </Text>
                        </div>
                    }
                >
                    {+amount > 0 && (
                        <div className="py-px flex justify-between w-full">
                            <Text
                                variant="body"
                                weight="medium"
                                color="steel-darker"
                            >
                                Stake Remaining
                            </Text>
                            <Text
                                variant="body"
                                weight="medium"
                                color="steel-darker"
                            >
                                {calculateRemaining} {symbol}
                            </Text>
                        </div>
                    )}
                </Card>
                <ErrorMessage
                    className={st.error}
                    name="amount"
                    component="div"
                />

                {submitError ? (
                    <div className="mt-2 flex flex-col flex-nowrap">
                        <Alert mode="warning">
                            <strong>
                                {unstake ? 'Unstake failed' : 'Stake failed'}.
                            </strong>
                            <small>{submitError}</small>
                        </Alert>
                    </div>
                ) : null}
            </Content>
        </Form>
    );
}

export default memo(StakeForm);
