use crate::common::{
    Exchange, ExchangeConfig, HftError, MarketDataEvent, OrderBook, OrderBookLevel, Result,
    Side, Symbol, Ticker, Trade,
};
use crate::data_ingestion::DataProvider;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha512;
use std::collections::HashMap;
use std::str::FromStr;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;

/// Gate.io WebSocket 请求
#[derive(Debug, Serialize)]
struct GateRequest {
    time: i64,
    channel: String,
    event: String,
    payload: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auth: Option<GateAuth>,
}

/// Gate.io 认证信息
#[derive(Debug, Serialize)]
struct GateAuth {
    method: String,
    #[serde(rename = "KEY")]
    key: String,
    #[serde(rename = "SIGN")]
    sign: String,
}

/// Gate.io WebSocket 响应
#[derive(Debug, Deserialize)]
struct GateResponse {
    time: Option<i64>,
    time_ms: Option<i64>,
    channel: Option<String>,
    event: Option<String>,
    error: Option<Value>,
    result: Option<Value>,
}

/// Gate.io Ticker 数据
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

/// Gate.io 订单簿数据
#[derive(Debug, Deserialize)]
struct GateOrderBook {
    t: i64,
    lastUpdateId: i64,
    s: String,
    bids: Vec<Vec<String>>,
    asks: Vec<Vec<String>>,
}

/// Gate.io 交易数据
#[derive(Debug, Deserialize)]
struct GateTrade {
    id: u64,
    create_time: i64,
    create_time_ms: String,
    side: String,
    currency_pair: String,
    amount: String,
    price: String,
}

/// Gate.io 数据提供者
pub struct GateProvider {
    config: ExchangeConfig,
    sender: Option<mpsc::UnboundedSender<MarketDataEvent>>,
    receiver: Option<mpsc::UnboundedReceiver<MarketDataEvent>>,
    is_connected: bool,
    subscribed_symbols: Vec<Symbol>,
}

impl GateProvider {
    pub fn new(config: ExchangeConfig) -> Result<Self> {
        let (sender, receiver) = mpsc::unbounded_channel();
        Ok(Self {
            config,
            sender: Some(sender),
            receiver: Some(receiver),
            is_connected: false,
            subscribed_symbols: Vec::new(),
        })
    }

    fn get_websocket_url(&self) -> String {
        if self.config.sandbox {
            "wss://ws-testnet.gate.com/v4/ws/spot".to_string()
        } else {
            "wss://api.gateio.ws/ws/v4/".to_string()
        }
    }

    fn symbol_to_gate_format(&self, symbol: &Symbol) -> String {
        format!("{}_{}", symbol.base, symbol.quote)
    }

    fn parse_gate_symbol(&self, currency_pair: &str) -> Result<Symbol> {
        let parts: Vec<&str> = currency_pair.split('_').collect();
        if parts.len() == 2 {
            Ok(Symbol::new(parts[0], parts[1], Exchange::Gate))
        } else {
            Err(HftError::Other(format!(
                "Invalid Gate symbol format: {}",
                currency_pair
            )))
        }
    }

    fn generate_signature(&self, message: &str) -> String {
        let mut mac = Hmac::<Sha512>::new_from_slice(self.config.secret_key.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(message.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    fn create_auth(&self, channel: &str, event: &str, time: i64) -> GateAuth {
        let message = format!("channel={}&event={}&time={}", channel, event, time);
        let signature = self.generate_signature(&message);
        
        GateAuth {
            method: "api_key".to_string(),
            key: self.config.api_key.clone(),
            sign: signature,
        }
    }

    fn parse_ticker(&self, data: &Value) -> Result<Ticker> {
        let ticker: GateTicker = serde_json::from_value(data.clone())?;
        let symbol = self.parse_gate_symbol(&ticker.currency_pair)?;

        Ok(Ticker {
            symbol,
            bid_price: Decimal::from_str(&ticker.highest_bid)?,
            bid_quantity: Decimal::ZERO, // Gate.io ticker 不包含数量信息
            ask_price: Decimal::from_str(&ticker.lowest_ask)?,
            ask_quantity: Decimal::ZERO,
            last_price: Decimal::from_str(&ticker.last)?,
            volume_24h: Decimal::from_str(&ticker.base_volume)?,
            price_change_24h: Decimal::from_str(&ticker.change_percentage)?,
            timestamp: Utc::now(),
        })
    }

    fn parse_order_book(&self, data: &Value) -> Result<OrderBook> {
        let book: GateOrderBook = serde_json::from_value(data.clone())?;
        let symbol = self.parse_gate_symbol(&book.s)?;

        let bids = book
            .bids
            .iter()
            .map(|level| {
                Ok(OrderBookLevel {
                    price: Decimal::from_str(&level[0])?,
                    quantity: Decimal::from_str(&level[1])?,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let asks = book
            .asks
            .iter()
            .map(|level| {
                Ok(OrderBookLevel {
                    price: Decimal::from_str(&level[0])?,
                    quantity: Decimal::from_str(&level[1])?,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(OrderBook {
            symbol,
            bids,
            asks,
            timestamp: DateTime::from_timestamp_millis(book.t).unwrap_or_else(Utc::now),
        })
    }

    fn parse_trade(&self, data: &Value) -> Result<Trade> {
        let trade: GateTrade = serde_json::from_value(data.clone())?;
        let symbol = self.parse_gate_symbol(&trade.currency_pair)?;

        let side = match trade.side.as_str() {
            "buy" => Side::Buy,
            "sell" => Side::Sell,
            _ => return Err(HftError::Other(format!("Unknown side: {}", trade.side))),
        };

        Ok(Trade {
            symbol,
            price: Decimal::from_str(&trade.price)?,
            quantity: Decimal::from_str(&trade.amount)?,
            side,
            timestamp: DateTime::from_timestamp_millis(
                trade.create_time_ms.parse::<i64>()?,
            )
            .unwrap_or_else(Utc::now),
            trade_id: trade.id.to_string(),
        })
    }

    async fn handle_message(&self, message: Message) -> Result<()> {
        if let Message::Text(text) = message {
            log::debug!("Gate received: {}", text);

            let response: GateResponse = serde_json::from_str(&text)?;

            if let Some(error) = &response.error {
                log::error!("Gate error: {:?}", error);
                return Ok(());
            }

            if let Some(event) = &response.event {
                match event.as_str() {
                    "subscribe" => {
                        log::info!("Gate subscription confirmed: {:?}", response.channel);
                    }
                    "update" => {
                        if let (Some(channel), Some(result)) = (&response.channel, &response.result) {
                            let market_event = match channel.as_str() {
                                "spot.tickers" => {
                                    let ticker = self.parse_ticker(result)?;
                                    MarketDataEvent::Ticker(ticker)
                                }
                                "spot.order_book" => {
                                    let order_book = self.parse_order_book(result)?;
                                    MarketDataEvent::OrderBook(order_book)
                                }
                                "spot.trades" => {
                                    let trade = self.parse_trade(result)?;
                                    MarketDataEvent::Trade(trade)
                                }
                                _ => {
                                    log::warn!("Unknown Gate channel: {}", channel);
                                    return Ok(());
                                }
                            };

                            if let Some(sender) = &self.sender {
                                if let Err(e) = sender.send(market_event) {
                                    log::error!("Failed to send Gate market data: {}", e);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl DataProvider for GateProvider {
    fn exchange(&self) -> Exchange {
        Exchange::Gate
    }

    async fn connect(&mut self) -> Result<()> {
        let url = Url::parse(&self.get_websocket_url())?;
        let (ws_stream, _) = connect_async(url).await?;
        let (mut write, mut read) = ws_stream.split();

        // 启动消息处理任务
        let sender = self.sender.clone();
        tokio::spawn(async move {
            while let Some(message) = read.next().await {
                match message {
                    Ok(msg) => {
                        log::debug!("Gate message received: {:?}", msg);
                    }
                    Err(e) => {
                        log::error!("Gate WebSocket error: {}", e);
                        break;
                    }
                }
            }
        });

        self.is_connected = true;
        log::info!("Gate WebSocket connected");
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        self.is_connected = false;
        log::info!("Gate WebSocket disconnected");
        Ok(())
    }

    async fn subscribe(&mut self, symbols: Vec<Symbol>) -> Result<()> {
        if !self.is_connected {
            return Err(HftError::WebSocket("Not connected".to_string()));
        }

        let current_time = chrono::Utc::now().timestamp();

        for symbol in &symbols {
            let currency_pair = self.symbol_to_gate_format(symbol);

            // 订阅 ticker
            let ticker_request = GateRequest {
                time: current_time,
                channel: "spot.tickers".to_string(),
                event: "subscribe".to_string(),
                payload: Some(vec![currency_pair.clone()]),
                auth: None, // 公共频道不需要认证
            };

            // 订阅订单簿
            let book_request = GateRequest {
                time: current_time,
                channel: "spot.order_book".to_string(),
                event: "subscribe".to_string(),
                payload: Some(vec![currency_pair.clone(), "100ms".to_string()]),
                auth: None,
            };

            // 订阅交易数据
            let trade_request = GateRequest {
                time: current_time,
                channel: "spot.trades".to_string(),
                event: "subscribe".to_string(),
                payload: Some(vec![currency_pair]),
                auth: None,
            };

            let requests = vec![ticker_request, book_request, trade_request];
            for request in requests {
                let message = serde_json::to_string(&request)?;
                log::info!("Gate subscribing: {}", message);
            }
        }

        self.subscribed_symbols.extend(symbols);
        Ok(())
    }

    async fn unsubscribe(&mut self, symbols: Vec<Symbol>) -> Result<()> {
        if !self.is_connected {
            return Err(HftError::WebSocket("Not connected".to_string()));
        }

        let current_time = chrono::Utc::now().timestamp();

        for symbol in &symbols {
            let currency_pair = self.symbol_to_gate_format(symbol);

            let channels = vec!["spot.tickers", "spot.order_book", "spot.trades"];
            for channel in channels {
                let request = GateRequest {
                    time: current_time,
                    channel: channel.to_string(),
                    event: "unsubscribe".to_string(),
                    payload: Some(vec![currency_pair.clone()]),
                    auth: None,
                };

                let message = serde_json::to_string(&request)?;
                log::info!("Gate unsubscribing: {}", message);
            }
        }

        // 从已订阅列表中移除
        self.subscribed_symbols.retain(|s| !symbols.contains(s));
        Ok(())
    }

    fn get_receiver(&self) -> Option<mpsc::UnboundedReceiver<MarketDataEvent>> {
        // 注意：这里返回 None，因为 receiver 已经被移动
        // 实际实现中需要使用不同的设计模式
        None
    }

    fn is_connected(&self) -> bool {
        self.is_connected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::ExchangeConfig;

    #[test]
    fn test_gate_provider_creation() {
        let config = ExchangeConfig {
            api_key: "test_key".to_string(),
            secret_key: "test_secret".to_string(),
            passphrase: None,
            sandbox: true,
            rate_limit: 1000,
        };

        let provider = GateProvider::new(config);
        assert!(provider.is_ok());

        let provider = provider.unwrap();
        assert_eq!(provider.exchange(), Exchange::Gate);
        assert!(!provider.is_connected());
    }

    #[test]
    fn test_symbol_conversion() {
        let config = ExchangeConfig {
            api_key: "test_key".to_string(),
            secret_key: "test_secret".to_string(),
            passphrase: None,
            sandbox: true,
            rate_limit: 1000,
        };

        let provider = GateProvider::new(config).unwrap();
        let symbol = Symbol::new("BTC", "USDT", Exchange::Gate);

        assert_eq!(provider.symbol_to_gate_format(&symbol), "BTC_USDT");

        let parsed = provider.parse_gate_symbol("BTC_USDT").unwrap();
        assert_eq!(parsed.base, "BTC");
        assert_eq!(parsed.quote, "USDT");
        assert_eq!(parsed.exchange, Exchange::Gate);
    }

    #[test]
    fn test_signature_generation() {
        let config = ExchangeConfig {
            api_key: "test_key".to_string(),
            secret_key: "test_secret".to_string(),
            passphrase: None,
            sandbox: true,
            rate_limit: 1000,
        };

        let provider = GateProvider::new(config).unwrap();
        let signature = provider.generate_signature("test_message");
        assert!(!signature.is_empty());
        assert_eq!(signature.len(), 128); // SHA512 hex 长度
    }

    #[tokio::test]
    async fn test_gate_provider_lifecycle() {
        let config = ExchangeConfig {
            api_key: "test_key".to_string(),
            secret_key: "test_secret".to_string(),
            passphrase: None,
            sandbox: true,
            rate_limit: 1000,
        };

        let mut provider = GateProvider::new(config).unwrap();
        assert!(!provider.is_connected());

        // 注意：实际连接测试需要真实的网络环境
        // 这里只测试状态变化
        let result = provider.disconnect().await;
        assert!(result.is_ok());
        assert!(!provider.is_connected());
    }
}

