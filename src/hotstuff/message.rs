use crate::{node::state::PeerId, types::Sha256Hash};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    block::{Block, BlockHash},
    crypto::{PartialSig, QuorumCertificate},
    replica::ViewNumber,
};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct HotStuffMessage {
    pub view_number: ViewNumber,
    pub node: Option<Block>,
    pub justify: Option<QuorumCertificate>,
    pub partial_sig: Option<PartialSig>,

    pub sender: PeerId,
    pub sender_view: ViewNumber,
}

// Hashable message should only contain these two f
#[derive(Serialize, Deserialize)]
pub struct HashableMessage {
    view_number: ViewNumber,
    block_hash: BlockHash,
}

impl HotStuffMessage {
    pub fn new(
        node: Option<Block>,
        option_justify: Option<QuorumCertificate>,
        curr_view: ViewNumber,
        sender: PeerId,
        sender_view: ViewNumber,
    ) -> Self {
        Self {
            view_number: curr_view,
            node,
            justify: option_justify,
            partial_sig: None,

            sender,
            sender_view,
        }
    }

    pub fn new_with_sig(
        node: Option<Block>,
        option_qc: Option<QuorumCertificate>,
        curr_view: ViewNumber,
        sender: PeerId,
        sender_view: ViewNumber,
        partial_sig: PartialSig,
    ) -> Self {
        Self {
            view_number: curr_view,
            node,
            justify: option_qc,
            partial_sig: Some(partial_sig),

            sender,
            sender_view,
        }
    }
    pub fn hash(&self) -> Sha256Hash {
        let block_hash = match &self.node {
            Some(block) => block.hash(),
            None => [0; 32],
        };
        let hashable = HashableMessage {
            view_number: self.view_number,
            block_hash,
        };

        let encoded = bincode::serialize(&hashable).unwrap();
        Sha256::digest(&encoded).into()
    }
}
