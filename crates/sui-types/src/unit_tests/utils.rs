// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use fastcrypto::traits::KeyPair as KeypairTraits;
use signature::Signer;

use crate::{
    base_types::{dbg_addr, ObjectID},
    committee::Committee,
    crypto::{
        get_key_pair, get_key_pair_from_rng, AccountKeyPair, AuthorityKeyPair,
        AuthorityPublicKeyBytes, Signature,
    },
    intent::{Intent, IntentMessage},
    messages::{Transaction, TransactionData, VerifiedTransaction},
    object::Object,
};
use std::{collections::BTreeMap, sync::Arc};

pub fn make_committee_key<R>(rand: &mut R) -> (Vec<AuthorityKeyPair>, Committee)
where
    R: rand::CryptoRng + rand::RngCore,
{
    make_committee_key_num(4, rand)
}

pub fn make_committee_key_num<R>(num: usize, rand: &mut R) -> (Vec<AuthorityKeyPair>, Committee)
where
    R: rand::CryptoRng + rand::RngCore,
{
    let mut authorities: BTreeMap<AuthorityPublicKeyBytes, u64> = BTreeMap::new();
    let mut keys = Vec::new();

    for _ in 0..num {
        let (_, inner_authority_key): (_, AuthorityKeyPair) = get_key_pair_from_rng(rand);
        authorities.insert(
            /* address */ AuthorityPublicKeyBytes::from(inner_authority_key.public()),
            /* voting right */ 1,
        );
        keys.push(inner_authority_key);
    }

    let committee = Committee::new(0, authorities).unwrap();
    (keys, committee)
}

// Creates a fake sender-signed transaction for testing. This transaction will
// not actually work.
pub fn create_fake_transaction() -> VerifiedTransaction {
    let (sender, sender_key): (_, AccountKeyPair) = get_key_pair();
    let recipient = dbg_addr(2);
    let object_id = ObjectID::random();
    let object = Object::immutable_with_id_for_testing(object_id);
    let data = TransactionData::new_transfer_sui(
        recipient,
        sender,
        None,
        object.compute_object_reference(),
        10000,
    );
    to_sender_signed_transaction(data, &sender_key)
}

// This is used to sign transaction with signer using default Intent.
pub fn to_sender_signed_transaction(
    data: TransactionData,
    signer: &dyn Signer<Signature>,
) -> VerifiedTransaction {
    VerifiedTransaction::new_unchecked(Transaction::from_data_and_signer(
        data,
        Intent::default(),
        signer,
    ))
}

// Workaround for benchmark setup.
pub fn to_sender_signed_transaction_arc(
    data: TransactionData,
    signer: &Arc<fastcrypto::ed25519::Ed25519KeyPair>,
) -> VerifiedTransaction {
    let data1 = data.clone();
    let intent_message = IntentMessage::new(Intent::default(), data);
    // OK to unwrap because this is used for benchmark only.
    let bytes = bcs::to_bytes(&intent_message).unwrap();
    let signature: Signature = signer.sign(&bytes);
    VerifiedTransaction::new_unchecked(Transaction::from_data(data1, Intent::default(), signature))
}
