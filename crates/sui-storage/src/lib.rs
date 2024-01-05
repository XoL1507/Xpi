// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use anyhow::{anyhow, Result};
use byteorder::ReadBytesExt;
use bytes::{Buf, Bytes};
use integer_encoding::{VarInt, VarIntReader};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Write};
use std::path::PathBuf;
use std::{fs, io};
use sui_simulator::fastcrypto::hash::{HashFunction, Sha3_256};
use sui_types::messages_checkpoint::{CertifiedCheckpointSummary, VerifiedCheckpoint};
use sui_types::storage::{ReadStore, WriteStore};
use tracing::debug;

pub mod indexes;
pub use indexes::{IndexStore, IndexStoreTables};

pub mod mutex_table;
pub mod object_store;
pub mod package_object_cache;
pub mod sharded_lru;
pub mod write_path_pending_tx_log;

pub const SHA3_BYTES: usize = 32;
pub const MAX_VARINT_LENGTH: usize = 10;
pub const BLOB_ENCODING_BYTES: usize = 1;

#[derive(
    Copy,
    Clone,
    Default,
    Debug,
    Eq,
    PartialEq,
    Serialize,
    Deserialize,
    TryFromPrimitive,
    IntoPrimitive,
)]
#[repr(u8)]
pub enum FileCompression {
    None = 0,
    #[default]
    Zstd,
}

impl FileCompression {
    fn zstd_compress(source: &std::path::Path) -> io::Result<()> {
        let mut file = File::open(source)?;
        let tmp_file_name = source.with_extension("obj.tmp");
        let mut encoder = {
            let target = File::create(&tmp_file_name)?;
            // TODO: Add zstd compression level as function argument
            zstd::Encoder::new(target, 1)?
        };
        io::copy(&mut file, &mut encoder)?;
        encoder.finish()?;
        fs::rename(tmp_file_name, source)?;
        Ok(())
    }
    pub fn compress(&self, source: &std::path::Path) -> io::Result<()> {
        match self {
            FileCompression::Zstd => {
                Self::zstd_compress(source)?;
            }
            FileCompression::None => {}
        }
        Ok(())
    }
    pub fn bulk_decompress(&self, bytes: Bytes) -> Result<Box<dyn Read>> {
        let res: Box<dyn Read> = match self {
            FileCompression::Zstd => {
                let mut decompressor = zstd::bulk::Decompressor::new()?;
                //let vec = bytes.to_vec();
                let upper_bound = zstd::bulk::Decompressor::upper_bound(bytes.as_ref())
                    .ok_or(anyhow!("Failed to read buf"))?;
                let decompressed = decompressor.decompress(bytes.as_ref(), upper_bound)?;
                Box::new(Cursor::new(decompressed))
            }
            FileCompression::None => Box::new(BufReader::new(bytes.reader())),
        };
        Ok(res)
    }
    pub fn decompress(&self, source: &PathBuf) -> Result<Box<dyn Read>> {
        let file = File::open(source)?;
        let res: Box<dyn Read> = match self {
            FileCompression::Zstd => Box::new(zstd::stream::Decoder::new(file)?),
            FileCompression::None => Box::new(BufReader::new(file)),
        };
        Ok(res)
    }
    pub fn bytes_decompress(&self, bytes: Bytes) -> Result<Box<dyn Read>> {
        let res: Box<dyn Read> = match self {
            FileCompression::Zstd => Box::new(zstd::stream::Decoder::new(bytes.reader())?),
            FileCompression::None => Box::new(BufReader::new(bytes.reader())),
        };
        Ok(res)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, TryFromPrimitive, IntoPrimitive)]
#[repr(u8)]
pub enum Encoding {
    Bcs = 1,
}

pub struct Blob {
    pub data: Vec<u8>,
    pub encoding: Encoding,
}

impl Blob {
    pub fn encode<T: Serialize>(value: &T, encoding: Encoding) -> Result<Self> {
        let value_buf = bcs::to_bytes(value)?;
        let (data, encoding) = match encoding {
            Encoding::Bcs => (value_buf, encoding),
        };
        Ok(Blob { data, encoding })
    }
    pub fn decode<T: DeserializeOwned>(self) -> Result<T> {
        let data = match &self.encoding {
            Encoding::Bcs => self.data,
        };
        let res = bcs::from_bytes(&data)?;
        Ok(res)
    }
    pub fn read<R: Read>(rbuf: &mut R) -> Result<Blob> {
        let len = rbuf.read_varint::<u64>()? as usize;
        if len == 0 {
            return Err(anyhow!("Invalid object length of 0 in file"));
        }
        let encoding = rbuf.read_u8()?;
        let mut data = vec![0u8; len];
        rbuf.read_exact(&mut data)?;
        let blob = Blob {
            data,
            encoding: Encoding::try_from(encoding)?,
        };
        Ok(blob)
    }
    pub fn write<W: Write>(&self, wbuf: &mut W) -> Result<usize> {
        let mut buf = [0u8; MAX_VARINT_LENGTH];
        let mut counter = 0;
        let n = (self.data.len() as u64).encode_var(&mut buf);
        wbuf.write_all(&buf[0..n])?;
        counter += n;
        buf[0] = self.encoding.into();
        wbuf.write_all(&buf[0..BLOB_ENCODING_BYTES])?;
        counter += 1;
        wbuf.write_all(&self.data)?;
        counter += self.data.len();
        Ok(counter)
    }
    pub fn size(&self) -> usize {
        let mut blob_size = self.data.len().required_space();
        blob_size += BLOB_ENCODING_BYTES;
        blob_size += self.data.len();
        blob_size
    }
}

pub fn compute_sha3_checksum_for_file(file: &mut File) -> Result<[u8; 32]> {
    let mut hasher = Sha3_256::default();
    io::copy(file, &mut hasher)?;
    Ok(hasher.finalize().digest)
}

pub fn compute_sha3_checksum(source: &std::path::Path) -> Result<[u8; 32]> {
    let mut file = fs::File::open(source)?;
    compute_sha3_checksum_for_file(&mut file)
}

pub fn verify_checkpoint<S>(
    current: &VerifiedCheckpoint,
    store: S,
    checkpoint: CertifiedCheckpointSummary,
) -> Result<VerifiedCheckpoint, CertifiedCheckpointSummary>
where
    S: WriteStore,
    <S as ReadStore>::Error: std::error::Error,
{
    assert_eq!(
        *checkpoint.sequence_number(),
        current.sequence_number().saturating_add(1)
    );

    if Some(*current.digest()) != checkpoint.previous_digest {
        debug!(
            current_sequence_number = current.sequence_number(),
            current_digest =% current.digest(),
            checkpoint_sequence_number = checkpoint.sequence_number(),
            checkpoint_digest =% checkpoint.digest(),
            checkpoint_previous_digest =? checkpoint.previous_digest,
            "checkpoint not on same chain"
        );
        return Err(checkpoint);
    }

    let current_epoch = current.epoch();
    if checkpoint.epoch() != current_epoch && checkpoint.epoch() != current_epoch.saturating_add(1)
    {
        debug!(
            current_epoch = current_epoch,
            checkpoint_epoch = checkpoint.epoch(),
            "cannot verify checkpoint with too high of an epoch",
        );
        return Err(checkpoint);
    }

    if checkpoint.epoch() == current_epoch.saturating_add(1)
        && current.next_epoch_committee().is_none()
    {
        debug!(
            "next checkpoint claims to be from the next epoch but the latest verified \
            checkpoint does not indicate that it is the last checkpoint of an epoch"
        );
        return Err(checkpoint);
    }

    let committee = store
        .get_committee(checkpoint.epoch())
        .expect("store operation should not fail")
        .expect("BUG: should have a committee for an epoch before we try to verify checkpoints from an epoch");

    checkpoint.verify_signature(&committee).map_err(|e| {
        debug!("error verifying checkpoint: {e}");
        checkpoint.clone()
    })?;
    Ok(VerifiedCheckpoint::new_unchecked(checkpoint))
}
