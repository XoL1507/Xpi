// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::errors::IndexerError;
use crate::models::owners::OwnerType;
use crate::schema::objects;
use crate::schema::sql_types::BcsBytes;
use diesel::deserialize::FromSql;
use diesel::pg::{Pg, PgValue};
use diesel::prelude::*;
use diesel::serialize::{Output, ToSql, WriteTuple};
use diesel::sql_types::{Bytea, Nullable, Record, VarChar};
use diesel::SqlType;
use diesel_derive_enum::DbEnum;
use move_bytecode_utils::module_cache::GetModule;
use std::str::FromStr;
use sui_json_rpc_types::{SuiObjectData, SuiObjectRef, SuiRawData};
use sui_types::base_types::{EpochId, ObjectID, ObjectRef, ObjectType, SequenceNumber, SuiAddress};
use sui_types::digests::TransactionDigest;
use sui_types::messages_checkpoint::CheckpointSequenceNumber;
use sui_types::move_package::MovePackage;
use sui_types::object::{Data, MoveObject, ObjectFormatOptions, ObjectRead, Owner};

const OBJECT: &str = "object";

#[derive(Queryable, Insertable, Debug, Identifiable, Clone)]
#[diesel(table_name = objects, primary_key(object_id))]
pub struct Object {
    // epoch id in which this object got update.
    pub epoch: i64,
    // checkpoint seq number in which this object got update.
    pub checkpoint: i64,
    pub object_id: String,
    pub version: i64,
    pub object_digest: String,
    pub owner_type: OwnerType,
    pub owner_address: Option<String>,
    pub initial_shared_version: Option<i64>,
    pub previous_transaction: String,
    pub object_type: String,
    pub object_status: ObjectStatus,
    pub has_public_transfer: bool,
    pub storage_rebate: i64,
    pub bcs: Vec<NamedBcsBytes>,
}
#[derive(SqlType, Debug, Clone)]
#[diesel(sql_type = crate::schema::sql_types::BcsBytes)]
pub struct NamedBcsBytes(pub String, pub Vec<u8>);

impl ToSql<Nullable<BcsBytes>, Pg> for NamedBcsBytes {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> diesel::serialize::Result {
        WriteTuple::<(VarChar, Bytea)>::write_tuple(&(self.0.clone(), self.1.clone()), out)
    }
}

impl FromSql<Nullable<BcsBytes>, Pg> for NamedBcsBytes {
    fn from_sql(bytes: PgValue) -> diesel::deserialize::Result<Self> {
        let (name, data) = FromSql::<Record<(VarChar, Bytea)>, Pg>::from_sql(bytes)?;
        Ok(NamedBcsBytes(name, data))
    }
}

#[derive(Debug, Clone)]
pub struct DeletedObject {
    // epoch id in which this object got deleted.
    pub epoch: i64,
    // checkpoint seq number in which this object got deleted.
    pub checkpoint: i64,
    pub object_id: String,
    pub version: i64,
    pub object_digest: String,
    pub owner_type: OwnerType,
    pub previous_transaction: String,
    pub object_type: String,
    pub object_status: ObjectStatus,
    pub has_public_transfer: bool,
}

impl From<DeletedObject> for Object {
    fn from(o: DeletedObject) -> Self {
        Object {
            epoch: o.epoch,
            checkpoint: o.checkpoint,
            object_id: o.object_id,
            version: o.version,
            object_digest: o.object_digest,
            owner_type: o.owner_type,
            owner_address: None,
            initial_shared_version: None,
            previous_transaction: o.previous_transaction,
            object_type: o.object_type,
            object_status: o.object_status,
            has_public_transfer: o.has_public_transfer,
            storage_rebate: 0,
            bcs: vec![],
        }
    }
}

#[derive(DbEnum, Debug, Clone, Copy)]
#[ExistingTypePath = "crate::schema::sql_types::ObjectStatus"]
pub enum ObjectStatus {
    Created,
    Mutated,
    Deleted,
    Wrapped,
    Unwrapped,
    UnwrappedThenDeleted,
}

impl Object {
    pub fn from(
        epoch: &EpochId,
        checkpoint: &CheckpointSequenceNumber,
        status: &ObjectStatus,
        o: &SuiObjectData,
    ) -> Self {
        let (owner_type, owner_address, initial_shared_version) =
            owner_to_owner_info(&o.owner.expect("Expect the owner type to be non-empty"));

        let (has_public_transfer, bcs) =
            match o.bcs.clone().expect("Expect BCS data to be non-empty") {
                SuiRawData::MoveObject(o) => (
                    o.has_public_transfer,
                    vec![NamedBcsBytes(OBJECT.to_string(), o.bcs_bytes)],
                ),
                SuiRawData::Package(p) => (
                    false,
                    p.module_map
                        .into_iter()
                        .map(|(k, v)| NamedBcsBytes(k, v))
                        .collect(),
                ),
            };

        Object {
            epoch: *epoch as i64,
            checkpoint: *checkpoint as i64,
            object_id: o.object_id.to_string(),
            version: o.version.value() as i64,
            object_digest: o.digest.base58_encode(),
            owner_type,
            owner_address,
            initial_shared_version,
            previous_transaction: o
                .previous_transaction
                .expect("Expect previous transaction to be non-empty")
                .base58_encode(),
            object_type: o
                .type_
                .as_ref()
                .expect("Expect the object type to be non-empty")
                .to_string(),
            object_status: *status,
            has_public_transfer,
            storage_rebate: o.storage_rebate.unwrap_or_default() as i64,
            bcs,
        }
    }

    pub fn try_into_object_read(
        self,
        module_cache: &impl GetModule,
    ) -> Result<ObjectRead, IndexerError> {
        Ok(match self.object_status {
            ObjectStatus::Deleted | ObjectStatus::UnwrappedThenDeleted => {
                ObjectRead::Deleted(self.get_object_ref()?)
            }
            _ => {
                let oref = self.get_object_ref()?;
                let object: sui_types::object::Object = self.try_into()?;
                let layout = object.get_layout(ObjectFormatOptions::default(), module_cache)?;
                ObjectRead::Exists(oref, object, layout)
            }
        })
    }

    pub fn get_object_ref(&self) -> Result<ObjectRef, IndexerError> {
        let object_id = self.object_id.parse()?;
        let digest = self.object_digest.parse().map_err(|e| {
            IndexerError::SerdeError(format!(
                "Failed to parse object digest: {}, error: {}",
                self.object_digest, e
            ))
        })?;
        Ok((object_id, (self.version as u64).into(), digest))
    }
}

impl TryFrom<Object> for sui_types::object::Object {
    type Error = IndexerError;

    fn try_from(o: Object) -> Result<Self, Self::Error> {
        let object_type = ObjectType::from_str(&o.object_type)?;
        let object_id = ObjectID::from_str(&o.object_id)?;
        let version = SequenceNumber::from_u64(o.version as u64);
        let owner = match o.owner_type {
            OwnerType::AddressOwner => Owner::AddressOwner(SuiAddress::from_str(
                &o.owner_address.expect("Owner address should not be empty."),
            )?),
            OwnerType::ObjectOwner => Owner::ObjectOwner(SuiAddress::from_str(
                &o.owner_address.expect("Owner address should not be empty."),
            )?),
            OwnerType::Shared => Owner::Shared {
                initial_shared_version: SequenceNumber::from_u64(
                    o.initial_shared_version
                        .expect("Shared version should not be empty.") as u64,
                ),
            },
            OwnerType::Immutable => Owner::Immutable,
        };
        let previous_transaction = TransactionDigest::from_str(&o.previous_transaction)?;

        Ok(match object_type {
            ObjectType::Package => {
                let modules = o
                    .bcs
                    .into_iter()
                    .map(|NamedBcsBytes(name, bytes)| (name, bytes))
                    .collect();
                // Ok to unwrap, package size is safe guarded by the full node, we are not limiting size when reading back from DB.
                let package = MovePackage::new(object_id, version, &modules, u64::MAX).unwrap();
                sui_types::object::Object {
                    data: Data::Package(package),
                    owner,
                    previous_transaction,
                    storage_rebate: o.storage_rebate as u64,
                }
            }
            // Reconstructing MoveObject form database table, move VM safety concern is irrelevant here.
            ObjectType::Struct(object_type) => unsafe {
                let content = o
                    .bcs
                    .first()
                    .expect("BCS content should not be empty")
                    .1
                    .clone();
                // Ok to unwrap, object size is safe guarded by the full node, we are not limiting size when reading back from DB.
                let object = MoveObject::new_from_execution_with_limit(
                    object_type,
                    o.has_public_transfer,
                    version,
                    content,
                    u64::MAX,
                )
                .unwrap();

                sui_types::object::Object {
                    data: Data::Move(object),
                    owner,
                    previous_transaction,
                    storage_rebate: o.storage_rebate as u64,
                }
            },
        })
    }
}

impl DeletedObject {
    pub fn from(
        epoch: &EpochId,
        checkpoint: &CheckpointSequenceNumber,
        oref: &SuiObjectRef,
        previous_tx: &TransactionDigest,
        status: ObjectStatus,
    ) -> Self {
        Self {
            epoch: *epoch as i64,
            checkpoint: *checkpoint as i64,
            object_id: oref.object_id.to_string(),
            version: oref.version.value() as i64,
            // DeleteObject is use for upsert only, this value will not be inserted into the DB
            // this dummy value is use to satisfy non null constrain.
            object_digest: "DELETED".to_string(),
            owner_type: OwnerType::AddressOwner,
            previous_transaction: previous_tx.base58_encode(),
            object_type: "DELETED".to_string(),
            object_status: status,
            has_public_transfer: false,
        }
    }
}

// return owner_type, owner_address and initial_shared_version
pub fn owner_to_owner_info(owner: &Owner) -> (OwnerType, Option<String>, Option<i64>) {
    match owner {
        Owner::AddressOwner(address) => (OwnerType::AddressOwner, Some(address.to_string()), None),
        Owner::ObjectOwner(address) => (OwnerType::ObjectOwner, Some(address.to_string()), None),
        Owner::Shared {
            initial_shared_version,
        } => (
            OwnerType::Shared,
            None,
            Some(initial_shared_version.value() as i64),
        ),
        Owner::Immutable => (OwnerType::Immutable, None, None),
    }
}
