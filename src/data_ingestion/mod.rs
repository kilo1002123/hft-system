pub mod binance;
pub mod okx;
pub mod gate;

use crate::common::{Exchange, MarketDataEvent, Result, Symbol};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc;

/// 数据接入接口
#[async_trait]
pub trait DataProvider: Send + Sync {
    /// 获取交易所名称
    fn exchange(&self) -> Exchange;
    
    /// 连接到数据源
    async fn connect(&mut self) -> Result<()>;
    
    /// 断开连接
    async fn disconnect(&mut self) -> Result<()>;
    
    /// 订阅市场数据
    async fn subscribe(&mut self, symbols: Vec<Symbol>) -> Result<()>;
    
    /// 取消订阅
    async fn unsubscribe(&mut self, symbols: Vec<Symbol>) -> Result<()>;
    
    /// 获取数据接收器
    fn get_receiver(&self) -> Option<mpsc::UnboundedReceiver<MarketDataEvent>>;
    
    /// 检查连接状态
    fn is_connected(&self) -> bool;
}

/// 数据分发器
pub struct DataDistributor {
    providers: Vec<Box<dyn DataProvider>>,
    subscribers: Vec<mpsc::UnboundedSender<MarketDataEvent>>,
}

impl DataDistributor {
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
            subscribers: Vec::new(),
        }
    }
    
    /// 添加数据提供者
    pub fn add_provider(&mut self, provider: Box<dyn DataProvider>) {
        self.providers.push(provider);
    }
    
    /// 添加订阅者
    pub fn add_subscriber(&mut self, sender: mpsc::UnboundedSender<MarketDataEvent>) {
        self.subscribers.push(sender);
    }
    
    /// 启动数据分发
    pub async fn start(&mut self) -> Result<()> {
        // 连接所有提供者
        for provider in &mut self.providers {
            provider.connect().await?;
        }
        
        // 启动数据转发任务
        for provider in &mut self.providers {
            if let Some(mut receiver) = provider.get_receiver() {
                let subscribers = self.subscribers.clone();
                tokio::spawn(async move {
                    while let Some(event) = receiver.recv().await {
                        for subscriber in &subscribers {
                            if let Err(e) = subscriber.send(event.clone()) {
                                log::error!("Failed to send market data event: {}", e);
                            }
                        }
                    }
                });
            }
        }
        
        Ok(())
    }
    
    /// 停止数据分发
    pub async fn stop(&mut self) -> Result<()> {
        for provider in &mut self.providers {
            provider.disconnect().await?;
        }
        Ok(())
    }
    
    /// 订阅符号
    pub async fn subscribe(&mut self, symbols: Vec<Symbol>) -> Result<()> {
        // 按交易所分组
        let mut exchange_symbols = std::collections::HashMap::new();
        for symbol in symbols {
            exchange_symbols
                .entry(symbol.exchange)
                .or_insert_with(Vec::new)
                .push(symbol);
        }
        
        // 为每个交易所订阅相应的符号
        for provider in &mut self.providers {
            let exchange = provider.exchange();
            if let Some(symbols) = exchange_symbols.get(&exchange) {
                provider.subscribe(symbols.clone()).await?;
            }
        }
        
        Ok(())
    }
}

impl Default for DataDistributor {
    fn default() -> Self {
        Self::new()
    }
}

/// 创建数据提供者工厂函数
pub fn create_provider(
    exchange: Exchange,
    config: &crate::common::ExchangeConfig,
) -> Result<Box<dyn DataProvider>> {
    match exchange {
        Exchange::Binance => Ok(Box::new(binance::BinanceProvider::new(config.clone())?)),
        Exchange::OKX => Ok(Box::new(okx::OkxProvider::new(config.clone())?)),
        Exchange::Gate => Ok(Box::new(gate::GateProvider::new(config.clone())?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{Exchange, ExchangeConfig};
    
    #[tokio::test]
    async fn test_data_distributor_creation() {
        let mut distributor = DataDistributor::new();
        assert_eq!(distributor.providers.len(), 0);
        assert_eq!(distributor.subscribers.len(), 0);
    }
    
    #[test]
    fn test_create_provider() {
        let config = ExchangeConfig {
            api_key: "test_key".to_string(),
            secret_key: "test_secret".to_string(),
            passphrase: None,
            sandbox: true,
            rate_limit: 1000,
        };
        
        let provider = create_provider(Exchange::Binance, &config);
        assert!(provider.is_ok());
        
        let provider = create_provider(Exchange::OKX, &config);
        assert!(provider.is_ok());
        
        let provider = create_provider(Exchange::Gate, &config);
        assert!(provider.is_ok());
    }
}

