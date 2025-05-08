use serde::{Deserialize, Serialize};

use crate::types::transaction::PublicKeyHash;

pub type OrderId = u64;
pub type OrderPrice = u64;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
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
    pub id: OrderId,
    pub price: OrderPrice,
    pub size: u32,
    pub filled_size: u32,
    pub status: OrderStatus,
    pub direction: OrderDirection,
    pub account: PublicKeyHash,
    // type
    // trigger conditions
    // tp/sl
}
