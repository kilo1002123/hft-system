pub mod database;

pub use database::*;

use crate::common::{Exchange, Result, Symbol, MarketDataEvent, Order};
use crate::oms::{OrderResponse, Balance};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// 持久化配置
#[derive(Debug, Clone)]
pub struct PersistenceConfig {
    pub database: DatabaseConfig,
    pub enable_market_data_storage: bool,
    pub market_data_retention_days: u32,
    pub enable_strategy_logging: bool,
    pub batch_size: u32,
    pub flush_interval_ms: u64,
}

impl Default for PersistenceConfig {
    fn default() -> Self {
        Self {
            database: DatabaseConfig::default(),
            enable_market_data_storage: true,
            market_data_retention_days: 30,
            enable_strategy_logging: true,
            batch_size: 100,
            flush_interval_ms: 1000,
        }
    }
}

/// 数据持久化接口（保持向后兼容）
#[async_trait]
pub trait DataPersistence: Send + Sync {
    /// 保存市场数据
    async fn save_market_data(&self, event: &MarketDataEvent) -> Result<()>;
    
    /// 保存订单数据
    async fn save_order(&self, order: &Order) -> Result<()>;
    
    /// 查询历史数据
    async fn query_historical_data(&self, query: &str) -> Result<Vec<MarketDataEvent>>;
}

/// 持久化管理器
pub struct PersistenceManager {
    config: PersistenceConfig,
    database: Box<dyn PersistenceInterface>,
    batch_buffer: tokio::sync::Mutex<BatchBuffer>,
}

/// 批处理缓冲区
#[derive(Debug, Default)]
struct BatchBuffer {
    orders: Vec<OrderResponse>,
    balances: Vec<(Balance, Exchange)>,
    trades: Vec<TradeRecord>,
    market_data: Vec<MarketDataRecord>,
    strategy_executions: Vec<StrategyExecutionRecord>,
}

impl PersistenceManager {
    pub fn new(config: PersistenceConfig) -> Self {
        let database = Box::new(PostgresPersistence::new(config.database.clone()));
        
        Self {
            config,
            database,
            batch_buffer: tokio::sync::Mutex::new(BatchBuffer::default()),
        }
    }
    
    pub fn new_with_memory() -> Self {
        let config = PersistenceConfig::default();
        let database = Box::new(MemoryPersistence::new());
        
        Self {
            config,
            database,
            batch_buffer: tokio::sync::Mutex::new(BatchBuffer::default()),
        }
    }
    
    /// 初始化持久化管理器
    pub async fn initialize(&mut self) -> Result<()> {
        self.database.initialize().await?;
        
        // 启动批处理任务
        self.start_batch_processor().await;
        
        log::info!("Persistence manager initialized");
        Ok(())
    }
    
    /// 关闭持久化管理器
    pub async fn close(&mut self) -> Result<()> {
        // 刷新所有缓冲的数据
        self.flush_all().await?;
        
        // 关闭数据库连接
        self.database.close().await?;
        
        log::info!("Persistence manager closed");
        Ok(())
    }
    
    /// 健康检查
    pub async fn health_check(&self) -> Result<bool> {
        self.database.health_check().await
    }
    
    /// 保存订单（批处理）
    pub async fn save_order_batch(&self, order: OrderResponse) -> Result<()> {
        let mut buffer = self.batch_buffer.lock().await;
        buffer.orders.push(order);
        
        if buffer.orders.len() >= self.config.batch_size as usize {
            self.flush_orders(&mut buffer).await?;
        }
        
        Ok(())
    }
    
    /// 保存订单（立即）
    pub async fn save_order_immediate(&self, order: &OrderResponse) -> Result<Uuid> {
        self.database.save_order(order).await
    }
    
    /// 更新订单
    pub async fn update_order(&self, order: &OrderResponse) -> Result<()> {
        self.database.update_order(order).await
    }
    
    /// 获取订单
    pub async fn get_order(&self, id: Uuid) -> Result<Option<OrderRecord>> {
        self.database.get_order(id).await
    }
    
    /// 根据交易所订单ID获取订单
    pub async fn get_order_by_exchange_id(&self, exchange_order_id: &str, exchange: Exchange) -> Result<Option<OrderRecord>> {
        self.database.get_order_by_exchange_id(exchange_order_id, exchange).await
    }
    
    /// 保存余额（批处理）
    pub async fn save_balance_batch(&self, balance: Balance, exchange: Exchange) -> Result<()> {
        let mut buffer = self.batch_buffer.lock().await;
        buffer.balances.push((balance, exchange));
        
        if buffer.balances.len() >= self.config.batch_size as usize {
            self.flush_balances(&mut buffer).await?;
        }
        
        Ok(())
    }
    
    /// 保存余额（立即）
    pub async fn save_balance_immediate(&self, balance: &Balance, exchange: Exchange) -> Result<Uuid> {
        self.database.save_balance(balance, exchange).await
    }
    
    /// 获取余额
    pub async fn get_balance(&self, asset: &str, exchange: Exchange) -> Result<Option<BalanceRecord>> {
        self.database.get_balance(asset, exchange).await
    }
    
    /// 获取所有余额
    pub async fn get_all_balances(&self, exchange: Exchange) -> Result<Vec<BalanceRecord>> {
        self.database.get_all_balances(exchange).await
    }
    
    /// 保存交易记录
    pub async fn save_trade(&self, trade: TradeRecord) -> Result<()> {
        let mut buffer = self.batch_buffer.lock().await;
        buffer.trades.push(trade);
        
        if buffer.trades.len() >= self.config.batch_size as usize {
            self.flush_trades(&mut buffer).await?;
        }
        
        Ok(())
    }
    
    /// 保存市场数据
    pub async fn save_market_data(&self, data: MarketDataRecord) -> Result<()> {
        if !self.config.enable_market_data_storage {
            return Ok(());
        }
        
        let mut buffer = self.batch_buffer.lock().await;
        buffer.market_data.push(data);
        
        if buffer.market_data.len() >= self.config.batch_size as usize {
            self.flush_market_data(&mut buffer).await?;
        }
        
        Ok(())
    }
    
    /// 保存策略执行记录
    pub async fn save_strategy_execution(&self, execution: StrategyExecutionRecord) -> Result<()> {
        if !self.config.enable_strategy_logging {
            return Ok(());
        }
        
        let mut buffer = self.batch_buffer.lock().await;
        buffer.strategy_executions.push(execution);
        
        if buffer.strategy_executions.len() >= self.config.batch_size as usize {
            self.flush_strategy_executions(&mut buffer).await?;
        }
        
        Ok(())
    }
    
    /// 获取订单统计
    pub async fn get_order_statistics(
        &self,
        symbol: Option<&Symbol>,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<OrderStatistics> {
        self.database.get_order_statistics(symbol, start_time, end_time).await
    }
    
    /// 获取交易性能
    pub async fn get_trading_performance(
        &self,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<TradingPerformance> {
        self.database.get_trading_performance(start_time, end_time).await
    }
    
    /// 清理旧的市场数据
    pub async fn cleanup_old_market_data(&self) -> Result<u64> {
        let cutoff_time = Utc::now() - chrono::Duration::days(self.config.market_data_retention_days as i64);
        self.database.cleanup_old_market_data(cutoff_time).await
    }
    
    /// 刷新所有缓冲的数据
    pub async fn flush_all(&self) -> Result<()> {
        let mut buffer = self.batch_buffer.lock().await;
        
        self.flush_orders(&mut buffer).await?;
        self.flush_balances(&mut buffer).await?;
        self.flush_trades(&mut buffer).await?;
        self.flush_market_data(&mut buffer).await?;
        self.flush_strategy_executions(&mut buffer).await?;
        
        Ok(())
    }
    
    /// 启动批处理任务
    async fn start_batch_processor(&self) {
        let flush_interval = std::time::Duration::from_millis(self.config.flush_interval_ms);
        
        // 这里应该启动一个后台任务来定期刷新缓冲区
        // 为了简化，这里只是记录日志
        log::info!("Batch processor would start with interval: {:?}", flush_interval);
    }
    
    /// 刷新订单缓冲区
    async fn flush_orders(&self, buffer: &mut BatchBuffer) -> Result<()> {
        if buffer.orders.is_empty() {
            return Ok(());
        }
        
        for order in buffer.orders.drain(..) {
            if let Err(e) = self.database.save_order(&order).await {
                log::error!("Failed to save order: {}", e);
            }
        }
        
        Ok(())
    }
    
    /// 刷新余额缓冲区
    async fn flush_balances(&self, buffer: &mut BatchBuffer) -> Result<()> {
        if buffer.balances.is_empty() {
            return Ok(());
        }
        
        for (balance, exchange) in buffer.balances.drain(..) {
            if let Err(e) = self.database.save_balance(&balance, exchange).await {
                log::error!("Failed to save balance: {}", e);
            }
        }
        
        Ok(())
    }
    
    /// 刷新交易记录缓冲区
    async fn flush_trades(&self, buffer: &mut BatchBuffer) -> Result<()> {
        if buffer.trades.is_empty() {
            return Ok(());
        }
        
        for trade in buffer.trades.drain(..) {
            if let Err(e) = self.database.save_trade(&trade).await {
                log::error!("Failed to save trade: {}", e);
            }
        }
        
        Ok(())
    }
    
    /// 刷新市场数据缓冲区
    async fn flush_market_data(&self, buffer: &mut BatchBuffer) -> Result<()> {
        if buffer.market_data.is_empty() {
            return Ok(());
        }
        
        for data in buffer.market_data.drain(..) {
            if let Err(e) = self.database.save_market_data(&data).await {
                log::error!("Failed to save market data: {}", e);
            }
        }
        
        Ok(())
    }
    
    /// 刷新策略执行记录缓冲区
    async fn flush_strategy_executions(&self, buffer: &mut BatchBuffer) -> Result<()> {
        if buffer.strategy_executions.is_empty() {
            return Ok(());
        }
        
        for execution in buffer.strategy_executions.drain(..) {
            if let Err(e) = self.database.save_strategy_execution(&execution).await {
                log::error!("Failed to save strategy execution: {}", e);
            }
        }
        
        Ok(())
    }
}

/// 简单的内存持久化实现（保持向后兼容）
pub struct MemoryPersistence {
    orders: tokio::sync::RwLock<HashMap<Uuid, OrderRecord>>,
    balances: tokio::sync::RwLock<HashMap<String, BalanceRecord>>,
    trades: tokio::sync::RwLock<Vec<TradeRecord>>,
    market_data: tokio::sync::RwLock<Vec<MarketDataRecord>>,
    strategy_executions: tokio::sync::RwLock<Vec<StrategyExecutionRecord>>,
    legacy_market_data: tokio::sync::RwLock<Vec<MarketDataEvent>>,
    legacy_orders: tokio::sync::RwLock<Vec<Order>>,
}

impl MemoryPersistence {
    pub fn new() -> Self {
        Self {
            orders: tokio::sync::RwLock::new(HashMap::new()),
            balances: tokio::sync::RwLock::new(HashMap::new()),
            trades: tokio::sync::RwLock::new(Vec::new()),
            market_data: tokio::sync::RwLock::new(Vec::new()),
            strategy_executions: tokio::sync::RwLock::new(Vec::new()),
            legacy_market_data: tokio::sync::RwLock::new(Vec::new()),
            legacy_orders: tokio::sync::RwLock::new(Vec::new()),
        }
    }
    
    fn balance_key(asset: &str, exchange: Exchange) -> String {
        format!("{}:{:?}", asset, exchange)
    }
}

impl Default for MemoryPersistence {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DataPersistence for MemoryPersistence {
    async fn save_market_data(&self, event: &MarketDataEvent) -> Result<()> {
        let mut market_data = self.legacy_market_data.write().await;
        market_data.push(event.clone());
        log::debug!("Saving market data: {:?}", event);
        Ok(())
    }
    
    async fn save_order(&self, order: &Order) -> Result<()> {
        let mut orders = self.legacy_orders.write().await;
        orders.push(order.clone());
        log::debug!("Saving order: {:?}", order);
        Ok(())
    }
    
    async fn query_historical_data(&self, query: &str) -> Result<Vec<MarketDataEvent>> {
        log::debug!("Querying historical data: {}", query);
        let market_data = self.legacy_market_data.read().await;
        Ok(market_data.clone())
    }
}

#[async_trait]
impl PersistenceInterface for MemoryPersistence {
    async fn initialize(&mut self) -> Result<()> {
        log::info!("Memory persistence initialized");
        Ok(())
    }
    
    async fn close(&mut self) -> Result<()> {
        log::info!("Memory persistence closed");
        Ok(())
    }
    
    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
    
    async fn save_order(&self, order: &OrderResponse) -> Result<Uuid> {
        let id = Uuid::new_v4();
        let record = OrderRecord {
            id,
            exchange_order_id: order.order_id.clone(),
            client_order_id: order.client_order_id.clone(),
            exchange: format!("{:?}", order.symbol.exchange),
            symbol_base: order.symbol.base.clone(),
            symbol_quote: order.symbol.quote.clone(),
            side: format!("{:?}", order.side),
            order_type: format!("{:?}", order.order_type),
            original_quantity: order.original_quantity,
            executed_quantity: order.executed_quantity,
            remaining_quantity: order.remaining_quantity,
            price: order.price,
            average_price: order.average_price,
            status: format!("{:?}", order.status),
            time_in_force: order.time_in_force.map(|tif| format!("{:?}", tif)),
            created_at: order.created_at,
            updated_at: order.updated_at,
            exchange_data: serde_json::to_value(&order.exchange_data).unwrap_or_default(),
        };
        
        let mut orders = self.orders.write().await;
        orders.insert(id, record);
        
        Ok(id)
    }
    
    async fn update_order(&self, order: &OrderResponse) -> Result<()> {
        let mut orders = self.orders.write().await;
        
        // 查找现有订单
        for (_, record) in orders.iter_mut() {
            if record.exchange_order_id == order.order_id {
                record.executed_quantity = order.executed_quantity;
                record.remaining_quantity = order.remaining_quantity;
                record.average_price = order.average_price;
                record.status = format!("{:?}", order.status);
                record.updated_at = order.updated_at;
                record.exchange_data = serde_json::to_value(&order.exchange_data).unwrap_or_default();
                break;
            }
        }
        
        Ok(())
    }
    
    async fn get_order(&self, id: Uuid) -> Result<Option<OrderRecord>> {
        let orders = self.orders.read().await;
        Ok(orders.get(&id).cloned())
    }
    
    async fn get_order_by_exchange_id(&self, exchange_order_id: &str, exchange: Exchange) -> Result<Option<OrderRecord>> {
        let orders = self.orders.read().await;
        let exchange_str = format!("{:?}", exchange);
        
        for record in orders.values() {
            if record.exchange_order_id == exchange_order_id && record.exchange == exchange_str {
                return Ok(Some(record.clone()));
            }
        }
        
        Ok(None)
    }
    
    async fn get_orders_by_symbol(&self, symbol: &Symbol, limit: Option<u32>) -> Result<Vec<OrderRecord>> {
        let orders = self.orders.read().await;
        let limit = limit.unwrap_or(100) as usize;
        
        let mut result: Vec<OrderRecord> = orders
            .values()
            .filter(|record| record.symbol_base == symbol.base && record.symbol_quote == symbol.quote)
            .cloned()
            .collect();
        
        result.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        result.truncate(limit);
        
        Ok(result)
    }
    
    async fn get_orders_by_status(&self, status: &str, limit: Option<u32>) -> Result<Vec<OrderRecord>> {
        let orders = self.orders.read().await;
        let limit = limit.unwrap_or(100) as usize;
        
        let mut result: Vec<OrderRecord> = orders
            .values()
            .filter(|record| record.status == status)
            .cloned()
            .collect();
        
        result.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        result.truncate(limit);
        
        Ok(result)
    }
    
    async fn get_orders_by_time_range(
        &self,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        limit: Option<u32>,
    ) -> Result<Vec<OrderRecord>> {
        let orders = self.orders.read().await;
        let limit = limit.unwrap_or(1000) as usize;
        
        let mut result: Vec<OrderRecord> = orders
            .values()
            .filter(|record| record.created_at >= start_time && record.created_at <= end_time)
            .cloned()
            .collect();
        
        result.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        result.truncate(limit);
        
        Ok(result)
    }
    
    async fn save_balance(&self, balance: &Balance, exchange: Exchange) -> Result<Uuid> {
        let id = Uuid::new_v4();
        let key = Self::balance_key(&balance.asset, exchange);
        let record = BalanceRecord {
            id,
            exchange: format!("{:?}", exchange),
            asset: balance.asset.clone(),
            free: balance.free,
            locked: balance.locked,
            total: balance.total,
            updated_at: Utc::now(),
        };
        
        let mut balances = self.balances.write().await;
        balances.insert(key, record);
        
        Ok(id)
    }
    
    async fn update_balance(&self, balance: &Balance, exchange: Exchange) -> Result<()> {
        let key = Self::balance_key(&balance.asset, exchange);
        let mut balances = self.balances.write().await;
        
        if let Some(record) = balances.get_mut(&key) {
            record.free = balance.free;
            record.locked = balance.locked;
            record.total = balance.total;
            record.updated_at = Utc::now();
        }
        
        Ok(())
    }
    
    async fn get_balance(&self, asset: &str, exchange: Exchange) -> Result<Option<BalanceRecord>> {
        let key = Self::balance_key(asset, exchange);
        let balances = self.balances.read().await;
        Ok(balances.get(&key).cloned())
    }
    
    async fn get_all_balances(&self, exchange: Exchange) -> Result<Vec<BalanceRecord>> {
        let balances = self.balances.read().await;
        let exchange_str = format!("{:?}", exchange);
        
        let result: Vec<BalanceRecord> = balances
            .values()
            .filter(|record| record.exchange == exchange_str)
            .cloned()
            .collect();
        
        Ok(result)
    }
    
    async fn get_balance_history(
        &self,
        asset: &str,
        exchange: Exchange,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<Vec<BalanceRecord>> {
        // 内存实现不支持历史记录
        Ok(Vec::new())
    }
    
    async fn save_trade(&self, trade: &TradeRecord) -> Result<Uuid> {
        let mut trades = self.trades.write().await;
        trades.push(trade.clone());
        Ok(trade.id)
    }
    
    async fn get_trades_by_order(&self, order_id: Uuid) -> Result<Vec<TradeRecord>> {
        let trades = self.trades.read().await;
        let result: Vec<TradeRecord> = trades
            .iter()
            .filter(|trade| trade.order_id == order_id)
            .cloned()
            .collect();
        Ok(result)
    }
    
    async fn get_trades_by_symbol(&self, symbol: &Symbol, limit: Option<u32>) -> Result<Vec<TradeRecord>> {
        let trades = self.trades.read().await;
        let limit = limit.unwrap_or(100) as usize;
        
        let mut result: Vec<TradeRecord> = trades
            .iter()
            .filter(|trade| trade.symbol_base == symbol.base && trade.symbol_quote == symbol.quote)
            .cloned()
            .collect();
        
        result.sort_by(|a, b| b.executed_at.cmp(&a.executed_at));
        result.truncate(limit);
        
        Ok(result)
    }
    
    async fn get_trades_by_time_range(
        &self,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        limit: Option<u32>,
    ) -> Result<Vec<TradeRecord>> {
        let trades = self.trades.read().await;
        let limit = limit.unwrap_or(1000) as usize;
        
        let mut result: Vec<TradeRecord> = trades
            .iter()
            .filter(|trade| trade.executed_at >= start_time && trade.executed_at <= end_time)
            .cloned()
            .collect();
        
        result.sort_by(|a, b| b.executed_at.cmp(&a.executed_at));
        result.truncate(limit);
        
        Ok(result)
    }
    
    async fn save_market_data(&self, data: &MarketDataRecord) -> Result<Uuid> {
        let mut market_data = self.market_data.write().await;
        market_data.push(data.clone());
        Ok(data.id)
    }
    
    async fn get_market_data(
        &self,
        symbol: &Symbol,
        data_type: &str,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        limit: Option<u32>,
    ) -> Result<Vec<MarketDataRecord>> {
        let market_data = self.market_data.read().await;
        let limit = limit.unwrap_or(1000) as usize;
        
        let mut result: Vec<MarketDataRecord> = market_data
            .iter()
            .filter(|data| {
                data.symbol_base == symbol.base
                    && data.symbol_quote == symbol.quote
                    && data.data_type == data_type
                    && data.timestamp >= start_time
                    && data.timestamp <= end_time
            })
            .cloned()
            .collect();
        
        result.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        result.truncate(limit);
        
        Ok(result)
    }
    
    async fn cleanup_old_market_data(&self, before: DateTime<Utc>) -> Result<u64> {
        let mut market_data = self.market_data.write().await;
        let original_len = market_data.len();
        
        market_data.retain(|data| data.timestamp >= before);
        
        Ok((original_len - market_data.len()) as u64)
    }
    
    async fn save_strategy_execution(&self, execution: &StrategyExecutionRecord) -> Result<Uuid> {
        let mut executions = self.strategy_executions.write().await;
        executions.push(execution.clone());
        Ok(execution.id)
    }
    
    async fn get_strategy_executions(
        &self,
        strategy_name: &str,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        limit: Option<u32>,
    ) -> Result<Vec<StrategyExecutionRecord>> {
        let executions = self.strategy_executions.read().await;
        let limit = limit.unwrap_or(1000) as usize;
        
        let mut result: Vec<StrategyExecutionRecord> = executions
            .iter()
            .filter(|execution| {
                execution.strategy_name == strategy_name
                    && execution.executed_at >= start_time
                    && execution.executed_at <= end_time
            })
            .cloned()
            .collect();
        
        result.sort_by(|a, b| b.executed_at.cmp(&a.executed_at));
        result.truncate(limit);
        
        Ok(result)
    }
    
    async fn get_order_statistics(
        &self,
        symbol: Option<&Symbol>,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<OrderStatistics> {
        let orders = self.orders.read().await;
        
        let filtered_orders: Vec<&OrderRecord> = orders
            .values()
            .filter(|order| {
                let time_match = order.created_at >= start_time && order.created_at <= end_time;
                let symbol_match = symbol.map_or(true, |s| {
                    order.symbol_base == s.base && order.symbol_quote == s.quote
                });
                time_match && symbol_match
            })
            .collect();
        
        let total_orders = filtered_orders.len() as i64;
        let filled_orders = filtered_orders.iter().filter(|o| o.status == "Filled").count() as i64;
        let canceled_orders = filtered_orders.iter().filter(|o| o.status == "Canceled").count() as i64;
        let rejected_orders = filtered_orders.iter().filter(|o| o.status == "Rejected").count() as i64;
        
        let total_volume: Decimal = filtered_orders
            .iter()
            .map(|o| o.executed_quantity)
            .sum();
        
        let average_fill_rate = if total_orders > 0 {
            Decimal::from(filled_orders) / Decimal::from(total_orders)
        } else {
            Decimal::ZERO
        };
        
        Ok(OrderStatistics {
            total_orders,
            filled_orders,
            canceled_orders,
            rejected_orders,
            total_volume,
            average_fill_rate,
            average_execution_time: 0.0,
        })
    }
    
    async fn get_trading_performance(
        &self,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<TradingPerformance> {
        let trades = self.trades.read().await;
        
        let filtered_trades: Vec<&TradeRecord> = trades
            .iter()
            .filter(|trade| trade.executed_at >= start_time && trade.executed_at <= end_time)
            .collect();
        
        let total_trades = filtered_trades.len() as i64;
        let total_volume: Decimal = filtered_trades.iter().map(|t| t.quantity).sum();
        let total_fees: Decimal = filtered_trades.iter().map(|t| t.fee).sum();
        
        let average_trade_size = if total_trades > 0 {
            total_volume / Decimal::from(total_trades)
        } else {
            Decimal::ZERO
        };
        
        Ok(TradingPerformance {
            total_trades,
            total_volume,
            total_fees,
            realized_pnl: Decimal::ZERO,
            win_rate: Decimal::ZERO,
            average_trade_size,
            sharpe_ratio: None,
            max_drawdown: None,
        })
    }
}

