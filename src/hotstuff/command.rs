use crate::types::{ Sha256Hash, Transaction };
use serde::{ Serialize, Deserialize };

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Command {
    transactions: Transaction,
}

impl Command {
    pub fn hash(&self) -> Sha256Hash {
        return self.transactions.hash();
    }
}
