#!/bin/sh
# Copyright (c) The Move Contributors
# SPDX-License-Identifier: Apache-2.0

ROOT="$(git rev-parse --show-toplevel)"
TYPE="$(echo "$1" | sed s/^--resolve-move-//)"
PACKAGE="$2"

# Print Diagnostics for test output
cat <<EOF >&2
Successful External Resolver
PWD:     $(pwd | sed "s,^$ROOT,\$ROOT,")
Type:    $TYPE
Package: $PACKAGE
EOF

# Print lock file
cat <<EOF
[move]
version = 0
$TYPE = [
    { name = "$PACKAGE" },
]

[[move.package]]
name = "$PACKAGE"
source = { local = "./deps_only/$PACKAGE" }
dependencies = [
    { name = "${PACKAGE}Dep", addr_subst = { "A" = "0x42" } },
]

[[move.package]]
name = "${PACKAGE}Dep"
source = { local = "./deps_only/${PACKAGE}Dep" }
EOF
