use std::collections::{ HashMap, HashSet };

use ed25519_dalek::{ SigningKey, VerifyingKey, Signer };

use super::{
    block::{ Block, BlockHash },
    config,
    crypto::{ PartialSig, QuorumCertificate },
    message::{ self, HotStuffMessage, HotStuffMessageType },
};

pub type ViewNumber = u64;

type MessageKey = (BlockHash, ViewNumber, HotStuffMessageType);

pub struct HotStuffReplica {
    pub validator_set: HashSet<VerifyingKey>,
    signing_key: SigningKey,
    view_number: ViewNumber,
    locked_qc: Option<QuorumCertificate>,
    prepare_qc: Option<QuorumCertificate>,
    current_proposal: Option<Block>,
}

impl HotStuffReplica {
    pub fn new(node_id: usize) -> Self {
        let signing_key = config::retrieve_signing_key(node_id);

        HotStuffReplica {
            validator_set: config::retrieve_validator_set(),
            signing_key,
            view_number: 0,
            locked_qc: None,
            prepare_qc: None,
            current_proposal: None,
        }
    }

    pub fn get_public_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    fn sign(&self, message: &HotStuffMessage) -> PartialSig {
        let message_hash = message.hash();
        let signature = self.signing_key.sign(&message_hash);

        PartialSig { signer_id: self.get_public_key(), signature }
    }

    pub fn vote_message(
        &self,
        message_type: HotStuffMessageType,
        node: Block,
        qc: QuorumCertificate,
        curr_view: ViewNumber
    ) -> HotStuffMessage {
        let mut message = HotStuffMessage::new(message_type, node, qc, curr_view);
        message.partial_sig = Some(self.sign(&message));
        message
    }

    pub fn matching_message(
        message: HotStuffMessage,
        message_type: HotStuffMessageType,
        view_number: ViewNumber
    ) -> bool {
        message_type == message.message_type && view_number == message.view_number
    }

    pub fn matching_qc(
        qc: QuorumCertificate,
        message_type: HotStuffMessageType,
        view_number: ViewNumber
    ) -> bool {
        qc.message_type == message_type && qc.view_number == view_number
    }

    fn quorum_threshold(&self) -> usize {
        let n = self.validator_set.len();
        2 * ((n - 1) / 3) + 1
    }

    pub fn get_highest_vote_group<'a>(votes: Vec<&'a HotStuffMessage>) -> Vec<&'a HotStuffMessage> {
        if votes.len() < 2 {
            return votes; // returns Vec<&HotStuffMessage>
        }

        let mut groups: HashMap<MessageKey, Vec<&'a HotStuffMessage>> = HashMap::new();
        let mut max_key: Option<MessageKey> = None;
        let mut max_size: usize = 0;

        // aggregates votes by message_key
        for vote in votes {
            let message_key = (vote.node.hash(), vote.view_number, vote.message_type.clone());
            let cloned_message_key = message_key.clone();
            let group = groups.entry(message_key).or_default();
            group.push(vote);

            if group.len() > max_size {
                max_key = Some(cloned_message_key);
                max_size = group.len();
            }
        }

        let key = match max_key {
            Some(message_key) => message_key,
            None => {
                return vec![];
            }
        };

        // return group with the highest count
        // in this case, we dont really care if there are mulitple groups with the same count
        groups.get(&key).cloned().unwrap_or_default()
    }

    fn validate_vote_signatures<'a>(
        &self,
        votes: &'a Vec<HotStuffMessage>
    ) -> Vec<&'a HotStuffMessage> {
        let mut validated_votes = vec![];

        let validator_set = &self.validator_set;
        let mut seen_validators = HashSet::new();

        for vote in votes.iter() {
            if let Some(partial_sig) = &vote.partial_sig {
                let verifying_key = &partial_sig.signer_id;

                if !validator_set.contains(verifying_key) || !seen_validators.insert(verifying_key) {
                    // reject if not part of validator set or validator already voted
                    continue;
                }

                if verifying_key.verify_strict(&vote.hash(), &partial_sig.signature).is_ok() {
                    validated_votes.push(vote);
                }
            }
        }

        validated_votes
    }

    pub fn validate_votes(&self, votes: &Vec<HotStuffMessage>) -> bool {
        // validate votes:
        // - all from known validators
        // - same block hash, view, type
        // - signatures are valid
        // - valid votes above quorum

        if votes.len() > self.validator_set.len() {
            return false;
        }

        let quorum_threhold = self.quorum_threshold();

        // Validate votes before grouping to defend against spam
        let validated_votes = self.validate_vote_signatures(&votes);

        if validated_votes.len() < quorum_threhold {
            return false;
        }

        let votes = Self::get_highest_vote_group(validated_votes);

        votes.len() >= quorum_threhold
    }

    pub fn from_votes(&self, votes: &Vec<HotStuffMessage>) -> Option<QuorumCertificate> {
        if self.validate_votes(votes) {
            QuorumCertificate::from_votes_unchecked(votes)
        } else {
            None
        }
    }
}
