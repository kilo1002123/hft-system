use crate::common::{
    Exchange, ExchangeConfig, HftError, MarketDataEvent, OrderBook, OrderBookLevel, Result,
    Side, Symbol, Ticker, Trade,
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

/// OKX WebSocket 订阅请求
#[derive(Debug, Serialize)]
struct OkxSubscribeRequest {
    op: String,
    args: Vec<OkxChannel>,
}

/// OKX 频道
#[derive(Debug, Serialize)]
struct OkxChannel {
    channel: String,
    #[serde(rename = "instId")]
    inst_id: String,
}

/// OKX WebSocket 响应
#[derive(Debug, Deserialize)]
struct OkxResponse {
    event: Option<String>,
    arg: Option<OkxChannel>,
    data: Option<Vec<Value>>,
    code: Option<String>,
    msg: Option<String>,
}

/// OKX Ticker 数据
#[derive(Debug, Deserialize)]
struct OkxTicker {
    #[serde(rename = "instId")]
    inst_id: String,
    #[serde(rename = "last")]
    last_price: String,
    #[serde(rename = "bidPx")]
    bid_price: String,
    #[serde(rename = "bidSz")]
    bid_size: String,
    #[serde(rename = "askPx")]
    ask_price: String,
    #[serde(rename = "askSz")]
    ask_size: String,
    #[serde(rename = "vol24h")]
    volume_24h: String,
    #[serde(rename = "volCcy24h")]
    volume_ccy_24h: String,
    #[serde(rename = "ts")]
    timestamp: String,
}

/// OKX 订单簿数据
#[derive(Debug, Deserialize)]
struct OkxOrderBook {
    #[serde(rename = "instId")]
    inst_id: String,
    asks: Vec<Vec<String>>,
    bids: Vec<Vec<String>>,
    ts: String,
}

/// OKX 交易数据
#[derive(Debug, Deserialize)]
struct OkxTrade {
    #[serde(rename = "instId")]
    inst_id: String,
    #[serde(rename = "tradeId")]
    trade_id: String,
    px: String,
    sz: String,
    side: String,
    ts: String,
}

/// OKX 数据提供者
pub struct OkxProvider {
    config: ExchangeConfig,
    sender: Option<mpsc::UnboundedSender<MarketDataEvent>>,
    receiver: Option<mpsc::UnboundedReceiver<MarketDataEvent>>,
    is_connected: bool,
    subscribed_symbols: Vec<Symbol>,
}

impl OkxProvider {
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
            "wss://wspap.okx.com:8443/ws/v5/public".to_string()
        } else {
            "wss://ws.okx.com:8443/ws/v5/public".to_string()
        }
    }

    fn symbol_to_okx_format(&self, symbol: &Symbol) -> String {
        format!("{}-{}", symbol.base, symbol.quote)
    }

    fn parse_okx_symbol(&self, inst_id: &str) -> Result<Symbol> {
        let parts: Vec<&str> = inst_id.split('-').collect();
        if parts.len() >= 2 {
            Ok(Symbol::new(parts[0], parts[1], Exchange::OKX))
        } else {
            Err(HftError::Other(format!("Invalid OKX symbol format: {}", inst_id)))
        }
    }

    fn parse_ticker(&self, data: &Value) -> Result<Ticker> {
        let ticker: OkxTicker = serde_json::from_value(data.clone())?;
        let symbol = self.parse_okx_symbol(&ticker.inst_id)?;
        
        Ok(Ticker {
            symbol,
            bid_price: Decimal::from_str(&ticker.bid_price)?,
            bid_quantity: Decimal::from_str(&ticker.bid_size)?,
            ask_price: Decimal::from_str(&ticker.ask_price)?,
            ask_quantity: Decimal::from_str(&ticker.ask_size)?,
            last_price: Decimal::from_str(&ticker.last_price)?,
            volume_24h: Decimal::from_str(&ticker.volume_24h)?,
            price_change_24h: Decimal::ZERO, // OKX 不直接提供，需要计算
            timestamp: DateTime::from_timestamp_millis(ticker.timestamp.parse::<i64>()?)
                .unwrap_or_else(Utc::now),
        })
    }

    fn parse_order_book(&self, data: &Value) -> Result<OrderBook> {
        let book: OkxOrderBook = serde_json::from_value(data.clone())?;
        let symbol = self.parse_okx_symbol(&book.inst_id)?;

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
            timestamp: DateTime::from_timestamp_millis(book.ts.parse::<i64>()?)
                .unwrap_or_else(Utc::now),
        })
    }

    fn parse_trade(&self, data: &Value) -> Result<Trade> {
        let trade: OkxTrade = serde_json::from_value(data.clone())?;
        let symbol = self.parse_okx_symbol(&trade.inst_id)?;

        let side = match trade.side.as_str() {
            "buy" => Side::Buy,
            "sell" => Side::Sell,
            _ => return Err(HftError::Other(format!("Unknown side: {}", trade.side))),
        };

        Ok(Trade {
            symbol,
            price: Decimal::from_str(&trade.px)?,
            quantity: Decimal::from_str(&trade.sz)?,
            side,
            timestamp: DateTime::from_timestamp_millis(trade.ts.parse::<i64>()?)
                .unwrap_or_else(Utc::now),
            trade_id: trade.trade_id,
        })
    }

    async fn handle_message(&self, message: Message) -> Result<()> {
        if let Message::Text(text) = message {
            log::debug!("OKX received: {}", text);
            
            let response: OkxResponse = serde_json::from_str(&text)?;
            
            if let Some(event) = &response.event {
                match event.as_str() {
                    "subscribe" => {
                        log::info!("OKX subscription confirmed: {:?}", response.arg);
                    }
                    "error" => {
                        log::error!("OKX error: {:?} - {:?}", response.code, response.msg);
                    }
                    _ => {}
                }
                return Ok(());
            }

            if let (Some(arg), Some(data)) = (&response.arg, &response.data) {
                for item in data {
                    let event = match arg.channel.as_str() {
                        "tickers" => {
                            let ticker = self.parse_ticker(item)?;
                            MarketDataEvent::Ticker(ticker)
                        }
                        "books" => {
                            let order_book = self.parse_order_book(item)?;
                            MarketDataEvent::OrderBook(order_book)
                        }
                        "trades" => {
                            let trade = self.parse_trade(item)?;
                            MarketDataEvent::Trade(trade)
                        }
                        _ => {
                            log::warn!("Unknown OKX channel: {}", arg.channel);
                            continue;
                        }
                    };

                    if let Some(sender) = &self.sender {
                        if let Err(e) = sender.send(event) {
                            log::error!("Failed to send OKX market data: {}", e);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl DataProvider for OkxProvider {
    fn exchange(&self) -> Exchange {
        Exchange::OKX
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
                        // 这里需要重新设计，因为我们无法在这里访问 self
                        // 实际实现中需要使用 Arc<Mutex<Self>> 或其他共享机制
                        log::debug!("OKX message received: {:?}", msg);
                    }
                    Err(e) => {
                        log::error!("OKX WebSocket error: {}", e);
                        break;
                    }
                }
            }
        });

        self.is_connected = true;
        log::info!("OKX WebSocket connected");
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        self.is_connected = false;
        log::info!("OKX WebSocket disconnected");
        Ok(())
    }

    async fn subscribe(&mut self, symbols: Vec<Symbol>) -> Result<()> {
        if !self.is_connected {
            return Err(HftError::WebSocket("Not connected".to_string()));
        }

        let mut channels = Vec::new();
        
        for symbol in &symbols {
            let inst_id = self.symbol_to_okx_format(symbol);
            
            // 订阅 ticker
            channels.push(OkxChannel {
                channel: "tickers".to_string(),
                inst_id: inst_id.clone(),
            });
            
            // 订阅订单簿
            channels.push(OkxChannel {
                channel: "books".to_string(),
                inst_id: inst_id.clone(),
            });
            
            // 订阅交易数据
            channels.push(OkxChannel {
                channel: "trades".to_string(),
                inst_id,
            });
        }

        let request = OkxSubscribeRequest {
            op: "subscribe".to_string(),
            args: channels,
        };

        let message = serde_json::to_string(&request)?;
        log::info!("OKX subscribing: {}", message);

        self.subscribed_symbols.extend(symbols);
        Ok(())
    }

    async fn unsubscribe(&mut self, symbols: Vec<Symbol>) -> Result<()> {
        if !self.is_connected {
            return Err(HftError::WebSocket("Not connected".to_string()));
        }

        let mut channels = Vec::new();
        
        for symbol in &symbols {
            let inst_id = self.symbol_to_okx_format(symbol);
            
            channels.push(OkxChannel {
                channel: "tickers".to_string(),
                inst_id: inst_id.clone(),
            });
            
            channels.push(OkxChannel {
                channel: "books".to_string(),
                inst_id: inst_id.clone(),
            });
            
            channels.push(OkxChannel {
                channel: "trades".to_string(),
                inst_id,
            });
        }

        let request = OkxSubscribeRequest {
            op: "unsubscribe".to_string(),
            args: channels,
        };

        let message = serde_json::to_string(&request)?;
        log::info!("OKX unsubscribing: {}", message);

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
    fn test_okx_provider_creation() {
        let config = ExchangeConfig {
            api_key: "test_key".to_string(),
            secret_key: "test_secret".to_string(),
            passphrase: Some("test_passphrase".to_string()),
            sandbox: true,
            rate_limit: 1000,
        };

        let provider = OkxProvider::new(config);
        assert!(provider.is_ok());
        
        let provider = provider.unwrap();
        assert_eq!(provider.exchange(), Exchange::OKX);
        assert!(!provider.is_connected());
    }

    #[test]
    fn test_symbol_conversion() {
        let config = ExchangeConfig {
            api_key: "test_key".to_string(),
            secret_key: "test_secret".to_string(),
            passphrase: Some("test_passphrase".to_string()),
            sandbox: true,
            rate_limit: 1000,
        };

        let provider = OkxProvider::new(config).unwrap();
        let symbol = Symbol::new("BTC", "USDT", Exchange::OKX);
        
        assert_eq!(provider.symbol_to_okx_format(&symbol), "BTC-USDT");
        
        let parsed = provider.parse_okx_symbol("BTC-USDT").unwrap();
        assert_eq!(parsed.base, "BTC");
        assert_eq!(parsed.quote, "USDT");
        assert_eq!(parsed.exchange, Exchange::OKX);
    }

    #[tokio::test]
    async fn test_okx_provider_lifecycle() {
        let config = ExchangeConfig {
            api_key: "test_key".to_string(),
            secret_key: "test_secret".to_string(),
            passphrase: Some("test_passphrase".to_string()),
            sandbox: true,
            rate_limit: 1000,
        };

        let mut provider = OkxProvider::new(config).unwrap();
        assert!(!provider.is_connected());

        // 注意：实际连接测试需要真实的网络环境
        // 这里只测试状态变化
        let result = provider.disconnect().await;
        assert!(result.is_ok());
        assert!(!provider.is_connected());
    }
}

