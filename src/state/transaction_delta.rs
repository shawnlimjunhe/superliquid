use crate::types::transaction::PublicKeyHash;

use super::{asset::AssetId, order::Order};

pub struct AssetDelta {
    pub account: PublicKeyHash,
    pub asset_id: AssetId,
    pub amount: u128,
    pub is_increase: bool,
}

pub struct TransferDelta {
    pub(crate) initiator: PublicKeyHash,
    pub(crate) asset_in: AssetDelta,
    pub(crate) asset_out: AssetDelta,
}

pub struct SpotCancelOrderDelta {
    pub(crate) initiator: PublicKeyHash,
    pub(crate) account_order_position: usize,
    pub(crate) order_level_index: usize,
    pub(crate) order_index: usize,
}

pub struct OrderBookDelta {}

pub struct OrderDelta {

}

pub struct SpotOrderDelta {
    pub(crate) initiator: PublicKeyHash,
    pub(crate) order: Order,
    pub(crate) balance_deltas: Vec<AssetDelta>,
    pub(crate) order_book_delta: OrderBookDelta,
    pub(crate) order_deltas: 
}
