// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
use crate::test_keys;
use sui_types::base_types::{ObjectID, SuiAddress, TransactionDigest};
use sui_types::gas_coin::GasCoin;
use sui_types::object::{MoveObject, Object, Owner, OBJECT_START_VERSION};

/// Make a few test gas objects (all with the same owner).
pub fn test_gas_objects() -> Vec<Object> {
    (0..9)
        .map(|i| {
            let seed = format!("0x444444444444444{i}");
            let gas_object_id = ObjectID::from_hex_literal(&seed).unwrap();
            let (owner, _) = test_keys().pop().unwrap();
            Object::with_id_owner_for_testing(gas_object_id, owner)
        })
        .collect()
}

/// Make a few test gas objects with a specific owners.
pub fn test_gas_objects_with_owners<O>(owners: O) -> Vec<Object>
where
    O: IntoIterator<Item = SuiAddress>,
{
    owners
        .into_iter()
        .enumerate()
        .map(|(i, owner)| {
            let seed = format!("0x555555555555555{i}");
            let gas_object_id = ObjectID::from_hex_literal(&seed).unwrap();
            Object::with_id_owner_for_testing(gas_object_id, owner)
        })
        .collect()
}

/// make a test shared object.
pub fn test_shared_object() -> Object {
    let seed = "0x6666666666666660";
    let shared_object_id = ObjectID::from_hex_literal(seed).unwrap();
    let content = GasCoin::new(shared_object_id, OBJECT_START_VERSION, 10);
    let obj = MoveObject::new(/* type */ GasCoin::type_(), content.to_bcs_bytes());
    Object::new_move(obj, Owner::Shared, TransactionDigest::genesis())
}
