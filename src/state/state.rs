use std::collections::HashMap;

use ed25519_dalek::VerifyingKey;

pub type Balance = u128;
pub type Nonce = u64;

pub struct AccountInfo {
    pub balance: Balance,
    pub nonce: Nonce,
}

impl AccountInfo {
    pub(crate) fn new(&self) -> Self {
        Self {
            balance: 1000, // Create 100 for now
            nonce: 0,
        }
    }
}

pub struct LedgerState {
    pub accounts: HashMap<VerifyingKey, AccountInfo>,
    pub account_alias: HashMap<String, VerifyingKey>,
}
