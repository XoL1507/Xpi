// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use sui_json_rpc_types::ObjectChange;
use sui_types::base_types::{ObjectID, ObjectRef, SequenceNumber, SuiAddress};
use sui_types::object::Owner;
use sui_types::storage::{DeleteKind, WriteKind};

use crate::ObjectProvider;

pub async fn get_object_changes<P: ObjectProvider<Error = E>, E>(
    object_provider: &P,
    sender: SuiAddress,
    modified_at_versions: &[(ObjectID, SequenceNumber)],
    all_changed_objects: Vec<(&ObjectRef, &Owner, WriteKind)>,
    all_deleted: Vec<(&ObjectRef, DeleteKind)>,
) -> Result<Vec<ObjectChange>, E> {
    let mut object_changes = vec![];

    let modify_at_version = modified_at_versions
        .iter()
        .cloned()
        .collect::<BTreeMap<_, _>>();

    for ((id, version, digest), owner, kind) in all_changed_objects {
        let o = object_provider.get_object(id, version).await?;
        if let Some(type_) = o.type_() {
            let object_type = type_.clone().into();

            match kind {
                WriteKind::Mutate => object_changes.push(ObjectChange::Mutated {
                    sender,
                    owner: *owner,
                    object_type,
                    object_id: *id,
                    version: *version,
                    // modify_at_version should always be available for mutated object
                    previous_version: modify_at_version.get(id).cloned().unwrap_or_default(),
                    digest: *digest,
                }),
                WriteKind::Create => object_changes.push(ObjectChange::Created {
                    sender,
                    owner: *owner,
                    object_type,
                    object_id: *id,
                    version: *version,
                    digest: *digest,
                }),
                _ => {}
            }
        } else if let Some(p) = o.data.try_as_package() {
            if kind == WriteKind::Create {
                object_changes.push(ObjectChange::Published {
                    package_id: p.id(),
                    version: p.version(),
                    digest: *digest,
                    modules: p.serialized_module_map().keys().cloned().collect(),
                })
            }
        };
    }

    for ((id, version, _), kind) in all_deleted {
        let o = object_provider
            .find_object_lt_or_eq_version(id, version)
            .await?;
        if let Some(o) = o {
            if let Some(type_) = o.type_() {
                let object_type = type_.clone().into();
                match kind {
                    DeleteKind::Normal => object_changes.push(ObjectChange::Deleted {
                        sender,
                        object_type,
                        object_id: *id,
                        version: *version,
                    }),
                    DeleteKind::Wrap => object_changes.push(ObjectChange::Wrapped {
                        sender,
                        object_type,
                        object_id: *id,
                        version: *version,
                    }),
                    _ => {}
                }
            }
        };
    }

    Ok(object_changes)
}
