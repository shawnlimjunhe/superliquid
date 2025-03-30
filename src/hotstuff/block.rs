use std::collections::HashMap;
use serde::{ Serialize, Deserialize };
use sha2::{ Digest, Sha256 };
use crate::{ hotstuff::command::Command, types::Sha256Hash };

use super::{ crypto::QuorumCertificate, replica::ViewNumber };

pub type BlockHash = Sha256Hash;

pub struct BlockStore {
    blocks: HashMap<BlockHash, Block>,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub enum BlockViewNum {
    Genesis,
    Chained(ViewNumber),
}

pub struct Block {
    parent_id: BlockHash,
    cmd: Command,
    view_number: BlockViewNum,
    justify: Option<QuorumCertificate>,
}

#[derive(Serialize, Deserialize)]
struct HashableBlock {
    parent_id: BlockHash,
    cmd_hash: Sha256Hash,
    view_number: BlockViewNum,
}

impl Block {
    pub fn create_leaf(parent: Box<Block>, cmd: Command) {}

    pub fn hash(&self) -> BlockHash {
        let hashable = HashableBlock {
            parent_id: self.parent_id,
            cmd_hash: self.cmd.hash(),
            view_number: self.view_number,
        };

        let encoded = bincode::serialize(&hashable).unwrap();
        Sha256::digest(&encoded).into()
    }
}
