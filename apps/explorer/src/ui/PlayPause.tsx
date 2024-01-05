// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import {
    AutorefreshPlay24 as PlayIcon,
    AutorefreshPause24 as PauseIcon,
} from '@mysten/icons';

export interface PlayPauseProps {
    paused?: boolean;
    onChange(): void;
}

// TODO: Create generalized `IconButton` component:
export function PlayPause({ paused, onChange }: PlayPauseProps) {
    return (
        <button
            type="button"
            aria-label={paused ? 'Paused' : 'Playing'}
            onClick={onChange}
            className="cursor-pointer border-none bg-transparent text-steel hover:text-steel-darker"
        >
            {paused ? (
                <PlayIcon height="24px" width="24px" />
            ) : (
                <PauseIcon height="24px" width="24px" />
            )}
        </button>
    );
}
