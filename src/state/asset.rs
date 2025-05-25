use serde::{Deserialize, Serialize};

pub type AssetId = u32;

type AssetIdCounter = AssetId;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Asset {
    pub asset_id: AssetId,
    pub asset_name: String,
    pub lot_size: u32,
    pub decimals: u8,
}

pub struct AssetManager {
    pub next_asset_id: AssetIdCounter,
    pub assets: Vec<Asset>,
}

impl AssetManager {
    pub fn new() -> Self {
        let asset_0 = Asset {
            asset_id: 0,
            asset_name: "SUPE".to_owned(),
            decimals: 4,
            lot_size: 100,
        };

        let asset_1 = Asset {
            asset_id: 1,
            asset_name: "USD".to_owned(),
            decimals: 4,
            lot_size: 100,
        };

        let assets = vec![asset_0, asset_1];

        Self {
            next_asset_id: 2,
            assets,
        }
    }
}
