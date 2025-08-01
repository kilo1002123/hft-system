pub mod exchange_adapter;

pub use exchange_adapter::*;

use crate::common::{Exchange, Result};
use async_trait::async_trait;
use std::collections::HashMap;

/// 传统订单结构 (保持向后兼容)
#[derive(Debug, Clone)]
pub struct Order {
    pub id: uuid::Uuid,
    pub exchange_order_id: Option<String>,
    pub symbol: crate::common::Symbol,
    pub side: crate::common::Side,
    pub order_type: crate::common::OrderType,
    pub quantity: rust_decimal::Decimal,
    pub price: Option<rust_decimal::Decimal>,
    pub status: crate::common::OrderStatus,
    pub filled_quantity: rust_decimal::Decimal,
    pub average_price: Option<rust_decimal::Decimal>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub metadata: HashMap<String, String>,
}

/// 订单管理系统接口 (保持向后兼容)
#[async_trait]
pub trait OrderManagementSystem: Send + Sync {
    /// 提交订单
    async fn submit_order(&mut self, order: Order) -> Result<String>;
    
    /// 取消订单
    async fn cancel_order(&mut self, order_id: &str) -> Result<()>;
    
    /// 修改订单
    async fn modify_order(&mut self, order_id: &str, new_order: Order) -> Result<()>;
    
    /// 查询订单状态
    async fn get_order_status(&self, order_id: &str) -> Result<Order>;
    
    /// 获取活跃订单
    async fn get_active_orders(&self) -> Result<Vec<Order>>;
}

/// 现代化的订单管理系统实现
pub struct ModernOMS {
    order_manager: OrderManager,
    default_exchange: Exchange,
}

impl ModernOMS {
    pub fn new(default_exchange: Exchange) -> Self {
        Self {
            order_manager: OrderManager::new(),
            default_exchange,
        }
    }
    
    pub fn add_exchange_adapter(&mut self, adapter: Box<dyn ExchangeAdapter>) {
        self.order_manager.add_adapter(adapter);
    }
    
    pub async fn place_order_on_exchange(
        &self,
        exchange: Exchange,
        request: &OrderRequest,
    ) -> Result<OrderResponse, ExchangeError> {
        self.order_manager.place_order(exchange, request).await
    }
    
    pub async fn cancel_order_on_exchange(
        &self,
        exchange: Exchange,
        order_id: &str,
        symbol: &crate::common::Symbol,
    ) -> Result<OrderResponse, ExchangeError> {
        self.order_manager.cancel_order(exchange, order_id, symbol).await
    }
    
    pub async fn get_order_from_exchange(
        &self,
        exchange: Exchange,
        order_id: &str,
        symbol: &crate::common::Symbol,
    ) -> Result<OrderResponse, ExchangeError> {
        self.order_manager.get_order(exchange, order_id, symbol).await
    }
    
    pub async fn get_balance_from_exchange(
        &self,
        exchange: Exchange,
    ) -> Result<Vec<Balance>, ExchangeError> {
        self.order_manager.get_balance(exchange).await
    }
}

/// 简单的订单管理系统实现 (保持向后兼容)
pub struct SimpleOMS {
    exchange: Exchange,
    modern_oms: ModernOMS,
}

impl SimpleOMS {
    pub fn new(exchange: Exchange) -> Self {
        Self { 
            exchange,
            modern_oms: ModernOMS::new(exchange),
        }
    }
    
    pub fn add_exchange_adapter(&mut self, adapter: Box<dyn ExchangeAdapter>) {
        self.modern_oms.add_exchange_adapter(adapter);
    }
}

#[async_trait]
impl OrderManagementSystem for SimpleOMS {
    async fn submit_order(&mut self, order: Order) -> Result<String> {
        log::info!("Submitting order: {:?}", order);
        
        // 转换为新的订单请求格式
        let request = OrderRequest {
            client_order_id: Some(order.id.to_string()),
            symbol: order.symbol,
            side: match order.side {
                crate::common::Side::Buy => OrderSide::Buy,
                crate::common::Side::Sell => OrderSide::Sell,
            },
            order_type: match order.order_type {
                crate::common::OrderType::Market => OrderType::Market,
                crate::common::OrderType::Limit => OrderType::Limit,
                crate::common::OrderType::StopLoss => OrderType::StopLoss,
                crate::common::OrderType::TakeProfit => OrderType::TakeProfit,
            },
            quantity: order.quantity,
            price: order.price,
            stop_price: None,
            time_in_force: Some(TimeInForce::GTC),
            reduce_only: None,
            metadata: order.metadata,
        };
        
        match self.modern_oms.place_order_on_exchange(self.exchange, &request).await {
            Ok(response) => Ok(response.order_id),
            Err(e) => {
                log::error!("Failed to submit order: {}", e);
                Err(crate::common::HftError::Other(format!("Order submission failed: {}", e)))
            }
        }
    }
    
    async fn cancel_order(&mut self, order_id: &str) -> Result<()> {
        log::info!("Cancelling order: {}", order_id);
        
        // 注意: 这里需要知道交易对，实际实现中应该从订单缓存中获取
        let symbol = crate::common::Symbol::new("BTC", "USDT", self.exchange);
        
        match self.modern_oms.cancel_order_on_exchange(self.exchange, order_id, &symbol).await {
            Ok(_) => Ok(()),
            Err(e) => {
                log::error!("Failed to cancel order: {}", e);
                Err(crate::common::HftError::Other(format!("Order cancellation failed: {}", e)))
            }
        }
    }
    
    async fn modify_order(&mut self, order_id: &str, new_order: Order) -> Result<()> {
        log::info!("Modifying order: {} -> {:?}", order_id, new_order);
        
        // 先取消原订单，再创建新订单
        self.cancel_order(order_id).await?;
        self.submit_order(new_order).await?;
        
        Ok(())
    }
    
    async fn get_order_status(&self, order_id: &str) -> Result<Order> {
        log::info!("Getting order status: {}", order_id);
        
        // 注意: 这里需要知道交易对，实际实现中应该从订单缓存中获取
        let symbol = crate::common::Symbol::new("BTC", "USDT", self.exchange);
        
        match self.modern_oms.get_order_from_exchange(self.exchange, order_id, &symbol).await {
            Ok(response) => {
                // 转换为旧的订单格式
                Ok(Order {
                    id: uuid::Uuid::parse_str(&response.client_order_id.unwrap_or_default())
                        .unwrap_or_else(|_| uuid::Uuid::new_v4()),
                    exchange_order_id: Some(response.order_id),
                    symbol: response.symbol,
                    side: match response.side {
                        OrderSide::Buy => crate::common::Side::Buy,
                        OrderSide::Sell => crate::common::Side::Sell,
                    },
                    order_type: match response.order_type {
                        OrderType::Market => crate::common::OrderType::Market,
                        OrderType::Limit => crate::common::OrderType::Limit,
                        OrderType::StopLoss => crate::common::OrderType::StopLoss,
                        OrderType::TakeProfit => crate::common::OrderType::TakeProfit,
                        _ => crate::common::OrderType::Limit,
                    },
                    quantity: response.original_quantity,
                    price: response.price,
                    status: match response.status {
                        OrderStatus::New => crate::common::OrderStatus::New,
                        OrderStatus::PartiallyFilled => crate::common::OrderStatus::PartiallyFilled,
                        OrderStatus::Filled => crate::common::OrderStatus::Filled,
                        OrderStatus::Canceled => crate::common::OrderStatus::Canceled,
                        OrderStatus::Rejected => crate::common::OrderStatus::Rejected,
                        OrderStatus::Expired => crate::common::OrderStatus::Expired,
                        _ => crate::common::OrderStatus::New,
                    },
                    filled_quantity: response.executed_quantity,
                    average_price: response.average_price,
                    created_at: response.created_at,
                    updated_at: response.updated_at,
                    metadata: response.exchange_data,
                })
            }
            Err(e) => {
                log::error!("Failed to get order status: {}", e);
                Err(crate::common::HftError::Other(format!("Order query failed: {}", e)))
            }
        }
    }
    
    async fn get_active_orders(&self) -> Result<Vec<Order>> {
        log::info!("Getting active orders");
        // 这里应该实现实际的活跃订单查询逻辑
        // 由于需要遍历所有交易对，这里暂时返回空列表
        Ok(Vec::new())
    }
}

