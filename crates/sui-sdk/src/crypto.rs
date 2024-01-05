// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use signature::{Error, Signer};
use std::collections::BTreeMap;
use std::fmt::Write;
use std::fmt::{Display, Formatter};
use std::fs;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use sui_types::base_types::SuiAddress;
use sui_types::crypto::{
    get_key_pair, AccountKeyPair, EncodeDecodeBase64, KeypairTraits, Signature,
};

#[derive(Serialize, Deserialize)]
#[non_exhaustive]
// This will work on user signatures, but not suitable for authority signatures.
pub enum KeystoreType {
    File(PathBuf),
}

pub trait Keystore: Send + Sync {
    fn sign(&self, address: &SuiAddress, msg: &[u8]) -> Result<Signature, signature::Error>;
    fn add_random_key(&mut self) -> Result<SuiAddress, anyhow::Error>;
    fn add_key(&mut self, keypair: AccountKeyPair) -> Result<(), anyhow::Error>;
}

impl KeystoreType {
    pub fn init(&self) -> Result<Box<dyn Keystore>, anyhow::Error> {
        Ok(match self {
            KeystoreType::File(path) => Box::new(SuiKeystore::load_or_create(path)?),
        })
    }
}

impl Display for KeystoreType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut writer = String::new();
        match self {
            KeystoreType::File(path) => {
                writeln!(writer, "Keystore Type : File")?;
                write!(writer, "Keystore Path : {:?}", path)?;
                write!(f, "{}", writer)
            }
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct SuiKeystore {
    keys: BTreeMap<SuiAddress, AccountKeyPair>,
    path: Option<PathBuf>,
}

impl Keystore for SuiKeystore {
    fn sign(&self, address: &SuiAddress, msg: &[u8]) -> Result<Signature, signature::Error> {
        self.keys
            .get(address)
            .ok_or_else(|| {
                signature::Error::from_source(format!("Cannot find key for address: [{address}]"))
            })?
            .try_sign(msg)
    }

    fn add_random_key(&mut self) -> Result<SuiAddress, anyhow::Error> {
        let (address, keypair): (_, AccountKeyPair) = get_key_pair();
        self.keys.insert(address, keypair);
        self.save()?;
        Ok(address)
    }

    fn add_key(&mut self, keypair: AccountKeyPair) -> Result<(), anyhow::Error> {
        let address: SuiAddress = keypair.public().into();
        self.keys.insert(address, keypair);
        self.save()?;
        Ok(())
    }
}

impl SuiKeystore {
    pub fn load_or_create(path: &Path) -> Result<Self, anyhow::Error> {
        let keys: Vec<AccountKeyPair> = if path.exists() {
            let reader = BufReader::new(File::open(path)?);
            let kp_strings: Vec<String> = serde_json::from_reader(reader)?;
            kp_strings
                .iter()
                .map(|kpstr| AccountKeyPair::decode_base64(kpstr))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| anyhow::anyhow!("Invalid Keypair file"))?
        } else {
            Vec::new()
        };

        let keys = keys
            .into_iter()
            .map(|key| (key.public().into(), key))
            .collect();

        Ok(Self {
            keys,
            path: Some(path.to_path_buf()),
        })
    }

    pub fn set_path(&mut self, path: &Path) {
        self.path = Some(path.to_path_buf());
    }

    pub fn save(&self) -> Result<(), anyhow::Error> {
        if let Some(path) = &self.path {
            let store = serde_json::to_string_pretty(
                &self
                    .keys
                    .values()
                    .map(|k| k.encode_base64())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            fs::write(path, store)?
        }
        Ok(())
    }

    pub fn add_key(
        &mut self,
        address: SuiAddress,
        keypair: AccountKeyPair,
    ) -> Result<(), anyhow::Error> {
        self.keys.insert(address, keypair);
        Ok(())
    }

    pub fn addresses(&self) -> Vec<SuiAddress> {
        self.keys.keys().cloned().collect()
    }

    pub fn key_pairs(&self) -> Vec<&AccountKeyPair> {
        self.keys.values().collect()
    }

    pub fn signer(&self, signer: SuiAddress) -> impl Signer<Signature> + '_ {
        SuiKeystoreSigner::new(self, signer)
    }
}

struct SuiKeystoreSigner<'a> {
    keystore: &'a SuiKeystore,
    address: SuiAddress,
}

impl<'a> SuiKeystoreSigner<'a> {
    pub fn new(keystore: &'a SuiKeystore, account: SuiAddress) -> Self {
        Self {
            keystore,
            address: account,
        }
    }
}

impl Signer<Signature> for SuiKeystoreSigner<'_> {
    fn try_sign(&self, msg: &[u8]) -> Result<Signature, Error> {
        self.keystore.sign(&self.address, msg)
    }
}
