// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::build;
use clap::Parser;
use move_cli::base::{
    self,
    test::{self, UnitTestResult},
};
use move_package::BuildConfig;
use move_unit_test::{extensions::set_extension_hook, UnitTestingConfig};
use move_vm_runtime::native_extensions::NativeContextExtensions;
use once_cell::sync::Lazy;
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
use sui_move_natives::{object_runtime::ObjectRuntime, NativesCostTable};
use sui_protocol_config::ProtocolConfig;
use sui_types::{
    base_types::{ObjectID, SequenceNumber},
    error::SuiResult,
    gas_model::tables::initial_cost_schedule_for_unit_tests,
    metrics::LimitsMetrics,
    object::Object,
    storage::ChildObjectResolver,
};

// Move unit tests will halt after executing this many steps. This is a protection to avoid divergence
const MAX_UNIT_TEST_INSTRUCTIONS: u64 = 1_000_000;

#[derive(Parser)]
pub struct Test {
    #[clap(flatten)]
    pub test: test::Test,
    /// If `true`, enable linters
    #[clap(long, global = true)]
    pub lint: bool,
}

impl Test {
    pub fn execute(
        &self,
        path: Option<PathBuf>,
        build_config: BuildConfig,
        unit_test_config: UnitTestingConfig,
    ) -> anyhow::Result<UnitTestResult> {
        // find manifest file directory from a given path or (if missing) from current dir
        let rerooted_path = base::reroot_path(path)?;
        // pre build for Sui-specific verifications
        let with_unpublished_deps = false;
        let dump_bytecode_as_base64 = false;
        let generate_struct_layouts: bool = false;
        build::Build::execute_internal(
            rerooted_path.clone(),
            BuildConfig {
                test_mode: true, // make sure to verify tests
                ..build_config.clone()
            },
            with_unpublished_deps,
            dump_bytecode_as_base64,
            generate_struct_layouts,
            self.lint,
        )?;
        run_move_unit_tests(
            rerooted_path,
            build_config,
            Some(unit_test_config),
            self.test.compute_coverage,
        )
    }
}

struct DummyChildObjectStore {}

impl ChildObjectResolver for DummyChildObjectStore {
    fn read_child_object(
        &self,
        _parent: &ObjectID,
        _child: &ObjectID,
        _child_version_upper_bound: SequenceNumber,
    ) -> SuiResult<Option<Object>> {
        Ok(None)
    }
}

static TEST_STORE: Lazy<DummyChildObjectStore> = Lazy::new(|| DummyChildObjectStore {});

static SET_EXTENSION_HOOK: Lazy<()> =
    Lazy::new(|| set_extension_hook(Box::new(new_testing_object_and_natives_cost_runtime)));

/// This function returns a result of UnitTestResult. The outer result indicates whether it
/// successfully started running the test, and the inner result indicatests whether all tests pass.
pub fn run_move_unit_tests(
    path: PathBuf,
    build_config: BuildConfig,
    config: Option<UnitTestingConfig>,
    compute_coverage: bool,
) -> anyhow::Result<UnitTestResult> {
    // bind the extension hook if it has not yet been done
    Lazy::force(&SET_EXTENSION_HOOK);

    let config = config
        .unwrap_or_else(|| UnitTestingConfig::default_with_bound(Some(MAX_UNIT_TEST_INSTRUCTIONS)));

    move_cli::base::test::run_move_unit_tests(
        &path,
        build_config,
        UnitTestingConfig {
            report_stacktrace_on_abort: true,
            ignore_compile_warnings: true,
            ..config
        },
        sui_move_natives::all_natives(/* silent */ false),
        Some(initial_cost_schedule_for_unit_tests()),
        compute_coverage,
        &mut std::io::sink(),
        &mut std::io::stdout(),
    )
}

fn new_testing_object_and_natives_cost_runtime(ext: &mut NativeContextExtensions) {
    // Use a throwaway metrics registry for testing.
    let registry = prometheus::Registry::new();
    let metrics = Arc::new(LimitsMetrics::new(&registry));
    let store = Lazy::force(&TEST_STORE);

    ext.add(ObjectRuntime::new(
        store,
        BTreeMap::new(),
        false,
        &ProtocolConfig::get_for_min_version(),
        metrics,
    ));
    ext.add(NativesCostTable::from_protocol_config(
        &ProtocolConfig::get_for_min_version(),
    ));
}
