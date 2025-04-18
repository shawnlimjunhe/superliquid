use std::collections::HashSet;

use crate::types::Sha256Hash;
use ed25519::Signature;
use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};

use super::{block::BlockHash, hexstring, message::HotStuffMessage, replica::ViewNumber};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PartialSig {
    #[serde(
        serialize_with = "hexstring::serialize_verifying_key",
        deserialize_with = "hexstring::deserialize_verifying_key"
    )]
    pub signer_id: VerifyingKey,

    #[serde(
        serialize_with = "hexstring::serialize_signature",
        deserialize_with = "hexstring::deserialize_signature"
    )]
    pub signature: Signature,
}

impl PartialSig {
    pub fn new(signer_id: VerifyingKey, signature: Signature) -> Self {
        Self {
            signer_id,
            signature,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct QuorumCertificate {
    pub(crate) view_number: ViewNumber,
    pub(crate) block_hash: BlockHash,
    pub(crate) message_hash: Sha256Hash,
    pub(crate) partial_sigs: Vec<PartialSig>,
}

impl QuorumCertificate {
    const GENESIS_BLOCK_HASH: [u8; 32] = [
        144, 17, 49, 216, 56, 177, 122, 172, 15, 120, 133, 184, 30, 3, 203, 220, 159, 81, 87, 160,
        3, 67, 211, 10, 178, 32, 131, 104, 94, 209, 65, 106,
    ];

    pub(crate) fn create_genesis_qc() -> QuorumCertificate {
        QuorumCertificate {
            view_number: 0,
            block_hash: Self::GENESIS_BLOCK_HASH,
            message_hash: [0u8; 32],
            partial_sigs: vec![],
        }
    }

    pub fn from_votes_unchecked(votes: &Vec<HotStuffMessage>) -> Option<Self> {
        // assume all votes are valid and consistent
        let first_vote = votes.first()?;

        let block_hash = match &first_vote.node {
            Some(block) => block.hash(),
            None => [0; 32],
        };

        let view_number = first_vote.view_number;

        let sigs = votes.iter().filter_map(|v| v.partial_sig.clone()).collect();

        Some(QuorumCertificate {
            view_number,
            block_hash,
            message_hash: first_vote.hash(),
            partial_sigs: sigs,
        })
    }

    pub fn verify(&self, validator_set: &HashSet<VerifyingKey>, quorum_size: usize) -> bool {
        let mut unique_signers = HashSet::new();

        let mut valid_sig_count = 0;

        if self.view_number == 0 && self.block_hash == Self::GENESIS_BLOCK_HASH {
            return true;
        }

        for sig in &self.partial_sigs {
            let pk = &sig.signer_id;
            // count signatures only from known validators
            if !validator_set.contains(pk) {
                continue;
            }

            if !unique_signers.insert(pk) {
                continue;
            }

            // verify that signatures are valid
            if pk.verify_strict(&self.message_hash, &sig.signature).is_ok() {
                valid_sig_count += 1;
            }
        }

        valid_sig_count >= quorum_size
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use ed25519::signature::SignerMut;
    use ed25519_dalek::SigningKey;

    use crate::{
        hotstuff::{crypto::PartialSig, message::HotStuffMessage, replica::ViewNumber},
        types::Sha256Hash,
    };

    use super::QuorumCertificate;

    impl QuorumCertificate {
        pub fn mock(view_number: ViewNumber) -> Self {
            QuorumCertificate {
                view_number,
                block_hash: [0u8; 32],
                message_hash: [0u8; 32],
                partial_sigs: vec![],
            }
        }
    }

    #[test]
    fn test_create_genesis_qc_defaults() {
        let qc = QuorumCertificate::create_genesis_qc();
        assert_eq!(qc.view_number, 0);
        assert_eq!(
            qc.block_hash,
            [
                144, 17, 49, 216, 56, 177, 122, 172, 15, 120, 133, 184, 30, 3, 203, 220, 159, 81,
                87, 160, 3, 67, 211, 10, 178, 32, 131, 104, 94, 209, 65, 106,
            ]
        );
        assert_eq!(qc.message_hash, [0u8; 32]);
        assert!(qc.partial_sigs.is_empty());
    }

    #[test]
    fn test_from_votes_unchecked_constructs_qc() {
        let mut signing_key = SigningKey::generate(&mut rand::rngs::OsRng);
        let public_key = signing_key.verifying_key();
        let message_hash = Sha256Hash::from([1u8; 32]);

        let signature = signing_key.sign(&message_hash);
        let partial_sig = PartialSig::new(public_key, signature);

        let vote = HotStuffMessage {
            view_number: 1,
            node: None,
            justify: None,
            partial_sig: Some(partial_sig.clone()),
            sender: 1,
            sender_view: 0,
        };

        let qc = QuorumCertificate::from_votes_unchecked(&vec![vote]).unwrap();

        assert_eq!(qc.view_number, 1);
        assert_eq!(qc.partial_sigs.len(), 1);
        assert_eq!(
            qc.partial_sigs[0].signer_id.to_bytes(),
            public_key.to_bytes()
        );
    }

    #[test]
    fn test_verify_qc_with_valid_sigs() {
        let mut sk1 = SigningKey::generate(&mut rand::rngs::OsRng);
        let pk1 = sk1.verifying_key();
        let message_hash = Sha256Hash::from([2u8; 32]);
        let sig1 = sk1.sign(&message_hash);

        let mut sk2 = SigningKey::generate(&mut rand::rngs::OsRng);
        let pk2 = sk2.verifying_key();
        let sig2 = sk2.sign(&message_hash);

        let mut sk3 = SigningKey::generate(&mut rand::rngs::OsRng);
        let pk3 = sk3.verifying_key();
        let sig3 = sk3.sign(&message_hash);

        let sk4 = SigningKey::generate(&mut rand::rngs::OsRng);
        let pk4 = sk4.verifying_key();

        let partials = vec![
            PartialSig::new(pk1, sig1),
            PartialSig::new(pk2, sig2),
            PartialSig::new(pk3, sig3),
        ];

        let qc = QuorumCertificate {
            view_number: 5,
            block_hash: [3u8; 32],
            message_hash,
            partial_sigs: partials.clone(),
        };

        let mut validator_set = HashSet::new();
        validator_set.insert(pk1);
        validator_set.insert(pk2);
        validator_set.insert(pk3);
        validator_set.insert(pk4);

        assert!(qc.verify(&validator_set, 3));
        assert!(!qc.verify(&validator_set, 4)); // not enough
    }

    #[test]
    fn test_verify_qc_rejects_invalid_sigs() {
        let mut sk1 = SigningKey::generate(&mut rand::rngs::OsRng);
        let pk1 = sk1.verifying_key();

        let mut sk2 = SigningKey::generate(&mut rand::rngs::OsRng);
        let pk2 = sk2.verifying_key();

        let wrong_message = Sha256Hash::from([10u8; 32]);

        let sig1 = sk1.sign(&Sha256Hash::from([9u8; 32])); // incorrect message
        let sig2 = sk2.sign(&Sha256Hash::from([8u8; 32])); // incorrect message

        let partials = vec![PartialSig::new(pk1, sig1), PartialSig::new(pk2, sig2)];

        let qc = QuorumCertificate {
            view_number: 2,
            block_hash: [11u8; 32],
            message_hash: wrong_message,
            partial_sigs: partials,
        };

        let mut validator_set = HashSet::new();
        validator_set.insert(pk1);
        validator_set.insert(pk2);

        assert!(!qc.verify(&validator_set, 1));
        assert!(!qc.verify(&validator_set, 2));
    }

    #[test]
    fn test_signature_serialization_round_trip() {
        let mut signing_key = SigningKey::generate(&mut rand::rngs::OsRng);
        let verifying_key = signing_key.verifying_key();
        let message = b"hello consensus";
        let signature = signing_key.sign(message);

        let partialsig = PartialSig::new(verifying_key, signature);

        // Serialize
        let json = serde_json::to_string(&partialsig).expect("Failed to serialize signature");

        // Deserialize
        let deserialized: PartialSig =
            serde_json::from_str(&json).expect("Failed to deserialize signature");

        assert_eq!(
            partialsig.signature.to_bytes(),
            deserialized.signature.to_bytes()
        );

        assert_eq!(
            partialsig.signer_id.to_bytes(),
            deserialized.signer_id.to_bytes()
        );
    }
}
