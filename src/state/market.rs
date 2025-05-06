use super::order::Order;

pub struct Level {
    pub price: u64,
    pub volume: u64,
    pub orders: Vec<Order>,
}

pub enum Side {
    Bid,
    Ask,
}

pub struct Market {
    // pub tick_size: (),
    // pub lot_size: (),

    // levels are in reverse order, best prices are at the end
    pub bid_levels: Vec<Level>,
    pub ask_levels: Vec<Level>,
}

impl Market {
    pub fn add_order_to_bids(self, price: u64, volume: u64, order_id: u128) {}

    pub fn add_order(&self, side: Side, price: u64, volume: u64, order_id: u128) {
        match side {
            Side::Ask => {}
            Side::Bid => {}
        }
        todo!();
    }

    pub fn cancel_order() {
        todo!()
    }

    pub fn get_best_prices(&self) -> (Option<u64>, Option<u64>) {
        let best_bid = self.bid_levels.last().map(|level| level.price);
        let best_ask = self.ask_levels.last().map(|level| level.price);

        (best_bid, best_ask)
    }
}
