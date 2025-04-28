use std::collections::{BTreeMap, HashMap};

use crate::{
    state::state::Nonce,
    types::transaction::{PublicKeyString, SignedTransaction, UnsignedTransaction},
};

pub type AccountQueue = BTreeMap<Nonce, SignedTransaction>;
const PRIORITY_LEVELS: u8 = 3;

pub struct PriorityMempool {
    account_queues: HashMap<PublicKeyString, AccountQueue>,
    priority_buckets: [Vec<SignedTransaction>; PRIORITY_LEVELS as usize],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Priority {
    Liquidation = 0,
    Cancel = 1,
    Other = 2,
}

impl PriorityMempool {
    pub fn new() -> Self {
        PriorityMempool {
            account_queues: HashMap::new(),
            priority_buckets: Default::default(),
        }
    }

    fn insert(&mut self, txn: SignedTransaction) {
        match &txn.tx {
            UnsignedTransaction::Transfer(tx) => {
                let account = self.account_queues.entry(tx.from.clone()).or_default();
            }
            UnsignedTransaction::Empty => {}
        }
    }

    fn pop_next_n(&mut self, n: u16) -> Vec<SignedTransaction> {
        todo!()
    }
}
