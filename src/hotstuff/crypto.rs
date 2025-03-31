use std::collections::HashSet;

use crate::types::Sha256Hash;
use ed25519::Signature;
use ed25519_dalek::VerifyingKey;

use super::{
    block::BlockHash,
    message::{HotStuffMessage, HotStuffMessageType},
    replica::ViewNumber,
};

#[derive(Clone)]
pub struct PartialSig {
    pub signer_id: VerifyingKey,
    pub signature: Signature,
}

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

        let block_hash = first_vote.node.hash();
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
