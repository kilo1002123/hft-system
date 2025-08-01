use crate::common::{
    Exchange, ExchangeConfig, HftError, MarketDataEvent, OrderBook, OrderBookLevel, Result,
    Side, Symbol, Ticker, Trade, Kline,
};
use crate::data_ingestion::DataProvider;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::{SinkExt, StreamExt};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::str::FromStr;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;

/// Binance WebSocket 订阅请求
#[derive(Debug, Serialize)]
struct BinanceSubscribeRequest {
    method: String,
    params: Vec<String>,
    id: u64,
}

/// Binance WebSocket 响应
#[derive(Debug, Deserialize)]
struct BinanceResponse {
    stream: Option<String>,
    data: Option<Value>,
    result: Option<Value>,
    id: Option<u64>,
    error: Option<BinanceError>,
}

/// Binance 错误信息
#[derive(Debug, Deserialize)]
struct BinanceError {
    code: i32,
    msg: String,
}

/// Binance 24hr Ticker 数据
#[derive(Debug, Deserialize)]
struct BinanceTicker {
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "c")]
    close_price: String,
    #[serde(rename = "o")]
    open_price: String,
    #[serde(rename = "h")]
    high_price: String,
    #[serde(rename = "l")]
    low_price: String,
    #[serde(rename = "v")]
    volume: String,
    #[serde(rename = "q")]
    quote_volume: String,
    #[serde(rename = "P")]
    price_change_percent: String,
    #[serde(rename = "p")]
    price_change: String,
    #[serde(rename = "w")]
    weighted_avg_price: String,
    #[serde(rename = "x")]
    prev_close_price: String,
    #[serde(rename = "Q")]
    last_qty: String,
    #[serde(rename = "b")]
    bid_price: String,
    #[serde(rename = "B")]
    bid_qty: String,
    #[serde(rename = "a")]
    ask_price: String,
    #[serde(rename = "A")]
    ask_qty: String,
    #[serde(rename = "O")]
    open_time: u64,
    #[serde(rename = "C")]
    close_time: u64,
    #[serde(rename = "F")]
    first_id: u64,
    #[serde(rename = "L")]
    last_id: u64,
    #[serde(rename = "n")]
    count: u64,
}

/// Binance 订单簿数据
#[derive(Debug, Deserialize)]
struct BinanceOrderBook {
    #[serde(rename = "lastUpdateId")]
    last_update_id: u64,
    bids: Vec<Vec<String>>,
    asks: Vec<Vec<String>>,
}

/// Binance 深度更新数据
#[derive(Debug, Deserialize)]
struct BinanceDepthUpdate {
    #[serde(rename = "e")]
    event_type: String,
    #[serde(rename = "E")]
    event_time: u64,
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "U")]
    first_update_id: u64,
    #[serde(rename = "u")]
    final_update_id: u64,
    #[serde(rename = "b")]
    bids: Vec<Vec<String>>,
    #[serde(rename = "a")]
    asks: Vec<Vec<String>>,
}

/// Binance 交易数据
#[derive(Debug, Deserialize)]
struct BinanceTrade {
    #[serde(rename = "e")]
    event_type: String,
    #[serde(rename = "E")]
    event_time: u64,
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "t")]
    trade_id: u64,
    #[serde(rename = "p")]
    price: String,
    #[serde(rename = "q")]
    quantity: String,
    #[serde(rename = "b")]
    buyer_order_id: u64,
    #[serde(rename = "a")]
    seller_order_id: u64,
    #[serde(rename = "T")]
    trade_time: u64,
    #[serde(rename = "m")]
    is_buyer_maker: bool,
    #[serde(rename = "M")]
    ignore: bool,
}

/// Binance K线数据
#[derive(Debug, Deserialize)]
struct BinanceKline {
    #[serde(rename = "e")]
    event_type: String,
    #[serde(rename = "E")]
    event_time: u64,
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "k")]
    kline: BinanceKlineData,
}

#[derive(Debug, Deserialize)]
struct BinanceKlineData {
    #[serde(rename = "t")]
    open_time: u64,
    #[serde(rename = "T")]
    close_time: u64,
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "i")]
    interval: String,
    #[serde(rename = "f")]
    first_trade_id: u64,
    #[serde(rename = "L")]
    last_trade_id: u64,
    #[serde(rename = "o")]
    open_price: String,
    #[serde(rename = "c")]
    close_price: String,
    #[serde(rename = "h")]
    high_price: String,
    #[serde(rename = "l")]
    low_price: String,
    #[serde(rename = "v")]
    volume: String,
    #[serde(rename = "n")]
    trade_count: u64,
    #[serde(rename = "x")]
    is_closed: bool,
    #[serde(rename = "q")]
    quote_volume: String,
    #[serde(rename = "V")]
    taker_buy_volume: String,
    #[serde(rename = "Q")]
    taker_buy_quote_volume: String,
}

/// Binance 数据提供者
pub struct BinanceProvider {
    config: ExchangeConfig,
    sender: Option<mpsc::UnboundedSender<MarketDataEvent>>,
    receiver: Option<mpsc::UnboundedReceiver<MarketDataEvent>>,
    is_connected: bool,
    subscribed_symbols: Vec<Symbol>,
    request_id: u64,
}

impl BinanceProvider {
    pub fn new(config: ExchangeConfig) -> Result<Self> {
        let (sender, receiver) = mpsc::unbounded_channel();
        Ok(Self {
            config,
            sender: Some(sender),
            receiver: Some(receiver),
            is_connected: false,
            subscribed_symbols: Vec::new(),
            request_id: 1,
        })
    }

    fn get_websocket_url(&self) -> String {
        if self.config.sandbox {
            "wss://testnet.binance.vision/ws".to_string()
        } else {
            "wss://stream.binance.com:9443/ws".to_string()
        }
    }

    fn symbol_to_binance_format(&self, symbol: &Symbol) -> String {
        format!("{}{}", symbol.base.to_lowercase(), symbol.quote.to_lowercase())
    }

    fn parse_binance_symbol(&self, symbol: &str) -> Result<Symbol> {
        // Binance 符号格式比较复杂，这里简化处理
        // 实际应用中需要维护一个符号映射表
        let symbol_upper = symbol.to_uppercase();
        
        // 常见的 USDT 交易对
        if symbol_upper.ends_with("USDT") {
            let base = &symbol_upper[..symbol_upper.len() - 4];
            return Ok(Symbol::new(base, "USDT", Exchange::Binance));
        }
        
        // 常见的 BTC 交易对
        if symbol_upper.ends_with("BTC") {
            let base = &symbol_upper[..symbol_upper.len() - 3];
            return Ok(Symbol::new(base, "BTC", Exchange::Binance));
        }
        
        // 常见的 ETH 交易对
        if symbol_upper.ends_with("ETH") {
            let base = &symbol_upper[..symbol_upper.len() - 3];
            return Ok(Symbol::new(base, "ETH", Exchange::Binance));
        }
        
        // 常见的 BNB 交易对
        if symbol_upper.ends_with("BNB") {
            let base = &symbol_upper[..symbol_upper.len() - 3];
            return Ok(Symbol::new(base, "BNB", Exchange::Binance));
        }

        Err(HftError::Other(format!(
            "Cannot parse Binance symbol: {}",
            symbol
        )))
    }

    fn parse_ticker(&self, data: &Value) -> Result<Ticker> {
        let ticker: BinanceTicker = serde_json::from_value(data.clone())?;
        let symbol = self.parse_binance_symbol(&ticker.symbol)?;

        Ok(Ticker {
            symbol,
            bid_price: Decimal::from_str(&ticker.bid_price)?,
            bid_quantity: Decimal::from_str(&ticker.bid_qty)?,
            ask_price: Decimal::from_str(&ticker.ask_price)?,
            ask_quantity: Decimal::from_str(&ticker.ask_qty)?,
            last_price: Decimal::from_str(&ticker.close_price)?,
            volume_24h: Decimal::from_str(&ticker.volume)?,
            price_change_24h: Decimal::from_str(&ticker.price_change)?,
            timestamp: DateTime::from_timestamp_millis(ticker.close_time as i64)
                .unwrap_or_else(Utc::now),
        })
    }

    fn parse_depth_update(&self, data: &Value) -> Result<OrderBook> {
        let depth: BinanceDepthUpdate = serde_json::from_value(data.clone())?;
        let symbol = self.parse_binance_symbol(&depth.symbol)?;

        let bids = depth
            .bids
            .iter()
            .map(|level| {
                Ok(OrderBookLevel {
                    price: Decimal::from_str(&level[0])?,
                    quantity: Decimal::from_str(&level[1])?,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let asks = depth
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
            timestamp: DateTime::from_timestamp_millis(depth.event_time as i64)
                .unwrap_or_else(Utc::now),
        })
    }

    fn parse_trade(&self, data: &Value) -> Result<Trade> {
        let trade: BinanceTrade = serde_json::from_value(data.clone())?;
        let symbol = self.parse_binance_symbol(&trade.symbol)?;

        // Binance 中 is_buyer_maker 为 true 表示买方是挂单方，即这是一个卖单
        let side = if trade.is_buyer_maker {
            Side::Sell
        } else {
            Side::Buy
        };

        Ok(Trade {
            symbol,
            price: Decimal::from_str(&trade.price)?,
            quantity: Decimal::from_str(&trade.quantity)?,
            side,
            timestamp: DateTime::from_timestamp_millis(trade.trade_time as i64)
                .unwrap_or_else(Utc::now),
            trade_id: trade.trade_id.to_string(),
        })
    }

    fn parse_kline(&self, data: &Value) -> Result<Kline> {
        let kline_event: BinanceKline = serde_json::from_value(data.clone())?;
        let symbol = self.parse_binance_symbol(&kline_event.kline.symbol)?;
        let k = &kline_event.kline;

        Ok(Kline {
            symbol,
            open_time: DateTime::from_timestamp_millis(k.open_time as i64)
                .unwrap_or_else(Utc::now),
            close_time: DateTime::from_timestamp_millis(k.close_time as i64)
                .unwrap_or_else(Utc::now),
            open_price: Decimal::from_str(&k.open_price)?,
            high_price: Decimal::from_str(&k.high_price)?,
            low_price: Decimal::from_str(&k.low_price)?,
            close_price: Decimal::from_str(&k.close_price)?,
            volume: Decimal::from_str(&k.volume)?,
            trade_count: k.trade_count,
        })
    }

    async fn handle_message(&self, message: Message) -> Result<()> {
        if let Message::Text(text) = message {
            log::debug!("Binance received: {}", text);

            let response: BinanceResponse = serde_json::from_str(&text)?;

            if let Some(error) = &response.error {
                log::error!("Binance error: {} - {}", error.code, error.msg);
                return Ok(());
            }

            if let Some(result) = &response.result {
                log::info!("Binance subscription result: {:?}", result);
                return Ok(());
            }

            if let (Some(stream), Some(data)) = (&response.stream, &response.data) {
                let event = if stream.contains("@ticker") {
                    let ticker = self.parse_ticker(data)?;
                    MarketDataEvent::Ticker(ticker)
                } else if stream.contains("@depth") {
                    let order_book = self.parse_depth_update(data)?;
                    MarketDataEvent::OrderBook(order_book)
                } else if stream.contains("@trade") {
                    let trade = self.parse_trade(data)?;
                    MarketDataEvent::Trade(trade)
                } else if stream.contains("@kline") {
                    let kline = self.parse_kline(data)?;
                    MarketDataEvent::Kline(kline)
                } else {
                    log::warn!("Unknown Binance stream: {}", stream);
                    return Ok(());
                };

                if let Some(sender) = &self.sender {
                    if let Err(e) = sender.send(event) {
                        log::error!("Failed to send Binance market data: {}", e);
                    }
                }
            }
        }
        Ok(())
    }

    fn next_request_id(&mut self) -> u64 {
        let id = self.request_id;
        self.request_id += 1;
        id
    }
}

#[async_trait]
impl DataProvider for BinanceProvider {
    fn exchange(&self) -> Exchange {
        Exchange::Binance
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
                        log::debug!("Binance message received: {:?}", msg);
                    }
                    Err(e) => {
                        log::error!("Binance WebSocket error: {}", e);
                        break;
                    }
                }
            }
        });

        self.is_connected = true;
        log::info!("Binance WebSocket connected");
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        self.is_connected = false;
        log::info!("Binance WebSocket disconnected");
        Ok(())
    }

    async fn subscribe(&mut self, symbols: Vec<Symbol>) -> Result<()> {
        if !self.is_connected {
            return Err(HftError::WebSocket("Not connected".to_string()));
        }

        let mut streams = Vec::new();

        for symbol in &symbols {
            let symbol_lower = self.symbol_to_binance_format(symbol);

            // 订阅 24hr ticker
            streams.push(format!("{}@ticker", symbol_lower));

            // 订阅深度更新
            streams.push(format!("{}@depth@100ms", symbol_lower));

            // 订阅交易数据
            streams.push(format!("{}@trade", symbol_lower));

            // 订阅 1分钟 K线
            streams.push(format!("{}@kline_1m", symbol_lower));
        }

        let request = BinanceSubscribeRequest {
            method: "SUBSCRIBE".to_string(),
            params: streams,
            id: self.next_request_id(),
        };

        let message = serde_json::to_string(&request)?;
        log::info!("Binance subscribing: {}", message);

        self.subscribed_symbols.extend(symbols);
        Ok(())
    }

    async fn unsubscribe(&mut self, symbols: Vec<Symbol>) -> Result<()> {
        if !self.is_connected {
            return Err(HftError::WebSocket("Not connected".to_string()));
        }

        let mut streams = Vec::new();

        for symbol in &symbols {
            let symbol_lower = self.symbol_to_binance_format(symbol);

            streams.push(format!("{}@ticker", symbol_lower));
            streams.push(format!("{}@depth@100ms", symbol_lower));
            streams.push(format!("{}@trade", symbol_lower));
            streams.push(format!("{}@kline_1m", symbol_lower));
        }

        let request = BinanceSubscribeRequest {
            method: "UNSUBSCRIBE".to_string(),
            params: streams,
            id: self.next_request_id(),
        };

        let message = serde_json::to_string(&request)?;
        log::info!("Binance unsubscribing: {}", message);

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
    fn test_binance_provider_creation() {
        let config = ExchangeConfig {
            api_key: "test_key".to_string(),
            secret_key: "test_secret".to_string(),
            passphrase: None,
            sandbox: true,
            rate_limit: 1000,
        };

        let provider = BinanceProvider::new(config);
        assert!(provider.is_ok());

        let provider = provider.unwrap();
        assert_eq!(provider.exchange(), Exchange::Binance);
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

        let provider = BinanceProvider::new(config).unwrap();
        let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);

        assert_eq!(provider.symbol_to_binance_format(&symbol), "btcusdt");

        let parsed = provider.parse_binance_symbol("BTCUSDT").unwrap();
        assert_eq!(parsed.base, "BTC");
        assert_eq!(parsed.quote, "USDT");
        assert_eq!(parsed.exchange, Exchange::Binance);
    }

    #[test]
    fn test_request_id_generation() {
        let config = ExchangeConfig {
            api_key: "test_key".to_string(),
            secret_key: "test_secret".to_string(),
            passphrase: None,
            sandbox: true,
            rate_limit: 1000,
        };

        let mut provider = BinanceProvider::new(config).unwrap();
        
        assert_eq!(provider.next_request_id(), 1);
        assert_eq!(provider.next_request_id(), 2);
        assert_eq!(provider.next_request_id(), 3);
    }

    #[tokio::test]
    async fn test_binance_provider_lifecycle() {
        let config = ExchangeConfig {
            api_key: "test_key".to_string(),
            secret_key: "test_secret".to_string(),
            passphrase: None,
            sandbox: true,
            rate_limit: 1000,
        };

        let mut provider = BinanceProvider::new(config).unwrap();
        assert!(!provider.is_connected());

        // 注意：实际连接测试需要真实的网络环境
        // 这里只测试状态变化
        let result = provider.disconnect().await;
        assert!(result.is_ok());
        assert!(!provider.is_connected());
    }
}

