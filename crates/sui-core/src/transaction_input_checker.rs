// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

pub use checked::*;

mod checked {
    use crate::authority::authority_per_epoch_store::AuthorityPerEpochStore;
    use crate::authority::AuthorityStore;
    use crate::transaction_signing_filter;
    use std::collections::{BTreeMap, HashSet};
    use std::sync::Arc;
    use sui_config::transaction_deny_config::TransactionDenyConfig;
    use sui_protocol_config::ProtocolConfig;
    use sui_types::base_types::{ObjectID, ObjectRef};
    use sui_types::error::{UserInputError, UserInputResult};
    use sui_types::executable_transaction::VerifiedExecutableTransaction;
    use sui_types::metrics::BytecodeVerifierMetrics;
    use sui_types::transaction::{
        InputObjectKind, InputObjects, TransactionData, TransactionDataAPI, TransactionKind,
        VersionedProtocolMessage,
    };
    use sui_types::{
        base_types::{SequenceNumber, SuiAddress},
        error::SuiResult,
        fp_ensure,
        gas::SuiGasStatus,
        object::{Object, Owner},
    };
    use sui_types::{storage::ObjectStore, SUI_CLOCK_OBJECT_ID, SUI_CLOCK_OBJECT_SHARED_VERSION};
    use tracing::instrument;

    // Entry point for all checks related to gas.
    // Called on both signing and execution.
    // On success the gas part of the transaction (gas data and gas coins)
    // is verified and good to go
    async fn get_gas_status(
        objects: &[Object],
        gas: &[ObjectRef],
        epoch_store: &AuthorityPerEpochStore,
        transaction: &TransactionData,
    ) -> SuiResult<SuiGasStatus> {
        check_gas(
            objects,
            epoch_store,
            gas,
            transaction.gas_budget(),
            transaction.gas_price(),
            transaction.kind(),
        )
        .await
    }

    #[instrument(level = "trace", skip_all)]
    pub async fn check_transaction_input(
        store: &AuthorityStore,
        epoch_store: &AuthorityPerEpochStore,
        transaction: &TransactionData,
        transaction_deny_config: &TransactionDenyConfig,
        metrics: &Arc<BytecodeVerifierMetrics>,
    ) -> SuiResult<(SuiGasStatus, InputObjects)> {
        transaction.check_version_supported(epoch_store.protocol_config())?;
        transaction.validity_check(epoch_store.protocol_config())?;
        let input_objects = transaction.input_objects()?;
        transaction_signing_filter::check_transaction_for_signing(
            transaction,
            &input_objects,
            transaction_deny_config,
            store,
        )?;

        // Runs verifier, which could be expensive.
        check_non_system_packages_to_be_published(
            transaction,
            epoch_store.protocol_config(),
            metrics,
        )?;

        let objects = store.check_input_objects(&input_objects, epoch_store.protocol_config())?;
        let gas_status =
            get_gas_status(&objects, transaction.gas(), epoch_store, transaction).await?;
        let input_objects = check_objects(store, epoch_store, transaction, input_objects, objects)?;
        Ok((gas_status, input_objects))
    }

    pub async fn check_transaction_input_with_given_gas(
        store: &AuthorityStore,
        epoch_store: &AuthorityPerEpochStore,
        transaction: &TransactionData,
        gas_object: Object,
        metrics: &Arc<BytecodeVerifierMetrics>,
    ) -> SuiResult<(SuiGasStatus, InputObjects)> {
        transaction.check_version_supported(epoch_store.protocol_config())?;
        transaction.validity_check_no_gas_check(epoch_store.protocol_config())?;
        check_non_system_packages_to_be_published(
            transaction,
            epoch_store.protocol_config(),
            metrics,
        )?;
        let mut input_objects = transaction.input_objects()?;
        let mut objects =
            store.check_input_objects(&input_objects, epoch_store.protocol_config())?;

        let gas_object_ref = gas_object.compute_object_reference();
        input_objects.push(InputObjectKind::ImmOrOwnedMoveObject(gas_object_ref));
        objects.push(gas_object);

        let gas_status =
            get_gas_status(&objects, &[gas_object_ref], epoch_store, transaction).await?;
        let input_objects = check_objects(store, epoch_store, transaction, input_objects, objects)?;
        Ok((gas_status, input_objects))
    }

    /// WARNING! This should only be used for the dev-inspect transaction. This transaction type
    /// bypasses many of the normal object checks
    pub(crate) async fn check_dev_inspect_input(
        store: &AuthorityStore,
        config: &ProtocolConfig,
        kind: &TransactionKind,
        gas_object: Object,
    ) -> SuiResult<(ObjectRef, InputObjects)> {
        let gas_object_ref = gas_object.compute_object_reference();
        kind.validity_check(config)?;
        match kind {
            TransactionKind::ProgrammableTransaction(_) => (),
            TransactionKind::ChangeEpoch(_)
            | TransactionKind::Genesis(_)
            | TransactionKind::ConsensusCommitPrologue(_) => {
                return Err(UserInputError::Unsupported(format!(
                    "Transaction kind {} is not supported in dev-inspect",
                    kind
                ))
                .into())
            }
        }
        let mut input_objects = kind.input_objects()?;
        let mut objects = store.check_input_objects(&input_objects, config)?;
        let mut used_objects: HashSet<SuiAddress> = HashSet::new();
        for object in &objects {
            if !object.is_immutable() {
                fp_ensure!(
                    used_objects.insert(object.id().into()),
                    UserInputError::MutableObjectUsedMoreThanOnce {
                        object_id: object.id()
                    }
                    .into()
                );
            }
        }
        input_objects.push(InputObjectKind::ImmOrOwnedMoveObject(gas_object_ref));
        objects.push(gas_object);
        let input_objects = InputObjects::new(input_objects.into_iter().zip(objects).collect());
        Ok((gas_object_ref, input_objects))
    }

    pub async fn check_certificate_input(
        store: &AuthorityStore,
        epoch_store: &AuthorityPerEpochStore,
        cert: &VerifiedExecutableTransaction,
    ) -> SuiResult<(SuiGasStatus, InputObjects)> {
        let protocol_version = epoch_store.protocol_version();

        // This should not happen - validators should not have signed the txn in the first place.
        assert!(
            cert.data()
                .transaction_data()
                .check_version_supported(epoch_store.protocol_config())
                .is_ok(),
            "Certificate formed with unsupported message version {:?} for protocol version {:?}",
            cert.message_version(),
            protocol_version
        );

        let tx_data = &cert.data().intent_message().value;
        let input_object_kinds = tx_data.input_objects()?;
        let input_object_data = if tx_data.is_change_epoch_tx() {
            // When changing the epoch, we update a the system object, which is shared, without going
            // through sequencing, so we must bypass the sequence checks here.
            store.check_input_objects(&input_object_kinds, epoch_store.protocol_config())?
        } else {
            store.check_sequenced_input_objects(cert.digest(), &input_object_kinds, epoch_store)?
        };
        let gas_status =
            get_gas_status(&input_object_data, tx_data.gas(), epoch_store, tx_data).await?;
        let input_objects = check_objects(
            store,
            epoch_store,
            tx_data,
            input_object_kinds,
            input_object_data,
        )?;
        Ok((gas_status, input_objects))
    }

    /// Check transaction gas data/info and gas coins consistency.
    /// Return the gas status to be used for the lifecycle of the transaction.
    #[instrument(level = "trace", skip_all)]
    async fn check_gas(
        objects: &[Object],
        epoch_store: &AuthorityPerEpochStore,
        gas: &[ObjectRef],
        gas_budget: u64,
        gas_price: u64,
        tx_kind: &TransactionKind,
    ) -> SuiResult<SuiGasStatus> {
        if tx_kind.is_system_tx() {
            Ok(SuiGasStatus::new_unmetered())
        } else {
            let protocol_config = epoch_store.protocol_config();
            let reference_gas_price = epoch_store.reference_gas_price();
            let gas_status =
                SuiGasStatus::new(gas_budget, gas_price, reference_gas_price, protocol_config)?;

            // check balance and coins consistency
            // load all gas coins
            let objects: BTreeMap<_, _> = objects.iter().map(|o| (o.id(), o)).collect();
            let mut gas_objects = vec![];
            for obj_ref in gas {
                let obj = objects.get(&obj_ref.0);
                let obj = *obj.ok_or(UserInputError::ObjectNotFound {
                    object_id: obj_ref.0,
                    version: Some(obj_ref.1),
                })?;
                gas_objects.push(obj);
            }
            gas_status.check_gas_balance(&gas_objects, gas_budget)?;
            Ok(gas_status)
        }
    }

    /// Check all the objects used in the transaction against the database, and ensure
    /// that they are all the correct version and number.
    #[instrument(level = "trace", skip_all)]
    pub fn check_objects(
        store: &AuthorityStore,
        epoch_store: &AuthorityPerEpochStore,
        transaction: &TransactionData,
        input_objects: Vec<InputObjectKind>,
        objects: Vec<Object>,
    ) -> UserInputResult<InputObjects> {
        // We require that mutable objects cannot show up more than once.
        let mut used_objects: HashSet<SuiAddress> = HashSet::new();
        for object in objects.iter() {
            if !object.is_immutable() {
                fp_ensure!(
                    used_objects.insert(object.id().into()),
                    UserInputError::MutableObjectUsedMoreThanOnce {
                        object_id: object.id()
                    }
                );
            }
        }

        // Get all objects owned by this transaction's sender.
        let mut object_roots: HashSet<ObjectID> = input_objects
            .iter()
            .zip(objects.iter())
            .filter_map(|(obj_kind, obj)| match (obj_kind, obj.owner) {
                (InputObjectKind::ImmOrOwnedMoveObject(obj_ref), Owner::AddressOwner(a))
                    if a == transaction.sender() =>
                {
                    Some(obj_ref.0)
                }
                _ => None,
            })
            .collect();

        // Gather all objects and errors.
        let mut all_objects = Vec::with_capacity(input_objects.len());
        let mut num_loaded_objects = input_objects.len() as u64;

        for (object_kind, object) in input_objects.into_iter().zip(objects) {
            // For Gas Object, we check the object is owned by gas owner
            // TODO: this is a quadratic check and though limits are low we should do it differently
            let owner_address = if transaction
                .gas()
                .iter()
                .any(|obj_ref| *obj_ref.0 == *object.id())
            {
                transaction.gas_owner()
            } else {
                transaction.sender()
            };
            // Check if the object contents match the type of lock we need for
            // this object.
            let system_transaction = transaction.is_system_tx();
            check_one_object(
                store,
                epoch_store,
                &owner_address,
                object_kind,
                &object,
                system_transaction,
                &mut object_roots,
                &mut num_loaded_objects,
            )?;
            all_objects.push((object_kind, object));
        }

        if !transaction.is_genesis_tx() && all_objects.is_empty() {
            return Err(UserInputError::ObjectInputArityViolation);
        }

        Ok(InputObjects::new(all_objects))
    }

    /// Check one object against a reference
    fn check_one_object(
        store: &AuthorityStore,
        epoch_store: &AuthorityPerEpochStore,
        owner: &SuiAddress,
        object_kind: InputObjectKind,
        object: &Object,
        system_transaction: bool,
        object_roots: &mut HashSet<ObjectID>,
        num_loaded_objects: &mut u64,
    ) -> UserInputResult {
        match object_kind {
            InputObjectKind::MovePackage(package_id) => {
                fp_ensure!(
                    object.data.try_as_package().is_some(),
                    UserInputError::MoveObjectAsPackage {
                        object_id: package_id
                    }
                );
            }
            InputObjectKind::ImmOrOwnedMoveObject((object_id, sequence_number, object_digest)) => {
                fp_ensure!(
                    !object.is_package(),
                    UserInputError::MovePackageAsObject { object_id }
                );
                fp_ensure!(
                    sequence_number < SequenceNumber::MAX,
                    UserInputError::InvalidSequenceNumber
                );

                // This is an invariant - we just load the object with the given ID and version.
                assert_eq!(
                    object.version(),
                    sequence_number,
                    "The fetched object version {} does not match the requested version {}, object id: {}",
                    object.version(),
                    sequence_number,
                    object.id(),
                );

                // Check the digest matches - user could give a mismatched ObjectDigest
                let expected_digest = object.digest();
                fp_ensure!(
                    expected_digest == object_digest,
                    UserInputError::InvalidObjectDigest {
                        object_id,
                        expected_digest
                    }
                );

                match object.owner {
                    Owner::Immutable => {
                        // Nothing else to check for Immutable.
                    }
                    Owner::AddressOwner(actual_owner) => {
                        // Check the owner is correct.
                        fp_ensure!(
                        owner == &actual_owner,
                        UserInputError::IncorrectUserSignature {
                            error: format!("Object {:?} is owned by account address {:?}, but given owner/signer address is {:?}", object_id, actual_owner, owner),
                        }
                    );
                    }
                    Owner::ObjectOwner(owner) => {
                        return Err(UserInputError::InvalidChildObjectArgument {
                            child_id: object.id(),
                            parent_id: owner.into(),
                        });
                    }
                    Owner::Shared { .. } => {
                        // This object is a mutable shared object. However the transaction
                        // specifies it as an owned object. This is inconsistent.
                        return Err(UserInputError::NotSharedObjectError);
                    }
                };
            }
            InputObjectKind::SharedMoveObject {
                id: SUI_CLOCK_OBJECT_ID,
                initial_shared_version: SUI_CLOCK_OBJECT_SHARED_VERSION,
                mutable: true,
            } => {
                // Only system transactions can accept the Clock
                // object as a mutable parameter.
                if system_transaction {
                    return Ok(());
                } else {
                    return Err(UserInputError::ImmutableParameterExpectedError {
                        object_id: SUI_CLOCK_OBJECT_ID,
                    });
                }
            }
            InputObjectKind::SharedMoveObject {
                initial_shared_version: input_initial_shared_version,
                ..
            } => {
                fp_ensure!(
                    object.version() < SequenceNumber::MAX,
                    UserInputError::InvalidSequenceNumber
                );

                match object.owner {
                    Owner::AddressOwner(_) | Owner::ObjectOwner(_) | Owner::Immutable => {
                        // When someone locks an object as shared it must be shared already.
                        return Err(UserInputError::NotSharedObjectError);
                    }
                    Owner::Shared {
                        initial_shared_version: actual_initial_shared_version,
                    } => {
                        fp_ensure!(
                            input_initial_shared_version == actual_initial_shared_version,
                            UserInputError::SharedObjectStartingVersionMismatch
                        )
                    }
                }
            }
            InputObjectKind::Receiving((object_id, sequence_number, object_digest)) => {
                fp_ensure!(
                    !object.is_package(),
                    UserInputError::MovePackageAsObject { object_id }
                );
                fp_ensure!(
                    sequence_number < SequenceNumber::MAX,
                    UserInputError::InvalidSequenceNumber
                );

                // This is an invariant - we just load the object with the given ID and version.
                assert_eq!(
                    object.version(),
                    sequence_number,
                    "The fetched object version {} does not match the requested version {}, object id: {}",
                    object.version(),
                    sequence_number,
                    object.id(),
                );

                // Check the digest matches - user could give a mismatched ObjectDigest
                let expected_digest = object.digest();
                fp_ensure!(
                    expected_digest == object_digest,
                    UserInputError::InvalidObjectDigest {
                        object_id,
                        expected_digest
                    }
                );

                let object_owner_id: ObjectID = match object.owner {
                    Owner::Immutable | Owner::Shared { .. } | Owner::ObjectOwner(_) => {
                        return Err(UserInputError::InvalidReceivingObjectInput { object_id });
                    }
                    Owner::AddressOwner(actual_owner) => actual_owner.into(),
                };

                let mut seen = HashSet::new();
                check_has_valid_receiving_path(
                    store,
                    epoch_store,
                    object_owner_id,
                    object_roots,
                    &mut seen,
                    num_loaded_objects,
                )?;
                object_roots.extend(seen);
            }
        };
        Ok(())
    }

    #[instrument(level = "trace", skip_all)]
    fn check_has_valid_receiving_path(
        store: &AuthorityStore,
        epoch_store: &AuthorityPerEpochStore,
        owner: ObjectID,
        object_roots: &mut HashSet<ObjectID>,
        seen: &mut HashSet<ObjectID>,
        num_loaded_objects: &mut u64,
    ) -> UserInputResult<()> {
        let mut outer_owner = Some(owner);
        while let Some(inner_owner) = outer_owner {
            // Have traversed to a root -- no need to load the object (it may not exist -- could be
            // a root address).
            if object_roots.contains(&inner_owner) {
                return Ok(());
            }

            // Make sure we don't load more than max_input_objects number of objects.
            *num_loaded_objects += 1;
            if *num_loaded_objects > epoch_store.protocol_config().max_input_objects() {
                return Err(UserInputError::SizeLimitExceeded {
                    limit: "maximum input objects in a transaction".to_string(),
                    value: epoch_store
                        .protocol_config()
                        .max_input_objects()
                        .to_string(),
                });
            }

            // If we can't load it for some reason declare that it's not found and return.
            let object =
                store
                    .get_object(&inner_owner)
                    .map_err(|_| UserInputError::ObjectNotFound {
                        object_id: inner_owner,
                        version: None,
                    })?;

            match object.map(|o| o.owner) {
                // In the future we will look up in the wrapped table at this point.
                None => {
                    return Err(UserInputError::ObjectNotFound {
                        object_id: inner_owner,
                        version: None,
                    })
                }
                Some(Owner::AddressOwner(address)) if seen.contains(&address.into()) => {
                    return Err(UserInputError::CyclicObjectDependency {
                        object_id: inner_owner,
                    });
                }
                Some(Owner::AddressOwner(address) | Owner::ObjectOwner(address)) => {
                    seen.insert(address.into());
                    outer_owner = Some(address.into());
                }
                Some(Owner::Shared { .. }) => {
                    return Err(UserInputError::Unsupported(
                        "Receiving on shared objects is not supported yet".to_string(),
                    ))
                }
                Some(Owner::Immutable) => {
                    return Err(UserInputError::MovePackageAsObject {
                        object_id: inner_owner,
                    })
                }
            }
        }
        Ok(())
    }

    /// Check package verification timeout
    #[instrument(level = "trace", skip_all)]
    pub fn check_non_system_packages_to_be_published(
        transaction: &TransactionData,
        protocol_config: &ProtocolConfig,
        metrics: &Arc<BytecodeVerifierMetrics>,
    ) -> UserInputResult<()> {
        // Only meter non-system programmable transaction blocks
        if transaction.is_system_tx() {
            return Ok(());
        }

        let TransactionKind::ProgrammableTransaction(pt) = transaction.kind() else {
        return Ok(());
    };

        // We use a custom config with metering enabled
        let is_metered = true;
        // Use the same verifier and meter for all packages
        let mut verifier = sui_execution::verifier(protocol_config, is_metered, metrics);

        // Measure time for verifying all packages in the PTB
        let shared_meter_verifier_timer = metrics
            .verifier_runtime_per_ptb_success_latency
            .start_timer();

        let verifier_status = pt
            .non_system_packages_to_be_published()
            .try_for_each(|module_bytes| verifier.meter_module_bytes(protocol_config, module_bytes))
            .map_err(|e| UserInputError::PackageVerificationTimedout { err: e.to_string() });

        match verifier_status {
            Ok(_) => {
                // Success: stop and record the success timer
                shared_meter_verifier_timer.stop_and_record();
            }
            Err(err) => {
                // Failure: redirect the success timers output to the failure timer and
                // discard the success timer
                metrics
                    .verifier_runtime_per_ptb_timeout_latency
                    .observe(shared_meter_verifier_timer.stop_and_discard());
                return Err(err);
            }
        };

        Ok(())
    }
}
