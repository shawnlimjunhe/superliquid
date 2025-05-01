use std::collections::{BTreeMap, HashMap, VecDeque};

use crate::{
    state::state::Nonce,
    types::transaction::{PublicKeyHash, SignedTransaction, UnsignedTransaction},
};

/// Per-account transaction queue, ordered by nonce.
///
/// BTreeMap is used to efficiently find the smallest (next expected) nonce for an account.
/// - Fast insertion O(log n) and fast retrieval of next txn.
/// - Nonce order is critical for correctness.
/// - The number of pending transactions per account is expected to be small (typically a few transactions),
///   ensuring efficient O(log n) performance even with a BTreeMap.
pub type AccountQueue = BTreeMap<Nonce, SignedTransaction>;

const PRIORITY_LEVELS: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Priority {
    Liquidation = 0,
    Cancel = 1,
    Other = 2,
}

/// PriorityMempool organizes transactions for fast block proposal selection.
///
/// # Design Goals
/// - Prioritize urgent actions (e.g., liquidations) before normal transfers.
/// - Enforce strict per-account nonce ordering.
/// - Allow efficient lookup, insertion, and promotion of ready transactions.
/// - Support asynchronous transaction submission and network asynchrony.
///
/// # Mempool Invariants
/// - For each `(account, nonce)`:
///   - At most one future transaction (nonce > expected) exists in `account_queues`, enforced by BTreeMap semantics.
///   - Multiple ready-to-execute transactions (for the current expected nonce) may temporarily exist across the mempool, pending transactions, and new submissions.
///     - This occurs because a transaction may leave the mempool for block proposal (pending state),
///       but the LedgerState's account nonce is only updated after final execution.
/// - Final uniqueness and validity are enforced at execution time by the LedgerState.
/// - Re-submission of transactions for previously executed nonces is not allowed.
/// - Transactions are immediately removed from `account_queues` after execution.
/// - Memory safety is guaranteed by strict immediate removal of executed transactions, preventing stale references.
pub struct PriorityMempool {
    account_queues: HashMap<PublicKeyHash, AccountQueue>,
    /// VecDeque is chosen for fast push/pop from both ends.
    /// - Liquidations and cancels are processed first.
    /// - Transfers are processed after urgent actions are handled.
    priority_buckets: [VecDeque<(PublicKeyHash, Nonce)>; PRIORITY_LEVELS as usize],
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
                    return;
                }

                let account = self
                    .account_queues
                    .entry(transfer_tx.from.clone())
                    .or_default();

                if transfer_tx.nonce == expected_nonce {
                    self.priority_buckets[Priority::Other as usize]
                        .push_back((transfer_tx.from.clone(), expected_nonce));
                }

                account.insert(transfer_tx.nonce, txn);
            }
        }
    }

    pub fn pop_next(&mut self) -> Option<SignedTransaction> {
        for priority in [Priority::Liquidation, Priority::Cancel, Priority::Other] {
            if let Some((pk, nonce)) = self.priority_buckets[priority as usize].pop_front() {
                match self.account_queues.get_mut(&pk) {
                    Some(account_queue) => return account_queue.remove(&nonce),
                    None => return None,
                }
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

    pub fn update_after_execution(&mut self, accounts_nonces: Vec<Option<(PublicKeyHash, Nonce)>>) {
        for (pk, expected_nonce) in accounts_nonces.into_iter().flatten() {
            let Some(account) = self.account_queues.get_mut(&pk) else {
                continue;
            };

            if account.contains_key(&expected_nonce) {
                self.priority_buckets[Priority::Other as usize].push_back((pk, expected_nonce));
            }
        }
    }
}
