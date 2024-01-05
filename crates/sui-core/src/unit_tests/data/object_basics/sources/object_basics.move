// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

/// Test CTURD object basics (create, transfer, update, read, delete)
module examples::object_basics {
    use sui::dynamic_object_field as ofield;
    use sui::event;
    use sui::object::{Self, UID};
    use sui::tx_context::{Self, TxContext};
    use sui::transfer;

    struct Object has key, store {
        id: UID,
        value: u64,
    }

    struct Wrapper has key {
        id: UID,
        o: Object
    }

    struct NewValueEvent has copy, drop {
        new_value: u64
    }

    public entry fun create(value: u64, recipient: address, ctx: &mut TxContext) {
        transfer::transfer(
            Object { id: object::new(ctx), value },
            recipient
        )
    }

    public entry fun transfer(o: Object, recipient: address) {
        transfer::transfer(o, recipient)
    }

    public entry fun freeze_object(o: Object) {
        transfer::freeze_object(o)
    }

    public entry fun set_value(o: &mut Object, value: u64) {
        o.value = value;
    }

    // test that reading o2 and updating o1 works
    public entry fun update(o1: &mut Object, o2: &Object) {
        o1.value = o2.value;
        // emit an event so the world can see the new value
        event::emit(NewValueEvent { new_value: o2.value })
    }

    public entry fun delete(o: Object) {
        let Object { id, value: _ } = o;
        object::delete(id);
    }

    public entry fun wrap(o: Object, ctx: &mut TxContext) {
        transfer::transfer(Wrapper { id: object::new(ctx), o }, tx_context::sender(ctx))
    }

    public entry fun unwrap(w: Wrapper, ctx: &mut TxContext) {
        let Wrapper { id, o } = w;
        object::delete(id);
        transfer::transfer(o, tx_context::sender(ctx))
    }

    public entry fun add_ofield(o: &mut Object, v: Object) {
        ofield::add(&mut o.id, true, v);
    }

    public entry fun remove_ofield(o: &mut Object, ctx: &mut TxContext) {
        transfer::transfer(
            ofield::remove<bool, Object>(&mut o.id, true),
            tx_context::sender(ctx),
        );
    }
}
