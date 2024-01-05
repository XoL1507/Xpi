// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

pub use checked::*;

#[sui_macros::with_checked_arithmetic]
mod checked {
    use std::{
        borrow::Borrow,
        collections::{BTreeMap, BTreeSet, HashMap},
        sync::Arc,
    };

    use crate::adapter::new_native_extensions;
    use crate::error::convert_vm_error;
    use linked_hash_map::LinkedHashMap;
    use move_binary_format::errors::{PartialVMError, PartialVMResult};
    use move_binary_format::{
        errors::{Location, VMError, VMResult},
        file_format::{CodeOffset, FunctionDefinitionIndex, TypeParameterIndex},
        CompiledModule,
    };
    use move_core_types::gas_algebra::NumBytes;
    use move_core_types::resolver::ModuleResolver;
    use move_core_types::value::MoveTypeLayout;
    use move_core_types::vm_status::StatusCode;
    use move_core_types::{
        account_address::AccountAddress,
        identifier::IdentStr,
        language_storage::{ModuleId, StructTag, TypeTag},
    };
    #[cfg(debug_assertions)]
    use move_vm_profiler::GasProfiler;
    use move_vm_runtime::native_extensions::NativeContextExtensions;
    use move_vm_runtime::{
        move_vm::MoveVM,
        session::{LoadedFunctionInstantiation, SerializedReturnValues},
    };
    use move_vm_types::data_store::DataStore;
    #[cfg(debug_assertions)]
    use move_vm_types::gas::GasMeter;
    use move_vm_types::loaded_data::runtime_types::Type;
    use move_vm_types::values::{GlobalValue, Value as VMValue};
    use sui_move_natives::object_runtime::{
        self, get_all_uids, max_event_error, ObjectRuntime, RuntimeResults,
    };
    use sui_protocol_config::ProtocolConfig;
    use sui_types::gas::GasCharger;
    use sui_types::{
        balance::Balance,
        base_types::{MoveObjectType, ObjectID, SequenceNumber, SuiAddress, TxContext},
        coin::Coin,
        error::{ExecutionError, ExecutionErrorKind},
        execution::{
            ExecutionResults, ExecutionState, InputObjectMetadata, InputValue, ObjectValue,
            RawValueType, ResultValue, UsageKind,
        },
        metrics::LimitsMetrics,
        move_package::MovePackage,
        object::{Data, MoveObject, Object, Owner},
        storage::{
            BackingPackageStore, DeleteKind, DeleteKindWithOldVersion, ObjectChange, WriteKind,
        },
        transaction::{Argument, CallArg, ObjectArg},
        type_resolver::TypeTagResolver,
    };
    use sui_types::{
        error::command_argument_error,
        execution::{CommandKind, ObjectContents, TryFromValue, Value},
        execution_mode::ExecutionMode,
        execution_status::CommandArgumentError,
    };

    use crate::programmable_transactions::linkage_view::LinkageView;

    /// Maintains all runtime state specific to programmable transactions
    pub struct ExecutionContext<'vm, 'state, 'a> {
        /// The protocol config
        pub protocol_config: &'a ProtocolConfig,
        /// Metrics for reporting exceeded limits
        pub metrics: Arc<LimitsMetrics>,
        /// The MoveVM
        pub vm: &'vm MoveVM,
        /// The LinkageView for this session
        pub linkage_view: LinkageView<'state>,
        pub native_extensions: NativeContextExtensions<'state>,
        /// The global state, used for resolving packages
        pub state_view: &'state dyn ExecutionState,
        /// A shared transaction context, contains transaction digest information and manages the
        /// creation of new object IDs
        pub tx_context: &'a mut TxContext,
        /// The gas charger used for metering
        pub gas_charger: &'a mut GasCharger,
        /// Additional transfers not from the Move runtime
        additional_transfers: Vec<(/* new owner */ SuiAddress, ObjectValue)>,
        /// Newly published packages
        new_packages: LinkedHashMap<ObjectID, MovePackage>,
        /// User events are claimed after each Move call
        user_events: Vec<(ModuleId, StructTag, Vec<u8>)>,
        // runtime data
        /// The runtime value for the Gas coin, None if it has been taken/moved
        gas: InputValue,
        /// The runtime value for the inputs/call args, None if it has been taken/moved
        inputs: Vec<InputValue>,
        /// The results of a given command. For most commands, the inner vector will have length 1.
        /// It will only not be 1 for Move calls with multiple return values.
        /// Inner values are None if taken/moved by-value
        results: Vec<Vec<ResultValue>>,
        /// Map of arguments that are currently borrowed in this command, true if the borrow is mutable
        /// This gets cleared out when new results are pushed, i.e. the end of a command
        borrowed: HashMap<Argument, /* mut */ bool>,
    }

    /// A write for an object that was generated outside of the Move ObjectRuntime
    struct AdditionalWrite {
        /// The new owner of the object
        recipient: Owner,
        /// the type of the object,
        type_: Type,
        /// if the object has public transfer or not, i.e. if it has store
        has_public_transfer: bool,
        /// contents of the object
        bytes: Vec<u8>,
    }

    impl<'vm, 'state, 'a> ExecutionContext<'vm, 'state, 'a> {
        pub fn new(
            protocol_config: &'a ProtocolConfig,
            metrics: Arc<LimitsMetrics>,
            vm: &'vm MoveVM,
            state_view: &'state dyn ExecutionState,
            tx_context: &'a mut TxContext,
            gas_charger: &'a mut GasCharger,
            inputs: Vec<CallArg>,
        ) -> Result<Self, ExecutionError> {
            let mut linkage_view = LinkageView::new(Box::new(state_view.as_sui_resolver()));
            let mut input_object_map = BTreeMap::new();
            let inputs = inputs
                .into_iter()
                .map(|call_arg| {
                    load_call_arg(
                        vm,
                        state_view,
                        &mut linkage_view,
                        &LinkedHashMap::new(),
                        &mut input_object_map,
                        call_arg,
                    )
                })
                .collect::<Result<_, ExecutionError>>()?;
            let gas = if let Some(gas_coin) = gas_charger.gas_coin() {
                let mut gas = load_object(
                    vm,
                    state_view,
                    &mut linkage_view,
                    &LinkedHashMap::new(),
                    &mut input_object_map,
                    /* imm override */ false,
                    gas_coin,
                )?;
                // subtract the max gas budget. This amount is off limits in the programmable transaction,
                // so to mimic this "off limits" behavior, we act as if the coin has less balance than
                // it really does
                let Some(Value::Object(ObjectValue {
                    contents: ObjectContents::Coin(coin),
                    ..
                })) = &mut gas.inner.value else {
                    invariant_violation!("Gas object should be a populated coin")
                };

                let max_gas_in_balance = gas_charger.gas_budget();
                let Some(new_balance) = coin.balance.value().checked_sub(max_gas_in_balance) else {
                    invariant_violation!(
                        "Transaction input checker should check that there is enough gas"
                    );
                };
                coin.balance = Balance::new(new_balance);
                gas
            } else {
                InputValue {
                    object_metadata: None,
                    inner: ResultValue {
                        last_usage_kind: None,
                        value: None,
                    },
                }
            };
            let native_extensions = new_native_extensions(
                state_view.as_child_resolver(),
                input_object_map,
                !gas_charger.is_unmetered(),
                protocol_config,
                metrics.clone(),
            );

            // Set the profiler if in debug mode
            #[cfg(debug_assertions)]
            {
                let tx_digest = tx_context.digest();
                let remaining_gas: u64 =
                    move_vm_types::gas::GasMeter::remaining_gas(gas_charger.move_gas_status())
                        .into();
                gas_charger
                    .move_gas_status_mut()
                    .set_profiler(GasProfiler::init(
                        &vm.config().profiler_config,
                        format!("{}", tx_digest),
                        remaining_gas,
                    ));
            }

            Ok(Self {
                protocol_config,
                metrics,
                vm,
                linkage_view,
                native_extensions,
                state_view,
                tx_context,
                gas_charger,
                gas,
                inputs,
                results: vec![],
                additional_transfers: vec![],
                new_packages: LinkedHashMap::new(),
                user_events: vec![],
                borrowed: HashMap::new(),
            })
        }

        pub fn object_runtime(&mut self) -> &ObjectRuntime {
            self.native_extensions.get()
        }

        /// Create a new ID and update the state
        pub fn fresh_id(&mut self) -> Result<ObjectID, ExecutionError> {
            let object_id = self.tx_context.fresh_id();
            let object_runtime: &mut ObjectRuntime = self.native_extensions.get_mut();
            object_runtime
                .new_id(object_id)
                .map_err(|e| self.convert_vm_error(e.finish(Location::Undefined)))?;
            Ok(object_id)
        }

        /// Delete an ID and update the state
        pub fn delete_id(&mut self, object_id: ObjectID) -> Result<(), ExecutionError> {
            let object_runtime: &mut ObjectRuntime = self.native_extensions.get_mut();
            object_runtime
                .delete_id(object_id)
                .map_err(|e| self.convert_vm_error(e.finish(Location::Undefined)))
        }

        /// Set the link context for the session from the linkage information in the MovePackage found
        /// at `package_id`.  Returns the runtime ID of the link context package on success.
        pub fn set_link_context(
            &mut self,
            package_id: ObjectID,
        ) -> Result<AccountAddress, ExecutionError> {
            if self.linkage_view.has_linkage(package_id) {
                // Setting same context again, can skip.
                return Ok(self
                    .linkage_view
                    .original_package_id()
                    .unwrap_or(*package_id));
            }

            let package = package_for_linkage(&self.linkage_view, package_id)
                .map_err(|e| self.convert_vm_error(e))?;

            self.linkage_view.set_linkage(&package)
        }

        /// Load a type using the context's current session.
        pub fn load_type(&mut self, type_tag: &TypeTag) -> VMResult<Type> {
            load_type(
                self.vm,
                &mut self.linkage_view,
                &self.new_packages,
                type_tag,
            )
        }

        /// Load a type using the context's current session.
        pub fn load_type_from_struct(&mut self, struct_tag: &StructTag) -> VMResult<Type> {
            load_type_from_struct(
                self.vm,
                &mut self.linkage_view,
                &self.new_packages,
                struct_tag,
            )
        }

        /// Takes the user events from the runtime and tags them with the Move module of the function
        /// that was invoked for the command
        pub fn take_user_events(
            &mut self,
            module_id: &ModuleId,
            function: FunctionDefinitionIndex,
            last_offset: CodeOffset,
        ) -> Result<(), ExecutionError> {
            let object_runtime: &mut ObjectRuntime = self.native_extensions.get_mut();
            let events = object_runtime.take_user_events();
            let num_events = self.user_events.len() + events.len();
            let max_events = self.protocol_config.max_num_event_emit();
            if num_events as u64 > max_events {
                let err = max_event_error(max_events)
                    .at_code_offset(function, last_offset)
                    .finish(Location::Module(module_id.clone()));
                return Err(self.convert_vm_error(err));
            }
            let new_events = events
                .into_iter()
                .map(|(ty, tag, value)| {
                    let layout = self
                        .vm
                        .get_runtime()
                        .type_to_type_layout(&ty)
                        .map_err(|e| self.convert_vm_error(e))?;
                    let Some(bytes) = value.simple_serialize(&layout) else {
                        invariant_violation!("Failed to deserialize already serialized Move value");
                    };
                    Ok((module_id.clone(), tag, bytes))
                })
                .collect::<Result<Vec<_>, ExecutionError>>()?;
            self.user_events.extend(new_events);
            Ok(())
        }

        /// Get the argument value. Cloning the value if it is copyable, and setting its value to None
        /// if it is not (making it unavailable).
        /// Errors if out of bounds, if the argument is borrowed, if it is unavailable (already taken),
        /// or if it is an object that cannot be taken by value (shared or immutable)
        pub fn by_value_arg<V: TryFromValue>(
            &mut self,
            command_kind: CommandKind<'_>,
            arg_idx: usize,
            arg: Argument,
        ) -> Result<V, ExecutionError> {
            self.by_value_arg_(command_kind, arg)
                .map_err(|e| command_argument_error(e, arg_idx))
        }
        fn by_value_arg_<V: TryFromValue>(
            &mut self,
            command_kind: CommandKind<'_>,
            arg: Argument,
        ) -> Result<V, CommandArgumentError> {
            let is_borrowed = self.arg_is_borrowed(&arg);
            let (input_metadata_opt, val_opt) = self.borrow_mut(arg, UsageKind::ByValue)?;
            let is_copyable = if let Some(val) = val_opt {
                val.is_copyable()
            } else {
                return Err(CommandArgumentError::InvalidValueUsage);
            };
            // If it was taken, we catch this above.
            // If it was not copyable and was borrowed, error as it creates a dangling reference in
            // effect.
            // We allow copyable values to be copied out even if borrowed, as we do not care about
            // referential transparency at this level.
            if !is_copyable && is_borrowed {
                return Err(CommandArgumentError::InvalidValueUsage);
            }
            // Gas coin cannot be taken by value, except in TransferObjects
            if matches!(arg, Argument::GasCoin)
                && !matches!(command_kind, CommandKind::TransferObjects)
            {
                return Err(CommandArgumentError::InvalidGasCoinUsage);
            }
            // Immutable objects and shared objects cannot be taken by value
            if matches!(
                input_metadata_opt,
                Some(InputObjectMetadata {
                    owner: Owner::Immutable | Owner::Shared { .. },
                    ..
                })
            ) {
                return Err(CommandArgumentError::InvalidObjectByValue);
            }
            let val = if is_copyable {
                val_opt.as_ref().unwrap().clone()
            } else {
                val_opt.take().unwrap()
            };
            V::try_from_value(val)
        }

        /// Mimic a mutable borrow by taking the argument value, setting its value to None,
        /// making it unavailable. The value will be marked as borrowed and must be returned with
        /// restore_arg
        /// Errors if out of bounds, if the argument is borrowed, if it is unavailable (already taken),
        /// or if it is an object that cannot be mutably borrowed (immutable)
        pub fn borrow_arg_mut<V: TryFromValue>(
            &mut self,
            arg_idx: usize,
            arg: Argument,
        ) -> Result<V, ExecutionError> {
            self.borrow_arg_mut_(arg)
                .map_err(|e| command_argument_error(e, arg_idx))
        }
        fn borrow_arg_mut_<V: TryFromValue>(
            &mut self,
            arg: Argument,
        ) -> Result<V, CommandArgumentError> {
            // mutable borrowing requires unique usage
            if self.arg_is_borrowed(&arg) {
                return Err(CommandArgumentError::InvalidValueUsage);
            }
            self.borrowed.insert(arg, /* is_mut */ true);
            let (input_metadata_opt, val_opt) = self.borrow_mut(arg, UsageKind::BorrowMut)?;
            let is_copyable = if let Some(val) = val_opt {
                val.is_copyable()
            } else {
                // error if taken
                return Err(CommandArgumentError::InvalidValueUsage);
            };
            if input_metadata_opt.is_some() && !input_metadata_opt.unwrap().is_mutable_input {
                return Err(CommandArgumentError::InvalidObjectByMutRef);
            }
            // if it is copyable, don't take it as we allow for the value to be copied even if
            // mutably borrowed
            let val = if is_copyable {
                val_opt.as_ref().unwrap().clone()
            } else {
                val_opt.take().unwrap()
            };
            V::try_from_value(val)
        }

        /// Mimics an immutable borrow by cloning the argument value without setting its value to None
        /// Errors if out of bounds, if the argument is mutably borrowed,
        /// or if it is unavailable (already taken)
        pub fn borrow_arg<V: TryFromValue>(
            &mut self,
            arg_idx: usize,
            arg: Argument,
        ) -> Result<V, ExecutionError> {
            self.borrow_arg_(arg)
                .map_err(|e| command_argument_error(e, arg_idx))
        }
        fn borrow_arg_<V: TryFromValue>(
            &mut self,
            arg: Argument,
        ) -> Result<V, CommandArgumentError> {
            // immutable borrowing requires the value was not mutably borrowed.
            // If it was copied, that is okay.
            // If it was taken/moved, we will find out below
            if self.arg_is_mut_borrowed(&arg) {
                return Err(CommandArgumentError::InvalidValueUsage);
            }
            self.borrowed.insert(arg, /* is_mut */ false);
            let (_input_metadata_opt, val_opt) = self.borrow_mut(arg, UsageKind::BorrowImm)?;
            if val_opt.is_none() {
                return Err(CommandArgumentError::InvalidValueUsage);
            }
            V::try_from_value(val_opt.as_ref().unwrap().clone())
        }

        /// Restore an argument after being mutably borrowed
        pub fn restore_arg<Mode: ExecutionMode>(
            &mut self,
            updates: &mut Mode::ArgumentUpdates,
            arg: Argument,
            value: Value,
        ) -> Result<(), ExecutionError> {
            Mode::add_argument_update(self, updates, arg, &value)?;
            let was_mut_opt = self.borrowed.remove(&arg);
            assert_invariant!(
                was_mut_opt.is_some() && was_mut_opt.unwrap(),
                "Should never restore a non-mut borrowed value. \
                The take+restore is an implementation detail of mutable references"
            );
            // restore is exclusively used for mut
            let Ok((_, value_opt)) = self.borrow_mut_impl(arg, None) else {
                invariant_violation!("Should be able to borrow argument to restore it")
            };
            let old_value = value_opt.replace(value);
            assert_invariant!(
                old_value.is_none() || old_value.unwrap().is_copyable(),
                "Should never restore a non-taken value, unless it is copyable. \
                The take+restore is an implementation detail of mutable references"
            );
            Ok(())
        }

        /// Transfer the object to a new owner
        pub fn transfer_object(
            &mut self,
            obj: ObjectValue,
            addr: SuiAddress,
        ) -> Result<(), ExecutionError> {
            self.additional_transfers.push((addr, obj));
            Ok(())
        }

        /// Create a new package
        pub fn new_package<'p>(
            &self,
            modules: &[CompiledModule],
            dependencies: impl IntoIterator<Item = &'p MovePackage>,
        ) -> Result<MovePackage, ExecutionError> {
            MovePackage::new_initial(
                modules,
                self.protocol_config.max_move_package_size(),
                dependencies,
            )
        }

        /// Create a package upgrade from `previous_package` with `new_modules` and `dependencies`
        pub fn upgrade_package<'p>(
            &self,
            storage_id: ObjectID,
            previous_package: &MovePackage,
            new_modules: &[CompiledModule],
            dependencies: impl IntoIterator<Item = &'p MovePackage>,
        ) -> Result<MovePackage, ExecutionError> {
            previous_package.new_upgraded(
                storage_id,
                new_modules,
                self.protocol_config,
                dependencies,
            )
        }

        /// Add a newly created package to write as an effect of the transaction
        pub fn write_package(&mut self, package: MovePackage) {
            self.new_packages.insert(package.id(), package);
        }

        /// Return the last pacakge pushed in `write_package`.
        /// This function should be used in block of codes that push a pacakge, verify
        /// it, run the init and in case of error will remove the package.
        /// The package has to be pushed for the init to run correctly.
        pub fn remove_package(&mut self, id: ObjectID) -> Option<MovePackage> {
            self.new_packages.remove(&id)
        }

        /// Finish a command: clearing the borrows and adding the results to the result vector
        pub fn push_command_results(&mut self, results: Vec<Value>) -> Result<(), ExecutionError> {
            assert_invariant!(
                self.borrowed.values().all(|is_mut| !is_mut),
                "all mut borrows should be restored"
            );
            // clear borrow state
            self.borrowed = HashMap::new();
            self.results
                .push(results.into_iter().map(ResultValue::new).collect());
            Ok(())
        }

        /// Determine the object changes and collect all user events
        pub fn finish<Mode: ExecutionMode>(self) -> Result<ExecutionResults, ExecutionError> {
            let Self {
                protocol_config,
                vm,
                linkage_view,
                mut native_extensions,
                tx_context,
                gas_charger,
                additional_transfers,
                new_packages,
                gas,
                inputs,
                results,
                user_events,
                ..
            } = self;
            let tx_digest = tx_context.digest();
            let mut additional_writes = BTreeMap::new();
            let mut input_object_metadata = BTreeMap::new();
            // Any object value that has not been taken (still has `Some` for it's value) needs to
            // written as it's value might have changed (and eventually it's sequence number will need
            // to increase)
            let mut by_value_inputs = BTreeSet::new();
            let mut add_input_object_write = |input| -> Result<(), ExecutionError> {
                let InputValue {
                    object_metadata: object_metadata_opt,
                    inner: ResultValue { value, .. },
                } = input;
                let Some(object_metadata) = object_metadata_opt else { return Ok(()) };
                let is_mutable_input = object_metadata.is_mutable_input;
                let owner = object_metadata.owner;
                let id = object_metadata.id;
                input_object_metadata.insert(object_metadata.id, object_metadata);
                let Some(Value::Object(object_value)) = value else {
                    by_value_inputs.insert(id);
                    return Ok(())
                };
                if is_mutable_input {
                    add_additional_write(&mut additional_writes, owner, object_value)?;
                }
                Ok(())
            };
            let gas_id_opt = gas.object_metadata.as_ref().map(|info| info.id);
            add_input_object_write(gas)?;
            for input in inputs {
                add_input_object_write(input)?
            }
            // check for unused values
            // disable this check for dev inspect
            if !Mode::allow_arbitrary_values() {
                for (i, command_result) in results.iter().enumerate() {
                    for (j, result_value) in command_result.iter().enumerate() {
                        let ResultValue {
                            last_usage_kind,
                            value,
                        } = result_value;
                        match value {
                            None => (),
                            Some(Value::Object(_)) => {
                                return Err(ExecutionErrorKind::UnusedValueWithoutDrop {
                                    result_idx: i as u16,
                                    secondary_idx: j as u16,
                                }
                                .into())
                            }
                            Some(Value::Raw(RawValueType::Any, _)) => (),
                            Some(Value::Raw(RawValueType::Loaded { abilities, .. }, _)) => {
                                // - nothing to check for drop
                                // - if it does not have drop, but has copy,
                                //   the last usage must be by value in order to "lie" and say that the
                                //   last usage is actually a take instead of a clone
                                // - Otherwise, an error
                                if abilities.has_drop()
                                    || (abilities.has_copy()
                                        && matches!(last_usage_kind, Some(UsageKind::ByValue)))
                                {
                                } else {
                                    let msg = if abilities.has_copy() {
                                        "The value has copy, but not drop. \
                                        Its last usage must be by-value so it can be taken."
                                    } else {
                                        "Unused value without drop"
                                    };
                                    return Err(ExecutionError::new_with_source(
                                        ExecutionErrorKind::UnusedValueWithoutDrop {
                                            result_idx: i as u16,
                                            secondary_idx: j as u16,
                                        },
                                        msg,
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            // add transfers from TransferObjects command
            for (recipient, object_value) in additional_transfers {
                let owner = Owner::AddressOwner(recipient);
                add_additional_write(&mut additional_writes, owner, object_value)?;
            }
            // Refund unused gas
            if let Some(gas_id) = gas_id_opt {
                refund_max_gas_budget(&mut additional_writes, gas_charger, gas_id)?;
            }

            let object_runtime: ObjectRuntime = native_extensions.remove();
            let new_ids = object_runtime.new_ids().clone();
            // tell the object runtime what input objects were taken and which were transferred
            let external_transfers = additional_writes.keys().copied().collect();
            let RuntimeResults {
                writes,
                deletions,
                user_events: remaining_events,
                loaded_child_objects,
            } = object_runtime.finish(by_value_inputs, external_transfers)?;
            assert_invariant!(
                remaining_events.is_empty(),
                "Events should be taken after every Move call"
            );
            let mut object_changes = BTreeMap::new();
            for (_, package) in new_packages {
                let package_obj = Object::new_from_package(package, tx_digest);
                let id = package_obj.id();
                let change = ObjectChange::Write(package_obj, WriteKind::Create);
                object_changes.insert(id, change);
            }
            for (id, additional_write) in additional_writes {
                let AdditionalWrite {
                    recipient,
                    type_,
                    has_public_transfer,
                    bytes,
                } = additional_write;
                let write_kind = if input_object_metadata.contains_key(&id)
                    || loaded_child_objects.contains_key(&id)
                {
                    assert_invariant!(
                        !new_ids.contains_key(&id),
                        "new id should not be in mutations"
                    );
                    WriteKind::Mutate
                } else if new_ids.contains_key(&id) {
                    WriteKind::Create
                } else {
                    WriteKind::Unwrap
                };
                // safe given the invariant that the runtime correctly propagates has_public_transfer
                let move_object = unsafe {
                    create_written_object(
                        vm,
                        &linkage_view,
                        protocol_config,
                        &input_object_metadata,
                        &loaded_child_objects,
                        id,
                        type_,
                        has_public_transfer,
                        bytes,
                        write_kind,
                    )?
                };
                let object = Object::new_move(move_object, recipient, tx_digest);
                let change = ObjectChange::Write(object, write_kind);
                object_changes.insert(id, change);
            }

            for (id, (write_kind, recipient, ty, value)) in writes {
                let abilities = vm
                    .get_runtime()
                    .get_type_abilities(&ty)
                    .map_err(|e| convert_vm_error(e, vm, &linkage_view))?;
                let has_public_transfer = abilities.has_store();
                let layout = vm
                    .get_runtime()
                    .type_to_type_layout(&ty)
                    .map_err(|e| convert_vm_error(e, vm, &linkage_view))?;
                let Some(bytes) = value.simple_serialize(&layout) else {
                    invariant_violation!("Failed to deserialize already serialized Move value");
                };
                // safe because has_public_transfer has been determined by the abilities
                let move_object = unsafe {
                    create_written_object(
                        vm,
                        &linkage_view,
                        protocol_config,
                        &input_object_metadata,
                        &loaded_child_objects,
                        id,
                        ty,
                        has_public_transfer,
                        bytes,
                        write_kind,
                    )?
                };
                let object = Object::new_move(move_object, recipient, tx_digest);
                let change = ObjectChange::Write(object, write_kind);
                object_changes.insert(id, change);
            }
            for (id, delete_kind) in deletions {
                // For deleted and wrapped objects, the object must exist either in the input or was
                // loaded as child object. We can read them to get the previous version.
                // For unwrap_then_delete, we don't need the old version.
                let delete_kind_with_seq = match delete_kind {
                    DeleteKind::Normal | DeleteKind::Wrap => {
                        let old_version = match input_object_metadata.get(&id) {
                            Some(metadata) => {
                                assert_invariant!(
                                    !matches!(metadata.owner, Owner::Immutable),
                                    "Attempting to delete immutable object {id} via delete kind {delete_kind}"
                                );
                                metadata.version
                            }
                            None => {
                                match loaded_child_objects.get(&id) {
                                    Some(version) => *version,
                                    None => invariant_violation!("Deleted/wrapped object {id} must be either in input or loaded child objects")
                                }
                            }
                        };
                        if delete_kind == DeleteKind::Normal {
                            DeleteKindWithOldVersion::Normal(old_version)
                        } else {
                            DeleteKindWithOldVersion::Wrap(old_version)
                        }
                    }
                    DeleteKind::UnwrapThenDelete => DeleteKindWithOldVersion::UnwrapThenDelete,
                };
                object_changes.insert(id, ObjectChange::Delete(delete_kind_with_seq));
            }

            Ok(ExecutionResults {
                object_changes,
                user_events,
            })
        }

        /// Convert a VM Error to an execution one
        pub fn convert_vm_error(&self, error: VMError) -> ExecutionError {
            crate::error::convert_vm_error(error, self.vm, &self.linkage_view)
        }

        /// Special case errors for type arguments to Move functions
        pub fn convert_type_argument_error(&self, idx: usize, error: VMError) -> ExecutionError {
            use move_core_types::vm_status::StatusCode;
            use sui_types::execution_status::TypeArgumentError;
            match error.major_status() {
                StatusCode::NUMBER_OF_TYPE_ARGUMENTS_MISMATCH => {
                    ExecutionErrorKind::TypeArityMismatch.into()
                }
                StatusCode::TYPE_RESOLUTION_FAILURE => ExecutionErrorKind::TypeArgumentError {
                    argument_idx: idx as TypeParameterIndex,
                    kind: TypeArgumentError::TypeNotFound,
                }
                .into(),
                StatusCode::CONSTRAINT_NOT_SATISFIED => ExecutionErrorKind::TypeArgumentError {
                    argument_idx: idx as TypeParameterIndex,
                    kind: TypeArgumentError::ConstraintNotSatisfied,
                }
                .into(),
                _ => self.convert_vm_error(error),
            }
        }

        /// Returns true if the value at the argument's location is borrowed, mutably or immutably
        fn arg_is_borrowed(&self, arg: &Argument) -> bool {
            self.borrowed.contains_key(arg)
        }

        /// Returns true if the value at the argument's location is mutably borrowed
        fn arg_is_mut_borrowed(&self, arg: &Argument) -> bool {
            matches!(self.borrowed.get(arg), Some(/* mut */ true))
        }

        /// Internal helper to borrow the value for an argument and update the most recent usage
        fn borrow_mut(
            &mut self,
            arg: Argument,
            usage: UsageKind,
        ) -> Result<(Option<&InputObjectMetadata>, &mut Option<Value>), CommandArgumentError>
        {
            self.borrow_mut_impl(arg, Some(usage))
        }

        /// Internal helper to borrow the value for an argument
        /// Updates the most recent usage if specified
        fn borrow_mut_impl(
            &mut self,
            arg: Argument,
            update_last_usage: Option<UsageKind>,
        ) -> Result<(Option<&InputObjectMetadata>, &mut Option<Value>), CommandArgumentError>
        {
            let (metadata, result_value) = match arg {
                Argument::GasCoin => (self.gas.object_metadata.as_ref(), &mut self.gas.inner),
                Argument::Input(i) => {
                    let Some(input_value) = self.inputs.get_mut(i as usize) else {
                        return Err(CommandArgumentError::IndexOutOfBounds { idx: i });
                    };
                    (input_value.object_metadata.as_ref(), &mut input_value.inner)
                }
                Argument::Result(i) => {
                    let Some(command_result) = self.results.get_mut(i as usize) else {
                        return Err(CommandArgumentError::IndexOutOfBounds { idx: i });
                    };
                    if command_result.len() != 1 {
                        return Err(CommandArgumentError::InvalidResultArity { result_idx: i });
                    }
                    (None, &mut command_result[0])
                }
                Argument::NestedResult(i, j) => {
                    let Some(command_result) = self.results.get_mut(i as usize) else {
                        return Err(CommandArgumentError::IndexOutOfBounds { idx: i });
                    };
                    let Some(result_value) = command_result.get_mut(j as usize) else {
                        return Err(CommandArgumentError::SecondaryIndexOutOfBounds {
                            result_idx: i,
                            secondary_idx: j,
                        });
                    };
                    (None, result_value)
                }
            };
            if let Some(usage) = update_last_usage {
                result_value.last_usage_kind = Some(usage);
            }
            Ok((metadata, &mut result_value.value))
        }

        pub(crate) fn execute_function_bypass_visibility(
            &mut self,
            module: &ModuleId,
            function_name: &IdentStr,
            ty_args: Vec<Type>,
            args: Vec<impl Borrow<[u8]>>,
        ) -> VMResult<SerializedReturnValues> {
            let gas_status = self.gas_charger.move_gas_status_mut();
            let mut data_store = SuiDataStore::new(&self.linkage_view, &self.new_packages);
            self.vm.get_runtime().execute_function_bypass_visibility(
                module,
                function_name,
                ty_args,
                args,
                &mut data_store,
                gas_status,
                &mut self.native_extensions,
            )
        }

        pub(crate) fn load_function(
            &mut self,
            module_id: &ModuleId,
            function_name: &IdentStr,
            type_arguments: &[Type],
        ) -> VMResult<LoadedFunctionInstantiation> {
            let mut data_store = SuiDataStore::new(&self.linkage_view, &self.new_packages);
            self.vm.get_runtime().load_function(
                module_id,
                function_name,
                type_arguments,
                &mut data_store,
            )
        }

        pub(crate) fn make_object_value(
            &mut self,
            type_: MoveObjectType,
            has_public_transfer: bool,
            used_in_non_entry_move_call: bool,
            contents: &[u8],
        ) -> Result<ObjectValue, ExecutionError> {
            make_object_value(
                self.vm,
                &mut self.linkage_view,
                &self.new_packages,
                type_,
                has_public_transfer,
                used_in_non_entry_move_call,
                contents,
            )
        }

        pub fn publish_module_bundle(
            &mut self,
            modules: Vec<Vec<u8>>,
            sender: AccountAddress,
        ) -> VMResult<()> {
            // TODO: publish_module_bundle() currently doesn't charge gas.
            // Do we want to charge there?
            let mut data_store = SuiDataStore::new(&self.linkage_view, &self.new_packages);
            self.vm.get_runtime().publish_module_bundle(
                modules,
                sender,
                &mut data_store,
                self.gas_charger.move_gas_status_mut(),
            )
        }
    }

    impl<'vm, 'state, 'a> TypeTagResolver for ExecutionContext<'vm, 'state, 'a> {
        fn get_type_tag(&self, type_: &Type) -> Result<TypeTag, ExecutionError> {
            self.vm
                .get_runtime()
                .get_type_tag(type_)
                .map_err(|e| self.convert_vm_error(e))
        }
    }

    /// Fetch the package at `package_id` with a view to using it as a link context.  Produces an error
    /// if the object at that ID does not exist, or is not a package.
    fn package_for_linkage(
        linkage_view: &LinkageView,
        package_id: ObjectID,
    ) -> VMResult<MovePackage> {
        use move_binary_format::errors::PartialVMError;
        use move_core_types::vm_status::StatusCode;

        match linkage_view.get_package(&package_id) {
            Ok(Some(package)) => Ok(package),
            Ok(None) => Err(PartialVMError::new(StatusCode::LINKER_ERROR)
                .with_message(format!("Cannot find link context {package_id} in store"))
                .finish(Location::Undefined)),
            Err(err) => Err(PartialVMError::new(StatusCode::LINKER_ERROR)
                .with_message(format!(
                    "Error loading link context {package_id} from store: {err}"
                ))
                .finish(Location::Undefined)),
        }
    }

    pub fn load_type_from_struct(
        vm: &MoveVM,
        linkage_view: &mut LinkageView,
        new_packages: &LinkedHashMap<ObjectID, MovePackage>,
        struct_tag: &StructTag,
    ) -> VMResult<Type> {
        fn verification_error<T>(code: StatusCode) -> VMResult<T> {
            Err(PartialVMError::new(code).finish(Location::Undefined))
        }

        let StructTag {
            address,
            module,
            name,
            type_params,
        } = struct_tag;

        // Load the package that the struct is defined in, in storage
        let defining_id = ObjectID::from_address(*address);
        let package = package_for_linkage(linkage_view, defining_id)?;

        // Set the defining package as the link context while loading the
        // struct
        let original_address = linkage_view.set_linkage(&package).map_err(|e| {
            PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR)
                .with_message(e.to_string())
                .finish(Location::Undefined)
        })?;

        let runtime_id = ModuleId::new(original_address, module.clone());
        let data_store = SuiDataStore::new(linkage_view, new_packages);
        let res = vm.get_runtime().load_struct(&runtime_id, name, &data_store);
        linkage_view.reset_linkage();
        let (idx, struct_type) = res?;

        // Recursively load type parameters, if necessary
        let type_param_constraints = struct_type.type_param_constraints();
        if type_param_constraints.len() != type_params.len() {
            return verification_error(StatusCode::NUMBER_OF_TYPE_ARGUMENTS_MISMATCH);
        }

        if type_params.is_empty() {
            Ok(Type::Struct(idx))
        } else {
            let loaded_type_params = type_params
                .iter()
                .map(|type_param| load_type(vm, linkage_view, new_packages, type_param))
                .collect::<VMResult<Vec<_>>>()?;

            // Verify that the type parameter constraints on the struct are met
            for (constraint, param) in type_param_constraints.zip(&loaded_type_params) {
                let abilities = vm.get_runtime().get_type_abilities(param)?;
                if !constraint.is_subset(abilities) {
                    return verification_error(StatusCode::CONSTRAINT_NOT_SATISFIED);
                }
            }

            Ok(Type::StructInstantiation(idx, loaded_type_params))
        }
    }

    /// Load `type_tag` to get a `Type` in the provided `session`.  `session`'s linkage context may be
    /// reset after this operation, because during the operation, it may change when loading a struct.
    pub fn load_type(
        vm: &MoveVM,
        linkage_view: &mut LinkageView,
        new_packages: &LinkedHashMap<ObjectID, MovePackage>,
        type_tag: &TypeTag,
    ) -> VMResult<Type> {
        Ok(match type_tag {
            TypeTag::Bool => Type::Bool,
            TypeTag::U8 => Type::U8,
            TypeTag::U16 => Type::U16,
            TypeTag::U32 => Type::U32,
            TypeTag::U64 => Type::U64,
            TypeTag::U128 => Type::U128,
            TypeTag::U256 => Type::U256,
            TypeTag::Address => Type::Address,
            TypeTag::Signer => Type::Signer,

            TypeTag::Vector(inner) => {
                Type::Vector(Box::new(load_type(vm, linkage_view, new_packages, inner)?))
            }
            TypeTag::Struct(struct_tag) => {
                return load_type_from_struct(vm, linkage_view, new_packages, struct_tag)
            }
        })
    }

    pub(crate) fn make_object_value(
        vm: &MoveVM,
        linkage_view: &mut LinkageView,
        new_packages: &LinkedHashMap<ObjectID, MovePackage>,
        type_: MoveObjectType,
        has_public_transfer: bool,
        used_in_non_entry_move_call: bool,
        contents: &[u8],
    ) -> Result<ObjectValue, ExecutionError> {
        let contents = if type_.is_coin() {
            let Ok(coin) = Coin::from_bcs_bytes(contents) else {
                invariant_violation!("Could not deserialize a coin")
            };
            ObjectContents::Coin(coin)
        } else {
            ObjectContents::Raw(contents.to_vec())
        };

        let tag: StructTag = type_.into();
        let type_ = load_type_from_struct(vm, linkage_view, new_packages, &tag)
            .map_err(|e| crate::error::convert_vm_error(e, vm, linkage_view))?;
        Ok(ObjectValue {
            type_,
            has_public_transfer,
            used_in_non_entry_move_call,
            contents,
        })
    }

    pub(crate) fn value_from_object(
        vm: &MoveVM,
        linkage_view: &mut LinkageView,
        new_packages: &LinkedHashMap<ObjectID, MovePackage>,
        object: &Object,
    ) -> Result<ObjectValue, ExecutionError> {
        let Object { data: Data::Move(object), .. } = object else {
            invariant_violation!("Expected a Move object");
        };

        let used_in_non_entry_move_call = false;
        make_object_value(
            vm,
            linkage_view,
            new_packages,
            object.type_().clone(),
            object.has_public_transfer(),
            used_in_non_entry_move_call,
            object.contents(),
        )
    }

    /// Load an input object from the state_view
    fn load_object(
        vm: &MoveVM,
        state_view: &dyn ExecutionState,
        linkage_view: &mut LinkageView,
        new_packages: &LinkedHashMap<ObjectID, MovePackage>,
        input_object_map: &mut BTreeMap<ObjectID, object_runtime::InputObject>,
        override_as_immutable: bool,
        id: ObjectID,
    ) -> Result<InputValue, ExecutionError> {
        let Some(obj) = state_view.read_object(&id) else {
            // protected by transaction input checker
            invariant_violation!("Object {} does not exist yet", id);
        };
        // override_as_immutable ==> Owner::Shared
        assert_invariant!(
            !override_as_immutable || matches!(obj.owner, Owner::Shared { .. }),
            "override_as_immutable should only be set for shared objects"
        );
        let is_mutable_input = match obj.owner {
            Owner::AddressOwner(_) => true,
            Owner::Shared { .. } => !override_as_immutable,
            Owner::Immutable => false,
            Owner::ObjectOwner(_) => {
                // protected by transaction input checker
                invariant_violation!("ObjectOwner objects cannot be input")
            }
        };
        let owner = obj.owner;
        let version = obj.version();
        let object_metadata = InputObjectMetadata {
            id,
            is_mutable_input,
            owner,
            version,
        };
        let obj_value = value_from_object(vm, linkage_view, new_packages, obj)?;
        let contained_uids = {
            let fully_annotated_layout = vm
                .get_runtime()
                .type_to_fully_annotated_layout(&obj_value.type_)
                .map_err(|e| convert_vm_error(e, vm, linkage_view))?;
            let mut bytes = vec![];
            obj_value.write_bcs_bytes(&mut bytes);
            match get_all_uids(&fully_annotated_layout, &bytes) {
                Err(e) => {
                    invariant_violation!("Unable to retrieve UIDs for object. Got error: {e}")
                }
                Ok(uids) => uids,
            }
        };
        let runtime_input = object_runtime::InputObject {
            contained_uids,
            owner,
            version,
        };
        let prev = input_object_map.insert(id, runtime_input);
        // protected by transaction input checker
        assert_invariant!(prev.is_none(), "Duplicate input object {}", id);
        Ok(InputValue::new_object(object_metadata, obj_value))
    }

    /// Load an a CallArg, either an object or a raw set of BCS bytes
    fn load_call_arg(
        vm: &MoveVM,
        state_view: &dyn ExecutionState,
        linkage_view: &mut LinkageView,
        new_packages: &LinkedHashMap<ObjectID, MovePackage>,
        input_object_map: &mut BTreeMap<ObjectID, object_runtime::InputObject>,
        call_arg: CallArg,
    ) -> Result<InputValue, ExecutionError> {
        Ok(match call_arg {
            CallArg::Pure(bytes) => InputValue::new_raw(RawValueType::Any, bytes),
            CallArg::Object(obj_arg) => load_object_arg(
                vm,
                state_view,
                linkage_view,
                new_packages,
                input_object_map,
                obj_arg,
            )?,
        })
    }

    /// Load an ObjectArg from state view, marking if it can be treated as mutable or not
    fn load_object_arg(
        vm: &MoveVM,
        state_view: &dyn ExecutionState,
        linkage_view: &mut LinkageView,
        new_packages: &LinkedHashMap<ObjectID, MovePackage>,
        input_object_map: &mut BTreeMap<ObjectID, object_runtime::InputObject>,
        obj_arg: ObjectArg,
    ) -> Result<InputValue, ExecutionError> {
        match obj_arg {
            ObjectArg::ImmOrOwnedObject((id, _, _)) => load_object(
                vm,
                state_view,
                linkage_view,
                new_packages,
                input_object_map,
                /* imm override */ false,
                id,
            ),
            ObjectArg::SharedObject { id, mutable, .. } => load_object(
                vm,
                state_view,
                linkage_view,
                new_packages,
                input_object_map,
                /* imm override */ !mutable,
                id,
            ),
        }
    }

    /// Generate an additional write for an ObjectValue
    fn add_additional_write(
        additional_writes: &mut BTreeMap<ObjectID, AdditionalWrite>,
        owner: Owner,
        object_value: ObjectValue,
    ) -> Result<(), ExecutionError> {
        let ObjectValue {
            type_,
            has_public_transfer,
            contents,
            ..
        } = object_value;
        let bytes = match contents {
            ObjectContents::Coin(coin) => coin.to_bcs_bytes(),
            ObjectContents::Raw(bytes) => bytes,
        };
        let object_id = MoveObject::id_opt(&bytes).map_err(|e| {
            ExecutionError::invariant_violation(format!("No id for Raw object bytes. {e}"))
        })?;
        let additional_write = AdditionalWrite {
            recipient: owner,
            type_,
            has_public_transfer,
            bytes,
        };
        additional_writes.insert(object_id, additional_write);
        Ok(())
    }

    /// The max budget was deducted from the gas coin at the beginning of the transaction,
    /// now we return exactly that amount. Gas will be charged by the execution engine
    fn refund_max_gas_budget(
        additional_writes: &mut BTreeMap<ObjectID, AdditionalWrite>,
        gas_charger: &mut GasCharger,
        gas_id: ObjectID,
    ) -> Result<(), ExecutionError> {
        let Some(AdditionalWrite { bytes,.. }) = additional_writes.get_mut(&gas_id) else {
            invariant_violation!("Gas object cannot be wrapped or destroyed")
        };
        let Ok(mut coin) = Coin::from_bcs_bytes(bytes) else {
            invariant_violation!("Gas object must be a coin")
        };
        let Some(new_balance) = coin
            .balance
            .value()
            .checked_add(gas_charger.gas_budget()) else {
                return Err(ExecutionError::new_with_source(
                    ExecutionErrorKind::CoinBalanceOverflow,
                    "Gas coin too large after returning the max gas budget",
                ));
            };
        coin.balance = Balance::new(new_balance);
        *bytes = coin.to_bcs_bytes();
        Ok(())
    }

    /// Generate an MoveObject given an updated/written object
    /// # Safety
    ///
    /// This function assumes proper generation of has_public_transfer, either from the abilities of
    /// the StructTag, or from the runtime correctly propagating from the inputs
    unsafe fn create_written_object(
        vm: &MoveVM,
        linkage_view: &LinkageView,
        protocol_config: &ProtocolConfig,
        input_object_metadata: &BTreeMap<ObjectID, InputObjectMetadata>,
        loaded_child_objects: &BTreeMap<ObjectID, SequenceNumber>,
        id: ObjectID,
        type_: Type,
        has_public_transfer: bool,
        contents: Vec<u8>,
        write_kind: WriteKind,
    ) -> Result<MoveObject, ExecutionError> {
        debug_assert_eq!(
            id,
            MoveObject::id_opt(&contents).expect("object contents should start with an id")
        );
        let metadata_opt = input_object_metadata.get(&id);
        let loaded_child_version_opt = loaded_child_objects.get(&id);
        assert_invariant!(
            metadata_opt.is_none() || loaded_child_version_opt.is_none(),
            "Loaded {id} as a child, but that object was an input object",
        );

        let old_obj_ver = metadata_opt
            .map(|metadata| metadata.version)
            .or_else(|| loaded_child_version_opt.copied());

        let type_tag = vm
            .get_runtime()
            .get_type_tag(&type_)
            .map_err(|e| crate::error::convert_vm_error(e, vm, linkage_view))?;

        debug_assert!(
            (write_kind == WriteKind::Mutate) == old_obj_ver.is_some(),
            "Inconsistent state for [{type_tag:?}]({id:?}): write_kind: {write_kind:?}, old ver: {old_obj_ver:?}"
        );

        let struct_tag = match type_tag {
            TypeTag::Struct(inner) => *inner,
            _ => invariant_violation!("Non struct type for object"),
        };
        MoveObject::new_from_execution(
            struct_tag.into(),
            has_public_transfer,
            old_obj_ver.unwrap_or_else(SequenceNumber::new),
            contents,
            protocol_config,
        )
    }

    // Implementation of the `DataStore` trait for the Move VM.
    // When used during execution it may have a list of new packages that have
    // just been published in the current context. Thos are used for module/type
    // resolution when executing module init.
    // It may be created with an empty slice of packages either when no publish/upgrade
    // are performed or when a type is requested not during execution.
    pub(crate) struct SuiDataStore<'state, 'a> {
        linkage_view: &'a LinkageView<'state>,
        new_packages: &'a LinkedHashMap<ObjectID, MovePackage>,
    }

    impl<'state, 'a> SuiDataStore<'state, 'a> {
        pub(crate) fn new(
            linkage_view: &'a LinkageView<'state>,
            new_packages: &'a LinkedHashMap<ObjectID, MovePackage>,
        ) -> Self {
            Self {
                linkage_view,
                new_packages,
            }
        }

        fn get_module(&self, module_id: &ModuleId) -> Option<&Vec<u8>> {
            self.new_packages
                .get(&ObjectID::from(*module_id.address()))
                .and_then(|package| package.get_module(module_id))
        }
    }

    // TODO: `DataStore` wil be reworked and this is likely to disappear.
    //       Leaving this comment around until then as testament to better days to come...
    impl<'state, 'a> DataStore for SuiDataStore<'state, 'a> {
        fn link_context(&self) -> AccountAddress {
            self.linkage_view.link_context()
        }

        fn relocate(&self, module_id: &ModuleId) -> PartialVMResult<ModuleId> {
            self.linkage_view.relocate(module_id).map_err(|err| {
                PartialVMError::new(StatusCode::LINKER_ERROR)
                    .with_message(format!("Error relocating {module_id}: {err:?}"))
            })
        }

        fn defining_module(
            &self,
            runtime_id: &ModuleId,
            struct_: &IdentStr,
        ) -> PartialVMResult<ModuleId> {
            self.linkage_view
                .defining_module(runtime_id, struct_)
                .map_err(|err| {
                    PartialVMError::new(StatusCode::LINKER_ERROR).with_message(format!(
                        "Error finding defining module for {runtime_id}::{struct_}: {err:?}"
                    ))
                })
        }

        fn load_module(&self, module_id: &ModuleId) -> VMResult<Vec<u8>> {
            if let Some(bytes) = self.get_module(module_id) {
                return Ok(bytes.clone());
            }
            match self.linkage_view.get_module(module_id) {
                Ok(Some(bytes)) => Ok(bytes),
                Ok(None) => Err(PartialVMError::new(StatusCode::LINKER_ERROR)
                    .with_message(format!("Cannot find {:?} in data cache", module_id))
                    .finish(Location::Undefined)),
                Err(err) => {
                    let msg = format!("Unexpected storage error: {:?}", err);
                    Err(
                        PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR)
                            .with_message(msg)
                            .finish(Location::Undefined),
                    )
                }
            }
        }

        //
        // TODO: later we will clean up the interface with the runtime and the functions below
        //       will likely be exposed via extensions
        //

        fn load_resource(
            &mut self,
            _addr: AccountAddress,
            _ty: &Type,
        ) -> PartialVMResult<(&mut GlobalValue, Option<Option<NumBytes>>)> {
            panic!("load_resource should never be called for LinkageView")
        }

        fn emit_event(
            &mut self,
            _guid: Vec<u8>,
            _seq_num: u64,
            _ty: Type,
            _val: VMValue,
        ) -> PartialVMResult<()> {
            panic!("emit_event should never be called for LinkageView")
        }

        fn events(&self) -> &Vec<(Vec<u8>, u64, Type, MoveTypeLayout, VMValue)> {
            panic!("events should never be called for LinkageView")
        }

        fn publish_module(&mut self, _module_id: &ModuleId, _blob: Vec<u8>) -> VMResult<()> {
            // we cannot panic here because during executon and publishing this is
            // currently called from the publish flow in the Move runtime
            Ok(())
        }
    }
}
