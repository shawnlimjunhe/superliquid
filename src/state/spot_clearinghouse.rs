use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{config, state::order::OrderDirection, types::transaction::PublicKeyHash};

use super::{
    asset::AssetId,
    order::{Order, OrderPrice, OrderStatus},
};

pub type MarketId = u32;
type MarketIdCounter = MarketId;

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
}

pub struct Level {
    pub price: u64,
    pub volume: u32,
    pub orders: Vec<Order>,
    pub cancelled: u32,
}
pub struct SpotMarket {
    pub market_id: MarketId,
    pub asset_one: AssetId,
    pub asset_two: AssetId,
    // pub tick_size: (),
    // pub lot_size: (),

    // levels are in reverse order, best prices are at the end
    pub bids_levels: Vec<Level>, // 0, 1, 2, ..
    pub asks_levels: Vec<Level>, // 10, 9, 8, ..
}

impl SpotMarket {
    fn new(market_id: MarketId, asset_one: AssetId, asset_two: AssetId) -> Self {
        Self {
            market_id,
            asset_one,
            asset_two,
            bids_levels: vec![],
            asks_levels: vec![],
        }
    }

    fn add_order_with_cmp<F>(levels: &mut Vec<Level>, order: Order, mut compare: F)
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
                levels[mid].volume += order.size;
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
                volume: order.size,
                orders: vec![order],
                cancelled: 0,
            },
        )
    }

    fn mark_order_as_cancelled(orders: &mut Vec<Order>, order: &Order) -> bool {
        let order_id = order.id;
        let mut left = 0;
        let mut right = orders.len();
        while left < right {
            let mid = left + (right - left) / 2;
            let mid_id = orders[mid].id;

            if order_id == mid_id {
                if orders[mid].status == OrderStatus::Cancelled {
                    return false;
                }
                orders[mid].status = OrderStatus::Cancelled;
                return true;
            } else if order_id < mid_id {
                right = mid;
            } else {
                left = mid + 1;
            }
        }
        return false;
    }

    fn cancel_order_with_cmp<F>(levels: &mut Vec<Level>, order: &Order, mut compare: F)
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
                let unfilled_size = order.size - order.filled_size;
                level.volume -= unfilled_size;

                if level.cancelled <= (level.orders.len() / 2) as u32 {
                    return;
                }
                // prune when vector is sparse enough
                level.orders.retain(|o| o.status != OrderStatus::Cancelled);
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

    pub fn add_bid(&mut self, order: Order) {
        Self::add_order_with_cmp(&mut self.bids_levels, order, |a, b| {
            a.partial_cmp(&b).unwrap()
        });
    }

    pub fn add_ask(&mut self, order: Order) {
        Self::add_order_with_cmp(&mut self.asks_levels, order, |a, b| {
            b.partial_cmp(&a).unwrap()
        });
    }

    pub fn cancel_bid(&mut self, order: &Order) {
        Self::cancel_order_with_cmp(&mut self.bids_levels, order, |a, b| {
            a.partial_cmp(&b).unwrap()
        });
    }

    pub fn cancel_ask(&mut self, order: &Order) {
        Self::cancel_order_with_cmp(&mut self.asks_levels, order, |a, b| {
            b.partial_cmp(&a).unwrap()
        });
    }

    pub fn execute_order<F>(
        levels: &mut Vec<Level>,
        order: Order,
        asset_to_give: AssetId,
        asset_to_recieve: AssetId,
        mut compare: F,
    ) where
        F: FnMut(OrderPrice, OrderPrice) -> std::cmp::Ordering,
    {
        let mut filled_orders: Vec<Order> = vec![];
        // We can have at most 1 partially filled order for the counter_party
        let mut counter_party_partially_filled_order_size: u32 = 0;
        let mut amount_to_give: u64 = 0;

        // todo: keep track of executed price;
        let mut remaining = order.size;

        while !levels.is_empty() {
            let level = levels.last_mut();
            let Some(level) = level else {
                break;
            };

            match compare(level.price, order.price) {
                std::cmp::Ordering::Less | std::cmp::Ordering::Equal => {
                    // Keep track of which index we fully fill to so we can remove them
                    let mut to_drain_end_index = 0;
                    let level_price = level.price;
                    for order in level.orders.iter_mut() {
                        let order_remaining = order.size - order.filled_size;
                        let filled = remaining.min(order_remaining);
                        remaining -= filled;
                        order.filled_size += filled;
                        amount_to_give += (filled as u64) * level_price;

                        if filled == order_remaining {
                            to_drain_end_index += 1;
                        }

                        if remaining == 0 {
                            if filled < order_remaining {
                                counter_party_partially_filled_order_size =
                                    order_remaining - filled;
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
                std::cmp::Ordering::Greater => {
                    // can no longer fill the order
                    break;
                }
            }
        }

        // Settle executed orders
        let to_give_balance =

        // Deduct amount from party
        // Transfer amount to counter parties
    }

    pub fn add_order(&mut self, order: Order) {
        let (best_bid, best_ask) = self.get_best_prices();

        match order.direction {
            OrderDirection::Buy => {
                if let Some(best_ask) = best_ask {
                    if best_ask <= order.price {
                        Self::execute_order(&mut self.asks_levels, order, |a, b| {
                            a.partial_cmp(&b).unwrap()
                        });
                        return;
                    }
                }
                self.add_bid(order)
            }
            OrderDirection::Sell => {
                if let Some(best_bid) = best_bid {
                    if best_bid >= order.price {
                        Self::execute_order(&mut self.bids_levels, order, |a, b| {
                            b.partial_cmp(&a).unwrap()
                        });
                        return;
                    }
                }
                self.add_ask(order)
            }
        }
    }

    pub fn cancel_order(&mut self, order: &Order) {
        match order.direction {
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
        state::order::{Order, OrderDirection, OrderId, OrderStatus},
        types::transaction::PublicKeyHash,
    };

    fn make_order(price: u64, size: u32, direction: OrderDirection, id: OrderId) -> Order {
        Order {
            price,
            size,
            direction,
            id,
            filled_size: 0,
            status: OrderStatus::Open,
            account: PublicKeyHash::default(),
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
        };

        market.add_order(make_order(100, 10, OrderDirection::Buy, 1));
        market.add_order(make_order(105, 5, OrderDirection::Buy, 2));
        market.add_order(make_order(103, 7, OrderDirection::Buy, 3));
        market.add_order(make_order(1, 7, OrderDirection::Buy, 4));

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
        };

        market.add_order(make_order(110, 8, OrderDirection::Sell, 1));
        market.add_order(make_order(1000, 8, OrderDirection::Sell, 2));
        market.add_order(make_order(1000, 10, OrderDirection::Sell, 5));
        market.add_order(make_order(107, 6, OrderDirection::Sell, 3));
        market.add_order(make_order(109, 4, OrderDirection::Sell, 4));

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
        };

        market.add_order(make_order(100, 10, OrderDirection::Buy, 1));
        market.add_order(make_order(100, 15, OrderDirection::Buy, 2));
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
        };

        market.add_order(make_order(110, 8, OrderDirection::Sell, 1));
        market.add_order(make_order(1000, 8, OrderDirection::Sell, 2));
        market.add_order(make_order(107, 6, OrderDirection::Sell, 3));
        market.add_order(make_order(109, 4, OrderDirection::Sell, 4));
        market.add_order(make_order(1000, 10, OrderDirection::Sell, 5));
        market.add_order(make_order(1000, 9, OrderDirection::Sell, 6));
        market.add_order(make_order(1000, 19, OrderDirection::Sell, 7));
        assert_eq!(market.asks_levels.len(), 4);
        assert_eq!(market.bids_levels.len(), 0);

        market.cancel_order(&make_order(1000, 8, OrderDirection::Sell, 5));
        let price_level = &market.asks_levels[0];
        println!("{:?}", price_level.orders);
        assert_eq!(price_level.orders.len(), 4);
        assert_eq!(price_level.orders[1].status, OrderStatus::Cancelled);
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
        };

        market.add_order(make_order(1, 8, OrderDirection::Buy, 1));
        market.add_order(make_order(3, 8, OrderDirection::Buy, 2));
        market.add_order(make_order(4, 6, OrderDirection::Buy, 3));
        market.add_order(make_order(3, 4, OrderDirection::Buy, 4));
        market.add_order(make_order(8, 10, OrderDirection::Buy, 5));
        market.add_order(make_order(3, 9, OrderDirection::Buy, 6));
        assert_eq!(market.bids_levels.len(), 4);
        assert_eq!(market.asks_levels.len(), 0);

        market.cancel_order(&make_order(3, 9, OrderDirection::Buy, 6));
        let price_level = &market.bids_levels[1];
        assert_eq!(price_level.orders[2].status, OrderStatus::Cancelled);
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
        };

        let order_1 = make_order(1, 8, OrderDirection::Buy, 1);
        let order_2 = make_order(3, 8, OrderDirection::Buy, 2);
        let order_3 = make_order(1, 6, OrderDirection::Buy, 3);
        let order_4 = make_order(1, 4, OrderDirection::Buy, 4);
        let order_5 = make_order(1, 10, OrderDirection::Buy, 5);
        let order_6 = make_order(1, 9, OrderDirection::Buy, 6);

        market.add_order(order_1.clone());
        market.add_order(order_2.clone());
        market.add_order(order_3.clone());
        market.add_order(order_4.clone());
        market.add_order(order_5.clone());
        market.add_order(order_6.clone());

        assert_eq!(market.bids_levels.len(), 2);
        assert_eq!(market.asks_levels.len(), 0);

        let mut expected_level_volume = 37;
        assert_eq!(market.bids_levels[0].volume, expected_level_volume);

        market.cancel_order(&order_1);
        expected_level_volume -= order_1.size;
        assert_eq!(market.bids_levels[0].cancelled, 1);
        assert_eq!(market.bids_levels[0].orders.len(), 5);
        assert_eq!(market.bids_levels[0].volume, expected_level_volume);

        // try cancel again
        market.cancel_order(&order_1);
        assert_eq!(market.bids_levels[0].cancelled, 1);
        assert_eq!(market.bids_levels[0].orders.len(), 5);
        assert_eq!(market.bids_levels[0].volume, expected_level_volume);

        market.cancel_order(&order_5);
        expected_level_volume -= order_5.size;
        assert_eq!(market.bids_levels[0].cancelled, 2);
        assert_eq!(market.bids_levels[0].orders.len(), 5);
        assert_eq!(market.bids_levels[0].volume, expected_level_volume);

        // should prune here
        market.cancel_order(&make_order(1, 9, OrderDirection::Buy, 6));
        expected_level_volume -= order_6.size;
        assert_eq!(market.bids_levels[0].cancelled, 0);
        assert_eq!(market.bids_levels[0].orders.len(), 2);
        assert_eq!(market.bids_levels[0].volume, expected_level_volume);
    }
}
