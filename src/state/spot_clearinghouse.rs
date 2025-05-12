use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{config, state::order::OrderDirection, types::transaction::PublicKeyHash};

use super::{
    asset::AssetId,
    order::{
        CounterPartyPartialFill, ExecutionResults, LimitFillResult, LimitOrder, MarketBuyOrder,
        MarketOrder, MarketOrderMatchingResults, MarketSellOrder, Order, OrderChange, OrderPrice,
        OrderStatus,
    },
};

pub type MarketId = usize;
type MarketIdCounter = MarketId;

pub fn base_to_quote(base_amount: u64, price: u64) -> u64 {
    base_amount * price
}

pub fn quote_to_base(quote_amount: u64, price: u64) -> u64 {
    quote_amount / price
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

    fn create_new_market(&mut self, normalised_pair: (AssetId, AssetId)) -> MarketId {
        let market_id = self.next_id;
        let market = SpotMarket::new(market_id, normalised_pair.0, normalised_pair.1);
        self.markets.push(market);
        self.asset_to_market_map.insert(normalised_pair, market_id);
        self.next_id += 1;
        market_id
    }

    pub fn add_market(&mut self, asset_one: AssetId, asset_two: AssetId) -> MarketId {
        if let Some(market_id) = self.get_market_id_from_pair(asset_one, asset_two) {
            return market_id;
        }

        let normalised_pair = Self::normalise_pair(asset_one, asset_two);

        self.create_new_market(normalised_pair)
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
    pub fn handle_order(&mut self, order: Order) -> Option<ExecutionResults> {
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
                        let base_amount = quote_to_base(limit_order.quote_size, limit_order.price);
                        let base_token_balance =
                            Self::get_account_token_balance_mut(account_balance, market.base_asset);

                        if base_token_balance.available_balance < base_amount as u128 {
                            println!("Not enough balance");
                            return None;
                        }
                        base_token_balance.available_balance -= base_amount as u128;
                    }
                    OrderDirection::Sell => {
                        let quote_amount = limit_order.quote_size;
                        let quote_token_balance = Self::get_account_token_balance_mut(
                            account_balance,
                            market.quote_asset,
                        );

                        if quote_token_balance.available_balance < quote_amount as u128 {
                            println!("Not enough balance");
                            return None;
                        }
                        quote_token_balance.available_balance -= quote_amount as u128;
                    }
                }

                let base_asset = market.base_asset;
                let quote_asset = market.quote_asset;
                let results = market.add_limit_order(limit_order.clone(), base_asset, quote_asset);

                match results {
                    Some(limit_fill_results) => {
                        // Limit order was able to execute at a better price
                        let LimitFillResult {
                            order_id,
                            asset_out: user_asset_out,
                            amount_out,
                            asset_in: user_asset_in,
                            amount_in,
                            filled_orders,
                            counterparty_partial_fill,
                            filled_size,
                        } = limit_fill_results;

                        // Modify user's token balance
                        let asset_out_balance =
                            Self::get_account_token_balance_mut(account_balance, user_asset_out);
                        asset_out_balance.total_balance -= amount_out as u128;

                        let asset_in_balance =
                            Self::get_account_token_balance_mut(account_balance, user_asset_in);
                        asset_in_balance.total_balance += amount_in as u128;
                        asset_in_balance.available_balance += amount_in as u128;

                        // Modify filled order's token balance if any
                        for order in filled_orders.iter() {
                            // Buy orders
                            let account_balance =
                                self.get_account_balance_mut(&order.common.account);

                            let filled_quote_amount = order.quote_size - order.filled_quote_size;

                            let mut counterparty_asset_in_amount = filled_quote_amount;
                            let mut counterparty_asset_out_amount = filled_quote_amount;

                            if user_asset_in == base_asset {
                                counterparty_asset_out_amount =
                                    quote_to_base(filled_quote_amount, order.price);
                            } else {
                                counterparty_asset_in_amount =
                                    quote_to_base(filled_quote_amount, order.price)
                            }

                            // counter pay recieves asset_out at it's price
                            let counterparty_asset_out_balance =
                                Self::get_account_token_balance_mut(account_balance, user_asset_in);

                            counterparty_asset_out_balance.total_balance -=
                                counterparty_asset_out_amount as u128;

                            let counterparty_asset_in_balance = Self::get_account_token_balance_mut(
                                account_balance,
                                user_asset_out,
                            );

                            counterparty_asset_in_balance.available_balance +=
                                counterparty_asset_in_amount as u128;
                            counterparty_asset_in_balance.total_balance +=
                                counterparty_asset_in_amount as u128;
                        }

                        // Modify partial fill's token balance if any
                        match &counterparty_partial_fill {
                            Some(counter_partial_fill) => {
                                // Handle partial fill
                                let CounterPartyPartialFill {
                                    account_public_key: counterparty_public_key,
                                    filled_quote_amount,
                                    order_price,
                                    ..
                                } = counter_partial_fill;

                                let counterparty_balance =
                                    &mut self.get_account_balance(&counterparty_public_key);

                                let mut counterparty_asset_in_amount = *filled_quote_amount;
                                let mut counterparty_asset_out_amount = *filled_quote_amount;

                                if user_asset_in == base_asset {
                                    counterparty_asset_out_amount =
                                        quote_to_base(*filled_quote_amount, *order_price);
                                } else {
                                    counterparty_asset_in_amount =
                                        quote_to_base(*filled_quote_amount, *order_price)
                                }

                                // counter pay recieves asset_out at it's price
                                let counterparty_asset_out_balance =
                                    Self::get_account_token_balance_mut(
                                        counterparty_balance,
                                        user_asset_in,
                                    );

                                counterparty_asset_out_balance.total_balance -=
                                    counterparty_asset_out_amount as u128;

                                let counterparty_asset_in_balance =
                                    Self::get_account_token_balance_mut(
                                        counterparty_balance,
                                        user_asset_out,
                                    );
                                counterparty_asset_in_balance.available_balance +=
                                    counterparty_asset_in_amount as u128;
                                counterparty_asset_in_balance.total_balance +=
                                    counterparty_asset_in_amount as u128;
                            }
                            None => {
                                // No partial fills, do nothing
                            }
                        }

                        let mut average_execution_price = amount_out / amount_in;
                        if user_asset_in == base_asset {
                            average_execution_price = amount_in / amount_out; // quote / base
                        }

                        return Some(ExecutionResults {
                            filled_orders,
                            counterparty_partial_fill,
                            user_order_change: Some(OrderChange::LimitOrderChange {
                                order_id,
                                filled_amount: filled_size,
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
                    counterparty_partial_fill: None,
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
                        let quote_token_balance = Self::get_account_token_balance_mut(
                            account_balance,
                            market.quote_asset,
                        );
                        if quote_token_balance.available_balance < sell_order.quote_size as u128 {
                            println!("Not enough balance");
                        }
                        quote_token_balance.available_balance -= sell_order.quote_size as u128;
                    }
                    MarketOrder::Buy(buy_order) => {
                        let base_token_balance =
                            Self::get_account_token_balance_mut(account_balance, market.base_asset);
                        if base_token_balance.available_balance < buy_order.base_size as u128 {
                            println!("Not enough balance");
                        }
                        base_token_balance.available_balance -= buy_order.base_size as u128;
                    }
                };

                let results = market.handle_market_order(market_order);
                let base_asset = market.base_asset;
                let quote_asset = market.quote_asset;

                // Settlement
                match results {
                    MarketOrderMatchingResults::SellInQuote {
                        order_id,
                        quote_filled_amount,
                        base_amount_in,
                        filled_orders,
                        counterparty_partial_fill,
                    } => {
                        let quote_token_balance =
                            Self::get_account_token_balance_mut(account_balance, quote_asset);
                        quote_token_balance.total_balance -= quote_filled_amount as u128;
                        let base_token_balance =
                            Self::get_account_token_balance_mut(account_balance, base_asset);
                        base_token_balance.total_balance += base_amount_in as u128;
                        base_token_balance.available_balance += base_amount_in as u128;

                        let average_execution_price = quote_filled_amount / base_amount_in;
                        for order in filled_orders.iter() {
                            // Buy orders
                            let account_balance =
                                self.get_account_balance_mut(&order.common.account);
                            let filled_quote_amount = order.quote_size - order.filled_quote_size;

                            let base_token_balance =
                                Self::get_account_token_balance_mut(account_balance, base_asset);
                            base_token_balance.total_balance -=
                                quote_to_base(filled_quote_amount, order.price) as u128;

                            let quote_token_balance =
                                Self::get_account_token_balance_mut(account_balance, quote_asset);

                            quote_token_balance.available_balance += filled_quote_amount as u128;
                            quote_token_balance.total_balance += filled_quote_amount as u128;
                        }
                        match &counterparty_partial_fill {
                            Some(counter_partial_fill) => {
                                let CounterPartyPartialFill {
                                    account_public_key,
                                    filled_quote_amount,
                                    order_price,
                                    ..
                                } = counter_partial_fill;

                                let account_balance =
                                    &mut self.get_account_balance(&account_public_key);

                                let base_token_balance = Self::get_account_token_balance_mut(
                                    account_balance,
                                    base_asset,
                                );
                                base_token_balance.total_balance -=
                                    quote_to_base(*filled_quote_amount, *order_price) as u128;

                                let quote_token_balance = Self::get_account_token_balance_mut(
                                    account_balance,
                                    quote_asset,
                                );

                                quote_token_balance.available_balance +=
                                    *filled_quote_amount as u128;
                                quote_token_balance.total_balance += *filled_quote_amount as u128;
                            }
                            None => {}
                        }

                        return Some(ExecutionResults {
                            filled_orders,
                            counterparty_partial_fill: counterparty_partial_fill,
                            user_order_change: Some(OrderChange::MarketOrderChange {
                                order_id,
                                filled_amount: quote_filled_amount,
                                average_execution_price: average_execution_price,
                            }),
                        });
                    }
                    MarketOrderMatchingResults::BuyInBase {
                        order_id,
                        base_filled_amount,
                        filled_orders,
                        quote_amount_in,
                        counterparty_partial_fill,
                    } => {
                        let base_token_balance =
                            Self::get_account_token_balance_mut(account_balance, market.base_asset);
                        base_token_balance.total_balance -= base_filled_amount as u128;

                        let quote_token_balance = Self::get_account_token_balance_mut(
                            account_balance,
                            market.quote_asset,
                        );
                        quote_token_balance.total_balance += quote_amount_in as u128;
                        quote_token_balance.available_balance += quote_amount_in as u128;

                        let average_execution_price = quote_amount_in / base_filled_amount;

                        for order in filled_orders.iter() {
                            // Sell orders
                            let account_balance =
                                self.get_account_balance_mut(&order.common.account);
                            let filled_quote_amount = order.quote_size - order.filled_quote_size;

                            let base_token_balance =
                                Self::get_account_token_balance_mut(account_balance, base_asset);
                            let filled_base_amount =
                                quote_to_base(filled_quote_amount, order.price) as u128;
                            base_token_balance.total_balance += filled_base_amount;
                            base_token_balance.available_balance += filled_base_amount;

                            let quote_token_balance =
                                Self::get_account_token_balance_mut(account_balance, quote_asset);

                            quote_token_balance.total_balance -= filled_quote_amount as u128;
                        }
                        match &counterparty_partial_fill {
                            Some(counter_partial_fill) => {
                                let CounterPartyPartialFill {
                                    account_public_key,
                                    filled_quote_amount,
                                    order_price,
                                    ..
                                } = counter_partial_fill;

                                let account_balance =
                                    self.get_account_balance_mut(&account_public_key);

                                let base_token_balance = Self::get_account_token_balance_mut(
                                    account_balance,
                                    base_asset,
                                );
                                let filled_base_amount =
                                    quote_to_base(*filled_quote_amount, *order_price) as u128;
                                base_token_balance.total_balance += filled_base_amount;
                                base_token_balance.available_balance += filled_base_amount;

                                let quote_token_balance = Self::get_account_token_balance_mut(
                                    account_balance,
                                    quote_asset,
                                );

                                quote_token_balance.total_balance -= *filled_quote_amount as u128;
                            }
                            None => {}
                        }
                        return Some(ExecutionResults {
                            filled_orders,
                            counterparty_partial_fill: counterparty_partial_fill,
                            user_order_change: Some(OrderChange::MarketOrderChange {
                                order_id,
                                filled_amount: base_filled_amount,
                                average_execution_price: average_execution_price,
                            }),
                        });
                    }
                }
            }
        }
    }
}

pub struct Level {
    pub price: u64,
    pub volume: u64,
    pub orders: Vec<LimitOrder>,
    pub cancelled: u32,
}
pub struct SpotMarket {
    pub market_id: MarketId,
    pub asset_one: AssetId,
    pub asset_two: AssetId,
    pub base_asset: AssetId,
    pub quote_asset: AssetId,
    // pub tick_size: (),
    // pub lot_size: (),

    // levels are in reverse order, best prices are at the end
    pub bids_levels: Vec<Level>, // 0, 1, 2, ..
    pub asks_levels: Vec<Level>, // 10, 9, 8, ..
}

impl SpotMarket {
    fn new(market_id: MarketId, base_asset: AssetId, quote_asset: AssetId) -> Self {
        let (asset_one, asset_two) = {
            if base_asset < quote_asset {
                (base_asset, quote_asset)
            } else {
                (quote_asset, base_asset)
            }
        };

        Self {
            market_id,
            asset_one,
            asset_two,
            base_asset,
            quote_asset,
            bids_levels: vec![],
            asks_levels: vec![],
        }
    }

    fn add_order_with_cmp<F>(levels: &mut Vec<Level>, order: LimitOrder, mut compare: F)
    where
        F: FnMut(OrderPrice, OrderPrice) -> std::cmp::Ordering,
    {
        let price = order.price;
        let mut left = 0;
        let mut right = levels.len();

        while left < right {
            let mid = left + (right - left) / 2;
            let mid_price = levels[mid].price;

            if price == mid_price {
                levels[mid].volume += order.quote_size;
                levels[mid].orders.push(order);
                return;
            } else {
                if compare(price, mid_price) == std::cmp::Ordering::Less {
                    right = mid;
                } else {
                    left = mid + 1;
                }
            }
        }

        levels.insert(
            left,
            Level {
                price,
                volume: order.quote_size,
                orders: vec![order],
                cancelled: 0,
            },
        )
    }

    fn mark_order_as_cancelled(orders: &mut Vec<LimitOrder>, order: &LimitOrder) -> bool {
        let order_id = order.common.id;
        let mut left = 0;
        let mut right = orders.len();
        while left < right {
            let mid = left + (right - left) / 2;
            let mid_id = orders[mid].common.id;

            if order_id == mid_id {
                if orders[mid].common.status == OrderStatus::Cancelled {
                    return false;
                }
                orders[mid].common.status = OrderStatus::Cancelled;
                return true;
            } else if order_id < mid_id {
                right = mid;
            } else {
                left = mid + 1;
            }
        }
        return false;
    }

    fn cancel_order_with_cmp<F>(levels: &mut Vec<Level>, order: &LimitOrder, mut compare: F)
    where
        F: FnMut(OrderPrice, OrderPrice) -> std::cmp::Ordering,
    {
        let price = order.price;
        let mut left = 0;
        let mut right = levels.len();

        while left < right {
            let mid = left + (right - left) / 2;
            let mid_price = levels[mid].price;

            if price == mid_price {
                let level = &mut levels[mid];
                if !Self::mark_order_as_cancelled(&mut level.orders, order) {
                    return;
                }

                level.cancelled += 1;
                let unfilled_size = order.quote_size - order.filled_quote_size;
                level.volume -= unfilled_size;

                if level.cancelled <= (level.orders.len() / 2) as u32 {
                    return;
                }
                // prune when vector is sparse enough
                level
                    .orders
                    .retain(|order| order.common.status != OrderStatus::Cancelled);
                level.cancelled = 0;
                return;
            } else {
                if compare(price, mid_price) == std::cmp::Ordering::Less {
                    right = mid;
                } else {
                    left = mid + 1;
                }
            }
        }
    }

    pub fn add_bid(&mut self, order: LimitOrder) {
        Self::add_order_with_cmp(&mut self.bids_levels, order, |a, b| {
            a.partial_cmp(&b).unwrap()
        });
    }

    pub fn add_ask(&mut self, order: LimitOrder) {
        Self::add_order_with_cmp(&mut self.asks_levels, order, |a, b| {
            b.partial_cmp(&a).unwrap()
        });
    }

    pub fn cancel_bid(&mut self, order: &LimitOrder) {
        Self::cancel_order_with_cmp(&mut self.bids_levels, order, |a, b| {
            a.partial_cmp(&b).unwrap()
        });
    }

    pub fn cancel_ask(&mut self, order: &LimitOrder) {
        Self::cancel_order_with_cmp(&mut self.asks_levels, order, |a, b| {
            b.partial_cmp(&a).unwrap()
        });
    }

    pub fn execute_limit<F>(
        levels: &mut Vec<Level>,
        order: &mut LimitOrder,
        asset_in: AssetId,
        asset_out: AssetId,
        is_buy: bool,
        mut compare: F,
    ) -> LimitFillResult
    where
        F: FnMut(OrderPrice, OrderPrice) -> std::cmp::Ordering,
    {
        let mut filled_orders: Vec<LimitOrder> = vec![];

        let mut counterparty_partial_fill: Option<CounterPartyPartialFill> = None;
        let mut amount_in: u64 = 0;
        let mut amount_out: u64 = 0;

        let order_price = order.price;
        let mut remaining_quote_amount = order.quote_size;
        while !levels.is_empty() && remaining_quote_amount > 0 {
            let level = levels.last_mut();
            let Some(level) = level else {
                break;
            };

            let level_price = level.price;
            match compare(order_price, level_price) {
                std::cmp::Ordering::Less => {
                    // fill the current level orders
                    let mut to_drain_end_index = 0;
                    for order in level.orders.iter_mut() {
                        let order_remaining = order.quote_size - order.filled_quote_size;
                        let curr_filled_quote_amount = remaining_quote_amount.min(order_remaining);
                        remaining_quote_amount -= curr_filled_quote_amount;

                        if is_buy {
                            amount_in += curr_filled_quote_amount;
                            amount_out += quote_to_base(curr_filled_quote_amount, level_price);
                        } else {
                            amount_out += curr_filled_quote_amount;
                            amount_in += quote_to_base(curr_filled_quote_amount, level_price);
                        }
                        amount_in += quote_to_base(curr_filled_quote_amount, level_price);

                        if curr_filled_quote_amount == order_remaining {
                            to_drain_end_index += 1;
                        }

                        if remaining_quote_amount <= 0 {
                            if curr_filled_quote_amount < order_remaining {
                                counterparty_partial_fill = Some(CounterPartyPartialFill {
                                    order_id: order.common.id,
                                    order_price: level_price,
                                    account_public_key: order.common.account,
                                    filled_quote_amount: curr_filled_quote_amount,
                                });
                                order.filled_quote_size += curr_filled_quote_amount;
                            }
                            break;
                        }
                    }

                    if to_drain_end_index < level.orders.len() {
                        filled_orders
                            .append(&mut level.orders.drain(0..to_drain_end_index).collect());
                        break;
                    }
                    // reached the end of the level without fully filling the order
                    // remove this level from the orderbook
                    filled_orders.append(&mut level.orders);
                    levels.pop();
                }
                _ => break,
            }
        }

        order.filled_quote_size = order.quote_size - remaining_quote_amount;

        // Return execution results for clearinghouse to settle
        return LimitFillResult {
            filled_orders,
            counterparty_partial_fill,
            order_id: order.common.id,
            amount_out,
            amount_in,
            asset_in,
            asset_out,
            filled_size: order.filled_quote_size,
        };
    }

    pub fn execute_market_buy_in_base_order(
        levels: &mut Vec<Level>,
        order: MarketBuyOrder,
    ) -> MarketOrderMatchingResults {
        let mut filled_orders: Vec<LimitOrder> = vec![];

        // We can have at most 1 partially filled order for the counter_party
        // which will be the first element in the best price level
        let mut counterparty_partial_fill: Option<CounterPartyPartialFill> = None;
        let mut quote_amount_in: u64 = 0;

        let mut remaining_base_amount = order.base_size;
        while !levels.is_empty() && remaining_base_amount > 0 {
            let level = levels.last_mut();
            let Some(level) = level else {
                break;
            };

            let mut to_drain_end_index = 0;
            let level_price = level.price;
            for order in level.orders.iter_mut() {
                let order_quote_remaining = order.quote_size - order.filled_quote_size;
                let order_base_remaining = quote_to_base(order_quote_remaining, level_price);

                let filled_base_amount = remaining_base_amount.min(order_base_remaining);
                remaining_base_amount -= filled_base_amount;

                let filled_quote_amount = base_to_quote(filled_base_amount, level_price);
                quote_amount_in += filled_quote_amount;
                // Don't modify the order's filled amount here as we are using it
                // to determine the filled amount when settling the order

                if filled_base_amount == order_base_remaining {
                    to_drain_end_index += 1;
                }

                if remaining_base_amount <= 0 {
                    if filled_base_amount < order_base_remaining {
                        let filled_quote_amount = base_to_quote(filled_base_amount, level_price);
                        counterparty_partial_fill = Some(CounterPartyPartialFill {
                            order_id: order.common.id,
                            order_price: level_price,
                            account_public_key: order.common.account,
                            filled_quote_amount,
                        });
                        order.filled_quote_size += filled_quote_amount;
                    }
                    break;
                }
            }

            if to_drain_end_index < level.orders.len() {
                filled_orders.append(&mut level.orders.drain(0..to_drain_end_index).collect());
                break;
            }
            // reached the end of the level without fully filling the order
            // remove this level from the orderbook
            filled_orders.append(&mut level.orders);
            levels.pop();
        }

        // Return execution results for clearinghouse to settle
        return MarketOrderMatchingResults::BuyInBase {
            base_filled_amount: order.base_size - remaining_base_amount,
            quote_amount_in,
            filled_orders,
            counterparty_partial_fill,
            order_id: order.common.id,
        };
    }

    /// Denominated in quote/base price
    pub fn execute_market_sell_quote_order(
        levels: &mut Vec<Level>,
        order: MarketSellOrder,
    ) -> MarketOrderMatchingResults {
        let mut filled_orders: Vec<LimitOrder> = vec![];

        // We can have at most 1 partially filled order for the counter_party
        // which will be the first element in the best price level
        let mut counterparty_partial_fill: Option<CounterPartyPartialFill> = None;
        let mut base_amount_in: u64 = 0;

        let mut remaining_quote_amount = order.quote_size;
        while !levels.is_empty() && remaining_quote_amount > 0 {
            let level = levels.last_mut();
            let Some(level) = level else {
                break;
            };
            let level_price = level.price;

            let mut to_drain_end_index = 0;
            for order in level.orders.iter_mut() {
                let order_remaining = order.quote_size - order.filled_quote_size;
                let filled_quote_amount = remaining_quote_amount.min(order_remaining);
                remaining_quote_amount -= filled_quote_amount;
                // Don't modify the order's filled amount here as we are using it
                // to determine the filled amount when settling the order

                base_amount_in += quote_to_base(filled_quote_amount, level_price);

                if filled_quote_amount == order_remaining {
                    to_drain_end_index += 1;
                }

                if remaining_quote_amount <= 0 {
                    if filled_quote_amount < order_remaining {
                        counterparty_partial_fill = Some(CounterPartyPartialFill {
                            order_id: order.common.id,
                            order_price: level_price,
                            account_public_key: order.common.account,
                            filled_quote_amount,
                        });
                        order.filled_quote_size += filled_quote_amount;
                    }
                    break;
                }
            }

            if to_drain_end_index < level.orders.len() {
                filled_orders.append(&mut level.orders.drain(0..to_drain_end_index).collect());
                break;
            }
            // reached the end of the level without fully filling the order
            // remove this level from the orderbook
            filled_orders.append(&mut level.orders);
            levels.pop();
        }

        // Return execution results for clearinghouse to settle
        return MarketOrderMatchingResults::SellInQuote {
            filled_orders,
            counterparty_partial_fill,
            quote_filled_amount: order.quote_size - remaining_quote_amount,
            base_amount_in,
            order_id: order.common.id,
        };
    }

    pub fn handle_market_order(&mut self, order: MarketOrder) -> MarketOrderMatchingResults {
        match order {
            MarketOrder::Sell(sell_order) => {
                Self::execute_market_sell_quote_order(&mut self.bids_levels, sell_order)
            }
            MarketOrder::Buy(buy_order) => {
                Self::execute_market_buy_in_base_order(&mut self.asks_levels, buy_order)
            }
        }
    }

    pub fn add_limit_order(
        &mut self,
        mut order: LimitOrder,
        base_asset: AssetId,
        quote_asset: AssetId,
    ) -> Option<LimitFillResult> {
        match order.common.direction {
            OrderDirection::Buy => {
                let best_ask_price = self.get_best_prices().1;

                let Some(best_ask_price) = best_ask_price else {
                    self.add_bid(order);
                    return None;
                };

                if best_ask_price < order.price {
                    // Attempt to execute order at a better price

                    let result = Self::execute_limit(
                        &mut self.asks_levels,
                        &mut order,
                        quote_asset,
                        base_asset,
                        true,
                        |a, b| b.partial_cmp(&a).unwrap(),
                    );

                    // Determine whether we need to add the order
                    if order.filled_quote_size < order.quote_size {
                        self.add_bid(order);
                    }
                    return Some(result);
                }

                self.add_bid(order);
                return None;
            }
            OrderDirection::Sell => {
                let best_bid_price = self.get_best_prices().0;

                let Some(best_bid_price) = best_bid_price else {
                    self.add_ask(order);
                    return None;
                };

                if best_bid_price > order.price {
                    // Attempt to execute order at a better price
                    let result = Self::execute_limit(
                        &mut self.bids_levels,
                        &mut order,
                        base_asset,
                        quote_asset,
                        false,
                        |a, b| a.partial_cmp(&b).unwrap(),
                    );
                    // Determine whether we need to add the order
                    if order.filled_quote_size < order.quote_size {
                        self.add_ask(order);
                    }
                    return Some(result);
                }

                self.add_ask(order);
                return None;
            }
        }
    }

    pub fn cancel_order(&mut self, order: &LimitOrder) {
        match order.common.direction {
            OrderDirection::Buy => self.cancel_bid(order),
            OrderDirection::Sell => self.cancel_ask(order),
        }
    }

    pub fn get_best_prices(&self) -> (Option<u64>, Option<u64>) {
        let best_bid = self.bids_levels.last().map(|level| level.price);
        let best_ask = self.asks_levels.last().map(|level| level.price);

        (best_bid, best_ask)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        state::order::{CommonOrderFields, LimitOrder, OrderDirection, OrderId, OrderStatus},
        types::transaction::PublicKeyHash,
    };

    fn make_order(price: u64, size: u64, direction: OrderDirection, id: OrderId) -> LimitOrder {
        LimitOrder {
            price,
            quote_size: size,
            filled_quote_size: 0,
            common: CommonOrderFields {
                id,
                market_id: 0,
                status: OrderStatus::Open,
                account: PublicKeyHash::default(),
                direction,
            },
        }
    }
    fn make_market_buy_order(id: OrderId, base_size: u64) -> MarketOrder {
        MarketOrder::Buy(MarketBuyOrder {
            base_size,
            filled_size: 0,
            average_execution_price: 0,
            common: CommonOrderFields {
                id,
                market_id: 0,
                status: OrderStatus::Open,
                account: PublicKeyHash::default(),
                direction: OrderDirection::Buy,
            },
        })
    }

    fn make_market_sell_order(id: OrderId, quote_size: u64) -> MarketOrder {
        MarketOrder::Sell(MarketSellOrder {
            quote_size,
            filled_size: 0,
            average_execution_price: 0,
            common: CommonOrderFields {
                id,
                market_id: 0,
                status: OrderStatus::Open,
                account: PublicKeyHash::default(),
                direction: OrderDirection::Sell,
            },
        })
    }

    impl SpotMarket {
        fn add_limit_order_helper(&mut self, order: LimitOrder) {
            self.add_limit_order(order, 0, 1);
        }
    }

    #[test]
    fn test_add_bid_order_inserts_correctly() {
        let mut market = SpotMarket {
            bids_levels: vec![],
            asks_levels: vec![],
            market_id: 0,
            asset_one: 0,
            asset_two: 1,
            base_asset: 0,
            quote_asset: 1,
        };

        market.add_limit_order_helper(make_order(100, 10, OrderDirection::Buy, 1));
        market.add_limit_order_helper(make_order(105, 5, OrderDirection::Buy, 2));
        market.add_limit_order_helper(make_order(103, 7, OrderDirection::Buy, 3));
        market.add_limit_order_helper(make_order(1, 7, OrderDirection::Buy, 4));

        assert_eq!(market.bids_levels.len(), 4);
        assert_eq!(market.bids_levels.last().unwrap().price, 105);
        assert_eq!(market.bids_levels.first().unwrap().price, 1);
        assert_eq!(market.get_best_prices().0, Some(105));
    }

    #[test]
    fn test_add_ask_order_inserts_correctly() {
        let mut market = SpotMarket {
            bids_levels: vec![],
            asks_levels: vec![],
            market_id: 0,
            asset_one: 0,
            asset_two: 1,
            base_asset: 0,
            quote_asset: 1,
        };

        market.add_limit_order_helper(make_order(110, 8, OrderDirection::Sell, 1));
        market.add_limit_order_helper(make_order(1000, 8, OrderDirection::Sell, 2));
        market.add_limit_order_helper(make_order(1000, 10, OrderDirection::Sell, 5));
        market.add_limit_order_helper(make_order(107, 6, OrderDirection::Sell, 3));
        market.add_limit_order_helper(make_order(109, 4, OrderDirection::Sell, 4));

        assert_eq!(market.asks_levels.len(), 4);
        assert_eq!(market.asks_levels.last().unwrap().price, 107);
        assert_eq!(market.asks_levels.first().unwrap().price, 1000);
        assert_eq!(market.get_best_prices().1, Some(107));

        assert_eq!(market.asks_levels[0].volume, 18); // price 1000
    }

    #[test]
    fn test_order_aggregation_on_same_price() {
        let mut market = SpotMarket {
            bids_levels: vec![],
            asks_levels: vec![],
            market_id: 0,
            asset_one: 0,
            asset_two: 1,
            base_asset: 0,
            quote_asset: 1,
        };

        market.add_limit_order_helper(make_order(100, 10, OrderDirection::Buy, 1));
        market.add_limit_order_helper(make_order(100, 15, OrderDirection::Buy, 2));
        assert_eq!(market.bids_levels.len(), 1);
        assert_eq!(market.bids_levels[0].volume, 25);
        assert_eq!(market.bids_levels[0].orders.len(), 2);
    }

    #[test]
    fn test_get_best_prices_returns_none_when_empty() {
        let market = SpotMarket {
            bids_levels: vec![],
            asks_levels: vec![],
            market_id: 0,
            asset_one: 0,
            asset_two: 1,
            base_asset: 0,
            quote_asset: 1,
        };

        assert_eq!(market.get_best_prices(), (None, None));
    }

    #[test]
    fn test_cancels_ask_order_correctly() {
        let mut market = SpotMarket {
            bids_levels: vec![],
            asks_levels: vec![],
            market_id: 0,
            asset_one: 0,
            asset_two: 1,
            base_asset: 0,
            quote_asset: 1,
        };

        market.add_limit_order_helper(make_order(1000, 8, OrderDirection::Sell, 2));
        market.add_limit_order_helper(make_order(107, 6, OrderDirection::Sell, 3));
        market.add_limit_order_helper(make_order(110, 8, OrderDirection::Sell, 1));
        market.add_limit_order_helper(make_order(109, 4, OrderDirection::Sell, 4));
        market.add_limit_order_helper(make_order(1000, 10, OrderDirection::Sell, 5));
        market.add_limit_order_helper(make_order(1000, 9, OrderDirection::Sell, 6));
        market.add_limit_order_helper(make_order(1000, 19, OrderDirection::Sell, 7));
        assert_eq!(market.asks_levels.len(), 4);
        assert_eq!(market.bids_levels.len(), 0);

        market.cancel_order(&make_order(1000, 8, OrderDirection::Sell, 5));
        let price_level = &market.asks_levels[0];
        println!("{:?}", price_level.orders);
        assert_eq!(price_level.orders.len(), 4);
        assert_eq!(price_level.orders[1].common.status, OrderStatus::Cancelled);
        assert_eq!(price_level.cancelled, 1);
    }

    #[test]
    fn test_cancels_buy_order_correctly() {
        let mut market = SpotMarket {
            bids_levels: vec![],
            asks_levels: vec![],
            market_id: 0,
            asset_one: 0,
            asset_two: 1,
            base_asset: 0,
            quote_asset: 1,
        };

        market.add_limit_order_helper(make_order(1, 8, OrderDirection::Buy, 1));
        market.add_limit_order_helper(make_order(3, 8, OrderDirection::Buy, 2));
        market.add_limit_order_helper(make_order(4, 6, OrderDirection::Buy, 3));
        market.add_limit_order_helper(make_order(3, 4, OrderDirection::Buy, 4));
        market.add_limit_order_helper(make_order(8, 10, OrderDirection::Buy, 5));
        market.add_limit_order_helper(make_order(3, 9, OrderDirection::Buy, 6));
        assert_eq!(market.bids_levels.len(), 4);
        assert_eq!(market.asks_levels.len(), 0);

        market.cancel_order(&make_order(3, 9, OrderDirection::Buy, 6));
        let price_level = &market.bids_levels[1];
        assert_eq!(price_level.orders[2].common.status, OrderStatus::Cancelled);
        assert_eq!(price_level.cancelled, 1);
    }

    #[test]
    fn test_prunes_cancelled_orders_correctly() {
        let mut market = SpotMarket {
            bids_levels: vec![],
            asks_levels: vec![],
            market_id: 0,
            asset_one: 0,
            asset_two: 1,
            base_asset: 0,
            quote_asset: 1,
        };

        let order_1 = make_order(1, 8, OrderDirection::Buy, 1);
        let order_2 = make_order(3, 8, OrderDirection::Buy, 2);
        let order_3 = make_order(1, 6, OrderDirection::Buy, 3);
        let order_4 = make_order(1, 4, OrderDirection::Buy, 4);
        let order_5 = make_order(1, 10, OrderDirection::Buy, 5);
        let order_6 = make_order(1, 9, OrderDirection::Buy, 6);

        market.add_limit_order_helper(order_1.clone());
        market.add_limit_order_helper(order_2.clone());
        market.add_limit_order_helper(order_3.clone());
        market.add_limit_order_helper(order_4.clone());
        market.add_limit_order_helper(order_5.clone());
        market.add_limit_order_helper(order_6.clone());

        assert_eq!(market.bids_levels.len(), 2);
        assert_eq!(market.asks_levels.len(), 0);

        let mut expected_level_volume = 37;
        assert_eq!(market.bids_levels[0].volume, expected_level_volume);

        market.cancel_order(&order_1);
        expected_level_volume -= order_1.quote_size;
        assert_eq!(market.bids_levels[0].cancelled, 1);
        assert_eq!(market.bids_levels[0].orders.len(), 5);
        assert_eq!(market.bids_levels[0].volume, expected_level_volume);

        // try cancel again
        market.cancel_order(&order_1);
        assert_eq!(market.bids_levels[0].cancelled, 1);
        assert_eq!(market.bids_levels[0].orders.len(), 5);
        assert_eq!(market.bids_levels[0].volume, expected_level_volume);

        market.cancel_order(&order_5);
        expected_level_volume -= order_5.quote_size;
        assert_eq!(market.bids_levels[0].cancelled, 2);
        assert_eq!(market.bids_levels[0].orders.len(), 5);
        assert_eq!(market.bids_levels[0].volume, expected_level_volume);

        // should prune here
        market.cancel_order(&make_order(1, 9, OrderDirection::Buy, 6));
        expected_level_volume -= order_6.quote_size;
        assert_eq!(market.bids_levels[0].cancelled, 0);
        assert_eq!(market.bids_levels[0].orders.len(), 2);
        assert_eq!(market.bids_levels[0].volume, expected_level_volume);
    }
}
