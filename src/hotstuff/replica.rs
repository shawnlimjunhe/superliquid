use std::collections::HashSet;

use ed25519_dalek::{ SigningKey, VerifyingKey, Signer };

use super::{
    block::Block,
    crypto::{ PartialSig, QuorumCertificate },
    message::{ HotStuffMessage, HotStuffMessageType },
    config,
};

pub type ViewNumber = u64;

pub struct HotStuffReplica {
    pub validator_set: HashSet<VerifyingKey>,
    signing_key: SigningKey,
    view_number: ViewNumber,
    locked_qc: Option<QuorumCertificate>,
    prepare_qc: Option<QuorumCertificate>,
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
}
