use super::asset::AssetId;

pub struct AssetDelta {
    pub asset_id: AssetId,
    pub amount: u128,
}

pub enum TransactionDelta {
    TransferDelta {
        asset_in: AssetDelta,
        asset_out: AssetDelta,
    },
    SpotOrderDelta {},
    SpotCancelDelta {},
}
