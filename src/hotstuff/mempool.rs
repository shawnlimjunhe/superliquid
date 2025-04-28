use std::collections::{BTreeMap, HashMap, VecDeque};

use crate::{
    state::state::Nonce,
    types::transaction::{PublicKeyString, SignedTransaction, UnsignedTransaction},
};

pub type AccountQueue = BTreeMap<Nonce, SignedTransaction>;
const PRIORITY_LEVELS: u8 = 3;

pub struct PriorityMempool {
    account_queues: HashMap<PublicKeyString, AccountQueue>,
    priority_buckets: [VecDeque<SignedTransaction>; PRIORITY_LEVELS as usize],
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

    /// We allow duplicate nonce to be added to the ready bucket, but we enforce uniqueness on execution
    pub fn insert(&mut self, txn: SignedTransaction, expected_nonce: Nonce) {
        match &txn.tx {
            UnsignedTransaction::Transfer(transfer_tx) => {
                if transfer_tx.nonce < expected_nonce {
                    // reject stale transaction
                    return;
                }

                if transfer_tx.nonce == expected_nonce {
                    self.priority_buckets[Priority::Other as usize].push_back(txn);
                    return;
                }

                let account = self
                    .account_queues
                    .entry(transfer_tx.from.clone())
                    .or_default();
                account.insert(transfer_tx.nonce, txn);

                // We might have the correct transaction here, if so remove from account_queue and add to ready transactions
                if let Some(txn) = account.remove(&expected_nonce) {
                    self.priority_buckets[Priority::Other as usize].push_back(txn);
                }
            }
            UnsignedTransaction::Empty => {}
        }
    }

    pub fn pop_next(&mut self) -> Option<SignedTransaction> {
        for priority in [Priority::Liquidation, Priority::Cancel, Priority::Other] {
            if let Some(txn) = self.priority_buckets[priority as usize].pop_front() {
                return Some(txn);
            }
        }
        None
    }

    pub fn len(&self) -> usize {
        let mut sum = 0;
        for priority in [Priority::Liquidation, Priority::Cancel, Priority::Other] {
            sum += self.priority_buckets[priority as usize].len();
        }
        sum
    }

    pub fn update_after_execution(&mut self, accounts_nonces: Vec<(PublicKeyString, Nonce)>) {
        for (pk, expected_nonce) in accounts_nonces.iter() {
            let Some(account) = self.account_queues.get_mut(&pk) else {
                continue;
            };

            let Some(txn) = account.remove(expected_nonce) else {
                continue;
            };

            self.priority_buckets[Priority::Other as usize].push_back(txn);
        }
    }
}
