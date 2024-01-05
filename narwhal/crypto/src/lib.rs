// Copyright (c) 2021, Facebook, Inc. and its affiliates
// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
#![warn(
    future_incompatible,
    nonstandard_style,
    rust_2018_idioms,
    rust_2021_compatibility
)]

use fastcrypto::{
    ed25519,
    hash::{Blake2b256, HashFunction},
    unsecure,
};

// This re-export allows using the trait-defined APIs
pub use fastcrypto::traits;

////////////////////////////////////////////////////////////////////////
/// Type aliases selecting the signature algorithm for the code base.
////////////////////////////////////////////////////////////////////////
// Here we select the types that are used by default in the code base.
// The whole code base should only:
// - refer to those aliases and not use the individual scheme implementations
// - not use the schemes in a way that break genericity (e.g. using their Struct impl functions)
// - swap one of those aliases to point to another type if necessary
//
// Beware: if you change those aliases to point to another scheme implementation, you will have
// to change all four aliases to point to concrete types that work with each other. Failure to do
// so will result in a ton of compilation errors, and worse: it will not make sense!

pub type PublicKey = unsecure::signature::UnsecurePublicKey;
pub type Signature = unsecure::signature::UnsecureSignature;
pub type AggregateSignature = unsecure::signature::UnsecureAggregateSignature;
pub type PrivateKey = unsecure::signature::UnsecurePrivateKey;
pub type KeyPair = unsecure::signature::UnsecureKeyPair;

pub type NetworkPublicKey = ed25519::Ed25519PublicKey;
pub type NetworkKeyPair = ed25519::Ed25519KeyPair;

////////////////////////////////////////////////////////////////////////

// Type alias selecting the default hash function for the code base.
pub type DefaultHashFunction = Blake2b256;
pub const DIGEST_LENGTH: usize = DefaultHashFunction::OUTPUT_SIZE;
