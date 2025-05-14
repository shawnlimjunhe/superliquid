use serde::{Deserialize, Serialize};

use crate::types::transaction::PublicKeyHash;

use super::{asset::AssetId, spot_clearinghouse::MarketId};

pub type OrderId = u64;
pub type OrderPriceMultiple = u64;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum OrderStatus {
    Open,
    Cancelled,
    Rejected,
    Filled,
    PartiallyFilled,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum OrderDirection {
    Buy,
    Sell,
}

pub struct OrderStateManager {
    next_id: OrderId,
}

impl OrderStateManager {
    pub fn new() -> Self {
        Self { next_id: 0 }
    }

    pub fn new_limit_order(
        &mut self,
        market_id: MarketId,
        account: PublicKeyHash,
        direction: OrderDirection,
        price_multiple: OrderPriceMultiple,
        quote_size: u64,
    ) -> LimitOrder {
        let id = self.next_id;
        self.next_id += 1;
        LimitOrder {
            common: CommonOrderFields {
                id,
                market_id,
                status: OrderStatus::Open,
                account,
                direction,
            },
            price_multiple,
            base_lots: quote_size,
            filled_base_lots: 0,
        }
    }

    pub fn new_market_order(
        &mut self,
        market_id: MarketId,
        account: PublicKeyHash,
        direction: OrderDirection,
        size: u64,
    ) -> MarketOrder {
        let id = self.next_id;
        self.next_id += 1;
        match direction {
            OrderDirection::Buy => MarketOrder::Buy(MarketBuyOrder {
                quote_size: size,
                filled_size: 0,
                average_execution_price: 0,
                common: CommonOrderFields {
                    id,
                    market_id,
                    status: OrderStatus::Open,
                    account,
                    direction,
                },
            }),
            OrderDirection::Sell => MarketOrder::Sell(MarketSellOrder {
                base_size: size,
                filled_size: 0,
                average_execution_price: 0,
                common: CommonOrderFields {
                    id,
                    market_id,
                    status: OrderStatus::Open,
                    account,
                    direction,
                },
            }),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum OrderType {
    Limit(OrderPriceMultiple),
    Market,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CommonOrderFields {
    pub id: OrderId,
    pub market_id: MarketId,
    pub status: OrderStatus,
    pub account: PublicKeyHash,
    pub direction: OrderDirection,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Order {
    Limit(LimitOrder),
    Market(MarketOrder),
}
impl Order {
    pub fn get_account(&self) -> &PublicKeyHash {
        match self {
            Order::Limit(limit_order) => &limit_order.common.account,
            Order::Market(MarketOrder::Buy(order)) => &order.common.account,
            Order::Market(MarketOrder::Sell(order)) => &order.common.account,
        }
    }

    pub fn get_market_id(&self) -> &MarketId {
        match self {
            Order::Limit(limit_order) => &limit_order.common.market_id,
            Order::Market(MarketOrder::Buy(order)) => &order.common.market_id,
            Order::Market(MarketOrder::Sell(order)) => &order.common.market_id,
        }
    }
}
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LimitOrder {
    pub common: CommonOrderFields,
    pub price_multiple: OrderPriceMultiple, // quote/base
    pub base_lots: u64,
    pub filled_base_lots: u64,
    // type
    // trigger conditions
    // tp/sl
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MarketOrder {
    Sell(MarketSellOrder),
    Buy(MarketBuyOrder),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MarketSellOrder {
    pub base_size: u64,
    pub filled_size: u64,
    pub average_execution_price: u64,
    pub common: CommonOrderFields,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MarketBuyOrder {
    pub quote_size: u64,
    pub filled_size: u64,
    pub average_execution_price: u64,
    pub common: CommonOrderFields,
}

impl MarketOrder {
    pub fn get_account(&self) -> &PublicKeyHash {
        match self {
            MarketOrder::Sell(market_sell_order) => &market_sell_order.common.account,
            MarketOrder::Buy(market_buy_order) => &market_buy_order.common.account,
        }
    }
}

pub struct ResidualOrder {
    pub order_id: OrderId,
    pub account_public_key: PublicKeyHash,
    pub price_multiple: OrderPriceMultiple,
    pub filled_base_lots: u64,
}

#[derive(Debug)]
pub struct UserExecutionResult {
    pub order_id: OrderId,
    pub asset_in: AssetId,
    pub lots_in: u64,
    pub asset_out: AssetId,
    pub lots_out: u64,
    pub filled_size: u64,
}

pub struct LimitFillResult {
    pub user_order: UserExecutionResult,
    pub filled_orders: Vec<LimitOrder>,
    pub residual_order: Option<ResidualOrder>,
}

pub enum MarketOrderMatchingResults {
    Sell {
        order_id: OrderId,
        base_filled_lots: u64,
        quote_lots_in: u64,
        filled_orders: Vec<LimitOrder>,
        residual_order: Option<ResidualOrder>,
    },
    Buy {
        order_id: OrderId,
        quote_filled_lots: u64,
        base_lots_in: u64,
        filled_orders: Vec<LimitOrder>,
        residual_order: Option<ResidualOrder>,
    },
}
pub enum OrderChange {
    LimitOrderChange {
        order_id: OrderId,
        filled_lots: u64,
        average_execution_price: u128,
    },
    MarketOrderChange {
        order_id: OrderId,
        filled_lots: u64,
        average_execution_price: u64,
    },
}

pub struct ExecutionResults {
    pub filled_orders: Vec<LimitOrder>,
    pub residual_order: Option<ResidualOrder>,
    pub user_order_change: Option<OrderChange>,
}
