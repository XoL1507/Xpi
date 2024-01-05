// Copyright (c) 2021, Facebook, Inc. and its affiliates
// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use roaring::RoaringBitmap;

use crate::crypto::get_key_pair;

use super::*;

fn random_object_ref() -> ObjectRef {
    (
        ObjectID::random(),
        SequenceNumber::new(),
        ObjectDigest::new([0; 32]),
    )
}

#[test]
fn test_signed_values() {
    let mut authorities = BTreeMap::new();
    // TODO: refactor this test to not reuse the same keys for user and authority signing
    let (a1, sec1) = get_key_pair();
    let (a2, sec2) = get_key_pair();
    let (_, sec3) = get_key_pair();

    authorities.insert(
        /* address */ *sec1.public_key_bytes(),
        /* voting right */ 1,
    );
    authorities.insert(
        /* address */ *sec2.public_key_bytes(),
        /* voting right */ 0,
    );
    let committee = Committee::new(0, authorities).unwrap();

    let transaction = Transaction::from_data(
        TransactionData::new_transfer(a2, random_object_ref(), a1, random_object_ref(), 10000),
        &sec1,
    );
    let bad_transaction = Transaction::from_data(
        TransactionData::new_transfer(a2, random_object_ref(), a1, random_object_ref(), 10000),
        &sec2,
    );

    let v = SignedTransaction::new(
        committee.epoch(),
        transaction.clone(),
        *sec1.public_key_bytes(),
        &sec1,
    );
    assert!(v.verify(&committee).is_ok());

    let v = SignedTransaction::new(
        committee.epoch(),
        transaction.clone(),
        *sec2.public_key_bytes(),
        &sec2,
    );
    assert!(v.verify(&committee).is_err());

    let v = SignedTransaction::new(
        committee.epoch(),
        transaction,
        *sec3.public_key_bytes(),
        &sec3,
    );
    assert!(v.verify(&committee).is_err());

    let v = SignedTransaction::new(
        committee.epoch(),
        bad_transaction,
        *sec1.public_key_bytes(),
        &sec1,
    );
    assert!(v.verify(&committee).is_err());
}

#[test]
fn test_certificates() {
    let (a1, sec1) = get_key_pair();
    let (a2, sec2) = get_key_pair();
    let (_, sec3) = get_key_pair();

    let mut authorities = BTreeMap::new();
    authorities.insert(
        /* address */ *sec1.public_key_bytes(),
        /* voting right */ 1,
    );
    authorities.insert(
        /* address */ *sec2.public_key_bytes(),
        /* voting right */ 1,
    );
    let committee = Committee::new(0, authorities).unwrap();

    let transaction = Transaction::from_data(
        TransactionData::new_transfer(a2, random_object_ref(), a1, random_object_ref(), 10000),
        &sec1,
    );
    let bad_transaction = Transaction::from_data(
        TransactionData::new_transfer(a2, random_object_ref(), a1, random_object_ref(), 10000),
        &sec2,
    );

    let v1 = SignedTransaction::new(
        committee.epoch(),
        transaction.clone(),
        *sec1.public_key_bytes(),
        &sec1,
    );
    let v2 = SignedTransaction::new(
        committee.epoch(),
        transaction.clone(),
        *sec2.public_key_bytes(),
        &sec2,
    );
    let v3 = SignedTransaction::new(
        committee.epoch(),
        transaction.clone(),
        *sec3.public_key_bytes(),
        &sec3,
    );

    let mut builder = SignatureAggregator::try_new(transaction.clone(), &committee).unwrap();
    assert!(builder
        .append(v1.auth_sign_info.authority, v1.auth_sign_info.signature)
        .unwrap()
        .is_none());
    let mut c = builder
        .append(v2.auth_sign_info.authority, v2.auth_sign_info.signature)
        .unwrap()
        .unwrap();
    assert!(c.verify(&committee).is_ok());
    c.auth_sign_info.signatures.pop();
    assert!(c.verify(&committee).is_err());

    let mut builder = SignatureAggregator::try_new(transaction, &committee).unwrap();
    assert!(builder
        .append(v1.auth_sign_info.authority, v1.auth_sign_info.signature)
        .unwrap()
        .is_none());
    assert!(builder
        .append(v3.auth_sign_info.authority, v3.auth_sign_info.signature)
        .is_err());

    assert!(SignatureAggregator::try_new(bad_transaction, &committee).is_err());
}

#[derive(Serialize, Deserialize)]
struct Foo(String);
impl BcsSignable for Foo {}

#[test]
fn test_new_with_signatures() {
    let mut signatures: Vec<(AuthorityName, AuthoritySignature)> = Vec::new();
    let mut authorities = BTreeMap::new();

    for _ in 0..5 {
        let (_, sec) = get_key_pair();
        let sig = AuthoritySignature::new(&Foo("some data".to_string()), &sec);
        signatures.push((*sec.public_key_bytes(), sig));
        authorities.insert(*sec.public_key_bytes(), 1);
    }
    let (_, sec) = get_key_pair();
    authorities.insert(*sec.public_key_bytes(), 1);

    let committee = Committee::new(0, authorities.clone()).unwrap();
    let quorum =
        AuthorityStrongQuorumSignInfo::new_with_signatures(0, signatures.clone(), &committee)
            .unwrap();

    let sig_clone = signatures.clone();
    let mut alphabetical_authorities = sig_clone
        .iter()
        .map(|(pubx, _)| pubx)
        .collect::<Vec<&AuthorityName>>();
    alphabetical_authorities.sort();
    assert_eq!(
        quorum
            .authorities(&committee)
            .collect::<SuiResult<Vec<&AuthorityName>>>()
            .unwrap(),
        alphabetical_authorities
    );
}

#[test]
fn test_add_signatures() {
    let mut signatures: Vec<(AuthorityName, AuthoritySignature)> = Vec::new();
    let mut authorities = BTreeMap::new();
    let mut quorum = AuthorityStrongQuorumSignInfo::new(0);

    for _ in 0..5 {
        let (_, sec) = get_key_pair();
        let sig = AuthoritySignature::new(&Foo("some data".to_string()), &sec);
        signatures.push((*sec.public_key_bytes(), sig));
        authorities.insert(*sec.public_key_bytes(), 1);
    }

    let committee = Committee::new(0, authorities.clone()).unwrap();

    for (name, sig) in &signatures {
        assert!(quorum.add_signature(*sig, *name, &committee).is_ok());
    }

    let sig_clone = signatures.clone();
    let mut alphabetical_authorities = sig_clone
        .iter()
        .map(|(pubx, _)| pubx)
        .collect::<Vec<&AuthorityName>>();
    alphabetical_authorities.sort();

    assert_eq!(
        quorum
            .authorities(&committee)
            .collect::<SuiResult<Vec<&AuthorityName>>>()
            .unwrap(),
        alphabetical_authorities
    );
}

fn get_obligation_input<T>(value: &T) -> (VerificationObligation, usize)
where
    T: BcsSignable,
{
    let mut obligation = VerificationObligation::default();

    // Add the obligation of the authority signature verifications.
    let mut sign_message: Vec<u8> = Vec::new();
    value.write(&mut sign_message);
    let idx = obligation.add_message(sign_message);
    (obligation, idx)
}

#[test]
fn test_handle_reject_malicious_signature() {
    let message: messages_tests::Foo = Foo("some data".to_string());
    let mut signatures: Vec<(AuthorityName, AuthoritySignature)> = Vec::new();
    let mut authorities = BTreeMap::new();

    for _ in 0..5 {
        let (_, sec) = get_key_pair();
        let sig = AuthoritySignature::new(&Foo("some data".to_string()), &sec);
        authorities.insert(*sec.public_key_bytes(), 1);
        signatures.push((*sec.public_key_bytes(), sig));
    }

    let committee = Committee::new(0, authorities.clone()).unwrap();
    let mut quorum =
        AuthorityStrongQuorumSignInfo::new_with_signatures(0, signatures, &committee).unwrap();
    {
        let (_, sec) = get_key_pair();
        let sig = AuthoritySignature::new(&message, &sec);
        let sigs_len = quorum.signatures.len();
        quorum.signatures[sigs_len - 1] = sig;
    }
    let (mut obligation, idx) = get_obligation_input(&message);
    assert!(quorum
        .add_to_verification_obligation(&committee, &mut obligation, idx)
        .is_ok());
    assert!(obligation.verify_all().is_err());
}

#[test]
fn test_bitmap_out_of_range() {
    let message: messages_tests::Foo = Foo("some data".to_string());
    let mut signatures: Vec<(AuthorityName, AuthoritySignature)> = Vec::new();
    let mut authorities = BTreeMap::new();
    for _ in 0..5 {
        let (_, sec) = get_key_pair();
        let sig = AuthoritySignature::new(&Foo("some data".to_string()), &sec);
        authorities.insert(*sec.public_key_bytes(), 1);
        signatures.push((*sec.public_key_bytes(), sig));
    }

    let committee = Committee::new(0, authorities.clone()).unwrap();
    let mut quorum =
        AuthorityStrongQuorumSignInfo::new_with_signatures(0, signatures, &committee).unwrap();

    // Insert outside of range
    quorum.signers_map.insert(10);

    let (mut obligation, idx) = get_obligation_input(&message);
    assert!(quorum
        .add_to_verification_obligation(&committee, &mut obligation, idx)
        .is_err());
}

#[test]
fn test_reject_extra_public_key() {
    let message: messages_tests::Foo = Foo("some data".to_string());
    let mut signatures: Vec<(AuthorityName, AuthoritySignature)> = Vec::new();
    let mut authorities = BTreeMap::new();
    for _ in 0..5 {
        let (_, sec) = get_key_pair();
        let sig = AuthoritySignature::new(&Foo("some data".to_string()), &sec);
        authorities.insert(*sec.public_key_bytes(), 1);
        signatures.push((*sec.public_key_bytes(), sig));
    }

    signatures.sort_by_key(|k| k.0);

    let used_signatures: Vec<(AuthorityName, AuthoritySignature)> =
        vec![signatures[0], signatures[1], signatures[2], signatures[3]];

    let committee = Committee::new(0, authorities.clone()).unwrap();
    let mut quorum =
        AuthorityStrongQuorumSignInfo::new_with_signatures(0, used_signatures, &committee).unwrap();

    quorum.signers_map.insert(3);

    let (mut obligation, idx) = get_obligation_input(&message);
    assert!(quorum
        .add_to_verification_obligation(&committee, &mut obligation, idx)
        .is_ok());
}

#[test]
fn test_reject_reuse_signatures() {
    let message: messages_tests::Foo = Foo("some data".to_string());
    let mut signatures: Vec<(AuthorityName, AuthoritySignature)> = Vec::new();
    let mut authorities = BTreeMap::new();
    for _ in 0..5 {
        let (_, sec) = get_key_pair();
        let sig = AuthoritySignature::new(&Foo("some data".to_string()), &sec);
        authorities.insert(*sec.public_key_bytes(), 1);
        signatures.push((*sec.public_key_bytes(), sig));
    }

    let used_signatures: Vec<(AuthorityName, AuthoritySignature)> =
        vec![signatures[0], signatures[1], signatures[2], signatures[2]];

    let committee = Committee::new(0, authorities.clone()).unwrap();
    let quorum =
        AuthorityStrongQuorumSignInfo::new_with_signatures(0, used_signatures, &committee).unwrap();

    let (mut obligation, idx) = get_obligation_input(&message);
    assert!(quorum
        .add_to_verification_obligation(&committee, &mut obligation, idx)
        .is_err());
}

#[test]
fn test_empty_bitmap() {
    let message: messages_tests::Foo = Foo("some data".to_string());
    let mut signatures: Vec<(AuthorityName, AuthoritySignature)> = Vec::new();
    let mut authorities = BTreeMap::new();
    for _ in 0..5 {
        let (_, sec) = get_key_pair();
        let sig = AuthoritySignature::new(&Foo("some data".to_string()), &sec);
        authorities.insert(*sec.public_key_bytes(), 1);
        signatures.push((*sec.public_key_bytes(), sig));
    }

    let committee = Committee::new(0, authorities.clone()).unwrap();
    let mut quorum =
        AuthorityStrongQuorumSignInfo::new_with_signatures(0, signatures, &committee).unwrap();
    quorum.signers_map = RoaringBitmap::new();

    let (mut obligation, idx) = get_obligation_input(&message);
    assert!(quorum
        .add_to_verification_obligation(&committee, &mut obligation, idx)
        .is_err());
}
