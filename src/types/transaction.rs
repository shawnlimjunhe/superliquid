use std::{fmt, ops::Deref};

use ed25519::{
    Signature,
    signature::{self, SignerMut},
};
use ed25519_dalek::{SigningKey, VerifyingKey};
use hex::FromHex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::hotstuff::utils;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HashableTransaction {
    pub from: String,
    pub to: String,
    pub amount: u128,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UnsignedTransaction {
    pub from: String,
    pub to: String,
    pub amount: u128,
}

pub type Sha256Hash = [u8; 32];

impl UnsignedTransaction {
    pub fn hash(&self) -> Sha256Hash {
        // probably want to implement my own encoding and hashing
        let encoded = bincode::serialize(&self).unwrap();
        Sha256::digest(&encoded).into()
    }

    pub fn sign(self, signing_key: &mut SigningKey) -> SignedTransaction {
        let transaction_hash = self.hash();
        let signature = signing_key.sign(&transaction_hash);
        let signature = SignatureString(utils::sig_to_string(&signature));

        SignedTransaction {
            tx: self,
            signature,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SignedTransaction {
    pub tx: UnsignedTransaction,
    pub signature: SignatureString,
}

impl SignedTransaction {
    pub fn verify(&self, public_key: &VerifyingKey) -> bool {
        let tx_hash = self.tx.hash();
        let signature = utils::string_to_sig(&self.signature.as_str())
            .expect("Conversion from string to signature failed");
        public_key.verify_strict(&tx_hash, &signature).is_ok()
    }
}

impl Deref for SignedTransaction {
    type Target = UnsignedTransaction;

    fn deref(&self) -> &Self::Target {
        &self.tx
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SignatureString(String);

impl SignatureString {
    pub fn new(s: String) -> Result<Self, &'static str> {
        let bytes = <[u8; 64]>::from_hex(&s).map_err(|_| "Invalid hex")?;
        Signature::from_bytes(&bytes);
        Ok(SignatureString(s))
    }

    pub fn as_signature(&self) -> Signature {
        let bytes = <[u8; 64]>::from_hex(&self.0).unwrap();
        Signature::from_bytes(&bytes)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Deref for SignatureString {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for SignatureString {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SignatureString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
