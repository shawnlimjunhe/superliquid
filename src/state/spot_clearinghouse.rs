use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{config, state::order::OrderDirection, types::transaction::PublicKeyHash};

use super::{
    asset::AssetId,
    order::{
        ExecutionResults, LimitFillResult, MarketOrder, MarketOrderMatchingResults, Order,
        OrderChange, OrderStatus, ResidualOrder, UserExecutionResult,
    },
    spot_market::SpotMarket,
};

pub type MarketId = usize;
type MarketIdCounter = MarketId;

#[derive(Debug)]
pub struct MarketPrecision {
    pub base_lot_size: u32,  // base units per base lot
    pub quote_lot_size: u32, // quote units per quote lot
    pub tick: u32,           // quote units per tick
    pub tick_decimals: u8,
}

pub fn base_to_quote_lots(base_lots: u64, price_ticks: u64, precision: &MarketPrecision) -> u64 {
    let numerator = base_lots as u128
        * precision.base_lot_size as u128
        * price_ticks as u128
        * precision.tick as u128;

    let denominator = precision.quote_lot_size as u128 * 10u128.pow(precision.tick_decimals as u32);

    (numerator / denominator) as u64
}

pub fn quote_lots_to_base_lots(
    quote_lots: u64,
    price_ticks: u64,
    precision: &MarketPrecision,
) -> u64 {
    let numerator = quote_lots as u128
        * precision.quote_lot_size as u128
        * 10u128.pow(precision.tick_decimals as u32);
    let denominator =
        price_ticks as u128 * precision.tick as u128 * precision.base_lot_size as u128;

    (numerator / denominator) as u64
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct AccountTokenBalance {
    pub asset_id: AssetId,
    pub available_balance: u128, // not used for orders
    pub total_balance: u128,
}

impl AccountTokenBalance {
    pub fn locked_balance(&self) -> u128 {
        self.total_balance - self.available_balance
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct AccountBalance {
    pub asset_balances: Vec<AccountTokenBalance>,
}

impl AccountBalance {
    pub fn new() -> Self {
        Self {
            asset_balances: vec![],
        }
    }
}

pub struct SpotClearingHouse {
    next_id: MarketIdCounter,
    accounts: HashMap<PublicKeyHash, AccountBalance>,
    markets: Vec<SpotMarket>,
    asset_to_market_map: HashMap<(AssetId, AssetId), MarketId>,
}

impl SpotClearingHouse {
    pub fn new() -> Self {
        let clearing_house = Self {
            next_id: 0,
            accounts: HashMap::new(),
            markets: vec![],
            asset_to_market_map: HashMap::new(),
        };

        clearing_house
    }

    /// Create faucet account with max balance for token 0 and 1
    pub fn add_faucet_account(&mut self) {
        let (pk, _) = config::retrieve_faucet_keys();
        let asset_balance_one = AccountTokenBalance {
            asset_id: 0,
            available_balance: u128::MAX,
            total_balance: u128::MAX,
        };

        let asset_balance_two = AccountTokenBalance {
            asset_id: 1,
            total_balance: u128::MAX,
            available_balance: u128::MAX,
        };

        self.accounts.insert(
            pk.to_bytes(),
            AccountBalance {
                asset_balances: vec![asset_balance_one, asset_balance_two],
            },
        );
    }

    pub fn get_account_balance(&self, public_key: &PublicKeyHash) -> AccountBalance {
        self.accounts.get(public_key).cloned().unwrap_or_default()
    }

    pub fn get_account_balance_mut(&mut self, public_key: &PublicKeyHash) -> &mut AccountBalance {
        self.accounts
            .entry(*public_key)
            .or_insert_with(|| AccountBalance::new())
    }

    fn normalise_pair(asset_one: AssetId, asset_two: AssetId) -> (AssetId, AssetId) {
        if asset_one < asset_two {
            (asset_one, asset_two)
        } else {
            (asset_two, asset_one)
        }
    }

    pub fn get_market_id_from_pair(
        &self,
        asset_one: AssetId,
        asset_two: AssetId,
    ) -> Option<MarketId> {
        let pair = Self::normalise_pair(asset_one, asset_two);
        self.asset_to_market_map.get(&pair).copied()
    }

    pub fn get_quote_base_tick_from_id(
        &self,
        market_id: MarketId,
    ) -> Option<(AssetId, AssetId, u32, u8)> {
        let Some(market) = self.markets.get(market_id) else {
            return None;
        };
        Some((
            market.base_asset,
            market.base_asset,
            market.tick,
            market.tick_decimals,
        ))
    }

    fn create_new_market(
        &mut self,
        normalised_pair: (AssetId, AssetId),
        tick: u32,
        tick_decimals: u8,
    ) -> MarketId {
        let market_id = self.next_id;
        let market = SpotMarket::new(
            market_id,
            normalised_pair.0,
            normalised_pair.1,
            tick,
            tick_decimals,
        );
        self.markets.push(market);
        self.asset_to_market_map.insert(normalised_pair, market_id);
        self.next_id += 1;
        market_id
    }

    pub fn add_market(
        &mut self,
        asset_one: AssetId,
        asset_two: AssetId,
        tick: u32,
        tick_decimals: u8,
    ) -> MarketId {
        if let Some(market_id) = self.get_market_id_from_pair(asset_one, asset_two) {
            return market_id;
        }

        let normalised_pair = Self::normalise_pair(asset_one, asset_two);

        self.create_new_market(normalised_pair, tick, tick_decimals)
    }

    pub fn get_account_token_balance_mut(
        account_balance: &mut AccountBalance,
        asset_id: AssetId,
    ) -> &mut AccountTokenBalance {
        let pos = account_balance
            .asset_balances
            .iter()
            .position(|asset| asset.asset_id == asset_id);

        if let Some(index) = pos {
            return &mut account_balance.asset_balances[index];
        }

        account_balance.asset_balances.push(AccountTokenBalance {
            asset_id,
            available_balance: 0,
            total_balance: 0,
        });

        account_balance.asset_balances.last_mut().unwrap()
    }

    pub fn get_market_and_account_balance(
        &mut self,
        market_id: MarketId,
        public_key: &PublicKeyHash,
    ) -> (Option<&mut SpotMarket>, &mut AccountBalance) {
        let account_balance = self
            .accounts
            .entry(*public_key)
            .or_insert_with(|| AccountBalance::new());

        let market = self.markets.get_mut(market_id);

        return (market, account_balance);
    }

    /// Handles order matching and resultant balance transfers if any
    pub fn handle_order(
        &mut self,
        order: Order,
        precision: &MarketPrecision,
    ) -> Option<ExecutionResults> {
        let market_id = order.get_market_id().clone();

        match order {
            Order::Limit(limit_order) => {
                let (market, account_balance) =
                    self.get_market_and_account_balance(market_id, &limit_order.common.account);
                let Some(market) = market else {
                    println!("Can't find market with id");
                    return None;
                };

                // Check whether account has enough balance to place the order
                match limit_order.common.direction {
                    OrderDirection::Buy => {
                        let quote_lots = base_to_quote_lots(
                            limit_order.base_lots,
                            limit_order.price_multiple,
                            precision,
                        );

                        let quote_amount = quote_lots * precision.quote_lot_size as u64;

                        let quote_token_balance =
                            Self::get_account_token_balance_mut(account_balance, market.base_asset);

                        if quote_token_balance.available_balance < quote_amount as u128 {
                            println!("Not enough balance");
                            return None;
                        }
                        quote_token_balance.available_balance -= quote_amount as u128;
                    }
                    OrderDirection::Sell => {
                        let base_lots = limit_order.base_lots;
                        let base_token_balance =
                            Self::get_account_token_balance_mut(account_balance, market.base_asset);

                        let base_amount = base_lots * precision.base_lot_size as u64;

                        if base_token_balance.available_balance < base_amount as u128 {
                            println!("Not enough balance");
                            return None;
                        }
                        base_token_balance.available_balance -= base_amount as u128;
                    }
                }

                let base_asset = market.base_asset;
                let quote_asset = market.base_asset;
                let results =
                    market.add_limit_order(limit_order.clone(), base_asset, quote_asset, precision);

                match results {
                    Some(limit_fill_results) => {
                        // Limit order was able to execute at a better price
                        let LimitFillResult {
                            residual_order,
                            user_order,
                            filled_orders,
                        } = limit_fill_results;

                        let UserExecutionResult {
                            order_id,
                            asset_out: user_asset_out,
                            lots_out,
                            asset_in: user_asset_in,
                            lots_in,
                            filled_size,
                        } = user_order;

                        let mut out_lot_size = precision.base_lot_size;
                        let mut in_lot_size = precision.quote_lot_size;

                        if user_asset_out == quote_asset {
                            (out_lot_size, in_lot_size) = (in_lot_size, out_lot_size)
                        }
                        // Modify user's token balance
                        let asset_out_balance =
                            Self::get_account_token_balance_mut(account_balance, user_asset_out);
                        let amount_out = out_lot_size as u128 * lots_out as u128;
                        asset_out_balance.total_balance -= amount_out;

                        let asset_in_balance =
                            Self::get_account_token_balance_mut(account_balance, user_asset_in);
                        let amount_in = in_lot_size as u128 * lots_in as u128;
                        asset_in_balance.total_balance += amount_in;
                        asset_in_balance.available_balance += amount_in;

                        // counterparty is symmetric to user
                        let counterparty_asset_in = user_asset_out;
                        let counterparty_asset_out = user_asset_in;
                        let counterparty_out_size = in_lot_size;
                        let counterparty_in_size = out_lot_size;

                        // Modify filled order's token balance if any
                        for filled_order in filled_orders.iter() {
                            if filled_order.common.status == OrderStatus::Cancelled {
                                continue;
                            }
                            // Buy orders
                            let account_balance =
                                self.get_account_balance_mut(&filled_order.common.account);

                            let filled_base_lots =
                                filled_order.base_lots - filled_order.filled_base_lots;

                            let mut counterparty_asset_in_lots = filled_base_lots;
                            let mut counterparty_asset_out_lots = filled_base_lots;

                            if counterparty_asset_in == base_asset {
                                // counter pay recieves quote asset
                                counterparty_asset_out_lots = base_to_quote_lots(
                                    filled_base_lots,
                                    filled_order.price_multiple,
                                    precision,
                                );
                            } else {
                                counterparty_asset_in_lots = base_to_quote_lots(
                                    filled_base_lots,
                                    filled_order.price_multiple,
                                    precision,
                                )
                            }

                            // counter pay recieves asset_out at it's price
                            let counterparty_asset_out_balance =
                                Self::get_account_token_balance_mut(
                                    account_balance,
                                    counterparty_asset_out,
                                );

                            counterparty_asset_out_balance.total_balance -=
                                counterparty_asset_out_lots as u128 * counterparty_out_size as u128;

                            let counterparty_asset_in_balance = Self::get_account_token_balance_mut(
                                account_balance,
                                user_asset_out,
                            );

                            let amount_in =
                                counterparty_asset_in_lots as u128 * counterparty_in_size as u128;
                            counterparty_asset_in_balance.available_balance += amount_in;
                            counterparty_asset_in_balance.total_balance += amount_in;
                        }

                        // Modify partial fill's token balance if any
                        match &residual_order {
                            Some(counter_partial_fill) => {
                                // Handle partial fill
                                let ResidualOrder {
                                    account_public_key: counterparty_public_key,
                                    filled_base_lots,
                                    price_multiple: order_price,
                                    ..
                                } = counter_partial_fill;

                                let counterparty_balance =
                                    &mut self.get_account_balance(&counterparty_public_key);

                                let mut counterparty_asset_in_lots = *filled_base_lots;
                                let mut counterparty_asset_out_lots = *filled_base_lots;

                                if counterparty_asset_in == base_asset {
                                    counterparty_asset_out_lots = base_to_quote_lots(
                                        *filled_base_lots,
                                        *order_price,
                                        precision,
                                    );
                                } else {
                                    counterparty_asset_in_lots = base_to_quote_lots(
                                        *filled_base_lots,
                                        *order_price,
                                        precision,
                                    )
                                }

                                // counter pay recieves asset_out at it's price
                                let counterparty_asset_out_balance =
                                    Self::get_account_token_balance_mut(
                                        counterparty_balance,
                                        counterparty_asset_out,
                                    );

                                counterparty_asset_out_balance.total_balance -=
                                    counterparty_asset_out_lots as u128
                                        * counterparty_out_size as u128;

                                let counterparty_asset_in_balance =
                                    Self::get_account_token_balance_mut(
                                        counterparty_balance,
                                        counterparty_asset_in,
                                    );

                                let amount_in = counterparty_asset_in_lots as u128
                                    * counterparty_in_size as u128;
                                counterparty_asset_in_balance.available_balance += amount_in;
                                counterparty_asset_in_balance.total_balance += amount_in;
                            }
                            None => {
                                // No partial fills, do nothing
                            }
                        }

                        let mut average_execution_price = lots_out / lots_in;
                        if user_asset_in == base_asset {
                            average_execution_price = lots_in / lots_out; // quote / base
                        }

                        return Some(ExecutionResults {
                            filled_orders,
                            residual_order,
                            user_order_change: Some(OrderChange::LimitOrderChange {
                                order_id,
                                filled_lots: filled_size,
                                average_execution_price: average_execution_price as u128,
                            }),
                        });
                    }
                    None => {
                        // No orders filled, do nothing
                    }
                }

                return Some(ExecutionResults {
                    filled_orders: vec![],
                    residual_order: None,
                    user_order_change: None,
                });
            }
            Order::Market(market_order) => {
                let (market, account_balance) =
                    self.get_market_and_account_balance(market_id, &market_order.get_account());
                let Some(market) = market else {
                    println!("Can't find market with id");
                    return None;
                };

                // Check available balance
                match &market_order {
                    MarketOrder::Sell(sell_order) => {
                        let quote_token_balance =
                            Self::get_account_token_balance_mut(account_balance, market.base_asset);
                        if quote_token_balance.available_balance < sell_order.base_size as u128 {
                            println!("Not enough balance");
                        }
                        quote_token_balance.available_balance -= sell_order.base_size as u128;
                    }
                    MarketOrder::Buy(buy_order) => {
                        let base_token_balance =
                            Self::get_account_token_balance_mut(account_balance, market.base_asset);
                        if base_token_balance.available_balance < buy_order.quote_size as u128 {
                            println!("Not enough balance");
                        }
                        base_token_balance.available_balance -= buy_order.quote_size as u128;
                    }
                };

                let results = market.handle_market_order(market_order, precision);
                let base_asset = market.base_asset;
                let quote_asset = market.base_asset;

                // Settlement
                match results {
                    MarketOrderMatchingResults::Sell {
                        order_id,
                        base_filled_lots: quote_filled_amount,
                        quote_lots_in: base_amount_in,
                        filled_orders,
                        residual_order: counterparty_partial_fill,
                    } => {
                        let quote_token_balance =
                            Self::get_account_token_balance_mut(account_balance, quote_asset);
                        quote_token_balance.total_balance -= quote_filled_amount as u128;
                        let base_token_balance =
                            Self::get_account_token_balance_mut(account_balance, base_asset);
                        base_token_balance.total_balance += base_amount_in as u128;
                        base_token_balance.available_balance += base_amount_in as u128;

                        let average_execution_price = quote_filled_amount / base_amount_in;

                        for filled_order in filled_orders.iter() {
                            if filled_order.common.status == OrderStatus::Cancelled {
                                continue;
                            }
                            // Buy orders
                            let account_balance =
                                self.get_account_balance_mut(&filled_order.common.account);
                            let filled_quote_amount =
                                filled_order.base_lots - filled_order.filled_base_lots;

                            let base_token_balance =
                                Self::get_account_token_balance_mut(account_balance, base_asset);
                            base_token_balance.total_balance -= base_to_quote_lots(
                                filled_quote_amount,
                                filled_order.price_multiple,
                                precision,
                            )
                                as u128;

                            let quote_token_balance =
                                Self::get_account_token_balance_mut(account_balance, quote_asset);

                            quote_token_balance.available_balance += filled_quote_amount as u128;
                            quote_token_balance.total_balance += filled_quote_amount as u128;
                        }
                        match &counterparty_partial_fill {
                            Some(counter_partial_fill) => {
                                let ResidualOrder {
                                    account_public_key,
                                    filled_base_lots: filled_quote_lots,
                                    price_multiple: order_price,
                                    ..
                                } = counter_partial_fill;

                                let account_balance =
                                    &mut self.get_account_balance(&account_public_key);

                                let base_token_balance = Self::get_account_token_balance_mut(
                                    account_balance,
                                    base_asset,
                                );
                                base_token_balance.total_balance -=
                                    base_to_quote_lots(*filled_quote_lots, *order_price, precision)
                                        as u128;

                                let quote_token_balance = Self::get_account_token_balance_mut(
                                    account_balance,
                                    quote_asset,
                                );

                                quote_token_balance.available_balance += *filled_quote_lots as u128;
                                quote_token_balance.total_balance += *filled_quote_lots as u128;
                            }
                            None => {}
                        }

                        return Some(ExecutionResults {
                            filled_orders,
                            residual_order: counterparty_partial_fill,
                            user_order_change: Some(OrderChange::MarketOrderChange {
                                order_id,
                                filled_lots: quote_filled_amount,
                                average_execution_price: average_execution_price,
                            }),
                        });
                    }
                    MarketOrderMatchingResults::Buy {
                        order_id,
                        quote_filled_lots: base_filled_amount,
                        filled_orders,
                        base_lots_in: quote_amount_in,
                        residual_order: counterparty_partial_fill,
                    } => {
                        let base_token_balance =
                            Self::get_account_token_balance_mut(account_balance, market.base_asset);
                        base_token_balance.total_balance -= base_filled_amount as u128;

                        let quote_token_balance =
                            Self::get_account_token_balance_mut(account_balance, market.base_asset);
                        quote_token_balance.total_balance += quote_amount_in as u128;
                        quote_token_balance.available_balance += quote_amount_in as u128;

                        let average_execution_price = quote_amount_in / base_filled_amount;

                        for filled_order in filled_orders.iter() {
                            if filled_order.common.status == OrderStatus::Cancelled {
                                continue;
                            }
                            // Sell orders
                            let account_balance =
                                self.get_account_balance_mut(&filled_order.common.account);
                            let filled_quote_amount =
                                filled_order.base_lots - filled_order.filled_base_lots;

                            let base_token_balance =
                                Self::get_account_token_balance_mut(account_balance, base_asset);
                            let filled_base_amount = base_to_quote_lots(
                                filled_quote_amount,
                                filled_order.price_multiple,
                                precision,
                            ) as u128;
                            base_token_balance.total_balance += filled_base_amount;
                            base_token_balance.available_balance += filled_base_amount;

                            let quote_token_balance =
                                Self::get_account_token_balance_mut(account_balance, quote_asset);

                            quote_token_balance.total_balance -= filled_quote_amount as u128;
                        }
                        match &counterparty_partial_fill {
                            Some(counter_partial_fill) => {
                                let ResidualOrder {
                                    account_public_key,
                                    filled_base_lots: filled_quote_lots,
                                    price_multiple: order_price,
                                    ..
                                } = counter_partial_fill;

                                let account_balance =
                                    self.get_account_balance_mut(&account_public_key);

                                let base_token_balance = Self::get_account_token_balance_mut(
                                    account_balance,
                                    base_asset,
                                );
                                let filled_base_lots =
                                    base_to_quote_lots(*filled_quote_lots, *order_price, precision)
                                        as u128;
                                base_token_balance.total_balance += filled_base_lots;
                                base_token_balance.available_balance += filled_base_lots;

                                let quote_token_balance = Self::get_account_token_balance_mut(
                                    account_balance,
                                    quote_asset,
                                );

                                quote_token_balance.total_balance -= *filled_quote_lots as u128;
                            }
                            None => {}
                        }
                        return Some(ExecutionResults {
                            filled_orders,
                            residual_order: counterparty_partial_fill,
                            user_order_change: Some(OrderChange::MarketOrderChange {
                                order_id,
                                filled_lots: base_filled_amount,
                                average_execution_price: average_execution_price,
                            }),
                        });
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    mod test_limit_execution_side_effects {}

    mod test_market_execution_side_effects {}
}
