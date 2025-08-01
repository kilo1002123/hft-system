use crate::common::{MarketDataEvent, Order, Result, Symbol};
use crate::strategy_engine::TradingStrategy;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use std::collections::HashMap;

/// 回测引擎
pub struct BacktestEngine {
    initial_balance: Decimal,
    current_balance: Decimal,
    positions: HashMap<Symbol, Decimal>,
    orders: Vec<Order>,
    trades: Vec<BacktestTrade>,
}

/// 回测交易记录
#[derive(Debug, Clone)]
pub struct BacktestTrade {
    pub symbol: Symbol,
    pub price: Decimal,
    pub quantity: Decimal,
    pub timestamp: DateTime<Utc>,
    pub pnl: Decimal,
}

/// 回测结果
#[derive(Debug)]
pub struct BacktestResult {
    pub total_return: Decimal,
    pub sharpe_ratio: Decimal,
    pub max_drawdown: Decimal,
    pub win_rate: Decimal,
    pub total_trades: usize,
}

impl BacktestEngine {
    pub fn new(initial_balance: Decimal) -> Self {
        Self {
            initial_balance,
            current_balance: initial_balance,
            positions: HashMap::new(),
            orders: Vec::new(),
            trades: Vec::new(),
        }
    }
    
    pub async fn run_backtest(
        &mut self,
        strategy: &mut dyn TradingStrategy,
        market_data: Vec<MarketDataEvent>,
    ) -> Result<BacktestResult> {
        log::info!("Starting backtest with {} market data events", market_data.len());
        
        strategy.initialize().await?;
        
        for event in market_data {
            let orders = strategy.on_market_data(event.clone()).await?;
            for order in orders {
                self.process_order(order, &event).await?;
            }
        }
        
        strategy.stop().await?;
        
        Ok(self.calculate_results())
    }
    
    async fn process_order(&mut self, order: Order, market_event: &MarketDataEvent) -> Result<()> {
        log::debug!("Processing backtest order: {:?}", order);
        
        // 简化的订单执行逻辑
        if let MarketDataEvent::Ticker(ticker) = market_event {
            if ticker.symbol == order.symbol {
                let trade = BacktestTrade {
                    symbol: order.symbol.clone(),
                    price: ticker.last_price,
                    quantity: order.quantity,
                    timestamp: ticker.timestamp,
                    pnl: Decimal::ZERO, // 简化处理
                };
                
                self.trades.push(trade);
            }
        }
        
        self.orders.push(order);
        Ok(())
    }
    
    fn calculate_results(&self) -> BacktestResult {
        let total_return = if self.initial_balance > Decimal::ZERO {
            (self.current_balance - self.initial_balance) / self.initial_balance * Decimal::new(100, 0)
        } else {
            Decimal::ZERO
        };
        
        BacktestResult {
            total_return,
            sharpe_ratio: Decimal::ZERO, // 需要更复杂的计算
            max_drawdown: Decimal::ZERO, // 需要更复杂的计算
            win_rate: Decimal::ZERO, // 需要更复杂的计算
            total_trades: self.trades.len(),
        }
    }
    
    pub fn get_current_balance(&self) -> Decimal {
        self.current_balance
    }
    
    pub fn get_positions(&self) -> &HashMap<Symbol, Decimal> {
        &self.positions
    }
    
    pub fn get_trades(&self) -> &[BacktestTrade] {
        &self.trades
    }
}

