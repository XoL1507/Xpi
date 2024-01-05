// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//# init --addresses P0=0x0 --accounts A --simulator

//# publish
module P0::m {
    use sui::object::{Self, UID};
    use sui::transfer;
    use sui::tx_context::{Self, TxContext};

    struct Foo has key, store {
        id: UID,
    }

    struct Bar has key {
        id: UID,
    }

    public fun foo(ctx: &mut TxContext): Foo {
        Foo { id: object::new(ctx) }
    }

    public fun bar(ctx: &mut TxContext) {
        transfer::transfer(
            Bar { id: object::new(ctx) },
            tx_context::sender(ctx),
        )
    }
}

//# programmable --inputs @A
//> 0: P0::m::foo();
//> 1: P0::m::bar();
//> TransferObjects([Result(0)], Input(0))

//# create-checkpoint

//# run-graphql
{
    transactionBlockConnection(last: 1) {
        nodes {
            effects {
                objectChanges {
                    outputState {
                        asMoveObject {
                            contents { type { repr } }
                            hasPublicTransfer
                        }
                    }
                }
            }
        }
    }
}
