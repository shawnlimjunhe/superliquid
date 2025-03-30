use ed25519::Signature;
use ed25519_dalek::VerifyingKey;
use super::{ block::Block, message::{ HotStuffMessage, HotStuffMessageType }, replica::ViewNumber };

pub struct PartialSig {
    pub signer_id: VerifyingKey,
    pub signature: Signature,
}

pub struct QuorumCertificate {
    pub message_type: HotStuffMessageType,
    pub view_number: ViewNumber,
    node: Block,
    signature: Vec<Signature>,
}

impl QuorumCertificate {
    pub fn from_votes(votes: &[HotStuffMessage]) -> Option<Self> {
        None
    }
}
