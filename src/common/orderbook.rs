use crate::common::{OrderBook, OrderBookLevel, Result, Side, Symbol};
use dashmap::DashMap;
use rust_decimal::Decimal;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};

/// 高效的内存订单簿管理器
pub struct OrderBookManager {
    /// 使用 DashMap 实现无锁的并发访问
    order_books: DashMap<Symbol, Arc<RwLock<ManagedOrderBook>>>,
}

/// 管理的订单簿
#[derive(Debug, Clone)]
pub struct ManagedOrderBook {
    pub symbol: Symbol,
    /// 使用 BTreeMap 保持价格排序，买单按价格降序，卖单按价格升序
    pub bids: BTreeMap<OrderedDecimal, Decimal>, // price -> quantity
    pub asks: BTreeMap<OrderedDecimal, Decimal>, // price -> quantity
    pub last_update: DateTime<Utc>,
    pub sequence: u64,
}

/// 包装 Decimal 以实现正确的排序
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct OrderedDecimal(pub Decimal);

impl From<Decimal> for OrderedDecimal {
    fn from(d: Decimal) -> Self {
        OrderedDecimal(d)
    }
}

impl From<OrderedDecimal> for Decimal {
    fn from(od: OrderedDecimal) -> Self {
        od.0
    }
}

/// 订单簿快照
#[derive(Debug, Clone)]
pub struct OrderBookSnapshot {
    pub symbol: Symbol,
    pub bids: Vec<OrderBookLevel>,
    pub asks: Vec<OrderBookLevel>,
    pub timestamp: DateTime<Utc>,
    pub sequence: u64,
}

/// 订单簿统计信息
#[derive(Debug, Clone)]
pub struct OrderBookStats {
    pub symbol: Symbol,
    pub best_bid: Option<Decimal>,
    pub best_ask: Option<Decimal>,
    pub spread: Option<Decimal>,
    pub spread_bps: Option<Decimal>, // 基点
    pub mid_price: Option<Decimal>,
    pub bid_depth: Decimal,
    pub ask_depth: Decimal,
    pub total_bid_volume: Decimal,
    pub total_ask_volume: Decimal,
}

impl OrderBookManager {
    pub fn new() -> Self {
        Self {
            order_books: DashMap::new(),
        }
    }

    /// 更新订单簿
    pub async fn update_order_book(&self, order_book: OrderBook) -> Result<()> {
        let symbol = order_book.symbol.clone();
        
        // 获取或创建订单簿
        let managed_book = self.order_books
            .entry(symbol.clone())
            .or_insert_with(|| Arc::new(RwLock::new(ManagedOrderBook::new(symbol))))
            .clone();

        let mut book = managed_book.write().await;
        book.update_from_snapshot(order_book).await?;
        
        Ok(())
    }

    /// 增量更新订单簿
    pub async fn update_order_book_incremental(
        &self,
        symbol: Symbol,
        bids: Vec<OrderBookLevel>,
        asks: Vec<OrderBookLevel>,
        sequence: u64,
    ) -> Result<()> {
        if let Some(managed_book) = self.order_books.get(&symbol) {
            let mut book = managed_book.write().await;
            book.update_incremental(bids, asks, sequence).await?;
        }
        Ok(())
    }

    /// 获取订单簿快照
    pub async fn get_snapshot(&self, symbol: &Symbol) -> Option<OrderBookSnapshot> {
        if let Some(managed_book) = self.order_books.get(symbol) {
            let book = managed_book.read().await;
            Some(book.get_snapshot())
        } else {
            None
        }
    }

    /// 获取最佳买卖价
    pub async fn get_best_bid_ask(&self, symbol: &Symbol) -> Option<(Option<Decimal>, Option<Decimal>)> {
        if let Some(managed_book) = self.order_books.get(symbol) {
            let book = managed_book.read().await;
            Some(book.get_best_bid_ask())
        } else {
            None
        }
    }

    /// 获取订单簿统计信息
    pub async fn get_stats(&self, symbol: &Symbol) -> Option<OrderBookStats> {
        if let Some(managed_book) = self.order_books.get(symbol) {
            let book = managed_book.read().await;
            Some(book.get_stats())
        } else {
            None
        }
    }

    /// 获取指定深度的订单簿
    pub async fn get_depth(&self, symbol: &Symbol, depth: usize) -> Option<OrderBookSnapshot> {
        if let Some(managed_book) = self.order_books.get(symbol) {
            let book = managed_book.read().await;
            Some(book.get_depth(depth))
        } else {
            None
        }
    }

    /// 计算市价单的预期成交价格和数量
    pub async fn calculate_market_impact(
        &self,
        symbol: &Symbol,
        side: Side,
        quantity: Decimal,
    ) -> Option<(Decimal, Decimal)> { // (average_price, filled_quantity)
        if let Some(managed_book) = self.order_books.get(symbol) {
            let book = managed_book.read().await;
            Some(book.calculate_market_impact(side, quantity))
        } else {
            None
        }
    }

    /// 清理过期的订单簿
    pub async fn cleanup_stale_books(&self, max_age_seconds: u64) {
        let now = Utc::now();
        let mut to_remove = Vec::new();

        for entry in self.order_books.iter() {
            let symbol = entry.key().clone();
            let book = entry.value().read().await;
            
            if (now - book.last_update).num_seconds() as u64 > max_age_seconds {
                to_remove.push(symbol);
            }
        }

        for symbol in to_remove {
            self.order_books.remove(&symbol);
            log::info!("Removed stale order book for {}", symbol);
        }
    }

    /// 获取所有活跃的交易对
    pub fn get_active_symbols(&self) -> Vec<Symbol> {
        self.order_books.iter().map(|entry| entry.key().clone()).collect()
    }
}

impl Default for OrderBookManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ManagedOrderBook {
    pub fn new(symbol: Symbol) -> Self {
        Self {
            symbol,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            last_update: Utc::now(),
            sequence: 0,
        }
    }

    /// 从完整快照更新订单簿
    pub async fn update_from_snapshot(&mut self, order_book: OrderBook) -> Result<()> {
        // 清空现有数据
        self.bids.clear();
        self.asks.clear();

        // 更新买单
        for level in order_book.bids {
            if level.quantity > Decimal::ZERO {
                self.bids.insert(OrderedDecimal(level.price), level.quantity);
            }
        }

        // 更新卖单
        for level in order_book.asks {
            if level.quantity > Decimal::ZERO {
                self.asks.insert(OrderedDecimal(level.price), level.quantity);
            }
        }

        self.last_update = order_book.timestamp;
        self.sequence += 1;

        Ok(())
    }

    /// 增量更新订单簿
    pub async fn update_incremental(
        &mut self,
        bids: Vec<OrderBookLevel>,
        asks: Vec<OrderBookLevel>,
        sequence: u64,
    ) -> Result<()> {
        // 检查序列号，防止乱序更新
        if sequence <= self.sequence {
            log::warn!("Ignoring out-of-order update: {} <= {}", sequence, self.sequence);
            return Ok(());
        }

        // 更新买单
        for level in bids {
            let price = OrderedDecimal(level.price);
            if level.quantity == Decimal::ZERO {
                self.bids.remove(&price);
            } else {
                self.bids.insert(price, level.quantity);
            }
        }

        // 更新卖单
        for level in asks {
            let price = OrderedDecimal(level.price);
            if level.quantity == Decimal::ZERO {
                self.asks.remove(&price);
            } else {
                self.asks.insert(price, level.quantity);
            }
        }

        self.last_update = Utc::now();
        self.sequence = sequence;

        Ok(())
    }

    /// 获取订单簿快照
    pub fn get_snapshot(&self) -> OrderBookSnapshot {
        let bids: Vec<OrderBookLevel> = self.bids
            .iter()
            .rev() // 买单按价格降序
            .map(|(price, quantity)| OrderBookLevel {
                price: price.0,
                quantity: *quantity,
            })
            .collect();

        let asks: Vec<OrderBookLevel> = self.asks
            .iter() // 卖单按价格升序
            .map(|(price, quantity)| OrderBookLevel {
                price: price.0,
                quantity: *quantity,
            })
            .collect();

        OrderBookSnapshot {
            symbol: self.symbol.clone(),
            bids,
            asks,
            timestamp: self.last_update,
            sequence: self.sequence,
        }
    }

    /// 获取指定深度的订单簿
    pub fn get_depth(&self, depth: usize) -> OrderBookSnapshot {
        let bids: Vec<OrderBookLevel> = self.bids
            .iter()
            .rev()
            .take(depth)
            .map(|(price, quantity)| OrderBookLevel {
                price: price.0,
                quantity: *quantity,
            })
            .collect();

        let asks: Vec<OrderBookLevel> = self.asks
            .iter()
            .take(depth)
            .map(|(price, quantity)| OrderBookLevel {
                price: price.0,
                quantity: *quantity,
            })
            .collect();

        OrderBookSnapshot {
            symbol: self.symbol.clone(),
            bids,
            asks,
            timestamp: self.last_update,
            sequence: self.sequence,
        }
    }

    /// 获取最佳买卖价
    pub fn get_best_bid_ask(&self) -> (Option<Decimal>, Option<Decimal>) {
        let best_bid = self.bids.iter().next_back().map(|(price, _)| price.0);
        let best_ask = self.asks.iter().next().map(|(price, _)| price.0);
        (best_bid, best_ask)
    }

    /// 获取统计信息
    pub fn get_stats(&self) -> OrderBookStats {
        let (best_bid, best_ask) = self.get_best_bid_ask();
        
        let spread = match (best_bid, best_ask) {
            (Some(bid), Some(ask)) => Some(ask - bid),
            _ => None,
        };

        let spread_bps = match (spread, best_bid) {
            (Some(s), Some(bid)) if bid > Decimal::ZERO => {
                Some(s / bid * Decimal::new(10000, 0)) // 转换为基点
            }
            _ => None,
        };

        let mid_price = match (best_bid, best_ask) {
            (Some(bid), Some(ask)) => Some((bid + ask) / Decimal::new(2, 0)),
            _ => None,
        };

        let total_bid_volume = self.bids.values().sum();
        let total_ask_volume = self.asks.values().sum();

        // 计算前5档深度
        let bid_depth = self.bids.iter().rev().take(5).map(|(_, qty)| qty).sum();
        let ask_depth = self.asks.iter().take(5).map(|(_, qty)| qty).sum();

        OrderBookStats {
            symbol: self.symbol.clone(),
            best_bid,
            best_ask,
            spread,
            spread_bps,
            mid_price,
            bid_depth,
            ask_depth,
            total_bid_volume,
            total_ask_volume,
        }
    }

    /// 计算市价单的市场冲击
    pub fn calculate_market_impact(&self, side: Side, quantity: Decimal) -> (Decimal, Decimal) {
        let mut remaining_quantity = quantity;
        let mut total_cost = Decimal::ZERO;
        let mut filled_quantity = Decimal::ZERO;

        match side {
            Side::Buy => {
                // 买单消耗卖方订单簿
                for (price, available_quantity) in &self.asks {
                    if remaining_quantity <= Decimal::ZERO {
                        break;
                    }

                    let fill_quantity = remaining_quantity.min(*available_quantity);
                    total_cost += fill_quantity * price.0;
                    filled_quantity += fill_quantity;
                    remaining_quantity -= fill_quantity;
                }
            }
            Side::Sell => {
                // 卖单消耗买方订单簿
                for (price, available_quantity) in self.bids.iter().rev() {
                    if remaining_quantity <= Decimal::ZERO {
                        break;
                    }

                    let fill_quantity = remaining_quantity.min(*available_quantity);
                    total_cost += fill_quantity * price.0;
                    filled_quantity += fill_quantity;
                    remaining_quantity -= fill_quantity;
                }
            }
        }

        let average_price = if filled_quantity > Decimal::ZERO {
            total_cost / filled_quantity
        } else {
            Decimal::ZERO
        };

        (average_price, filled_quantity)
    }
}

