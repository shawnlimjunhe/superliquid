use std::{fmt, ops::Deref};

use ed25519::{Signature, signature::SignerMut};
use ed25519_dalek::{SigningKey, VerifyingKey};
use hex::{FromHex, encode as hex_encode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::hotstuff::utils;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HashableTransaction {
    pub from: PublicKeyString,
    pub to: PublicKeyString,
    pub amount: u128,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UnsignedTransaction {
    pub from: PublicKeyString,
    pub to: PublicKeyString,
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
    pub fn verify_sender(&self) -> bool {
        let public_key = self.from.as_public_key();
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct PublicKeyString(pub String);

impl PublicKeyString {
    pub fn from_public_key(pk: &VerifyingKey) -> Self {
        let pk_string = hex_encode(pk.to_bytes());
        PublicKeyString(pk_string)
    }

    pub fn from_string(s: String) -> Result<Self, &'static str> {
        let bytes = <[u8; 32]>::from_hex(&s).map_err(|_| "Invalid hex")?;
        VerifyingKey::from_bytes(&bytes).expect("Invalid hex");
        Ok(PublicKeyString(s))
    }

    pub fn as_public_key(&self) -> VerifyingKey {
        let bytes = <[u8; 32]>::from_hex(&self.0).unwrap();
        VerifyingKey::from_bytes(&bytes).expect("Expect to be public key")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Deref for PublicKeyString {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for PublicKeyString {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PublicKeyString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Default for PublicKeyString {
    fn default() -> Self {
        PublicKeyString(
            "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    fn generate_keypair() -> (SigningKey, VerifyingKey) {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        (signing_key, verifying_key)
    }

    #[test]
    fn test_unsigned_transaction_hash_stable() {
        let tx1 = UnsignedTransaction {
            from: PublicKeyString::default(),
            to: PublicKeyString::default(),
            amount: 100,
        };
        let tx2 = tx1.clone();
        assert_eq!(tx1.hash(), tx2.hash());
    }

    #[test]
    fn test_sign_and_verify_transaction() {
        let (mut signing_key, verifying_key) = generate_keypair();

        let unsigned = UnsignedTransaction {
            from: PublicKeyString::from_public_key(&verifying_key),
            to: PublicKeyString::default(),
            amount: 123,
        };

        let signed = unsigned.clone().sign(&mut signing_key);
        assert!(signed.verify_sender(), "Signature should verify correctly");
    }

    #[test]
    fn test_invalid_signature_fails_verification() {
        let (_signing_key1, verifying_key1) = generate_keypair();
        let (mut signing_key2, _) = generate_keypair();

        let unsigned = UnsignedTransaction {
            from: PublicKeyString::from_public_key(&verifying_key1),
            to: PublicKeyString::default(),
            amount: 456,
        };

        let signed = unsigned.clone().sign(&mut signing_key2); // signed with wrong key
        assert!(
            !signed.verify_sender(),
            "Signature verification should fail with wrong key"
        );
    }

    #[test]
    fn test_signature_string_conversion() {
        let (mut signing_key, _) = generate_keypair();

        let unsigned = UnsignedTransaction {
            from: PublicKeyString::default(),
            to: PublicKeyString::default(),
            amount: 789,
        };

        let signed = unsigned.clone().sign(&mut signing_key);

        let sig_str = signed.signature.as_str();
        let sig_bytes = SignatureString::new(sig_str.to_string()).expect("Should parse hex");
        assert_eq!(sig_bytes.as_str(), sig_str);
    }

    #[test]
    fn test_public_key_string_conversion() {
        let (_, verifying_key) = generate_keypair();

        let pk_string = PublicKeyString::from_public_key(&verifying_key);
        let reconstructed = pk_string.as_public_key();

        assert_eq!(verifying_key.to_bytes(), reconstructed.to_bytes());
    }

    #[test]
    fn test_deref_signed_transaction() {
        let (mut signing_key, verifying_key) = generate_keypair();

        let unsigned = UnsignedTransaction {
            from: PublicKeyString::from_public_key(&verifying_key),
            to: PublicKeyString::default(),
            amount: 999,
        };

        let signed = unsigned.clone().sign(&mut signing_key);

        assert_eq!(signed.amount, 999);
        assert_eq!(signed.to.as_str(), unsigned.to.as_str());
    }

    #[test]
    #[should_panic(expected = "Invalid hex")]
    fn test_signature_string_invalid_hex() {
        let invalid = "ZZZ".repeat(22); // invalid hex string
        let _ = SignatureString::new(invalid).unwrap();
    }

    #[test]
    #[should_panic(expected = "Invalid hex")]
    fn test_public_key_string_invalid_hex() {
        let invalid = "GGG".repeat(10);
        let _ = PublicKeyString::from_string(invalid).unwrap();
    }
}
