use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]

pub enum OrderStatus {
    Open,
    Cancelled,
    Rejected,
    Filled,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum OrderDirection {
    Buy,
    Sell,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Order {
    pub id: u128,
    pub price: u64,
    pub size: u32,
    pub filled_size: u32,
    pub status: OrderStatus,
    pub direction: OrderDirection,
    // type
    // trigger conditions
    // tp/sl
}
