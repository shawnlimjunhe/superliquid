use std::collections::HashMap;

use crate::types::transaction::{Sha256Hash, SignedTransaction};
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    crypto::{self, QuorumCertificate},
    replica::ViewNumber,
};

pub type BlockHash = Sha256Hash;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Block {
    Genesis {
        transactions: SignedTransaction,
        view_number: ViewNumber,
        justify: QuorumCertificate,
    },
    Normal {
        parent_id: BlockHash,
        transactions: SignedTransaction,
        view_number: ViewNumber,
        justify: QuorumCertificate,
        // proposer: PublicKeyString,
        // block_hash: TODO
    },
}

#[derive(Serialize, Deserialize)]
struct HashableBlock {
    parent_id: BlockHash,
    txns_hash: Sha256Hash,
    view_number: ViewNumber,
}

impl Block {
    pub fn create_leaf(
        parent: &Block,
        transactions: SignedTransaction,
        view_number: ViewNumber,
        justify: QuorumCertificate,
    ) -> Self {
        return Self::Normal {
            parent_id: parent.hash(),
            transactions,
            view_number,
            justify,
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
                transactions,
                view_number,
                ..
            } => {
                let hashable = HashableBlock {
                    parent_id: *parent_id,
                    txns_hash: transactions.hash(),
                    view_number: *view_number,
                };

                let encoded = bincode::serialize(&hashable).unwrap();
                Sha256::digest(&encoded).into()
            }
        }
    }

    pub fn transactions(&self) -> &SignedTransaction {
        match self {
            Block::Genesis { transactions, .. } => transactions,
            Block::Normal { transactions, .. } => transactions,
        }
    }

    pub fn create_genesis_block(signing_key: &mut SigningKey) -> (Block, QuorumCertificate) {
        let qc = crypto::QuorumCertificate::create_genesis_qc();

        let genesis = Block::Genesis {
            view_number: 0,
            justify: qc.clone(),
            transactions: SignedTransaction::create_empty_signed_transaction(signing_key),
        };
        return (genesis, qc);
    }
}
