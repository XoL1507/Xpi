// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

/// The `Collection` type represents a collection of objects of the same type `T`.
/// In contrast to `vector<T>` which stores the object in the vector directly,
/// `Collection<T>` only tracks the ownership indirectly, by keeping a list of
/// references to the object IDs.
/// When using `vector<T>`, since the objects will be wrapped inside the vector,
/// these objects will not be stored in the global object pool, and hence not
/// directly accessible.
/// Collection allows us to own a list of same-typed objects, but still able to
/// access and operate on each individual object.
/// In contrast to `Bag`, `Collection` requires all objects have the same type.
module sui::collection {
    use std::errors;
    use std::option::{Self, Option};
    use std::vector::Self;
    use sui::id::{Self, ID, VersionedID};
    use sui::transfer::{Self, ChildRef};
    use sui::tx_context::{Self, TxContext};

    // Error codes
    /// When removing an object from the collection, EObjectNotFound
    /// will be triggered if the object is not owned by the collection.
    const EObjectNotFound: u64 = 0;

    /// Adding the same object to the collection twice is not allowed.
    const EObjectDoubleAdd: u64 = 1;

    /// The max capacity set for the collection cannot exceed the hard limit
    /// which is DEFAULT_MAX_CAPACITY.
    const EInvalidMaxCapacity: u64 = 2;

    /// Trying to add object to the collection when the collection is
    /// already at its maximum capacity.
    const EMaxCapacityExceeded: u64 = 3;

    // TODO: this is a placeholder number
    // We want to limit the capacity of collection because it requires O(N)
    // for search and removals. We could relax the capacity constraint once
    // we could use more efficient data structure such as set.
    const DEFAULT_MAX_CAPACITY: u64 = 65536;

    struct Collection<phantom T: key + store> has key {
        id: VersionedID,
        objects: vector<ChildRef<T>>,
        max_capacity: u64,
    }

    /// Create a new Collection and return it.
    public fun new<T: key + store>(ctx: &mut TxContext): Collection<T> {
        new_with_max_capacity(ctx, DEFAULT_MAX_CAPACITY)
    }

    /// Create a new Collection with custom size limit and return it.
    public fun new_with_max_capacity<T: key + store>(
        ctx: &mut TxContext,
        max_capacity: u64,
    ): Collection<T> {
        assert!(
            max_capacity <= DEFAULT_MAX_CAPACITY && max_capacity > 0 ,
            errors::limit_exceeded(EInvalidMaxCapacity)
        );
        Collection {
            id: tx_context::new_id(ctx),
            objects: vector::empty(),
            max_capacity,
        }
    }

    /// Create a new Collection and transfer it to the signer.
    public entry fun create<T: key + store>(ctx: &mut TxContext) {
        transfer::transfer(new<T>(ctx), tx_context::sender(ctx))
    }

    /// Returns the size of the collection.
    public fun size<T: key + store>(c: &Collection<T>): u64 {
        vector::length(&c.objects)
    }

    /// Add an object to the collection.
    /// If the object was owned by another object, an `old_child_ref` would be around
    /// and need to be consumed as well.
    /// Abort if the object is already in the collection.
    fun add_impl<T: key + store>(
        c: &mut Collection<T>,
        object: T,
        old_child_ref: Option<ChildRef<T>>,
    ) {
        assert!(
            size(c) + 1 <= c.max_capacity,
            errors::limit_exceeded(EMaxCapacityExceeded)
        );
        let id = id::id(&object);
        assert!(!contains(c, id), EObjectDoubleAdd);
        let child_ref = if (option::is_none(&old_child_ref)) {
            transfer::transfer_to_object(object, c)
        } else {
            let old_child_ref = option::extract(&mut old_child_ref);
            transfer::transfer_child_to_object(object, old_child_ref, c)
        };
        vector::push_back(&mut c.objects, child_ref);
        option::destroy_none(old_child_ref);
    }

    /// Add an object to the collection.
    /// Abort if the object is already in the collection.
    public fun add<T: key + store>(c: &mut Collection<T>, object: T) {
        add_impl(c, object, option::none())
    }

    /// Transfer an object that was owned by another object to the collection.
    /// Since the object is a child object of another object, an `old_child_ref`
    /// is around and needs to be consumed.
    public fun add_child_object<T: key + store>(
        c: &mut Collection<T>,
        object: T,
        old_child_ref: ChildRef<T>,
    ) {
        add_impl(c, object, option::some(old_child_ref))
    }

    /// Check whether the collection contains a specific object,
    /// identified by the object id in bytes.
    public fun contains<T: key + store>(c: &Collection<T>, id: &ID): bool {
        option::is_some(&find(c, id))
    }

    /// Remove and return the object from the collection.
    /// Abort if the object is not found.
    public fun remove<T: key + store>(c: &mut Collection<T>, object: T): (T, ChildRef<T>) {
        let idx = find(c, id::id(&object));
        assert!(option::is_some(&idx), EObjectNotFound);
        let child_ref = vector::remove(&mut c.objects, *option::borrow(&idx));
        (object, child_ref)
    }

    /// Remove the object from the collection, and then transfer it to the signer.
    public entry fun remove_and_take<T: key + store>(
        c: &mut Collection<T>,
        object: T,
        ctx: &mut TxContext,
    ) {
        let (object, child_ref) = remove(c, object);
        transfer::transfer_child_to_address(object, child_ref, tx_context::sender(ctx));
    }

    /// Transfer the entire collection to `recipient`.
    public entry fun transfer<T: key + store>(c: Collection<T>, recipient: address) {
        transfer::transfer(c, recipient)
    }

    public fun transfer_to_object_id<T: key + store>(
        obj: Collection<T>,
        owner_id: VersionedID,
    ): (VersionedID, ChildRef<Collection<T>>) {
        transfer::transfer_to_object_id(obj, owner_id)
    }

    /// Look for the object identified by `id_bytes` in the collection.
    /// Returns the index if found, none if not found.
    fun find<T: key + store>(c: &Collection<T>, id: &ID): Option<u64> {
        let i = 0;
        let len = size(c);
        while (i < len) {
            let child_ref = vector::borrow(&c.objects, i);
            if (transfer::is_child_unsafe(child_ref,  id)) {
                return option::some(i)
            };
            i = i + 1;
        };
        option::none()
    }
}
