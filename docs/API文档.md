# API 接口文档

## 📡 交易所 API 集成

### Binance API
**基础信息**:
- Base URL: `https://api.binance.com`
- 测试网: `https://testnet.binance.vision`
- 认证: HMAC-SHA256

**核心接口**:
```rust
// 下单
POST /api/v3/order
{
    "symbol": "BTCUSDT",
    "side": "BUY",
    "type": "LIMIT",
    "quantity": "0.01",
    "price": "50000",
    "timeInForce": "GTC"
}

// 查询订单
GET /api/v3/order?symbol=BTCUSDT&orderId=123456

// 取消订单  
DELETE /api/v3/order?symbol=BTCUSDT&orderId=123456

// 账户信息
GET /api/v3/account
```

### OKX API
**基础信息**:
- Base URL: `https://www.okx.com`
- 认证: HMAC-SHA256 + Passphrase

**核心接口**:
```rust
// 下单
POST /api/v5/trade/order
{
    "instId": "BTC-USDT",
    "tdMode": "cash",
    "side": "buy",
    "ordType": "limit",
    "sz": "0.01",
    "px": "50000"
}

// 查询订单
GET /api/v5/trade/order?instId=BTC-USDT&ordId=123456

// 取消订单
POST /api/v5/trade/cancel-order
{
    "instId": "BTC-USDT",
    "ordId": "123456"
}

// 账户余额
GET /api/v5/account/balance
```

### Gate API
**基础信息**:
- Base URL: `https://api.gateio.ws/api/v4`
- 认证: HMAC-SHA512

**核心接口**:
```rust
// 下单
POST /spot/orders
{
    "currency_pair": "BTC_USDT",
    "side": "buy",
    "type": "limit",
    "amount": "0.01",
    "price": "50000"
}

// 查询订单
GET /spot/orders/123456?currency_pair=BTC_USDT

// 取消订单
DELETE /spot/orders/123456?currency_pair=BTC_USDT

// 账户余额
GET /spot/accounts
```

## 🔌 内部 API 接口

### 1. 订单管理 API

#### 下单接口
```rust
pub async fn place_order(
    exchange: Exchange,
    request: OrderRequest
) -> Result<OrderResponse>

// 请求参数
pub struct OrderRequest {
    pub symbol: Symbol,
    pub side: OrderSide,      // Buy, Sell
    pub order_type: OrderType, // Market, Limit
    pub quantity: Decimal,
    pub price: Option<Decimal>,
    pub time_in_force: Option<TimeInForce>, // GTC, IOC, FOK
}

// 响应数据
pub struct OrderResponse {
    pub order_id: String,
    pub status: OrderStatus,  // New, Filled, Canceled
    pub executed_quantity: Decimal,
    pub average_price: Option<Decimal>,
    pub created_at: DateTime<Utc>,
}
```

#### 查询订单
```rust
pub async fn get_order(
    exchange: Exchange,
    order_id: &str,
    symbol: &Symbol
) -> Result<OrderResponse>
```

#### 取消订单
```rust
pub async fn cancel_order(
    exchange: Exchange,
    order_id: &str,
    symbol: &Symbol
) -> Result<OrderResponse>
```

### 2. 账户管理 API

#### 查询余额
```rust
pub async fn get_balance(
    exchange: Exchange
) -> Result<Vec<Balance>>

pub struct Balance {
    pub asset: String,
    pub free: Decimal,      // 可用余额
    pub locked: Decimal,    // 冻结余额
    pub total: Decimal,     // 总余额
}
```

#### 查询持仓
```rust
pub async fn get_positions(
    exchange: Exchange
) -> Result<Vec<Position>>

pub struct Position {
    pub symbol: Symbol,
    pub quantity: Decimal,
    pub average_price: Decimal,
    pub unrealized_pnl: Decimal,
    pub realized_pnl: Decimal,
}
```

### 3. 市场数据 API

#### 获取行情
```rust
pub async fn get_ticker(
    exchange: Exchange,
    symbol: &Symbol
) -> Result<Ticker>

pub struct Ticker {
    pub symbol: Symbol,
    pub bid: Decimal,
    pub ask: Decimal,
    pub last_price: Decimal,
    pub volume_24h: Decimal,
    pub change_24h: Decimal,
}
```

#### 获取订单簿
```rust
pub async fn get_orderbook(
    exchange: Exchange,
    symbol: &Symbol,
    depth: u32
) -> Result<OrderBook>

pub struct OrderBook {
    pub symbol: Symbol,
    pub bids: Vec<PriceLevel>,
    pub asks: Vec<PriceLevel>,
    pub timestamp: DateTime<Utc>,
}

pub struct PriceLevel {
    pub price: Decimal,
    pub quantity: Decimal,
}
```

### 4. 策略管理 API

#### 启动策略
```rust
pub async fn start_strategy(
    strategy_name: &str,
    config: StrategyConfig
) -> Result<()>

pub struct StrategyConfig {
    pub enabled: bool,
    pub symbols: Vec<Symbol>,
    pub parameters: HashMap<String, Value>,
}
```

#### 停止策略
```rust
pub async fn stop_strategy(
    strategy_name: &str
) -> Result<()>
```

#### 查询策略状态
```rust
pub async fn get_strategy_status(
    strategy_name: &str
) -> Result<StrategyStatus>

pub struct StrategyStatus {
    pub name: String,
    pub status: String,     // Running, Stopped, Error
    pub pnl: Decimal,
    pub orders_count: u32,
    pub last_update: DateTime<Utc>,
}
```

## 🔒 认证机制

### API 签名算法
```rust
pub fn create_signature(
    secret_key: &str,
    timestamp: u64,
    method: &str,
    path: &str,
    body: &str
) -> String {
    let message = format!("{}{}{}{}", timestamp, method, path, body);
    let signature = hmac_sha256(secret_key.as_bytes(), message.as_bytes());
    hex::encode(signature)
}
```

### 请求头设置
```rust
headers.insert("X-API-KEY", api_key);
headers.insert("X-TIMESTAMP", timestamp.to_string());
headers.insert("X-SIGNATURE", signature);
```

## 📊 WebSocket 数据流

### 市场数据订阅
```rust
// Binance WebSocket
wss://stream.binance.com:9443/ws/btcusdt@ticker
wss://stream.binance.com:9443/ws/btcusdt@depth

// OKX WebSocket  
wss://ws.okx.com:8443/ws/v5/public
{
    "op": "subscribe",
    "args": [{"channel": "tickers", "instId": "BTC-USDT"}]
}

// Gate WebSocket
wss://api.gateio.ws/ws/v4/
{
    "method": "ticker.subscribe",
    "params": ["BTC_USDT"],
    "id": 1
}
```

### 私有数据订阅
```rust
// 订单更新
{
    "method": "order.subscribe",
    "params": ["BTC_USDT"],
    "id": 1
}

// 余额更新
{
    "method": "balance.subscribe", 
    "params": [],
    "id": 2
}
```

## 🚨 错误处理

### 错误代码定义
```rust
pub enum HftError {
    NetworkError(String),      // 网络错误
    ApiError(i32, String),     // API 错误
    AuthError(String),         // 认证错误
    RateLimitError(String),    // 限流错误
    InsufficientBalance(String), // 余额不足
    InvalidOrder(String),      // 无效订单
    SystemError(String),       // 系统错误
}
```

### 重试机制
```rust
pub struct RetryConfig {
    pub max_retries: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub backoff_factor: f64,
}

pub async fn retry_with_backoff<F, T>(
    operation: F,
    config: &RetryConfig
) -> Result<T>
where
    F: Fn() -> Future<Output = Result<T>>,
{
    // 指数退避重试逻辑
}
```

## 📈 性能指标

### API 性能要求
- **延迟**: < 10ms (99th percentile)
- **吞吐量**: > 1000 requests/second
- **可用性**: > 99.9%
- **错误率**: < 0.1%

### 监控指标
```rust
pub struct ApiMetrics {
    pub request_count: u64,
    pub success_count: u64,
    pub error_count: u64,
    pub avg_latency: Duration,
    pub p99_latency: Duration,
}
```

## 🔧 配置示例

### API 配置
```toml
[api]
timeout = 30
max_retries = 3
rate_limit = 1000

[exchanges.binance]
api_key = "${BINANCE_API_KEY}"
secret_key = "${BINANCE_SECRET_KEY}"
base_url = "https://api.binance.com"
testnet = false

[exchanges.okx]
api_key = "${OKX_API_KEY}"
secret_key = "${OKX_SECRET_KEY}"
passphrase = "${OKX_PASSPHRASE}"
base_url = "https://www.okx.com"

[exchanges.gate]
api_key = "${GATE_API_KEY}"
secret_key = "${GATE_SECRET_KEY}"
base_url = "https://api.gateio.ws/api/v4"
```

---

**API 版本**: v1.0  
**最后更新**: 2025-01-01

