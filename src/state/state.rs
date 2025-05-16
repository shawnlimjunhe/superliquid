use serde::{Deserialize, Serialize};
use std::{collections::HashMap, vec};

use crate::{
    config,
    hotstuff::block::Block,
    types::transaction::{
        OrderTransaction, PublicKeyHash, PublicKeyString, SignedTransaction, UnsignedTransaction,
    },
};

use super::{
    asset::AssetManager,
    order::{
        self, ExecutionResults, LimitOrder, MarketOrder, Order, OrderId, OrderStateManager,
        OrderStatus, ResidualOrder,
    },
    spot_clearinghouse::{AccountBalance, AccountTokenBalance, MarketPrecision, SpotClearingHouse},
};

pub type Balance = u128;
pub type Nonce = u64;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct AccountInfo {
    pub expected_nonce: Nonce,
    pub open_orders: Vec<LimitOrder>, // sorted by orderId
    pub completed_orders: Vec<Order>, // sorted by completion
    _private: (),                     // prevent creation of accountinfo outside of this struct
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
            completed_orders: vec![],
            _private: (),
        }
    }

    fn create_faucet() -> Self {
        Self {
            expected_nonce: 0,
            open_orders: vec![],
            completed_orders: vec![],
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
    pub order_manager: OrderStateManager,
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
            order_manager: OrderStateManager::new(),
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
        self.spot_clearinghouse
            .get_account_balance_or_default(public_key)
    }

    fn get_order_position_from_open_orders(
        account_info: &mut AccountInfo,
        order_id: OrderId,
    ) -> Option<usize> {
        account_info
            .open_orders
            .iter()
            .position(|order| order.common.id == order_id)
    }

    pub(crate) fn handle_order_transaction(
        &mut self,
        transaction: OrderTransaction,
    ) -> Option<(PublicKeyHash, Nonce)> {
        let OrderTransaction {
            market_id,
            from: user_account,
            direction,
            order_size,
            order_type,
            nonce,
        } = transaction;
        // check nonce
        let from_account_info = self.get_account_info(&user_account);
        if nonce < from_account_info.expected_nonce {
            println!("Duplicate nonce");
            return None;
            // Err(ExecError::DuplicateNonce {
            //     from: PublicKeyString::from_bytes(user_account),
            //     nonce,
            // });
        }

        if nonce > from_account_info.expected_nonce {
            println!("Out of order nonce");
            return None;
            // return Err(ExecError::OutOfOrderNonce {
            //     from: PublicKeyString::from_bytes(user_account),
            //     nonce,
            // });
        }

        let order = match order_type {
            order::OrderType::Limit(price) => {
                let order = self.order_manager.new_limit_order(
                    market_id,
                    user_account,
                    direction,
                    price,
                    transaction.order_size,
                );
                let account_info = self.get_account_info_mut(&user_account);
                account_info.open_orders.push(order.clone());
                Order::Limit(order)
            }
            order::OrderType::Market => {
                let order = self.order_manager.new_market_order(
                    market_id,
                    user_account,
                    direction,
                    order_size,
                );

                Order::Market(order)
            }
        };
        let Some((quote_asset, base_asset, tick, tick_decimals)) = self
            .spot_clearinghouse
            .get_quote_base_tick_from_id(market_id)
        else {
            return None;
        };
        let Some(quote_asset) = self.asset_manager.assets.get(quote_asset as usize) else {
            return None;
        };
        let Some(base_asset) = self.asset_manager.assets.get(base_asset as usize) else {
            return None;
        };

        let precision = MarketPrecision {
            base_lot_size: base_asset.lot_size,
            quote_lot_size: quote_asset.lot_size,
            tick,
            tick_decimals: tick_decimals,
        };

        let result = self
            .spot_clearinghouse
            .handle_order(order.clone(), &precision);

        // Update changes to respective account infos
        match result {
            Some(result) => {
                let ExecutionResults {
                    filled_orders,
                    residual_order: counterparty_partial_fill,
                    user_order_change,
                } = result;

                match user_order_change {
                    Some(order_change) => match order_change {
                        order::OrderChange::LimitOrderChange {
                            order_id,
                            filled_lots: filled_amount,
                            average_execution_price: _,
                        } => {
                            let account_info = self.get_account_info_mut(&user_account);
                            let limit_order_index =
                                Self::get_order_position_from_open_orders(account_info, order_id)
                                    .expect("No open order with order_id");

                            let limit_order = account_info
                                .open_orders
                                .get_mut(limit_order_index)
                                .expect("No open order with order_id");

                            let remaining_size =
                                limit_order.base_lots - limit_order.filled_base_lots;

                            if filled_amount < remaining_size {
                                limit_order.common.status = OrderStatus::PartiallyFilled;
                            } else {
                                // fulled filled
                                limit_order.common.status = OrderStatus::Filled;
                                let limit_order =
                                    account_info.open_orders.remove(limit_order_index);
                                account_info
                                    .completed_orders
                                    .push(Order::Limit(limit_order));
                            }
                        }
                        order::OrderChange::MarketOrderChange {
                            order_id: _,
                            filled_lots,
                            average_execution_price,
                        } => match order {
                            Order::Market(MarketOrder::Buy(mut order)) => {
                                if order.quote_size < filled_lots {
                                    order.common.status = OrderStatus::PartiallyFilled;
                                }
                                order.filled_size = filled_lots;
                                order.average_execution_price = average_execution_price;

                                let account_info = self.get_account_info_mut(&order.common.account);

                                account_info
                                    .completed_orders
                                    .push(Order::Market(MarketOrder::Buy(order)));
                            }
                            _ => {}
                        },
                    },
                    None => {}
                }

                // Update filled orders
                for filled_order in filled_orders.iter() {
                    let order_account = self.get_account_info_mut(&filled_order.common.account);

                    let removed = Self::get_order_position_from_open_orders(
                        order_account,
                        filled_order.common.id,
                    )
                    .map(|i| order_account.open_orders.remove(i));

                    let Some(mut removed) = removed else {
                        continue;
                    };

                    removed.common.status = OrderStatus::Filled;
                    order_account.completed_orders.push(Order::Limit(removed));
                }

                match counterparty_partial_fill {
                    Some(partial_fill) => {
                        let ResidualOrder {
                            order_id,
                            account_public_key,
                            filled_base_lots: filled_quote_lots,
                            ..
                        } = partial_fill;

                        let order_account = self.get_account_info_mut(&account_public_key);
                        let order = order_account
                            .open_orders
                            .iter_mut()
                            .find(|order| order.common.id == order_id)
                            .expect("Cant find open order with order id");
                        order.filled_base_lots += filled_quote_lots;
                    }
                    None => {
                        // do nothing
                    }
                }
            }
            None => {
                // do nothing
            }
        }
        let account = self.get_account_info_mut(&user_account);
        account.expected_nonce += 1;
        return Some((user_account, account.expected_nonce));
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

                            if tx.nonce < from_account_info.expected_nonce {
                                println!("Duplicate nonce");
                                return Err(ExecError::DuplicateNonce {
                                    from: PublicKeyString::from_bytes(tx.from),
                                    nonce: tx.nonce,
                                });
                            }

                            if tx.nonce > from_account_info.expected_nonce {
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
                UnsignedTransaction::Order(order_transaction) => {
                    account_nonces.push(self.handle_order_transaction(order_transaction.clone()))
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
