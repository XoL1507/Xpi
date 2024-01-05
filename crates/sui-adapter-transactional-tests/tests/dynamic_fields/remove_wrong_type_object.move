// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// similar to dynamic_object_field_tests but over multiple transactions,
// as this uses a different code path
// test borrow with the wrong value type

//# init --addresses a=0x0 --accounts A

//# publish
module a::m {

use sui::dynamic_object_field::{add, remove};
use sui::object;
use sui::tx_context::{sender, TxContext};

struct Obj has key, store {
    id: object::UID,
}

struct Fake has key, store {
    id: object::UID,
}

entry fun t1(ctx: &mut TxContext) {
    let id = object::new(ctx);
    add(&mut id, 0, Obj { id: object::new(ctx) });
    sui::transfer::public_transfer(Obj { id }, sender(ctx))
}

entry fun t2(obj: &mut Obj) {
    let Fake { id } = remove<u64, Fake>(&mut obj.id, 0);
    object::delete(id);
}

}

//# run a::m::t1 --sender A

//# run a::m::t2 --sender A --args object(107)
