// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::signature::{AuthenticatorTrait, VerifyParams};
use crate::utils::make_zklogin_tx;
use crate::{
    base_types::SuiAddress, signature::GenericSignature, zk_login_util::DEFAULT_JWK_BYTES,
};
use fastcrypto::traits::ToFromBytes;
use fastcrypto_zkp::bn254::zk_login::{parse_jwks, JwkId, OIDCProvider, JWK};
use fastcrypto_zkp::bn254::zk_login_api::ZkLoginEnv;
use im::hashmap::HashMap as ImHashMap;
use shared_crypto::intent::{Intent, IntentMessage};

#[test]
fn zklogin_authenticator_jwk() {
    let (user_address, tx, authenticator) = make_zklogin_tx();

    let intent_msg = IntentMessage::new(
        Intent::sui_transaction(),
        tx.into_data().transaction_data().clone(),
    );

    let parsed: ImHashMap<JwkId, JWK> = parse_jwks(DEFAULT_JWK_BYTES, &OIDCProvider::Twitch)
        .unwrap()
        .into_iter()
        .collect();

    // Construct the required info to verify a zk login authenticator, jwks, supported providers list and env (prod/test).
    let aux_verify_data =
        VerifyParams::new(parsed.clone(), vec![OIDCProvider::Twitch], ZkLoginEnv::Test);

    let res =
        authenticator.verify_authenticator(&intent_msg, user_address, Some(0), &aux_verify_data);
    // Verify passes.
    assert!(res.is_ok());

    // Pass in supported list does not contain twitch fails to verify.
    let aux_verify_data =
        VerifyParams::new(parsed.clone(), vec![OIDCProvider::Google], ZkLoginEnv::Test);
    let res =
        authenticator.verify_authenticator(&intent_msg, user_address, Some(0), &aux_verify_data);
    assert!(res.is_err());

    // Epoch expired fails to verify.
    let aux_verify_data =
        VerifyParams::new(parsed.clone(), vec![OIDCProvider::Twitch], ZkLoginEnv::Test);
    assert!(authenticator
        .verify_authenticator(&intent_msg, user_address, Some(11), &aux_verify_data)
        .is_err());
    let parsed: ImHashMap<JwkId, JWK> = parsed
        .into_iter()
        .map(|(jwk_id, v)| {
            (
                JwkId::new(format!("nosuchkey_{}", jwk_id.iss), jwk_id.kid),
                v,
            )
        })
        .collect();

    // Correct kid can no longer be found fails to verify.
    let aux_verify_data = VerifyParams::new(parsed, vec![OIDCProvider::Twitch], ZkLoginEnv::Test);
    assert!(authenticator
        .verify_authenticator(&intent_msg, user_address, Some(0), &aux_verify_data)
        .is_err());
}

#[test]
fn test_serde_zk_login_signature() {
    let (user_address, _tx, authenticator) = make_zklogin_tx();
    let serialized = authenticator.as_ref();
    let deserialized = GenericSignature::from_bytes(serialized).unwrap();
    assert_eq!(deserialized, authenticator);

    let addr: SuiAddress = (&authenticator).try_into().unwrap();
    assert_eq!(addr, user_address);
}
