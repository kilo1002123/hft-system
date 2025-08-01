//! 通用数据类型和工具函数

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use uuid::Uuid;

pub mod orderbook;
pub mod data_normalizer;

/// 系统错误类型
#[derive(Debug, thiserror::Error)]
pub enum HftError {
    #[error("网络错误: {0}")]
    Network(String),
    
    #[error("API 错误 [{0}]: {1}")]
    Api(i32, String),
    
    #[error("认证失败: {0}")]
    Auth(String),
    
    #[error("限流错误: {0}")]
    RateLimit(String),
    
    #[error("余额不足: {0}")]
    InsufficientBalance(String),
    
    #[error("无效订单: {0}")]
    InvalidOrder(String),
    
    #[error("系统错误: {0}")]
    System(String),
    
    #[error("其他错误: {0}")]
    Other(String),
}

/// 系统结果类型
pub type Result<T> = std::result::Result<T, HftError>;

/// 交易所枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Exchange {
    Binance,
    Okx,
    Gate,
}

impl fmt::Display for Exchange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Exchange::Binance => write!(f, "binance"),
            Exchange::Okx => write!(f, "okx"),
            Exchange::Gate => write!(f, "gate"),
        }
    }
}

/// 交易对
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Symbol {
    pub base: String,
    pub quote: String,
    pub exchange: Exchange,
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.base, self.quote)
    }
}

/// 订单方向
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderSide {
    Buy,
    Sell,
}

/// 订单类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderType {
    Market,
    Limit,
    StopLoss,
    StopLossLimit,
    TakeProfit,
    TakeProfitLimit,
}

/// 订单状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    New,
    PartiallyFilled,
    Filled,
    Canceled,
    Rejected,
    Expired,
}

/// 时效类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeInForce {
    GTC, // Good Till Cancel
    IOC, // Immediate Or Cancel
    FOK, // Fill Or Kill
}

/// 订单请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRequest {
    pub client_order_id: Option<String>,
    pub symbol: Symbol,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub quantity: Decimal,
    pub price: Option<Decimal>,
    pub time_in_force: Option<TimeInForce>,
    pub reduce_only: Option<bool>,
    pub metadata: HashMap<String, String>,
}

/// 订单响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderResponse {
    pub order_id: String,
    pub client_order_id: Option<String>,
    pub symbol: Symbol,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub original_quantity: Decimal,
    pub executed_quantity: Decimal,
    pub remaining_quantity: Decimal,
    pub price: Option<Decimal>,
    pub average_price: Option<Decimal>,
    pub status: OrderStatus,
    pub time_in_force: Option<TimeInForce>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub exchange_data: HashMap<String, String>,
}

/// 账户余额
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Balance {
    pub asset: String,
    pub free: Decimal,
    pub locked: Decimal,
    pub total: Decimal,
}

/// 持仓信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub symbol: Symbol,
    pub quantity: Decimal,
    pub average_price: Decimal,
    pub unrealized_pnl: Decimal,
    pub realized_pnl: Decimal,
    pub updated_at: DateTime<Utc>,
}

/// 市场数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketData {
    pub symbol: Symbol,
    pub bid: Decimal,
    pub ask: Decimal,
    pub last_price: Decimal,
    pub volume_24h: Decimal,
    pub change_24h: Decimal,
    pub timestamp: DateTime<Utc>,
}

/// K线数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Kline {
    pub symbol: Symbol,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
    pub open_time: DateTime<Utc>,
    pub close_time: DateTime<Utc>,
}

/// 订单簿价格档位
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceLevel {
    pub price: Decimal,
    pub quantity: Decimal,
}

/// 订单簿
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBook {
    pub symbol: Symbol,
    pub bids: Vec<PriceLevel>,
    pub asks: Vec<PriceLevel>,
    pub timestamp: DateTime<Utc>,
}

/// 交易记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub id: String,
    pub symbol: Symbol,
    pub side: OrderSide,
    pub quantity: Decimal,
    pub price: Decimal,
    pub fee: Decimal,
    pub fee_asset: String,
    pub is_maker: bool,
    pub timestamp: DateTime<Utc>,
}

/// 市场数据事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MarketDataEvent {
    Ticker(MarketData),
    OrderBook(OrderBook),
    Trade(Trade),
    Kline(Kline),
}

/// 交易所配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeConfig {
    pub api_key: String,
    pub secret_key: String,
    pub passphrase: Option<String>,
    pub base_url: String,
    pub timeout: std::time::Duration,
    pub max_retries: u32,
    pub retry_delay: std::time::Duration,
}

/// 策略配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    pub enabled: bool,
    pub symbols: Vec<Symbol>,
    pub parameters: HashMap<String, serde_json::Value>,
}

/// 工具函数
impl Symbol {
    pub fn new(base: &str, quote: &str, exchange: Exchange) -> Self {
        Self {
            base: base.to_uppercase(),
            quote: quote.to_uppercase(),
            exchange,
        }
    }
    
    pub fn to_exchange_format(&self) -> String {
        match self.exchange {
            Exchange::Binance => format!("{}{}", self.base, self.quote),
            Exchange::Okx => format!("{}-{}", self.base, self.quote),
            Exchange::Gate => format!("{}_{}", self.base, self.quote),
        }
    }
}

impl OrderRequest {
    pub fn new_limit(symbol: Symbol, side: OrderSide, quantity: Decimal, price: Decimal) -> Self {
        Self {
            client_order_id: Some(Uuid::new_v4().to_string()),
            symbol,
            side,
            order_type: OrderType::Limit,
            quantity,
            price: Some(price),
            time_in_force: Some(TimeInForce::GTC),
            reduce_only: None,
            metadata: HashMap::new(),
        }
    }
    
    pub fn new_market(symbol: Symbol, side: OrderSide, quantity: Decimal) -> Self {
        Self {
            client_order_id: Some(Uuid::new_v4().to_string()),
            symbol,
            side,
            order_type: OrderType::Market,
            quantity,
            price: None,
            time_in_force: Some(TimeInForce::IOC),
            reduce_only: None,
            metadata: HashMap::new(),
        }
    }
}

/// 常用常量
pub mod constants {
    use rust_decimal::Decimal;
    
    pub const ZERO: Decimal = Decimal::ZERO;
    pub const ONE: Decimal = Decimal::ONE;
    pub const HUNDRED: Decimal = Decimal::from_parts(100, 0, 0, false, 0);
    
    pub const DEFAULT_TIMEOUT_SECS: u64 = 30;
    pub const DEFAULT_MAX_RETRIES: u32 = 3;
    pub const DEFAULT_RETRY_DELAY_MS: u64 = 1000;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_creation() {
        let symbol = Symbol::new("btc", "usdt", Exchange::Binance);
        assert_eq!(symbol.base, "BTC");
        assert_eq!(symbol.quote, "USDT");
        assert_eq!(symbol.exchange, Exchange::Binance);
    }

    #[test]
    fn test_symbol_exchange_format() {
        let binance_symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
        assert_eq!(binance_symbol.to_exchange_format(), "BTCUSDT");
        
        let okx_symbol = Symbol::new("BTC", "USDT", Exchange::Okx);
        assert_eq!(okx_symbol.to_exchange_format(), "BTC-USDT");
        
        let gate_symbol = Symbol::new("BTC", "USDT", Exchange::Gate);
        assert_eq!(gate_symbol.to_exchange_format(), "BTC_USDT");
    }

    #[test]
    fn test_order_request_creation() {
        let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
        let order = OrderRequest::new_limit(
            symbol.clone(),
            OrderSide::Buy,
            Decimal::new(1, 2), // 0.01
            Decimal::new(50000, 0)
        );
        
        assert_eq!(order.symbol, symbol);
        assert_eq!(order.side, OrderSide::Buy);
        assert_eq!(order.order_type, OrderType::Limit);
        assert!(order.client_order_id.is_some());
    }
}

