use serde::{Deserialize, Serialize};
use std::sync::RwLock;
use std::{collections::HashMap, sync::Arc};

use crate::{
    config,
    hotstuff::block::Block,
    types::transaction::{PublicKeyString, SignedTransaction, UnsignedTransaction},
};

pub type Balance = u128;
pub type Nonce = u64;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct AccountInfo {
    pub balance: Balance,
    pub nonce: Nonce,
    _private: (), // prevent creation of accountinfo outside of this struct
}

impl AccountInfo {
    pub(crate) fn new() -> Self {
        Self {
            balance: 0, // Create 100 for now
            nonce: 0,
            _private: (),
        }
    }

    fn create_faucet() -> Self {
        Self {
            balance: u128::MAX,
            nonce: 0,
            _private: (),
        }
    }
}

#[derive(Debug)]
pub enum ExecError {
    InsufficientFunds {
        from: PublicKeyString,
        have: u128,
        need: u128,
    },
}

pub struct LedgerState {
    pub accounts: HashMap<PublicKeyString, AccountInfo>,
}

impl LedgerState {
    pub(crate) fn new() -> Self {
        let (pk, _) = config::retrieve_faucet_keys();
        let mut accounts: HashMap<PublicKeyString, AccountInfo> = HashMap::new();
        accounts.insert(
            PublicKeyString::from_public_key(&pk),
            AccountInfo::create_faucet(),
        );

        LedgerState { accounts }
    }

    pub(crate) fn retrieve_by_pk(&self, public_key: &PublicKeyString) -> AccountInfo {
        self.accounts.get(public_key).cloned().unwrap_or_default()
    }

    // retrieves account info by public key, creates one if one doesn't exist
    pub(crate) fn retrieve_by_pk_mut(&mut self, public_key: &PublicKeyString) -> &mut AccountInfo {
        self.accounts
            .entry(public_key.clone())
            .or_insert_with(AccountInfo::new)
    }

    pub(crate) fn apply(&mut self, transaction: &SignedTransaction) -> Result<(), ExecError> {
        match &transaction.tx {
            UnsignedTransaction::Transfer(tx) => {
                let from_info = self.retrieve_by_pk_mut(&tx.from);
                if from_info.balance < tx.amount {
                    return Err(ExecError::InsufficientFunds {
                        from: tx.from.clone(),
                        have: from_info.balance,
                        need: tx.amount,
                    });
                }
                from_info.balance -= tx.amount;

                let to_info = self.retrieve_by_pk_mut(&tx.to);
                to_info.balance += tx.amount;

                Ok(())
            }
            UnsignedTransaction::Empty => Ok(()),
        }
    }

    pub(crate) fn apply_block(&mut self, block: &Block) {
        let _ = self.apply(&block.transactions());
    }
}
