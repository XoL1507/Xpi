// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use move_binary_format::file_format::AbilitySet;
use move_core_types::{
    identifier::IdentStr,
    language_storage::{ModuleId, StructTag},
    resolver::{ModuleResolver, ResourceResolver},
};
use move_vm_types::loaded_data::runtime_types::Type;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

use crate::{
    base_types::{ObjectID, ObjectRef, SequenceNumber, SuiAddress},
    coin::Coin,
    digests::ObjectDigest,
    error::{ExecutionError, ExecutionErrorKind, SuiError},
    execution_status::CommandArgumentError,
    object::{Object, Owner},
    storage::{BackingPackageStore, ChildObjectResolver, StorageView},
};

pub trait SuiResolver:
    ResourceResolver<Error = SuiError> + ModuleResolver<Error = SuiError> + BackingPackageStore
{
    fn as_backing_package_store(&self) -> &dyn BackingPackageStore;
}

impl<T> SuiResolver for T
where
    T: ResourceResolver<Error = SuiError>,
    T: ModuleResolver<Error = SuiError>,
    T: BackingPackageStore,
{
    fn as_backing_package_store(&self) -> &dyn BackingPackageStore {
        self
    }
}

/// Interface with the store necessary to execute a programmable transaction
pub trait ExecutionState: StorageView + SuiResolver {
    fn as_sui_resolver(&self) -> &dyn SuiResolver;
    fn as_child_resolver(&self) -> &dyn ChildObjectResolver;
}

impl<T> ExecutionState for T
where
    T: StorageView,
    T: SuiResolver,
{
    fn as_sui_resolver(&self) -> &dyn SuiResolver {
        self
    }

    fn as_child_resolver(&self) -> &dyn ChildObjectResolver {
        self
    }
}

/// View of the store necessary to produce the layouts of types.
pub trait TypeLayoutStore: BackingPackageStore + ModuleResolver<Error = SuiError> {}
impl<T> TypeLayoutStore for T
where
    T: BackingPackageStore,
    T: ModuleResolver<Error = SuiError>,
{
}

pub struct ExecutionResults {
    written_objects: BTreeMap<ObjectID, Object>,
    objects_modified_at: BTreeMap<ObjectID, (SequenceNumber, ObjectDigest)>,
    created_object_ids: BTreeSet<ObjectID>,
    deleted_object_ids: BTreeSet<ObjectID>,
    /// Ordered sequence of events emitted by execution
    user_events: Vec<Event>,
}

impl ExecutionResults {
    pub fn new(mutable_input_objects: &[ObjectRef]) -> Self {
        let mut results = Self { ..Default };
        results.reset(mutable_input_objects);
        results
    }

    pub fn read_object(&self, id: &ObjectID) -> Option<&Object> {
        self.written_objects.get(id)
    }

    pub fn write_object(&mut self, object: Object) {
        self.written_objects.insert(object.id(), object);
    }

    pub fn reset(&mut self, mutable_input_objects: &[ObjectRef]) {
        self.written_objects.clear();
        self.objects_modified_at = mutable_input_objects
            .iter()
            .map(|oref| (oref.0, (oref.1, oref.2)))
            .collect();
        self.created_object_ids.clear();
        self.deleted_object_ids.clear();
        self.user_events.clear();
    }

    pub fn record_execution_results(&mut self, results: Self) {
        #[cfg(debug_assertions)]
        {
            for new_modified_at in results.objects_modified_at {
                if let Some(old) = self.objects_modified_at.get(&new_modified_at.0) {
                    assert_eq!(old, &new_modified_at.1);
                }
            }
        }
        self.written_objects.extend(results.written_objects);
        self.objects_modified_at.extend(results.objects_modified_at);
        self.created_object_ids.extend(results.created_object_ids);
        self.deleted_object_ids.extend(results.deleted_object_ids);
        self.user_events.extend(results.user_events);
    }
}

#[derive(Clone, Debug)]
pub struct InputObjectMetadata {
    pub id: ObjectID,
    pub is_mutable_input: bool,
    pub owner: Owner,
    pub version: SequenceNumber,
    pub digest: ObjectDigest,
}

#[derive(Debug, PartialEq, Eq)]
pub struct LoadedChildObjectMetadata {
    pub version: SequenceNumber,
    pub digest: ObjectDigest,
    pub storage_rebate: u64,
}

#[derive(Clone, Debug)]
pub struct InputValue {
    /// Used to remember the object ID and owner even if the value is taken
    pub object_metadata: Option<InputObjectMetadata>,
    pub inner: ResultValue,
}

#[derive(Clone, Debug)]
pub struct ResultValue {
    /// This is used primarily for values that have `copy` but not `drop` as they must have been
    /// copied after the last borrow, otherwise we cannot consider the last "copy" to be instead
    /// a "move" of the value.
    pub last_usage_kind: Option<UsageKind>,
    pub value: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageKind {
    BorrowImm,
    BorrowMut,
    ByValue,
}

#[derive(Debug, Clone)]
pub enum Value {
    Object(ObjectValue),
    Raw(RawValueType, Vec<u8>),
}

#[derive(Debug, Clone)]
pub struct ObjectValue {
    pub type_: Type,
    pub has_public_transfer: bool,
    // true if it has been used in a public, non-entry Move call
    // In other words, false if all usages have been with non-Move commands or
    // entry Move functions
    pub used_in_non_entry_move_call: bool,
    pub contents: ObjectContents,
}

#[derive(Debug, Clone)]
pub enum ObjectContents {
    Coin(Coin),
    Raw(Vec<u8>),
}

#[derive(Debug, Clone)]
pub enum RawValueType {
    Any,
    Loaded {
        ty: Type,
        abilities: AbilitySet,
        used_in_non_entry_move_call: bool,
    },
}

#[derive(Clone, Copy)]
pub enum CommandKind<'a> {
    MoveCall {
        package: ObjectID,
        module: &'a IdentStr,
        function: &'a IdentStr,
    },
    MakeMoveVec,
    TransferObjects,
    SplitCoins,
    MergeCoins,
    Publish,
    Upgrade,
}

impl InputValue {
    pub fn new_object(object_metadata: InputObjectMetadata, value: ObjectValue) -> Self {
        InputValue {
            object_metadata: Some(object_metadata),
            inner: ResultValue::new(Value::Object(value)),
        }
    }

    pub fn new_raw(ty: RawValueType, value: Vec<u8>) -> Self {
        InputValue {
            object_metadata: None,
            inner: ResultValue::new(Value::Raw(ty, value)),
        }
    }
}

impl ResultValue {
    pub fn new(value: Value) -> Self {
        Self {
            last_usage_kind: None,
            value: Some(value),
        }
    }
}

impl Value {
    pub fn is_copyable(&self) -> bool {
        match self {
            Value::Object(_) => false,
            Value::Raw(RawValueType::Any, _) => true,
            Value::Raw(RawValueType::Loaded { abilities, .. }, _) => abilities.has_copy(),
        }
    }

    pub fn write_bcs_bytes(&self, buf: &mut Vec<u8>) {
        match self {
            Value::Object(obj_value) => obj_value.write_bcs_bytes(buf),
            Value::Raw(_, bytes) => buf.extend(bytes),
        }
    }

    pub fn was_used_in_non_entry_move_call(&self) -> bool {
        match self {
            Value::Object(obj) => obj.used_in_non_entry_move_call,
            // Any is only used for Pure inputs, and if it was used by &mut it would have switched
            // to Loaded
            Value::Raw(RawValueType::Any, _) => false,
            Value::Raw(
                RawValueType::Loaded {
                    used_in_non_entry_move_call,
                    ..
                },
                _,
            ) => *used_in_non_entry_move_call,
        }
    }
}

impl ObjectValue {
    /// # Safety
    /// We must have the Type is the coin type, but we are unable to check it at this spot
    pub unsafe fn coin(type_: Type, coin: Coin) -> Self {
        Self {
            type_,
            has_public_transfer: true,
            used_in_non_entry_move_call: false,
            contents: ObjectContents::Coin(coin),
        }
    }

    pub fn ensure_public_transfer_eligible(&self) -> Result<(), ExecutionError> {
        if !self.has_public_transfer {
            return Err(ExecutionErrorKind::InvalidTransferObject.into());
        }
        Ok(())
    }

    pub fn write_bcs_bytes(&self, buf: &mut Vec<u8>) {
        match &self.contents {
            ObjectContents::Raw(bytes) => buf.extend(bytes),
            ObjectContents::Coin(coin) => buf.extend(coin.to_bcs_bytes()),
        }
    }
}

pub trait TryFromValue: Sized {
    fn try_from_value(value: Value) -> Result<Self, CommandArgumentError>;
}

impl TryFromValue for Value {
    fn try_from_value(value: Value) -> Result<Self, CommandArgumentError> {
        Ok(value)
    }
}

impl TryFromValue for ObjectValue {
    fn try_from_value(value: Value) -> Result<Self, CommandArgumentError> {
        match value {
            Value::Object(o) => Ok(o),
            Value::Raw(RawValueType::Any, _) => Err(CommandArgumentError::TypeMismatch),
            Value::Raw(RawValueType::Loaded { .. }, _) => Err(CommandArgumentError::TypeMismatch),
        }
    }
}

impl TryFromValue for SuiAddress {
    fn try_from_value(value: Value) -> Result<Self, CommandArgumentError> {
        try_from_value_prim(&value, Type::Address)
    }
}

impl TryFromValue for u64 {
    fn try_from_value(value: Value) -> Result<Self, CommandArgumentError> {
        try_from_value_prim(&value, Type::U64)
    }
}

fn try_from_value_prim<'a, T: Deserialize<'a>>(
    value: &'a Value,
    expected_ty: Type,
) -> Result<T, CommandArgumentError> {
    match value {
        Value::Object(_) => Err(CommandArgumentError::TypeMismatch),
        Value::Raw(RawValueType::Any, bytes) => {
            bcs::from_bytes(bytes).map_err(|_| CommandArgumentError::InvalidBCSBytes)
        }
        Value::Raw(RawValueType::Loaded { ty, .. }, bytes) => {
            if ty != &expected_ty {
                return Err(CommandArgumentError::TypeMismatch);
            }
            bcs::from_bytes(bytes).map_err(|_| CommandArgumentError::InvalidBCSBytes)
        }
    }
}
