use std::collections::HashMap;

use ed25519_dalek::VerifyingKey;

use crate::config;

pub type Balance = u128;
pub type Nonce = u64;

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

pub struct LedgerState {
    pub accounts: HashMap<VerifyingKey, AccountInfo>,
    pub account_alias: HashMap<String, VerifyingKey>,
}

impl LedgerState {
    pub(crate) fn new() -> Self {
        let (pk, _) = config::retrieve_faucet_keys();
        let mut accounts: HashMap<VerifyingKey, AccountInfo> = HashMap::new();
        accounts.insert(pk.clone(), AccountInfo::create_faucet());

        let mut account_alias: HashMap<String, VerifyingKey> = HashMap::new();
        account_alias.insert("faucet".to_string(), pk);

        LedgerState {
            accounts,
            account_alias,
        }
    }

    // retrieves account info by public key, creates one if one doesn't exist
    pub(crate) fn retrieve_by_pk(&mut self, public_key: &VerifyingKey) -> &AccountInfo {
        self.accounts
            .entry(public_key.clone())
            .or_insert_with(AccountInfo::new)
    }

    pub(crate) fn retrieve_by_alias(&mut self, alias: &str) -> Option<&AccountInfo> {
        let pk = self.account_alias.get(alias)?.clone();
        Some(self.retrieve_by_pk(&pk))
    }
}
