use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{
    config,
    hotstuff::block::Block,
    types::transaction::{PublicKeyHash, PublicKeyString, SignedTransaction, UnsignedTransaction},
};

use super::{
    order::Order,
    spot_market::{MarketId, SpotMarket},
};

pub type Balance = u128;
pub type Nonce = u64;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct AccountInfo {
    pub balance: Balance,
    pub expected_nonce: Nonce,
    pub open_orders: Vec<Order>, // sorted by orderId
    // pub order: Vec<Order>, ignore storing order history for now
    _private: (), // prevent creation of accountinfo outside of this struct
}

impl AccountInfo {
    pub(crate) fn new() -> Self {
        Self {
            balance: 0, // Create 100 for now
            expected_nonce: 0,
            open_orders: vec![],
            _private: (),
        }
    }

    fn create_faucet() -> Self {
        Self {
            balance: u128::MAX,
            expected_nonce: 0,
            open_orders: vec![],
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
    DuplicateNonce {
        from: PublicKeyString,
        nonce: Nonce,
    },
    OutOfOrderNonce {
        from: PublicKeyString,
        nonce: Nonce,
    },
}

pub type asset_id = u32;

pub struct LedgerState {
    pub accounts: HashMap<PublicKeyHash, AccountInfo>,
    pub asset_name_map: HashMap<asset_id, String>,
    pub spot_clearinghouse: (),
    pub perps_clearinghouse: (),
}

impl LedgerState {
    pub(crate) fn new() -> Self {
        let (pk, _) = config::retrieve_faucet_keys();
        let mut accounts: HashMap<PublicKeyHash, AccountInfo> = HashMap::new();
        accounts.insert(pk.to_bytes(), AccountInfo::create_faucet());

        LedgerState {
            accounts,
            asset_name_map: HashMap::new(),
            spot_clearinghouse: (),
            perps_clearinghouse: (),
        }
    }

    pub(crate) fn retrieve_by_pk(&self, public_key: &PublicKeyHash) -> AccountInfo {
        self.accounts.get(public_key).cloned().unwrap_or_default()
    }

    // retrieves account info by public key, creates one if one doesn't exist
    pub(crate) fn retrieve_by_pk_mut(&mut self, public_key: &PublicKeyHash) -> &mut AccountInfo {
        self.accounts
            .entry(*public_key)
            .or_insert_with(AccountInfo::new)
    }

    pub(crate) fn apply(
        &mut self,
        transactions: &Vec<SignedTransaction>,
    ) -> Result<Vec<Option<(PublicKeyHash, Nonce)>>, ExecError> {
        let mut account_nonces: Vec<Option<(PublicKeyHash, Nonce)>> = vec![];

        for transaction in transactions.iter() {
            match &transaction.tx {
                UnsignedTransaction::Transfer(tx) => {
                    let from_info = self.retrieve_by_pk_mut(&tx.from);
                    if from_info.balance < tx.amount {
                        println!("Insufficient Funds");
                        return Err(ExecError::InsufficientFunds {
                            from: PublicKeyString::from_bytes(tx.from),
                            have: from_info.balance,
                            need: tx.amount,
                        });
                    }

                    if from_info.expected_nonce < tx.nonce {
                        println!("Duplicate nonce");
                        return Err(ExecError::DuplicateNonce {
                            from: PublicKeyString::from_bytes(tx.from),
                            nonce: tx.nonce,
                        });
                    }

                    if from_info.expected_nonce > tx.nonce {
                        println!("Out of order nonce");
                        return Err(ExecError::OutOfOrderNonce {
                            from: PublicKeyString::from_bytes(tx.from),
                            nonce: tx.nonce,
                        });
                    }

                    from_info.balance -= tx.amount;
                    from_info.expected_nonce += 1;
                    let new_expected_nonce = from_info.expected_nonce;

                    let to_info = self.retrieve_by_pk_mut(&tx.to);
                    to_info.balance += tx.amount;

                    account_nonces.push(Some((tx.from, new_expected_nonce)));
                }
            }
        }
        return Ok(account_nonces);
    }

    pub(crate) fn apply_block(&mut self, block: &Block) -> Vec<Option<(PublicKeyHash, Nonce)>> {
        match self.apply(&block.transactions()) {
            Ok(v) => return v,
            Err(_) => return vec![],
        }
    }
}
