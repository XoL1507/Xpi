// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::programmable_transactions::context::load_type_from_struct;
use crate::programmable_transactions::linkage_view::LinkageView;
use linked_hash_map::LinkedHashMap;
use move_core_types::account_address::AccountAddress;
use move_core_types::language_storage::{ModuleId, StructTag};
use move_core_types::resolver::{ModuleResolver, ResourceResolver};
use move_core_types::value::{MoveStructLayout, MoveTypeLayout};
use move_vm_runtime::move_vm::MoveVM;
use sui_types::base_types::ObjectID;
use sui_types::error::SuiResult;
use sui_types::execution::TypeLayoutStore;
use sui_types::object::Object;
use sui_types::storage::BackingPackageStore;
use sui_types::{
    error::SuiError,
    object::{MoveObject, ObjectFormatOptions},
    type_resolver::LayoutResolver,
};

/// Retrieve a `MoveStructLayout` from a `Type`.
/// Invocation into the `Session` to leverage the `LinkageView` implementation
/// common to the runtime.
pub struct TypeLayoutResolver<'state, 'vm> {
    vm: &'vm MoveVM,
    linkage_view: LinkageView<'state>,
}

/// Implements SuiResolver traits by providing null implementations for module and resource
/// resolution and delegating backing package resolution to the trait object.
struct NullSuiResolver<'state>(Box<dyn TypeLayoutStore + 'state>);

impl<'state, 'vm> TypeLayoutResolver<'state, 'vm> {
    pub fn new(vm: &'vm MoveVM, state_view: Box<dyn TypeLayoutStore + 'state>) -> Self {
        let linkage_view = LinkageView::new(Box::new(NullSuiResolver(state_view)));
        Self { vm, linkage_view }
    }
}

impl<'state, 'vm> LayoutResolver for TypeLayoutResolver<'state, 'vm> {
    fn get_layout(
        &mut self,
        object: &MoveObject,
        format: ObjectFormatOptions,
    ) -> Result<MoveStructLayout, SuiError> {
        let struct_tag: StructTag = object.type_().clone().into();
        let Ok(ty) = load_type_from_struct(self.vm, &mut self.linkage_view, &LinkedHashMap::new(), &struct_tag) else {
            return Err(SuiError::FailObjectLayout {
                st: format!("{}", struct_tag),
            });
        };
        let layout = if format.include_types() {
            self.vm.get_runtime().type_to_fully_annotated_layout(&ty)
        } else {
            self.vm.get_runtime().type_to_type_layout(&ty)
        };
        let Ok(MoveTypeLayout::Struct(layout)) = layout else {
            return Err(SuiError::FailObjectLayout {
                st: format!("{}", struct_tag),
            })
        };
        Ok(layout)
    }
}

impl<'state> BackingPackageStore for NullSuiResolver<'state> {
    fn get_package_object(&self, package_id: &ObjectID) -> SuiResult<Option<Object>> {
        self.0.get_package_object(package_id)
    }
}

impl<'state> ModuleResolver for NullSuiResolver<'state> {
    type Error = SuiError;

    fn get_module(&self, id: &ModuleId) -> Result<Option<Vec<u8>>, Self::Error> {
        self.0.get_module(id)
    }
}

impl<'state> ResourceResolver for NullSuiResolver<'state> {
    type Error = SuiError;

    fn get_resource(
        &self,
        _address: &AccountAddress,
        _typ: &StructTag,
    ) -> Result<Option<Vec<u8>>, Self::Error> {
        Ok(None)
    }
}
