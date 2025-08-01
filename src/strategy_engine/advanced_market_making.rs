use super::market_making::{
    MarketMakingConfig, MarketMakingStrategy, MarketSnapshot, Quote, StrategyEvent,
    StrategyMetrics, StrategyState,
};
use crate::common::{
    Exchange, HftError, MarketDataEvent, OrderBook, Result, Side, Symbol, Ticker, Trade,
};
use crate::oms::{Order, OrderId, OrderStatus, OrderType};
use crate::rms::{Position, RiskManager};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use uuid::Uuid;

/// 高级做市策略配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvancedMarketMakingConfig {
    /// 基础配置
    pub base_config: MarketMakingConfig,
    /// 动态价差调整
    pub dynamic_spread_adjustment: bool,
    /// 订单簿不平衡调整
    pub order_book_imbalance_adjustment: bool,
    /// 成交量加权调整
    pub volume_weighted_adjustment: bool,
    /// 时间衰减因子
    pub time_decay_factor: Decimal,
    /// 最大订单层数
    pub max_order_levels: u32,
    /// 层间价差倍数
    pub level_spread_multiplier: Decimal,
    /// 最小盈利阈值
    pub min_profit_threshold: Decimal,
    /// 快速取消阈值 (毫秒)
    pub fast_cancel_threshold_ms: u64,
    /// 市场冲击保护
    pub market_impact_protection: bool,
    /// 自适应学习开关
    pub adaptive_learning: bool,
}

impl Default for AdvancedMarketMakingConfig {
    fn default() -> Self {
        Self {
            base_config: MarketMakingConfig::default(),
            dynamic_spread_adjustment: true,
            order_book_imbalance_adjustment: true,
            volume_weighted_adjustment: true,
            time_decay_factor: Decimal::new(95, 2), // 0.95
            max_order_levels: 3,
            level_spread_multiplier: Decimal::new(15, 1), // 1.5
            min_profit_threshold: Decimal::new(1, 4), // 0.0001
            fast_cancel_threshold_ms: 500,
            market_impact_protection: true,
            adaptive_learning: true,
        }
    }
}

/// 订单簿不平衡指标
#[derive(Debug, Clone)]
pub struct OrderBookImbalance {
    /// 买卖量比率
    pub bid_ask_ratio: Decimal,
    /// 加权中间价
    pub weighted_mid_price: Decimal,
    /// 不平衡强度
    pub imbalance_strength: Decimal,
    /// 计算时间
    pub timestamp: DateTime<Utc>,
}

/// 市场微观结构指标
#[derive(Debug, Clone)]
pub struct MarketMicrostructure {
    /// 有效价差
    pub effective_spread: Decimal,
    /// 价格冲击
    pub price_impact: Decimal,
    /// 订单流不平衡
    pub order_flow_imbalance: Decimal,
    /// 成交量加权平均价格
    pub vwap: Decimal,
    /// 时间加权平均价格
    pub twap: Decimal,
    /// 更新时间
    pub timestamp: DateTime<Utc>,
}

/// 自适应参数
#[derive(Debug, Clone)]
pub struct AdaptiveParameters {
    /// 学习率
    pub learning_rate: Decimal,
    /// 价差调整因子
    pub spread_adjustment_factor: Decimal,
    /// 库存调整因子
    pub inventory_adjustment_factor: Decimal,
    /// 波动率调整因子
    pub volatility_adjustment_factor: Decimal,
    /// 更新次数
    pub update_count: u64,
    /// 最后更新时间
    pub last_update: DateTime<Utc>,
}

/// 高级做市策略
pub struct AdvancedMarketMaker {
    /// 策略配置
    config: AdvancedMarketMakingConfig,
    /// 策略状态
    state: Arc<RwLock<StrategyState>>,
    /// 当前市场快照
    market_snapshot: Arc<RwLock<Option<MarketSnapshot>>>,
    /// 当前报价
    current_quotes: Arc<RwLock<Vec<Quote>>>,
    /// 活跃订单
    active_orders: Arc<RwLock<HashMap<OrderId, Order>>>,
    /// 当前持仓
    position: Arc<RwLock<Position>>,
    /// 策略指标
    metrics: Arc<RwLock<StrategyMetrics>>,
    /// 风险管理器
    risk_manager: Arc<dyn RiskManager>,
    /// 事件发送器
    event_sender: mpsc::UnboundedSender<StrategyEvent>,
    /// 订单发送器
    order_sender: mpsc::UnboundedSender<Order>,
    /// 价格历史
    price_history: Arc<Mutex<VecDeque<(DateTime<Utc>, Decimal)>>>,
    /// 成交历史
    trade_history: Arc<Mutex<VecDeque<Trade>>>,
    /// 订单簿历史
    order_book_history: Arc<Mutex<VecDeque<OrderBook>>>,
    /// 订单簿不平衡
    order_book_imbalance: Arc<RwLock<Option<OrderBookImbalance>>>,
    /// 市场微观结构
    market_microstructure: Arc<RwLock<Option<MarketMicrostructure>>>,
    /// 自适应参数
    adaptive_params: Arc<RwLock<AdaptiveParameters>>,
    /// 最大历史长度
    max_history_length: usize,
}

impl AdvancedMarketMaker {
    pub fn new(
        config: AdvancedMarketMakingConfig,
        risk_manager: Arc<dyn RiskManager>,
        event_sender: mpsc::UnboundedSender<StrategyEvent>,
        order_sender: mpsc::UnboundedSender<Order>,
    ) -> Self {
        let initial_position = Position {
            symbol: config.base_config.symbol.clone(),
            quantity: Decimal::ZERO,
            average_price: Decimal::ZERO,
            unrealized_pnl: Decimal::ZERO,
            realized_pnl: Decimal::ZERO,
            last_update: Utc::now(),
        };

        let adaptive_params = AdaptiveParameters {
            learning_rate: Decimal::new(1, 3), // 0.001
            spread_adjustment_factor: Decimal::ONE,
            inventory_adjustment_factor: Decimal::ONE,
            volatility_adjustment_factor: Decimal::ONE,
            update_count: 0,
            last_update: Utc::now(),
        };

        Self {
            config,
            state: Arc::new(RwLock::new(StrategyState::Initializing)),
            market_snapshot: Arc::new(RwLock::new(None)),
            current_quotes: Arc::new(RwLock::new(Vec::new())),
            active_orders: Arc::new(RwLock::new(HashMap::new())),
            position: Arc::new(RwLock::new(initial_position)),
            metrics: Arc::new(RwLock::new(StrategyMetrics::default())),
            risk_manager,
            event_sender,
            order_sender,
            price_history: Arc::new(Mutex::new(VecDeque::new())),
            trade_history: Arc::new(Mutex::new(VecDeque::new())),
            order_book_history: Arc::new(Mutex::new(VecDeque::new())),
            order_book_imbalance: Arc::new(RwLock::new(None)),
            market_microstructure: Arc::new(RwLock::new(None)),
            adaptive_params: Arc::new(RwLock::new(adaptive_params)),
            max_history_length: 1000,
        }
    }

    /// 计算订单簿不平衡
    async fn calculate_order_book_imbalance(&self, order_book: &OrderBook) -> OrderBookImbalance {
        let mut total_bid_volume = Decimal::ZERO;
        let mut total_ask_volume = Decimal::ZERO;
        let mut weighted_bid_price = Decimal::ZERO;
        let mut weighted_ask_price = Decimal::ZERO;

        // 计算前5档的加权价格和总量
        for (i, level) in order_book.bids.iter().take(5).enumerate() {
            let weight = Decimal::new(5 - i as i64, 0);
            total_bid_volume += level.quantity * weight;
            weighted_bid_price += level.price * level.quantity * weight;
        }

        for (i, level) in order_book.asks.iter().take(5).enumerate() {
            let weight = Decimal::new(5 - i as i64, 0);
            total_ask_volume += level.quantity * weight;
            weighted_ask_price += level.price * level.quantity * weight;
        }

        let bid_ask_ratio = if !total_ask_volume.is_zero() {
            total_bid_volume / total_ask_volume
        } else {
            Decimal::ONE
        };

        let weighted_mid_price = if !total_bid_volume.is_zero() && !total_ask_volume.is_zero() {
            (weighted_bid_price / total_bid_volume + weighted_ask_price / total_ask_volume) / Decimal::TWO
        } else {
            (order_book.bids[0].price + order_book.asks[0].price) / Decimal::TWO
        };

        let imbalance_strength = (bid_ask_ratio - Decimal::ONE).abs();

        OrderBookImbalance {
            bid_ask_ratio,
            weighted_mid_price,
            imbalance_strength,
            timestamp: Utc::now(),
        }
    }

    /// 计算市场微观结构指标
    async fn calculate_market_microstructure(&self) -> Option<MarketMicrostructure> {
        let trade_history = self.trade_history.lock().await;
        let order_book_history = self.order_book_history.lock().await;

        if trade_history.len() < 10 || order_book_history.is_empty() {
            return None;
        }

        // 计算有效价差
        let recent_trades: Vec<&Trade> = trade_history.iter().rev().take(10).collect();
        let mut total_volume = Decimal::ZERO;
        let mut volume_weighted_price = Decimal::ZERO;

        for trade in &recent_trades {
            total_volume += trade.quantity;
            volume_weighted_price += trade.price * trade.quantity;
        }

        let vwap = if !total_volume.is_zero() {
            volume_weighted_price / total_volume
        } else {
            recent_trades[0].price
        };

        // 计算时间加权平均价格
        let time_span = recent_trades[0].timestamp - recent_trades.last().unwrap().timestamp;
        let time_span_seconds = time_span.num_seconds() as f64;
        
        let mut time_weighted_price = Decimal::ZERO;
        let mut total_time_weight = Decimal::ZERO;

        for (i, trade) in recent_trades.iter().enumerate() {
            if i < recent_trades.len() - 1 {
                let time_diff = (trade.timestamp - recent_trades[i + 1].timestamp).num_seconds() as f64;
                let weight = Decimal::from_f64_retain(time_diff).unwrap_or(Decimal::ONE);
                time_weighted_price += trade.price * weight;
                total_time_weight += weight;
            }
        }

        let twap = if !total_time_weight.is_zero() {
            time_weighted_price / total_time_weight
        } else {
            vwap
        };

        // 计算价格冲击
        let latest_order_book = order_book_history.back()?;
        let mid_price = (latest_order_book.bids[0].price + latest_order_book.asks[0].price) / Decimal::TWO;
        let effective_spread = latest_order_book.asks[0].price - latest_order_book.bids[0].price;
        
        // 简化的价格冲击计算
        let price_impact = effective_spread / mid_price;

        // 计算订单流不平衡
        let buy_volume: Decimal = recent_trades.iter()
            .filter(|t| t.side == Side::Buy)
            .map(|t| t.quantity)
            .sum();
        
        let sell_volume: Decimal = recent_trades.iter()
            .filter(|t| t.side == Side::Sell)
            .map(|t| t.quantity)
            .sum();

        let order_flow_imbalance = if !total_volume.is_zero() {
            (buy_volume - sell_volume) / total_volume
        } else {
            Decimal::ZERO
        };

        Some(MarketMicrostructure {
            effective_spread,
            price_impact,
            order_flow_imbalance,
            vwap,
            twap,
            timestamp: Utc::now(),
        })
    }

    /// 更新自适应参数
    async fn update_adaptive_parameters(&self, performance_feedback: Decimal) {
        if !self.config.adaptive_learning {
            return;
        }

        let mut params = self.adaptive_params.write().await;
        
        // 简单的强化学习更新
        let learning_rate = params.learning_rate;
        
        // 根据性能反馈调整参数
        if performance_feedback > Decimal::ZERO {
            // 正反馈，增强当前参数
            params.spread_adjustment_factor *= (Decimal::ONE + learning_rate);
            params.inventory_adjustment_factor *= (Decimal::ONE + learning_rate);
        } else {
            // 负反馈，减弱当前参数
            params.spread_adjustment_factor *= (Decimal::ONE - learning_rate);
            params.inventory_adjustment_factor *= (Decimal::ONE - learning_rate);
        }

        // 限制参数范围
        params.spread_adjustment_factor = params.spread_adjustment_factor
            .max(Decimal::new(5, 1)) // 0.5
            .min(Decimal::new(20, 1)); // 2.0

        params.inventory_adjustment_factor = params.inventory_adjustment_factor
            .max(Decimal::new(5, 1)) // 0.5
            .min(Decimal::new(20, 1)); // 2.0

        params.update_count += 1;
        params.last_update = Utc::now();

        log::debug!("Updated adaptive parameters: spread_adj={}, inventory_adj={}", 
                   params.spread_adjustment_factor, params.inventory_adjustment_factor);
    }

    /// 计算动态价差
    async fn calculate_dynamic_spread(&self, base_spread: Decimal) -> Decimal {
        let mut adjusted_spread = base_spread;

        // 订单簿不平衡调整
        if self.config.order_book_imbalance_adjustment {
            if let Some(imbalance) = self.order_book_imbalance.read().await.as_ref() {
                let imbalance_adjustment = imbalance.imbalance_strength * Decimal::new(5, 3); // 0.005
                adjusted_spread += imbalance_adjustment;
            }
        }

        // 市场微观结构调整
        if let Some(microstructure) = self.market_microstructure.read().await.as_ref() {
            let impact_adjustment = microstructure.price_impact * Decimal::new(2, 0); // 2x
            adjusted_spread += impact_adjustment;
        }

        // 自适应调整
        if self.config.adaptive_learning {
            let params = self.adaptive_params.read().await;
            adjusted_spread *= params.spread_adjustment_factor;
        }

        // 确保价差在合理范围内
        let min_spread = Decimal::new(self.config.base_config.min_spread_bps as i64, 4);
        let max_spread = Decimal::new(self.config.base_config.max_spread_bps as i64, 4);
        
        adjusted_spread.max(min_spread).min(max_spread)
    }

    /// 计算多层报价
    async fn calculate_multi_level_quotes(&self, market: &MarketSnapshot) -> Result<Vec<Quote>> {
        let mut quotes = Vec::new();
        let base_spread = self.calculate_dynamic_spread(
            Decimal::new(self.config.base_config.base_spread_bps as i64, 4)
        ).await;

        for level in 0..self.config.max_order_levels {
            let level_multiplier = Decimal::ONE + 
                (Decimal::new(level as i64, 0) * self.config.level_spread_multiplier);
            
            let level_spread = base_spread * level_multiplier;
            let half_spread = level_spread / Decimal::TWO;

            // 库存偏差调整
            let inventory_skew = self.calculate_inventory_skew().await;
            
            let bid_price = market.mid_price - half_spread - inventory_skew;
            let ask_price = market.mid_price + half_spread + inventory_skew;

            // 调整订单大小（远离市场的订单可以更大）
            let size_multiplier = Decimal::ONE + Decimal::new(level as i64, 1); // 1.0, 1.1, 1.2, ...
            let order_size = self.config.base_config.order_size * size_multiplier;

            let quote = Quote {
                bid_price,
                bid_size: order_size,
                ask_price,
                ask_size: order_size,
                mid_price: market.mid_price,
                spread: level_spread,
                timestamp: Utc::now(),
            };

            quotes.push(quote);
        }

        Ok(quotes)
    }

    /// 计算库存偏差调整
    async fn calculate_inventory_skew(&self) -> Decimal {
        let position = self.position.read().await;
        let max_position = self.config.base_config.max_position;
        
        if max_position.is_zero() {
            return Decimal::ZERO;
        }

        let inventory_ratio = position.quantity / max_position;
        let base_skew = inventory_ratio * self.config.base_config.max_inventory_skew;

        // 自适应调整
        if self.config.adaptive_learning {
            let params = self.adaptive_params.read().await;
            base_skew * params.inventory_adjustment_factor
        } else {
            base_skew
        }
    }

    /// 检查快速取消条件
    async fn should_fast_cancel(&self, order: &Order) -> bool {
        let now = Utc::now();
        let order_age = now - order.created_at;
        
        if order_age.num_milliseconds() > self.config.fast_cancel_threshold_ms as i64 {
            return true;
        }

        // 检查市场冲击保护
        if self.config.market_impact_protection {
            if let Some(market) = self.market_snapshot.read().await.as_ref() {
                let price_change = match order.side {
                    Side::Buy => {
                        if let Some(price) = order.price {
                            (market.best_bid - price).abs() / price
                        } else {
                            Decimal::ZERO
                        }
                    }
                    Side::Sell => {
                        if let Some(price) = order.price {
                            (market.best_ask - price).abs() / price
                        } else {
                            Decimal::ZERO
                        }
                    }
                };

                // 如果价格变化超过阈值，快速取消
                if price_change > Decimal::new(5, 3) { // 0.5%
                    return true;
                }
            }
        }

        false
    }

    /// 更新历史数据
    async fn update_history(&self, event: &MarketDataEvent) {
        match event {
            MarketDataEvent::Trade(trade) => {
                let mut history = self.trade_history.lock().await;
                history.push_back(trade.clone());
                if history.len() > self.max_history_length {
                    history.pop_front();
                }
            }
            MarketDataEvent::OrderBook(order_book) => {
                let mut history = self.order_book_history.lock().await;
                history.push_back(order_book.clone());
                if history.len() > self.max_history_length {
                    history.pop_front();
                }

                // 计算订单簿不平衡
                let imbalance = self.calculate_order_book_imbalance(order_book).await;
                *self.order_book_imbalance.write().await = Some(imbalance);
            }
            MarketDataEvent::Ticker(ticker) => {
                let mut history = self.price_history.lock().await;
                history.push_back((ticker.timestamp, ticker.last_price));
                if history.len() > self.max_history_length {
                    history.pop_front();
                }
            }
            _ => {}
        }

        // 更新市场微观结构指标
        if let Some(microstructure) = self.calculate_market_microstructure().await {
            *self.market_microstructure.write().await = Some(microstructure);
        }
    }
}

#[async_trait]
impl MarketMakingStrategy for AdvancedMarketMaker {
    async fn initialize(&mut self) -> Result<()> {
        log::info!("Initializing advanced market making strategy for {}", 
                  self.config.base_config.symbol);
        
        *self.state.write().await = StrategyState::Initializing;
        
        log::info!("Advanced market making strategy initialized successfully");
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        log::info!("Starting advanced market making strategy");
        
        *self.state.write().await = StrategyState::Running;
        
        // 发送启动事件
        let _ = self.event_sender.send(StrategyEvent::Started);
        
        log::info!("Advanced market making strategy started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        log::info!("Stopping advanced market making strategy");
        
        // 取消所有活跃订单
        let active_orders = self.active_orders.read().await;
        for order_id in active_orders.keys() {
            log::info!("Cancelling order: {}", order_id);
        }
        
        *self.state.write().await = StrategyState::Stopped;
        
        // 发送停止事件
        let _ = self.event_sender.send(StrategyEvent::Stopped);
        
        log::info!("Advanced market making strategy stopped");
        Ok(())
    }

    async fn pause(&mut self) -> Result<()> {
        log::info!("Pausing advanced market making strategy");
        
        // 取消所有活跃订单
        let active_orders = self.active_orders.read().await;
        for order_id in active_orders.keys() {
            log::info!("Cancelling order: {}", order_id);
        }
        
        *self.state.write().await = StrategyState::Paused;
        
        log::info!("Advanced market making strategy paused");
        Ok(())
    }

    async fn resume(&mut self) -> Result<()> {
        log::info!("Resuming advanced market making strategy");
        
        *self.state.write().await = StrategyState::Running;
        
        // 重新计算并提交报价
        self.update_quotes().await?;
        
        log::info!("Advanced market making strategy resumed");
        Ok(())
    }

    async fn on_market_data(&mut self, event: MarketDataEvent) -> Result<()> {
        // 更新历史数据
        self.update_history(&event).await;

        match event {
            MarketDataEvent::Ticker(ticker) => {
                if ticker.symbol == self.config.base_config.symbol {
                    // 更新市场快照
                    let snapshot = MarketSnapshot {
                        last_price: ticker.last_price,
                        best_bid: ticker.bid_price,
                        best_ask: ticker.ask_price,
                        mid_price: (ticker.bid_price + ticker.ask_price) / Decimal::TWO,
                        spread: ticker.ask_price - ticker.bid_price,
                        volume_24h: ticker.volume_24h,
                        volatility: Decimal::new(20, 2), // 简化的波动率
                        timestamp: ticker.timestamp,
                    };

                    *self.market_snapshot.write().await = Some(snapshot);
                    
                    // 如果策略正在运行，更新报价
                    if matches!(*self.state.read().await, StrategyState::Running) {
                        self.update_quotes().await?;
                    }
                }
            }
            MarketDataEvent::OrderBook(order_book) => {
                if order_book.symbol == self.config.base_config.symbol && 
                   !order_book.bids.is_empty() && !order_book.asks.is_empty() {
                    
                    let best_bid = order_book.bids[0].price;
                    let best_ask = order_book.asks[0].price;
                    let mid_price = (best_bid + best_ask) / Decimal::TWO;

                    // 更新市场快照
                    let mut snapshot_guard = self.market_snapshot.write().await;
                    if let Some(ref mut snapshot) = *snapshot_guard {
                        snapshot.best_bid = best_bid;
                        snapshot.best_ask = best_ask;
                        snapshot.mid_price = mid_price;
                        snapshot.spread = best_ask - best_bid;
                        snapshot.timestamp = order_book.timestamp;
                    } else {
                        *snapshot_guard = Some(MarketSnapshot {
                            last_price: mid_price,
                            best_bid,
                            best_ask,
                            mid_price,
                            spread: best_ask - best_bid,
                            volume_24h: Decimal::ZERO,
                            volatility: Decimal::new(20, 2),
                            timestamp: order_book.timestamp,
                        });
                    }

                    // 如果策略正在运行，更新报价
                    if matches!(*self.state.read().await, StrategyState::Running) {
                        self.update_quotes().await?;
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }

    async fn on_order_update(&mut self, order: Order) -> Result<()> {
        let order_id = order.id.clone();
        
        match order.status {
            OrderStatus::Filled => {
                // 更新持仓
                let mut position = self.position.write().await;
                let trade_quantity = match order.side {
                    Side::Buy => order.filled_quantity,
                    Side::Sell => -order.filled_quantity,
                };

                let new_quantity = position.quantity + trade_quantity;
                if !new_quantity.is_zero() {
                    position.average_price = (position.average_price * position.quantity + 
                                            order.average_fill_price * trade_quantity) / new_quantity;
                }
                position.quantity = new_quantity;
                position.last_update = Utc::now();

                // 更新策略指标
                let mut metrics = self.metrics.write().await;
                metrics.total_trades += 1;
                metrics.current_inventory = position.quantity;

                // 计算盈亏并更新自适应参数
                let pnl = match order.side {
                    Side::Buy => Decimal::ZERO, // 买入时暂时无法计算盈亏
                    Side::Sell => {
                        // 卖出时计算盈亏
                        order.average_fill_price - position.average_price
                    }
                };

                if !pnl.is_zero() {
                    self.update_adaptive_parameters(pnl).await;
                }

                // 从活跃订单中移除
                self.active_orders.write().await.remove(&order_id);

                // 发送成交事件
                let _ = self.event_sender.send(StrategyEvent::OrderFilled(
                    order_id,
                    order.filled_quantity,
                    order.average_fill_price,
                ));

                log::info!("Order filled: {} {} @ {}", 
                          order.side, order.filled_quantity, order.average_fill_price);
            }
            OrderStatus::Cancelled => {
                self.active_orders.write().await.remove(&order_id);
                let _ = self.event_sender.send(StrategyEvent::OrderCancelled(order_id));
            }
            OrderStatus::Rejected => {
                self.active_orders.write().await.remove(&order_id);
                let _ = self.event_sender.send(StrategyEvent::Error(
                    format!("Order rejected: {}", order.id)
                ));
            }
            _ => {
                // 检查是否需要快速取消
                if self.should_fast_cancel(&order).await {
                    log::info!("Fast cancelling order: {}", order_id);
                    // 这里应该发送取消订单请求
                }

                // 更新活跃订单状态
                if let Some(active_order) = self.active_orders.write().await.get_mut(&order_id) {
                    *active_order = order;
                }
            }
        }

        Ok(())
    }

    async fn calculate_quote(&self, market: &MarketSnapshot) -> Result<Quote> {
        // 使用多层报价的第一层作为主要报价
        let quotes = self.calculate_multi_level_quotes(market).await?;
        Ok(quotes.into_iter().next().unwrap_or_else(|| Quote {
            bid_price: market.mid_price - Decimal::new(10, 4),
            bid_size: self.config.base_config.order_size,
            ask_price: market.mid_price + Decimal::new(10, 4),
            ask_size: self.config.base_config.order_size,
            mid_price: market.mid_price,
            spread: Decimal::new(20, 4),
            timestamp: Utc::now(),
        }))
    }

    async fn update_quotes(&mut self) -> Result<()> {
        let market_snapshot = self.market_snapshot.read().await;
        
        if let Some(ref market) = *market_snapshot {
            // 计算多层报价
            let new_quotes = self.calculate_multi_level_quotes(market).await?;

            // 取消现有订单
            let active_orders = self.active_orders.read().await;
            for order_id in active_orders.keys() {
                log::debug!("Cancelling order: {}", order_id);
            }
            drop(active_orders);

            // 提交新的多层订单
            for (level, quote) in new_quotes.iter().enumerate() {
                // 提交买单
                let bid_order_id = OrderId::new();
                let bid_order = Order {
                    id: bid_order_id.clone(),
                    symbol: self.config.base_config.symbol.clone(),
                    side: Side::Buy,
                    order_type: OrderType::Limit,
                    quantity: quote.bid_size,
                    price: Some(quote.bid_price),
                    status: OrderStatus::Pending,
                    filled_quantity: Decimal::ZERO,
                    average_fill_price: Decimal::ZERO,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                    exchange: self.config.base_config.symbol.exchange,
                };

                self.order_sender.send(bid_order.clone())
                    .map_err(|e| HftError::Other(format!("Failed to send bid order: {}", e)))?;

                // 提交卖单
                let ask_order_id = OrderId::new();
                let ask_order = Order {
                    id: ask_order_id.clone(),
                    symbol: self.config.base_config.symbol.clone(),
                    side: Side::Sell,
                    order_type: OrderType::Limit,
                    quantity: quote.ask_size,
                    price: Some(quote.ask_price),
                    status: OrderStatus::Pending,
                    filled_quantity: Decimal::ZERO,
                    average_fill_price: Decimal::ZERO,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                    exchange: self.config.base_config.symbol.exchange,
                };

                self.order_sender.send(ask_order.clone())
                    .map_err(|e| HftError::Other(format!("Failed to send ask order: {}", e)))?;

                // 记录活跃订单
                self.active_orders.write().await.insert(bid_order_id.clone(), bid_order);
                self.active_orders.write().await.insert(ask_order_id.clone(), ask_order);

                log::info!("Level {} quotes: bid {} @ {}, ask {} @ {}",
                          level + 1, quote.bid_size, quote.bid_price,
                          quote.ask_size, quote.ask_price);
            }

            // 更新当前报价
            *self.current_quotes.write().await = new_quotes.clone();

            // 发送报价更新事件
            if let Some(main_quote) = new_quotes.first() {
                let _ = self.event_sender.send(StrategyEvent::QuoteUpdated(main_quote.clone()));
            }
        }

        Ok(())
    }

    fn get_state(&self) -> StrategyState {
        self.state.try_read()
            .map(|guard| guard.clone())
            .unwrap_or(StrategyState::Error("Failed to read state".to_string()))
    }

    fn get_metrics(&self) -> StrategyMetrics {
        self.metrics.try_read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    fn get_current_quote(&self) -> Option<Quote> {
        self.current_quotes.try_read()
            .and_then(|guard| guard.first().cloned())
            .unwrap_or(None)
    }
}

