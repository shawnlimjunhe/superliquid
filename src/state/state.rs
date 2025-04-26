use std::collections::HashMap;

use crate::{
    config,
    hotstuff::{
        block::Block,
        client_command::{Action, ClientCommand},
    },
    types::transaction::PublicKeyString,
};

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

    // retrieves account info by public key, creates one if one doesn't exist
    pub(crate) fn retrieve_by_pk_mut(&mut self, public_key: &PublicKeyString) -> &mut AccountInfo {
        self.accounts
            .entry(public_key.clone())
            .or_insert_with(AccountInfo::new)
    }

    pub(crate) fn apply(&mut self, action: &Action) -> Result<(), ExecError> {
        match action {
            Action::Transfer { from, to, amount } => {
                let from_info = self.retrieve_by_pk_mut(&from);
                if from_info.balance < *amount {
                    return Err(ExecError::InsufficientFunds {
                        from: from.clone(),
                        have: from_info.balance,
                        need: *amount,
                    });
                }
                from_info.balance -= amount;

                let to_info = self.retrieve_by_pk_mut(&to);
                to_info.balance += amount;

                Ok(())
            }
            Action::Empty => Ok(()),
        }
    }

    pub(crate) fn apply_block(&mut self, block: &Block) {
        let cmd: &ClientCommand = block.get_command();
        let _ = self.apply(&cmd.transactions);
    }
}
