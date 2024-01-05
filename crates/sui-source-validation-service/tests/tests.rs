// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use expect_test::expect;
use reqwest::Client;
use std::{collections::BTreeMap, path::PathBuf};

use move_core_types::account_address::AccountAddress;
use move_symbol_pool::Symbol;
use sui_source_validation_service::{
    initialize, serve, verify_packages, AppState, CloneCommand, Config, PackageSources,
    RepositorySource, SourceInfo, SourceResponse,
};
use test_cluster::TestClusterBuilder;

const TEST_FIXTURES_DIR: &str = "tests/fixture";

#[tokio::test]
async fn test_verify_packages() -> anyhow::Result<()> {
    let mut cluster = TestClusterBuilder::new().build().await;
    let context = &mut cluster.wallet;

    let config = Config {
        packages: vec![PackageSources::Repository(RepositorySource {
            repository: "https://github.com/mystenlabs/sui".into(),
            branch: "main".into(),
            paths: vec!["move-stdlib".into()],
        })],
    };

    let fixtures = tempfile::tempdir()?;
    fs_extra::dir::copy(
        PathBuf::from(TEST_FIXTURES_DIR).join("sui"),
        fixtures.path(),
        &fs_extra::dir::CopyOptions::default(),
    )?;
    let result = verify_packages(context, &config, fixtures.path()).await;
    let truncated_error_message = &result
        .unwrap_err()
        .to_string()
        .lines()
        .take(3)
        .map(|s| s.into())
        .collect::<Vec<String>>()
        .join("\n");
    let expected = expect![
        r#"
Multiple source verification errors found:

- Local dependency did not match its on-chain version at 0000000000000000000000000000000000000000000000000000000000000001::MoveStdlib::address"#
    ];
    expected.assert_eq(truncated_error_message);
    Ok(())
}

#[tokio::test]
async fn test_api_route() -> anyhow::Result<()> {
    let mut cluster = TestClusterBuilder::new().build().await;
    let context = &mut cluster.wallet;
    let config = Config { packages: vec![] };
    let tmp_dir = tempfile::tempdir()?;
    initialize(context, &config, tmp_dir.path()).await?;

    // set up sample lookup to serve
    let fixtures = tempfile::tempdir()?;
    fs_extra::dir::copy(
        PathBuf::from(TEST_FIXTURES_DIR).join("sui"),
        fixtures.path(),
        &fs_extra::dir::CopyOptions::default(),
    )?;

    let address = "0x2";
    let module = "address";
    let source_path = fixtures
        .into_path()
        .join("sui/move-stdlib/sources/address.move");

    let mut sources = BTreeMap::new();
    sources.insert(
        (
            AccountAddress::from_hex_literal(address).unwrap(),
            Symbol::from(module),
        ),
        SourceInfo {
            path: source_path,
            source: Some("module address {...}".to_owned()),
        },
    );
    tokio::spawn(serve(AppState { sources }).expect("Cannot start service."));

    // check that serve returns expected sample code
    let client = Client::new();
    let json = client
        .get(format!(
            "http://0.0.0.0:8000/api?address={address}&module={module}"
        ))
        .send()
        .await
        .expect("Request failed.")
        .json::<SourceResponse>()
        .await?;

    let expected = expect!["module address {...}"];
    expected.assert_eq(&json.source);
    Ok(())
}

#[test]
fn test_parse_package_config() -> anyhow::Result<()> {
    let config = r#"
    [[packages]]
    source = "Repository"
    [packages.values]
    repository = "https://github.com/mystenlabs/sui"
    branch = "main"
    paths = [
        "crates/sui-framework/packages/deepbook",
        "crates/sui-framework/packages/move-stdlib",
        "crates/sui-framework/packages/sui-framework",
        "crates/sui-framework/packages/sui-system",
    ]

    [[packages]]
    source = "Directory"
    [packages.values]
    paths = [ "home/user/some/package" ]
"#;

    let config: Config = toml::from_str(config).unwrap();
    let expect = expect![
        r#"Config {
    packages: [
        Repository(
            RepositorySource {
                repository: "https://github.com/mystenlabs/sui",
                branch: "main",
                paths: [
                    "crates/sui-framework/packages/deepbook",
                    "crates/sui-framework/packages/move-stdlib",
                    "crates/sui-framework/packages/sui-framework",
                    "crates/sui-framework/packages/sui-system",
                ],
            },
        ),
        Directory(
            DirectorySource {
                paths: [
                    "home/user/some/package",
                ],
            },
        ),
    ],
}"#
    ];
    expect.assert_eq(&format!("{:#?}", config));
    Ok(())
}

#[test]
fn test_clone_command() -> anyhow::Result<()> {
    let source = RepositorySource {
        repository: "https://github.com/user/repo".into(),
        branch: "main".into(),
        paths: vec!["a".into(), "b".into()],
    };

    let command = CloneCommand::new(&source, PathBuf::from("/tmp").as_path())?;
    let expect = expect![
        r#"CloneCommand {
    args: [
        [
            "clone",
            "--no-checkout",
            "--depth=1",
            "--filter=tree:0",
            "--branch=main",
            "https://github.com/user/repo",
            "/tmp/repo",
        ],
        [
            "-C",
            "/tmp/repo",
            "sparse-checkout",
            "set",
            "--no-cone",
            "a",
            "b",
        ],
        [
            "-C",
            "/tmp/repo",
            "checkout",
        ],
    ],
    repo_url: "https://github.com/user/repo",
}"#
    ];
    expect.assert_eq(&format!("{:#?}", command));
    Ok(())
}
