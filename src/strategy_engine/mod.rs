pub mod market_making;
pub mod advanced_market_making;
pub mod risk_controls;

use crate::common::{HftError, MarketDataEvent, Result, Symbol};
use crate::oms::Order;
use async_trait::async_trait;
use market_making::{MarketMakingStrategy, StrategyEvent};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

/// 策略引擎配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyEngineConfig {
    /// 最大并发策略数
    pub max_concurrent_strategies: usize,
    /// 策略更新间隔 (毫秒)
    pub update_interval_ms: u64,
    /// 事件缓冲区大小
    pub event_buffer_size: usize,
}

impl Default for StrategyEngineConfig {
    fn default() -> Self {
        Self {
            max_concurrent_strategies: 10,
            update_interval_ms: 100,
            event_buffer_size: 1000,
        }
    }
}

/// 策略类型
#[derive(Debug, Clone, PartialEq)]
pub enum StrategyType {
    MarketMaking,
    Arbitrage,
    TrendFollowing,
    MeanReversion,
}

/// 策略信息
#[derive(Debug, Clone)]
pub struct StrategyInfo {
    pub id: String,
    pub name: String,
    pub strategy_type: StrategyType,
    pub symbols: Vec<Symbol>,
    pub is_active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// 策略引擎
pub struct StrategyEngine {
    config: StrategyEngineConfig,
    strategies: Arc<RwLock<HashMap<String, Arc<dyn MarketMakingStrategy>>>>,
    strategy_info: Arc<RwLock<HashMap<String, StrategyInfo>>>,
    is_running: Arc<RwLock<bool>>,
    event_sender: mpsc::UnboundedSender<StrategyEvent>,
    event_receiver: Option<mpsc::UnboundedReceiver<StrategyEvent>>,
    order_sender: mpsc::UnboundedSender<Order>,
    order_receiver: Option<mpsc::UnboundedReceiver<Order>>,
}

impl StrategyEngine {
    pub fn new(config: StrategyEngineConfig) -> Self {
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        let (order_sender, order_receiver) = mpsc::unbounded_channel();

        Self {
            config,
            strategies: Arc::new(RwLock::new(HashMap::new())),
            strategy_info: Arc::new(RwLock::new(HashMap::new())),
            is_running: Arc::new(RwLock::new(false)),
            event_sender,
            event_receiver: Some(event_receiver),
            order_sender,
            order_receiver: Some(order_receiver),
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        log::info!("Starting strategy engine");
        
        *self.is_running.write().await = true;

        // 启动事件处理任务
        if let Some(mut event_receiver) = self.event_receiver.take() {
            let is_running = self.is_running.clone();
            tokio::spawn(async move {
                while *is_running.read().await {
                    if let Some(event) = event_receiver.recv().await {
                        log::debug!("Strategy event: {:?}", event);
                        // 处理策略事件
                    }
                }
            });
        }

        // 启动订单处理任务
        if let Some(mut order_receiver) = self.order_receiver.take() {
            let is_running = self.is_running.clone();
            tokio::spawn(async move {
                while *is_running.read().await {
                    if let Some(order) = order_receiver.recv().await {
                        log::debug!("Strategy order: {:?}", order);
                        // 处理策略订单
                    }
                }
            });
        }

        log::info!("Strategy engine started");
        Ok(())
    }

    pub async fn stop(&mut self) -> Result<()> {
        log::info!("Stopping strategy engine");
        
        // 停止所有策略
        let strategies = self.strategies.read().await;
        for (name, strategy) in strategies.iter() {
            log::info!("Stopping strategy: {}", name);
            // strategy.stop().await?;
        }

        *self.is_running.write().await = false;
        
        log::info!("Strategy engine stopped");
        Ok(())
    }

    pub async fn add_strategy(
        &mut self,
        id: String,
        name: String,
        strategy_type: StrategyType,
        strategy: Arc<dyn MarketMakingStrategy>,
    ) -> Result<()> {
        let strategies = self.strategies.read().await;
        if strategies.len() >= self.config.max_concurrent_strategies {
            return Err(HftError::Other("Maximum strategies reached".to_string()));
        }
        drop(strategies);

        let symbols = vec![]; // strategy.get_symbols();
        let info = StrategyInfo {
            id: id.clone(),
            name: name.clone(),
            strategy_type,
            symbols,
            is_active: false,
            created_at: chrono::Utc::now(),
        };

        self.strategies.write().await.insert(id.clone(), strategy);
        self.strategy_info.write().await.insert(id, info);

        log::info!("Added strategy: {}", name);
        Ok(())
    }

    pub async fn remove_strategy(&mut self, id: &str) -> Result<()> {
        if let Some(strategy) = self.strategies.write().await.remove(id) {
            // strategy.stop().await?;
            self.strategy_info.write().await.remove(id);
            log::info!("Removed strategy: {}", id);
            Ok(())
        } else {
            Err(HftError::Other(format!("Strategy not found: {}", id)))
        }
    }

    pub async fn start_strategy(&self, id: &str) -> Result<()> {
        let strategies = self.strategies.read().await;
        if let Some(strategy) = strategies.get(id) {
            // strategy.start().await?;
            
            // 更新策略信息
            if let Some(info) = self.strategy_info.write().await.get_mut(id) {
                info.is_active = true;
            }
            
            log::info!("Started strategy: {}", id);
            Ok(())
        } else {
            Err(HftError::Other(format!("Strategy not found: {}", id)))
        }
    }

    pub async fn stop_strategy(&self, id: &str) -> Result<()> {
        let strategies = self.strategies.read().await;
        if let Some(strategy) = strategies.get(id) {
            // strategy.stop().await?;
            
            // 更新策略信息
            if let Some(info) = self.strategy_info.write().await.get_mut(id) {
                info.is_active = false;
            }
            
            log::info!("Stopped strategy: {}", id);
            Ok(())
        } else {
            Err(HftError::Other(format!("Strategy not found: {}", id)))
        }
    }

    pub async fn on_market_data(&self, event: MarketDataEvent) -> Result<()> {
        let strategies = self.strategies.read().await;
        
        for (id, strategy) in strategies.iter() {
            // 检查策略是否对此符号感兴趣
            // if strategy.is_interested_in_symbol(&event.get_symbol()) {
            //     if let Err(e) = strategy.on_market_data(event.clone()).await {
            //         log::error!("Strategy {} failed to process market data: {}", id, e);
            //     }
            // }
        }
        
        Ok(())
    }

    pub async fn on_order_update(&self, order: Order) -> Result<()> {
        let strategies = self.strategies.read().await;
        
        for (id, strategy) in strategies.iter() {
            // 检查订单是否属于此策略
            // if strategy.owns_order(&order.id) {
            //     if let Err(e) = strategy.on_order_update(order.clone()).await {
            //         log::error!("Strategy {} failed to process order update: {}", id, e);
            //     }
            // }
        }
        
        Ok(())
    }

    pub async fn get_strategy_info(&self, id: &str) -> Option<StrategyInfo> {
        self.strategy_info.read().await.get(id).cloned()
    }

    pub async fn list_strategies(&self) -> Vec<StrategyInfo> {
        self.strategy_info.read().await.values().cloned().collect()
    }

    pub async fn is_running(&self) -> bool {
        *self.is_running.read().await
    }

    pub fn get_event_sender(&self) -> mpsc::UnboundedSender<StrategyEvent> {
        self.event_sender.clone()
    }

    pub fn get_order_sender(&self) -> mpsc::UnboundedSender<Order> {
        self.order_sender.clone()
    }
}

/// 策略 trait
#[async_trait]
pub trait Strategy: Send + Sync {
    async fn initialize(&mut self) -> Result<()>;
    async fn start(&mut self) -> Result<()>;
    async fn stop(&mut self) -> Result<()>;
    fn get_name(&self) -> &str;
    fn get_symbols(&self) -> Vec<Symbol>;
}

/// 交易策略接口 (保持向后兼容)
#[async_trait]
pub trait TradingStrategy: Send + Sync {
    /// 策略名称
    fn name(&self) -> &str;
    
    /// 处理市场数据
    async fn on_market_data(&mut self, event: MarketDataEvent) -> Result<Vec<Order>>;
    
    /// 处理订单更新
    async fn on_order_update(&mut self, order: Order) -> Result<Vec<Order>>;
    
    /// 策略初始化
    async fn initialize(&mut self) -> Result<()>;
    
    /// 策略停止
    async fn stop(&mut self) -> Result<()>;
}

/// 简单的做市策略 (保持向后兼容)
pub struct SimpleMarketMakingStrategy {
    name: String,
    target_symbol: Symbol,
    spread: rust_decimal::Decimal,
}

impl SimpleMarketMakingStrategy {
    pub fn new(name: String, target_symbol: Symbol, spread: rust_decimal::Decimal) -> Self {
        Self {
            name,
            target_symbol,
            spread,
        }
    }
}

#[async_trait]
impl TradingStrategy for SimpleMarketMakingStrategy {
    fn name(&self) -> &str {
        &self.name
    }
    
    async fn on_market_data(&mut self, event: MarketDataEvent) -> Result<Vec<Order>> {
        match event {
            MarketDataEvent::Ticker(ticker) => {
                if ticker.symbol == self.target_symbol {
                    log::info!("Processing ticker for {}: {}", self.target_symbol, ticker.last_price);
                    // 这里可以实现做市逻辑
                }
            }
            MarketDataEvent::OrderBook(order_book) => {
                if order_book.symbol == self.target_symbol {
                    log::debug!("Processing order book for {}", self.target_symbol);
                    // 这里可以实现基于订单簿的做市逻辑
                }
            }
            _ => {}
        }
        Ok(Vec::new())
    }
    
    async fn on_order_update(&mut self, order: Order) -> Result<Vec<Order>> {
        log::info!("Strategy {} received order update: {:?}", self.name, order);
        Ok(Vec::new())
    }
    
    async fn initialize(&mut self) -> Result<()> {
        log::info!("Initializing strategy: {}", self.name);
        Ok(())
    }
    
    async fn stop(&mut self) -> Result<()> {
        log::info!("Stopping strategy: {}", self.name);
        Ok(())
    }
}

