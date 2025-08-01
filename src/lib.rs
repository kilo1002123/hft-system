//! # 高频交易系统 (HFT System)
//! 
//! 基于 Rust 的高性能虚拟货币高频交易系统
//! 
//! ## 核心功能
//! - 多交易所支持 (Binance, OKX, Gate)
//! - 做市策略引擎
//! - 实时风险管理
//! - 高性能数据持久化
//! 
//! ## 快速开始
//! ```rust
//! use hft_system::*;
//! 
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let system = HftSystem::new().await?;
//!     system.start().await?;
//!     Ok(())
//! }
//! ```

pub mod common;
pub mod data_ingestion;
pub mod oms;
pub mod rms;
pub mod strategy_engine;
pub mod monitoring;
pub mod persistence;
pub mod backtesting;

// 重新导出核心类型
pub use common::*;
pub use oms::{OrderRequest, OrderResponse, ExchangeAdapter};
pub use strategy_engine::{Strategy, MarketMakingStrategy};
pub use persistence::{PersistenceManager, PersistenceConfig};

/// 系统主入口
pub struct HftSystem {
    config: SystemConfig,
    oms: oms::OrderManager,
    strategy: strategy_engine::StrategyManager,
    persistence: persistence::PersistenceManager,
}

impl HftSystem {
    /// 创建新的交易系统实例
    pub async fn new() -> Result<Self> {
        let config = SystemConfig::load()?;
        let persistence = PersistenceManager::new(config.persistence.clone());
        let oms = oms::OrderManager::new(config.exchanges.clone());
        let strategy = strategy_engine::StrategyManager::new();

        Ok(Self {
            config,
            oms,
            strategy,
            persistence,
        })
    }

    /// 启动系统
    pub async fn start(&mut self) -> Result<()> {
        // 初始化持久化
        self.persistence.initialize().await?;
        
        // 启动订单管理
        self.oms.start().await?;
        
        // 启动策略引擎
        self.strategy.start().await?;
        
        log::info!("HFT System started successfully");
        Ok(())
    }

    /// 停止系统
    pub async fn stop(&mut self) -> Result<()> {
        self.strategy.stop().await?;
        self.oms.stop().await?;
        self.persistence.close().await?;
        
        log::info!("HFT System stopped");
        Ok(())
    }
}

/// 系统配置
#[derive(Debug, Clone)]
pub struct SystemConfig {
    pub log_level: String,
    pub worker_threads: usize,
    pub exchanges: std::collections::HashMap<String, ExchangeConfig>,
    pub persistence: persistence::PersistenceConfig,
    pub strategies: std::collections::HashMap<String, StrategyConfig>,
}

impl SystemConfig {
    /// 从配置文件加载
    pub fn load() -> Result<Self> {
        // 实现配置加载逻辑
        Ok(Self::default())
    }
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            log_level: "info".to_string(),
            worker_threads: 4,
            exchanges: std::collections::HashMap::new(),
            persistence: persistence::PersistenceConfig::default(),
            strategies: std::collections::HashMap::new(),
        }
    }
}

