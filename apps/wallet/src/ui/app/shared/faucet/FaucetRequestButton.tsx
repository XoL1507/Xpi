// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { toast } from 'react-hot-toast';

import FaucetMessageInfo from './FaucetMessageInfo';
import { useFaucetMutation } from './useFaucetMutation';
import { API_ENV_TO_INFO } from '_app/ApiProvider';
import { Button, type ButtonProps } from '_app/shared/ButtonUI';
import { useAppSelector } from '_hooks';
import { trackEvent } from '_shared/plausible';

export type FaucetRequestButtonProps = {
    variant?: ButtonProps['variant'];
    trackEventSource: 'home' | 'settings';
};

function FaucetRequestButton({
    variant = 'primary',
    trackEventSource,
}: FaucetRequestButtonProps) {
    const network = useAppSelector(({ app }) => app.apiEnv);
    const networkName = API_ENV_TO_INFO[network].name.replace(/sui\s*/gi, '');
    const mutation = useFaucetMutation();

    return mutation.enabled ? (
        <Button
            variant={variant}
            onClick={() => {
                toast.promise(mutation.mutateAsync(), {
                    loading: <FaucetMessageInfo loading />,
                    success: (totalReceived) => (
                        <FaucetMessageInfo totalReceived={totalReceived} />
                    ),
                    error: (error) => (
                        <FaucetMessageInfo error={error.message} />
                    ),
                });
                trackEvent('RequestGas', {
                    props: { source: trackEventSource, networkName },
                });
            }}
            loading={mutation.isMutating}
            text={`Request ${networkName} SUI Tokens`}
        />
    ) : null;
}

export default FaucetRequestButton;
