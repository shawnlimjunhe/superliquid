use crate::types::transaction::PublicKeyHash;

use super::asset::AssetId;

pub struct AssetDelta {
    pub account: PublicKeyHash,
    pub asset_id: AssetId,
    pub amount: u128,
    pub is_increase: bool,
}

pub struct TransferDelta {
    pub(crate) asset_in: AssetDelta,
    pub(crate) asset_out: AssetDelta,
    pub(crate) nonce_delta: PublicKeyHash,
}

pub enum TransactionDelta {
    TransferDelta {
        asset_in: AssetDelta,
        asset_out: AssetDelta,
    },
    SpotOrderDelta {},
    SpotCancelDelta {},
}
