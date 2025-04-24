use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::types::transaction::Sha256Hash;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Action {
    Transfer {
        from: String,
        to: String,
        amount: u128,
    },
    Empty,
}

impl Action {
    pub(crate) fn hash(&self) -> Sha256Hash {
        let encoded = bincode::serialize(&self).unwrap();
        Sha256::digest(&encoded).into()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ClientCommand {
    pub(crate) transactions: Action,
}

impl ClientCommand {
    pub fn hash(&self) -> Sha256Hash {
        return self.transactions.hash();
    }

    pub(crate) fn create_empty_command() -> Self {
        ClientCommand {
            transactions: Action::Transfer {
                from: "".to_owned(),
                to: "".to_owned(),
                amount: 0,
            },
        }
    }
}
