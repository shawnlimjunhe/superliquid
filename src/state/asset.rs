pub type AssetId = u32;

type AssetIdCounter = AssetId;
pub struct Asset {
    pub asset_id: AssetId,
    pub asset_name: String,
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
            asset_name: "Supe".to_owned(),
            decimals: 6,
        };

        let asset_1 = Asset {
            asset_id: 1,
            asset_name: "USD".to_owned(),
            decimals: 6,
        };

        let assets = vec![asset_0, asset_1];

        Self {
            next_asset_id: 2,
            assets,
        }
    }
}
