use super::exchange_adapter::*;
use crate::common::{Exchange, Result, Symbol};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use reqwest::{Client, Method};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sha2::Sha512;
use std::collections::HashMap;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{sleep, Duration};

type HmacSha512 = Hmac<Sha512>;

/// Gate.io API响应结构
#[derive(Debug, Deserialize)]
struct GateServerTime {
    server_time: i64,
}

#[derive(Debug, Deserialize)]
struct GateCurrencyPair {
    id: String,
    base: String,
    quote: String,
    fee: String,
    min_base_amount: String,
    min_quote_amount: String,
    amount_precision: i32,
    precision: i32,
    trade_status: String,
    sell_start: i64,
    buy_start: i64,
}

#[derive(Debug, Serialize)]
struct GateOrderRequest {
    currency_pair: String,
    side: String,
    #[serde(rename = "type")]
    order_type: String,
    amount: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_in_force: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    iceberg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_borrow: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_repay: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct GateOrderResponse {
    id: String,
    text: String,
    create_time: String,
    update_time: String,
    create_time_ms: String,
    update_time_ms: String,
    status: String,
    currency_pair: String,
    #[serde(rename = "type")]
    order_type: String,
    account: String,
    side: String,
    amount: String,
    price: String,
    time_in_force: String,
    iceberg: String,
    auto_borrow: bool,
    auto_repay: bool,
    left: String,
    filled_total: String,
    fee: String,
    fee_currency: String,
    point_fee: String,
    gt_fee: String,
    gt_maker_fee: String,
    gt_taker_fee: String,
    gt_discount: bool,
    rebated_fee: String,
    rebated_fee_currency: String,
}

#[derive(Debug, Deserialize)]
struct GateBalance {
    currency: String,
    available: String,
    locked: String,
}

#[derive(Debug, Deserialize)]
struct GateOrderBook {
    id: u64,
    current: i64,
    update: i64,
    asks: Vec<[String; 2]>,
    bids: Vec<[String; 2]>,
}

#[derive(Debug, Deserialize)]
struct GateTicker {
    currency_pair: String,
    last: String,
    lowest_ask: String,
    highest_bid: String,
    change_percentage: String,
    base_volume: String,
    quote_volume: String,
    high_24h: String,
    low_24h: String,
}

/// Gate.io交易所适配器
pub struct GateAdapter {
    config: ExchangeConfig,
    client: Client,
    symbol_info_cache: tokio::sync::RwLock<HashMap<String, SymbolInfo>>,
}

impl GateAdapter {
    pub fn new(config: ExchangeConfig) -> Self {
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            config,
            client,
            symbol_info_cache: tokio::sync::RwLock::new(HashMap::new()),
        }
    }

    /// 获取基础URL
    fn base_url(&self) -> &str {
        if self.config.use_testnet {
            self.config.testnet_url.as_ref().unwrap_or(&self.config.base_url)
        } else {
            &self.config.base_url
        }
    }

    /// 生成签名
    fn generate_signature(&self, method: &str, uri: &str, query: &str, body: &str, timestamp: &str) -> String {
        let message = format!("{}\n{}\n{}\n{}\n{}", method, uri, query, body, timestamp);
        
        let mut mac = HmacSha512::new_from_slice(self.config.secret_key.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(message.as_bytes());
        
        hex::encode(mac.finalize().into_bytes())
    }

    /// 获取当前时间戳
    fn get_timestamp(&self) -> String {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string()
    }

    /// 发送认证请求
    async fn send_authenticated_request<T>(
        &self,
        method: Method,
        endpoint: &str,
        query: Option<&str>,
        body: Option<String>,
    ) -> Result<T, ExchangeError>
    where
        T: for<'de> Deserialize<'de>,
    {
        let timestamp = self.get_timestamp();
        let query_str = query.unwrap_or("");
        let body_str = body.unwrap_or_default();
        
        let signature = self.generate_signature(
            method.as_str(),
            endpoint,
            query_str,
            &body_str,
            &timestamp,
        );

        let url = if query_str.is_empty() {
            format!("{}{}", self.base_url(), endpoint)
        } else {
            format!("{}{}?{}", self.base_url(), endpoint, query_str)
        };
        
        let mut request = self.client.request(method, &url)
            .header("KEY", &self.config.api_key)
            .header("Timestamp", &timestamp)
            .header("SIGN", &signature)
            .header("Content-Type", "application/json");

        if !body_str.is_empty() {
            request = request.body(body_str);
        }

        let response = request.send().await
            .map_err(|e| ExchangeError::NetworkError { 
                message: e.to_string() 
            })?;

        if !response.status().is_success() {
            let error_text = response.text().await
                .unwrap_or_else(|_| "Unknown error".to_string());
            
            return Err(match response.status().as_u16() {
                401 => ExchangeError::AuthenticationError { 
                    message: error_text 
                },
                403 => ExchangeError::AuthenticationError { 
                    message: error_text 
                },
                404 => ExchangeError::OrderNotFound { 
                    order_id: "unknown".to_string() 
                },
                429 => ExchangeError::RateLimitError { 
                    retry_after: Some(Duration::from_secs(60)) 
                },
                _ => ExchangeError::ApiError { 
                    code: response.status().to_string(), 
                    message: error_text 
                },
            });
        }

        let text = response.text().await
            .map_err(|e| ExchangeError::NetworkError { 
                message: e.to_string() 
            })?;

        serde_json::from_str(&text)
            .map_err(|e| ExchangeError::SerializationError { 
                message: format!("Failed to parse response: {}, text: {}", e, text) 
            })
    }

    /// 发送公共请求
    async fn send_public_request<T>(&self, endpoint: &str, query: Option<&str>) -> Result<T, ExchangeError>
    where
        T: for<'de> Deserialize<'de>,
    {
        let url = if let Some(query) = query {
            format!("{}{}?{}", self.base_url(), endpoint, query)
        } else {
            format!("{}{}", self.base_url(), endpoint)
        };
        
        let response = self.client.get(&url).send().await
            .map_err(|e| ExchangeError::NetworkError { 
                message: e.to_string() 
            })?;

        if !response.status().is_success() {
            let error_text = response.text().await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(ExchangeError::ApiError {
                code: response.status().to_string(),
                message: error_text,
            });
        }

        let text = response.text().await
            .map_err(|e| ExchangeError::NetworkError { 
                message: e.to_string() 
            })?;

        serde_json::from_str(&text)
            .map_err(|e| ExchangeError::SerializationError { 
                message: format!("Failed to parse response: {}, text: {}", e, text) 
            })
    }

    /// 转换订单状态
    fn convert_order_status(status: &str) -> OrderStatus {
        match status {
            "open" => OrderStatus::New,
            "closed" => OrderStatus::Filled,
            "cancelled" => OrderStatus::Canceled,
            _ => OrderStatus::New,
        }
    }

    /// 转换订单类型
    fn convert_order_type_to_gate(order_type: OrderType) -> String {
        match order_type {
            OrderType::Market => "market".to_string(),
            OrderType::Limit => "limit".to_string(),
            _ => "limit".to_string(), // Gate.io主要支持limit和market
        }
    }

    /// 转换订单类型从Gate
    fn convert_order_type_from_gate(order_type: &str) -> OrderType {
        match order_type {
            "market" => OrderType::Market,
            "limit" => OrderType::Limit,
            _ => OrderType::Limit,
        }
    }

    /// 转换时间有效性
    fn convert_time_in_force(tif: TimeInForce) -> String {
        match tif {
            TimeInForce::GTC => "gtc".to_string(),
            TimeInForce::IOC => "ioc".to_string(),
            TimeInForce::FOK => "fok".to_string(),
            TimeInForce::PostOnly => "poc".to_string(),
            _ => "gtc".to_string(),
        }
    }

    /// 重试机制
    async fn retry_request<F, T>(&self, mut operation: F) -> Result<T, ExchangeError>
    where
        F: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, ExchangeError>> + Send>>,
    {
        let mut last_error = None;
        
        for attempt in 0..=self.config.max_retries {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    last_error = Some(e);
                    
                    if attempt < self.config.max_retries {
                        let delay = self.config.retry_delay * (2_u32.pow(attempt));
                        sleep(delay).await;
                    }
                }
            }
        }
        
        Err(last_error.unwrap())
    }
}

#[async_trait]
impl ExchangeAdapter for GateAdapter {
    fn exchange(&self) -> Exchange {
        Exchange::Gate
    }

    fn config(&self) -> &ExchangeConfig {
        &self.config
    }

    async fn test_connection(&self) -> Result<(), ExchangeError> {
        self.get_server_time().await?;
        Ok(())
    }

    async fn get_server_time(&self) -> Result<DateTime<Utc>, ExchangeError> {
        let response: GateServerTime = self.send_public_request("/spot/time", None).await?;
        Ok(DateTime::from_timestamp(response.server_time, 0).unwrap_or_else(Utc::now))
    }

    async fn get_symbol_info(&self, symbol: &Symbol) -> Result<SymbolInfo, ExchangeError> {
        let symbol_str = self.normalize_symbol(symbol);
        
        // 检查缓存
        {
            let cache = self.symbol_info_cache.read().await;
            if let Some(info) = cache.get(&symbol_str) {
                return Ok(info.clone());
            }
        }

        // 从API获取
        let response: Vec<GateCurrencyPair> = self.send_public_request("/spot/currency_pairs", None).await?;
        
        let gate_pair = response
            .iter()
            .find(|p| p.id == symbol_str)
            .ok_or_else(|| ExchangeError::InvalidSymbol { 
                symbol: symbol_str.clone() 
            })?;

        let symbol_info = SymbolInfo {
            symbol: symbol.clone(),
            base_asset: gate_pair.base.clone(),
            quote_asset: gate_pair.quote.clone(),
            is_trading: gate_pair.trade_status == "tradable",
            min_quantity: Decimal::from_str(&gate_pair.min_base_amount).unwrap_or_default(),
            max_quantity: Decimal::from_str("9000000000").unwrap(),
            quantity_step: Decimal::from_str("0.00000001").unwrap(),
            min_price: Decimal::from_str("0.00000001").unwrap(),
            max_price: Decimal::from_str("1000000").unwrap(),
            price_step: Decimal::from_str("0.00000001").unwrap(),
            min_notional: Decimal::from_str(&gate_pair.min_quote_amount).unwrap_or_default(),
            quantity_precision: gate_pair.amount_precision as u32,
            price_precision: gate_pair.precision as u32,
            fee_rate: Some(Decimal::from_str(&gate_pair.fee).unwrap_or_default()),
        };

        // 更新缓存
        {
            let mut cache = self.symbol_info_cache.write().await;
            cache.insert(symbol_str, symbol_info.clone());
        }

        Ok(symbol_info)
    }

    async fn get_all_symbols(&self) -> Result<Vec<SymbolInfo>, ExchangeError> {
        let response: Vec<GateCurrencyPair> = self.send_public_request("/spot/currency_pairs", None).await?;
        
        let mut symbols = Vec::new();
        for gate_pair in response {
            if gate_pair.trade_status == "tradable" {
                if let Ok(symbol) = self.parse_symbol(&gate_pair.id) {
                    if let Ok(info) = self.get_symbol_info(&symbol).await {
                        symbols.push(info);
                    }
                }
            }
        }
        
        Ok(symbols)
    }

    async fn place_order(&self, request: &OrderRequest) -> Result<OrderResponse, ExchangeError> {
        let symbol_str = self.normalize_symbol(&request.symbol);
        
        let gate_request = GateOrderRequest {
            currency_pair: symbol_str,
            side: match request.side {
                OrderSide::Buy => "buy".to_string(),
                OrderSide::Sell => "sell".to_string(),
            },
            order_type: Self::convert_order_type_to_gate(request.order_type),
            amount: request.quantity.to_string(),
            price: request.price.map(|p| p.to_string()),
            text: request.client_order_id.clone(),
            time_in_force: request.time_in_force.map(Self::convert_time_in_force),
            iceberg: None,
            auto_borrow: None,
            auto_repay: None,
        };

        let body = serde_json::to_string(&gate_request)
            .map_err(|e| ExchangeError::SerializationError { 
                message: format!("Failed to serialize request: {}", e) 
            })?;

        let response: GateOrderResponse = self.send_authenticated_request(
            Method::POST,
            "/spot/orders",
            None,
            Some(body),
        ).await?;

        Ok(OrderResponse {
            order_id: response.id,
            client_order_id: Some(response.text),
            symbol: request.symbol.clone(),
            side: request.side,
            order_type: Self::convert_order_type_from_gate(&response.order_type),
            original_quantity: Decimal::from_str(&response.amount).unwrap_or_default(),
            executed_quantity: Decimal::from_str(&response.amount).unwrap_or_default() 
                - Decimal::from_str(&response.left).unwrap_or_default(),
            remaining_quantity: Decimal::from_str(&response.left).unwrap_or_default(),
            price: if response.price != "0" { 
                Some(Decimal::from_str(&response.price).unwrap_or_default()) 
            } else { 
                None 
            },
            average_price: None, // Gate.io doesn't provide this directly
            status: Self::convert_order_status(&response.status),
            time_in_force: request.time_in_force,
            created_at: DateTime::from_timestamp(
                response.create_time.parse::<i64>().unwrap_or_default(), 0
            ).unwrap_or_else(Utc::now),
            updated_at: DateTime::from_timestamp(
                response.update_time.parse::<i64>().unwrap_or_default(), 0
            ).unwrap_or_else(Utc::now),
            exchange_data: HashMap::new(),
        })
    }

    async fn cancel_order(&self, order_id: &str, symbol: &Symbol) -> Result<OrderResponse, ExchangeError> {
        let symbol_str = self.normalize_symbol(symbol);
        let endpoint = format!("/spot/orders/{}", order_id);
        let query = format!("currency_pair={}", symbol_str);

        let response: GateOrderResponse = self.send_authenticated_request(
            Method::DELETE,
            &endpoint,
            Some(&query),
            None,
        ).await?;

        Ok(OrderResponse {
            order_id: response.id,
            client_order_id: Some(response.text),
            symbol: symbol.clone(),
            side: match response.side.as_str() {
                "buy" => OrderSide::Buy,
                "sell" => OrderSide::Sell,
                _ => OrderSide::Buy,
            },
            order_type: Self::convert_order_type_from_gate(&response.order_type),
            original_quantity: Decimal::from_str(&response.amount).unwrap_or_default(),
            executed_quantity: Decimal::from_str(&response.amount).unwrap_or_default() 
                - Decimal::from_str(&response.left).unwrap_or_default(),
            remaining_quantity: Decimal::from_str(&response.left).unwrap_or_default(),
            price: if response.price != "0" { 
                Some(Decimal::from_str(&response.price).unwrap_or_default()) 
            } else { 
                None 
            },
            average_price: None,
            status: Self::convert_order_status(&response.status),
            time_in_force: None,
            created_at: DateTime::from_timestamp(
                response.create_time.parse::<i64>().unwrap_or_default(), 0
            ).unwrap_or_else(Utc::now),
            updated_at: DateTime::from_timestamp(
                response.update_time.parse::<i64>().unwrap_or_default(), 0
            ).unwrap_or_else(Utc::now),
            exchange_data: HashMap::new(),
        })
    }

    async fn cancel_all_orders(&self, symbol: Option<&Symbol>) -> Result<Vec<OrderResponse>, ExchangeError> {
        // Gate.io doesn't have a direct cancel all orders API
        // We need to get open orders first, then cancel them
        let open_orders = self.get_open_orders(symbol).await?;
        let mut results = Vec::new();
        
        for order in open_orders {
            match self.cancel_order(&order.order_id, &order.symbol).await {
                Ok(response) => results.push(response),
                Err(e) => {
                    log::warn!("Failed to cancel order {}: {}", order.order_id, e);
                }
            }
        }
        
        Ok(results)
    }

    async fn get_order(&self, order_id: &str, symbol: &Symbol) -> Result<OrderResponse, ExchangeError> {
        let symbol_str = self.normalize_symbol(symbol);
        let endpoint = format!("/spot/orders/{}", order_id);
        let query = format!("currency_pair={}", symbol_str);
        
        let response: GateOrderResponse = self.send_authenticated_request(
            Method::GET,
            &endpoint,
            Some(&query),
            None,
        ).await?;

        Ok(OrderResponse {
            order_id: response.id,
            client_order_id: Some(response.text),
            symbol: symbol.clone(),
            side: match response.side.as_str() {
                "buy" => OrderSide::Buy,
                "sell" => OrderSide::Sell,
                _ => OrderSide::Buy,
            },
            order_type: Self::convert_order_type_from_gate(&response.order_type),
            original_quantity: Decimal::from_str(&response.amount).unwrap_or_default(),
            executed_quantity: Decimal::from_str(&response.amount).unwrap_or_default() 
                - Decimal::from_str(&response.left).unwrap_or_default(),
            remaining_quantity: Decimal::from_str(&response.left).unwrap_or_default(),
            price: if response.price != "0" { 
                Some(Decimal::from_str(&response.price).unwrap_or_default()) 
            } else { 
                None 
            },
            average_price: None,
            status: Self::convert_order_status(&response.status),
            time_in_force: None,
            created_at: DateTime::from_timestamp(
                response.create_time.parse::<i64>().unwrap_or_default(), 0
            ).unwrap_or_else(Utc::now),
            updated_at: DateTime::from_timestamp(
                response.update_time.parse::<i64>().unwrap_or_default(), 0
            ).unwrap_or_else(Utc::now),
            exchange_data: HashMap::new(),
        })
    }

    async fn get_open_orders(&self, symbol: Option<&Symbol>) -> Result<Vec<OrderResponse>, ExchangeError> {
        let mut query = "status=open".to_string();
        
        if let Some(symbol) = symbol {
            query.push_str(&format!("&currency_pair={}", self.normalize_symbol(symbol)));
        }

        let response: Vec<GateOrderResponse> = self.send_authenticated_request(
            Method::GET,
            "/spot/orders",
            Some(&query),
            None,
        ).await?;

        let mut orders = Vec::new();
        for order in response {
            let symbol = self.parse_symbol(&order.currency_pair)?;
            orders.push(OrderResponse {
                order_id: order.id,
                client_order_id: Some(order.text),
                symbol,
                side: match order.side.as_str() {
                    "buy" => OrderSide::Buy,
                    "sell" => OrderSide::Sell,
                    _ => OrderSide::Buy,
                },
                order_type: Self::convert_order_type_from_gate(&order.order_type),
                original_quantity: Decimal::from_str(&order.amount).unwrap_or_default(),
                executed_quantity: Decimal::from_str(&order.amount).unwrap_or_default() 
                    - Decimal::from_str(&order.left).unwrap_or_default(),
                remaining_quantity: Decimal::from_str(&order.left).unwrap_or_default(),
                price: if order.price != "0" { 
                    Some(Decimal::from_str(&order.price).unwrap_or_default()) 
                } else { 
                    None 
                },
                average_price: None,
                status: Self::convert_order_status(&order.status),
                time_in_force: None,
                created_at: DateTime::from_timestamp(
                    order.create_time.parse::<i64>().unwrap_or_default(), 0
                ).unwrap_or_else(Utc::now),
                updated_at: DateTime::from_timestamp(
                    order.update_time.parse::<i64>().unwrap_or_default(), 0
                ).unwrap_or_else(Utc::now),
                exchange_data: HashMap::new(),
            });
        }

        Ok(orders)
    }

    async fn get_order_history(
        &self,
        symbol: Option<&Symbol>,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> Result<Vec<OrderResponse>, ExchangeError> {
        let mut query_params = Vec::new();
        
        if let Some(symbol) = symbol {
            query_params.push(format!("currency_pair={}", self.normalize_symbol(symbol)));
        }
        
        if let Some(start_time) = start_time {
            query_params.push(format!("start={}", start_time.timestamp()));
        }
        
        if let Some(end_time) = end_time {
            query_params.push(format!("end={}", end_time.timestamp()));
        }
        
        if let Some(limit) = limit {
            query_params.push(format!("limit={}", limit));
        }

        let query = if query_params.is_empty() {
            None
        } else {
            Some(query_params.join("&"))
        };

        let response: Vec<GateOrderResponse> = self.send_authenticated_request(
            Method::GET,
            "/spot/orders",
            query.as_deref(),
            None,
        ).await?;

        let mut orders = Vec::new();
        for order in response {
            let symbol = self.parse_symbol(&order.currency_pair)?;
            orders.push(OrderResponse {
                order_id: order.id,
                client_order_id: Some(order.text),
                symbol,
                side: match order.side.as_str() {
                    "buy" => OrderSide::Buy,
                    "sell" => OrderSide::Sell,
                    _ => OrderSide::Buy,
                },
                order_type: Self::convert_order_type_from_gate(&order.order_type),
                original_quantity: Decimal::from_str(&order.amount).unwrap_or_default(),
                executed_quantity: Decimal::from_str(&order.amount).unwrap_or_default() 
                    - Decimal::from_str(&order.left).unwrap_or_default(),
                remaining_quantity: Decimal::from_str(&order.left).unwrap_or_default(),
                price: if order.price != "0" { 
                    Some(Decimal::from_str(&order.price).unwrap_or_default()) 
                } else { 
                    None 
                },
                average_price: None,
                status: Self::convert_order_status(&order.status),
                time_in_force: None,
                created_at: DateTime::from_timestamp(
                    order.create_time.parse::<i64>().unwrap_or_default(), 0
                ).unwrap_or_else(Utc::now),
                updated_at: DateTime::from_timestamp(
                    order.update_time.parse::<i64>().unwrap_or_default(), 0
                ).unwrap_or_else(Utc::now),
                exchange_data: HashMap::new(),
            });
        }

        Ok(orders)
    }

    async fn get_account_balance(&self) -> Result<Vec<Balance>, ExchangeError> {
        let response: Vec<GateBalance> = self.send_authenticated_request(
            Method::GET,
            "/spot/accounts",
            None,
            None,
        ).await?;

        let mut balances = Vec::new();
        for balance in response {
            let available = Decimal::from_str(&balance.available).unwrap_or_default();
            let locked = Decimal::from_str(&balance.locked).unwrap_or_default();
            let total = available + locked;
            
            if total > Decimal::ZERO {
                balances.push(Balance {
                    asset: balance.currency,
                    free: available,
                    locked,
                    total,
                });
            }
        }

        Ok(balances)
    }

    async fn get_asset_balance(&self, asset: &str) -> Result<Balance, ExchangeError> {
        let balances = self.get_account_balance().await?;
        
        balances.into_iter()
            .find(|b| b.asset == asset)
            .ok_or_else(|| ExchangeError::Unknown { 
                message: format!("Asset {} not found", asset) 
            })
    }

    async fn get_trade_history(
        &self,
        symbol: Option<&Symbol>,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> Result<Vec<Trade>, ExchangeError> {
        // Gate.io trade history implementation
        // This is a simplified version
        Ok(Vec::new())
    }

    async fn get_ticker_price(&self, symbol: &Symbol) -> Result<Decimal, ExchangeError> {
        let symbol_str = self.normalize_symbol(symbol);
        let query = format!("currency_pair={}", symbol_str);
        
        let response: GateTicker = self.send_public_request("/spot/tickers", Some(&query)).await?;
        
        Decimal::from_str(&response.last)
            .map_err(|e| ExchangeError::SerializationError { 
                message: format!("Failed to parse price: {}", e) 
            })
    }

    async fn get_order_book(&self, symbol: &Symbol, limit: Option<u32>) -> Result<OrderBookSnapshot, ExchangeError> {
        let symbol_str = self.normalize_symbol(symbol);
        let mut query = format!("currency_pair={}", symbol_str);
        
        if let Some(limit) = limit {
            query.push_str(&format!("&limit={}", limit));
        }
        
        let response: GateOrderBook = self.send_public_request("/spot/order_book", Some(&query)).await?;
        
        let mut bids = Vec::new();
        for bid in response.bids {
            bids.push(OrderBookLevel {
                price: Decimal::from_str(&bid[0]).unwrap_or_default(),
                quantity: Decimal::from_str(&bid[1]).unwrap_or_default(),
            });
        }
        
        let mut asks = Vec::new();
        for ask in response.asks {
            asks.push(OrderBookLevel {
                price: Decimal::from_str(&ask[0]).unwrap_or_default(),
                quantity: Decimal::from_str(&ask[1]).unwrap_or_default(),
            });
        }
        
        Ok(OrderBookSnapshot {
            symbol: symbol.clone(),
            bids,
            asks,
            timestamp: DateTime::from_timestamp(response.current, 0).unwrap_or_else(Utc::now),
            last_update_id: Some(response.id),
        })
    }

    fn normalize_symbol(&self, symbol: &Symbol) -> String {
        format!("{}_{}", symbol.base, symbol.quote)
    }

    fn parse_symbol(&self, symbol_str: &str) -> Result<Symbol, ExchangeError> {
        let parts: Vec<&str> = symbol_str.split('_').collect();
        if parts.len() != 2 {
            return Err(ExchangeError::InvalidSymbol { 
                symbol: symbol_str.to_string() 
            });
        }
        
        Ok(Symbol::new(parts[0], parts[1], Exchange::Gate))
    }

    fn format_quantity(&self, symbol: &Symbol, quantity: Decimal) -> Result<Decimal, ExchangeError> {
        // This should use the symbol info to format correctly
        // For now, return as-is
        Ok(quantity)
    }

    fn format_price(&self, symbol: &Symbol, price: Decimal) -> Result<Decimal, ExchangeError> {
        // This should use the symbol info to format correctly
        // For now, return as-is
        Ok(price)
    }
}

