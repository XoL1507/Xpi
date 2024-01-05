// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json as json;

/// Logical Groups categorise APIs exposed by GraphQL.  Groups can be enabled or disabled based on
/// settings in the RPC's TOML configuration file.
#[derive(Copy, Clone, Serialize, Deserialize, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum FunctionalGroup {
    /// Statistics about how the network was running (TPS, top packages, APY, etc)
    Analytics,

    /// Coin metadata, per-address coin and balance information.
    Coins,

    /// Querying an object's dynamic fields.
    DynamicFields,

    /// SuiNS name and reverse name look-up.
    NameService,

    /// Transaction and Event subscriptions.
    Subscriptions,

    /// Information about the system that changes from epoch to epoch (protocol config, committee,
    /// reference gas price).
    SystemState,
}

impl FunctionalGroup {
    /// Name that the group is referred to by in configuration and responses on the GraphQL API.
    /// Not a suitable `Display` implementation because it enquotes the representation.
    pub(crate) fn name(&self) -> String {
        json::ser::to_string(self).expect("Serializing `FunctionalGroup` cannot fail.")
    }
}

/// Mapping from type and field name in the schema to the functional group it belongs to.
fn functional_groups() -> &'static BTreeMap<(&'static str, &'static str), FunctionalGroup> {
    // TODO: Introduce a macro to declare the functional group for a field and/or type on the
    // appropriate type, field, or function, instead of here.  This may also be able to set the
    // graphql `visible` attribute to control schema visibility by functional groups.

    use FunctionalGroup as G;
    static GROUPS: Lazy<BTreeMap<(&str, &str), FunctionalGroup>> = Lazy::new(|| {
        BTreeMap::from_iter([
            (("Address", "balance"), G::Coins),
            (("Address", "balanceConnection"), G::Coins),
            (("Address", "coinConnection"), G::Coins),
            (("Address", "defaultNameServiceName"), G::NameService),
            (("Address", "nameServiceConnection"), G::NameService),
            (("Checkpoint", "addressMetrics"), G::Analytics),
            (("Checkpoint", "networkTotalTransactions"), G::Analytics),
            (("Epoch", "protocolConfig"), G::SystemState),
            (("Epoch", "referenceGasPrice"), G::SystemState),
            (("Epoch", "safeMode"), G::SystemState),
            (("Epoch", "storageFund"), G::SystemState),
            (("Epoch", "systemParameters"), G::SystemState),
            (("Epoch", "systemStateVersion"), G::SystemState),
            (("Epoch", "validatorSet"), G::SystemState),
            (("Object", "balance"), G::Coins),
            (("Object", "balanceConnection"), G::Coins),
            (("Object", "coinConnection"), G::Coins),
            (("Object", "defaultNameServiceName"), G::NameService),
            (("Object", "dynamicField"), G::DynamicFields),
            (("Object", "dynamicFieldConnection"), G::DynamicFields),
            (("Object", "nameServiceConnection"), G::NameService),
            (("Owner", "balance"), G::Coins),
            (("Owner", "balanceConnection"), G::Coins),
            (("Owner", "coinConnection"), G::Coins),
            (("Owner", "defaultNameServiceName"), G::NameService),
            (("Owner", "nameServiceConnection"), G::NameService),
            (("Query", "coinMetadata"), G::Coins),
            (("Query", "moveCallMetrics"), G::Analytics),
            (("Query", "networkMetrics"), G::Analytics),
            (("Query", "protocolConfig"), G::SystemState),
            (("Query", "resolveNameServiceAddress"), G::NameService),
            (("Subscription", "events"), G::Subscriptions),
            (("Subscription", "transactions"), G::Subscriptions),
        ])
    });

    Lazy::force(&GROUPS)
}

/// Map a type and field name to a functional group.  If an explicit group does not exist for the
/// field, then it is assumed to be a "core" feature.
pub(crate) fn functional_group(type_: &str, field: &str) -> Option<FunctionalGroup> {
    functional_groups().get(&(type_, field)).copied()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use async_graphql::registry::Registry;
    use async_graphql::OutputType;

    use crate::types::query::Query;

    use super::*;

    #[test]
    /// Makes sure all the functional groups correspond to real elements of the schema unless they
    /// are explicitly recorded as unimplemented.  Complementarily, makes sure that fields marked as
    /// unimplemented don't appear in the set of unimplemented fields.
    fn test_groups_match_schema() {
        let mut registry = Registry::default();
        Query::create_type_info(&mut registry);

        let unimplemented = BTreeSet::from_iter([
            ("Checkpoint", "addressMetrics"),
            ("Epoch", "protocolConfig"),
            ("Object", "dynamicField"),
            ("Object", "dynamicFieldConnection"),
            ("Query", "coinMetadata"),
            ("Query", "moveCallMetrics"),
            ("Query", "networkMetrics"),
            ("Query", "resolveNameServiceAddress"),
            ("Subscription", "events"),
            ("Subscription", "transactions"),
        ]);

        for (type_, field) in &unimplemented {
            let Some(meta_type) = registry.concrete_type_by_name(type_) else {
                continue;
            };

            let Some(_) = meta_type.field_by_name(field) else {
                continue;
            };

            panic!(
                "Field '{type_}.{field}' is marked as unimplemented in this test, but it's in the \
                 schema.  Fix this by removing it from the `unimplemented` set."
            );
        }

        for (type_, field) in functional_groups().keys() {
            if unimplemented.contains(&(type_, field)) {
                continue;
            }

            let Some(meta_type) = registry.concrete_type_by_name(type_) else {
                panic!("Type '{type_}' from functional group configs does not appear in schema.");
            };

            let Some(_) = meta_type.field_by_name(field) else {
                panic!(
                    "Field '{type_}.{field}' from functional group configs does not appear in \
                     schema."
                );
            };
        }
    }
}
