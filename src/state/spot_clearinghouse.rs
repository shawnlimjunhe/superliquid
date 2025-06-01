use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{config, state::order::OrderDirection, types::transaction::PublicKeyHash};

use super::{
    asset::AssetId,
    order::{
        ExecutionResults, LimitFillResult, LimitOrder, MarketOrder, MarketOrderMatchingResults,
        Order, OrderChange, OrderStatus, ResidualOrder, UserExecutionResult,
    },
    spot_market::{CancelOrderIndexes, MarketInfo, SpotMarket},
    state::{ExecError, Resource},
    transaction_delta::SpotCancelOrderDelta,
};

pub type MarketId = usize;
type MarketIdCounter = MarketId;

#[derive(Debug, Clone)]
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

    pub fn find_asset_id(&self, asset_id: AssetId) -> Option<&AccountTokenBalance> {
        self.asset_balances.iter().find(|b| b.asset_id == asset_id)
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

    pub fn get_account_balance_or_default(&self, public_key: &PublicKeyHash) -> AccountBalance {
        self.accounts.get(public_key).cloned().unwrap_or_default()
    }

    pub fn get_account_balance(&self, public_key: &PublicKeyHash) -> Option<&AccountBalance> {
        self.accounts.get(public_key)
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
        base_asset: AssetId,
        quote_asset: AssetId,
    ) -> Option<MarketId> {
        let pair = Self::normalise_pair(base_asset, quote_asset);
        self.asset_to_market_map.get(&pair).copied()
    }

    pub fn get_market_info_from_id(&self, market_id: MarketId) -> Option<MarketInfo> {
        let market = self.markets.get(market_id);
        let Some(market) = market else {
            return None;
        };
        Some(market.get_market_info())
    }

    pub fn get_markets(&self) -> Vec<MarketInfo> {
        self.markets
            .iter()
            .map(|market| market.get_market_info())
            .collect()
    }

    pub fn get_market(&self, id: MarketId) -> Option<&SpotMarket> {
        self.markets.get(id)
    }

    pub fn get_quote_base_tick_from_id(
        &self,
        market_id: MarketId,
    ) -> Option<(AssetId, AssetId, u32, u8)> {
        let Some(market) = self.markets.get(market_id) else {
            return None;
        };
        Some((
            market.quote_asset,
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
        base_asset: AssetId,
        quote_asset: AssetId,
        base_asset_name: String,
        quote_asset_name: String,
    ) -> MarketId {
        let market_id = self.next_id;

        let market = SpotMarket::new(
            market_id,
            normalised_pair,
            base_asset,
            quote_asset,
            base_asset_name,
            quote_asset_name,
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
        base_asset: AssetId,
        quote_asset: AssetId,
        base_asset_name: String,
        quote_asset_name: String,
        tick: u32,
        tick_decimals: u8,
    ) -> MarketId {
        if let Some(market_id) = self.get_market_id_from_pair(base_asset, quote_asset) {
            return market_id;
        }

        let normalised_pair = Self::normalise_pair(base_asset, quote_asset);

        self.create_new_market(
            normalised_pair,
            tick,
            tick_decimals,
            base_asset,
            quote_asset,
            base_asset_name,
            quote_asset_name,
        )
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

    pub fn find_cancel_order(&self, order: &LimitOrder) -> Result<CancelOrderIndexes, ExecError> {
        let market_id = order.common.market_id;
        let market = match self.get_market(market_id) {
            Some(market) => market,
            None => return Err(ExecError::ResourceNotFound(Resource::Market(market_id))),
        };

        market.find_cancel_order(order)
    }

    pub fn commit_cancel_order(
        &mut self,
        order: &LimitOrder,
        precision: MarketPrecision,
        delta: SpotCancelOrderDelta,
    ) {
        let market_id = order.common.market_id;
        let SpotCancelOrderDelta {
            initiator,
            order_level_index,
            order_index,
            ..
        } = delta;

        let market = &mut self.markets[market_id];
        let quote_asset = market.quote_asset;
        let base_asset = market.base_asset;

        let unfilled_base_lots =
            market.commit_cancel_order_to_orderbook(order_level_index, order_index, order);

        let account_balance = &mut self.get_account_balance_mut(&initiator);

        let price = order.price_multiple;

        match order.common.direction {
            OrderDirection::Buy => {
                let quote_token_balance =
                    Self::get_account_token_balance_mut(account_balance, quote_asset);

                let quote_lots = base_to_quote_lots(unfilled_base_lots, price, &precision);
                let quote_amount = quote_lots as u128 * precision.quote_lot_size as u128;
                quote_token_balance.available_balance += quote_amount;
            }
            OrderDirection::Sell => {
                let base_token_balance =
                    Self::get_account_token_balance_mut(account_balance, base_asset);

                let base_amount = unfilled_base_lots as u128 * precision.base_lot_size as u128;
                base_token_balance.available_balance += base_amount;
            }
        }
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

                let mut is_buy = true;

                let expected_balance_lock;
                // Check whether account has enough balance to place the order
                match limit_order.common.direction {
                    OrderDirection::Buy => {
                        let quote_lots = base_to_quote_lots(
                            limit_order.base_lots,
                            limit_order.price_multiple,
                            precision,
                        );

                        let quote_amount = quote_lots * precision.quote_lot_size as u64;

                        let quote_token_balance = Self::get_account_token_balance_mut(
                            account_balance,
                            market.quote_asset,
                        );

                        expected_balance_lock = quote_amount;
                        let available = quote_token_balance.available_balance;
                        if available < quote_amount as u128 {
                            println!(
                                "Not enough balance, available: {}, needed: {}",
                                available, quote_amount
                            );
                            return None;
                        }
                        quote_token_balance.available_balance -= quote_amount as u128;
                    }
                    OrderDirection::Sell => {
                        is_buy = false;
                        let base_lots = limit_order.base_lots;
                        let base_token_balance =
                            Self::get_account_token_balance_mut(account_balance, market.base_asset);

                        let base_amount = base_lots * precision.base_lot_size as u64;
                        expected_balance_lock = base_amount;

                        let available = base_token_balance.available_balance;
                        if available < base_amount as u128 {
                            println!(
                                "Not enough balance, available: {}, needed: {}",
                                available, base_amount
                            );
                            return None;
                        }
                        base_token_balance.available_balance -= base_amount as u128;
                    }
                }

                let base_asset = market.base_asset;
                let quote_asset = market.quote_asset;
                let results =
                    market.add_limit_order(limit_order.clone(), base_asset, quote_asset, precision);

                match results {
                    Some(limit_fill_results) => {
                        // Limit order was able to execute at a better price
                        let LimitFillResult {
                            residual_order,
                            user_order,
                            filled_orders,
                            last_executed_price: _,
                        } = limit_fill_results;

                        let UserExecutionResult {
                            order_id,
                            asset_out: user_asset_out,
                            lots_out,
                            asset_in: user_asset_in,
                            lots_in,
                            filled_size,
                        } = user_order;

                        let out_lot_size;
                        let in_lot_size;

                        if is_buy {
                            out_lot_size = precision.quote_lot_size;
                            in_lot_size = precision.base_lot_size;
                        } else {
                            out_lot_size = precision.base_lot_size;
                            in_lot_size = precision.quote_lot_size;
                        }

                        // Modify user's token balance
                        let asset_out_balance =
                            Self::get_account_token_balance_mut(account_balance, user_asset_out);
                        let amount_out = out_lot_size as u128 * lots_out as u128;

                        // Unlock the balance to handle cases where we fill at a better price
                        asset_out_balance.available_balance += expected_balance_lock as u128;

                        asset_out_balance.total_balance -= amount_out;
                        asset_out_balance.available_balance -= amount_out;

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

                            let counterparty_asset_in_lots;
                            let counterparty_asset_out_lots;

                            if is_buy {
                                counterparty_asset_in_lots = base_to_quote_lots(
                                    filled_base_lots,
                                    filled_order.price_multiple,
                                    precision,
                                );
                                counterparty_asset_out_lots = filled_base_lots;
                            } else {
                                counterparty_asset_in_lots = filled_base_lots;
                                counterparty_asset_out_lots = base_to_quote_lots(
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

                            let amount_out =
                                counterparty_asset_out_lots as u128 * counterparty_out_size as u128;
                            counterparty_asset_out_balance.total_balance -= amount_out;

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
                                    self.get_account_balance_mut(&counterparty_public_key);

                                let counterparty_asset_in_lots;
                                let counterparty_asset_out_lots;

                                if is_buy {
                                    counterparty_asset_in_lots = base_to_quote_lots(
                                        *filled_base_lots,
                                        *order_price,
                                        precision,
                                    );
                                    counterparty_asset_out_lots = *filled_base_lots;
                                } else {
                                    counterparty_asset_in_lots = *filled_base_lots;
                                    counterparty_asset_out_lots = base_to_quote_lots(
                                        *filled_base_lots,
                                        *order_price,
                                        precision,
                                    )
                                }

                                // counter pay recieves asset_out at it's price
                                let asset_out_balance = Self::get_account_token_balance_mut(
                                    counterparty_balance,
                                    counterparty_asset_out,
                                );

                                let amount_out = counterparty_asset_out_lots as u128
                                    * counterparty_out_size as u128;

                                asset_out_balance.total_balance -= amount_out;

                                let asset_in_balance = Self::get_account_token_balance_mut(
                                    counterparty_balance,
                                    counterparty_asset_in,
                                );

                                let amount_in = counterparty_asset_in_lots as u128
                                    * counterparty_in_size as u128;
                                asset_in_balance.available_balance += amount_in;
                                asset_in_balance.total_balance += amount_in;
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

                let lock_amount;
                // Check if available amount means order requirements
                match &market_order {
                    MarketOrder::Sell(sell_order) => {
                        let base_token_balance =
                            Self::get_account_token_balance_mut(account_balance, market.base_asset);

                        let base_needed =
                            sell_order.base_size as u128 * precision.base_lot_size as u128;

                        if base_token_balance.available_balance < base_needed {
                            println!("Not enough balance");
                            return None;
                        }

                        base_token_balance.available_balance -= base_needed;
                        lock_amount = base_needed;
                    }
                    MarketOrder::Buy(buy_order) => {
                        let quote_token_balance = Self::get_account_token_balance_mut(
                            account_balance,
                            market.quote_asset,
                        );

                        let quote_needed =
                            buy_order.quote_size as u128 * precision.quote_lot_size as u128;
                        if quote_token_balance.available_balance < quote_needed {
                            println!("Not enough balance");
                        }
                        quote_token_balance.available_balance -= quote_needed;
                        lock_amount = quote_needed;
                    }
                };

                let results = market.handle_market_order(market_order, precision);
                let base_asset = market.base_asset;
                let quote_asset = market.quote_asset;

                // Settlement
                match results {
                    MarketOrderMatchingResults::Sell {
                        order_id,
                        base_filled_lots,
                        quote_lots_in,
                        filled_orders,
                        residual_order,
                        self_fill,
                        last_executed_price: _,
                    } => {
                        // Handle user balance change
                        let base_token_balance =
                            Self::get_account_token_balance_mut(account_balance, base_asset);

                        // unlock the balance to handle partial order fill
                        base_token_balance.available_balance += lock_amount;

                        let base_filled_amount =
                            base_filled_lots as u128 * precision.base_lot_size as u128;
                        base_token_balance.total_balance -= base_filled_amount;
                        base_token_balance.available_balance -= base_filled_amount;

                        let quote_token_balance =
                            Self::get_account_token_balance_mut(account_balance, quote_asset);

                        let quote_amount_in =
                            quote_lots_in as u128 * precision.quote_lot_size as u128;
                        quote_token_balance.total_balance += quote_amount_in;
                        quote_token_balance.available_balance += quote_amount_in;

                        let average_execution_price = {
                            if base_filled_lots > 0 {
                                quote_lots_in / base_filled_lots
                            } else {
                                0
                            }
                        };
                        for filled_order in filled_orders.iter() {
                            if filled_order.common.status == OrderStatus::Cancelled {
                                continue;
                            }
                            // Buy orders
                            let account_balance =
                                self.get_account_balance_mut(&filled_order.common.account);
                            let filled_base_lots =
                                filled_order.base_lots - filled_order.filled_base_lots;

                            let base_amount_in =
                                filled_base_lots as u128 * precision.base_lot_size as u128;

                            let base_token_balance =
                                Self::get_account_token_balance_mut(account_balance, base_asset);

                            base_token_balance.total_balance += base_amount_in;
                            base_token_balance.available_balance += base_amount_in;

                            let quote_lots_out = base_to_quote_lots(
                                filled_base_lots,
                                filled_order.price_multiple,
                                precision,
                            ) as u128;

                            let quote_amount_out =
                                quote_lots_out as u128 * precision.quote_lot_size as u128;
                            let quote_token_balance =
                                Self::get_account_token_balance_mut(account_balance, quote_asset);

                            quote_token_balance.total_balance -= quote_amount_out;
                        }
                        match &residual_order {
                            Some(counter_partial_fill) => {
                                let ResidualOrder {
                                    account_public_key,
                                    filled_base_lots,
                                    price_multiple: order_price,
                                    ..
                                } = counter_partial_fill;

                                let account_balance =
                                    self.get_account_balance_mut(&account_public_key);

                                let base_token_balance = Self::get_account_token_balance_mut(
                                    account_balance,
                                    base_asset,
                                );

                                let base_amount =
                                    *filled_base_lots as u128 * precision.base_lot_size as u128;
                                base_token_balance.total_balance += base_amount;
                                base_token_balance.available_balance += base_amount;

                                let quote_lots: u128 =
                                    base_to_quote_lots(*filled_base_lots, *order_price, precision)
                                        as u128;
                                let quote_amount = quote_lots * precision.quote_lot_size as u128;

                                let quote_token_balance = Self::get_account_token_balance_mut(
                                    account_balance,
                                    quote_asset,
                                );

                                quote_token_balance.total_balance -= quote_amount;
                            }
                            None => {}
                        }

                        return Some(ExecutionResults {
                            filled_orders,
                            residual_order,
                            user_order_change: Some(OrderChange::MarketOrderChange {
                                order_id,
                                filled_lots: base_filled_lots,
                                self_fill,
                                average_execution_price: average_execution_price,
                            }),
                        });
                    }
                    MarketOrderMatchingResults::Buy {
                        order_id,
                        quote_filled_lots,
                        filled_orders,
                        base_lots_in,
                        residual_order,
                        self_fill,
                        last_executed_price: _,
                    } => {
                        let quote_token_balance = Self::get_account_token_balance_mut(
                            account_balance,
                            market.quote_asset,
                        );

                        // unlock the balance to handle partial order fill
                        quote_token_balance.available_balance += lock_amount;

                        let quote_amount: u128 =
                            quote_filled_lots as u128 * precision.quote_lot_size as u128;

                        quote_token_balance.total_balance -= quote_amount;
                        quote_token_balance.available_balance -= quote_amount;

                        let base_token_balance =
                            Self::get_account_token_balance_mut(account_balance, market.base_asset);

                        let base_amount = base_lots_in as u128 * precision.base_lot_size as u128;
                        base_token_balance.total_balance += base_amount;
                        base_token_balance.available_balance += base_amount;

                        let average_execution_price = {
                            if base_lots_in > 0 {
                                quote_filled_lots / base_lots_in
                            } else {
                                0
                            }
                        };

                        for filled_order in filled_orders.iter() {
                            if filled_order.common.status == OrderStatus::Cancelled {
                                continue;
                            }
                            // Sell orders
                            let account_balance =
                                self.get_account_balance_mut(&filled_order.common.account);
                            let filled_base_lots =
                                filled_order.base_lots - filled_order.filled_base_lots;
                            let base_amount =
                                filled_base_lots as u128 * precision.base_lot_size as u128;

                            let base_token_balance =
                                Self::get_account_token_balance_mut(account_balance, base_asset);

                            base_token_balance.total_balance -= base_amount;
                            let filled_quote_lots = base_to_quote_lots(
                                filled_base_lots,
                                filled_order.price_multiple,
                                precision,
                            ) as u128;

                            let quote_amount: u128 =
                                filled_quote_lots * precision.quote_lot_size as u128;

                            let quote_token_balance =
                                Self::get_account_token_balance_mut(account_balance, quote_asset);

                            quote_token_balance.total_balance += quote_amount;
                            quote_token_balance.available_balance += quote_amount;
                        }
                        match &residual_order {
                            Some(counter_partial_fill) => {
                                let ResidualOrder {
                                    account_public_key,
                                    filled_base_lots,
                                    price_multiple: order_price,
                                    ..
                                } = counter_partial_fill;

                                let account_balance =
                                    self.get_account_balance_mut(&account_public_key);

                                let base_amount =
                                    *filled_base_lots as u128 * precision.base_lot_size as u128;

                                let base_token_balance = Self::get_account_token_balance_mut(
                                    account_balance,
                                    base_asset,
                                );

                                base_token_balance.total_balance -= base_amount;

                                let quote_out_lots =
                                    base_to_quote_lots(*filled_base_lots, *order_price, precision)
                                        as u128;
                                let quote_amount =
                                    quote_out_lots * precision.quote_lot_size as u128;

                                let quote_token_balance = Self::get_account_token_balance_mut(
                                    account_balance,
                                    quote_asset,
                                );

                                quote_token_balance.total_balance += quote_amount;

                                quote_token_balance.available_balance += quote_amount;
                            }
                            None => {}
                        }
                        return Some(ExecutionResults {
                            filled_orders,
                            residual_order,
                            user_order_change: Some(OrderChange::MarketOrderChange {
                                order_id,
                                filled_lots: quote_filled_lots,
                                self_fill,
                                average_execution_price,
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

    use crate::{
        state::{
            order::{
                CommonOrderFields, LimitOrder, MarketBuyOrder, MarketOrder, MarketSellOrder, Order,
                OrderDirection, OrderId, OrderStatus,
            },
            transaction_delta::SpotCancelOrderDelta,
        },
        types::transaction::PublicKeyHash,
    };

    use super::{MarketPrecision, SpotClearingHouse};

    pub fn new_limit(
        price_tick: u64,
        lot_size: u64,
        direction: OrderDirection,
        id: OrderId,
        account: PublicKeyHash,
    ) -> LimitOrder {
        LimitOrder {
            price_multiple: price_tick,
            base_lots: lot_size,
            filled_base_lots: 0,
            self_filled: 0,
            common: CommonOrderFields {
                id,
                market_id: 0,
                status: OrderStatus::Open,
                account,
                direction,
            },
        }
    }

    fn cancel_order(
        clearing_house: &mut SpotClearingHouse,
        order: &LimitOrder,
        precision: MarketPrecision,
    ) {
        let indexes = clearing_house.find_cancel_order(order).unwrap();
        let delta = SpotCancelOrderDelta {
            initiator: order.common.account,
            account_order_position: 0, // not needed
            order_level_index: indexes.0,
            order_index: indexes.1,
        };
        clearing_house.commit_cancel_order(order, precision, delta);
    }

    fn new_market_buy(id: OrderId, quote_size: u64, account: PublicKeyHash) -> MarketOrder {
        MarketOrder::Buy(MarketBuyOrder {
            quote_size,
            filled_size: 0,
            average_execution_price: 0,
            self_filled: 0,
            common: CommonOrderFields {
                id,
                market_id: 0,
                status: OrderStatus::Open,
                account,
                direction: OrderDirection::Buy,
            },
        })
    }

    fn new_market_sell(id: OrderId, base_size: u64, account: PublicKeyHash) -> MarketOrder {
        MarketOrder::Sell(MarketSellOrder {
            base_size,
            filled_size: 0,
            average_execution_price: 0,
            self_filled: 0,
            common: CommonOrderFields {
                id,
                market_id: 0,
                status: OrderStatus::Open,
                account,
                direction: OrderDirection::Sell,
            },
        })
    }

    fn test_setup(
        user_public_key: PublicKeyHash,
        maker_one_public_key: PublicKeyHash,
        maker_two_public_key: PublicKeyHash,
    ) -> (SpotClearingHouse, MarketPrecision) {
        let mut spot_clearinghouse = SpotClearingHouse::new();
        spot_clearinghouse.add_faucet_account();

        let base_asset = 0;
        let base_asset_name = "".to_string();
        let quote_asset_name = "".to_string();
        let quote_asset = 1;
        let tick = 100;
        let tick_decimals = 2;
        let precision = super::MarketPrecision {
            base_lot_size: 100,
            quote_lot_size: 100,
            tick: tick,
            tick_decimals: tick_decimals,
        };

        spot_clearinghouse.add_market(
            base_asset,
            quote_asset,
            base_asset_name,
            quote_asset_name,
            tick,
            tick_decimals,
        );

        // Fund accounts
        {
            // Fund user accounts
            let user_account = spot_clearinghouse.get_account_balance_mut(&user_public_key);
            let base_token_balance =
                SpotClearingHouse::get_account_token_balance_mut(user_account, base_asset);
            base_token_balance.available_balance += 1_000_000_000;
            base_token_balance.total_balance += 1_000_000_000;

            let quote_token_balance =
                SpotClearingHouse::get_account_token_balance_mut(user_account, quote_asset);
            quote_token_balance.available_balance += 1_000_000_000_000;
            quote_token_balance.total_balance += 1_000_000_000_000;
        }

        {
            // Fund market maker one's account
            let maker_account = spot_clearinghouse.get_account_balance_mut(&maker_one_public_key);
            let base_token_balance =
                SpotClearingHouse::get_account_token_balance_mut(maker_account, base_asset);
            base_token_balance.available_balance += 1_000_000_000;
            base_token_balance.total_balance += 1_000_000_000;

            let quote_token_balance =
                SpotClearingHouse::get_account_token_balance_mut(maker_account, quote_asset);
            quote_token_balance.available_balance += 1_000_000_000_000;
            quote_token_balance.total_balance += 1_000_000_000_000;
        }

        {
            // Fund market maker two's account
            let maker_account = spot_clearinghouse.get_account_balance_mut(&maker_two_public_key);
            let base_token_balance =
                SpotClearingHouse::get_account_token_balance_mut(maker_account, base_asset);
            base_token_balance.available_balance += 1_000_000_000;
            base_token_balance.total_balance += 1_000_000_000;

            let quote_token_balance =
                SpotClearingHouse::get_account_token_balance_mut(maker_account, quote_asset);
            quote_token_balance.available_balance += 1_000_000_000_000;
            quote_token_balance.total_balance += 1_000_000_000_000;
        }

        // Buys
        let buy_1 = new_limit(2_200, 700, OrderDirection::Buy, 1, maker_one_public_key);
        let buy_2 = new_limit(2_300, 700, OrderDirection::Buy, 2, maker_two_public_key);
        let buy_3 = new_limit(2_300, 400, OrderDirection::Buy, 3, maker_two_public_key);
        let buy_5 = new_limit(2_300, 700, OrderDirection::Buy, 10, maker_two_public_key);
        let buy_4 = new_limit(2_450, 1_000, OrderDirection::Buy, 4, maker_one_public_key);

        // Sells
        let sell_1 = new_limit(2_500, 600, OrderDirection::Sell, 5, maker_one_public_key);
        let sell_2 = new_limit(2_500, 1_000, OrderDirection::Sell, 6, maker_two_public_key);
        let sell_3 = new_limit(2_600, 1_200, OrderDirection::Sell, 7, maker_two_public_key);
        let sell_4 = new_limit(2_700, 700, OrderDirection::Sell, 8, maker_one_public_key);
        let sell_5 = new_limit(2_800, 300, OrderDirection::Sell, 9, maker_one_public_key);
        let sell_6 = new_limit(2_500, 300, OrderDirection::Sell, 10, maker_one_public_key);

        spot_clearinghouse.handle_order(Order::Limit(buy_1), &precision);
        spot_clearinghouse.handle_order(Order::Limit(sell_1), &precision);
        spot_clearinghouse.handle_order(Order::Limit(sell_2), &precision);
        spot_clearinghouse.handle_order(Order::Limit(buy_2), &precision);
        spot_clearinghouse.handle_order(Order::Limit(buy_3), &precision);
        spot_clearinghouse.handle_order(Order::Limit(buy_4), &precision);
        spot_clearinghouse.handle_order(Order::Limit(sell_3), &precision);
        spot_clearinghouse.handle_order(Order::Limit(sell_5), &precision);
        spot_clearinghouse.handle_order(Order::Limit(sell_4), &precision);

        spot_clearinghouse.handle_order(Order::Limit(buy_5.clone()), &precision);
        cancel_order(&mut spot_clearinghouse, &buy_5, precision.clone());

        spot_clearinghouse.handle_order(Order::Limit(sell_6.clone()), &precision);
        cancel_order(&mut spot_clearinghouse, &sell_6, precision.clone());

        let market = spot_clearinghouse.markets.get(0).unwrap();

        {
            // Check market state
            assert_eq!(market.asks_levels.len(), 4);
            assert_eq!(market.bids_levels.len(), 3);
            assert_eq!(market.get_best_prices(), (Some(2450), Some(2500)));

            // Check user state
            let user_balance = spot_clearinghouse.get_account_balance_or_default(&user_public_key);
            let user_base_balance = user_balance.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first
            let user_quote_balance = user_balance.asset_balances.get(1).unwrap();
            assert_eq!(user_base_balance.total_balance, 1_000_000_000);
            assert_eq!(user_base_balance.available_balance, 1_000_000_000);
            assert_eq!(user_quote_balance.total_balance, 1_000_000_000_000);
            assert_eq!(user_quote_balance.available_balance, 1_000_000_000_000);

            // Check account state
            let mm_one = spot_clearinghouse.get_account_balance_or_default(&maker_one_public_key);
            let mm_one_base_balance = mm_one.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first
            let mm_one_quote_balance = mm_one.asset_balances.get(1).unwrap();
            assert_eq!(mm_one_base_balance.total_balance, 1_000_000_000);
            assert_eq!(
                mm_one_base_balance.available_balance,
                (1_000_000_000 - ((600 + 700 + 300) * precision.base_lot_size)) as u128
            );
            assert_eq!(mm_one_quote_balance.total_balance, 1_000_000_000_000);
            assert_eq!(
                mm_one_quote_balance.available_balance,
                (1_000_000_000_000u128
                    - (2_200 * 700 + 2_450 * 1_000) * precision.quote_lot_size as u128)
            );

            let mm_two = spot_clearinghouse.get_account_balance_or_default(&maker_two_public_key);
            let mm_two_base_balance = mm_two.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first
            let mm_two_quote_balance = mm_two.asset_balances.get(1).unwrap();
            assert_eq!(mm_two_base_balance.total_balance, 1_000_000_000);
            assert_eq!(
                mm_two_base_balance.available_balance,
                (1_000_000_000 - (1_000 + 1_200) * precision.base_lot_size) as u128
            );
            assert_eq!(mm_two_quote_balance.total_balance, 1_000_000_000_000);
            assert_eq!(
                mm_two_quote_balance.available_balance,
                1_000_000_000_000u128 - (2_300 * 1_100) * precision.quote_lot_size as u128
            );
        }

        (spot_clearinghouse, precision)
    }
    mod test_limit_execution_side_effects {
        use crate::state::order::{Order, OrderDirection};

        use super::{new_limit, test_setup};

        #[test]
        fn test_limit_buy_with_residual_order() {
            let user_public_key = [0; 32];
            let maker_one_public_key = [1; 32];
            let maker_two_public_key = [2; 32];
            let (mut spot_clearinghouse, precision) =
                test_setup(user_public_key, maker_one_public_key, maker_two_public_key);

            let buy_5 = new_limit(2_600, 1_800, OrderDirection::Buy, 10, user_public_key);
            spot_clearinghouse.handle_order(Order::Limit(buy_5), &precision);

            // asset user state
            {
                // Check account state
                let user_balance =
                    spot_clearinghouse.get_account_balance_or_default(&user_public_key);
                let user_base_balance = user_balance.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first
                let user_quote_balance = user_balance.asset_balances.get(1).unwrap();

                let expected_base_increase = (1_800 * precision.base_lot_size) as u128;
                assert_eq!(
                    user_base_balance.total_balance,
                    1_000_000_000 + expected_base_increase
                );
                assert_eq!(
                    user_base_balance.available_balance,
                    1_000_000_000 + expected_base_increase
                );

                let expected_quote_decrease =
                    (2_500 * 1_600 + 2_600 * 200) * precision.quote_lot_size as u128;
                assert_eq!(
                    user_quote_balance.total_balance,
                    1_000_000_000_000 - expected_quote_decrease
                );
                assert_eq!(
                    user_quote_balance.available_balance,
                    1_000_000_000_000u128 - expected_quote_decrease
                ); // Quote balance should decrease
            }
            // Check maker state
            {
                // MM 1
                // Asset base balance
                let mm_one =
                    spot_clearinghouse.get_account_balance_or_default(&maker_one_public_key);
                let base_balance = mm_one.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first

                let initial_balance = 1_000_000_000;
                let initial_available =
                    (1_000_000_000 - ((600 + 700 + 300) * precision.base_lot_size)) as u128;

                let expected_base_decrease = (600 * precision.base_lot_size) as u128;

                assert_eq!(
                    base_balance.total_balance,
                    initial_balance - expected_base_decrease
                );
                assert_eq!(base_balance.available_balance, initial_available);

                // Quote balance
                let quote_balance = mm_one.asset_balances.get(1).unwrap();
                let initial_balance = 1_000_000_000_000;
                let initial_available = 1_000_000_000_000u128
                    - (2_200 * 700 + 2_450 * 1_000) * precision.quote_lot_size as u128;

                let expected_quote_increase = (600 * 2500 * precision.quote_lot_size) as u128;

                assert_eq!(
                    quote_balance.total_balance,
                    initial_balance + expected_quote_increase
                );
                assert_eq!(
                    quote_balance.available_balance,
                    initial_available + expected_quote_increase
                );
            }

            {
                // MM 2
                // Assert base balance
                let mm_two = spot_clearinghouse
                    .get_account_balance(&maker_two_public_key)
                    .unwrap();
                let mm_two_base_balance = mm_two.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first

                let initial_total = 1_000_000_000;
                let initial_avail =
                    (1_000_000_000 - (1_000 + 1_200) * precision.base_lot_size) as u128;

                let expected_base_decrease = (1_000 + 200) * precision.base_lot_size as u128;

                assert_eq!(
                    mm_two_base_balance.total_balance,
                    initial_total - expected_base_decrease
                );
                assert_eq!(mm_two_base_balance.available_balance, initial_avail);

                // Assert quote balance
                let mm_two_quote_balance = mm_two.asset_balances.get(1).unwrap();

                let initial_total = 1_000_000_000_000u128;
                let initial_avail =
                    1_000_000_000_000u128 - (2_300 * 1_100) * precision.quote_lot_size as u128;

                let expected_quote_increase =
                    (2500 * 1_000 + 2_600 * 200) * precision.quote_lot_size as u128;

                assert_eq!(
                    mm_two_quote_balance.total_balance,
                    initial_total + expected_quote_increase
                );
                assert_eq!(
                    mm_two_quote_balance.available_balance,
                    initial_avail + expected_quote_increase
                );
            }
        }

        #[test]
        fn test_limit_buy_with_partial_fill() {
            let user_public_key = [0; 32];
            let maker_one_public_key = [1; 32];
            let maker_two_public_key = [2; 32];
            let (mut spot_clearinghouse, precision) =
                test_setup(user_public_key, maker_one_public_key, maker_two_public_key);

            let buy_5 = new_limit(2_550, 1_800, OrderDirection::Buy, 10, user_public_key);
            spot_clearinghouse.handle_order(Order::Limit(buy_5), &precision);

            // asset user state
            {
                // Check account state
                let user_balance =
                    spot_clearinghouse.get_account_balance_or_default(&user_public_key);
                let user_base_balance = user_balance.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first
                let user_quote_balance = user_balance.asset_balances.get(1).unwrap();

                let expected_base_increase = (1_600 * precision.base_lot_size) as u128;
                assert_eq!(
                    user_base_balance.total_balance,
                    1_000_000_000 + expected_base_increase
                );
                assert_eq!(
                    user_base_balance.available_balance,
                    1_000_000_000 + expected_base_increase
                );

                let expected_quote_decrease = (2_500 * 1_600) * precision.quote_lot_size as u128;
                assert_eq!(
                    user_quote_balance.total_balance,
                    1_000_000_000_000 - expected_quote_decrease
                );
                assert_eq!(
                    user_quote_balance.available_balance,
                    1_000_000_000_000u128 - expected_quote_decrease
                ); // Quote balance should decrease
            }
            // Check maker state
            {
                // MM 1
                // Asset base balance
                let mm_one =
                    spot_clearinghouse.get_account_balance_or_default(&maker_one_public_key);
                let base_balance = mm_one.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first

                let initial_balance = 1_000_000_000;
                let initial_available =
                    (1_000_000_000 - ((600 + 700 + 300) * precision.base_lot_size)) as u128;

                let expected_base_decrease = (600 * precision.base_lot_size) as u128;

                assert_eq!(
                    base_balance.total_balance,
                    initial_balance - expected_base_decrease
                );
                assert_eq!(base_balance.available_balance, initial_available);

                // Quote balance
                let quote_balance = mm_one.asset_balances.get(1).unwrap();
                let initial_balance = 1_000_000_000_000;
                let initial_available = 1_000_000_000_000u128
                    - (2_200 * 700 + 2_450 * 1_000) * precision.quote_lot_size as u128;

                let expected_quote_increase = (600 * 2500 * precision.quote_lot_size) as u128;

                assert_eq!(
                    quote_balance.total_balance,
                    initial_balance + expected_quote_increase
                );
                assert_eq!(
                    quote_balance.available_balance,
                    initial_available + expected_quote_increase
                );
            }

            {
                // MM 2
                // Assert base balance
                let mm_two = spot_clearinghouse
                    .get_account_balance(&maker_two_public_key)
                    .unwrap();
                let mm_two_base_balance = mm_two.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first

                let initial_total = 1_000_000_000;
                let initial_avail =
                    (1_000_000_000 - (1_000 + 1_200) * precision.base_lot_size) as u128;

                let expected_base_decrease = (1_000) * precision.base_lot_size as u128;

                assert_eq!(
                    mm_two_base_balance.total_balance,
                    initial_total - expected_base_decrease
                );
                assert_eq!(mm_two_base_balance.available_balance, initial_avail);

                // Assert quote balance
                let mm_two_quote_balance = mm_two.asset_balances.get(1).unwrap();

                let initial_total = 1_000_000_000_000u128;
                let initial_avail =
                    1_000_000_000_000u128 - (2_300 * 1_100) * precision.quote_lot_size as u128;

                let expected_quote_increase = (2500 * 1_000) * precision.quote_lot_size as u128;

                assert_eq!(
                    mm_two_quote_balance.total_balance,
                    initial_total + expected_quote_increase
                );
                assert_eq!(
                    mm_two_quote_balance.available_balance,
                    initial_avail + expected_quote_increase
                );
            }
        }

        #[test]
        fn test_limit_sell_with_residual_order() {
            let user_public_key = [0; 32];
            let maker_one_public_key = [1; 32];
            let maker_two_public_key = [2; 32];
            let (mut spot_clearinghouse, precision) =
                test_setup(user_public_key, maker_one_public_key, maker_two_public_key);
            let sell = new_limit(2_100, 2_200, OrderDirection::Sell, 10, user_public_key);
            spot_clearinghouse.handle_order(Order::Limit(sell), &precision);

            // asset user state
            {
                // Check account state
                let user_balance =
                    spot_clearinghouse.get_account_balance_or_default(&user_public_key);
                let user_base_balance = user_balance.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first
                let user_quote_balance = user_balance.asset_balances.get(1).unwrap();

                let expected_base_decrease = (2_200 * precision.base_lot_size) as u128;
                assert_eq!(
                    user_base_balance.total_balance,
                    1_000_000_000 - expected_base_decrease
                );
                assert_eq!(
                    user_base_balance.available_balance,
                    1_000_000_000 - expected_base_decrease
                );

                let expected_quote_increase =
                    (2_450 * 1_000 + 2_300 * 1_100 + 2200 * 100) * precision.quote_lot_size as u128;
                assert_eq!(
                    user_quote_balance.total_balance,
                    1_000_000_000_000 + expected_quote_increase
                );
                assert_eq!(
                    user_quote_balance.available_balance,
                    1_000_000_000_000u128 + expected_quote_increase
                ); // Quote balance should decrease
            }
            // Check maker state
            {
                // MM 1
                // Asset base balance
                let mm_one =
                    spot_clearinghouse.get_account_balance_or_default(&maker_one_public_key);
                let base_balance = mm_one.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first

                let initial_balance = 1_000_000_000;
                let initial_available =
                    (1_000_000_000 - ((600 + 700 + 300) * precision.base_lot_size)) as u128;

                let expected_base_increase = ((1_000 + 100) * precision.base_lot_size) as u128;

                assert_eq!(
                    base_balance.total_balance,
                    initial_balance + expected_base_increase
                );
                assert_eq!(
                    base_balance.available_balance,
                    initial_available + expected_base_increase
                );

                // Quote balance
                let quote_balance = mm_one.asset_balances.get(1).unwrap();
                let initial_balance = 1_000_000_000_000;
                let initial_available = 1_000_000_000_000u128
                    - (2_200 * 700 + 2_450 * 1_000) * precision.quote_lot_size as u128;

                let expected_quote_decrease =
                    ((2450 * 1000 + 2_200 * 100) * precision.quote_lot_size) as u128;

                assert_eq!(
                    quote_balance.total_balance,
                    initial_balance - expected_quote_decrease
                );
                assert_eq!(quote_balance.available_balance, initial_available);
            }

            {
                // MM 2
                // Assert base balance
                let mm_two = spot_clearinghouse
                    .get_account_balance(&maker_two_public_key)
                    .unwrap();
                let mm_two_base_balance = mm_two.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first

                let initial_total = 1_000_000_000;
                let initial_avail =
                    (1_000_000_000 - (1_000 + 1_200) * precision.base_lot_size) as u128;

                let expected_base_increase = (700 + 400) * precision.base_lot_size as u128;

                assert_eq!(
                    mm_two_base_balance.total_balance,
                    initial_total + expected_base_increase
                );
                assert_eq!(
                    mm_two_base_balance.available_balance,
                    initial_avail + expected_base_increase
                );

                // Assert quote balance
                let mm_two_quote_balance = mm_two.asset_balances.get(1).unwrap();

                let initial_total = 1_000_000_000_000u128;
                let initial_avail =
                    1_000_000_000_000u128 - (2_300 * 1_100) * precision.quote_lot_size as u128;

                let expected_quote_decrease =
                    (2300 * 700 + 2_300 * 400) * precision.quote_lot_size as u128;

                assert_eq!(
                    mm_two_quote_balance.total_balance,
                    initial_total - expected_quote_decrease
                );
                assert_eq!(mm_two_quote_balance.available_balance, initial_avail);
            }
        }

        #[test]
        fn test_limit_sell_with_partial_fill() {
            let user_public_key = [0; 32];
            let maker_one_public_key = [1; 32];
            let maker_two_public_key = [2; 32];
            let (mut spot_clearinghouse, precision) =
                test_setup(user_public_key, maker_one_public_key, maker_two_public_key);
            let sell = new_limit(2_250, 2_200, OrderDirection::Sell, 10, user_public_key);
            spot_clearinghouse.handle_order(Order::Limit(sell), &precision);

            // asset user state
            {
                // Check account state
                let user_balance =
                    spot_clearinghouse.get_account_balance_or_default(&user_public_key);
                let user_base_balance = user_balance.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first
                let user_quote_balance = user_balance.asset_balances.get(1).unwrap();

                let expected_base_decrease = (2_100 * precision.base_lot_size) as u128;
                assert_eq!(
                    user_base_balance.total_balance,
                    1_000_000_000 - expected_base_decrease
                );
                assert_eq!(
                    user_base_balance.available_balance,
                    1_000_000_000 - expected_base_decrease
                );

                let expected_quote_increase =
                    (2_450 * 1_000 + 2_300 * 1_100) * precision.quote_lot_size as u128;
                assert_eq!(
                    user_quote_balance.total_balance,
                    1_000_000_000_000 + expected_quote_increase
                );
                assert_eq!(
                    user_quote_balance.available_balance,
                    1_000_000_000_000u128 + expected_quote_increase
                ); // Quote balance should decrease
            }
            // Check maker state
            {
                // MM 1
                // Asset base balance
                let mm_one =
                    spot_clearinghouse.get_account_balance_or_default(&maker_one_public_key);
                let base_balance = mm_one.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first

                let initial_balance = 1_000_000_000;
                let initial_available =
                    (1_000_000_000 - ((600 + 700 + 300) * precision.base_lot_size)) as u128;

                let expected_base_increase = ((1_000) * precision.base_lot_size) as u128;

                assert_eq!(
                    base_balance.total_balance,
                    initial_balance + expected_base_increase
                );
                assert_eq!(
                    base_balance.available_balance,
                    initial_available + expected_base_increase
                );

                // Quote balance
                let quote_balance = mm_one.asset_balances.get(1).unwrap();
                let initial_balance = 1_000_000_000_000;
                let initial_available = 1_000_000_000_000u128
                    - (2_200 * 700 + 2_450 * 1_000) * precision.quote_lot_size as u128;

                let expected_quote_decrease = ((2450 * 1000) * precision.quote_lot_size) as u128;

                assert_eq!(
                    quote_balance.total_balance,
                    initial_balance - expected_quote_decrease
                );
                assert_eq!(quote_balance.available_balance, initial_available);
            }

            {
                // MM 2
                // Assert base balance
                let mm_two = spot_clearinghouse
                    .get_account_balance(&maker_two_public_key)
                    .unwrap();
                let mm_two_base_balance = mm_two.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first

                let initial_total = 1_000_000_000;
                let initial_avail =
                    (1_000_000_000 - (1_000 + 1_200) * precision.base_lot_size) as u128;

                let expected_base_increase = (700 + 400) * precision.base_lot_size as u128;

                assert_eq!(
                    mm_two_base_balance.total_balance,
                    initial_total + expected_base_increase
                );
                assert_eq!(
                    mm_two_base_balance.available_balance,
                    initial_avail + expected_base_increase
                );

                // Assert quote balance
                let mm_two_quote_balance = mm_two.asset_balances.get(1).unwrap();

                let initial_total = 1_000_000_000_000u128;
                let initial_avail =
                    1_000_000_000_000u128 - (2_300 * 1_100) * precision.quote_lot_size as u128;

                let expected_quote_decrease =
                    (2300 * 700 + 2_300 * 400) * precision.quote_lot_size as u128;

                assert_eq!(
                    mm_two_quote_balance.total_balance,
                    initial_total - expected_quote_decrease
                );
                assert_eq!(mm_two_quote_balance.available_balance, initial_avail);
            }
        }
    }

    mod test_market_execution_side_effects {
        use crate::state::order::Order;

        use super::{new_market_buy, new_market_sell, test_setup};

        #[test]
        fn test_market_buy() {
            let user_public_key = [0; 32];
            let maker_one_public_key = [1; 32];
            let maker_two_public_key = [2; 32];
            let (mut spot_clearinghouse, precision) =
                test_setup(user_public_key, maker_one_public_key, maker_two_public_key);

            let buy = new_market_buy(10, 2_500_500, user_public_key);
            spot_clearinghouse.handle_order(Order::Market(buy), &precision);

            // asset user state
            {
                // Check account state
                let user_balance =
                    spot_clearinghouse.get_account_balance_or_default(&user_public_key);
                let user_base_balance = user_balance.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first
                let user_quote_balance = user_balance.asset_balances.get(1).unwrap();

                let expected_base_increase = (1_000 * precision.base_lot_size) as u128;
                assert_eq!(
                    user_base_balance.total_balance,
                    1_000_000_000 + expected_base_increase
                );
                assert_eq!(
                    user_base_balance.available_balance,
                    1_000_000_000 + expected_base_increase
                );

                let expected_quote_decrease = (2_500 * 1_000) * precision.quote_lot_size as u128;
                assert_eq!(
                    user_quote_balance.total_balance,
                    1_000_000_000_000 - expected_quote_decrease
                );
                assert_eq!(
                    user_quote_balance.available_balance,
                    1_000_000_000_000u128 - expected_quote_decrease
                ); // Quote balance should decrease
            }
            // Check maker state
            {
                // MM 1
                // Asset base balance
                let mm_one =
                    spot_clearinghouse.get_account_balance_or_default(&maker_one_public_key);
                let base_balance = mm_one.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first

                let initial_balance = 1_000_000_000;
                let initial_available =
                    (1_000_000_000 - ((600 + 700 + 300) * precision.base_lot_size)) as u128;

                let expected_base_decrease = (600 * precision.base_lot_size) as u128;

                assert_eq!(
                    base_balance.total_balance,
                    initial_balance - expected_base_decrease
                );
                assert_eq!(base_balance.available_balance, initial_available);

                // Quote balance
                let quote_balance = mm_one.asset_balances.get(1).unwrap();
                let initial_balance = 1_000_000_000_000;
                let initial_available = 1_000_000_000_000u128
                    - (2_200 * 700 + 2_450 * 1_000) * precision.quote_lot_size as u128;

                let expected_quote_increase = (600 * 2500 * precision.quote_lot_size) as u128;

                assert_eq!(
                    quote_balance.total_balance,
                    initial_balance + expected_quote_increase
                );
                assert_eq!(
                    quote_balance.available_balance,
                    initial_available + expected_quote_increase
                );
            }

            {
                // MM 2
                // Assert base balance
                let mm_two = spot_clearinghouse
                    .get_account_balance(&maker_two_public_key)
                    .unwrap();
                let mm_two_base_balance = mm_two.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first

                let initial_total = 1_000_000_000;
                let initial_avail =
                    (1_000_000_000 - (1_000 + 1_200) * precision.base_lot_size) as u128;

                let expected_base_decrease = (400) * precision.base_lot_size as u128;

                assert_eq!(
                    mm_two_base_balance.total_balance,
                    initial_total - expected_base_decrease
                );
                assert_eq!(mm_two_base_balance.available_balance, initial_avail);

                // Assert quote balance
                let mm_two_quote_balance = mm_two.asset_balances.get(1).unwrap();

                let initial_total = 1_000_000_000_000u128;
                let initial_avail =
                    1_000_000_000_000u128 - (2_300 * 1_100) * precision.quote_lot_size as u128;

                let expected_quote_increase = (2500 * 400) * precision.quote_lot_size as u128;

                assert_eq!(
                    mm_two_quote_balance.total_balance,
                    initial_total + expected_quote_increase
                );
                assert_eq!(
                    mm_two_quote_balance.available_balance,
                    initial_avail + expected_quote_increase
                );
            }
        }

        #[test]
        fn test_market_buy_consuming_book() {
            let user_public_key = [0; 32];
            let maker_one_public_key = [1; 32];
            let maker_two_public_key = [2; 32];
            let (mut spot_clearinghouse, precision) =
                test_setup(user_public_key, maker_one_public_key, maker_two_public_key);

            let buy = new_market_buy(10, 50_000_000, user_public_key);
            spot_clearinghouse.handle_order(Order::Market(buy), &precision);

            // asset user state
            {
                // Check account state
                let user_balance =
                    spot_clearinghouse.get_account_balance_or_default(&user_public_key);
                let user_base_balance = user_balance.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first
                let user_quote_balance = user_balance.asset_balances.get(1).unwrap();

                let expected_base_increase = (3_800 * precision.base_lot_size) as u128;
                assert_eq!(
                    user_base_balance.total_balance,
                    1_000_000_000 + expected_base_increase
                );
                assert_eq!(
                    user_base_balance.available_balance,
                    1_000_000_000 + expected_base_increase
                );

                let expected_quote_decrease =
                    ((2_500 * 1_600) + (2_600 * 1_200) + (2_700 * 700) + (2_800 * 300))
                        * precision.quote_lot_size as u128;
                assert_eq!(
                    user_quote_balance.total_balance,
                    1_000_000_000_000 - expected_quote_decrease
                );
                assert_eq!(
                    user_quote_balance.available_balance,
                    1_000_000_000_000u128 - expected_quote_decrease
                ); // Quote balance should decrease
            }
            // Check maker state
            {
                // MM 1
                // Asset base balance
                let mm_one =
                    spot_clearinghouse.get_account_balance_or_default(&maker_one_public_key);
                let base_balance = mm_one.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first

                let initial_balance = 1_000_000_000;
                let initial_available =
                    (1_000_000_000 - ((600 + 700 + 300) * precision.base_lot_size)) as u128;

                let expected_base_decrease = (1_600 * precision.base_lot_size) as u128;

                assert_eq!(
                    base_balance.total_balance,
                    initial_balance - expected_base_decrease
                );
                assert_eq!(base_balance.available_balance, initial_available);

                // Quote balance
                let quote_balance = mm_one.asset_balances.get(1).unwrap();
                let initial_balance = 1_000_000_000_000;
                let initial_available = 1_000_000_000_000u128
                    - (2_200 * 700 + 2_450 * 1_000) * precision.quote_lot_size as u128;

                let expected_quote_increase: u128 =
                    (((2_500 * 600) + (2_700 * 700) + (2_800 * 300)) * precision.quote_lot_size)
                        as u128;

                assert_eq!(
                    quote_balance.total_balance,
                    initial_balance + expected_quote_increase
                );
                assert_eq!(
                    quote_balance.available_balance,
                    initial_available + expected_quote_increase
                );
            }

            {
                // MM 2
                // Assert base balance
                let mm_two = spot_clearinghouse
                    .get_account_balance(&maker_two_public_key)
                    .unwrap();
                let mm_two_base_balance = mm_two.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first

                let initial_total = 1_000_000_000;
                let initial_avail =
                    (1_000_000_000 - (1_000 + 1_200) * precision.base_lot_size) as u128;

                let expected_base_decrease = (1_000 + 1_200) * precision.base_lot_size as u128;

                assert_eq!(
                    mm_two_base_balance.total_balance,
                    initial_total - expected_base_decrease
                );
                assert_eq!(mm_two_base_balance.available_balance, initial_avail);

                // Assert quote balance
                let mm_two_quote_balance = mm_two.asset_balances.get(1).unwrap();

                let initial_total = 1_000_000_000_000u128;
                let initial_avail =
                    1_000_000_000_000u128 - (2_300 * 1_100) * precision.quote_lot_size as u128;

                let expected_quote_increase =
                    (2500 * 1_000 + 2_600 * 1_200) * precision.quote_lot_size as u128;

                assert_eq!(
                    mm_two_quote_balance.total_balance,
                    initial_total + expected_quote_increase
                );
                assert_eq!(
                    mm_two_quote_balance.available_balance,
                    initial_avail + expected_quote_increase
                );
            }
        }

        #[test]
        fn test_market_sell() {
            let user_public_key = [0; 32];
            let maker_one_public_key = [1; 32];
            let maker_two_public_key = [2; 32];
            let (mut spot_clearinghouse, precision) =
                test_setup(user_public_key, maker_one_public_key, maker_two_public_key);
            let sell = new_market_sell(10, 2_200, user_public_key);
            spot_clearinghouse.handle_order(Order::Market(sell), &precision);

            // asset user state
            {
                // Check account state
                let user_balance =
                    spot_clearinghouse.get_account_balance_or_default(&user_public_key);
                let user_base_balance = user_balance.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first
                let user_quote_balance = user_balance.asset_balances.get(1).unwrap();

                let expected_base_decrease = (2_200 * precision.base_lot_size) as u128;
                assert_eq!(
                    user_base_balance.total_balance,
                    1_000_000_000 - expected_base_decrease
                );
                assert_eq!(
                    user_base_balance.available_balance,
                    1_000_000_000 - expected_base_decrease
                );

                let expected_quote_increase =
                    (2_450 * 1_000 + 2_300 * 1_100 + 2200 * 100) * precision.quote_lot_size as u128;
                assert_eq!(
                    user_quote_balance.total_balance,
                    1_000_000_000_000 + expected_quote_increase
                );
                assert_eq!(
                    user_quote_balance.available_balance,
                    1_000_000_000_000u128 + expected_quote_increase
                ); // Quote balance should decrease
            }
            // Check maker state
            {
                // MM 1
                // Asset base balance
                let mm_one = spot_clearinghouse
                    .get_account_balance(&maker_one_public_key)
                    .unwrap();
                let base_balance = mm_one.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first

                let initial_balance = 1_000_000_000;
                let initial_available =
                    (1_000_000_000 - ((600 + 700 + 300) * precision.base_lot_size)) as u128;

                let expected_base_increase = ((1_000 + 100) * precision.base_lot_size) as u128;

                assert_eq!(
                    base_balance.total_balance,
                    initial_balance + expected_base_increase
                );
                assert_eq!(
                    base_balance.available_balance,
                    initial_available + expected_base_increase
                );

                // Quote balance
                let quote_balance = mm_one.asset_balances.get(1).unwrap();
                let initial_balance = 1_000_000_000_000;
                let initial_available = 1_000_000_000_000u128
                    - (2_200 * 700 + 2_450 * 1_000) * precision.quote_lot_size as u128;

                let expected_quote_decrease =
                    ((2450 * 1000 + 2_200 * 100) * precision.quote_lot_size) as u128;

                assert_eq!(
                    quote_balance.total_balance,
                    initial_balance - expected_quote_decrease
                );
                assert_eq!(quote_balance.available_balance, initial_available);
            }

            {
                // MM 2
                // Assert base balance
                let mm_two = spot_clearinghouse
                    .get_account_balance(&maker_two_public_key)
                    .unwrap();
                let mm_two_base_balance = mm_two.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first

                let initial_total = 1_000_000_000;
                let initial_avail =
                    (1_000_000_000 - (1_000 + 1_200) * precision.base_lot_size) as u128;

                let expected_base_increase = (700 + 400) * precision.base_lot_size as u128;

                assert_eq!(
                    mm_two_base_balance.total_balance,
                    initial_total + expected_base_increase
                );
                assert_eq!(
                    mm_two_base_balance.available_balance,
                    initial_avail + expected_base_increase
                );

                // Assert quote balance
                let mm_two_quote_balance = mm_two.asset_balances.get(1).unwrap();

                let initial_total = 1_000_000_000_000u128;
                let initial_avail =
                    1_000_000_000_000u128 - (2_300 * 1_100) * precision.quote_lot_size as u128;

                let expected_quote_decrease =
                    (2300 * 700 + 2_300 * 400) * precision.quote_lot_size as u128;

                assert_eq!(
                    mm_two_quote_balance.total_balance,
                    initial_total - expected_quote_decrease
                );
                assert_eq!(mm_two_quote_balance.available_balance, initial_avail);
            }
        }

        #[test]
        fn test_market_sell_consume_book() {
            let user_public_key = [0; 32];
            let maker_one_public_key = [1; 32];
            let maker_two_public_key = [2; 32];
            let (mut spot_clearinghouse, precision) =
                test_setup(user_public_key, maker_one_public_key, maker_two_public_key);
            let sell = new_market_sell(10, 10_000, user_public_key);
            spot_clearinghouse.handle_order(Order::Market(sell), &precision);

            // asset user state
            {
                // Check account state
                let user_balance =
                    spot_clearinghouse.get_account_balance_or_default(&user_public_key);
                let user_base_balance = user_balance.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first
                let user_quote_balance = user_balance.asset_balances.get(1).unwrap();

                let expected_base_decrease = (2_800 * precision.base_lot_size) as u128;
                assert_eq!(
                    user_base_balance.total_balance,
                    1_000_000_000 - expected_base_decrease
                );
                assert_eq!(
                    user_base_balance.available_balance,
                    1_000_000_000 - expected_base_decrease
                );

                let expected_quote_increase = (2_450 * 1_000 + 2_300 * 1_100 + 2_200 * 700)
                    * precision.quote_lot_size as u128;
                assert_eq!(
                    user_quote_balance.total_balance,
                    1_000_000_000_000 + expected_quote_increase
                );
                assert_eq!(
                    user_quote_balance.available_balance,
                    1_000_000_000_000u128 + expected_quote_increase
                ); // Quote balance should decrease
            }
            // Check maker state
            {
                // MM 1
                // Asset base balance
                let mm_one =
                    spot_clearinghouse.get_account_balance_or_default(&maker_one_public_key);
                let base_balance = mm_one.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first

                let initial_balance = 1_000_000_000;
                let initial_available =
                    (1_000_000_000 - ((600 + 700 + 300) * precision.base_lot_size)) as u128;

                let expected_base_increase = ((1_700) * precision.base_lot_size) as u128;

                assert_eq!(
                    base_balance.total_balance,
                    initial_balance + expected_base_increase
                );
                assert_eq!(
                    base_balance.available_balance,
                    initial_available + expected_base_increase
                );

                // Quote balance
                let quote_balance = mm_one.asset_balances.get(1).unwrap();
                let initial_balance = 1_000_000_000_000;
                let initial_available = 1_000_000_000_000u128
                    - (2_200 * 700 + 2_450 * 1_000) * precision.quote_lot_size as u128;

                let expected_quote_decrease =
                    ((2450 * 1000 + 2_200 * 700) * precision.quote_lot_size) as u128;

                assert_eq!(
                    quote_balance.total_balance,
                    initial_balance - expected_quote_decrease
                );
                assert_eq!(quote_balance.available_balance, initial_available);
            }

            {
                // MM 2
                // Assert base balance
                let mm_two = spot_clearinghouse
                    .get_account_balance(&maker_two_public_key)
                    .unwrap();
                let mm_two_base_balance = mm_two.asset_balances.get(0).unwrap(); // base should be index 0 since we initialise it first

                let initial_total = 1_000_000_000;
                let initial_avail =
                    (1_000_000_000 - (1_000 + 1_200) * precision.base_lot_size) as u128;

                let expected_base_increase = (700 + 400) * precision.base_lot_size as u128;

                assert_eq!(
                    mm_two_base_balance.total_balance,
                    initial_total + expected_base_increase
                );
                assert_eq!(
                    mm_two_base_balance.available_balance,
                    initial_avail + expected_base_increase
                );

                // Assert quote balance
                let mm_two_quote_balance = mm_two.asset_balances.get(1).unwrap();

                let initial_total = 1_000_000_000_000u128;
                let initial_avail =
                    1_000_000_000_000u128 - (2_300 * 1_100) * precision.quote_lot_size as u128;

                let expected_quote_decrease =
                    (2300 * 700 + 2_300 * 400) * precision.quote_lot_size as u128;

                assert_eq!(
                    mm_two_quote_balance.total_balance,
                    initial_total - expected_quote_decrease
                );
                assert_eq!(mm_two_quote_balance.available_balance, initial_avail);
            }
        }
    }
}
