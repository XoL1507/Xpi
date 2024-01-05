// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

export interface PlaceholderProps {
    width?: string;
    height?: string;
}

export function Placeholder({ width, height }: PlaceholderProps) {
    return (
        <div
            className="bg-placeholderShimmer h-[1em] w-full animate-shimmer rounded-[3px] bg-[length:1000px_100%]"
            style={{ width, height }}
        />
    );
}
