// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

diesel::table! {
    address_logs (last_processed_id) {
        last_processed_id -> Int8,
    }
}

diesel::table! {
    addresses (account_address) {
        account_address -> Varchar,
        first_appearance_tx -> Varchar,
        first_appearance_time -> Nullable<Timestamp>,
    }
}

diesel::table! {
    error_logs (id) {
        id -> Int8,
        error_type -> Varchar,
        error -> Text,
        error_time -> Timestamp,
    }
}

diesel::table! {
    event_logs (id) {
        id -> Int4,
        next_cursor_tx_seq -> Nullable<Int8>,
        next_cursor_event_seq -> Nullable<Int8>,
    }
}

diesel::table! {
    events (id) {
        id -> Int8,
        transaction_digest -> Nullable<Varchar>,
        transaction_sequence -> Int8,
        event_sequence -> Int8,
        event_time -> Nullable<Timestamp>,
        event_type -> Varchar,
        event_content -> Varchar,
    }
}

diesel::table! {
    object_logs (last_processed_id) {
        last_processed_id -> Int8,
    }
}

diesel::table! {
    objects (object_id) {
        object_id -> Varchar,
        version -> Int8,
        owner_type -> Varchar,
        owner_address -> Nullable<Varchar>,
        initial_shared_version -> Nullable<Int8>,
        package_id -> Text,
        transaction_module -> Text,
        object_type -> Nullable<Text>,
        object_status -> Varchar,
    }
}

diesel::table! {
    package_logs (last_processed_id) {
        last_processed_id -> Int8,
    }
}

diesel::table! {
    packages (package_id) {
        package_id -> Text,
        author -> Text,
        module_names -> Array<Nullable<Text>>,
        package_content -> Text,
    }
}

diesel::table! {
    transaction_logs (id) {
        id -> Int4,
        next_cursor_tx_digest -> Nullable<Text>,
    }
}

diesel::table! {
    transactions (id) {
        id -> Int8,
        transaction_digest -> Varchar,
        sender -> Varchar,
        transaction_time -> Nullable<Timestamp>,
        transaction_kinds -> Array<Nullable<Text>>,
        created -> Array<Nullable<Text>>,
        mutated -> Array<Nullable<Text>>,
        deleted -> Array<Nullable<Text>>,
        unwrapped -> Array<Nullable<Text>>,
        wrapped -> Array<Nullable<Text>>,
        gas_object_id -> Varchar,
        gas_object_sequence -> Int8,
        gas_object_digest -> Varchar,
        gas_budget -> Int8,
        total_gas_cost -> Int8,
        computation_cost -> Int8,
        storage_cost -> Int8,
        storage_rebate -> Int8,
        transaction_content -> Text,
    }
}

diesel::allow_tables_to_appear_in_same_query!(
    address_logs,
    addresses,
    error_logs,
    event_logs,
    events,
    object_logs,
    objects,
    package_logs,
    packages,
    transaction_logs,
    transactions,
);
