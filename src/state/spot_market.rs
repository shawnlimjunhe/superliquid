use super::{
    asset::AssetId,
    order::{
        LimitFillResult, LimitOrder, MarketBuyOrder, MarketOrder, MarketOrderMatchingResults,
        MarketSellOrder, OrderDirection, OrderPriceMultiple, OrderStatus, ResidualOrder,
        UserExecutionResult,
    },
    spot_clearinghouse::{MarketId, MarketPrecision, base_to_quote_lots},
};

#[derive(Debug)]
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
    pub tick: u32,
    pub tick_decimals: u8,
    // pub lot_size: (),

    // levels are in reverse order, best prices are at the end
    pub bids_levels: Vec<Level>, // 0, 1, 2, ..
    pub asks_levels: Vec<Level>, // 10, 9, 8, ..
}

impl SpotMarket {
    pub(crate) fn new(
        market_id: MarketId,
        base_asset: AssetId,
        quote_asset: AssetId,
        tick: u32,
        tick_decimals: u8,
    ) -> Self {
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
            tick,
            tick_decimals,
        }
    }

    fn add_order_with_cmp<F>(levels: &mut Vec<Level>, order: LimitOrder, mut compare: F)
    where
        F: FnMut(OrderPriceMultiple, OrderPriceMultiple) -> std::cmp::Ordering,
    {
        let price = order.price_multiple;
        let mut left = 0;
        let mut right = levels.len();

        while left < right {
            let mid = left + (right - left) / 2;
            let mid_price = levels[mid].price;

            if price == mid_price {
                levels[mid].volume += order.base_lots - order.filled_base_lots;
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
                volume: order.base_lots - order.filled_base_lots,
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
        F: FnMut(OrderPriceMultiple, OrderPriceMultiple) -> std::cmp::Ordering,
    {
        let price = order.price_multiple;
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
                let unfilled_size = order.base_lots - order.filled_base_lots;
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
        precision: &MarketPrecision,
        mut compare: F,
    ) -> LimitFillResult
    where
        F: FnMut(OrderPriceMultiple, OrderPriceMultiple) -> std::cmp::Ordering,
    {
        let mut filled_orders: Vec<LimitOrder> = vec![];

        let mut residual_order: Option<ResidualOrder> = None;
        let mut lots_in: u64 = 0;
        let mut lots_out: u64 = 0;

        let order_price = order.price_multiple;
        let mut remaining_base_amount = order.base_lots;
        while !levels.is_empty() && remaining_base_amount > 0 {
            let level = levels.last_mut();
            let Some(level) = level else {
                break;
            };

            let level_price = level.price;
            let level_filled = remaining_base_amount;
            let mut cancelled_seen = 0;

            match compare(order_price, level_price) {
                std::cmp::Ordering::Less | std::cmp::Ordering::Equal => {
                    // fill the current level orders
                    let mut to_drain_end_index = 0;
                    for order in level.orders.iter_mut() {
                        if order.common.status == OrderStatus::Cancelled {
                            cancelled_seen += 1;
                            continue;
                        }

                        let order_remaining = order.base_lots - order.filled_base_lots;
                        let curr_filled_base_amount = remaining_base_amount.min(order_remaining);
                        remaining_base_amount -= curr_filled_base_amount;

                        if is_buy {
                            lots_in += curr_filled_base_amount;
                            lots_out +=
                                base_to_quote_lots(curr_filled_base_amount, level_price, precision);
                        } else {
                            lots_out += curr_filled_base_amount;
                            lots_in +=
                                base_to_quote_lots(curr_filled_base_amount, level_price, precision);
                        }

                        to_drain_end_index += 1;
                        if curr_filled_base_amount == order_remaining {
                            // Include current index
                            to_drain_end_index += 1;
                        }

                        if remaining_base_amount <= 0 {
                            if curr_filled_base_amount < order_remaining {
                                // Partial fill
                                residual_order = Some(ResidualOrder {
                                    order_id: order.common.id,
                                    price_multiple: level_price,
                                    account_public_key: order.common.account,
                                    filled_base_lots: curr_filled_base_amount,
                                });
                                order.filled_base_lots += curr_filled_base_amount;
                            }
                            break;
                        }
                    }

                    if to_drain_end_index < level.orders.len() {
                        filled_orders
                            .append(&mut level.orders.drain(0..to_drain_end_index).collect());
                        level.volume -= level_filled;
                        level.cancelled -= cancelled_seen;
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

        order.filled_base_lots = order.base_lots - remaining_base_amount;

        // Return execution results for clearinghouse to settle
        return LimitFillResult {
            filled_orders,
            residual_order,
            user_order: UserExecutionResult {
                order_id: order.common.id,
                lots_out,
                lots_in,
                asset_in,
                asset_out,
                filled_size: order.filled_base_lots,
            },
        };
    }

    pub fn execute_market_buy_in_base_order(
        levels: &mut Vec<Level>,
        order: MarketBuyOrder,
        precision: &MarketPrecision,
    ) -> MarketOrderMatchingResults {
        let mut filled_orders: Vec<LimitOrder> = vec![];

        // We can have at most 1 partially filled order for the counter_party
        // which will be the first element in the best price level
        let mut maker_partial_fill: Option<ResidualOrder> = None;
        let mut quote_amount_in: u64 = 0;

        let mut remaining_base_amount = order.quote_size;
        while !levels.is_empty() && remaining_base_amount > 0 {
            let level = levels.last_mut();
            let Some(level) = level else {
                break;
            };

            let level_filled = quote_amount_in;
            let level_price = level.price;
            let mut level_cancelled = 0;

            let mut to_drain_end_index = 0;
            for order in level.orders.iter_mut() {
                if order.common.status == OrderStatus::Cancelled {
                    level_cancelled += 1;
                    continue;
                }

                let order_quote_remaining = order.base_lots - order.filled_base_lots;
                let order_base_remaining =
                    base_to_quote_lots(order_quote_remaining, level_price, precision);

                let filled_base_amount = remaining_base_amount.min(order_base_remaining);
                remaining_base_amount -= filled_base_amount;

                let filled_quote_amount =
                    base_to_quote_lots(filled_base_amount, level_price, precision);
                quote_amount_in += filled_quote_amount;
                // Don't modify the order's filled amount here as we are using it
                // to determine the filled amount when settling the order

                to_drain_end_index += 1;
                if filled_base_amount == order_base_remaining {
                    // Include the current index
                    to_drain_end_index += 1;
                }

                if remaining_base_amount <= 0 {
                    if filled_base_amount < order_base_remaining {
                        // Partial fill
                        let filled_quote_amount =
                            base_to_quote_lots(filled_base_amount, level_price, precision);
                        maker_partial_fill = Some(ResidualOrder {
                            order_id: order.common.id,
                            price_multiple: level_price,
                            account_public_key: order.common.account,
                            filled_base_lots: filled_quote_amount,
                        });
                        order.filled_base_lots += filled_quote_amount;
                    }
                    break;
                }
            }

            if to_drain_end_index < level.orders.len() {
                filled_orders.append(&mut level.orders.drain(0..to_drain_end_index).collect());
                level.volume -= level_filled;
                level.cancelled -= level_cancelled;
                break;
            }
            // reached the end of the level without fully filling the order
            // remove this level from the orderbook
            filled_orders.append(&mut level.orders);
            levels.pop();
        }

        // Return execution results for clearinghouse to settle
        return MarketOrderMatchingResults::BuyInBase {
            quote_filled_amount: order.quote_size - remaining_base_amount,
            base_lots_in: quote_amount_in,
            filled_orders,
            counterparty_partial_fill: maker_partial_fill,
            order_id: order.common.id,
        };
    }

    /// Denominated in quote/base price
    pub fn execute_market_sell_quote_order(
        levels: &mut Vec<Level>,
        order: MarketSellOrder,
        precision: &MarketPrecision,
    ) -> MarketOrderMatchingResults {
        let mut filled_orders: Vec<LimitOrder> = vec![];

        // We can have at most 1 partially filled order for the counter_party
        // which will be the first element in the best price level
        let mut maker_partial_fill: Option<ResidualOrder> = None;
        let mut base_amount_in: u64 = 0;

        let mut remaining_quote_amount = order.base_size;
        while !levels.is_empty() && remaining_quote_amount > 0 {
            let level = levels.last_mut();
            let Some(level) = level else {
                break;
            };
            let level_price = level.price;
            let level_filled = remaining_quote_amount;
            let mut cancelled_seen = 0;

            let mut to_drain_end_index = 0;
            for order in level.orders.iter_mut() {
                if order.common.status == OrderStatus::Cancelled {
                    cancelled_seen += 1;
                    continue;
                }
                let order_remaining = order.base_lots - order.filled_base_lots;
                let filled_quote_amount = remaining_quote_amount.min(order_remaining);
                remaining_quote_amount -= filled_quote_amount;
                // Don't modify the order's filled amount here as we are using it
                // to determine the filled amount when settling the order

                base_amount_in += base_to_quote_lots(filled_quote_amount, level_price, precision);

                to_drain_end_index += 1;
                if filled_quote_amount == order_remaining {
                    // include current index
                    to_drain_end_index += 1;
                }

                if remaining_quote_amount <= 0 {
                    if filled_quote_amount < order_remaining {
                        // Partial fill
                        maker_partial_fill = Some(ResidualOrder {
                            order_id: order.common.id,
                            price_multiple: level_price,
                            account_public_key: order.common.account,
                            filled_base_lots: filled_quote_amount,
                        });
                        order.filled_base_lots += filled_quote_amount;
                    }
                    break;
                }
            }

            if to_drain_end_index < level.orders.len() {
                filled_orders.append(&mut level.orders.drain(0..to_drain_end_index).collect());
                level.volume -= level_filled;
                level.cancelled -= cancelled_seen;
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
            counterparty_partial_fill: maker_partial_fill,
            base_filled_lots: order.base_size - remaining_quote_amount,
            quote_lots_in: base_amount_in,
            order_id: order.common.id,
        };
    }

    pub fn handle_market_order(
        &mut self,
        order: MarketOrder,
        precision: &MarketPrecision,
    ) -> MarketOrderMatchingResults {
        match order {
            MarketOrder::Sell(sell_order) => {
                Self::execute_market_sell_quote_order(&mut self.bids_levels, sell_order, precision)
            }
            MarketOrder::Buy(buy_order) => {
                Self::execute_market_buy_in_base_order(&mut self.asks_levels, buy_order, precision)
            }
        }
    }

    pub fn add_limit_order(
        &mut self,
        mut order: LimitOrder,
        base_asset: AssetId,
        quote_asset: AssetId,
        precision: &MarketPrecision,
    ) -> Option<LimitFillResult> {
        match order.common.direction {
            OrderDirection::Buy => {
                let best_ask_price = self.get_best_prices().1;

                let Some(best_ask_price) = best_ask_price else {
                    self.add_bid(order);
                    return None;
                };

                if best_ask_price <= order.price_multiple {
                    // Attempt to execute order at a better price

                    let result = Self::execute_limit(
                        &mut self.asks_levels,
                        &mut order,
                        base_asset,
                        quote_asset,
                        true,
                        precision,
                        |a, b| b.partial_cmp(&a).unwrap(),
                    );

                    // Determine whether we need to add the order
                    if order.filled_base_lots < order.base_lots {
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

                if best_bid_price >= order.price_multiple {
                    // Attempt to execute order at a better price
                    let result = Self::execute_limit(
                        &mut self.bids_levels,
                        &mut order,
                        quote_asset,
                        base_asset,
                        false,
                        precision,
                        |a, b| a.partial_cmp(&b).unwrap(),
                    );
                    // Determine whether we need to add the order
                    if order.filled_base_lots < order.base_lots {
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

    fn make_order(
        price_tick: u64,
        lot_size: u64,
        direction: OrderDirection,
        id: OrderId,
    ) -> LimitOrder {
        LimitOrder {
            price_multiple: price_tick,
            base_lots: lot_size,
            filled_base_lots: 0,
            common: CommonOrderFields {
                id,
                market_id: 0,
                status: OrderStatus::Open,
                account: PublicKeyHash::default(),
                direction,
            },
        }
    }
    fn make_market_buy_order(id: OrderId, quote_size: u64) -> MarketOrder {
        MarketOrder::Buy(MarketBuyOrder {
            quote_size,
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

    fn make_market_sell_order(id: OrderId, base_size: u64) -> MarketOrder {
        MarketOrder::Sell(MarketSellOrder {
            base_size,
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
        fn add_limit_order_helper(&mut self, order: LimitOrder, precision: &MarketPrecision) {
            self.add_limit_order(order, 0, 1, precision);
        }

        fn test_new(tick: u32, tick_decimals: u8) -> Self {
            Self {
                bids_levels: vec![],
                asks_levels: vec![],
                market_id: 0,
                asset_one: 0,
                asset_two: 1,
                base_asset: 0,
                quote_asset: 1,
                tick_decimals,
                tick,
            }
        }
    }

    mod test_limit_orders {
        use crate::state::{
            order::{OrderDirection, OrderStatus},
            spot_clearinghouse::MarketPrecision,
            spot_market::{SpotMarket, tests::make_order},
        };

        #[test]
        fn test_add_bid_order_inserts_correctly() {
            let mut market = SpotMarket::test_new(100, 2);

            let precision = MarketPrecision {
                base_lot_size: 100,
                quote_lot_size: 100,
                tick: market.tick,
                tick_decimals: market.tick_decimals,
            };

            market.add_limit_order_helper(make_order(100, 10, OrderDirection::Buy, 1), &precision);
            market.add_limit_order_helper(make_order(105, 5, OrderDirection::Buy, 2), &precision);
            market.add_limit_order_helper(make_order(103, 7, OrderDirection::Buy, 3), &precision);
            market.add_limit_order_helper(make_order(1, 7, OrderDirection::Buy, 4), &precision);

            assert_eq!(market.bids_levels.len(), 4);
            assert_eq!(market.bids_levels.last().unwrap().price, 105);
            assert_eq!(market.bids_levels.first().unwrap().price, 1);
            assert_eq!(market.get_best_prices().0, Some(105));
        }

        #[test]
        fn test_add_ask_order_inserts_correctly() {
            let mut market = SpotMarket::test_new(100, 2);
            let precision = MarketPrecision {
                base_lot_size: 100,
                quote_lot_size: 100,
                tick: market.tick,
                tick_decimals: market.tick_decimals,
            };

            market.add_limit_order_helper(make_order(110, 8, OrderDirection::Sell, 1), &precision);
            market.add_limit_order_helper(make_order(1000, 8, OrderDirection::Sell, 2), &precision);
            market
                .add_limit_order_helper(make_order(1000, 10, OrderDirection::Sell, 5), &precision);
            market.add_limit_order_helper(make_order(107, 6, OrderDirection::Sell, 3), &precision);
            market.add_limit_order_helper(make_order(109, 4, OrderDirection::Sell, 4), &precision);

            assert_eq!(market.asks_levels.len(), 4);
            assert_eq!(market.asks_levels.last().unwrap().price, 107);
            assert_eq!(market.asks_levels.first().unwrap().price, 1000);
            assert_eq!(market.get_best_prices().1, Some(107));

            assert_eq!(market.asks_levels[0].volume, 18); // price 1000
        }

        mod test_limit_execution {
            use crate::state::{
                order::{OrderDirection, OrderStatus},
                spot_clearinghouse::MarketPrecision,
                spot_market::{SpotMarket, tests::make_order},
            };

            #[test]
            fn test_limit_buy_above_best_ask_price_fully_filled() {
                let base_asset = 0;
                let quote_asset = 1;
                let tick = 100;
                let tick_decimals = 2;
                let mut market = SpotMarket::test_new(tick, tick_decimals);
                let precision = MarketPrecision {
                    base_lot_size: 10,
                    quote_lot_size: 10,
                    tick,
                    tick_decimals,
                };

                // Asks

                market.add_limit_order_helper(
                    make_order(2_500, 1100, OrderDirection::Sell, 1),
                    &precision,
                );
                market.add_limit_order_helper(
                    make_order(2_500, 800, OrderDirection::Sell, 2),
                    &precision,
                );

                // Cancelled order
                market.add_limit_order_helper(
                    make_order(2_550, 1000, OrderDirection::Sell, 3),
                    &precision,
                );
                market.add_limit_order_helper(
                    make_order(2_550, 600, OrderDirection::Sell, 4),
                    &precision,
                );

                market.add_limit_order_helper(
                    make_order(2_600, 800, OrderDirection::Sell, 5),
                    &precision,
                );
                market.add_limit_order_helper(
                    make_order(2700, 400, OrderDirection::Sell, 6),
                    &precision,
                );

                market.cancel_order(&make_order(2_550, 1000, OrderDirection::Sell, 3));
                // Check market state before execution
                {
                    assert_eq!(market.get_best_prices().1, Some(2_500));
                    assert_eq!(market.asks_levels.len(), 4);
                    // Check state of the level that limit order will fill to
                    assert_eq!(market.asks_levels.get(2).unwrap().volume, 600);
                    assert_eq!(market.asks_levels.get(2).unwrap().cancelled, 1);
                }

                let order_lot_size = 2_000;
                let order = make_order(2_550, order_lot_size, OrderDirection::Buy, 7);
                let limit_fill_result = market.add_limit_order(order, 0, 1, &precision);

                let Some(result) = limit_fill_result else {
                    panic!("Expected to be some result");
                };

                // Check market state
                {
                    assert_eq!(market.asks_levels.len(), 3);
                    let partial_fill_level = market
                        .asks_levels
                        .last()
                        .expect("Level should not be empty");
                    assert_eq!(partial_fill_level.volume, 500);
                    assert_eq!(partial_fill_level.cancelled, 0);

                    let residual_order = partial_fill_level
                        .orders
                        .get(0)
                        .expect("Level should not be 0");

                    assert_eq!(residual_order.filled_base_lots, 100);
                    // Order should not be inserted
                    assert_eq!(market.bids_levels.len(), 0);

                    assert_eq!(market.get_best_prices(), (None, Some(2_550)));
                }

                // Check matching result
                {
                    assert_eq!(result.filled_orders.len(), 3);
                    // filled quote size should be 0 to calculate token changes
                    assert_eq!(result.filled_orders[0].filled_base_lots, 0);
                    assert_eq!(result.filled_orders[1].filled_base_lots, 0);
                    assert_eq!(
                        result.filled_orders[2].common.status,
                        OrderStatus::Cancelled
                    );

                    let Some(residual_order) = result.residual_order else {
                        panic!("Expected to have a partial fill");
                    };

                    assert_eq!(residual_order.order_id, 4);
                    assert_eq!(residual_order.filled_base_lots, 100);

                    let user_execution_result = result.user_order;

                    let expected_lots_out = 2500 * 1900 + 2550 * 100;
                    assert_eq!(user_execution_result.lots_out, expected_lots_out);

                    assert_eq!(user_execution_result.lots_in, order_lot_size);
                    assert_eq!(user_execution_result.asset_in, base_asset);
                    assert_eq!(user_execution_result.asset_out, quote_asset);
                    assert_eq!(user_execution_result.filled_size, order_lot_size);
                }
            }

            #[test]
            fn test_limit_buy_above_best_ask_price_partial_filled() {
                let base_asset = 0;
                let quote_asset = 1;
                let tick = 100;
                let tick_decimals = 2;
                let mut market = SpotMarket::test_new(tick, tick_decimals);
                let precision = MarketPrecision {
                    base_lot_size: 10,
                    quote_lot_size: 10,
                    tick,
                    tick_decimals,
                };

                // Asks

                market.add_limit_order_helper(
                    make_order(2_500, 1100, OrderDirection::Sell, 1),
                    &precision,
                );
                market.add_limit_order_helper(
                    make_order(2_500, 800, OrderDirection::Sell, 2),
                    &precision,
                );

                // Cancelled order
                market.add_limit_order_helper(
                    make_order(2_550, 1000, OrderDirection::Sell, 3),
                    &precision,
                );
                market.add_limit_order_helper(
                    make_order(2_550, 600, OrderDirection::Sell, 4),
                    &precision,
                );

                market.add_limit_order_helper(
                    make_order(2_700, 800, OrderDirection::Sell, 5),
                    &precision,
                );
                market.add_limit_order_helper(
                    make_order(2_800, 400, OrderDirection::Sell, 6),
                    &precision,
                );

                market.cancel_order(&make_order(2_550, 1000, OrderDirection::Sell, 3));
                // Check market state before execution
                {
                    assert_eq!(market.get_best_prices().1, Some(2_500));
                    assert_eq!(market.asks_levels.len(), 4);
                }

                let order = make_order(2_650, 5_000, OrderDirection::Buy, 7);
                let limit_fill_result = market.add_limit_order(order, 0, 1, &precision);

                let Some(result) = limit_fill_result else {
                    panic!("Expected to be some result");
                };

                // Check market state after execution
                {
                    assert_eq!(market.asks_levels.len(), 2);
                    let best_ask_level = market
                        .asks_levels
                        .last()
                        .expect("Level should not be empty");
                    assert_eq!(best_ask_level.price, 2_700);
                    assert_eq!(best_ask_level.volume, 800);
                    assert_eq!(best_ask_level.cancelled, 0);

                    // Order should not be inserted
                    assert_eq!(market.bids_levels.len(), 1);
                    assert_eq!(market.bids_levels[0].price, 2_650);
                    assert_eq!(market.bids_levels[0].volume, 2_500);

                    assert_eq!(market.get_best_prices(), (Some(2_650), Some(2_700)));
                }

                // Check matching result
                {
                    assert_eq!(result.filled_orders.len(), 4);
                    // filled quote size should be 0 to calculate token changes
                    assert_eq!(result.filled_orders[0].filled_base_lots, 0);
                    assert_eq!(result.filled_orders[1].filled_base_lots, 0);
                    assert_eq!(
                        result.filled_orders[2].common.status,
                        OrderStatus::Cancelled
                    );
                    assert_eq!(result.filled_orders[3].filled_base_lots, 0);

                    if let Some(_) = result.residual_order {
                        panic!("Expected to not have a residual order");
                    };

                    let user_execution_result = result.user_order;

                    let expected_lots_out = 2_500 * 1_900 + 2_550 * 600;
                    assert_eq!(user_execution_result.lots_out, expected_lots_out);

                    assert_eq!(user_execution_result.lots_in, 2_500);
                    assert_eq!(user_execution_result.asset_in, base_asset);
                    assert_eq!(user_execution_result.asset_out, quote_asset);
                    assert_eq!(user_execution_result.filled_size, 2_500);
                }
            }

            #[test]
            fn test_limit_buy_above_best_ask_price_fully_consume_book() {
                let base_asset = 0;
                let quote_asset = 1;
                let tick = 100;
                let tick_decimals = 2;
                let mut market = SpotMarket::test_new(tick, tick_decimals);
                let precision = MarketPrecision {
                    base_lot_size: 10,
                    quote_lot_size: 10,
                    tick,
                    tick_decimals,
                };

                // Asks

                market.add_limit_order_helper(
                    make_order(2_500, 1100, OrderDirection::Sell, 1),
                    &precision,
                );
                market.add_limit_order_helper(
                    make_order(2_500, 800, OrderDirection::Sell, 2),
                    &precision,
                );

                // Cancelled order
                market.add_limit_order_helper(
                    make_order(2_550, 1000, OrderDirection::Sell, 3),
                    &precision,
                );
                market.add_limit_order_helper(
                    make_order(2_550, 600, OrderDirection::Sell, 4),
                    &precision,
                );

                // Cancelled order
                market.add_limit_order_helper(
                    make_order(2_600, 800, OrderDirection::Sell, 5),
                    &precision,
                );
                market.add_limit_order_helper(
                    make_order(2_700, 400, OrderDirection::Sell, 6),
                    &precision,
                );

                market.cancel_order(&make_order(2_550, 1000, OrderDirection::Sell, 3));
                // Check market state before execution
                {
                    assert_eq!(market.get_best_prices().1, Some(2_500));
                    assert_eq!(market.asks_levels.len(), 4);
                    // Check state of the level that limit order will fill to
                }

                // fully consume the order book
                let order = make_order(3_550, 4_000, OrderDirection::Buy, 7);
                let limit_fill_result = market.add_limit_order(order, 0, 1, &precision);

                let Some(result) = limit_fill_result else {
                    panic!("Expected to be some result");
                };

                // Check market state
                {
                    // Should have no asks
                    assert_eq!(market.asks_levels.len(), 0);

                    // Buy order should have been inserted
                    assert_eq!(market.bids_levels.len(), 1);
                    assert_eq!(market.bids_levels[0].volume, 300);
                    assert_eq!(market.bids_levels[0].price, 3_550);

                    assert_eq!(market.get_best_prices(), (Some(3_550), None));
                }

                // Check matching result
                {
                    assert_eq!(result.filled_orders.len(), 6);
                    // filled quote size should be 0 to calculate token changes
                    assert_eq!(result.filled_orders[0].filled_base_lots, 0);
                    assert_eq!(result.filled_orders[1].filled_base_lots, 0);
                    assert_eq!(
                        result.filled_orders[2].common.status,
                        OrderStatus::Cancelled
                    );
                    assert_eq!(result.filled_orders[3].filled_base_lots, 0);
                    assert_eq!(result.filled_orders[4].filled_base_lots, 0);
                    assert_eq!(result.filled_orders[5].filled_base_lots, 0);

                    if let Some(_) = result.residual_order {
                        panic!("Expected to have no residual order");
                    };

                    let user_execution_result = result.user_order;

                    let expected_lots_out = 2_500 * 1_900 + 2_550 * 600 + 2_600 * 800 + 2_700 * 400;
                    assert_eq!(user_execution_result.lots_out, expected_lots_out);

                    assert_eq!(user_execution_result.lots_in, 3_700);
                    assert_eq!(user_execution_result.asset_in, base_asset);
                    assert_eq!(user_execution_result.asset_out, quote_asset);
                    assert_eq!(user_execution_result.filled_size, 3_700);
                }
            }

            #[test]
            fn test_sell_limit_above_best_buy_price_fully_executed() {
                let base_asset = 0;
                let quote_asset = 1;
                let tick = 100;
                let tick_decimals = 2;
                let mut market = SpotMarket::test_new(tick, tick_decimals);
                let precision = MarketPrecision {
                    base_lot_size: 10,
                    quote_lot_size: 10,
                    tick,
                    tick_decimals,
                };

                // Asks

                market.add_limit_order_helper(
                    make_order(2_500, 1100, OrderDirection::Buy, 1),
                    &precision,
                );
                market.add_limit_order_helper(
                    make_order(2_500, 800, OrderDirection::Buy, 2),
                    &precision,
                );

                // Cancelled order
                market.add_limit_order_helper(
                    make_order(2_550, 1000, OrderDirection::Buy, 3),
                    &precision,
                );
                market.add_limit_order_helper(
                    make_order(2_550, 600, OrderDirection::Buy, 4),
                    &precision,
                );

                market.add_limit_order_helper(
                    make_order(2_600, 800, OrderDirection::Buy, 5),
                    &precision,
                );
                market.add_limit_order_helper(
                    make_order(2700, 400, OrderDirection::Buy, 6),
                    &precision,
                );

                market.cancel_order(&make_order(2_550, 1000, OrderDirection::Buy, 3));
                // Check market state before execution
                {
                    assert_eq!(market.get_best_prices().0, Some(2_700));
                    assert_eq!(market.bids_levels.len(), 4);
                    // Check state of the level that limit order will fill to
                    assert_eq!(market.bids_levels.get(1).unwrap().volume, 600); // price 2_550
                    assert_eq!(market.bids_levels.get(1).unwrap().cancelled, 1);
                }

                let order_lot_size = 1_400;
                let order = make_order(2_500, 1_400, OrderDirection::Sell, 7);
                let limit_fill_result = market.add_limit_order(order, 0, 1, &precision);

                let Some(result) = limit_fill_result else {
                    panic!("Expected to be some result");
                };

                // Check market state
                {
                    assert_eq!(market.bids_levels.len(), 2);
                    let partial_fill_level = market
                        .bids_levels
                        .last()
                        .expect("Level should not be empty");
                    assert_eq!(partial_fill_level.volume, 400);
                    assert_eq!(partial_fill_level.cancelled, 0); // Should remove cancel

                    let residual_order = partial_fill_level
                        .orders
                        .get(0)
                        .expect("Level should not be 0");

                    assert_eq!(residual_order.filled_base_lots, 200);
                    // Order should not be inserted
                    assert_eq!(market.asks_levels.len(), 0);

                    assert_eq!(market.get_best_prices(), (Some(2_550), None));
                }

                // Check matching result
                {
                    assert_eq!(result.filled_orders.len(), 3);
                    // filled quote size should be 0 to calculate token changes
                    assert_eq!(result.filled_orders[0].filled_base_lots, 0);
                    assert_eq!(result.filled_orders[1].filled_base_lots, 0);
                    assert_eq!(
                        result.filled_orders[2].common.status,
                        OrderStatus::Cancelled
                    );

                    let Some(residual_order) = result.residual_order else {
                        panic!("Expected to have a residual order");
                    };

                    assert_eq!(residual_order.order_id, 4);
                    assert_eq!(residual_order.filled_base_lots, 200);

                    let user_execution_result = result.user_order;

                    let expected_lots_in = 2_700 * 400 + 2_600 * 800 + 2_550 * 200;
                    assert_eq!(user_execution_result.lots_in, expected_lots_in);

                    assert_eq!(user_execution_result.lots_out, order_lot_size);
                    assert_eq!(user_execution_result.asset_in, quote_asset);
                    assert_eq!(user_execution_result.asset_out, base_asset);
                    assert_eq!(user_execution_result.filled_size, order_lot_size);
                }
            }

            #[test]
            fn test_sell_limit_above_best_buy_price_partially_filled() {
                let base_asset = 0;
                let quote_asset = 1;
                let tick = 100;
                let tick_decimals = 2;
                let mut market = SpotMarket::test_new(tick, tick_decimals);
                let precision = MarketPrecision {
                    base_lot_size: 10,
                    quote_lot_size: 10,
                    tick,
                    tick_decimals,
                };

                // Asks

                market.add_limit_order_helper(
                    make_order(2_500, 1100, OrderDirection::Buy, 1),
                    &precision,
                );
                market.add_limit_order_helper(
                    make_order(2_500, 800, OrderDirection::Buy, 2),
                    &precision,
                );

                // Cancelled order
                market.add_limit_order_helper(
                    make_order(2_550, 1000, OrderDirection::Buy, 3),
                    &precision,
                );
                market.add_limit_order_helper(
                    make_order(2_550, 600, OrderDirection::Buy, 4),
                    &precision,
                );

                market.add_limit_order_helper(
                    make_order(2_600, 800, OrderDirection::Buy, 5),
                    &precision,
                );
                market.add_limit_order_helper(
                    make_order(2700, 400, OrderDirection::Buy, 6),
                    &precision,
                );

                market.cancel_order(&make_order(2_550, 1000, OrderDirection::Buy, 3));
                // Check market state before execution
                {
                    assert_eq!(market.get_best_prices().0, Some(2_700));
                    assert_eq!(market.bids_levels.len(), 4);
                }

                let order = make_order(2_575, 5_000, OrderDirection::Sell, 7);
                let limit_fill_result = market.add_limit_order(order, 0, 1, &precision);

                let Some(result) = limit_fill_result else {
                    panic!("Expected to be some result");
                };

                // Check market state
                {
                    assert_eq!(market.bids_levels.len(), 2);
                    assert_eq!(market.bids_levels[0].price, 2_500);
                    assert_eq!(market.bids_levels[1].price, 2_550);
                    assert_eq!(market.bids_levels[1].volume, 600);

                    // Order should be inserted
                    assert_eq!(market.asks_levels.len(), 1);
                    assert_eq!(market.asks_levels[0].price, 2_575);
                    assert_eq!(market.asks_levels[0].volume, 3_800);

                    assert_eq!(market.get_best_prices(), (Some(2_550), Some(2_575)));
                }

                // Check matching result
                {
                    assert_eq!(result.filled_orders.len(), 2);
                    // filled quote size should be 0 to calculate token changes
                    assert_eq!(result.filled_orders[0].filled_base_lots, 0);
                    assert_eq!(result.filled_orders[1].filled_base_lots, 0);

                    if let Some(_) = result.residual_order {
                        panic!("Expected to not have a residual order");
                    };

                    let user_execution_result = result.user_order;

                    let expected_lots_in = 2_700 * 400 + 2_600 * 800;
                    assert_eq!(user_execution_result.lots_in, expected_lots_in);

                    assert_eq!(user_execution_result.lots_out, 1_200);
                    assert_eq!(user_execution_result.asset_in, quote_asset);
                    assert_eq!(user_execution_result.asset_out, base_asset);
                    assert_eq!(user_execution_result.filled_size, 1_200);
                }
            }

            #[test]
            fn test_sell_limit_above_best_buy_price_fully_consume_orderbook() {
                let base_asset = 0;
                let quote_asset = 1;
                let tick = 100;
                let tick_decimals = 2;
                let mut market = SpotMarket::test_new(tick, tick_decimals);
                let precision = MarketPrecision {
                    base_lot_size: 10,
                    quote_lot_size: 10,
                    tick,
                    tick_decimals,
                };

                // Asks

                market.add_limit_order_helper(
                    make_order(2_500, 1100, OrderDirection::Buy, 1),
                    &precision,
                );
                market.add_limit_order_helper(
                    make_order(2_500, 800, OrderDirection::Buy, 2),
                    &precision,
                );

                // Cancelled order
                market.add_limit_order_helper(
                    make_order(2_550, 1000, OrderDirection::Buy, 3),
                    &precision,
                );
                market.add_limit_order_helper(
                    make_order(2_550, 600, OrderDirection::Buy, 4),
                    &precision,
                );

                market.add_limit_order_helper(
                    make_order(2_600, 800, OrderDirection::Buy, 5),
                    &precision,
                );
                market.add_limit_order_helper(
                    make_order(2700, 400, OrderDirection::Buy, 6),
                    &precision,
                );

                market.cancel_order(&make_order(2_550, 1000, OrderDirection::Buy, 3));
                // Check market state before execution
                {
                    assert_eq!(market.get_best_prices().0, Some(2_700));
                    assert_eq!(market.bids_levels.len(), 4);
                }

                let order = make_order(2_000, 5_000, OrderDirection::Sell, 7);
                let limit_fill_result = market.add_limit_order(order, 0, 1, &precision);

                let Some(result) = limit_fill_result else {
                    panic!("Expected to be some result");
                };

                // Check market state
                {
                    assert_eq!(market.bids_levels.len(), 0);

                    // Order should be inserted
                    assert_eq!(market.asks_levels.len(), 1);
                    assert_eq!(market.asks_levels[0].price, 2_000);
                    assert_eq!(market.asks_levels[0].volume, 1_300);

                    assert_eq!(market.get_best_prices(), (None, Some(2_000)));
                }

                // Check matching result
                {
                    assert_eq!(result.filled_orders.len(), 6);
                    // filled quote size should be 0 to calculate token changes
                    assert_eq!(result.filled_orders[0].filled_base_lots, 0);
                    assert_eq!(result.filled_orders[1].filled_base_lots, 0);
                    assert_eq!(
                        result.filled_orders[2].common.status,
                        OrderStatus::Cancelled
                    );
                    assert_eq!(result.filled_orders[3].filled_base_lots, 0);

                    assert_eq!(result.filled_orders[4].filled_base_lots, 0);
                    assert_eq!(result.filled_orders[5].filled_base_lots, 0);

                    if let Some(_) = result.residual_order {
                        panic!("Expected to not have a residual order");
                    };

                    let user_execution_result = result.user_order;

                    let expected_lots_in = 2_700 * 400 + 2_600 * 800 + 2_550 * 600 + 2_500 * 1_900;
                    assert_eq!(user_execution_result.lots_in, expected_lots_in);

                    assert_eq!(user_execution_result.lots_out, 3_700);
                    assert_eq!(user_execution_result.asset_in, quote_asset);
                    assert_eq!(user_execution_result.asset_out, base_asset);
                    assert_eq!(user_execution_result.filled_size, 3_700);
                }
            }
        }

        #[test]
        fn test_order_aggregation_on_same_price() {
            let mut market = SpotMarket::test_new(100, 2);
            let precision = MarketPrecision {
                base_lot_size: 100,
                quote_lot_size: 100,
                tick: market.tick,
                tick_decimals: market.tick_decimals,
            };
            market.add_limit_order_helper(make_order(100, 10, OrderDirection::Buy, 1), &precision);
            market.add_limit_order_helper(make_order(100, 15, OrderDirection::Buy, 2), &precision);
            assert_eq!(market.bids_levels.len(), 1);
            assert_eq!(market.bids_levels[0].volume, 25);
            assert_eq!(market.bids_levels[0].orders.len(), 2);
        }

        #[test]
        fn test_get_best_prices_returns_none_when_empty() {
            let market = SpotMarket::test_new(100, 2);

            assert_eq!(market.get_best_prices(), (None, None));
        }

        #[test]
        fn test_cancels_ask_order_correctly() {
            let mut market = SpotMarket::test_new(100, 2);
            let precision = MarketPrecision {
                base_lot_size: 100,
                quote_lot_size: 100,
                tick: market.tick,
                tick_decimals: market.tick_decimals,
            };

            market.add_limit_order_helper(make_order(1000, 8, OrderDirection::Sell, 2), &precision);
            market.add_limit_order_helper(make_order(107, 6, OrderDirection::Sell, 3), &precision);
            market.add_limit_order_helper(make_order(110, 8, OrderDirection::Sell, 1), &precision);
            market.add_limit_order_helper(make_order(109, 4, OrderDirection::Sell, 4), &precision);
            market
                .add_limit_order_helper(make_order(1000, 10, OrderDirection::Sell, 5), &precision);
            market.add_limit_order_helper(make_order(1000, 9, OrderDirection::Sell, 6), &precision);
            market
                .add_limit_order_helper(make_order(1000, 19, OrderDirection::Sell, 7), &precision);
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
            let mut market = SpotMarket::test_new(100, 2);
            let precision = MarketPrecision {
                base_lot_size: 100,
                quote_lot_size: 100,
                tick: market.tick,
                tick_decimals: market.tick_decimals,
            };

            market.add_limit_order_helper(make_order(1, 8, OrderDirection::Buy, 1), &precision);
            market.add_limit_order_helper(make_order(3, 8, OrderDirection::Buy, 2), &precision);
            market.add_limit_order_helper(make_order(4, 6, OrderDirection::Buy, 3), &precision);
            market.add_limit_order_helper(make_order(3, 4, OrderDirection::Buy, 4), &precision);
            market.add_limit_order_helper(make_order(8, 10, OrderDirection::Buy, 5), &precision);
            market.add_limit_order_helper(make_order(3, 9, OrderDirection::Buy, 6), &precision);
            assert_eq!(market.bids_levels.len(), 4);
            assert_eq!(market.asks_levels.len(), 0);

            market.cancel_order(&make_order(3, 9, OrderDirection::Buy, 6));
            let price_level = &market.bids_levels[1];
            assert_eq!(price_level.orders[2].common.status, OrderStatus::Cancelled);
            assert_eq!(price_level.cancelled, 1);
        }

        #[test]
        fn test_prunes_cancelled_orders_correctly() {
            let mut market = SpotMarket::test_new(100, 2);
            let precision = MarketPrecision {
                base_lot_size: 100,
                quote_lot_size: 100,
                tick: market.tick,
                tick_decimals: market.tick_decimals,
            };

            let order_1 = make_order(1, 8, OrderDirection::Buy, 1);
            let order_2 = make_order(3, 8, OrderDirection::Buy, 2);
            let order_3 = make_order(1, 6, OrderDirection::Buy, 3);
            let order_4 = make_order(1, 4, OrderDirection::Buy, 4);
            let order_5 = make_order(1, 10, OrderDirection::Buy, 5);
            let order_6 = make_order(1, 9, OrderDirection::Buy, 6);

            market.add_limit_order_helper(order_1.clone(), &precision);
            market.add_limit_order_helper(order_2.clone(), &precision);
            market.add_limit_order_helper(order_3.clone(), &precision);
            market.add_limit_order_helper(order_4.clone(), &precision);
            market.add_limit_order_helper(order_5.clone(), &precision);
            market.add_limit_order_helper(order_6.clone(), &precision);

            assert_eq!(market.bids_levels.len(), 2);
            assert_eq!(market.asks_levels.len(), 0);

            let mut expected_level_volume = 37;
            assert_eq!(market.bids_levels[0].volume, expected_level_volume);

            market.cancel_order(&order_1);
            expected_level_volume -= order_1.base_lots;
            assert_eq!(market.bids_levels[0].cancelled, 1);
            assert_eq!(market.bids_levels[0].orders.len(), 5);
            assert_eq!(market.bids_levels[0].volume, expected_level_volume);

            // try cancel again
            market.cancel_order(&order_1);
            assert_eq!(market.bids_levels[0].cancelled, 1);
            assert_eq!(market.bids_levels[0].orders.len(), 5);
            assert_eq!(market.bids_levels[0].volume, expected_level_volume);

            market.cancel_order(&order_5);
            expected_level_volume -= order_5.base_lots;
            assert_eq!(market.bids_levels[0].cancelled, 2);
            assert_eq!(market.bids_levels[0].orders.len(), 5);
            assert_eq!(market.bids_levels[0].volume, expected_level_volume);

            // should prune here
            market.cancel_order(&make_order(1, 9, OrderDirection::Buy, 6));
            expected_level_volume -= order_6.base_lots;
            assert_eq!(market.bids_levels[0].cancelled, 0);
            assert_eq!(market.bids_levels[0].orders.len(), 2);
            assert_eq!(market.bids_levels[0].volume, expected_level_volume);
        }
    }
}
