// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use move_core_types::language_storage::StructTag;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use serde_with::Bytes;
use std::convert::{From, TryFrom};
use sui_types::base_types::{ObjectDigest, SequenceNumber, TransactionDigest};
use sui_types::crypto::{sha3_hash, Signable};
use sui_types::error::SuiError;
use sui_types::move_package::MovePackage;
use sui_types::object::{Data, MoveObject, Object, Owner};

/// Forked version of [`sui_types::object::Object`]
/// Used for efficient storing of move objects in the database
#[derive(Eq, PartialEq, Debug, Clone, Deserialize, Serialize, Hash)]
pub struct StoreObject {
    pub data: StoreData,
    pub owner: Owner,
    pub previous_transaction: TransactionDigest,
    pub storage_rebate: u64,
}

/// Forked version of [`sui_types::object::Data`]
/// Adds extra enum value `IndirectObject`, which represents a reference to an object stored separately
#[derive(Eq, PartialEq, Debug, Clone, Deserialize, Serialize, Hash)]
pub enum StoreData {
    Move(MoveObject),
    Package(MovePackage),
    IndirectObject(IndirectObjectMetadata),
}

/// Metadata of stored moved object
#[derive(Eq, PartialEq, Debug, Clone, Deserialize, Serialize, Hash)]
pub struct IndirectObjectMetadata {
    version: SequenceNumber,
    pub digest: ObjectDigest,
}

/// Separately stored move object
#[serde_as]
#[derive(Eq, PartialEq, Debug, Clone, Deserialize, Serialize, Hash)]
pub struct StoreMoveObject {
    pub type_: StructTag,
    has_public_transfer: bool,
    #[serde_as(as = "Bytes")]
    contents: Vec<u8>,
    /// reference count of `MoveMetadata` that point to the same content
    /// once it hits 0, the object gets deleted by a compaction job
    ref_count: usize,
}

impl<W> Signable<W> for StoreMoveObject
where
    W: std::io::Write,
{
    fn write(&self, writer: &mut W) {
        write!(writer, "StoreMoveObject::").expect("Hasher should not fail");
        bcs::serialize_into(writer, &self).expect("Message serialization should not fail");
    }
}

impl StoreMoveObject {
    pub fn digest(&self) -> ObjectDigest {
        ObjectDigest::new(sha3_hash(self))
    }
}

pub struct StoreObjectPair(pub StoreObject, pub Option<StoreMoveObject>);

impl From<Object> for StoreObjectPair {
    fn from(object: Object) -> Self {
        let mut indirect_object = None;

        let data = match object.data {
            Data::Package(package) => StoreData::Package(package),
            Data::Move(move_obj) => {
                // TODO: add real heuristic to decide if object needs to be stored indirectly
                if cfg!(test) {
                    let move_object = StoreMoveObject {
                        type_: move_obj.type_.clone(),
                        has_public_transfer: move_obj.has_public_transfer(),
                        contents: move_obj.contents().to_vec(),
                        ref_count: 1,
                    };
                    let digest = move_object.digest();
                    indirect_object = Some(move_object);
                    StoreData::IndirectObject(IndirectObjectMetadata {
                        version: move_obj.version(),
                        digest,
                    })
                } else {
                    StoreData::Move(move_obj)
                }
            }
        };
        let store_object = StoreObject {
            data,
            owner: object.owner,
            previous_transaction: object.previous_transaction,
            storage_rebate: object.storage_rebate,
        };
        Self(store_object, indirect_object)
    }
}

impl TryFrom<StoreObjectPair> for Object {
    type Error = SuiError;

    fn try_from(object: StoreObjectPair) -> Result<Self, Self::Error> {
        let StoreObjectPair(store_object, indirect_object) = object;

        let data = match (store_object.data, indirect_object) {
            (StoreData::Move(object), None) => Data::Move(object),
            (StoreData::Package(package), None) => Data::Package(package),
            (StoreData::IndirectObject(metadata), Some(indirect_obj)) => unsafe {
                Data::Move(MoveObject::new_from_execution_with_limit(
                    indirect_obj.type_,
                    indirect_obj.has_public_transfer,
                    metadata.version,
                    indirect_obj.contents,
                    // verification is already done during initial execution
                    u64::MAX,
                )?)
            },
            _ => {
                return Err(SuiError::StorageCorruptedFieldError(
                    "inconsistent object representation".to_string(),
                ))
            }
        };

        Ok(Self {
            data,
            owner: store_object.owner,
            previous_transaction: store_object.previous_transaction,
            storage_rebate: store_object.storage_rebate,
        })
    }
}
