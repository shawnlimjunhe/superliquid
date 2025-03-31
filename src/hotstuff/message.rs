use crate::types::Sha256Hash;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    block::{Block, BlockHash},
    crypto::{PartialSig, QuorumCertificate},
    replica::ViewNumber,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum HotStuffMessageType {
    NewView,
    Prepare,
    PreCommit,
    Commit,
    Decide,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HotStuffMessage {
    pub message_type: HotStuffMessageType,
    pub view_number: ViewNumber,
    pub node: Option<Block>,
    pub justify: Option<QuorumCertificate>,
    pub partial_sig: Option<PartialSig>,
}

#[derive(Serialize, Deserialize)]
pub struct HashableMessage {
    message_type: HotStuffMessageType,
    view_number: ViewNumber,
    block_hash: BlockHash,
}

impl HotStuffMessage {
    pub fn new(
        message_type: HotStuffMessageType,
        node: Option<Block>,
        option_qc: Option<QuorumCertificate>,
        curr_view: ViewNumber,
    ) -> Self {
        Self {
            message_type,
            view_number: curr_view,
            node,
            justify: option_qc,
            partial_sig: None,
        }
    }

    pub fn hash(&self) -> Sha256Hash {
        let block_hash = match &self.node {
            Some(block) => block.hash(),
            None => [0; 32],
        };
        let hashable = HashableMessage {
            message_type: self.message_type.clone(),
            view_number: self.view_number,
            block_hash,
        };

        let encoded = bincode::serialize(&hashable).unwrap();
        Sha256::digest(&encoded).into()
    }
}
