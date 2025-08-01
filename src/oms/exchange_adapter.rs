use crate::common::{Exchange, Result, Symbol};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

/// 订单ID类型
pub type OrderId = String;
pub type ClientOrderId = String;

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
    TakeProfit,
    StopLossLimit,
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
    Pending,
}

/// 时间有效性
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeInForce {
    /// Good Till Canceled
    GTC,
    /// Immediate Or Cancel
    IOC,
    /// Fill Or Kill
    FOK,
    /// Good Till Date
    GTD,
    /// Post Only
    PostOnly,
}

/// 订单请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRequest {
    /// 客户端订单ID
    pub client_order_id: Option<ClientOrderId>,
    /// 交易对
    pub symbol: Symbol,
    /// 订单方向
    pub side: OrderSide,
    /// 订单类型
    pub order_type: OrderType,
    /// 数量
    pub quantity: Decimal,
    /// 价格 (限价单必需)
    pub price: Option<Decimal>,
    /// 止损价格
    pub stop_price: Option<Decimal>,
    /// 时间有效性
    pub time_in_force: Option<TimeInForce>,
    /// 只减仓
    pub reduce_only: Option<bool>,
    /// 用户自定义数据
    pub metadata: HashMap<String, String>,
}

/// 订单响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderResponse {
    /// 交易所订单ID
    pub order_id: OrderId,
    /// 客户端订单ID
    pub client_order_id: Option<ClientOrderId>,
    /// 交易对
    pub symbol: Symbol,
    /// 订单方向
    pub side: OrderSide,
    /// 订单类型
    pub order_type: OrderType,
    /// 原始数量
    pub original_quantity: Decimal,
    /// 已成交数量
    pub executed_quantity: Decimal,
    /// 剩余数量
    pub remaining_quantity: Decimal,
    /// 订单价格
    pub price: Option<Decimal>,
    /// 平均成交价格
    pub average_price: Option<Decimal>,
    /// 订单状态
    pub status: OrderStatus,
    /// 时间有效性
    pub time_in_force: Option<TimeInForce>,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 更新时间
    pub updated_at: DateTime<Utc>,
    /// 交易所特定数据
    pub exchange_data: HashMap<String, String>,
}

/// 账户余额
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Balance {
    /// 币种
    pub asset: String,
    /// 可用余额
    pub free: Decimal,
    /// 冻结余额
    pub locked: Decimal,
    /// 总余额
    pub total: Decimal,
}

/// 交易记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    /// 交易ID
    pub trade_id: String,
    /// 订单ID
    pub order_id: OrderId,
    /// 交易对
    pub symbol: Symbol,
    /// 交易方向
    pub side: OrderSide,
    /// 数量
    pub quantity: Decimal,
    /// 价格
    pub price: Decimal,
    /// 手续费
    pub fee: Decimal,
    /// 手续费币种
    pub fee_asset: String,
    /// 交易时间
    pub timestamp: DateTime<Utc>,
    /// 是否为maker
    pub is_maker: bool,
}

/// 交易所配置
#[derive(Debug, Clone)]
pub struct ExchangeConfig {
    /// 交易所类型
    pub exchange: Exchange,
    /// API密钥
    pub api_key: String,
    /// 密钥
    pub secret_key: String,
    /// 密码短语 (OKX需要)
    pub passphrase: Option<String>,
    /// 基础URL
    pub base_url: String,
    /// 测试网URL
    pub testnet_url: Option<String>,
    /// 是否使用测试网
    pub use_testnet: bool,
    /// 请求超时
    pub timeout: Duration,
    /// 重试次数
    pub max_retries: u32,
    /// 重试间隔
    pub retry_delay: Duration,
    /// 速率限制配置
    pub rate_limit: RateLimitConfig,
}

/// 速率限制配置
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// 每秒请求数
    pub requests_per_second: u32,
    /// 每分钟请求数
    pub requests_per_minute: u32,
    /// 订单速率限制
    pub orders_per_second: u32,
    /// 权重限制
    pub weight_per_minute: u32,
}

/// 交易所错误
#[derive(Debug, thiserror::Error)]
pub enum ExchangeError {
    #[error("Authentication failed: {message}")]
    AuthenticationError { message: String },
    
    #[error("Rate limit exceeded: {retry_after:?}")]
    RateLimitError { retry_after: Option<Duration> },
    
    #[error("Order not found: {order_id}")]
    OrderNotFound { order_id: String },
    
    #[error("Insufficient balance: required {required}, available {available}")]
    InsufficientBalance { required: Decimal, available: Decimal },
    
    #[error("Invalid symbol: {symbol}")]
    InvalidSymbol { symbol: String },
    
    #[error("Invalid order: {reason}")]
    InvalidOrder { reason: String },
    
    #[error("Market closed")]
    MarketClosed,
    
    #[error("Network error: {message}")]
    NetworkError { message: String },
    
    #[error("API error: code={code}, message={message}")]
    ApiError { code: String, message: String },
    
    #[error("Timeout error")]
    TimeoutError,
    
    #[error("Serialization error: {message}")]
    SerializationError { message: String },
    
    #[error("Unknown error: {message}")]
    Unknown { message: String },
}

/// 交易所适配器接口
#[async_trait]
pub trait ExchangeAdapter: Send + Sync {
    /// 获取交易所类型
    fn exchange(&self) -> Exchange;
    
    /// 获取配置
    fn config(&self) -> &ExchangeConfig;
    
    /// 测试连接
    async fn test_connection(&self) -> Result<(), ExchangeError>;
    
    /// 获取服务器时间
    async fn get_server_time(&self) -> Result<DateTime<Utc>, ExchangeError>;
    
    /// 获取交易对信息
    async fn get_symbol_info(&self, symbol: &Symbol) -> Result<SymbolInfo, ExchangeError>;
    
    /// 获取所有交易对信息
    async fn get_all_symbols(&self) -> Result<Vec<SymbolInfo>, ExchangeError>;
    
    // === 订单管理 ===
    
    /// 创建订单
    async fn place_order(&self, request: &OrderRequest) -> Result<OrderResponse, ExchangeError>;
    
    /// 取消订单
    async fn cancel_order(&self, order_id: &str, symbol: &Symbol) -> Result<OrderResponse, ExchangeError>;
    
    /// 取消所有订单
    async fn cancel_all_orders(&self, symbol: Option<&Symbol>) -> Result<Vec<OrderResponse>, ExchangeError>;
    
    /// 查询订单
    async fn get_order(&self, order_id: &str, symbol: &Symbol) -> Result<OrderResponse, ExchangeError>;
    
    /// 查询活跃订单
    async fn get_open_orders(&self, symbol: Option<&Symbol>) -> Result<Vec<OrderResponse>, ExchangeError>;
    
    /// 查询历史订单
    async fn get_order_history(
        &self,
        symbol: Option<&Symbol>,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> Result<Vec<OrderResponse>, ExchangeError>;
    
    // === 账户管理 ===
    
    /// 获取账户余额
    async fn get_account_balance(&self) -> Result<Vec<Balance>, ExchangeError>;
    
    /// 获取特定币种余额
    async fn get_asset_balance(&self, asset: &str) -> Result<Balance, ExchangeError>;
    
    /// 获取交易历史
    async fn get_trade_history(
        &self,
        symbol: Option<&Symbol>,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> Result<Vec<Trade>, ExchangeError>;
    
    // === 市场数据 ===
    
    /// 获取最新价格
    async fn get_ticker_price(&self, symbol: &Symbol) -> Result<Decimal, ExchangeError>;
    
    /// 获取订单簿
    async fn get_order_book(&self, symbol: &Symbol, limit: Option<u32>) -> Result<OrderBookSnapshot, ExchangeError>;
    
    // === 工具方法 ===
    
    /// 标准化交易对格式
    fn normalize_symbol(&self, symbol: &Symbol) -> String;
    
    /// 解析交易对
    fn parse_symbol(&self, symbol_str: &str) -> Result<Symbol, ExchangeError>;
    
    /// 格式化数量精度
    fn format_quantity(&self, symbol: &Symbol, quantity: Decimal) -> Result<Decimal, ExchangeError>;
    
    /// 格式化价格精度
    fn format_price(&self, symbol: &Symbol, price: Decimal) -> Result<Decimal, ExchangeError>;
}

/// 交易对信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInfo {
    /// 交易对
    pub symbol: Symbol,
    /// 基础币种
    pub base_asset: String,
    /// 计价币种
    pub quote_asset: String,
    /// 是否可交易
    pub is_trading: bool,
    /// 最小订单数量
    pub min_quantity: Decimal,
    /// 最大订单数量
    pub max_quantity: Decimal,
    /// 数量步长
    pub quantity_step: Decimal,
    /// 最小价格
    pub min_price: Decimal,
    /// 最大价格
    pub max_price: Decimal,
    /// 价格步长
    pub price_step: Decimal,
    /// 最小名义价值
    pub min_notional: Decimal,
    /// 数量精度
    pub quantity_precision: u32,
    /// 价格精度
    pub price_precision: u32,
    /// 手续费率
    pub fee_rate: Option<Decimal>,
}

/// 订单簿快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookSnapshot {
    /// 交易对
    pub symbol: Symbol,
    /// 买单
    pub bids: Vec<OrderBookLevel>,
    /// 卖单
    pub asks: Vec<OrderBookLevel>,
    /// 时间戳
    pub timestamp: DateTime<Utc>,
    /// 最后更新ID
    pub last_update_id: Option<u64>,
}

/// 订单簿层级
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookLevel {
    /// 价格
    pub price: Decimal,
    /// 数量
    pub quantity: Decimal,
}

/// 订单管理器
pub struct OrderManager {
    adapters: HashMap<Exchange, Box<dyn ExchangeAdapter>>,
}

impl OrderManager {
    /// 创建新的订单管理器
    pub fn new() -> Self {
        Self {
            adapters: HashMap::new(),
        }
    }
    
    /// 添加交易所适配器
    pub fn add_adapter(&mut self, adapter: Box<dyn ExchangeAdapter>) {
        let exchange = adapter.exchange();
        self.adapters.insert(exchange, adapter);
    }
    
    /// 获取适配器
    pub fn get_adapter(&self, exchange: Exchange) -> Option<&dyn ExchangeAdapter> {
        self.adapters.get(&exchange).map(|a| a.as_ref())
    }
    
    /// 在指定交易所下单
    pub async fn place_order(
        &self,
        exchange: Exchange,
        request: &OrderRequest,
    ) -> Result<OrderResponse, ExchangeError> {
        let adapter = self.get_adapter(exchange)
            .ok_or_else(|| ExchangeError::Unknown {
                message: format!("No adapter found for exchange: {:?}", exchange)
            })?;
        
        adapter.place_order(request).await
    }
    
    /// 在指定交易所取消订单
    pub async fn cancel_order(
        &self,
        exchange: Exchange,
        order_id: &str,
        symbol: &Symbol,
    ) -> Result<OrderResponse, ExchangeError> {
        let adapter = self.get_adapter(exchange)
            .ok_or_else(|| ExchangeError::Unknown {
                message: format!("No adapter found for exchange: {:?}", exchange)
            })?;
        
        adapter.cancel_order(order_id, symbol).await
    }
    
    /// 查询订单状态
    pub async fn get_order(
        &self,
        exchange: Exchange,
        order_id: &str,
        symbol: &Symbol,
    ) -> Result<OrderResponse, ExchangeError> {
        let adapter = self.get_adapter(exchange)
            .ok_or_else(|| ExchangeError::Unknown {
                message: format!("No adapter found for exchange: {:?}", exchange)
            })?;
        
        adapter.get_order(order_id, symbol).await
    }
    
    /// 获取账户余额
    pub async fn get_balance(&self, exchange: Exchange) -> Result<Vec<Balance>, ExchangeError> {
        let adapter = self.get_adapter(exchange)
            .ok_or_else(|| ExchangeError::Unknown {
                message: format!("No adapter found for exchange: {:?}", exchange)
            })?;
        
        adapter.get_account_balance().await
    }
}

impl Default for OrderManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 创建默认的交易所配置
impl ExchangeConfig {
    pub fn new(exchange: Exchange, api_key: String, secret_key: String) -> Self {
        let (base_url, testnet_url) = match exchange {
            Exchange::Binance => (
                "https://api.binance.com".to_string(),
                Some("https://testnet.binance.vision".to_string()),
            ),
            Exchange::OKX => (
                "https://www.okx.com".to_string(),
                Some("https://www.okx.com".to_string()),
            ),
            Exchange::Gate => (
                "https://api.gateio.ws/api/v4".to_string(),
                Some("https://api-testnet.gateapi.io/api/v4".to_string()),
            ),
        };
        
        Self {
            exchange,
            api_key,
            secret_key,
            passphrase: None,
            base_url,
            testnet_url,
            use_testnet: false,
            timeout: Duration::from_secs(30),
            max_retries: 3,
            retry_delay: Duration::from_millis(1000),
            rate_limit: RateLimitConfig::default_for_exchange(exchange),
        }
    }
    
    pub fn with_passphrase(mut self, passphrase: String) -> Self {
        self.passphrase = Some(passphrase);
        self
    }
    
    pub fn with_testnet(mut self, use_testnet: bool) -> Self {
        self.use_testnet = use_testnet;
        self
    }
    
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl RateLimitConfig {
    pub fn default_for_exchange(exchange: Exchange) -> Self {
        match exchange {
            Exchange::Binance => Self {
                requests_per_second: 10,
                requests_per_minute: 1200,
                orders_per_second: 10,
                weight_per_minute: 1200,
            },
            Exchange::OKX => Self {
                requests_per_second: 30,
                requests_per_minute: 1800,
                orders_per_second: 30,
                weight_per_minute: 1800,
            },
            Exchange::Gate => Self {
                requests_per_second: 15,
                requests_per_minute: 900,
                orders_per_second: 15,
                weight_per_minute: 900,
            },
        }
    }
}

/// 订单构建器
pub struct OrderBuilder {
    request: OrderRequest,
}

impl OrderBuilder {
    pub fn new(symbol: Symbol, side: OrderSide, quantity: Decimal) -> Self {
        Self {
            request: OrderRequest {
                client_order_id: None,
                symbol,
                side,
                order_type: OrderType::Market,
                quantity,
                price: None,
                stop_price: None,
                time_in_force: None,
                reduce_only: None,
                metadata: HashMap::new(),
            },
        }
    }
    
    pub fn limit_order(mut self, price: Decimal) -> Self {
        self.request.order_type = OrderType::Limit;
        self.request.price = Some(price);
        self
    }
    
    pub fn market_order(mut self) -> Self {
        self.request.order_type = OrderType::Market;
        self.request.price = None;
        self
    }
    
    pub fn stop_loss(mut self, stop_price: Decimal) -> Self {
        self.request.order_type = OrderType::StopLoss;
        self.request.stop_price = Some(stop_price);
        self
    }
    
    pub fn client_order_id(mut self, id: String) -> Self {
        self.request.client_order_id = Some(id);
        self
    }
    
    pub fn time_in_force(mut self, tif: TimeInForce) -> Self {
        self.request.time_in_force = Some(tif);
        self
    }
    
    pub fn reduce_only(mut self, reduce_only: bool) -> Self {
        self.request.reduce_only = Some(reduce_only);
        self
    }
    
    pub fn metadata(mut self, key: String, value: String) -> Self {
        self.request.metadata.insert(key, value);
        self
    }
    
    pub fn build(self) -> OrderRequest {
        self.request
    }
}

