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
    length: usize,
    ready_transactions_length: usize,
    account_queues: HashMap<PublicKeyHash, AccountQueue>,
    /// VecDeque is chosen for fast push/pop from both ends.
    /// - Liquidations and cancels are processed first.
    /// - Transfers are processed after urgent actions are handled.
    priority_buckets: [VecDeque<(PublicKeyHash, Nonce)>; PRIORITY_LEVELS as usize],
}

impl PriorityMempool {
    pub fn new() -> Self {
        PriorityMempool {
            length: 0,
            ready_transactions_length: 0,
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
                    self.ready_transactions_length += 1;
                }
                self.length += 1;
                account.insert(transfer_tx.nonce, txn);
            }
        }
    }

    pub fn pop_next(&mut self) -> Option<SignedTransaction> {
        for priority in [Priority::Liquidation, Priority::Cancel, Priority::Other] {
            if let Some((pk, nonce)) = self.priority_buckets[priority as usize].pop_front() {
                match self.account_queues.get_mut(&pk) {
                    Some(account_queue) => {
                        self.length -= 1;
                        self.ready_transactions_length -= 1;
                        return account_queue.remove(&nonce);
                    }
                    None => return None,
                }
            }
        }
        None
    }

    pub fn len(&self) -> usize {
        self.length
    }

    pub fn ready_transactions_length(&self) -> usize {
        self.ready_transactions_length
    }

    pub fn update_after_execution(&mut self, accounts_nonces: Vec<Option<(PublicKeyHash, Nonce)>>) {
        for (pk, next_expected_nonce) in accounts_nonces.into_iter().flatten() {
            let Some(account) = self.account_queues.get_mut(&pk) else {
                continue;
            };

            if account.contains_key(&next_expected_nonce) {
                self.priority_buckets[Priority::Other as usize]
                    .push_back((pk, next_expected_nonce));
                self.ready_transactions_length += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::state::Nonce;
    use crate::test_utils::test_helpers::get_alice_sk;
    use crate::types::transaction::{SignedTransaction, TransferTransaction};

    fn mock_tx(pk: PublicKeyHash, nonce: Nonce) -> SignedTransaction {
        let mut alice_sk = get_alice_sk();
        let tx = UnsignedTransaction::Transfer(TransferTransaction {
            from: pk,
            to: [2u8; 32],
            amount: 10,
            nonce,
        });
        tx.sign(&mut alice_sk)
    }

    #[test]
    fn test_len() {
        let pk = [1u8; 32];
        let mut mempool = PriorityMempool::new();

        let tx = mock_tx(pk, 0);
        mempool.insert(tx, 0);
        assert_eq!(mempool.len(), 1);
        assert_eq!(mempool.ready_transactions_length(), 1);
        mempool.pop_next();
        assert_eq!(mempool.ready_transactions_length(), 0);
    }

    #[test]
    fn test_pop_next_returns_ready_tx() {
        let pk = [1u8; 32];
        let mut mempool = PriorityMempool::new();

        let tx = mock_tx(pk, 0);
        mempool.insert(tx.clone(), 0);
        assert_eq!(mempool.ready_transactions_length(), 1);
        let popped = mempool.pop_next().unwrap();
        assert_eq!(popped.hash, tx.hash);
        assert_eq!(mempool.len(), 0);
    }

    #[test]
    fn test_pop_next_returns_ready_tx_in_insertion_order() {
        let pk_1 = [1u8; 32];
        let pk_2 = [2u8; 32];
        let mut mempool = PriorityMempool::new();

        let tx_1 = mock_tx(pk_1, 0);
        let tx_2 = mock_tx(pk_2, 0);
        mempool.insert(tx_1.clone(), 0);
        mempool.insert(tx_2.clone(), 0);
        assert_eq!(mempool.ready_transactions_length(), 2);
        let popped = mempool.pop_next().unwrap();
        assert_eq!(popped.hash, tx_1.hash);
        assert_eq!(mempool.len(), 1);

        let popped = mempool.pop_next().unwrap();
        assert_eq!(popped.hash, tx_2.hash);
        assert_eq!(mempool.len(), 0);
    }

    #[test]
    fn test_pop_next_does_not_return_future_tx() {
        let pk = [1u8; 32];
        let mut mempool = PriorityMempool::new();

        let tx = mock_tx(pk, 1);
        mempool.insert(tx.clone(), 0);
        assert_eq!(mempool.ready_transactions_length(), 0);
        let popped = mempool.pop_next();
        assert!(popped.is_none());
        assert_eq!(mempool.len(), 1);
        assert_eq!(mempool.ready_transactions_length(), 0);
    }

    #[test]
    fn test_insert_ignores_old_nonce() {
        let pk = [1u8; 32];
        let mut mempool = PriorityMempool::new();

        let tx = mock_tx(pk, 0);
        mempool.insert(tx.clone(), 1); // old nonce
        assert_eq!(mempool.len(), 0);
    }

    #[test]
    fn test_update_after_execution_pushes_ready_nonce() {
        let pk = [1u8; 32];
        let mut mempool = PriorityMempool::new();

        // Insert two txs: nonce 0 (ready), 1 (future)
        let tx0 = mock_tx(pk, 0);
        let tx1 = mock_tx(pk, 1);
        mempool.insert(tx0, 0);
        mempool.insert(tx1.clone(), 0);

        let _ = mempool.pop_next().unwrap();
        // Execute nonce 0
        let next_nonce = Some((pk, 1));
        mempool.update_after_execution(vec![next_nonce]);

        // Should now pop tx1
        let tx = mempool.pop_next().unwrap();

        assert_eq!(tx.hash, tx1.hash);
    }
}
