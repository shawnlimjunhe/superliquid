use crate::types::{Sha256Hash, Transaction};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientCommand {
    transactions: Transaction,
}

impl ClientCommand {
    pub fn hash(&self) -> Sha256Hash {
        return self.transactions.hash();
    }

    pub(crate) fn create_empty_command() -> Self {
        ClientCommand {
            transactions: Transaction {
                from: "".to_owned(),
                to: "".to_owned(),
                amount: 0,
            },
        }
    }
}
