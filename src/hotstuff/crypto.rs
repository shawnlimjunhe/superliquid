use std::collections::HashSet;

use crate::types::Sha256Hash;
use ed25519::{Signature, signature};
use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};

use super::{
    block::BlockHash,
    hexstring,
    message::{HotStuffMessage, HotStuffMessageType},
    replica::ViewNumber,
};

#[derive(Serialize, Deserialize, Debug, Clone)]
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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct QuorumCertificate {
    pub message_type: HotStuffMessageType,
    pub view_number: ViewNumber,
    pub block_hash: BlockHash,
    message_hash: Sha256Hash,
    partial_sigs: Vec<PartialSig>,
}

impl QuorumCertificate {
    pub fn from_votes_unchecked(votes: &Vec<HotStuffMessage>) -> Option<Self> {
        // assume all votes are valid and consistent
        let first_vote = votes.first()?;

        let block_hash = match &first_vote.node {
            Some(block) => block.hash(),
            None => [0; 32],
        };

        let view_number = first_vote.view_number;
        let message_type = first_vote.message_type.clone();

        let sigs = votes.iter().filter_map(|v| v.partial_sig.clone()).collect();

        Some(QuorumCertificate {
            message_type,
            view_number,
            block_hash,
            message_hash: first_vote.hash(),
            partial_sigs: sigs,
        })
    }

    pub fn verify(&self, validator_set: &HashSet<VerifyingKey>, quorum_size: usize) -> bool {
        let mut unique_signers = HashSet::new();

        let mut valid_sig_count = 0;

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
    use ed25519::signature::SignerMut;
    use ed25519_dalek::SigningKey;

    use crate::hotstuff::crypto::PartialSig;

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
