use super::exchange_adapter::*;
use crate::common::{Exchange, Result, Symbol};
use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use reqwest::{Client, Method};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashMap;
use std::str::FromStr;
use tokio::time::{sleep, Duration};

type HmacSha256 = Hmac<Sha256>;

/// OKX API响应结构
#[derive(Debug, Deserialize)]
struct OkxResponse<T> {
    code: String,
    msg: String,
    data: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct OkxServerTime {
    ts: String,
}

#[derive(Debug, Deserialize)]
struct OkxInstrument {
    #[serde(rename = "instId")]
    inst_id: String,
    #[serde(rename = "instType")]
    inst_type: String,
    #[serde(rename = "baseCcy")]
    base_ccy: String,
    #[serde(rename = "quoteCcy")]
    quote_ccy: String,
    state: String,
    #[serde(rename = "minSz")]
    min_sz: String,
    #[serde(rename = "maxSz")]
    max_sz: String,
    #[serde(rename = "tickSz")]
    tick_sz: String,
    #[serde(rename = "lotSz")]
    lot_sz: String,
    #[serde(rename = "ctVal")]
    ct_val: Option<String>,
    #[serde(rename = "ctMult")]
    ct_mult: Option<String>,
}

#[derive(Debug, Serialize)]
struct OkxOrderRequest {
    #[serde(rename = "instId")]
    inst_id: String,
    #[serde(rename = "tdMode")]
    td_mode: String,
    side: String,
    #[serde(rename = "ordType")]
    ord_type: String,
    sz: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    px: Option<String>,
    #[serde(rename = "clOrdId", skip_serializing_if = "Option::is_none")]
    cl_ord_id: Option<String>,
    #[serde(rename = "tgtCcy", skip_serializing_if = "Option::is_none")]
    tgt_ccy: Option<String>,
    #[serde(rename = "reduceOnly", skip_serializing_if = "Option::is_none")]
    reduce_only: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct OkxOrderResponse {
    #[serde(rename = "ordId")]
    ord_id: String,
    #[serde(rename = "clOrdId")]
    cl_ord_id: String,
    tag: String,
    #[serde(rename = "sCode")]
    s_code: String,
    #[serde(rename = "sMsg")]
    s_msg: String,
}

#[derive(Debug, Deserialize)]
struct OkxOrderInfo {
    #[serde(rename = "instId")]
    inst_id: String,
    #[serde(rename = "ordId")]
    ord_id: String,
    #[serde(rename = "clOrdId")]
    cl_ord_id: String,
    tag: String,
    px: String,
    sz: String,
    #[serde(rename = "ordType")]
    ord_type: String,
    side: String,
    #[serde(rename = "posSide")]
    pos_side: String,
    #[serde(rename = "tdMode")]
    td_mode: String,
    #[serde(rename = "accFillSz")]
    acc_fill_sz: String,
    #[serde(rename = "fillPx")]
    fill_px: String,
    #[serde(rename = "tradeId")]
    trade_id: String,
    #[serde(rename = "fillSz")]
    fill_sz: String,
    #[serde(rename = "fillTime")]
    fill_time: String,
    state: String,
    #[serde(rename = "avgPx")]
    avg_px: String,
    lever: String,
    #[serde(rename = "tpTriggerPx")]
    tp_trigger_px: String,
    #[serde(rename = "tpOrdPx")]
    tp_ord_px: String,
    #[serde(rename = "slTriggerPx")]
    sl_trigger_px: String,
    #[serde(rename = "slOrdPx")]
    sl_ord_px: String,
    fee: String,
    #[serde(rename = "feeCcy")]
    fee_ccy: String,
    rebate: String,
    #[serde(rename = "rebateCcy")]
    rebate_ccy: String,
    #[serde(rename = "tgtCcy")]
    tgt_ccy: String,
    category: String,
    #[serde(rename = "uTime")]
    u_time: String,
    #[serde(rename = "cTime")]
    c_time: String,
}

#[derive(Debug, Deserialize)]
struct OkxBalance {
    #[serde(rename = "ccy")]
    ccy: String,
    #[serde(rename = "bal")]
    bal: String,
    #[serde(rename = "frozenBal")]
    frozen_bal: String,
    #[serde(rename = "availBal")]
    avail_bal: String,
}

#[derive(Debug, Deserialize)]
struct OkxAccountInfo {
    details: Vec<OkxBalance>,
}

#[derive(Debug, Deserialize)]
struct OkxOrderBook {
    asks: Vec<[String; 4]>,
    bids: Vec<[String; 4]>,
    ts: String,
}

#[derive(Debug, Deserialize)]
struct OkxTicker {
    #[serde(rename = "instId")]
    inst_id: String,
    last: String,
    #[serde(rename = "lastSz")]
    last_sz: String,
    #[serde(rename = "askPx")]
    ask_px: String,
    #[serde(rename = "askSz")]
    ask_sz: String,
    #[serde(rename = "bidPx")]
    bid_px: String,
    #[serde(rename = "bidSz")]
    bid_sz: String,
    open24h: String,
    high24h: String,
    low24h: String,
    #[serde(rename = "volCcy24h")]
    vol_ccy_24h: String,
    #[serde(rename = "vol24h")]
    vol_24h: String,
    ts: String,
    #[serde(rename = "sodUtc0")]
    sod_utc0: String,
    #[serde(rename = "sodUtc8")]
    sod_utc8: String,
}

/// OKX交易所适配器
pub struct OkxAdapter {
    config: ExchangeConfig,
    client: Client,
    symbol_info_cache: tokio::sync::RwLock<HashMap<String, SymbolInfo>>,
}

impl OkxAdapter {
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
    fn generate_signature(&self, timestamp: &str, method: &str, request_path: &str, body: &str) -> String {
        let prehash = format!("{}{}{}{}", timestamp, method, request_path, body);
        
        let mut mac = HmacSha256::new_from_slice(self.config.secret_key.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(prehash.as_bytes());
        
        general_purpose::STANDARD.encode(mac.finalize().into_bytes())
    }

    /// 获取当前时间戳
    fn get_timestamp(&self) -> String {
        Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
    }

    /// 发送认证请求
    async fn send_authenticated_request<T>(
        &self,
        method: Method,
        endpoint: &str,
        body: Option<String>,
    ) -> Result<OkxResponse<T>, ExchangeError>
    where
        T: for<'de> Deserialize<'de>,
    {
        let timestamp = self.get_timestamp();
        let body_str = body.unwrap_or_default();
        let signature = self.generate_signature(&timestamp, method.as_str(), endpoint, &body_str);
        
        let passphrase = self.config.passphrase.as_ref()
            .ok_or_else(|| ExchangeError::AuthenticationError { 
                message: "Passphrase is required for OKX".to_string() 
            })?;

        let url = format!("{}{}", self.base_url(), endpoint);
        
        let mut request = self.client.request(method, &url)
            .header("OK-ACCESS-KEY", &self.config.api_key)
            .header("OK-ACCESS-SIGN", &signature)
            .header("OK-ACCESS-TIMESTAMP", &timestamp)
            .header("OK-ACCESS-PASSPHRASE", passphrase)
            .header("Content-Type", "application/json");

        if !body_str.is_empty() {
            request = request.body(body_str);
        }

        let response = request.send().await
            .map_err(|e| ExchangeError::NetworkError { 
                message: e.to_string() 
            })?;

        let text = response.text().await
            .map_err(|e| ExchangeError::NetworkError { 
                message: e.to_string() 
            })?;

        let okx_response: OkxResponse<T> = serde_json::from_str(&text)
            .map_err(|e| ExchangeError::SerializationError { 
                message: format!("Failed to parse response: {}, text: {}", e, text) 
            })?;

        if okx_response.code != "0" {
            return Err(match okx_response.code.as_str() {
                "50001" => ExchangeError::AuthenticationError { 
                    message: okx_response.msg 
                },
                "50004" => ExchangeError::AuthenticationError { 
                    message: okx_response.msg 
                },
                "50013" => ExchangeError::InvalidOrder { 
                    reason: okx_response.msg 
                },
                "51008" => ExchangeError::OrderNotFound { 
                    order_id: "unknown".to_string() 
                },
                "51024" => ExchangeError::InsufficientBalance { 
                    required: Decimal::ZERO, 
                    available: Decimal::ZERO 
                },
                _ => ExchangeError::ApiError { 
                    code: okx_response.code, 
                    message: okx_response.msg 
                },
            });
        }

        Ok(okx_response)
    }

    /// 发送公共请求
    async fn send_public_request<T>(&self, endpoint: &str) -> Result<OkxResponse<T>, ExchangeError>
    where
        T: for<'de> Deserialize<'de>,
    {
        let url = format!("{}{}", self.base_url(), endpoint);
        
        let response = self.client.get(&url).send().await
            .map_err(|e| ExchangeError::NetworkError { 
                message: e.to_string() 
            })?;

        let text = response.text().await
            .map_err(|e| ExchangeError::NetworkError { 
                message: e.to_string() 
            })?;

        let okx_response: OkxResponse<T> = serde_json::from_str(&text)
            .map_err(|e| ExchangeError::SerializationError { 
                message: format!("Failed to parse response: {}, text: {}", e, text) 
            })?;

        if okx_response.code != "0" {
            return Err(ExchangeError::ApiError { 
                code: okx_response.code, 
                message: okx_response.msg 
            });
        }

        Ok(okx_response)
    }

    /// 转换订单状态
    fn convert_order_status(state: &str) -> OrderStatus {
        match state {
            "live" => OrderStatus::New,
            "partially_filled" => OrderStatus::PartiallyFilled,
            "filled" => OrderStatus::Filled,
            "canceled" => OrderStatus::Canceled,
            "mmp_canceled" => OrderStatus::Canceled,
            _ => OrderStatus::New,
        }
    }

    /// 转换订单类型
    fn convert_order_type_to_okx(order_type: OrderType) -> String {
        match order_type {
            OrderType::Market => "market".to_string(),
            OrderType::Limit => "limit".to_string(),
            OrderType::StopLoss => "conditional".to_string(),
            OrderType::TakeProfit => "conditional".to_string(),
            OrderType::StopLossLimit => "conditional".to_string(),
            OrderType::TakeProfitLimit => "conditional".to_string(),
        }
    }

    /// 转换订单类型从OKX
    fn convert_order_type_from_okx(ord_type: &str) -> OrderType {
        match ord_type {
            "market" => OrderType::Market,
            "limit" => OrderType::Limit,
            "post_only" => OrderType::Limit,
            "fok" => OrderType::Limit,
            "ioc" => OrderType::Limit,
            "conditional" => OrderType::StopLoss,
            _ => OrderType::Limit,
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
impl ExchangeAdapter for OkxAdapter {
    fn exchange(&self) -> Exchange {
        Exchange::OKX
    }

    fn config(&self) -> &ExchangeConfig {
        &self.config
    }

    async fn test_connection(&self) -> Result<(), ExchangeError> {
        self.get_server_time().await?;
        Ok(())
    }

    async fn get_server_time(&self) -> Result<DateTime<Utc>, ExchangeError> {
        let response: OkxResponse<OkxServerTime> = self.send_public_request("/api/v5/public/time").await?;
        
        if let Some(server_time) = response.data.first() {
            let timestamp = server_time.ts.parse::<i64>()
                .map_err(|e| ExchangeError::SerializationError { 
                    message: format!("Failed to parse timestamp: {}", e) 
                })?;
            
            Ok(DateTime::from_timestamp_millis(timestamp).unwrap_or_else(Utc::now))
        } else {
            Err(ExchangeError::ApiError { 
                code: "empty_response".to_string(), 
                message: "Empty server time response".to_string() 
            })
        }
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
        let endpoint = format!("/api/v5/public/instruments?instType=SPOT&instId={}", symbol_str);
        let response: OkxResponse<OkxInstrument> = self.send_public_request(&endpoint).await?;
        
        let instrument = response.data.first()
            .ok_or_else(|| ExchangeError::InvalidSymbol { 
                symbol: symbol_str.clone() 
            })?;

        let symbol_info = SymbolInfo {
            symbol: symbol.clone(),
            base_asset: instrument.base_ccy.clone(),
            quote_asset: instrument.quote_ccy.clone(),
            is_trading: instrument.state == "live",
            min_quantity: Decimal::from_str(&instrument.min_sz).unwrap_or_default(),
            max_quantity: Decimal::from_str(&instrument.max_sz).unwrap_or(Decimal::from_str("9000000000").unwrap()),
            quantity_step: Decimal::from_str(&instrument.lot_sz).unwrap_or_default(),
            min_price: Decimal::from_str("0.00000001").unwrap(),
            max_price: Decimal::from_str("1000000").unwrap(),
            price_step: Decimal::from_str(&instrument.tick_sz).unwrap_or_default(),
            min_notional: Decimal::from_str("1").unwrap(),
            quantity_precision: 8,
            price_precision: 8,
            fee_rate: Some(Decimal::from_str("0.001").unwrap()),
        };

        // 更新缓存
        {
            let mut cache = self.symbol_info_cache.write().await;
            cache.insert(symbol_str, symbol_info.clone());
        }

        Ok(symbol_info)
    }

    async fn get_all_symbols(&self) -> Result<Vec<SymbolInfo>, ExchangeError> {
        let response: OkxResponse<OkxInstrument> = self.send_public_request("/api/v5/public/instruments?instType=SPOT").await?;
        
        let mut symbols = Vec::new();
        for instrument in response.data {
            if instrument.state == "live" {
                if let Ok(symbol) = self.parse_symbol(&instrument.inst_id) {
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
        
        let okx_request = OkxOrderRequest {
            inst_id: symbol_str,
            td_mode: "cash".to_string(), // 现货交易模式
            side: match request.side {
                OrderSide::Buy => "buy".to_string(),
                OrderSide::Sell => "sell".to_string(),
            },
            ord_type: Self::convert_order_type_to_okx(request.order_type),
            sz: request.quantity.to_string(),
            px: request.price.map(|p| p.to_string()),
            cl_ord_id: request.client_order_id.clone(),
            tgt_ccy: Some("base_ccy".to_string()), // 以基础币种计价
            reduce_only: request.reduce_only,
        };

        let body = serde_json::to_string(&[okx_request])
            .map_err(|e| ExchangeError::SerializationError { 
                message: format!("Failed to serialize request: {}", e) 
            })?;

        let response: OkxResponse<OkxOrderResponse> = self.send_authenticated_request(
            Method::POST,
            "/api/v5/trade/order",
            Some(body),
        ).await?;

        let order_response = response.data.first()
            .ok_or_else(|| ExchangeError::ApiError { 
                code: "empty_response".to_string(), 
                message: "Empty order response".to_string() 
            })?;

        if order_response.s_code != "0" {
            return Err(ExchangeError::ApiError { 
                code: order_response.s_code.clone(), 
                message: order_response.s_msg.clone() 
            });
        }

        Ok(OrderResponse {
            order_id: order_response.ord_id.clone(),
            client_order_id: Some(order_response.cl_ord_id.clone()),
            symbol: request.symbol.clone(),
            side: request.side,
            order_type: request.order_type,
            original_quantity: request.quantity,
            executed_quantity: Decimal::ZERO,
            remaining_quantity: request.quantity,
            price: request.price,
            average_price: None,
            status: OrderStatus::New,
            time_in_force: request.time_in_force,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            exchange_data: HashMap::new(),
        })
    }

    async fn cancel_order(&self, order_id: &str, symbol: &Symbol) -> Result<OrderResponse, ExchangeError> {
        let symbol_str = self.normalize_symbol(symbol);
        
        let cancel_request = serde_json::json!([{
            "instId": symbol_str,
            "ordId": order_id
        }]);

        let body = cancel_request.to_string();

        let response: OkxResponse<OkxOrderResponse> = self.send_authenticated_request(
            Method::POST,
            "/api/v5/trade/cancel-order",
            Some(body),
        ).await?;

        let order_response = response.data.first()
            .ok_or_else(|| ExchangeError::ApiError { 
                code: "empty_response".to_string(), 
                message: "Empty cancel response".to_string() 
            })?;

        if order_response.s_code != "0" {
            return Err(ExchangeError::ApiError { 
                code: order_response.s_code.clone(), 
                message: order_response.s_msg.clone() 
            });
        }

        // 获取订单详情
        self.get_order(order_id, symbol).await
    }

    async fn cancel_all_orders(&self, symbol: Option<&Symbol>) -> Result<Vec<OrderResponse>, ExchangeError> {
        // OKX doesn't have a direct cancel all orders API
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
        let endpoint = format!("/api/v5/trade/order?instId={}&ordId={}", symbol_str, order_id);
        
        let response: OkxResponse<OkxOrderInfo> = self.send_authenticated_request(
            Method::GET,
            &endpoint,
            None,
        ).await?;

        let order_info = response.data.first()
            .ok_or_else(|| ExchangeError::OrderNotFound { 
                order_id: order_id.to_string() 
            })?;

        Ok(OrderResponse {
            order_id: order_info.ord_id.clone(),
            client_order_id: Some(order_info.cl_ord_id.clone()),
            symbol: symbol.clone(),
            side: match order_info.side.as_str() {
                "buy" => OrderSide::Buy,
                "sell" => OrderSide::Sell,
                _ => OrderSide::Buy,
            },
            order_type: Self::convert_order_type_from_okx(&order_info.ord_type),
            original_quantity: Decimal::from_str(&order_info.sz).unwrap_or_default(),
            executed_quantity: Decimal::from_str(&order_info.acc_fill_sz).unwrap_or_default(),
            remaining_quantity: Decimal::from_str(&order_info.sz).unwrap_or_default() 
                - Decimal::from_str(&order_info.acc_fill_sz).unwrap_or_default(),
            price: if order_info.px != "" { 
                Some(Decimal::from_str(&order_info.px).unwrap_or_default()) 
            } else { 
                None 
            },
            average_price: if order_info.avg_px != "" { 
                Some(Decimal::from_str(&order_info.avg_px).unwrap_or_default()) 
            } else { 
                None 
            },
            status: Self::convert_order_status(&order_info.state),
            time_in_force: None,
            created_at: DateTime::from_timestamp_millis(
                order_info.c_time.parse::<i64>().unwrap_or_default()
            ).unwrap_or_else(Utc::now),
            updated_at: DateTime::from_timestamp_millis(
                order_info.u_time.parse::<i64>().unwrap_or_default()
            ).unwrap_or_else(Utc::now),
            exchange_data: HashMap::new(),
        })
    }

    async fn get_open_orders(&self, symbol: Option<&Symbol>) -> Result<Vec<OrderResponse>, ExchangeError> {
        let mut endpoint = "/api/v5/trade/orders-pending".to_string();
        
        if let Some(symbol) = symbol {
            endpoint.push_str(&format!("?instId={}", self.normalize_symbol(symbol)));
        }

        let response: OkxResponse<OkxOrderInfo> = self.send_authenticated_request(
            Method::GET,
            &endpoint,
            None,
        ).await?;

        let mut orders = Vec::new();
        for order_info in response.data {
            let symbol = self.parse_symbol(&order_info.inst_id)?;
            orders.push(OrderResponse {
                order_id: order_info.ord_id.clone(),
                client_order_id: Some(order_info.cl_ord_id.clone()),
                symbol,
                side: match order_info.side.as_str() {
                    "buy" => OrderSide::Buy,
                    "sell" => OrderSide::Sell,
                    _ => OrderSide::Buy,
                },
                order_type: Self::convert_order_type_from_okx(&order_info.ord_type),
                original_quantity: Decimal::from_str(&order_info.sz).unwrap_or_default(),
                executed_quantity: Decimal::from_str(&order_info.acc_fill_sz).unwrap_or_default(),
                remaining_quantity: Decimal::from_str(&order_info.sz).unwrap_or_default() 
                    - Decimal::from_str(&order_info.acc_fill_sz).unwrap_or_default(),
                price: if order_info.px != "" { 
                    Some(Decimal::from_str(&order_info.px).unwrap_or_default()) 
                } else { 
                    None 
                },
                average_price: if order_info.avg_px != "" { 
                    Some(Decimal::from_str(&order_info.avg_px).unwrap_or_default()) 
                } else { 
                    None 
                },
                status: Self::convert_order_status(&order_info.state),
                time_in_force: None,
                created_at: DateTime::from_timestamp_millis(
                    order_info.c_time.parse::<i64>().unwrap_or_default()
                ).unwrap_or_else(Utc::now),
                updated_at: DateTime::from_timestamp_millis(
                    order_info.u_time.parse::<i64>().unwrap_or_default()
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
        let mut endpoint = "/api/v5/trade/orders-history".to_string();
        let mut params = Vec::new();
        
        if let Some(symbol) = symbol {
            params.push(format!("instId={}", self.normalize_symbol(symbol)));
        }
        
        if let Some(start_time) = start_time {
            params.push(format!("after={}", start_time.timestamp_millis()));
        }
        
        if let Some(end_time) = end_time {
            params.push(format!("before={}", end_time.timestamp_millis()));
        }
        
        if let Some(limit) = limit {
            params.push(format!("limit={}", limit));
        }

        if !params.is_empty() {
            endpoint.push('?');
            endpoint.push_str(&params.join("&"));
        }

        let response: OkxResponse<OkxOrderInfo> = self.send_authenticated_request(
            Method::GET,
            &endpoint,
            None,
        ).await?;

        let mut orders = Vec::new();
        for order_info in response.data {
            let symbol = self.parse_symbol(&order_info.inst_id)?;
            orders.push(OrderResponse {
                order_id: order_info.ord_id.clone(),
                client_order_id: Some(order_info.cl_ord_id.clone()),
                symbol,
                side: match order_info.side.as_str() {
                    "buy" => OrderSide::Buy,
                    "sell" => OrderSide::Sell,
                    _ => OrderSide::Buy,
                },
                order_type: Self::convert_order_type_from_okx(&order_info.ord_type),
                original_quantity: Decimal::from_str(&order_info.sz).unwrap_or_default(),
                executed_quantity: Decimal::from_str(&order_info.acc_fill_sz).unwrap_or_default(),
                remaining_quantity: Decimal::from_str(&order_info.sz).unwrap_or_default() 
                    - Decimal::from_str(&order_info.acc_fill_sz).unwrap_or_default(),
                price: if order_info.px != "" { 
                    Some(Decimal::from_str(&order_info.px).unwrap_or_default()) 
                } else { 
                    None 
                },
                average_price: if order_info.avg_px != "" { 
                    Some(Decimal::from_str(&order_info.avg_px).unwrap_or_default()) 
                } else { 
                    None 
                },
                status: Self::convert_order_status(&order_info.state),
                time_in_force: None,
                created_at: DateTime::from_timestamp_millis(
                    order_info.c_time.parse::<i64>().unwrap_or_default()
                ).unwrap_or_else(Utc::now),
                updated_at: DateTime::from_timestamp_millis(
                    order_info.u_time.parse::<i64>().unwrap_or_default()
                ).unwrap_or_else(Utc::now),
                exchange_data: HashMap::new(),
            });
        }

        Ok(orders)
    }

    async fn get_account_balance(&self) -> Result<Vec<Balance>, ExchangeError> {
        let response: OkxResponse<OkxAccountInfo> = self.send_authenticated_request(
            Method::GET,
            "/api/v5/account/balance",
            None,
        ).await?;

        let account_info = response.data.first()
            .ok_or_else(|| ExchangeError::ApiError { 
                code: "empty_response".to_string(), 
                message: "Empty balance response".to_string() 
            })?;

        let mut balances = Vec::new();
        for balance in &account_info.details {
            let total = Decimal::from_str(&balance.bal).unwrap_or_default();
            let available = Decimal::from_str(&balance.avail_bal).unwrap_or_default();
            let frozen = Decimal::from_str(&balance.frozen_bal).unwrap_or_default();
            
            if total > Decimal::ZERO {
                balances.push(Balance {
                    asset: balance.ccy.clone(),
                    free: available,
                    locked: frozen,
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
        // OKX trade history implementation
        // This is a simplified version
        Ok(Vec::new())
    }

    async fn get_ticker_price(&self, symbol: &Symbol) -> Result<Decimal, ExchangeError> {
        let symbol_str = self.normalize_symbol(symbol);
        let endpoint = format!("/api/v5/market/ticker?instId={}", symbol_str);
        
        let response: OkxResponse<OkxTicker> = self.send_public_request(&endpoint).await?;
        
        let ticker = response.data.first()
            .ok_or_else(|| ExchangeError::InvalidSymbol { 
                symbol: symbol_str 
            })?;
        
        Decimal::from_str(&ticker.last)
            .map_err(|e| ExchangeError::SerializationError { 
                message: format!("Failed to parse price: {}", e) 
            })
    }

    async fn get_order_book(&self, symbol: &Symbol, limit: Option<u32>) -> Result<OrderBookSnapshot, ExchangeError> {
        let symbol_str = self.normalize_symbol(symbol);
        let limit_str = limit.unwrap_or(20).to_string();
        let endpoint = format!("/api/v5/market/books?instId={}&sz={}", symbol_str, limit_str);
        
        let response: OkxResponse<OkxOrderBook> = self.send_public_request(&endpoint).await?;
        
        let order_book = response.data.first()
            .ok_or_else(|| ExchangeError::InvalidSymbol { 
                symbol: symbol_str 
            })?;
        
        let mut bids = Vec::new();
        for bid in &order_book.bids {
            bids.push(OrderBookLevel {
                price: Decimal::from_str(&bid[0]).unwrap_or_default(),
                quantity: Decimal::from_str(&bid[1]).unwrap_or_default(),
            });
        }
        
        let mut asks = Vec::new();
        for ask in &order_book.asks {
            asks.push(OrderBookLevel {
                price: Decimal::from_str(&ask[0]).unwrap_or_default(),
                quantity: Decimal::from_str(&ask[1]).unwrap_or_default(),
            });
        }
        
        Ok(OrderBookSnapshot {
            symbol: symbol.clone(),
            bids,
            asks,
            timestamp: DateTime::from_timestamp_millis(
                order_book.ts.parse::<i64>().unwrap_or_default()
            ).unwrap_or_else(Utc::now),
            last_update_id: None,
        })
    }

    fn normalize_symbol(&self, symbol: &Symbol) -> String {
        format!("{}-{}", symbol.base, symbol.quote)
    }

    fn parse_symbol(&self, symbol_str: &str) -> Result<Symbol, ExchangeError> {
        let parts: Vec<&str> = symbol_str.split('-').collect();
        if parts.len() != 2 {
            return Err(ExchangeError::InvalidSymbol { 
                symbol: symbol_str.to_string() 
            });
        }
        
        Ok(Symbol::new(parts[0], parts[1], Exchange::OKX))
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

