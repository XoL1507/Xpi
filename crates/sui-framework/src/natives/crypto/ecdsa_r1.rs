// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
use crate::natives::NativesCostTable;
use fastcrypto::error::FastCryptoError;
use fastcrypto::hash::{Keccak256, Sha256};
use fastcrypto::traits::RecoverableSignature;
use fastcrypto::{
    secp256r1::{
        recoverable::Secp256r1RecoverableSignature, Secp256r1PublicKey, Secp256r1Signature,
    },
    traits::ToFromBytes,
};
use move_binary_format::errors::PartialVMResult;
use move_core_types::gas_algebra::InternalGas;
use move_vm_runtime::native_charge_gas_early_exit;
use move_vm_runtime::native_functions::NativeContext;
use move_vm_types::{
    loaded_data::runtime_types::Type,
    natives::function::NativeResult,
    pop_arg,
    values::{Value, VectorRef},
};
use smallvec::smallvec;
use std::collections::VecDeque;

pub const FAIL_TO_RECOVER_PUBKEY: u64 = 0;
pub const INVALID_SIGNATURE: u64 = 1;

pub const KECCAK256: u8 = 0;
pub const SHA256: u8 = 1;

const KECCAK256_BLOCK_SIZE: usize = 136;
const SHA256_BLOCK_SIZE: usize = 64;

#[derive(Clone)]
pub struct EcdsaR1EcrecoverCostParams {
    /// Base cost for invoking the `ecrecover` function with `hash=0` implying KECCAK256
    pub ecdsa_r1_ecrecover_keccak256_cost_base: InternalGas,
    ///  Cost per byte of `msg` with `hash=0`implying KECCAK256
    pub ecdsa_r1_ecrecover_keccak256_msg_cost_per_byte: InternalGas,
    ///  Cost per block of `msg` with `hash=0`implying KECCAK256, with block size = 136
    pub ecdsa_r1_ecrecover_keccak256_msg_cost_per_block: InternalGas,

    /// Base cost for invoking the `ecrecover` function with `hash=1` implying SHA256
    pub ecdsa_r1_ecrecover_sha256_cost_base: InternalGas,
    ///  Cost per byte of `msg` with `hash=1`implying SHA256
    pub ecdsa_r1_ecrecover_sha256_msg_cost_per_byte: InternalGas,
    ///  Cost per block of `msg` with `hash=1`implying SHA256, with block size = 64
    pub ecdsa_r1_ecrecover_sha256_msg_cost_per_block: InternalGas,
}
pub fn ecrecover(
    context: &mut NativeContext,
    ty_args: Vec<Type>,
    mut args: VecDeque<Value>,
) -> PartialVMResult<NativeResult> {
    debug_assert!(ty_args.is_empty());
    debug_assert!(args.len() == 3);

    let hash = pop_arg!(args, u8);

    // Load the cost paramaters from the protocol config
    let (ecdsa_r1_ecrecover_cost_params, crypto_invalid_arguments_cost) = {
        let cost_table = &context.extensions().get::<NativesCostTable>();
        (
            cost_table.ecdsa_r1_ecrecover_cost_params.clone(),
            cost_table.crypto_invalid_arguments_cost,
        )
    };

    let (base_cost, cost_per_byte, cost_per_block, block_size) = match hash {
        KECCAK256 => (
            ecdsa_r1_ecrecover_cost_params.ecdsa_r1_ecrecover_keccak256_cost_base,
            ecdsa_r1_ecrecover_cost_params.ecdsa_r1_ecrecover_keccak256_msg_cost_per_byte,
            ecdsa_r1_ecrecover_cost_params.ecdsa_r1_ecrecover_keccak256_msg_cost_per_block,
            KECCAK256_BLOCK_SIZE,
        ),
        SHA256 => (
            ecdsa_r1_ecrecover_cost_params.ecdsa_r1_ecrecover_sha256_cost_base,
            ecdsa_r1_ecrecover_cost_params.ecdsa_r1_ecrecover_sha256_msg_cost_per_byte,
            ecdsa_r1_ecrecover_cost_params.ecdsa_r1_ecrecover_sha256_msg_cost_per_block,
            SHA256_BLOCK_SIZE,
        ),
        _ => {
            // Charge for failure but dont fail if we run out of gas otherwise the actual error is masked by OUT_OF_GAS error
            context.charge_gas(crypto_invalid_arguments_cost);

            return Ok(NativeResult::err(
                context.gas_used(),
                FAIL_TO_RECOVER_PUBKEY,
            ));
        }
    };

    // Charge the base cost for this oper
    native_charge_gas_early_exit!(context, base_cost);

    let msg = pop_arg!(args, VectorRef);
    let signature = pop_arg!(args, VectorRef);

    let msg_ref = msg.as_bytes_ref();
    let signature_ref = signature.as_bytes_ref();

    // Charge the arg size dependent costs
    native_charge_gas_early_exit!(
        context,
        cost_per_byte * (msg_ref.len() as u64).into()
            + cost_per_block * (((msg_ref.len() + block_size - 1) / block_size) as u64).into()
    );

    let cost = context.gas_used();

    let Ok(sig) = <Secp256r1RecoverableSignature as ToFromBytes>::from_bytes(&signature_ref) else {
        return Ok(NativeResult::err(cost, INVALID_SIGNATURE));
    };

    let pk = match hash {
        KECCAK256 => sig.recover_with_hash::<Keccak256>(&msg_ref),
        SHA256 => sig.recover_with_hash::<Sha256>(&msg_ref),
        _ => Err(FastCryptoError::InvalidInput),
    };

    match pk {
        Ok(pk) => Ok(NativeResult::ok(
            cost,
            smallvec![Value::vector_u8(pk.as_bytes().to_vec())],
        )),
        Err(_) => Ok(NativeResult::err(cost, FAIL_TO_RECOVER_PUBKEY)),
    }
}

#[derive(Clone)]
pub struct EcdsaR1Secp256R1VerifyCostParams {
    /// Base cost for invoking the `secp256r1_verify` function with `hash=0` implying KECCAK256
    pub ecdsa_r1_secp256r1_verify_keccak256_cost_base: InternalGas,
    ///  Cost per byte of `msg` with `hash=0`implying KECCAK256
    pub ecdsa_r1_secp256r1_verify_keccak256_msg_cost_per_byte: InternalGas,
    ///  Cost per block of `msg` with `hash=0`implying KECCAK256, with block size = 136
    pub ecdsa_r1_secp256r1_verify_keccak256_msg_cost_per_block: InternalGas,

    /// Base cost for invoking the `secp256r1_verify` function with `hash=1` implying SHA256
    pub ecdsa_r1_secp256r1_verify_sha256_cost_base: InternalGas,
    ///  Cost per byte of `msg` with `hash=1`implying SHA256
    pub ecdsa_r1_secp256r1_verify_sha256_msg_cost_per_byte: InternalGas,
    ///  Cost per block of `msg` with `hash=1`implying SHA256, with block size = 64
    pub ecdsa_r1_secp256r1_verify_sha256_msg_cost_per_block: InternalGas,
}
pub fn secp256r1_verify(
    context: &mut NativeContext,
    ty_args: Vec<Type>,
    mut args: VecDeque<Value>,
) -> PartialVMResult<NativeResult> {
    debug_assert!(ty_args.is_empty());
    debug_assert!(args.len() == 4);
    // Load the cost paramaters from the protocol config
    let (ecdsa_r1_secp256_r1_verify_cost_params, crypto_invalid_arguments_cost) = {
        let cost_table = &context.extensions().get::<NativesCostTable>();
        (
            cost_table.ecdsa_r1_secp256_r1_verify_cost_params.clone(),
            cost_table.crypto_invalid_arguments_cost,
        )
    };
    let hash = pop_arg!(args, u8);
    let (base_cost, cost_per_byte, cost_per_block, block_size) = match hash {
        KECCAK256 => (
            ecdsa_r1_secp256_r1_verify_cost_params.ecdsa_r1_secp256r1_verify_keccak256_cost_base,
            ecdsa_r1_secp256_r1_verify_cost_params
                .ecdsa_r1_secp256r1_verify_keccak256_msg_cost_per_byte,
            ecdsa_r1_secp256_r1_verify_cost_params
                .ecdsa_r1_secp256r1_verify_keccak256_msg_cost_per_block,
            KECCAK256_BLOCK_SIZE,
        ),
        SHA256 => (
            ecdsa_r1_secp256_r1_verify_cost_params.ecdsa_r1_secp256r1_verify_sha256_cost_base,
            ecdsa_r1_secp256_r1_verify_cost_params
                .ecdsa_r1_secp256r1_verify_sha256_msg_cost_per_byte,
            ecdsa_r1_secp256_r1_verify_cost_params
                .ecdsa_r1_secp256r1_verify_sha256_msg_cost_per_block,
            SHA256_BLOCK_SIZE,
        ),
        _ => {
            // Charge for failure but dont fail if we run out of gas otherwise the actual error is masked by OUT_OF_GAS error
            context.charge_gas(crypto_invalid_arguments_cost);
            return Ok(NativeResult::ok(
                context.gas_used(),
                smallvec![Value::bool(false)],
            ));
        }
    };

    // Charge the base cost for this oper
    native_charge_gas_early_exit!(context, base_cost);

    let msg = pop_arg!(args, VectorRef);
    let public_key_bytes = pop_arg!(args, VectorRef);
    let signature_bytes = pop_arg!(args, VectorRef);

    let msg_ref = msg.as_bytes_ref();
    let public_key_bytes_ref = public_key_bytes.as_bytes_ref();
    let signature_bytes_ref = signature_bytes.as_bytes_ref();

    // Charge the arg size dependent costs
    native_charge_gas_early_exit!(
        context,
        cost_per_byte * (msg_ref.len() as u64).into()
            + cost_per_block * (((msg_ref.len() + block_size - 1) / block_size) as u64).into()
    );

    let cost = context.gas_used();

    let Ok(sig) = <Secp256r1Signature as ToFromBytes>::from_bytes(&signature_bytes_ref) else {
        return Ok(NativeResult::ok(cost, smallvec![Value::bool(false)]));
    };

    let Ok(pk) = <Secp256r1PublicKey as ToFromBytes>::from_bytes(&public_key_bytes_ref) else {
        return Ok(NativeResult::ok(cost, smallvec![Value::bool(false)]));
    };

    let result = match hash {
        KECCAK256 => pk.verify_with_hash::<Keccak256>(&msg_ref, &sig).is_ok(),
        SHA256 => pk.verify_with_hash::<Sha256>(&msg_ref, &sig).is_ok(),
        _ => false,
    };

    Ok(NativeResult::ok(cost, smallvec![Value::bool(result)]))
}
