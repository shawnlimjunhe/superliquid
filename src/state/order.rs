use serde::{Deserialize, Serialize};

use crate::types::transaction::PublicKeyHash;

use super::{asset::AssetId, spot_clearinghouse::MarketId};

pub type OrderId = u64;
pub type OrderPrice = u64;

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
        price: OrderPrice,
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
            price,
            quote_size,
            filled_quote_size: 0,
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
            OrderDirection::Sell => MarketOrder::Sell(MarketSellOrder {
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
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum OrderType {
    Limit(OrderPrice),
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
    pub price: OrderPrice, // quote/base
    pub quote_size: u64,
    pub filled_quote_size: u64,
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
    pub quote_size: u64,
    pub filled_size: u64,
    pub average_execution_price: u64,
    pub common: CommonOrderFields,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MarketBuyOrder {
    pub base_size: u64,
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

pub struct CounterPartyPartialFill {
    pub order_id: OrderId,
    pub account_public_key: PublicKeyHash,
    pub order_price: OrderPrice,
    pub filled_quote_amount: u64,
}

pub struct LimitFillResult {
    pub order_id: OrderId,
    pub asset_in: AssetId,
    pub amount_in: u64,
    pub asset_out: AssetId,
    pub amount_out: u64,
    pub filled_orders: Vec<LimitOrder>,
    pub counterparty_partial_fill: Option<CounterPartyPartialFill>,
}

pub enum MarketOrderMatchingResults {
    SellInQuote {
        order_id: OrderId,
        quote_filled_amount: u64,
        base_amount_in: u64,
        filled_orders: Vec<LimitOrder>,
        counterparty_partial_fill: Option<CounterPartyPartialFill>,
    },
    BuyInBase {
        order_id: OrderId,
        base_filled_amount: u64,
        quote_amount_in: u64,
        filled_orders: Vec<LimitOrder>,
        counterparty_partial_fill: Option<CounterPartyPartialFill>,
    },
}
pub enum OrderChange {
    LimitOrderChange {
        order_id: OrderId,
        filled_amount: u64,
        average_execution_price: u128,
    },
    MarketOrderChange {
        order_id: OrderId,
        filled_amount: u64,
        average_execution_price: u64,
    },
}

pub struct ExecutionResults {
    pub filled_orders: Vec<LimitOrder>,
    pub counterparty_partial_fill: Option<CounterPartyPartialFill>,
    pub user_order_change: Option<OrderChange>,
}
