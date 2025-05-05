use super::order::Order;

pub struct Market {
    pub bids: Vec<Order>,
    pub asks: Vec<Order>,
}
