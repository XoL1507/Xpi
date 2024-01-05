// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { ArrowRight16 } from '@mysten/icons';
import { SUI_TYPE_ARG } from '@mysten/sui.js';
import { Field, Form, useFormikContext } from 'formik';
import { useEffect, useRef, useCallback } from 'react';

import { Button } from '_app/shared/ButtonUI';
import BottomMenuLayout, {
    Content,
    Menu,
} from '_app/shared/bottom-menu-layout';
import { Text } from '_app/shared/text';
import { IconTooltip } from '_app/shared/tooltip';
import AddressInput from '_components/address-input';
import Alert from '_components/alert';
import { parseAmount } from '_helpers';
import { useCoinDecimals, useFormatCoin } from '_hooks';
import { GAS_SYMBOL, GAS_TYPE_ARG } from '_redux/slices/sui-objects/Coin';
import { InputWithAction } from '_src/ui/app/shared/InputWithAction';

import type { FormValues } from '../';

export type TransferCoinFormProps = {
    submitError: string | null;
    coinType: string;
    gasCostEstimation: number | null;
    gasEstimationLoading?: boolean;
    onClearSubmitError: () => void;
    onAmountChanged: (amount: bigint) => void;
    balance: bigint | null;
};

//TODO: update the form input to use input with action component
export function StepOne({
    submitError,
    coinType,
    onClearSubmitError,
    onAmountChanged,
    gasCostEstimation,
    gasEstimationLoading,
    balance,
}: TransferCoinFormProps) {
    const {
        validateForm,
        values: { amount, to, isPayAllSui },
        setFieldValue,
        isValid,
        isSubmitting,
        submitForm,
    } = useFormikContext<FormValues>();
    const onClearRef = useRef(onClearSubmitError);
    onClearRef.current = onClearSubmitError;
    useEffect(() => {
        onClearRef.current();
    }, [amount, to]);

    const [coinDecimals, { isLoading: isCoinDecimalsLoading }] =
        useCoinDecimals(coinType);

    useEffect(() => {
        if (!isCoinDecimalsLoading) {
            const parsedAmount = parseAmount(amount, coinDecimals);
            onAmountChanged(parsedAmount);
            validateForm();
        }
    }, [
        amount,
        onAmountChanged,
        coinDecimals,
        isCoinDecimalsLoading,
        validateForm,
    ]);

    const [formattedGas] = useFormatCoin(gasCostEstimation, GAS_TYPE_ARG);
    const [maxToken, symbol, queryResult] = useFormatCoin(balance, coinType);

    // set sendMaxToken to true if the amount is equal to the maxToken
    useEffect(() => {
        // reset isPayAllSui to false if the amount is not equal to the maxToken
        if (amount !== maxToken) {
            setFieldValue('isPayAllSui', false);
        }
    }, [maxToken, setFieldValue, amount]);

    const setMaxToken = useCallback(() => {
        if (!maxToken) return;
        // for SUI coin type, set the amount to be the formatted maxToken
        // while for other coin type, set the amount to be the raw maxToken
        const maxAmount =
            coinType === SUI_TYPE_ARG ? maxToken : balance?.toString();
        setFieldValue('amount', maxAmount);
        if (coinType === SUI_TYPE_ARG) {
            setFieldValue('isPayAllSui', true);
        }
    }, [balance, coinType, maxToken, setFieldValue]);

    return (
        <BottomMenuLayout>
            <Content>
                <Form autoComplete="off" noValidate>
                    <div className="w-full flex gap-2.5 flex-col flex-grow">
                        <div className="px-2">
                            <Text
                                variant="captionSmall"
                                color="steel-dark"
                                weight="semibold"
                            >
                                Select SUI Amount to Send
                            </Text>
                        </div>
                        <InputWithAction
                            name="amount"
                            placeholder="0.00"
                            prefix={isPayAllSui ? '~ ' : ''}
                            actionText="Max"
                            suffix={` ${symbol}`}
                            type="number"
                            actionType="button"
                            allowNegative={false}
                            onActionClicked={setMaxToken}
                            actionDisabled={
                                maxToken === amount ||
                                queryResult.isLoading ||
                                !maxToken
                            }
                        />
                    </div>
                    <div className="px-2 mt-3 mb-5 flex w-full gap-2 justify-between">
                        <div className="flex gap-1">
                            <Text
                                variant="body"
                                color="gray-80"
                                weight="medium"
                            >
                                Estimated Gas Fees
                            </Text>
                            <div className="text-gray-60 h-4 items-end flex">
                                <IconTooltip
                                    tip="Estimated Gas Fees"
                                    placement="top"
                                />
                            </div>
                        </div>
                        <Text variant="body" color="gray-90" weight="medium">
                            {formattedGas} {GAS_SYMBOL}
                        </Text>
                    </div>
                    <div className="w-full flex gap-2.5 flex-col mt-7.5">
                        <div className="px-2 tracking-wider">
                            <Text
                                variant="caption"
                                color="steel-dark"
                                weight="semibold"
                            >
                                Enter Recipient Address
                            </Text>
                        </div>
                        <div className="w-full flex relative items-center flex-col">
                            <Field
                                component={AddressInput}
                                allowNegative={false}
                                name="to"
                                placeholder="Enter Address"
                                className="w-full py-3.5 px-3 flex items-center rounded-2lg text-gray-90 text-bodySmall leading-130 font-medium font-mono bg-white placeholder:text-steel-dark placeholder:font-normal placeholder:font-mono border border-solid border-gray-45 box-border focus:border-steel transition-all"
                            />
                        </div>

                        {submitError ? (
                            <div className="mt-3 w-full">
                                <Alert>{submitError}</Alert>
                            </div>
                        ) : null}
                    </div>
                </Form>
            </Content>
            <Menu
                stuckClass="sendCoin-cta"
                className="w-full px-0 pb-0 mx-0 gap-2.5"
            >
                <Button
                    type="submit"
                    onClick={submitForm}
                    variant="primary"
                    loading={isSubmitting}
                    disabled={!isValid || isSubmitting}
                    size="tall"
                    text={'Review'}
                    after={<ArrowRight16 />}
                />
            </Menu>
        </BottomMenuLayout>
    );
}
