use crate::common::{
    Exchange, HftError, MarketDataEvent, OrderBook, OrderBookLevel, Result, Side, Symbol, 
    Ticker, Trade, Kline
};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

/// 数据标准化器
pub struct DataNormalizer {
    symbol_mappings: HashMap<Exchange, HashMap<String, Symbol>>,
    price_precision: HashMap<Symbol, u32>,
    quantity_precision: HashMap<Symbol, u32>,
}

/// 标准化的市场数据
#[derive(Debug, Clone)]
pub struct NormalizedMarketData {
    pub event: MarketDataEvent,
    pub source_exchange: Exchange,
    pub raw_data: Option<String>,
    pub normalized_timestamp: DateTime<Utc>,
    pub quality_score: f64, // 数据质量评分 0-1
}

/// 聚合的订单簿
#[derive(Debug, Clone)]
pub struct AggregatedOrderBook {
    pub symbol: Symbol,
    pub bids: Vec<AggregatedLevel>,
    pub asks: Vec<AggregatedLevel>,
    pub timestamp: DateTime<Utc>,
    pub source_count: usize,
}

/// 聚合的订单簿档位
#[derive(Debug, Clone)]
pub struct AggregatedLevel {
    pub price: Decimal,
    pub total_quantity: Decimal,
    pub exchange_quantities: HashMap<Exchange, Decimal>,
    pub weighted_price: Decimal,
}

/// 聚合的价格数据
#[derive(Debug, Clone)]
pub struct AggregatedPrice {
    pub symbol: Symbol,
    pub weighted_price: Decimal,
    pub volume_weighted_price: Decimal,
    pub best_bid: Decimal,
    pub best_ask: Decimal,
    pub spread: Decimal,
    pub timestamp: DateTime<Utc>,
    pub source_exchanges: Vec<Exchange>,
}

/// 数据质量指标
#[derive(Debug, Clone)]
pub struct DataQualityMetrics {
    pub latency_ms: u64,
    pub completeness: f64, // 数据完整性 0-1
    pub consistency: f64,  // 数据一致性 0-1
    pub freshness: f64,    // 数据新鲜度 0-1
    pub accuracy: f64,     // 数据准确性 0-1
}

impl DataNormalizer {
    pub fn new() -> Self {
        let mut normalizer = Self {
            symbol_mappings: HashMap::new(),
            price_precision: HashMap::new(),
            quantity_precision: HashMap::new(),
        };
        
        normalizer.initialize_symbol_mappings();
        normalizer
    }

    /// 初始化交易对映射
    fn initialize_symbol_mappings(&mut self) {
        // Binance 符号映射
        let mut binance_mappings = HashMap::new();
        binance_mappings.insert("BTCUSDT".to_string(), Symbol::new("BTC", "USDT", Exchange::Binance));
        binance_mappings.insert("ETHUSDT".to_string(), Symbol::new("ETH", "USDT", Exchange::Binance));
        binance_mappings.insert("ADAUSDT".to_string(), Symbol::new("ADA", "USDT", Exchange::Binance));
        self.symbol_mappings.insert(Exchange::Binance, binance_mappings);

        // OKX 符号映射
        let mut okx_mappings = HashMap::new();
        okx_mappings.insert("BTC-USDT".to_string(), Symbol::new("BTC", "USDT", Exchange::OKX));
        okx_mappings.insert("ETH-USDT".to_string(), Symbol::new("ETH", "USDT", Exchange::OKX));
        okx_mappings.insert("ADA-USDT".to_string(), Symbol::new("ADA", "USDT", Exchange::OKX));
        self.symbol_mappings.insert(Exchange::OKX, okx_mappings);

        // Gate 符号映射
        let mut gate_mappings = HashMap::new();
        gate_mappings.insert("BTC_USDT".to_string(), Symbol::new("BTC", "USDT", Exchange::Gate));
        gate_mappings.insert("ETH_USDT".to_string(), Symbol::new("ETH", "USDT", Exchange::Gate));
        gate_mappings.insert("ADA_USDT".to_string(), Symbol::new("ADA", "USDT", Exchange::Gate));
        self.symbol_mappings.insert(Exchange::Gate, gate_mappings);
    }

    /// 标准化市场数据
    pub fn normalize_market_data(
        &self,
        event: MarketDataEvent,
        source_exchange: Exchange,
        raw_data: Option<String>,
    ) -> Result<NormalizedMarketData> {
        let normalized_event = match event {
            MarketDataEvent::Ticker(ticker) => {
                MarketDataEvent::Ticker(self.normalize_ticker(ticker, source_exchange)?)
            }
            MarketDataEvent::OrderBook(order_book) => {
                MarketDataEvent::OrderBook(self.normalize_order_book(order_book, source_exchange)?)
            }
            MarketDataEvent::Trade(trade) => {
                MarketDataEvent::Trade(self.normalize_trade(trade, source_exchange)?)
            }
            MarketDataEvent::Kline(kline) => {
                MarketDataEvent::Kline(self.normalize_kline(kline, source_exchange)?)
            }
        };

        let quality_score = self.calculate_quality_score(&normalized_event, source_exchange);

        Ok(NormalizedMarketData {
            event: normalized_event,
            source_exchange,
            raw_data,
            normalized_timestamp: Utc::now(),
            quality_score,
        })
    }

    /// 标准化 Ticker 数据
    fn normalize_ticker(&self, mut ticker: Ticker, exchange: Exchange) -> Result<Ticker> {
        // 标准化符号
        ticker.symbol = self.normalize_symbol(&ticker.symbol, exchange)?;
        
        // 标准化精度
        ticker.bid_price = self.normalize_price(ticker.bid_price, &ticker.symbol)?;
        ticker.ask_price = self.normalize_price(ticker.ask_price, &ticker.symbol)?;
        ticker.last_price = self.normalize_price(ticker.last_price, &ticker.symbol)?;
        ticker.bid_quantity = self.normalize_quantity(ticker.bid_quantity, &ticker.symbol)?;
        ticker.ask_quantity = self.normalize_quantity(ticker.ask_quantity, &ticker.symbol)?;
        ticker.volume_24h = self.normalize_quantity(ticker.volume_24h, &ticker.symbol)?;

        // 数据清洗：检查异常值
        if ticker.bid_price >= ticker.ask_price && ticker.bid_price > Decimal::ZERO && ticker.ask_price > Decimal::ZERO {
            log::warn!("Invalid bid/ask spread for {}: bid={}, ask={}", 
                ticker.symbol, ticker.bid_price, ticker.ask_price);
        }

        Ok(ticker)
    }

    /// 标准化订单簿数据
    fn normalize_order_book(&self, mut order_book: OrderBook, exchange: Exchange) -> Result<OrderBook> {
        // 标准化符号
        order_book.symbol = self.normalize_symbol(&order_book.symbol, exchange)?;

        // 标准化买单
        order_book.bids = order_book.bids
            .into_iter()
            .map(|level| self.normalize_order_book_level(level, &order_book.symbol))
            .collect::<Result<Vec<_>>>()?;

        // 标准化卖单
        order_book.asks = order_book.asks
            .into_iter()
            .map(|level| self.normalize_order_book_level(level, &order_book.symbol))
            .collect::<Result<Vec<_>>>()?;

        // 数据清洗：移除无效档位
        order_book.bids.retain(|level| level.price > Decimal::ZERO && level.quantity > Decimal::ZERO);
        order_book.asks.retain(|level| level.price > Decimal::ZERO && level.quantity > Decimal::ZERO);

        // 排序：买单按价格降序，卖单按价格升序
        order_book.bids.sort_by(|a, b| b.price.cmp(&a.price));
        order_book.asks.sort_by(|a, b| a.price.cmp(&b.price));

        Ok(order_book)
    }

    /// 标准化订单簿档位
    fn normalize_order_book_level(&self, mut level: OrderBookLevel, symbol: &Symbol) -> Result<OrderBookLevel> {
        level.price = self.normalize_price(level.price, symbol)?;
        level.quantity = self.normalize_quantity(level.quantity, symbol)?;
        Ok(level)
    }

    /// 标准化交易数据
    fn normalize_trade(&self, mut trade: Trade, exchange: Exchange) -> Result<Trade> {
        trade.symbol = self.normalize_symbol(&trade.symbol, exchange)?;
        trade.price = self.normalize_price(trade.price, &trade.symbol)?;
        trade.quantity = self.normalize_quantity(trade.quantity, &trade.symbol)?;
        Ok(trade)
    }

    /// 标准化 K线数据
    fn normalize_kline(&self, mut kline: Kline, exchange: Exchange) -> Result<Kline> {
        kline.symbol = self.normalize_symbol(&kline.symbol, exchange)?;
        kline.open_price = self.normalize_price(kline.open_price, &kline.symbol)?;
        kline.high_price = self.normalize_price(kline.high_price, &kline.symbol)?;
        kline.low_price = self.normalize_price(kline.low_price, &kline.symbol)?;
        kline.close_price = self.normalize_price(kline.close_price, &kline.symbol)?;
        kline.volume = self.normalize_quantity(kline.volume, &kline.symbol)?;
        Ok(kline)
    }

    /// 标准化符号
    fn normalize_symbol(&self, symbol: &Symbol, exchange: Exchange) -> Result<Symbol> {
        // 创建标准化的符号，统一使用大写
        Ok(Symbol::new(&symbol.base.to_uppercase(), &symbol.quote.to_uppercase(), exchange))
    }

    /// 标准化价格精度
    fn normalize_price(&self, price: Decimal, symbol: &Symbol) -> Result<Decimal> {
        let precision = self.price_precision.get(symbol).unwrap_or(&8);
        Ok(price.round_dp(*precision))
    }

    /// 标准化数量精度
    fn normalize_quantity(&self, quantity: Decimal, symbol: &Symbol) -> Result<Decimal> {
        let precision = self.quantity_precision.get(symbol).unwrap_or(&8);
        Ok(quantity.round_dp(*precision))
    }

    /// 计算数据质量评分
    fn calculate_quality_score(&self, event: &MarketDataEvent, exchange: Exchange) -> f64 {
        let mut score = 1.0;

        // 基于交易所的基础评分
        let exchange_score = match exchange {
            Exchange::Binance => 0.95,
            Exchange::OKX => 0.90,
            Exchange::Gate => 0.85,
        };

        score *= exchange_score;

        // 基于数据类型的评分
        match event {
            MarketDataEvent::Ticker(ticker) => {
                if ticker.bid_price <= Decimal::ZERO || ticker.ask_price <= Decimal::ZERO {
                    score *= 0.5;
                }
                if ticker.bid_price >= ticker.ask_price {
                    score *= 0.3;
                }
            }
            MarketDataEvent::OrderBook(book) => {
                if book.bids.is_empty() || book.asks.is_empty() {
                    score *= 0.6;
                }
            }
            MarketDataEvent::Trade(trade) => {
                if trade.price <= Decimal::ZERO || trade.quantity <= Decimal::ZERO {
                    score *= 0.4;
                }
            }
            MarketDataEvent::Kline(kline) => {
                if kline.high_price < kline.low_price {
                    score *= 0.2;
                }
            }
        }

        score.max(0.0).min(1.0)
    }

    /// 聚合多交易所订单簿
    pub fn aggregate_order_books(&self, order_books: Vec<OrderBook>) -> Result<AggregatedOrderBook> {
        if order_books.is_empty() {
            return Err(HftError::Other("No order books to aggregate".to_string()));
        }

        let symbol = order_books[0].symbol.clone();
        let mut price_levels: HashMap<Decimal, AggregatedLevel> = HashMap::new();

        // 聚合所有买单
        for book in &order_books {
            for bid in &book.bids {
                let level = price_levels.entry(bid.price).or_insert_with(|| AggregatedLevel {
                    price: bid.price,
                    total_quantity: Decimal::ZERO,
                    exchange_quantities: HashMap::new(),
                    weighted_price: bid.price,
                });
                
                level.total_quantity += bid.quantity;
                level.exchange_quantities.insert(book.symbol.exchange, bid.quantity);
            }
        }

        // 转换为排序的向量
        let mut bids: Vec<AggregatedLevel> = price_levels.into_values().collect();
        bids.sort_by(|a, b| b.price.cmp(&a.price)); // 买单按价格降序

        // 重复相同过程处理卖单
        let mut ask_price_levels: HashMap<Decimal, AggregatedLevel> = HashMap::new();
        
        for book in &order_books {
            for ask in &book.asks {
                let level = ask_price_levels.entry(ask.price).or_insert_with(|| AggregatedLevel {
                    price: ask.price,
                    total_quantity: Decimal::ZERO,
                    exchange_quantities: HashMap::new(),
                    weighted_price: ask.price,
                });
                
                level.total_quantity += ask.quantity;
                level.exchange_quantities.insert(book.symbol.exchange, ask.quantity);
            }
        }

        let mut asks: Vec<AggregatedLevel> = ask_price_levels.into_values().collect();
        asks.sort_by(|a, b| a.price.cmp(&b.price)); // 卖单按价格升序

        Ok(AggregatedOrderBook {
            symbol,
            bids,
            asks,
            timestamp: Utc::now(),
            source_count: order_books.len(),
        })
    }

    /// 计算聚合价格
    pub fn calculate_aggregated_price(&self, tickers: Vec<Ticker>) -> Result<AggregatedPrice> {
        if tickers.is_empty() {
            return Err(HftError::Other("No tickers to aggregate".to_string()));
        }

        let symbol = tickers[0].symbol.clone();
        let mut total_volume = Decimal::ZERO;
        let mut volume_weighted_sum = Decimal::ZERO;
        let mut price_sum = Decimal::ZERO;
        let mut best_bid = Decimal::ZERO;
        let mut best_ask = Decimal::MAX;
        let source_exchanges: Vec<Exchange> = tickers.iter().map(|t| t.symbol.exchange).collect();

        for ticker in &tickers {
            total_volume += ticker.volume_24h;
            volume_weighted_sum += ticker.last_price * ticker.volume_24h;
            price_sum += ticker.last_price;
            
            if ticker.bid_price > best_bid {
                best_bid = ticker.bid_price;
            }
            
            if ticker.ask_price < best_ask {
                best_ask = ticker.ask_price;
            }
        }

        let weighted_price = price_sum / Decimal::from(tickers.len());
        let volume_weighted_price = if total_volume > Decimal::ZERO {
            volume_weighted_sum / total_volume
        } else {
            weighted_price
        };

        let spread = best_ask - best_bid;

        Ok(AggregatedPrice {
            symbol,
            weighted_price,
            volume_weighted_price,
            best_bid,
            best_ask,
            spread,
            timestamp: Utc::now(),
            source_exchanges,
        })
    }
}

impl Default for DataNormalizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_data_normalizer_creation() {
        let normalizer = DataNormalizer::new();
        assert!(!normalizer.symbol_mappings.is_empty());
    }

    #[test]
    fn test_symbol_normalization() {
        let normalizer = DataNormalizer::new();
        let symbol = Symbol::new("btc", "usdt", Exchange::Binance);
        let normalized = normalizer.normalize_symbol(&symbol, Exchange::Binance).unwrap();
        
        assert_eq!(normalized.base, "BTC");
        assert_eq!(normalized.quote, "USDT");
    }

    #[test]
    fn test_ticker_normalization() {
        let normalizer = DataNormalizer::new();
        let ticker = Ticker {
            symbol: Symbol::new("btc", "usdt", Exchange::Binance),
            bid_price: Decimal::from_str("50000.123456789").unwrap(),
            bid_quantity: Decimal::from_str("1.123456789").unwrap(),
            ask_price: Decimal::from_str("50001.123456789").unwrap(),
            ask_quantity: Decimal::from_str("2.123456789").unwrap(),
            last_price: Decimal::from_str("50000.5").unwrap(),
            volume_24h: Decimal::from_str("1000.123456789").unwrap(),
            price_change_24h: Decimal::from_str("100.5").unwrap(),
            timestamp: Utc::now(),
        };

        let normalized = normalizer.normalize_ticker(ticker, Exchange::Binance).unwrap();
        assert_eq!(normalized.symbol.base, "BTC");
        assert_eq!(normalized.symbol.quote, "USDT");
    }
}

