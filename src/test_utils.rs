#[cfg(test)]
pub mod test_helpers {

    use crate::types::transaction::{SignedTransaction, UnsignedTransaction};
    use hex::FromHex;

    use ed25519_dalek::SigningKey;

    pub fn get_signing_key() -> SigningKey {
        let sk_hex = "b0761f505ca47779b167f79bc9824bf7751e83f0af2900bf501aef58ab64c9a2";
        let sk_bytes = <[u8; 32]>::from_hex(&sk_hex).expect("Invalid hex");
        SigningKey::from_bytes(&sk_bytes)
    }

    pub fn make_transaction() -> SignedTransaction {
        let mut sk1 = get_signing_key();
        let unsigned_txn = UnsignedTransaction {
            from: "alice".into(),
            to: "bob".into(),
            amount: 42,
        };
        unsigned_txn.sign(&mut sk1)
    }
}
