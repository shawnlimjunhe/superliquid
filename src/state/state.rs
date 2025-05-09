use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{
    config,
    hotstuff::block::Block,
    types::transaction::{PublicKeyHash, PublicKeyString, SignedTransaction, UnsignedTransaction},
};

use super::{
    asset::AssetManager,
    order::Order,
    spot_clearinghouse::{AccountBalance, AccountTokenBalance, SpotClearingHouse},
};

pub type Balance = u128;
pub type Nonce = u64;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct AccountInfo {
    pub expected_nonce: Nonce,
    pub open_orders: Vec<Order>, // sorted by orderId
    // pub order: Vec<Order>, ignore storing order history for now
    _private: (), // prevent creation of accountinfo outside of this struct
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct AccountInfoWithBalances {
    pub account_info: AccountInfo,
    pub spot_balances: AccountBalance,
}

impl AccountInfo {
    pub(crate) fn new() -> Self {
        Self {
            expected_nonce: 0,
            open_orders: vec![],
            _private: (),
        }
    }

    fn create_faucet() -> Self {
        Self {
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

pub struct LedgerState {
    pub accounts: HashMap<PublicKeyHash, AccountInfo>,
    pub asset_manager: AssetManager,
    pub spot_clearinghouse: SpotClearingHouse,
    pub perps_clearinghouse: (),
}

impl LedgerState {
    pub(crate) fn new() -> Self {
        let (pk, _) = config::retrieve_faucet_keys();
        let mut accounts: HashMap<PublicKeyHash, AccountInfo> = HashMap::new();
        accounts.insert(pk.to_bytes(), AccountInfo::create_faucet());

        let mut spot_clearinghouse = SpotClearingHouse::new();
        spot_clearinghouse.add_faucet_account();

        LedgerState {
            accounts,
            asset_manager: AssetManager::new(),
            spot_clearinghouse: spot_clearinghouse,
            perps_clearinghouse: (),
        }
    }

    pub(crate) fn get_account_info_with_balances(
        &self,
        public_key: &PublicKeyHash,
    ) -> AccountInfoWithBalances {
        let account_info = self.get_account_info(public_key);
        let spot_balances = self.get_account_spot_balances(public_key);

        AccountInfoWithBalances {
            account_info,
            spot_balances,
        }
    }

    pub(crate) fn get_account_info(&self, public_key: &PublicKeyHash) -> AccountInfo {
        self.accounts.get(public_key).cloned().unwrap_or_default()
    }
    // retrieves account info by public key, creates one if one doesn't exist
    pub(crate) fn get_account_info_mut(&mut self, public_key: &PublicKeyHash) -> &mut AccountInfo {
        self.accounts
            .entry(*public_key)
            .or_insert_with(|| AccountInfo::new())
    }

    pub(crate) fn get_account_spot_balances_mut(
        &mut self,
        public_key: &PublicKeyHash,
    ) -> &mut AccountBalance {
        self.spot_clearinghouse.get_account_balance_mut(public_key)
    }

    pub(crate) fn get_account_spot_balances(&self, public_key: &PublicKeyHash) -> AccountBalance {
        self.spot_clearinghouse.get_account_balance(public_key)
    }

    pub(crate) fn apply(
        &mut self,
        transactions: &Vec<SignedTransaction>,
    ) -> Result<Vec<Option<(PublicKeyHash, Nonce)>>, ExecError> {
        let mut account_nonces: Vec<Option<(PublicKeyHash, Nonce)>> = vec![];

        for transaction in transactions.iter() {
            match &transaction.tx {
                UnsignedTransaction::Transfer(tx) => {
                    let new_expected_nonce = {
                        {
                            let from_account_info = self.get_account_info(&tx.from);

                            if from_account_info.expected_nonce < tx.nonce {
                                println!("Duplicate nonce");
                                return Err(ExecError::DuplicateNonce {
                                    from: PublicKeyString::from_bytes(tx.from),
                                    nonce: tx.nonce,
                                });
                            }

                            if from_account_info.expected_nonce > tx.nonce {
                                println!("Out of order nonce");
                                return Err(ExecError::OutOfOrderNonce {
                                    from: PublicKeyString::from_bytes(tx.from),
                                    nonce: tx.nonce,
                                });
                            }
                        }

                        {
                            let from_account_balances =
                                self.get_account_spot_balances_mut(&tx.from);
                            let from_token_balance_opt = from_account_balances
                                .asset_balances
                                .iter_mut()
                                .find(|a| a.asset_id == tx.asset_id);

                            let Some(from_token_balance) = from_token_balance_opt else {
                                println!("Insufficient Funds");
                                return Err(ExecError::InsufficientFunds {
                                    from: PublicKeyString::from_bytes(tx.from),
                                    have: 0,
                                    need: tx.amount,
                                });
                            };

                            if from_token_balance.available_balance < tx.amount {
                                println!("Insufficient Funds");
                                return Err(ExecError::InsufficientFunds {
                                    from: PublicKeyString::from_bytes(tx.from),
                                    have: from_token_balance.total_balance,
                                    need: tx.amount,
                                });
                            }
                            from_token_balance.available_balance -= tx.amount;
                            from_token_balance.total_balance -= tx.amount;
                        }
                        {
                            let from_account_info = self.get_account_info_mut(&tx.from);
                            from_account_info.expected_nonce += 1;
                            from_account_info.expected_nonce
                        }
                    };

                    let to_account_balances = self.get_account_spot_balances_mut(&tx.to);
                    let to_token_balance_opt = to_account_balances
                        .asset_balances
                        .iter_mut()
                        .find(|a| a.asset_id == tx.asset_id);

                    match to_token_balance_opt {
                        Some(account_balance) => account_balance.total_balance += tx.amount,
                        None => to_account_balances
                            .asset_balances
                            .push(AccountTokenBalance {
                                asset_id: tx.asset_id,
                                available_balance: tx.amount,
                                total_balance: tx.amount,
                            }),
                    }

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
