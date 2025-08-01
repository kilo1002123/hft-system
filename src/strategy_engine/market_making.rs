use crate::common::{
    Exchange, HftError, MarketDataEvent, OrderBook, Result, Side, Symbol, Ticker, Trade,
};
use crate::oms::{Order, OrderId, OrderStatus, OrderType};
use crate::rms::{Position, RiskManager};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use uuid::Uuid;

/// 做市策略配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketMakingConfig {
    /// 目标交易对
    pub symbol: Symbol,
    /// 基础价差 (基点，1基点 = 0.01%)
    pub base_spread_bps: u32,
    /// 最小价差 (基点)
    pub min_spread_bps: u32,
    /// 最大价差 (基点)
    pub max_spread_bps: u32,
    /// 单次下单数量
    pub order_size: Decimal,
    /// 最大持仓数量
    pub max_position: Decimal,
    /// 最大库存偏差
    pub max_inventory_skew: Decimal,
    /// 价格更新阈值 (基点)
    pub price_update_threshold_bps: u32,
    /// 订单刷新间隔 (毫秒)
    pub refresh_interval_ms: u64,
    /// 风险调整因子
    pub risk_adjustment_factor: Decimal,
    /// 波动率调整开关
    pub volatility_adjustment: bool,
    /// 库存偏差调整开关
    pub inventory_skew_adjustment: bool,
}

impl Default for MarketMakingConfig {
    fn default() -> Self {
        Self {
            symbol: Symbol::new("BTC", "USDT", Exchange::Binance),
            base_spread_bps: 10,      // 0.1%
            min_spread_bps: 5,        // 0.05%
            max_spread_bps: 50,       // 0.5%
            order_size: Decimal::new(1, 2), // 0.01
            max_position: Decimal::new(1, 0), // 1.0
            max_inventory_skew: Decimal::new(5, 1), // 0.5
            price_update_threshold_bps: 5, // 0.05%
            refresh_interval_ms: 1000,
            risk_adjustment_factor: Decimal::new(15, 1), // 1.5
            volatility_adjustment: true,
            inventory_skew_adjustment: true,
        }
    }
}

/// 做市策略状态
#[derive(Debug, Clone, PartialEq)]
pub enum StrategyState {
    /// 初始化中
    Initializing,
    /// 运行中
    Running,
    /// 暂停
    Paused,
    /// 停止
    Stopped,
    /// 错误状态
    Error(String),
}

/// 做市报价
#[derive(Debug, Clone)]
pub struct Quote {
    /// 买价
    pub bid_price: Decimal,
    /// 买量
    pub bid_size: Decimal,
    /// 卖价
    pub ask_price: Decimal,
    /// 卖量
    pub ask_size: Decimal,
    /// 中间价
    pub mid_price: Decimal,
    /// 价差
    pub spread: Decimal,
    /// 时间戳
    pub timestamp: DateTime<Utc>,
}

/// 策略指标
#[derive(Debug, Clone, Default)]
pub struct StrategyMetrics {
    /// 总交易次数
    pub total_trades: u64,
    /// 盈利交易次数
    pub profitable_trades: u64,
    /// 总盈亏
    pub total_pnl: Decimal,
    /// 已实现盈亏
    pub realized_pnl: Decimal,
    /// 未实现盈亏
    pub unrealized_pnl: Decimal,
    /// 最大回撤
    pub max_drawdown: Decimal,
    /// 夏普比率
    pub sharpe_ratio: Decimal,
    /// 平均持仓时间 (秒)
    pub avg_holding_time: f64,
    /// 成交率
    pub fill_rate: Decimal,
    /// 当前库存
    pub current_inventory: Decimal,
}

/// 市场数据快照
#[derive(Debug, Clone)]
pub struct MarketSnapshot {
    /// 最新价格
    pub last_price: Decimal,
    /// 最佳买价
    pub best_bid: Decimal,
    /// 最佳卖价
    pub best_ask: Decimal,
    /// 中间价
    pub mid_price: Decimal,
    /// 价差
    pub spread: Decimal,
    /// 24小时成交量
    pub volume_24h: Decimal,
    /// 波动率 (年化)
    pub volatility: Decimal,
    /// 更新时间
    pub timestamp: DateTime<Utc>,
}

/// 做市策略事件
#[derive(Debug, Clone)]
pub enum StrategyEvent {
    /// 策略启动
    Started,
    /// 策略停止
    Stopped,
    /// 报价更新
    QuoteUpdated(Quote),
    /// 订单提交
    OrderSubmitted(OrderId),
    /// 订单成交
    OrderFilled(OrderId, Decimal, Decimal),
    /// 订单取消
    OrderCancelled(OrderId),
    /// 风险警告
    RiskWarning(String),
    /// 错误事件
    Error(String),
}

/// 做市策略 trait
#[async_trait]
pub trait MarketMakingStrategy: Send + Sync {
    /// 初始化策略
    async fn initialize(&mut self) -> Result<()>;
    
    /// 启动策略
    async fn start(&mut self) -> Result<()>;
    
    /// 停止策略
    async fn stop(&mut self) -> Result<()>;
    
    /// 暂停策略
    async fn pause(&mut self) -> Result<()>;
    
    /// 恢复策略
    async fn resume(&mut self) -> Result<()>;
    
    /// 处理市场数据
    async fn on_market_data(&mut self, event: MarketDataEvent) -> Result<()>;
    
    /// 处理订单更新
    async fn on_order_update(&mut self, order: Order) -> Result<()>;
    
    /// 计算报价
    async fn calculate_quote(&self, market: &MarketSnapshot) -> Result<Quote>;
    
    /// 更新报价
    async fn update_quotes(&mut self) -> Result<()>;
    
    /// 获取策略状态
    fn get_state(&self) -> StrategyState;
    
    /// 获取策略指标
    fn get_metrics(&self) -> StrategyMetrics;
    
    /// 获取当前报价
    fn get_current_quote(&self) -> Option<Quote>;
}

/// 基础做市策略实现
pub struct BasicMarketMaker {
    /// 策略配置
    config: MarketMakingConfig,
    /// 策略状态
    state: Arc<RwLock<StrategyState>>,
    /// 当前市场快照
    market_snapshot: Arc<RwLock<Option<MarketSnapshot>>>,
    /// 当前报价
    current_quote: Arc<RwLock<Option<Quote>>>,
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
    /// 价格历史 (用于计算波动率)
    price_history: Arc<Mutex<Vec<(DateTime<Utc>, Decimal)>>>,
    /// 最大价格历史长度
    max_price_history: usize,
}

impl BasicMarketMaker {
    /// 创建新的做市策略实例
    pub fn new(
        config: MarketMakingConfig,
        risk_manager: Arc<dyn RiskManager>,
        event_sender: mpsc::UnboundedSender<StrategyEvent>,
        order_sender: mpsc::UnboundedSender<Order>,
    ) -> Self {
        let initial_position = Position {
            symbol: config.symbol.clone(),
            quantity: Decimal::ZERO,
            average_price: Decimal::ZERO,
            unrealized_pnl: Decimal::ZERO,
            realized_pnl: Decimal::ZERO,
            last_update: Utc::now(),
        };

        Self {
            config,
            state: Arc::new(RwLock::new(StrategyState::Initializing)),
            market_snapshot: Arc::new(RwLock::new(None)),
            current_quote: Arc::new(RwLock::new(None)),
            active_orders: Arc::new(RwLock::new(HashMap::new())),
            position: Arc::new(RwLock::new(initial_position)),
            metrics: Arc::new(RwLock::new(StrategyMetrics::default())),
            risk_manager,
            event_sender,
            order_sender,
            price_history: Arc::new(Mutex::new(Vec::new())),
            max_price_history: 1000,
        }
    }

    /// 计算波动率
    async fn calculate_volatility(&self) -> Decimal {
        let price_history = self.price_history.lock().await;
        
        if price_history.len() < 2 {
            return Decimal::new(20, 2); // 默认20%年化波动率
        }

        let returns: Vec<f64> = price_history
            .windows(2)
            .map(|window| {
                let prev_price = window[0].1.to_f64().unwrap_or(0.0);
                let curr_price = window[1].1.to_f64().unwrap_or(0.0);
                if prev_price > 0.0 {
                    (curr_price / prev_price).ln()
                } else {
                    0.0
                }
            })
            .collect();

        if returns.is_empty() {
            return Decimal::new(20, 2);
        }

        let mean = returns.iter().sum::<f64>() / returns.len() as f64;
        let variance = returns
            .iter()
            .map(|r| (r - mean).powi(2))
            .sum::<f64>() / returns.len() as f64;
        
        let volatility = variance.sqrt() * (365.25 * 24.0 * 60.0).sqrt(); // 年化
        Decimal::from_f64_retain(volatility).unwrap_or(Decimal::new(20, 2))
    }

    /// 计算库存偏差调整
    async fn calculate_inventory_skew(&self) -> Decimal {
        let position = self.position.read().await;
        let max_position = self.config.max_position;
        
        if max_position.is_zero() {
            return Decimal::ZERO;
        }

        let inventory_ratio = position.quantity / max_position;
        
        // 库存偏差调整：当持有多头时，降低买价，提高卖价
        inventory_ratio * self.config.max_inventory_skew
    }

    /// 更新价格历史
    async fn update_price_history(&self, price: Decimal) {
        let mut history = self.price_history.lock().await;
        history.push((Utc::now(), price));
        
        // 保持历史长度在限制内
        if history.len() > self.max_price_history {
            history.remove(0);
        }
    }

    /// 检查是否需要更新报价
    async fn should_update_quote(&self, new_mid_price: Decimal) -> bool {
        let current_quote = self.current_quote.read().await;
        
        match current_quote.as_ref() {
            Some(quote) => {
                let price_change = (new_mid_price - quote.mid_price).abs();
                let threshold = quote.mid_price * Decimal::new(self.config.price_update_threshold_bps as i64, 4);
                price_change >= threshold
            }
            None => true,
        }
    }

    /// 取消所有活跃订单
    async fn cancel_all_orders(&self) -> Result<()> {
        let active_orders = self.active_orders.read().await;
        
        for order_id in active_orders.keys() {
            // 这里应该调用订单管理系统取消订单
            log::info!("Cancelling order: {}", order_id);
        }
        
        Ok(())
    }

    /// 提交新订单
    async fn submit_order(&self, side: Side, price: Decimal, size: Decimal) -> Result<OrderId> {
        let order_id = OrderId::new();
        
        let order = Order {
            id: order_id.clone(),
            symbol: self.config.symbol.clone(),
            side,
            order_type: OrderType::Limit,
            quantity: size,
            price: Some(price),
            status: OrderStatus::Pending,
            filled_quantity: Decimal::ZERO,
            average_fill_price: Decimal::ZERO,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            exchange: self.config.symbol.exchange,
        };

        // 发送订单到订单管理系统
        self.order_sender.send(order.clone())
            .map_err(|e| HftError::Other(format!("Failed to send order: {}", e)))?;

        // 记录活跃订单
        self.active_orders.write().await.insert(order_id.clone(), order);

        // 发送策略事件
        let _ = self.event_sender.send(StrategyEvent::OrderSubmitted(order_id.clone()));

        Ok(order_id)
    }
}

#[async_trait]
impl MarketMakingStrategy for BasicMarketMaker {
    async fn initialize(&mut self) -> Result<()> {
        log::info!("Initializing market making strategy for {}", self.config.symbol);
        
        // 初始化风险管理器
        // self.risk_manager.initialize().await?;
        
        *self.state.write().await = StrategyState::Initializing;
        
        log::info!("Market making strategy initialized successfully");
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        log::info!("Starting market making strategy");
        
        *self.state.write().await = StrategyState::Running;
        
        // 发送启动事件
        let _ = self.event_sender.send(StrategyEvent::Started);
        
        log::info!("Market making strategy started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        log::info!("Stopping market making strategy");
        
        // 取消所有活跃订单
        self.cancel_all_orders().await?;
        
        *self.state.write().await = StrategyState::Stopped;
        
        // 发送停止事件
        let _ = self.event_sender.send(StrategyEvent::Stopped);
        
        log::info!("Market making strategy stopped");
        Ok(())
    }

    async fn pause(&mut self) -> Result<()> {
        log::info!("Pausing market making strategy");
        
        // 取消所有活跃订单
        self.cancel_all_orders().await?;
        
        *self.state.write().await = StrategyState::Paused;
        
        log::info!("Market making strategy paused");
        Ok(())
    }

    async fn resume(&mut self) -> Result<()> {
        log::info!("Resuming market making strategy");
        
        *self.state.write().await = StrategyState::Running;
        
        // 重新计算并提交报价
        self.update_quotes().await?;
        
        log::info!("Market making strategy resumed");
        Ok(())
    }

    async fn on_market_data(&mut self, event: MarketDataEvent) -> Result<()> {
        match event {
            MarketDataEvent::Ticker(ticker) => {
                if ticker.symbol == self.config.symbol {
                    // 更新市场快照
                    let snapshot = MarketSnapshot {
                        last_price: ticker.last_price,
                        best_bid: ticker.bid_price,
                        best_ask: ticker.ask_price,
                        mid_price: (ticker.bid_price + ticker.ask_price) / Decimal::TWO,
                        spread: ticker.ask_price - ticker.bid_price,
                        volume_24h: ticker.volume_24h,
                        volatility: self.calculate_volatility().await,
                        timestamp: ticker.timestamp,
                    };

                    // 更新价格历史
                    self.update_price_history(snapshot.mid_price).await;

                    // 检查是否需要更新报价
                    if self.should_update_quote(snapshot.mid_price).await {
                        *self.market_snapshot.write().await = Some(snapshot);
                        
                        // 如果策略正在运行，更新报价
                        if matches!(*self.state.read().await, StrategyState::Running) {
                            self.update_quotes().await?;
                        }
                    }
                }
            }
            MarketDataEvent::OrderBook(order_book) => {
                if order_book.symbol == self.config.symbol && !order_book.bids.is_empty() && !order_book.asks.is_empty() {
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
                            volatility: self.calculate_volatility().await,
                            timestamp: order_book.timestamp,
                        });
                    }

                    // 更新价格历史
                    self.update_price_history(mid_price).await;

                    // 检查是否需要更新报价
                    if self.should_update_quote(mid_price).await {
                        // 如果策略正在运行，更新报价
                        if matches!(*self.state.read().await, StrategyState::Running) {
                            self.update_quotes().await?;
                        }
                    }
                }
            }
            MarketDataEvent::Trade(trade) => {
                if trade.symbol == self.config.symbol {
                    // 更新价格历史
                    self.update_price_history(trade.price).await;
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

                // 更新持仓数量和平均价格
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
                // 从活跃订单中移除
                self.active_orders.write().await.remove(&order_id);

                // 发送取消事件
                let _ = self.event_sender.send(StrategyEvent::OrderCancelled(order_id));

                log::info!("Order cancelled: {}", order.id);
            }
            OrderStatus::Rejected => {
                // 从活跃订单中移除
                self.active_orders.write().await.remove(&order_id);

                // 发送错误事件
                let _ = self.event_sender.send(StrategyEvent::Error(
                    format!("Order rejected: {}", order.id)
                ));

                log::warn!("Order rejected: {}", order.id);
            }
            _ => {
                // 更新活跃订单状态
                if let Some(active_order) = self.active_orders.write().await.get_mut(&order_id) {
                    *active_order = order;
                }
            }
        }

        Ok(())
    }

    async fn calculate_quote(&self, market: &MarketSnapshot) -> Result<Quote> {
        let mut base_spread = Decimal::new(self.config.base_spread_bps as i64, 4);
        
        // 波动率调整
        if self.config.volatility_adjustment {
            let vol_adjustment = market.volatility * self.config.risk_adjustment_factor / Decimal::new(100, 0);
            base_spread += vol_adjustment;
        }

        // 库存偏差调整
        let inventory_skew = if self.config.inventory_skew_adjustment {
            self.calculate_inventory_skew().await
        } else {
            Decimal::ZERO
        };

        // 确保价差在合理范围内
        let min_spread = Decimal::new(self.config.min_spread_bps as i64, 4);
        let max_spread = Decimal::new(self.config.max_spread_bps as i64, 4);
        base_spread = base_spread.max(min_spread).min(max_spread);

        // 计算买卖价格
        let half_spread = base_spread / Decimal::TWO;
        let bid_price = market.mid_price - half_spread - inventory_skew;
        let ask_price = market.mid_price + half_spread + inventory_skew;

        let quote = Quote {
            bid_price,
            bid_size: self.config.order_size,
            ask_price,
            ask_size: self.config.order_size,
            mid_price: market.mid_price,
            spread: ask_price - bid_price,
            timestamp: Utc::now(),
        };

        Ok(quote)
    }

    async fn update_quotes(&mut self) -> Result<()> {
        let market_snapshot = self.market_snapshot.read().await;
        
        if let Some(ref market) = *market_snapshot {
            // 计算新报价
            let new_quote = self.calculate_quote(market).await?;

            // 风险检查
            // if !self.risk_manager.check_quote_risk(&new_quote).await? {
            //     let _ = self.event_sender.send(StrategyEvent::RiskWarning(
            //         "Quote rejected by risk manager".to_string()
            //     ));
            //     return Ok(());
            // }

            // 取消现有订单
            self.cancel_all_orders().await?;

            // 提交新订单
            let bid_order_id = self.submit_order(
                Side::Buy,
                new_quote.bid_price,
                new_quote.bid_size,
            ).await?;

            let ask_order_id = self.submit_order(
                Side::Sell,
                new_quote.ask_price,
                new_quote.ask_size,
            ).await?;

            // 更新当前报价
            *self.current_quote.write().await = Some(new_quote.clone());

            // 发送报价更新事件
            let _ = self.event_sender.send(StrategyEvent::QuoteUpdated(new_quote));

            log::info!("Updated quotes: bid {} @ {}, ask {} @ {}",
                      new_quote.bid_size, new_quote.bid_price,
                      new_quote.ask_size, new_quote.ask_price);
        }

        Ok(())
    }

    fn get_state(&self) -> StrategyState {
        // 注意：这里使用 try_read 避免阻塞
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
        self.current_quote.try_read()
            .and_then(|guard| guard.clone())
            .unwrap_or(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rms::MockRiskManager;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_market_making_config_default() {
        let config = MarketMakingConfig::default();
        assert_eq!(config.base_spread_bps, 10);
        assert_eq!(config.min_spread_bps, 5);
        assert_eq!(config.max_spread_bps, 50);
    }

    #[tokio::test]
    async fn test_basic_market_maker_creation() {
        let config = MarketMakingConfig::default();
        let risk_manager = Arc::new(MockRiskManager::new());
        let (event_sender, _) = mpsc::unbounded_channel();
        let (order_sender, _) = mpsc::unbounded_channel();

        let strategy = BasicMarketMaker::new(
            config,
            risk_manager,
            event_sender,
            order_sender,
        );

        assert!(matches!(strategy.get_state(), StrategyState::Initializing));
    }

    #[tokio::test]
    async fn test_strategy_lifecycle() {
        let config = MarketMakingConfig::default();
        let risk_manager = Arc::new(MockRiskManager::new());
        let (event_sender, _) = mpsc::unbounded_channel();
        let (order_sender, _) = mpsc::unbounded_channel();

        let mut strategy = BasicMarketMaker::new(
            config,
            risk_manager,
            event_sender,
            order_sender,
        );

        // 初始化
        assert!(strategy.initialize().await.is_ok());

        // 启动
        assert!(strategy.start().await.is_ok());
        assert!(matches!(strategy.get_state(), StrategyState::Running));

        // 暂停
        assert!(strategy.pause().await.is_ok());
        assert!(matches!(strategy.get_state(), StrategyState::Paused));

        // 恢复
        assert!(strategy.resume().await.is_ok());
        assert!(matches!(strategy.get_state(), StrategyState::Running));

        // 停止
        assert!(strategy.stop().await.is_ok());
        assert!(matches!(strategy.get_state(), StrategyState::Stopped));
    }
}

