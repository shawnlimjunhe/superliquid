use crate::state::order::OrderDirection;

use super::order::Order;

pub struct Level {
    pub price: u64,
    pub volume: u32,
    pub orders: Vec<Order>,
}

pub struct Market {
    // pub tick_size: (),
    // pub lot_size: (),

    // levels are in reverse order, best prices are at the end
    pub bids_levels: Vec<Level>, // 0, 1, 2, ..
    pub asks_levels: Vec<Level>, // 10, 9, 8, ..
}

impl Market {
    fn add_order_with_cmp<F>(order: Order, levels: &mut Vec<Level>, mut compare: F)
    where
        F: FnMut(u64, u64) -> std::cmp::Ordering,
    {
        let price = order.price;
        let mut left = 0;
        let mut right = levels.len();

        while left < right {
            let mid = left + (right - left) / 2;
            let mid_price = levels[mid].price;

            match price.partial_cmp(&mid_price).unwrap() {
                std::cmp::Ordering::Equal => {
                    levels[mid].volume += order.size;
                    levels[mid].orders.push(order);
                    return;
                }
                _ => {
                    if compare(price, mid_price) == std::cmp::Ordering::Less {
                        right = mid;
                    } else {
                        left = mid + 1;
                    }
                }
            }
        }

        levels.insert(
            left,
            Level {
                price,
                volume: order.size,
                orders: vec![order],
            },
        )
    }

    pub fn add_order_to_bids(&mut self, order: Order) {
        Self::add_order_with_cmp(order, &mut self.bids_levels, |a, b| {
            a.partial_cmp(&b).unwrap()
        });
    }

    pub fn add_order_to_asks(&mut self, order: Order) {
        Self::add_order_with_cmp(order, &mut self.asks_levels, |a, b| {
            b.partial_cmp(&a).unwrap()
        });
    }

    pub fn add_order(&mut self, order: Order) {
        match order.direction {
            OrderDirection::Buy => {
                self.add_order_to_bids(order);
            }
            OrderDirection::Sell => {
                self.add_order_to_asks(order);
            }
        }
    }

    pub fn cancel_order() {
        todo!()
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
    use crate::state::order::{Order, OrderDirection, OrderStatus};

    fn make_order(price: u64, size: u32, direction: OrderDirection) -> Order {
        Order {
            price,
            size,
            direction,
            id: 0,
            filled_size: 0,
            status: OrderStatus::Open,
        }
    }

    #[test]
    fn test_add_bid_order_inserts_correctly() {
        let mut market = Market {
            bids_levels: vec![],
            asks_levels: vec![],
        };

        market.add_order(make_order(100, 10, OrderDirection::Buy));
        market.add_order(make_order(105, 5, OrderDirection::Buy));
        market.add_order(make_order(103, 7, OrderDirection::Buy));
        market.add_order(make_order(1, 7, OrderDirection::Buy));

        assert_eq!(market.bids_levels.len(), 4);
        assert_eq!(market.bids_levels.last().unwrap().price, 105);
        assert_eq!(market.bids_levels.first().unwrap().price, 1);
        assert_eq!(market.get_best_prices().0, Some(105));
    }

    #[test]
    fn test_add_ask_order_inserts_correctly() {
        let mut market = Market {
            bids_levels: vec![],
            asks_levels: vec![],
        };

        market.add_order(make_order(110, 8, OrderDirection::Sell));
        market.add_order(make_order(1000, 8, OrderDirection::Sell));
        market.add_order(make_order(107, 6, OrderDirection::Sell));
        market.add_order(make_order(109, 4, OrderDirection::Sell));

        assert_eq!(market.asks_levels.len(), 4);
        assert_eq!(market.asks_levels.last().unwrap().price, 107);
        assert_eq!(market.asks_levels.first().unwrap().price, 1000);
        assert_eq!(market.get_best_prices().1, Some(107));
    }

    #[test]
    fn test_order_aggregation_on_same_price() {
        let mut market = Market {
            bids_levels: vec![],
            asks_levels: vec![],
        };

        market.add_order(make_order(100, 10, OrderDirection::Buy));
        market.add_order(make_order(100, 15, OrderDirection::Buy));

        assert_eq!(market.bids_levels.len(), 1);
        assert_eq!(market.bids_levels[0].volume, 25);
        assert_eq!(market.bids_levels[0].orders.len(), 2);
    }

    #[test]
    fn test_get_best_prices_returns_none_when_empty() {
        let market = Market {
            bids_levels: vec![],
            asks_levels: vec![],
        };

        assert_eq!(market.get_best_prices(), (None, None));
    }
}
