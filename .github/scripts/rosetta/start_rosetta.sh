#!/bin/bash
# Copyright (c) Mysten Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

echo "Start Rosetta online server"
cargo run --bin sui-rosetta start-online-server &

echo "Start Rosetta offline server"
cargo run --bin sui-rosetta start-offline-server &
