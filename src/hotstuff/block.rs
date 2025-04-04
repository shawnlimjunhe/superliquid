use std::{collections::HashMap, vec};

use crate::{
    hotstuff::client_command::ClientCommand,
    types::{Sha256Hash, Transaction},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    crypto::{self, QuorumCertificate},
    replica::ViewNumber,
};

pub type BlockHash = Sha256Hash;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Block {
    Genesis {
        cmd: ClientCommand,
        view_number: ViewNumber,
        justify: QuorumCertificate,
    },
    Normal {
        parent_id: BlockHash,
        cmd: ClientCommand,
        view_number: ViewNumber,
        justify: Option<QuorumCertificate>,
    },
}

#[derive(Serialize, Deserialize)]
struct HashableBlock {
    parent_id: BlockHash,
    cmd_hash: Sha256Hash,
    view_number: ViewNumber,
}

impl Block {
    pub fn create_leaf(parent: &Block, cmd: ClientCommand, view_number: ViewNumber) -> Self {
        return Self::Normal {
            parent_id: parent.hash(),
            cmd,
            view_number,
            justify: None,
        };
    }

    pub fn extends_from(
        &self,
        locked_block_hash: BlockHash,
        block_store: &HashMap<BlockHash, Block>,
    ) -> bool {
        let mut current = self;

        // check 3 parents up
        for _ in 0..3 {
            match current {
                Block::Genesis { .. } => {
                    return false;
                }
                Block::Normal { parent_id, .. } => {
                    if *parent_id == locked_block_hash {
                        return true;
                    }
                    current = match block_store.get(parent_id) {
                        Some(node) => node,
                        None => {
                            return false;
                        } // missing parent, unsafe
                    };
                }
            }
        }
        false
    }

    pub fn hash(&self) -> BlockHash {
        match self {
            Self::Genesis { .. } => Sha256::digest(b"GENESIS").into(),
            Self::Normal {
                parent_id,
                cmd,
                view_number,
                ..
            } => {
                let hashable = HashableBlock {
                    parent_id: *parent_id,
                    cmd_hash: cmd.hash(),
                    view_number: *view_number,
                };

                let encoded = bincode::serialize(&hashable).unwrap();
                Sha256::digest(&encoded).into()
            }
        }
    }

    pub fn create_genesis_block() -> Block {
        let qc = crypto::QuorumCertificate::create_genesis_qc();

        Block::Genesis {
            cmd: ClientCommand::create_empty_command(),
            view_number: 0,
            justify: qc,
        }
    }
}
