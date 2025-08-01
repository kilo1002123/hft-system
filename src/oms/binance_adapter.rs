use super::exchange_adapter::*;
use crate::common::{Exchange, Result, Symbol};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use reqwest::{Client, Method};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashMap;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{sleep, Duration};

type HmacSha256 = Hmac<Sha256>;

/// Binance API响应结构
#[derive(Debug, Deserialize)]
struct BinanceServerTime {
    #[serde(rename = "serverTime")]
    server_time: i64,
}

#[derive(Debug, Deserialize)]
struct BinanceSymbolInfo {
    symbol: String,
    status: String,
    #[serde(rename = "baseAsset")]
    base_asset: String,
    #[serde(rename = "quoteAsset")]
    quote_asset: String,
    filters: Vec<BinanceFilter>,
}

#[derive(Debug, Deserialize)]
struct BinanceFilter {
    #[serde(rename = "filterType")]
    filter_type: String,
    #[serde(rename = "minQty")]
    min_qty: Option<String>,
    #[serde(rename = "maxQty")]
    max_qty: Option<String>,
    #[serde(rename = "stepSize")]
    step_size: Option<String>,
    #[serde(rename = "minPrice")]
    min_price: Option<String>,
    #[serde(rename = "maxPrice")]
    max_price: Option<String>,
    #[serde(rename = "tickSize")]
    tick_size: Option<String>,
    #[serde(rename = "minNotional")]
    min_notional: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BinanceExchangeInfo {
    symbols: Vec<BinanceSymbolInfo>,
}

#[derive(Debug, Serialize)]
struct BinanceOrderRequest {
    symbol: String,
    side: String,
    #[serde(rename = "type")]
    order_type: String,
    #[serde(rename = "timeInForce", skip_serializing_if = "Option::is_none")]
    time_in_force: Option<String>,
    quantity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    price: Option<String>,
    #[serde(rename = "newClientOrderId", skip_serializing_if = "Option::is_none")]
    new_client_order_id: Option<String>,
    #[serde(rename = "stopPrice", skip_serializing_if = "Option::is_none")]
    stop_price: Option<String>,
    #[serde(rename = "recvWindow")]
    recv_window: u64,
    timestamp: u64,
}

#[derive(Debug, Deserialize)]
struct BinanceOrderResponse {
    symbol: String,
    #[serde(rename = "orderId")]
    order_id: u64,
    #[serde(rename = "orderListId")]
    order_list_id: i64,
    #[serde(rename = "clientOrderId")]
    client_order_id: String,
    #[serde(rename = "transactTime")]
    transact_time: u64,
    price: String,
    #[serde(rename = "origQty")]
    orig_qty: String,
    #[serde(rename = "executedQty")]
    executed_qty: String,
    #[serde(rename = "cummulativeQuoteQty")]
    cummulative_quote_qty: String,
    status: String,
    #[serde(rename = "timeInForce")]
    time_in_force: String,
    #[serde(rename = "type")]
    order_type: String,
    side: String,
    #[serde(rename = "workingTime")]
    working_time: u64,
    #[serde(rename = "selfTradePreventionMode")]
    self_trade_prevention_mode: String,
}

#[derive(Debug, Deserialize)]
struct BinanceBalance {
    asset: String,
    free: String,
    locked: String,
}

#[derive(Debug, Deserialize)]
struct BinanceAccountInfo {
    balances: Vec<BinanceBalance>,
}

#[derive(Debug, Deserialize)]
struct BinanceOrderBookResponse {
    #[serde(rename = "lastUpdateId")]
    last_update_id: u64,
    bids: Vec<[String; 2]>,
    asks: Vec<[String; 2]>,
}

#[derive(Debug, Deserialize)]
struct BinanceTickerPrice {
    symbol: String,
    price: String,
}

#[derive(Debug, Deserialize)]
struct BinanceError {
    code: i32,
    msg: String,
}

/// Binance交易所适配器
pub struct BinanceAdapter {
    config: ExchangeConfig,
    client: Client,
    symbol_info_cache: tokio::sync::RwLock<HashMap<String, SymbolInfo>>,
}

impl BinanceAdapter {
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
    fn generate_signature(&self, query_string: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(self.config.secret_key.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(query_string.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    /// 获取当前时间戳
    fn get_timestamp(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    /// 构建查询字符串
    fn build_query_string(&self, params: &HashMap<String, String>) -> String {
        let mut query_params: Vec<String> = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        query_params.sort();
        query_params.join("&")
    }

    /// 发送认证请求
    async fn send_authenticated_request<T>(
        &self,
        method: Method,
        endpoint: &str,
        params: HashMap<String, String>,
    ) -> Result<T, ExchangeError>
    where
        T: for<'de> Deserialize<'de>,
    {
        let mut all_params = params;
        all_params.insert("timestamp".to_string(), self.get_timestamp().to_string());
        all_params.insert("recvWindow".to_string(), "5000".to_string());

        let query_string = self.build_query_string(&all_params);
        let signature = self.generate_signature(&query_string);
        let signed_query = format!("{}&signature={}", query_string, signature);

        let url = format!("{}{}", self.base_url(), endpoint);
        let full_url = if method == Method::GET {
            format!("{}?{}", url, signed_query)
        } else {
            url
        };

        let mut request = self.client.request(method, &full_url)
            .header("X-MBX-APIKEY", &self.config.api_key);

        if method != Method::GET {
            request = request.header("Content-Type", "application/x-www-form-urlencoded")
                .body(signed_query);
        }

        let response = request.send().await
            .map_err(|e| ExchangeError::NetworkError { 
                message: e.to_string() 
            })?;

        if !response.status().is_success() {
            let error_text = response.text().await
                .unwrap_or_else(|_| "Unknown error".to_string());
            
            // 尝试解析Binance错误格式
            if let Ok(binance_error) = serde_json::from_str::<BinanceError>(&error_text) {
                return Err(match binance_error.code {
                    -1021 => ExchangeError::TimeoutError,
                    -1022 => ExchangeError::AuthenticationError { 
                        message: binance_error.msg 
                    },
                    -2010 => ExchangeError::OrderNotFound { 
                        order_id: "unknown".to_string() 
                    },
                    -2011 => ExchangeError::OrderNotFound { 
                        order_id: "unknown".to_string() 
                    },
                    _ => ExchangeError::ApiError { 
                        code: binance_error.code.to_string(), 
                        message: binance_error.msg 
                    },
                });
            }

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

    /// 发送公共请求
    async fn send_public_request<T>(&self, endpoint: &str) -> Result<T, ExchangeError>
    where
        T: for<'de> Deserialize<'de>,
    {
        let url = format!("{}{}", self.base_url(), endpoint);
        
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
            "NEW" => OrderStatus::New,
            "PARTIALLY_FILLED" => OrderStatus::PartiallyFilled,
            "FILLED" => OrderStatus::Filled,
            "CANCELED" => OrderStatus::Canceled,
            "REJECTED" => OrderStatus::Rejected,
            "EXPIRED" => OrderStatus::Expired,
            _ => OrderStatus::New,
        }
    }

    /// 转换订单类型
    fn convert_order_type_to_binance(order_type: OrderType) -> String {
        match order_type {
            OrderType::Market => "MARKET".to_string(),
            OrderType::Limit => "LIMIT".to_string(),
            OrderType::StopLoss => "STOP_LOSS".to_string(),
            OrderType::TakeProfit => "TAKE_PROFIT".to_string(),
            OrderType::StopLossLimit => "STOP_LOSS_LIMIT".to_string(),
            OrderType::TakeProfitLimit => "TAKE_PROFIT_LIMIT".to_string(),
        }
    }

    /// 转换订单类型从Binance
    fn convert_order_type_from_binance(order_type: &str) -> OrderType {
        match order_type {
            "MARKET" => OrderType::Market,
            "LIMIT" => OrderType::Limit,
            "STOP_LOSS" => OrderType::StopLoss,
            "TAKE_PROFIT" => OrderType::TakeProfit,
            "STOP_LOSS_LIMIT" => OrderType::StopLossLimit,
            "TAKE_PROFIT_LIMIT" => OrderType::TakeProfitLimit,
            _ => OrderType::Limit,
        }
    }

    /// 转换时间有效性
    fn convert_time_in_force(tif: TimeInForce) -> String {
        match tif {
            TimeInForce::GTC => "GTC".to_string(),
            TimeInForce::IOC => "IOC".to_string(),
            TimeInForce::FOK => "FOK".to_string(),
            TimeInForce::GTD => "GTD".to_string(),
            TimeInForce::PostOnly => "GTX".to_string(), // Binance uses GTX for post-only
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
impl ExchangeAdapter for BinanceAdapter {
    fn exchange(&self) -> Exchange {
        Exchange::Binance
    }

    fn config(&self) -> &ExchangeConfig {
        &self.config
    }

    async fn test_connection(&self) -> Result<(), ExchangeError> {
        self.get_server_time().await?;
        Ok(())
    }

    async fn get_server_time(&self) -> Result<DateTime<Utc>, ExchangeError> {
        let response: BinanceServerTime = self.send_public_request("/api/v3/time").await?;
        let timestamp = response.server_time / 1000;
        Ok(DateTime::from_timestamp(timestamp, 0).unwrap_or_else(Utc::now))
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
        let exchange_info: BinanceExchangeInfo = self.send_public_request("/api/v3/exchangeInfo").await?;
        
        let binance_symbol = exchange_info.symbols
            .iter()
            .find(|s| s.symbol == symbol_str)
            .ok_or_else(|| ExchangeError::InvalidSymbol { 
                symbol: symbol_str.clone() 
            })?;

        let mut symbol_info = SymbolInfo {
            symbol: symbol.clone(),
            base_asset: binance_symbol.base_asset.clone(),
            quote_asset: binance_symbol.quote_asset.clone(),
            is_trading: binance_symbol.status == "TRADING",
            min_quantity: Decimal::from_str("0.00000001").unwrap(),
            max_quantity: Decimal::from_str("9000000000").unwrap(),
            quantity_step: Decimal::from_str("0.00000001").unwrap(),
            min_price: Decimal::from_str("0.00000001").unwrap(),
            max_price: Decimal::from_str("1000000").unwrap(),
            price_step: Decimal::from_str("0.00000001").unwrap(),
            min_notional: Decimal::from_str("10").unwrap(),
            quantity_precision: 8,
            price_precision: 8,
            fee_rate: Some(Decimal::from_str("0.001").unwrap()),
        };

        // 解析过滤器
        for filter in &binance_symbol.filters {
            match filter.filter_type.as_str() {
                "LOT_SIZE" => {
                    if let Some(min_qty) = &filter.min_qty {
                        symbol_info.min_quantity = Decimal::from_str(min_qty).unwrap_or(symbol_info.min_quantity);
                    }
                    if let Some(max_qty) = &filter.max_qty {
                        symbol_info.max_quantity = Decimal::from_str(max_qty).unwrap_or(symbol_info.max_quantity);
                    }
                    if let Some(step_size) = &filter.step_size {
                        symbol_info.quantity_step = Decimal::from_str(step_size).unwrap_or(symbol_info.quantity_step);
                    }
                }
                "PRICE_FILTER" => {
                    if let Some(min_price) = &filter.min_price {
                        symbol_info.min_price = Decimal::from_str(min_price).unwrap_or(symbol_info.min_price);
                    }
                    if let Some(max_price) = &filter.max_price {
                        symbol_info.max_price = Decimal::from_str(max_price).unwrap_or(symbol_info.max_price);
                    }
                    if let Some(tick_size) = &filter.tick_size {
                        symbol_info.price_step = Decimal::from_str(tick_size).unwrap_or(symbol_info.price_step);
                    }
                }
                "MIN_NOTIONAL" => {
                    if let Some(min_notional) = &filter.min_notional {
                        symbol_info.min_notional = Decimal::from_str(min_notional).unwrap_or(symbol_info.min_notional);
                    }
                }
                _ => {}
            }
        }

        // 更新缓存
        {
            let mut cache = self.symbol_info_cache.write().await;
            cache.insert(symbol_str, symbol_info.clone());
        }

        Ok(symbol_info)
    }

    async fn get_all_symbols(&self) -> Result<Vec<SymbolInfo>, ExchangeError> {
        let exchange_info: BinanceExchangeInfo = self.send_public_request("/api/v3/exchangeInfo").await?;
        
        let mut symbols = Vec::new();
        for binance_symbol in exchange_info.symbols {
            if binance_symbol.status == "TRADING" {
                if let Ok(symbol) = self.parse_symbol(&binance_symbol.symbol) {
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
        
        let mut params = HashMap::new();
        params.insert("symbol".to_string(), symbol_str);
        params.insert("side".to_string(), match request.side {
            OrderSide::Buy => "BUY".to_string(),
            OrderSide::Sell => "SELL".to_string(),
        });
        params.insert("type".to_string(), Self::convert_order_type_to_binance(request.order_type));
        params.insert("quantity".to_string(), request.quantity.to_string());

        if let Some(price) = request.price {
            params.insert("price".to_string(), price.to_string());
        }

        if let Some(stop_price) = request.stop_price {
            params.insert("stopPrice".to_string(), stop_price.to_string());
        }

        if let Some(tif) = request.time_in_force {
            params.insert("timeInForce".to_string(), Self::convert_time_in_force(tif));
        }

        if let Some(client_order_id) = &request.client_order_id {
            params.insert("newClientOrderId".to_string(), client_order_id.clone());
        }

        let response: BinanceOrderResponse = self.send_authenticated_request(
            Method::POST,
            "/api/v3/order",
            params,
        ).await?;

        Ok(OrderResponse {
            order_id: response.order_id.to_string(),
            client_order_id: Some(response.client_order_id),
            symbol: request.symbol.clone(),
            side: request.side,
            order_type: Self::convert_order_type_from_binance(&response.order_type),
            original_quantity: Decimal::from_str(&response.orig_qty).unwrap_or_default(),
            executed_quantity: Decimal::from_str(&response.executed_qty).unwrap_or_default(),
            remaining_quantity: Decimal::from_str(&response.orig_qty).unwrap_or_default() 
                - Decimal::from_str(&response.executed_qty).unwrap_or_default(),
            price: if response.price != "0.00000000" { 
                Some(Decimal::from_str(&response.price).unwrap_or_default()) 
            } else { 
                None 
            },
            average_price: None, // Binance doesn't provide this in order response
            status: Self::convert_order_status(&response.status),
            time_in_force: request.time_in_force,
            created_at: DateTime::from_timestamp((response.transact_time / 1000) as i64, 0).unwrap_or_else(Utc::now),
            updated_at: DateTime::from_timestamp((response.working_time / 1000) as i64, 0).unwrap_or_else(Utc::now),
            exchange_data: HashMap::new(),
        })
    }

    async fn cancel_order(&self, order_id: &str, symbol: &Symbol) -> Result<OrderResponse, ExchangeError> {
        let symbol_str = self.normalize_symbol(symbol);
        
        let mut params = HashMap::new();
        params.insert("symbol".to_string(), symbol_str);
        params.insert("orderId".to_string(), order_id.to_string());

        let response: BinanceOrderResponse = self.send_authenticated_request(
            Method::DELETE,
            "/api/v3/order",
            params,
        ).await?;

        Ok(OrderResponse {
            order_id: response.order_id.to_string(),
            client_order_id: Some(response.client_order_id),
            symbol: symbol.clone(),
            side: match response.side.as_str() {
                "BUY" => OrderSide::Buy,
                "SELL" => OrderSide::Sell,
                _ => OrderSide::Buy,
            },
            order_type: Self::convert_order_type_from_binance(&response.order_type),
            original_quantity: Decimal::from_str(&response.orig_qty).unwrap_or_default(),
            executed_quantity: Decimal::from_str(&response.executed_qty).unwrap_or_default(),
            remaining_quantity: Decimal::from_str(&response.orig_qty).unwrap_or_default() 
                - Decimal::from_str(&response.executed_qty).unwrap_or_default(),
            price: if response.price != "0.00000000" { 
                Some(Decimal::from_str(&response.price).unwrap_or_default()) 
            } else { 
                None 
            },
            average_price: None,
            status: Self::convert_order_status(&response.status),
            time_in_force: None,
            created_at: DateTime::from_timestamp((response.transact_time / 1000) as i64, 0).unwrap_or_else(Utc::now),
            updated_at: DateTime::from_timestamp((response.working_time / 1000) as i64, 0).unwrap_or_else(Utc::now),
            exchange_data: HashMap::new(),
        })
    }

    async fn cancel_all_orders(&self, symbol: Option<&Symbol>) -> Result<Vec<OrderResponse>, ExchangeError> {
        let mut params = HashMap::new();
        
        if let Some(symbol) = symbol {
            params.insert("symbol".to_string(), self.normalize_symbol(symbol));
        }

        // Binance doesn't have a direct cancel all orders API for all symbols
        // We need to implement this by getting open orders first, then canceling them
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
        
        let mut params = HashMap::new();
        params.insert("symbol".to_string(), symbol_str);
        params.insert("orderId".to_string(), order_id.to_string());

        let response: BinanceOrderResponse = self.send_authenticated_request(
            Method::GET,
            "/api/v3/order",
            params,
        ).await?;

        Ok(OrderResponse {
            order_id: response.order_id.to_string(),
            client_order_id: Some(response.client_order_id),
            symbol: symbol.clone(),
            side: match response.side.as_str() {
                "BUY" => OrderSide::Buy,
                "SELL" => OrderSide::Sell,
                _ => OrderSide::Buy,
            },
            order_type: Self::convert_order_type_from_binance(&response.order_type),
            original_quantity: Decimal::from_str(&response.orig_qty).unwrap_or_default(),
            executed_quantity: Decimal::from_str(&response.executed_qty).unwrap_or_default(),
            remaining_quantity: Decimal::from_str(&response.orig_qty).unwrap_or_default() 
                - Decimal::from_str(&response.executed_qty).unwrap_or_default(),
            price: if response.price != "0.00000000" { 
                Some(Decimal::from_str(&response.price).unwrap_or_default()) 
            } else { 
                None 
            },
            average_price: None,
            status: Self::convert_order_status(&response.status),
            time_in_force: None,
            created_at: DateTime::from_timestamp((response.transact_time / 1000) as i64, 0).unwrap_or_else(Utc::now),
            updated_at: DateTime::from_timestamp((response.working_time / 1000) as i64, 0).unwrap_or_else(Utc::now),
            exchange_data: HashMap::new(),
        })
    }

    async fn get_open_orders(&self, symbol: Option<&Symbol>) -> Result<Vec<OrderResponse>, ExchangeError> {
        let mut params = HashMap::new();
        
        if let Some(symbol) = symbol {
            params.insert("symbol".to_string(), self.normalize_symbol(symbol));
        }

        let response: Vec<BinanceOrderResponse> = self.send_authenticated_request(
            Method::GET,
            "/api/v3/openOrders",
            params,
        ).await?;

        let mut orders = Vec::new();
        for order in response {
            let symbol = self.parse_symbol(&order.symbol)?;
            orders.push(OrderResponse {
                order_id: order.order_id.to_string(),
                client_order_id: Some(order.client_order_id),
                symbol,
                side: match order.side.as_str() {
                    "BUY" => OrderSide::Buy,
                    "SELL" => OrderSide::Sell,
                    _ => OrderSide::Buy,
                },
                order_type: Self::convert_order_type_from_binance(&order.order_type),
                original_quantity: Decimal::from_str(&order.orig_qty).unwrap_or_default(),
                executed_quantity: Decimal::from_str(&order.executed_qty).unwrap_or_default(),
                remaining_quantity: Decimal::from_str(&order.orig_qty).unwrap_or_default() 
                    - Decimal::from_str(&order.executed_qty).unwrap_or_default(),
                price: if order.price != "0.00000000" { 
                    Some(Decimal::from_str(&order.price).unwrap_or_default()) 
                } else { 
                    None 
                },
                average_price: None,
                status: Self::convert_order_status(&order.status),
                time_in_force: None,
                created_at: DateTime::from_timestamp((order.transact_time / 1000) as i64, 0).unwrap_or_else(Utc::now),
                updated_at: DateTime::from_timestamp((order.working_time / 1000) as i64, 0).unwrap_or_else(Utc::now),
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
        let mut params = HashMap::new();
        
        if let Some(symbol) = symbol {
            params.insert("symbol".to_string(), self.normalize_symbol(symbol));
        }
        
        if let Some(start_time) = start_time {
            params.insert("startTime".to_string(), (start_time.timestamp_millis() as u64).to_string());
        }
        
        if let Some(end_time) = end_time {
            params.insert("endTime".to_string(), (end_time.timestamp_millis() as u64).to_string());
        }
        
        if let Some(limit) = limit {
            params.insert("limit".to_string(), limit.to_string());
        }

        let response: Vec<BinanceOrderResponse> = self.send_authenticated_request(
            Method::GET,
            "/api/v3/allOrders",
            params,
        ).await?;

        let mut orders = Vec::new();
        for order in response {
            let symbol = self.parse_symbol(&order.symbol)?;
            orders.push(OrderResponse {
                order_id: order.order_id.to_string(),
                client_order_id: Some(order.client_order_id),
                symbol,
                side: match order.side.as_str() {
                    "BUY" => OrderSide::Buy,
                    "SELL" => OrderSide::Sell,
                    _ => OrderSide::Buy,
                },
                order_type: Self::convert_order_type_from_binance(&order.order_type),
                original_quantity: Decimal::from_str(&order.orig_qty).unwrap_or_default(),
                executed_quantity: Decimal::from_str(&order.executed_qty).unwrap_or_default(),
                remaining_quantity: Decimal::from_str(&order.orig_qty).unwrap_or_default() 
                    - Decimal::from_str(&order.executed_qty).unwrap_or_default(),
                price: if order.price != "0.00000000" { 
                    Some(Decimal::from_str(&order.price).unwrap_or_default()) 
                } else { 
                    None 
                },
                average_price: None,
                status: Self::convert_order_status(&order.status),
                time_in_force: None,
                created_at: DateTime::from_timestamp((order.transact_time / 1000) as i64, 0).unwrap_or_else(Utc::now),
                updated_at: DateTime::from_timestamp((order.working_time / 1000) as i64, 0).unwrap_or_else(Utc::now),
                exchange_data: HashMap::new(),
            });
        }

        Ok(orders)
    }

    async fn get_account_balance(&self) -> Result<Vec<Balance>, ExchangeError> {
        let response: BinanceAccountInfo = self.send_authenticated_request(
            Method::GET,
            "/api/v3/account",
            HashMap::new(),
        ).await?;

        let mut balances = Vec::new();
        for balance in response.balances {
            let free = Decimal::from_str(&balance.free).unwrap_or_default();
            let locked = Decimal::from_str(&balance.locked).unwrap_or_default();
            
            if free > Decimal::ZERO || locked > Decimal::ZERO {
                balances.push(Balance {
                    asset: balance.asset,
                    free,
                    locked,
                    total: free + locked,
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
        // Binance requires symbol for trade history
        let symbol = symbol.ok_or_else(|| ExchangeError::InvalidOrder { 
            reason: "Symbol is required for trade history".to_string() 
        })?;
        
        let mut params = HashMap::new();
        params.insert("symbol".to_string(), self.normalize_symbol(symbol));
        
        if let Some(start_time) = start_time {
            params.insert("startTime".to_string(), (start_time.timestamp_millis() as u64).to_string());
        }
        
        if let Some(end_time) = end_time {
            params.insert("endTime".to_string(), (end_time.timestamp_millis() as u64).to_string());
        }
        
        if let Some(limit) = limit {
            params.insert("limit".to_string(), limit.to_string());
        }

        // Note: This is a simplified implementation
        // Binance trade history API has different structure
        Ok(Vec::new())
    }

    async fn get_ticker_price(&self, symbol: &Symbol) -> Result<Decimal, ExchangeError> {
        let symbol_str = self.normalize_symbol(symbol);
        let endpoint = format!("/api/v3/ticker/price?symbol={}", symbol_str);
        
        let response: BinanceTickerPrice = self.send_public_request(&endpoint).await?;
        
        Decimal::from_str(&response.price)
            .map_err(|e| ExchangeError::SerializationError { 
                message: format!("Failed to parse price: {}", e) 
            })
    }

    async fn get_order_book(&self, symbol: &Symbol, limit: Option<u32>) -> Result<OrderBookSnapshot, ExchangeError> {
        let symbol_str = self.normalize_symbol(symbol);
        let limit_str = limit.unwrap_or(100).to_string();
        let endpoint = format!("/api/v3/depth?symbol={}&limit={}", symbol_str, limit_str);
        
        let response: BinanceOrderBookResponse = self.send_public_request(&endpoint).await?;
        
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
            timestamp: Utc::now(),
            last_update_id: Some(response.last_update_id),
        })
    }

    fn normalize_symbol(&self, symbol: &Symbol) -> String {
        format!("{}{}", symbol.base, symbol.quote)
    }

    fn parse_symbol(&self, symbol_str: &str) -> Result<Symbol, ExchangeError> {
        // Binance symbols don't have separators, so we need to guess
        // This is a simplified implementation - in practice, you'd use the exchange info
        if symbol_str.ends_with("USDT") {
            let base = &symbol_str[..symbol_str.len() - 4];
            Ok(Symbol::new(base, "USDT", Exchange::Binance))
        } else if symbol_str.ends_with("BTC") {
            let base = &symbol_str[..symbol_str.len() - 3];
            Ok(Symbol::new(base, "BTC", Exchange::Binance))
        } else if symbol_str.ends_with("ETH") {
            let base = &symbol_str[..symbol_str.len() - 3];
            Ok(Symbol::new(base, "ETH", Exchange::Binance))
        } else {
            Err(ExchangeError::InvalidSymbol { 
                symbol: symbol_str.to_string() 
            })
        }
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

