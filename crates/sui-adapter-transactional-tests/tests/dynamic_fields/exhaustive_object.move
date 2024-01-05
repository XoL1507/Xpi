// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// similar to dynamic_object_field_tests but over multiple transactions,
// as this uses a different code path
// test remove with the wrong value type

//# init --addresses a=0x0 --accounts A

//# publish
module a::m {

use sui::dynamic_object_field::{add, exists_, borrow, borrow_mut, remove};
use sui::object::{Self, UID};
use sui::tx_context::{sender, TxContext};

struct Obj has key, store {
    id: UID,
}

struct Counter has key, store {
    id: UID,
    count: u64,
}

fun new(id: UID): Counter {
    Counter { id, count: 0 }
}

fun count(counter: &Counter): u64 {
    counter.count
}

fun bump(counter: &mut Counter): &mut Counter {
    counter.count = counter.count + 1;
    counter
}

fun destroy(counter: Counter): u64 {
    let Counter { id, count } = counter;
    object::delete(id);
    count
}

entry fun t0(ctx: &mut TxContext) {
    let id = object::new(ctx);
    sui::transfer::transfer(Obj { id }, sender(ctx))
}

entry fun t1(obj: &mut Obj, ctx: &mut TxContext) {
    let id = &mut obj.id;
    add(id, 0, new(object::new(ctx)));
    add(id, b"", new(object::new(ctx)));
    add(id, false, new(object::new(ctx)));
}

entry fun t2(obj: &Obj) {
    let id = &obj.id;
    assert!(exists_<u64>(id, 0), 0);
    assert!(exists_<vector<u8>>(id, b""), 0);
    assert!(exists_<bool>(id, false), 0);
}

entry fun t3(obj: &Obj) {
    let id = &obj.id;
    assert!(count(borrow(id, 0)) == 0, 0);
    assert!(count(borrow(id, b"")) == 0, 0);
    assert!(count(borrow(id, false)) == 0, 0);
}

entry fun t4(obj: &mut Obj) {
    let id = &mut obj.id;
    bump(borrow_mut(id, 0));
    bump(bump(borrow_mut(id, b"")));
    bump(bump(bump(borrow_mut(id, false))));
}

entry fun t5(obj: &mut Obj) {
    let id = &mut obj.id;
    assert!(count(borrow(id, 0)) == 1, 0);
    assert!(count(borrow(id, b"")) == 2, 0);
    assert!(count(borrow(id, false)) == 3, 0);
}

entry fun t6(obj: &mut Obj) {
    let id = &mut obj.id;
    assert!(destroy(remove(id, 0)) == 1, 0);
    assert!(destroy(remove(id, b"")) == 2, 0);
    // do not remove at least one
}

entry fun t7(obj: &Obj) {
    let id = &obj.id;
    assert!(!exists_<u64>(id, 0), 0);
    assert!(!exists_<vector<u8>>(id, b""), 0);
    assert!(exists_<bool>(id, false), 0);
}

entry fun t8(obj: Obj) {
    let Obj { id } = obj;
    object::delete(id);
}

}

//# run a::m::t0 --sender A

//# run a::m::t1 --sender A --args object(106)

//# run a::m::t2 --sender A --args object(106)

//# run a::m::t3 --sender A --args object(106)

//# run a::m::t4 --sender A --args object(106)

//# run a::m::t5 --sender A --args object(106)

//# run a::m::t6 --sender A --args object(106)

//# run a::m::t7 --sender A --args object(106)

//# run a::m::t8 --sender A --args object(106)
