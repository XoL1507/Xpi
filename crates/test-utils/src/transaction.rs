// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
use crate::authority::get_client;
use crate::messages::{
    create_publish_move_package_transaction, get_account_and_gas_coins,
    get_gas_object_with_wallet_context, make_tx_certs_and_signed_effects, MAX_GAS,
};
use crate::test_account_keys;
use futures::StreamExt;
use move_package::BuildConfig;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use sui::client_commands::WalletContext;
use sui::client_commands::{SuiClientCommandResult, SuiClientCommands};
use sui_config::ValidatorInfo;
use sui_core::authority::AuthorityState;
use sui_core::authority_client::AuthorityAPI;
use sui_json_rpc_types::SuiObjectRead;
use sui_json_rpc_types::{SuiParsedTransactionResponse, SuiTransactionResponse};
use sui_sdk::json::SuiJsonValue;
use sui_types::base_types::ObjectRef;
use sui_types::base_types::{ObjectID, SuiAddress, TransactionDigest};
use sui_types::batch::UpdateItem;
use sui_types::error::SuiResult;
use sui_types::messages::{
    BatchInfoRequest, BatchInfoResponseItem, CallArg, ObjectArg, ObjectInfoRequest,
    ObjectInfoResponse, Transaction, TransactionData, TransactionEffects, TransactionInfoResponse,
};
use sui_types::object::{Object, Owner};
use sui_types::SUI_FRAMEWORK_OBJECT_ID;
use tokio::time::{sleep, Duration};
use tracing::debug;
use tracing::info;

pub fn make_publish_package(gas_object: Object, path: PathBuf) -> Transaction {
    let (sender, keypair) = test_account_keys().pop().unwrap();
    create_publish_move_package_transaction(
        gas_object.compute_object_reference(),
        path,
        sender,
        &keypair,
    )
}

pub async fn publish_package(
    gas_object: Object,
    path: PathBuf,
    configs: &[ValidatorInfo],
) -> ObjectRef {
    let effects = publish_package_for_effects(gas_object, path, configs).await;
    parse_package_ref(&effects).unwrap()
}

pub async fn publish_package_for_effects(
    gas_object: Object,
    path: PathBuf,
    configs: &[ValidatorInfo],
) -> TransactionEffects {
    submit_single_owner_transaction(make_publish_package(gas_object, path), configs).await
}

/// Helper function to publish the move package of a simple shared counter.
pub async fn publish_counter_package(gas_object: Object, configs: &[ValidatorInfo]) -> ObjectRef {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("../../sui_programmability/examples/basics");
    publish_package(gas_object, path, configs).await
}

/// A helper function to publish basic package using gateway API
pub async fn publish_basics_package(context: &WalletContext, sender: SuiAddress) -> ObjectRef {
    let transaction = {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("../../sui_programmability/examples/basics");

        let build_config = BuildConfig::default();
        let modules = sui_framework::build_move_package(&path, build_config).unwrap();

        let all_module_bytes = modules
            .iter()
            .map(|m| {
                let mut module_bytes = Vec::new();
                m.serialize(&mut module_bytes).unwrap();
                module_bytes
            })
            .collect();

        let data = context
            .client
            .transaction_builder()
            .publish(sender, all_module_bytes, None, 50000)
            .await
            .unwrap();

        let signature = context.keystore.sign(&sender, &data.to_bytes()).unwrap();
        Transaction::new(data, signature)
    };

    let resp = context
        .client
        .quorum_driver()
        .execute_transaction(transaction)
        .await
        .unwrap();

    if let Some(SuiParsedTransactionResponse::Publish(resp)) = resp.parsed_data {
        resp.package.to_object_ref()
    } else {
        panic!()
    }
}

/// A helper function to submit a move transaction using gateway API
pub async fn submit_move_transaction(
    context: &WalletContext,
    module: &'static str,
    function: &'static str,
    package_ref: ObjectRef,
    arguments: Vec<SuiJsonValue>,
    sender: SuiAddress,
    gas_object: Option<ObjectID>,
) -> SuiTransactionResponse {
    debug!(?package_ref, ?arguments, "move_transaction");

    let data = context
        .client
        .transaction_builder()
        .move_call(
            sender,
            package_ref.0,
            module,
            function,
            vec![], // type_args
            arguments,
            gas_object,
            50000,
        )
        .await
        .unwrap();

    let signature = context.keystore.sign(&sender, &data.to_bytes()).unwrap();
    let tx = Transaction::new(data, signature);
    let tx_digest = tx.digest();
    debug!(?tx_digest, "submitting move transaction");

    context
        .client
        .quorum_driver()
        .execute_transaction(tx)
        .await
        .unwrap()
}

/// A helper function to publish the basics package and make counter objects
pub async fn publish_basics_package_and_make_counter(
    context: &WalletContext,
    sender: SuiAddress,
) -> (ObjectRef, ObjectID) {
    let package_ref = publish_basics_package(context, sender).await;

    debug!(?package_ref);

    let create_shared_obj_resp = submit_move_transaction(
        context,
        "counter",
        "create",
        package_ref,
        vec![],
        sender,
        None,
    )
    .await;

    let counter_id = create_shared_obj_resp.effects.created[0]
        .clone()
        .reference
        .object_id;
    debug!(?counter_id);
    (package_ref, counter_id)
}

pub async fn increment_counter(
    context: &WalletContext,
    sender: SuiAddress,
    gas_object: Option<ObjectID>,
    package_ref: ObjectRef,
    counter_id: ObjectID,
) -> SuiTransactionResponse {
    submit_move_transaction(
        context,
        "counter",
        "increment",
        package_ref,
        vec![SuiJsonValue::new(json!(counter_id.to_hex_literal())).unwrap()],
        sender,
        gas_object,
    )
    .await
}

pub async fn create_devnet_nft(
    context: &mut WalletContext,
) -> Result<(SuiAddress, ObjectID, TransactionDigest), anyhow::Error> {
    let (sender, gas_objects) = get_account_and_gas_coins(context).await?.swap_remove(0);
    let gas_object = gas_objects.get(0).unwrap().id();

    let res = SuiClientCommands::CreateExampleNFT {
        name: Some("example_nft_name".into()),
        description: Some("example_nft_desc".into()),
        url: Some("https://sui.io/_nuxt/img/sui-logo.8d3c44e.svg".into()),
        gas: Some(*gas_object),
        gas_budget: Some(50000),
    }
    .execute(context)
    .await?;

    let (object_id, digest) = if let SuiClientCommandResult::CreateExampleNFT(
        SuiObjectRead::Exists(obj),
    ) = res
    {
        (obj.reference.object_id, obj.previous_transaction)
    } else {
        panic!("CreateExampleNFT command did not return WalletCommandResult::CreateExampleNFT(SuiObjectRead::Exists, got {:?}", res);
    };

    Ok((sender, object_id, digest))
}

pub async fn transfer_sui(
    context: &mut WalletContext,
    sender: Option<SuiAddress>,
    receiver: Option<SuiAddress>,
) -> Result<(ObjectID, SuiAddress, SuiAddress, TransactionDigest), anyhow::Error> {
    let sender = match sender {
        None => context.keystore.addresses().get(0).cloned().unwrap(),
        Some(addr) => addr,
    };
    let receiver = match receiver {
        None => context.keystore.addresses().get(1).cloned().unwrap(),
        Some(addr) => addr,
    };
    let gas_ref = get_gas_object_with_wallet_context(context, &sender)
        .await
        .unwrap();

    let res = SuiClientCommands::TransferSui {
        to: receiver,
        amount: None,
        sui_coin_object_id: gas_ref.0,
        gas_budget: 50000,
    }
    .execute(context)
    .await?;

    let digest = if let SuiClientCommandResult::TransferSui(tx_cert, _effects) = res {
        tx_cert.transaction_digest
    } else {
        panic!("transfer command did not return WalletCommandResult::TransferSui");
    };

    Ok((gas_ref.0, sender, receiver, digest))
}

pub async fn transfer_coin(
    context: &mut WalletContext,
) -> Result<(ObjectID, SuiAddress, SuiAddress, TransactionDigest), anyhow::Error> {
    let sender = context.keystore.addresses().get(0).cloned().unwrap();
    let receiver = context.keystore.addresses().get(1).cloned().unwrap();

    let object_refs = context
        .client
        .read_api()
        .get_objects_owned_by_address(sender)
        .await?;
    let object_to_send = object_refs.get(1).unwrap().object_id;

    // Send an object
    info!(
        "transferring coin {:?} from {:?} -> {:?}",
        object_to_send, sender, receiver
    );
    let res = SuiClientCommands::Transfer {
        to: receiver,
        object_id: object_to_send,
        gas: None,
        gas_budget: 50000,
    }
    .execute(context)
    .await?;

    let digest = if let SuiClientCommandResult::Transfer(_, cert, _) = res {
        cert.transaction_digest
    } else {
        panic!("transfer command did not return WalletCommandResult::Transfer");
    };

    Ok((object_to_send, sender, receiver, digest))
}

pub async fn split_coin_with_wallet_context(context: &mut WalletContext, coin_id: ObjectID) {
    SuiClientCommands::SplitCoin {
        coin_id,
        amounts: None,
        count: 2,
        gas: None,
        gas_budget: MAX_GAS,
    }
    .execute(context)
    .await
    .unwrap();
}

pub async fn delete_devnet_nft(
    context: &mut WalletContext,
    sender: &SuiAddress,
    nft_to_delete: ObjectRef,
    package_ref: ObjectRef,
) -> SuiTransactionResponse {
    let gas = get_gas_object_with_wallet_context(context, sender)
        .await
        .unwrap_or_else(|| panic!("Expect {sender} to have at least one gas object"));
    let data = TransactionData::new_move_call(
        *sender,
        package_ref,
        "devnet_nft".parse().unwrap(),
        "burn".parse().unwrap(),
        Vec::new(),
        gas,
        vec![CallArg::Object(ObjectArg::ImmOrOwnedObject(nft_to_delete))],
        MAX_GAS,
    );

    let signature = context.keystore.sign(sender, &data.to_bytes()).unwrap();
    let tx = Transaction::new(data, signature);

    context
        .client
        .quorum_driver()
        .execute_transaction(tx)
        .await
        .unwrap()
}

/// Submit a certificate containing only owned-objects to all authorities.
pub async fn submit_single_owner_transaction(
    transaction: Transaction,
    configs: &[ValidatorInfo],
) -> TransactionEffects {
    let certificate = make_tx_certs_and_signed_effects(vec![transaction])
        .0
        .pop()
        .unwrap();

    let mut responses = Vec::new();
    for config in configs {
        let client = get_client(config);
        let reply = client
            .handle_certificate(certificate.clone())
            .await
            .unwrap();
        responses.push(reply);
    }
    get_unique_effects(responses)
}

/// Keep submitting the certificates of a shared-object transaction until it is sequenced by
/// at least one consensus node. We use the loop since some consensus protocols (like Tusk)
/// may drop transactions. The certificate is submitted to every Sui authority.
pub async fn submit_shared_object_transaction(
    transaction: Transaction,
    configs: &[ValidatorInfo],
) -> SuiResult<TransactionEffects> {
    let certificate = make_tx_certs_and_signed_effects(vec![transaction])
        .0
        .pop()
        .unwrap();

    let replies = loop {
        let futures: Vec<_> = configs
            .iter()
            .map(|config| {
                let client = get_client(config);
                let cert = certificate.clone();
                async move { client.handle_certificate(cert).await }
            })
            .collect();

        let replies: Vec<_> = futures::future::join_all(futures)
            .await
            .into_iter()
            // Remove all `FailedToHearBackFromConsensus` replies. Note that the original Sui error type
            // `SuiError::FailedToHearBackFromConsensus(..)` is lost when the message is sent through the
            // network (it is replaced by `RpcError`). As a result, the following filter doesn't work:
            // `.filter(|result| !matches!(result, Err(SuiError::FailedToHearBackFromConsensus(..))))`.
            .filter(|result| match result {
                Err(e) => !e.to_string().contains("deadline has elapsed"),
                _ => true,
            })
            .collect();

        if !replies.is_empty() {
            break replies;
        }
    };
    let replies: SuiResult<Vec<_>> = replies.into_iter().collect();
    replies.map(get_unique_effects)
}

pub fn get_unique_effects(replies: Vec<TransactionInfoResponse>) -> TransactionEffects {
    let mut all_effects = HashMap::new();
    for reply in replies {
        let effects = reply.signed_effects.unwrap().effects;
        all_effects.insert(effects.digest(), effects);
    }
    assert_eq!(all_effects.len(), 1);
    all_effects.into_values().next().unwrap()
}

/// Extract the package reference from a transaction effect. This is useful to deduce the
/// authority-created package reference after attempting to publish a new Move package.
pub fn parse_package_ref(effects: &TransactionEffects) -> Option<ObjectRef> {
    effects
        .created
        .iter()
        .find(|(_, owner)| matches!(owner, Owner::Immutable))
        .map(|(reference, _)| *reference)
}

/// Get the framework object
pub async fn get_framework_object(configs: &[ValidatorInfo]) -> Object {
    let mut responses = Vec::new();
    for config in configs {
        let client = get_client(config);
        let reply = client
            .handle_object_info_request(ObjectInfoRequest::latest_object_info_request(
                SUI_FRAMEWORK_OBJECT_ID,
                None,
            ))
            .await
            .unwrap();
        responses.push(reply);
    }
    extract_obj(responses)
}

pub fn extract_obj(replies: Vec<ObjectInfoResponse>) -> Object {
    let mut all_objects = HashSet::new();
    for reply in replies {
        all_objects.insert(reply.object_and_lock.unwrap().object);
    }
    assert_eq!(all_objects.len(), 1);
    all_objects.into_iter().next().unwrap()
}

pub async fn wait_for_tx(wait_digest: TransactionDigest, state: Arc<AuthorityState>) {
    wait_for_all_txes(vec![wait_digest], state).await
}

pub async fn wait_for_all_txes(wait_digests: Vec<TransactionDigest>, state: Arc<AuthorityState>) {
    let mut wait_digests: HashSet<_> = wait_digests.iter().collect();

    let mut timeout = Box::pin(sleep(Duration::from_millis(15_000)));

    let mut max_seq = Some(0);

    let mut stream = Box::pin(
        state
            .handle_batch_streaming(BatchInfoRequest {
                start: max_seq,
                length: 1000,
            })
            .await
            .unwrap(),
    );

    loop {
        tokio::select! {
            _ = &mut timeout => panic!("wait_for_tx timed out"),

            items = &mut stream.next() => {
                match items {
                    // Upon receiving a batch
                    Some(Ok(BatchInfoResponseItem(UpdateItem::Batch(batch)) )) => {
                        max_seq = Some(batch.data().next_sequence_number);
                        info!(?max_seq, "Received Batch");
                    }
                    // Upon receiving a transaction digest we store it, if it is not processed already.
                    Some(Ok(BatchInfoResponseItem(UpdateItem::Transaction((_seq, digest))))) => {
                        info!(?digest, "Received Transaction");
                        if wait_digests.remove(&digest.transaction) {
                            info!(?digest, "Digest found");
                        }
                        if wait_digests.is_empty() {
                            info!(?digest, "all digests found");
                            break;
                        }
                    },

                    Some(Err( err )) => panic!("{}", err),
                    None => {
                        info!(?max_seq, "Restarting Batch");
                        stream = Box::pin(
                                state
                                    .handle_batch_streaming(BatchInfoRequest {
                                        start: max_seq,
                                        length: 1000,
                                    })
                                    .await
                                    .unwrap(),
                            );

                    }
                }
            },
        }
    }

    // A small delay is needed so that the batch process can finish notifying other subscribers,
    // which tests may depend on. Otherwise tests can pass or fail depending on whether the
    // subscriber in this function was notified first or last.
    sleep(Duration::from_millis(10)).await;
}
