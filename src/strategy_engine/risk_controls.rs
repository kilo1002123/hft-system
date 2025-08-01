use crate::common::{HftError, Result, Side, Symbol};
use crate::oms::{Order, OrderId};
use crate::rms::Position;
use crate::strategy_engine::market_making::Quote;
use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 风险控制配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskControlConfig {
    /// 最大持仓限制
    pub max_position_limit: Decimal,
    /// 最大日内损失限制
    pub max_daily_loss: Decimal,
    /// 最大单笔订单大小
    pub max_order_size: Decimal,
    /// 最大价格偏离 (基点)
    pub max_price_deviation_bps: u32,
    /// 最大价差 (基点)
    pub max_spread_bps: u32,
    /// 最小价差 (基点)
    pub min_spread_bps: u32,
    /// 最大订单数量
    pub max_order_count: u32,
    /// 最大成交频率 (每秒)
    pub max_trade_frequency: u32,
    /// 波动率限制
    pub max_volatility: Decimal,
    /// 流动性检查开关
    pub liquidity_check_enabled: bool,
    /// 最小流动性要求
    pub min_liquidity_requirement: Decimal,
    /// 紧急停止开关
    pub emergency_stop_enabled: bool,
}

impl Default for RiskControlConfig {
    fn default() -> Self {
        Self {
            max_position_limit: Decimal::new(10, 0), // 10 units
            max_daily_loss: Decimal::new(1000, 0),   // 1000 USDT
            max_order_size: Decimal::new(1, 0),      // 1 unit
            max_price_deviation_bps: 100,            // 1%
            max_spread_bps: 100,                     // 1%
            min_spread_bps: 1,                       // 0.01%
            max_order_count: 20,
            max_trade_frequency: 10,
            max_volatility: Decimal::new(50, 2),     // 50%
            liquidity_check_enabled: true,
            min_liquidity_requirement: Decimal::new(1000, 0), // 1000 USDT
            emergency_stop_enabled: true,
        }
    }
}

/// 风险检查结果
#[derive(Debug, Clone)]
pub struct RiskCheckResult {
    /// 是否通过检查
    pub passed: bool,
    /// 风险级别
    pub risk_level: RiskLevel,
    /// 检查消息
    pub messages: Vec<String>,
    /// 建议操作
    pub suggested_actions: Vec<RiskAction>,
}

/// 风险级别
#[derive(Debug, Clone, PartialEq)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

/// 风险操作建议
#[derive(Debug, Clone)]
pub enum RiskAction {
    /// 继续正常操作
    Continue,
    /// 减少订单大小
    ReduceOrderSize(Decimal),
    /// 减少持仓
    ReducePosition(Decimal),
    /// 暂停交易
    PauseTrading,
    /// 紧急停止
    EmergencyStop,
    /// 取消所有订单
    CancelAllOrders,
}

/// 交易统计
#[derive(Debug, Clone, Default)]
pub struct TradingStatistics {
    /// 今日交易次数
    pub daily_trade_count: u32,
    /// 今日盈亏
    pub daily_pnl: Decimal,
    /// 最近交易时间
    pub last_trade_time: Option<DateTime<Utc>>,
    /// 交易频率 (每秒)
    pub trade_frequency: f64,
    /// 成功率
    pub success_rate: Decimal,
    /// 平均盈利
    pub average_profit: Decimal,
    /// 最大连续亏损
    pub max_consecutive_losses: u32,
    /// 当前连续亏损
    pub current_consecutive_losses: u32,
}

/// 流动性指标
#[derive(Debug, Clone)]
pub struct LiquidityMetrics {
    /// 买方流动性
    pub bid_liquidity: Decimal,
    /// 卖方流动性
    pub ask_liquidity: Decimal,
    /// 总流动性
    pub total_liquidity: Decimal,
    /// 流动性不平衡
    pub liquidity_imbalance: Decimal,
    /// 更新时间
    pub timestamp: DateTime<Utc>,
}

/// 策略风险控制器
pub struct StrategyRiskController {
    /// 配置
    config: RiskControlConfig,
    /// 当前持仓
    position: Arc<RwLock<Position>>,
    /// 活跃订单
    active_orders: Arc<RwLock<HashMap<OrderId, Order>>>,
    /// 交易统计
    trading_stats: Arc<RwLock<TradingStatistics>>,
    /// 流动性指标
    liquidity_metrics: Arc<RwLock<Option<LiquidityMetrics>>>,
    /// 紧急停止状态
    emergency_stop: Arc<RwLock<bool>>,
    /// 风险事件历史
    risk_events: Arc<RwLock<Vec<(DateTime<Utc>, RiskLevel, String)>>>,
}

impl StrategyRiskController {
    pub fn new(config: RiskControlConfig) -> Self {
        Self {
            config,
            position: Arc::new(RwLock::new(Position {
                symbol: Symbol::new("BTC", "USDT", crate::common::Exchange::Binance),
                quantity: Decimal::ZERO,
                average_price: Decimal::ZERO,
                unrealized_pnl: Decimal::ZERO,
                realized_pnl: Decimal::ZERO,
                last_update: Utc::now(),
            })),
            active_orders: Arc::new(RwLock::new(HashMap::new())),
            trading_stats: Arc::new(RwLock::new(TradingStatistics::default())),
            liquidity_metrics: Arc::new(RwLock::new(None)),
            emergency_stop: Arc::new(RwLock::new(false)),
            risk_events: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// 检查订单风险
    pub async fn check_order_risk(&self, order: &Order) -> RiskCheckResult {
        let mut result = RiskCheckResult {
            passed: true,
            risk_level: RiskLevel::Low,
            messages: Vec::new(),
            suggested_actions: Vec::new(),
        };

        // 检查紧急停止状态
        if *self.emergency_stop.read().await {
            result.passed = false;
            result.risk_level = RiskLevel::Critical;
            result.messages.push("Emergency stop is active".to_string());
            result.suggested_actions.push(RiskAction::EmergencyStop);
            return result;
        }

        // 检查订单大小
        if order.quantity > self.config.max_order_size {
            result.passed = false;
            result.risk_level = RiskLevel::High;
            result.messages.push(format!(
                "Order size {} exceeds maximum {}",
                order.quantity, self.config.max_order_size
            ));
            result.suggested_actions.push(RiskAction::ReduceOrderSize(self.config.max_order_size));
        }

        // 检查持仓限制
        let position = self.position.read().await;
        let new_position = match order.side {
            Side::Buy => position.quantity + order.quantity,
            Side::Sell => position.quantity - order.quantity,
        };

        if new_position.abs() > self.config.max_position_limit {
            result.passed = false;
            result.risk_level = RiskLevel::High;
            result.messages.push(format!(
                "New position {} would exceed limit {}",
                new_position, self.config.max_position_limit
            ));
            result.suggested_actions.push(RiskAction::ReducePosition(
                self.config.max_position_limit
            ));
        }

        // 检查价格偏离
        if let Some(order_price) = order.price {
            // 这里需要当前市场价格来比较
            // 简化实现，假设有一个参考价格
            let reference_price = position.average_price;
            if !reference_price.is_zero() {
                let price_deviation = (order_price - reference_price).abs() / reference_price;
                let max_deviation = Decimal::new(self.config.max_price_deviation_bps as i64, 4);
                
                if price_deviation > max_deviation {
                    result.risk_level = RiskLevel::Medium;
                    result.messages.push(format!(
                        "Price deviation {:.4} exceeds maximum {:.4}",
                        price_deviation, max_deviation
                    ));
                }
            }
        }

        // 检查订单数量限制
        let active_orders = self.active_orders.read().await;
        if active_orders.len() >= self.config.max_order_count as usize {
            result.passed = false;
            result.risk_level = RiskLevel::Medium;
            result.messages.push(format!(
                "Active order count {} exceeds maximum {}",
                active_orders.len(), self.config.max_order_count
            ));
            result.suggested_actions.push(RiskAction::CancelAllOrders);
        }

        // 检查交易频率
        let trading_stats = self.trading_stats.read().await;
        if trading_stats.trade_frequency > self.config.max_trade_frequency as f64 {
            result.passed = false;
            result.risk_level = RiskLevel::Medium;
            result.messages.push(format!(
                "Trade frequency {:.2} exceeds maximum {}",
                trading_stats.trade_frequency, self.config.max_trade_frequency
            ));
            result.suggested_actions.push(RiskAction::PauseTrading);
        }

        // 检查日内损失
        if trading_stats.daily_pnl < -self.config.max_daily_loss {
            result.passed = false;
            result.risk_level = RiskLevel::Critical;
            result.messages.push(format!(
                "Daily loss {} exceeds maximum {}",
                trading_stats.daily_pnl, self.config.max_daily_loss
            ));
            result.suggested_actions.push(RiskAction::EmergencyStop);
        }

        result
    }

    /// 检查报价风险
    pub async fn check_quote_risk(&self, quote: &Quote) -> RiskCheckResult {
        let mut result = RiskCheckResult {
            passed: true,
            risk_level: RiskLevel::Low,
            messages: Vec::new(),
            suggested_actions: Vec::new(),
        };

        // 检查价差
        let spread_bps = (quote.spread / quote.mid_price * Decimal::new(10000, 0)).to_u32().unwrap_or(0);
        
        if spread_bps > self.config.max_spread_bps {
            result.risk_level = RiskLevel::Medium;
            result.messages.push(format!(
                "Spread {} bps exceeds maximum {} bps",
                spread_bps, self.config.max_spread_bps
            ));
        }

        if spread_bps < self.config.min_spread_bps {
            result.passed = false;
            result.risk_level = RiskLevel::High;
            result.messages.push(format!(
                "Spread {} bps below minimum {} bps",
                spread_bps, self.config.min_spread_bps
            ));
        }

        // 检查流动性
        if self.config.liquidity_check_enabled {
            if let Some(liquidity) = self.liquidity_metrics.read().await.as_ref() {
                if liquidity.total_liquidity < self.config.min_liquidity_requirement {
                    result.passed = false;
                    result.risk_level = RiskLevel::High;
                    result.messages.push(format!(
                        "Insufficient liquidity: {} < {}",
                        liquidity.total_liquidity, self.config.min_liquidity_requirement
                    ));
                    result.suggested_actions.push(RiskAction::PauseTrading);
                }
            }
        }

        result
    }

    /// 更新持仓
    pub async fn update_position(&self, new_position: Position) {
        *self.position.write().await = new_position;
    }

    /// 更新活跃订单
    pub async fn update_active_orders(&self, orders: HashMap<OrderId, Order>) {
        *self.active_orders.write().await = orders;
    }

    /// 更新交易统计
    pub async fn update_trading_stats(&self, trade_pnl: Decimal, is_profitable: bool) {
        let mut stats = self.trading_stats.write().await;
        
        stats.daily_trade_count += 1;
        stats.daily_pnl += trade_pnl;
        stats.last_trade_time = Some(Utc::now());

        // 更新成功率
        if is_profitable {
            stats.current_consecutive_losses = 0;
        } else {
            stats.current_consecutive_losses += 1;
            stats.max_consecutive_losses = stats.max_consecutive_losses.max(stats.current_consecutive_losses);
        }

        // 计算交易频率 (简化实现)
        if let Some(last_time) = stats.last_trade_time {
            let time_diff = Utc::now() - last_time;
            if time_diff.num_seconds() > 0 {
                stats.trade_frequency = 1.0 / time_diff.num_seconds() as f64;
            }
        }

        // 计算平均盈利
        if stats.daily_trade_count > 0 {
            stats.average_profit = stats.daily_pnl / Decimal::new(stats.daily_trade_count as i64, 0);
        }
    }

    /// 更新流动性指标
    pub async fn update_liquidity_metrics(&self, metrics: LiquidityMetrics) {
        *self.liquidity_metrics.write().await = Some(metrics);
    }

    /// 触发紧急停止
    pub async fn trigger_emergency_stop(&self, reason: String) {
        *self.emergency_stop.write().await = true;
        
        // 记录风险事件
        let mut events = self.risk_events.write().await;
        events.push((Utc::now(), RiskLevel::Critical, reason));
        
        log::error!("Emergency stop triggered");
    }

    /// 解除紧急停止
    pub async fn release_emergency_stop(&self) {
        *self.emergency_stop.write().await = false;
        log::info!("Emergency stop released");
    }

    /// 检查是否处于紧急停止状态
    pub async fn is_emergency_stopped(&self) -> bool {
        *self.emergency_stop.read().await
    }

    /// 获取风险统计
    pub async fn get_risk_statistics(&self) -> RiskStatistics {
        let position = self.position.read().await;
        let trading_stats = self.trading_stats.read().await;
        let active_orders = self.active_orders.read().await;
        let emergency_stop = *self.emergency_stop.read().await;

        RiskStatistics {
            current_position: position.quantity,
            position_limit_utilization: (position.quantity.abs() / self.config.max_position_limit * Decimal::new(100, 0)),
            daily_pnl: trading_stats.daily_pnl,
            daily_loss_limit_utilization: if self.config.max_daily_loss > Decimal::ZERO {
                (trading_stats.daily_pnl.abs() / self.config.max_daily_loss * Decimal::new(100, 0))
            } else {
                Decimal::ZERO
            },
            active_order_count: active_orders.len() as u32,
            order_count_limit_utilization: (Decimal::new(active_orders.len() as i64, 0) / 
                                           Decimal::new(self.config.max_order_count as i64, 0) * Decimal::new(100, 0)),
            trade_frequency: trading_stats.trade_frequency,
            success_rate: trading_stats.success_rate,
            consecutive_losses: trading_stats.current_consecutive_losses,
            emergency_stop_active: emergency_stop,
        }
    }

    /// 重置日内统计
    pub async fn reset_daily_statistics(&self) {
        let mut stats = self.trading_stats.write().await;
        stats.daily_trade_count = 0;
        stats.daily_pnl = Decimal::ZERO;
        stats.current_consecutive_losses = 0;
        log::info!("Daily statistics reset");
    }

    /// 获取风险事件历史
    pub async fn get_risk_events(&self) -> Vec<(DateTime<Utc>, RiskLevel, String)> {
        self.risk_events.read().await.clone()
    }
}

/// 风险统计信息
#[derive(Debug, Clone)]
pub struct RiskStatistics {
    /// 当前持仓
    pub current_position: Decimal,
    /// 持仓限制使用率 (%)
    pub position_limit_utilization: Decimal,
    /// 日内盈亏
    pub daily_pnl: Decimal,
    /// 日内损失限制使用率 (%)
    pub daily_loss_limit_utilization: Decimal,
    /// 活跃订单数量
    pub active_order_count: u32,
    /// 订单数量限制使用率 (%)
    pub order_count_limit_utilization: Decimal,
    /// 交易频率
    pub trade_frequency: f64,
    /// 成功率
    pub success_rate: Decimal,
    /// 连续亏损次数
    pub consecutive_losses: u32,
    /// 紧急停止状态
    pub emergency_stop_active: bool,
}

/// 仓位控制器
pub struct PositionController {
    /// 目标持仓
    target_position: Arc<RwLock<Decimal>>,
    /// 最大持仓
    max_position: Decimal,
    /// 仓位调整速度
    adjustment_speed: Decimal,
    /// 当前持仓
    current_position: Arc<RwLock<Decimal>>,
}

impl PositionController {
    pub fn new(max_position: Decimal, adjustment_speed: Decimal) -> Self {
        Self {
            target_position: Arc::new(RwLock::new(Decimal::ZERO)),
            max_position,
            adjustment_speed,
            current_position: Arc::new(RwLock::new(Decimal::ZERO)),
        }
    }

    /// 设置目标持仓
    pub async fn set_target_position(&self, target: Decimal) {
        let clamped_target = target.max(-self.max_position).min(self.max_position);
        *self.target_position.write().await = clamped_target;
        log::info!("Target position set to: {}", clamped_target);
    }

    /// 获取目标持仓
    pub async fn get_target_position(&self) -> Decimal {
        *self.target_position.read().await
    }

    /// 更新当前持仓
    pub async fn update_current_position(&self, position: Decimal) {
        *self.current_position.write().await = position;
    }

    /// 计算需要的仓位调整
    pub async fn calculate_position_adjustment(&self) -> Decimal {
        let target = *self.target_position.read().await;
        let current = *self.current_position.read().await;
        let difference = target - current;
        
        // 根据调整速度限制单次调整量
        let max_adjustment = self.max_position * self.adjustment_speed;
        
        if difference.abs() <= max_adjustment {
            difference
        } else if difference > Decimal::ZERO {
            max_adjustment
        } else {
            -max_adjustment
        }
    }

    /// 检查仓位是否在限制内
    pub async fn is_position_within_limits(&self, position: Decimal) -> bool {
        position.abs() <= self.max_position
    }

    /// 获取仓位统计
    pub async fn get_position_statistics(&self) -> PositionStatistics {
        let target = *self.target_position.read().await;
        let current = *self.current_position.read().await;
        
        PositionStatistics {
            current_position: current,
            target_position: target,
            position_difference: target - current,
            max_position: self.max_position,
            position_utilization: (current.abs() / self.max_position * Decimal::new(100, 0)),
        }
    }
}

/// 仓位统计信息
#[derive(Debug, Clone)]
pub struct PositionStatistics {
    /// 当前持仓
    pub current_position: Decimal,
    /// 目标持仓
    pub target_position: Decimal,
    /// 仓位差异
    pub position_difference: Decimal,
    /// 最大持仓
    pub max_position: Decimal,
    /// 仓位使用率 (%)
    pub position_utilization: Decimal,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oms::OrderStatus;

    #[tokio::test]
    async fn test_risk_controller_creation() {
        let config = RiskControlConfig::default();
        let controller = StrategyRiskController::new(config);
        
        assert!(!controller.is_emergency_stopped().await);
    }

    #[tokio::test]
    async fn test_order_size_risk_check() {
        let config = RiskControlConfig {
            max_order_size: Decimal::new(1, 0),
            ..Default::default()
        };
        let controller = StrategyRiskController::new(config);

        let order = Order {
            id: OrderId::new(),
            symbol: Symbol::new("BTC", "USDT", crate::common::Exchange::Binance),
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: Decimal::new(2, 0), // 超过限制
            price: Some(Decimal::new(50000, 0)),
            status: OrderStatus::Pending,
            filled_quantity: Decimal::ZERO,
            average_fill_price: Decimal::ZERO,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            exchange: crate::common::Exchange::Binance,
        };

        let result = controller.check_order_risk(&order).await;
        assert!(!result.passed);
        assert_eq!(result.risk_level, RiskLevel::High);
    }

    #[tokio::test]
    async fn test_position_controller() {
        let controller = PositionController::new(
            Decimal::new(10, 0), // max position
            Decimal::new(1, 1),  // 10% adjustment speed
        );

        controller.set_target_position(Decimal::new(5, 0)).await;
        assert_eq!(controller.get_target_position().await, Decimal::new(5, 0));

        controller.update_current_position(Decimal::ZERO).await;
        let adjustment = controller.calculate_position_adjustment().await;
        assert_eq!(adjustment, Decimal::new(1, 0)); // 10% of max position
    }

    #[tokio::test]
    async fn test_emergency_stop() {
        let config = RiskControlConfig::default();
        let controller = StrategyRiskController::new(config);

        assert!(!controller.is_emergency_stopped().await);
        
        controller.trigger_emergency_stop("Test emergency".to_string()).await;
        assert!(controller.is_emergency_stopped().await);
        
        controller.release_emergency_stop().await;
        assert!(!controller.is_emergency_stopped().await);
    }

    #[tokio::test]
    async fn test_quote_risk_check() {
        let config = RiskControlConfig {
            min_spread_bps: 10,
            max_spread_bps: 100,
            ..Default::default()
        };
        let controller = StrategyRiskController::new(config);

        let quote = Quote {
            bid_price: Decimal::new(49999, 0),
            bid_size: Decimal::new(1, 1),
            ask_price: Decimal::new(50001, 0),
            ask_size: Decimal::new(1, 1),
            mid_price: Decimal::new(50000, 0),
            spread: Decimal::new(2, 0), // 2 USDT
            timestamp: Utc::now(),
        };

        let result = controller.check_quote_risk(&quote).await;
        assert!(result.passed); // 应该通过，因为价差在合理范围内
    }
}

