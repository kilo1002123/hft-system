use crate::common::{Exchange, Result, Symbol, HftError};
use crate::oms::{OrderResponse, Balance};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Postgres, Row, FromRow};
use std::collections::HashMap;
use uuid::Uuid;

/// 数据库配置
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
    pub min_connections: u32,
    pub connect_timeout: std::time::Duration,
    pub idle_timeout: Option<std::time::Duration>,
    pub max_lifetime: Option<std::time::Duration>,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: "postgresql://postgres:password@localhost:5432/hft_system".to_string(),
            max_connections: 10,
            min_connections: 1,
            connect_timeout: std::time::Duration::from_secs(30),
            idle_timeout: Some(std::time::Duration::from_secs(600)),
            max_lifetime: Some(std::time::Duration::from_secs(1800)),
        }
    }
}

/// 订单记录
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct OrderRecord {
    pub id: Uuid,
    pub exchange_order_id: String,
    pub client_order_id: Option<String>,
    pub exchange: String,
    pub symbol_base: String,
    pub symbol_quote: String,
    pub side: String,
    pub order_type: String,
    pub original_quantity: Decimal,
    pub executed_quantity: Decimal,
    pub remaining_quantity: Decimal,
    pub price: Option<Decimal>,
    pub average_price: Option<Decimal>,
    pub status: String,
    pub time_in_force: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub exchange_data: serde_json::Value,
}

/// 余额记录
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BalanceRecord {
    pub id: Uuid,
    pub exchange: String,
    pub asset: String,
    pub free: Decimal,
    pub locked: Decimal,
    pub total: Decimal,
    pub updated_at: DateTime<Utc>,
}

/// 交易记录
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TradeRecord {
    pub id: Uuid,
    pub exchange_trade_id: String,
    pub order_id: Uuid,
    pub exchange_order_id: String,
    pub exchange: String,
    pub symbol_base: String,
    pub symbol_quote: String,
    pub side: String,
    pub quantity: Decimal,
    pub price: Decimal,
    pub fee: Decimal,
    pub fee_asset: String,
    pub is_maker: bool,
    pub executed_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// 市场数据记录
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MarketDataRecord {
    pub id: Uuid,
    pub exchange: String,
    pub symbol_base: String,
    pub symbol_quote: String,
    pub data_type: String, // ticker, orderbook, trade, kline
    pub data: serde_json::Value,
    pub timestamp: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// 策略执行记录
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct StrategyExecutionRecord {
    pub id: Uuid,
    pub strategy_name: String,
    pub symbol_base: String,
    pub symbol_quote: String,
    pub action: String, // buy, sell, hold
    pub quantity: Option<Decimal>,
    pub price: Option<Decimal>,
    pub reason: String,
    pub metadata: serde_json::Value,
    pub executed_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// 持久化接口
#[async_trait]
pub trait PersistenceInterface: Send + Sync {
    /// 初始化数据库连接
    async fn initialize(&mut self) -> Result<()>;
    
    /// 关闭数据库连接
    async fn close(&mut self) -> Result<()>;
    
    /// 健康检查
    async fn health_check(&self) -> Result<bool>;
    
    // 订单相关操作
    async fn save_order(&self, order: &OrderResponse) -> Result<Uuid>;
    async fn update_order(&self, order: &OrderResponse) -> Result<()>;
    async fn get_order(&self, id: Uuid) -> Result<Option<OrderRecord>>;
    async fn get_order_by_exchange_id(&self, exchange_order_id: &str, exchange: Exchange) -> Result<Option<OrderRecord>>;
    async fn get_orders_by_symbol(&self, symbol: &Symbol, limit: Option<u32>) -> Result<Vec<OrderRecord>>;
    async fn get_orders_by_status(&self, status: &str, limit: Option<u32>) -> Result<Vec<OrderRecord>>;
    async fn get_orders_by_time_range(
        &self,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        limit: Option<u32>,
    ) -> Result<Vec<OrderRecord>>;
    
    // 余额相关操作
    async fn save_balance(&self, balance: &Balance, exchange: Exchange) -> Result<Uuid>;
    async fn update_balance(&self, balance: &Balance, exchange: Exchange) -> Result<()>;
    async fn get_balance(&self, asset: &str, exchange: Exchange) -> Result<Option<BalanceRecord>>;
    async fn get_all_balances(&self, exchange: Exchange) -> Result<Vec<BalanceRecord>>;
    async fn get_balance_history(
        &self,
        asset: &str,
        exchange: Exchange,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<Vec<BalanceRecord>>;
    
    // 交易记录操作
    async fn save_trade(&self, trade: &TradeRecord) -> Result<Uuid>;
    async fn get_trades_by_order(&self, order_id: Uuid) -> Result<Vec<TradeRecord>>;
    async fn get_trades_by_symbol(&self, symbol: &Symbol, limit: Option<u32>) -> Result<Vec<TradeRecord>>;
    async fn get_trades_by_time_range(
        &self,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        limit: Option<u32>,
    ) -> Result<Vec<TradeRecord>>;
    
    // 市场数据操作
    async fn save_market_data(&self, data: &MarketDataRecord) -> Result<Uuid>;
    async fn get_market_data(
        &self,
        symbol: &Symbol,
        data_type: &str,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        limit: Option<u32>,
    ) -> Result<Vec<MarketDataRecord>>;
    async fn cleanup_old_market_data(&self, before: DateTime<Utc>) -> Result<u64>;
    
    // 策略执行记录操作
    async fn save_strategy_execution(&self, execution: &StrategyExecutionRecord) -> Result<Uuid>;
    async fn get_strategy_executions(
        &self,
        strategy_name: &str,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        limit: Option<u32>,
    ) -> Result<Vec<StrategyExecutionRecord>>;
    
    // 统计和分析
    async fn get_order_statistics(
        &self,
        symbol: Option<&Symbol>,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<OrderStatistics>;
    async fn get_trading_performance(
        &self,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<TradingPerformance>;
}

/// 订单统计
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderStatistics {
    pub total_orders: i64,
    pub filled_orders: i64,
    pub canceled_orders: i64,
    pub rejected_orders: i64,
    pub total_volume: Decimal,
    pub average_fill_rate: Decimal,
    pub average_execution_time: f64, // seconds
}

/// 交易性能
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingPerformance {
    pub total_trades: i64,
    pub total_volume: Decimal,
    pub total_fees: Decimal,
    pub realized_pnl: Decimal,
    pub win_rate: Decimal,
    pub average_trade_size: Decimal,
    pub sharpe_ratio: Option<Decimal>,
    pub max_drawdown: Option<Decimal>,
}

/// PostgreSQL 持久化实现
pub struct PostgresPersistence {
    pool: Option<Pool<Postgres>>,
    config: DatabaseConfig,
}

impl PostgresPersistence {
    pub fn new(config: DatabaseConfig) -> Self {
        Self {
            pool: None,
            config,
        }
    }
    
    /// 获取数据库连接池
    fn pool(&self) -> Result<&Pool<Postgres>> {
        self.pool.as_ref().ok_or_else(|| {
            HftError::Other("Database not initialized".to_string())
        })
    }
    
    /// 创建数据库表
    async fn create_tables(&self) -> Result<()> {
        let pool = self.pool()?;
        
        // 创建订单表
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS orders (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                exchange_order_id VARCHAR NOT NULL,
                client_order_id VARCHAR,
                exchange VARCHAR NOT NULL,
                symbol_base VARCHAR NOT NULL,
                symbol_quote VARCHAR NOT NULL,
                side VARCHAR NOT NULL,
                order_type VARCHAR NOT NULL,
                original_quantity DECIMAL NOT NULL,
                executed_quantity DECIMAL NOT NULL DEFAULT 0,
                remaining_quantity DECIMAL NOT NULL,
                price DECIMAL,
                average_price DECIMAL,
                status VARCHAR NOT NULL,
                time_in_force VARCHAR,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                exchange_data JSONB DEFAULT '{}'::jsonb,
                UNIQUE(exchange_order_id, exchange)
            )
        "#)
        .execute(pool)
        .await
        .map_err(|e| HftError::Other(format!("Failed to create orders table: {}", e)))?;
        
        // 创建余额表
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS balances (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                exchange VARCHAR NOT NULL,
                asset VARCHAR NOT NULL,
                free DECIMAL NOT NULL DEFAULT 0,
                locked DECIMAL NOT NULL DEFAULT 0,
                total DECIMAL NOT NULL DEFAULT 0,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                UNIQUE(exchange, asset)
            )
        "#)
        .execute(pool)
        .await
        .map_err(|e| HftError::Other(format!("Failed to create balances table: {}", e)))?;
        
        // 创建交易记录表
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS trades (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                exchange_trade_id VARCHAR NOT NULL,
                order_id UUID NOT NULL,
                exchange_order_id VARCHAR NOT NULL,
                exchange VARCHAR NOT NULL,
                symbol_base VARCHAR NOT NULL,
                symbol_quote VARCHAR NOT NULL,
                side VARCHAR NOT NULL,
                quantity DECIMAL NOT NULL,
                price DECIMAL NOT NULL,
                fee DECIMAL NOT NULL DEFAULT 0,
                fee_asset VARCHAR NOT NULL,
                is_maker BOOLEAN NOT NULL DEFAULT false,
                executed_at TIMESTAMPTZ NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                UNIQUE(exchange_trade_id, exchange)
            )
        "#)
        .execute(pool)
        .await
        .map_err(|e| HftError::Other(format!("Failed to create trades table: {}", e)))?;
        
        // 创建市场数据表
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS market_data (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                exchange VARCHAR NOT NULL,
                symbol_base VARCHAR NOT NULL,
                symbol_quote VARCHAR NOT NULL,
                data_type VARCHAR NOT NULL,
                data JSONB NOT NULL,
                timestamp TIMESTAMPTZ NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
        "#)
        .execute(pool)
        .await
        .map_err(|e| HftError::Other(format!("Failed to create market_data table: {}", e)))?;
        
        // 创建策略执行记录表
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS strategy_executions (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                strategy_name VARCHAR NOT NULL,
                symbol_base VARCHAR NOT NULL,
                symbol_quote VARCHAR NOT NULL,
                action VARCHAR NOT NULL,
                quantity DECIMAL,
                price DECIMAL,
                reason TEXT NOT NULL,
                metadata JSONB DEFAULT '{}'::jsonb,
                executed_at TIMESTAMPTZ NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
        "#)
        .execute(pool)
        .await
        .map_err(|e| HftError::Other(format!("Failed to create strategy_executions table: {}", e)))?;
        
        // 创建索引
        self.create_indexes().await?;
        
        Ok(())
    }
    
    /// 创建数据库索引
    async fn create_indexes(&self) -> Result<()> {
        let pool = self.pool()?;
        
        let indexes = vec![
            "CREATE INDEX IF NOT EXISTS idx_orders_exchange_symbol ON orders(exchange, symbol_base, symbol_quote)",
            "CREATE INDEX IF NOT EXISTS idx_orders_status ON orders(status)",
            "CREATE INDEX IF NOT EXISTS idx_orders_created_at ON orders(created_at)",
            "CREATE INDEX IF NOT EXISTS idx_orders_exchange_order_id ON orders(exchange_order_id)",
            
            "CREATE INDEX IF NOT EXISTS idx_balances_exchange_asset ON balances(exchange, asset)",
            "CREATE INDEX IF NOT EXISTS idx_balances_updated_at ON balances(updated_at)",
            
            "CREATE INDEX IF NOT EXISTS idx_trades_order_id ON trades(order_id)",
            "CREATE INDEX IF NOT EXISTS idx_trades_exchange_symbol ON trades(exchange, symbol_base, symbol_quote)",
            "CREATE INDEX IF NOT EXISTS idx_trades_executed_at ON trades(executed_at)",
            
            "CREATE INDEX IF NOT EXISTS idx_market_data_exchange_symbol_type ON market_data(exchange, symbol_base, symbol_quote, data_type)",
            "CREATE INDEX IF NOT EXISTS idx_market_data_timestamp ON market_data(timestamp)",
            
            "CREATE INDEX IF NOT EXISTS idx_strategy_executions_name ON strategy_executions(strategy_name)",
            "CREATE INDEX IF NOT EXISTS idx_strategy_executions_executed_at ON strategy_executions(executed_at)",
        ];
        
        for index_sql in indexes {
            sqlx::query(index_sql)
                .execute(pool)
                .await
                .map_err(|e| HftError::Other(format!("Failed to create index: {}", e)))?;
        }
        
        Ok(())
    }
    
    /// 转换订单响应为订单记录
    fn order_response_to_record(&self, order: &OrderResponse) -> OrderRecord {
        OrderRecord {
            id: Uuid::new_v4(),
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
        }
    }
    
    /// 转换余额为余额记录
    fn balance_to_record(&self, balance: &Balance, exchange: Exchange) -> BalanceRecord {
        BalanceRecord {
            id: Uuid::new_v4(),
            exchange: format!("{:?}", exchange),
            asset: balance.asset.clone(),
            free: balance.free,
            locked: balance.locked,
            total: balance.total,
            updated_at: Utc::now(),
        }
    }
}

#[async_trait]
impl PersistenceInterface for PostgresPersistence {
    async fn initialize(&mut self) -> Result<()> {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(self.config.max_connections)
            .min_connections(self.config.min_connections)
            .acquire_timeout(self.config.connect_timeout)
            .idle_timeout(self.config.idle_timeout)
            .max_lifetime(self.config.max_lifetime)
            .connect(&self.config.url)
            .await
            .map_err(|e| HftError::Other(format!("Failed to connect to database: {}", e)))?;
        
        self.pool = Some(pool);
        self.create_tables().await?;
        
        log::info!("Database initialized successfully");
        Ok(())
    }
    
    async fn close(&mut self) -> Result<()> {
        if let Some(pool) = self.pool.take() {
            pool.close().await;
            log::info!("Database connection closed");
        }
        Ok(())
    }
    
    async fn health_check(&self) -> Result<bool> {
        let pool = self.pool()?;
        
        match sqlx::query("SELECT 1").fetch_one(pool).await {
            Ok(_) => Ok(true),
            Err(e) => {
                log::error!("Database health check failed: {}", e);
                Ok(false)
            }
        }
    }
    
    async fn save_order(&self, order: &OrderResponse) -> Result<Uuid> {
        let pool = self.pool()?;
        let record = self.order_response_to_record(order);
        
        let row = sqlx::query(r#"
            INSERT INTO orders (
                id, exchange_order_id, client_order_id, exchange, symbol_base, symbol_quote,
                side, order_type, original_quantity, executed_quantity, remaining_quantity,
                price, average_price, status, time_in_force, created_at, updated_at, exchange_data
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
            ON CONFLICT (exchange_order_id, exchange) 
            DO UPDATE SET
                executed_quantity = EXCLUDED.executed_quantity,
                remaining_quantity = EXCLUDED.remaining_quantity,
                average_price = EXCLUDED.average_price,
                status = EXCLUDED.status,
                updated_at = EXCLUDED.updated_at,
                exchange_data = EXCLUDED.exchange_data
            RETURNING id
        "#)
        .bind(record.id)
        .bind(&record.exchange_order_id)
        .bind(&record.client_order_id)
        .bind(&record.exchange)
        .bind(&record.symbol_base)
        .bind(&record.symbol_quote)
        .bind(&record.side)
        .bind(&record.order_type)
        .bind(record.original_quantity)
        .bind(record.executed_quantity)
        .bind(record.remaining_quantity)
        .bind(record.price)
        .bind(record.average_price)
        .bind(&record.status)
        .bind(&record.time_in_force)
        .bind(record.created_at)
        .bind(record.updated_at)
        .bind(&record.exchange_data)
        .fetch_one(pool)
        .await
        .map_err(|e| HftError::Other(format!("Failed to save order: {}", e)))?;
        
        Ok(row.get("id"))
    }
    
    async fn update_order(&self, order: &OrderResponse) -> Result<()> {
        let pool = self.pool()?;
        
        sqlx::query(r#"
            UPDATE orders SET
                executed_quantity = $1,
                remaining_quantity = $2,
                average_price = $3,
                status = $4,
                updated_at = $5,
                exchange_data = $6
            WHERE exchange_order_id = $7 AND exchange = $8
        "#)
        .bind(order.executed_quantity)
        .bind(order.remaining_quantity)
        .bind(order.average_price)
        .bind(format!("{:?}", order.status))
        .bind(order.updated_at)
        .bind(serde_json::to_value(&order.exchange_data).unwrap_or_default())
        .bind(&order.order_id)
        .bind(format!("{:?}", order.symbol.exchange))
        .execute(pool)
        .await
        .map_err(|e| HftError::Other(format!("Failed to update order: {}", e)))?;
        
        Ok(())
    }
    
    async fn get_order(&self, id: Uuid) -> Result<Option<OrderRecord>> {
        let pool = self.pool()?;
        
        match sqlx::query_as::<_, OrderRecord>("SELECT * FROM orders WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
        {
            Ok(order) => Ok(order),
            Err(e) => Err(HftError::Other(format!("Failed to get order: {}", e))),
        }
    }
    
    async fn get_order_by_exchange_id(&self, exchange_order_id: &str, exchange: Exchange) -> Result<Option<OrderRecord>> {
        let pool = self.pool()?;
        
        match sqlx::query_as::<_, OrderRecord>(
            "SELECT * FROM orders WHERE exchange_order_id = $1 AND exchange = $2"
        )
        .bind(exchange_order_id)
        .bind(format!("{:?}", exchange))
        .fetch_optional(pool)
        .await
        {
            Ok(order) => Ok(order),
            Err(e) => Err(HftError::Other(format!("Failed to get order by exchange id: {}", e))),
        }
    }
    
    async fn get_orders_by_symbol(&self, symbol: &Symbol, limit: Option<u32>) -> Result<Vec<OrderRecord>> {
        let pool = self.pool()?;
        let limit = limit.unwrap_or(100) as i64;
        
        match sqlx::query_as::<_, OrderRecord>(
            "SELECT * FROM orders WHERE symbol_base = $1 AND symbol_quote = $2 ORDER BY created_at DESC LIMIT $3"
        )
        .bind(&symbol.base)
        .bind(&symbol.quote)
        .bind(limit)
        .fetch_all(pool)
        .await
        {
            Ok(orders) => Ok(orders),
            Err(e) => Err(HftError::Other(format!("Failed to get orders by symbol: {}", e))),
        }
    }
    
    async fn get_orders_by_status(&self, status: &str, limit: Option<u32>) -> Result<Vec<OrderRecord>> {
        let pool = self.pool()?;
        let limit = limit.unwrap_or(100) as i64;
        
        match sqlx::query_as::<_, OrderRecord>(
            "SELECT * FROM orders WHERE status = $1 ORDER BY created_at DESC LIMIT $2"
        )
        .bind(status)
        .bind(limit)
        .fetch_all(pool)
        .await
        {
            Ok(orders) => Ok(orders),
            Err(e) => Err(HftError::Other(format!("Failed to get orders by status: {}", e))),
        }
    }
    
    async fn get_orders_by_time_range(
        &self,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        limit: Option<u32>,
    ) -> Result<Vec<OrderRecord>> {
        let pool = self.pool()?;
        let limit = limit.unwrap_or(1000) as i64;
        
        match sqlx::query_as::<_, OrderRecord>(
            "SELECT * FROM orders WHERE created_at >= $1 AND created_at <= $2 ORDER BY created_at DESC LIMIT $3"
        )
        .bind(start_time)
        .bind(end_time)
        .bind(limit)
        .fetch_all(pool)
        .await
        {
            Ok(orders) => Ok(orders),
            Err(e) => Err(HftError::Other(format!("Failed to get orders by time range: {}", e))),
        }
    }
    
    async fn save_balance(&self, balance: &Balance, exchange: Exchange) -> Result<Uuid> {
        let pool = self.pool()?;
        let record = self.balance_to_record(balance, exchange);
        
        let row = sqlx::query(r#"
            INSERT INTO balances (id, exchange, asset, free, locked, total, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (exchange, asset)
            DO UPDATE SET
                free = EXCLUDED.free,
                locked = EXCLUDED.locked,
                total = EXCLUDED.total,
                updated_at = EXCLUDED.updated_at
            RETURNING id
        "#)
        .bind(record.id)
        .bind(&record.exchange)
        .bind(&record.asset)
        .bind(record.free)
        .bind(record.locked)
        .bind(record.total)
        .bind(record.updated_at)
        .fetch_one(pool)
        .await
        .map_err(|e| HftError::Other(format!("Failed to save balance: {}", e)))?;
        
        Ok(row.get("id"))
    }
    
    async fn update_balance(&self, balance: &Balance, exchange: Exchange) -> Result<()> {
        let pool = self.pool()?;
        
        sqlx::query(r#"
            UPDATE balances SET
                free = $1,
                locked = $2,
                total = $3,
                updated_at = $4
            WHERE exchange = $5 AND asset = $6
        "#)
        .bind(balance.free)
        .bind(balance.locked)
        .bind(balance.total)
        .bind(Utc::now())
        .bind(format!("{:?}", exchange))
        .bind(&balance.asset)
        .execute(pool)
        .await
        .map_err(|e| HftError::Other(format!("Failed to update balance: {}", e)))?;
        
        Ok(())
    }
    
    async fn get_balance(&self, asset: &str, exchange: Exchange) -> Result<Option<BalanceRecord>> {
        let pool = self.pool()?;
        
        match sqlx::query_as::<_, BalanceRecord>(
            "SELECT * FROM balances WHERE asset = $1 AND exchange = $2"
        )
        .bind(asset)
        .bind(format!("{:?}", exchange))
        .fetch_optional(pool)
        .await
        {
            Ok(balance) => Ok(balance),
            Err(e) => Err(HftError::Other(format!("Failed to get balance: {}", e))),
        }
    }
    
    async fn get_all_balances(&self, exchange: Exchange) -> Result<Vec<BalanceRecord>> {
        let pool = self.pool()?;
        
        match sqlx::query_as::<_, BalanceRecord>(
            "SELECT * FROM balances WHERE exchange = $1 ORDER BY asset"
        )
        .bind(format!("{:?}", exchange))
        .fetch_all(pool)
        .await
        {
            Ok(balances) => Ok(balances),
            Err(e) => Err(HftError::Other(format!("Failed to get all balances: {}", e))),
        }
    }
    
    async fn get_balance_history(
        &self,
        asset: &str,
        exchange: Exchange,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<Vec<BalanceRecord>> {
        let pool = self.pool()?;
        
        match sqlx::query_as::<_, BalanceRecord>(
            "SELECT * FROM balances WHERE asset = $1 AND exchange = $2 AND updated_at >= $3 AND updated_at <= $4 ORDER BY updated_at"
        )
        .bind(asset)
        .bind(format!("{:?}", exchange))
        .bind(start_time)
        .bind(end_time)
        .fetch_all(pool)
        .await
        {
            Ok(balances) => Ok(balances),
            Err(e) => Err(HftError::Other(format!("Failed to get balance history: {}", e))),
        }
    }
    
    async fn save_trade(&self, trade: &TradeRecord) -> Result<Uuid> {
        let pool = self.pool()?;
        
        let row = sqlx::query(r#"
            INSERT INTO trades (
                id, exchange_trade_id, order_id, exchange_order_id, exchange,
                symbol_base, symbol_quote, side, quantity, price, fee, fee_asset,
                is_maker, executed_at, created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            ON CONFLICT (exchange_trade_id, exchange) DO NOTHING
            RETURNING id
        "#)
        .bind(trade.id)
        .bind(&trade.exchange_trade_id)
        .bind(trade.order_id)
        .bind(&trade.exchange_order_id)
        .bind(&trade.exchange)
        .bind(&trade.symbol_base)
        .bind(&trade.symbol_quote)
        .bind(&trade.side)
        .bind(trade.quantity)
        .bind(trade.price)
        .bind(trade.fee)
        .bind(&trade.fee_asset)
        .bind(trade.is_maker)
        .bind(trade.executed_at)
        .bind(trade.created_at)
        .fetch_optional(pool)
        .await
        .map_err(|e| HftError::Other(format!("Failed to save trade: {}", e)))?;
        
        match row {
            Some(row) => Ok(row.get("id")),
            None => Ok(trade.id), // Already exists
        }
    }
    
    async fn get_trades_by_order(&self, order_id: Uuid) -> Result<Vec<TradeRecord>> {
        let pool = self.pool()?;
        
        match sqlx::query_as::<_, TradeRecord>(
            "SELECT * FROM trades WHERE order_id = $1 ORDER BY executed_at"
        )
        .bind(order_id)
        .fetch_all(pool)
        .await
        {
            Ok(trades) => Ok(trades),
            Err(e) => Err(HftError::Other(format!("Failed to get trades by order: {}", e))),
        }
    }
    
    async fn get_trades_by_symbol(&self, symbol: &Symbol, limit: Option<u32>) -> Result<Vec<TradeRecord>> {
        let pool = self.pool()?;
        let limit = limit.unwrap_or(100) as i64;
        
        match sqlx::query_as::<_, TradeRecord>(
            "SELECT * FROM trades WHERE symbol_base = $1 AND symbol_quote = $2 ORDER BY executed_at DESC LIMIT $3"
        )
        .bind(&symbol.base)
        .bind(&symbol.quote)
        .bind(limit)
        .fetch_all(pool)
        .await
        {
            Ok(trades) => Ok(trades),
            Err(e) => Err(HftError::Other(format!("Failed to get trades by symbol: {}", e))),
        }
    }
    
    async fn get_trades_by_time_range(
        &self,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        limit: Option<u32>,
    ) -> Result<Vec<TradeRecord>> {
        let pool = self.pool()?;
        let limit = limit.unwrap_or(1000) as i64;
        
        match sqlx::query_as::<_, TradeRecord>(
            "SELECT * FROM trades WHERE executed_at >= $1 AND executed_at <= $2 ORDER BY executed_at DESC LIMIT $3"
        )
        .bind(start_time)
        .bind(end_time)
        .bind(limit)
        .fetch_all(pool)
        .await
        {
            Ok(trades) => Ok(trades),
            Err(e) => Err(HftError::Other(format!("Failed to get trades by time range: {}", e))),
        }
    }
    
    async fn save_market_data(&self, data: &MarketDataRecord) -> Result<Uuid> {
        let pool = self.pool()?;
        
        let row = sqlx::query(r#"
            INSERT INTO market_data (
                id, exchange, symbol_base, symbol_quote, data_type, data, timestamp, created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING id
        "#)
        .bind(data.id)
        .bind(&data.exchange)
        .bind(&data.symbol_base)
        .bind(&data.symbol_quote)
        .bind(&data.data_type)
        .bind(&data.data)
        .bind(data.timestamp)
        .bind(data.created_at)
        .fetch_one(pool)
        .await
        .map_err(|e| HftError::Other(format!("Failed to save market data: {}", e)))?;
        
        Ok(row.get("id"))
    }
    
    async fn get_market_data(
        &self,
        symbol: &Symbol,
        data_type: &str,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        limit: Option<u32>,
    ) -> Result<Vec<MarketDataRecord>> {
        let pool = self.pool()?;
        let limit = limit.unwrap_or(1000) as i64;
        
        match sqlx::query_as::<_, MarketDataRecord>(
            "SELECT * FROM market_data WHERE symbol_base = $1 AND symbol_quote = $2 AND data_type = $3 AND timestamp >= $4 AND timestamp <= $5 ORDER BY timestamp DESC LIMIT $6"
        )
        .bind(&symbol.base)
        .bind(&symbol.quote)
        .bind(data_type)
        .bind(start_time)
        .bind(end_time)
        .bind(limit)
        .fetch_all(pool)
        .await
        {
            Ok(data) => Ok(data),
            Err(e) => Err(HftError::Other(format!("Failed to get market data: {}", e))),
        }
    }
    
    async fn cleanup_old_market_data(&self, before: DateTime<Utc>) -> Result<u64> {
        let pool = self.pool()?;
        
        let result = sqlx::query("DELETE FROM market_data WHERE timestamp < $1")
            .bind(before)
            .execute(pool)
            .await
            .map_err(|e| HftError::Other(format!("Failed to cleanup old market data: {}", e)))?;
        
        Ok(result.rows_affected())
    }
    
    async fn save_strategy_execution(&self, execution: &StrategyExecutionRecord) -> Result<Uuid> {
        let pool = self.pool()?;
        
        let row = sqlx::query(r#"
            INSERT INTO strategy_executions (
                id, strategy_name, symbol_base, symbol_quote, action, quantity, price,
                reason, metadata, executed_at, created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            RETURNING id
        "#)
        .bind(execution.id)
        .bind(&execution.strategy_name)
        .bind(&execution.symbol_base)
        .bind(&execution.symbol_quote)
        .bind(&execution.action)
        .bind(execution.quantity)
        .bind(execution.price)
        .bind(&execution.reason)
        .bind(&execution.metadata)
        .bind(execution.executed_at)
        .bind(execution.created_at)
        .fetch_one(pool)
        .await
        .map_err(|e| HftError::Other(format!("Failed to save strategy execution: {}", e)))?;
        
        Ok(row.get("id"))
    }
    
    async fn get_strategy_executions(
        &self,
        strategy_name: &str,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        limit: Option<u32>,
    ) -> Result<Vec<StrategyExecutionRecord>> {
        let pool = self.pool()?;
        let limit = limit.unwrap_or(1000) as i64;
        
        match sqlx::query_as::<_, StrategyExecutionRecord>(
            "SELECT * FROM strategy_executions WHERE strategy_name = $1 AND executed_at >= $2 AND executed_at <= $3 ORDER BY executed_at DESC LIMIT $4"
        )
        .bind(strategy_name)
        .bind(start_time)
        .bind(end_time)
        .bind(limit)
        .fetch_all(pool)
        .await
        {
            Ok(executions) => Ok(executions),
            Err(e) => Err(HftError::Other(format!("Failed to get strategy executions: {}", e))),
        }
    }
    
    async fn get_order_statistics(
        &self,
        symbol: Option<&Symbol>,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<OrderStatistics> {
        let pool = self.pool()?;
        
        let (query, params): (String, Vec<Box<dyn sqlx::Encode<'_, sqlx::Postgres> + Send>>) = if let Some(symbol) = symbol {
            (
                "SELECT 
                    COUNT(*) as total_orders,
                    COUNT(CASE WHEN status = 'Filled' THEN 1 END) as filled_orders,
                    COUNT(CASE WHEN status = 'Canceled' THEN 1 END) as canceled_orders,
                    COUNT(CASE WHEN status = 'Rejected' THEN 1 END) as rejected_orders,
                    COALESCE(SUM(executed_quantity), 0) as total_volume
                FROM orders 
                WHERE symbol_base = $1 AND symbol_quote = $2 AND created_at >= $3 AND created_at <= $4".to_string(),
                vec![
                    Box::new(symbol.base.clone()),
                    Box::new(symbol.quote.clone()),
                    Box::new(start_time),
                    Box::new(end_time),
                ]
            )
        } else {
            (
                "SELECT 
                    COUNT(*) as total_orders,
                    COUNT(CASE WHEN status = 'Filled' THEN 1 END) as filled_orders,
                    COUNT(CASE WHEN status = 'Canceled' THEN 1 END) as canceled_orders,
                    COUNT(CASE WHEN status = 'Rejected' THEN 1 END) as rejected_orders,
                    COALESCE(SUM(executed_quantity), 0) as total_volume
                FROM orders 
                WHERE created_at >= $1 AND created_at <= $2".to_string(),
                vec![
                    Box::new(start_time),
                    Box::new(end_time),
                ]
            )
        };
        
        // 简化实现，实际应该使用动态查询
        let row = sqlx::query(&query)
            .bind(start_time)
            .bind(end_time)
            .fetch_one(pool)
            .await
            .map_err(|e| HftError::Other(format!("Failed to get order statistics: {}", e)))?;
        
        let total_orders: i64 = row.get("total_orders");
        let filled_orders: i64 = row.get("filled_orders");
        let canceled_orders: i64 = row.get("canceled_orders");
        let rejected_orders: i64 = row.get("rejected_orders");
        let total_volume: Decimal = row.get("total_volume");
        
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
            average_execution_time: 0.0, // 需要额外计算
        })
    }
    
    async fn get_trading_performance(
        &self,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<TradingPerformance> {
        let pool = self.pool()?;
        
        let row = sqlx::query(
            "SELECT 
                COUNT(*) as total_trades,
                COALESCE(SUM(quantity), 0) as total_volume,
                COALESCE(SUM(fee), 0) as total_fees
            FROM trades 
            WHERE executed_at >= $1 AND executed_at <= $2"
        )
        .bind(start_time)
        .bind(end_time)
        .fetch_one(pool)
        .await
        .map_err(|e| HftError::Other(format!("Failed to get trading performance: {}", e)))?;
        
        let total_trades: i64 = row.get("total_trades");
        let total_volume: Decimal = row.get("total_volume");
        let total_fees: Decimal = row.get("total_fees");
        
        let average_trade_size = if total_trades > 0 {
            total_volume / Decimal::from(total_trades)
        } else {
            Decimal::ZERO
        };
        
        Ok(TradingPerformance {
            total_trades,
            total_volume,
            total_fees,
            realized_pnl: Decimal::ZERO, // 需要额外计算
            win_rate: Decimal::ZERO, // 需要额外计算
            average_trade_size,
            sharpe_ratio: None, // 需要额外计算
            max_drawdown: None, // 需要额外计算
        })
    }
}

