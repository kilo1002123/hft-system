use hft_system::common::{Exchange, MarketDataEvent, OrderBook, OrderBookLevel, Side, Symbol, Ticker, Trade};
use hft_system::data_ingestion::DataProvider;
use hft_system::oms::{Order, OrderId, OrderStatus, OrderType, OrderManager};
use hft_system::rms::{MockRiskManager, Position, RiskManager};
use hft_system::strategy_engine::{
    StrategyEngine, StrategyEngineConfig, StrategyType,
    market_making::{BasicMarketMaker, MarketMakingConfig, MarketMakingStrategy, StrategyEvent},
    advanced_market_making::{AdvancedMarketMaker, AdvancedMarketMakingConfig},
    risk_controls::{RiskControlConfig, StrategyRiskController}
};
use chrono::{Duration, Utc};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{sleep, timeout, Duration as TokioDuration};

/// 模拟数据提供者
struct MockDataProvider {
    symbol: Symbol,
    event_sender: mpsc::UnboundedSender<MarketDataEvent>,
    is_running: Arc<RwLock<bool>>,
}

impl MockDataProvider {
    fn new(symbol: Symbol) -> (Self, mpsc::UnboundedReceiver<MarketDataEvent>) {
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        
        (
            Self {
                symbol,
                event_sender,
                is_running: Arc::new(RwLock::new(false)),
            },
            event_receiver,
        )
    }

    async fn start_simulation(&self) {
        *self.is_running.write().await = true;
        let is_running = self.is_running.clone();
        let sender = self.event_sender.clone();
        let symbol = self.symbol.clone();

        tokio::spawn(async move {
            let mut price = Decimal::new(50000, 0);
            let mut counter = 0;

            while *is_running.read().await {
                counter += 1;

                // 模拟价格波动
                let price_change = if counter % 2 == 0 {
                    Decimal::new(10, 0)
                } else {
                    Decimal::new(-5, 0)
                };
                price += price_change;

                // 发送ticker数据
                let ticker = Ticker {
                    symbol: symbol.clone(),
                    last_price: price,
                    bid_price: price - Decimal::new(5, 0),
                    ask_price: price + Decimal::new(5, 0),
                    volume_24h: Decimal::new(1000, 0),
                    price_change_24h: Decimal::new(100, 0),
                    timestamp: Utc::now(),
                };

                if sender.send(MarketDataEvent::Ticker(ticker)).is_err() {
                    break;
                }

                // 发送订单簿数据
                let order_book = OrderBook {
                    symbol: symbol.clone(),
                    bids: vec![
                        OrderBookLevel { price: price - Decimal::new(5, 0), quantity: Decimal::new(10, 0) },
                        OrderBookLevel { price: price - Decimal::new(10, 0), quantity: Decimal::new(20, 0) },
                    ],
                    asks: vec![
                        OrderBookLevel { price: price + Decimal::new(5, 0), quantity: Decimal::new(10, 0) },
                        OrderBookLevel { price: price + Decimal::new(10, 0), quantity: Decimal::new(20, 0) },
                    ],
                    timestamp: Utc::now(),
                };

                if sender.send(MarketDataEvent::OrderBook(order_book)).is_err() {
                    break;
                }

                // 发送交易数据
                let trade = Trade {
                    symbol: symbol.clone(),
                    price,
                    quantity: Decimal::new(1, 1),
                    side: if counter % 2 == 0 { Side::Buy } else { Side::Sell },
                    timestamp: Utc::now(),
                    trade_id: format!("trade_{}", counter),
                };

                if sender.send(MarketDataEvent::Trade(trade)).is_err() {
                    break;
                }

                sleep(TokioDuration::from_millis(100)).await;
            }
        });
    }

    async fn stop_simulation(&self) {
        *self.is_running.write().await = false;
    }
}

/// 模拟订单管理器
struct MockOrderManager {
    orders: Arc<RwLock<HashMap<OrderId, Order>>>,
    order_updates: mpsc::UnboundedSender<Order>,
}

impl MockOrderManager {
    fn new() -> (Self, mpsc::UnboundedReceiver<Order>) {
        let (order_updates, order_receiver) = mpsc::unbounded_channel();
        
        (
            Self {
                orders: Arc::new(RwLock::new(HashMap::new())),
                order_updates,
            },
            order_receiver,
        )
    }

    async fn submit_order(&self, mut order: Order) -> Result<OrderId, String> {
        // 模拟订单处理延迟
        sleep(TokioDuration::from_millis(10)).await;

        // 模拟订单状态变化
        order.status = OrderStatus::Pending;
        self.orders.write().await.insert(order.id.clone(), order.clone());
        
        // 发送订单更新
        if self.order_updates.send(order.clone()).is_err() {
            return Err("Failed to send order update".to_string());
        }

        // 模拟订单成交
        let order_id = order.id.clone();
        let orders = self.orders.clone();
        let updates = self.order_updates.clone();
        
        tokio::spawn(async move {
            sleep(TokioDuration::from_millis(50)).await;
            
            if let Some(mut order) = orders.write().await.get_mut(&order_id) {
                order.status = OrderStatus::Filled;
                order.filled_quantity = order.quantity;
                order.average_fill_price = order.price.unwrap_or(Decimal::ZERO);
                order.updated_at = Utc::now();
                
                let _ = updates.send(order.clone());
            }
        });

        Ok(order.id)
    }

    async fn cancel_order(&self, order_id: &OrderId) -> Result<(), String> {
        if let Some(mut order) = self.orders.write().await.get_mut(order_id) {
            order.status = OrderStatus::Cancelled;
            order.updated_at = Utc::now();
            
            if self.order_updates.send(order.clone()).is_err() {
                return Err("Failed to send order update".to_string());
            }
        }
        
        Ok(())
    }
}

/// 测试基础做市策略集成
#[tokio::test]
async fn test_basic_market_making_integration() {
    let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
    
    // 创建模拟数据提供者
    let (data_provider, mut market_data_receiver) = MockDataProvider::new(symbol.clone());
    
    // 创建模拟订单管理器
    let (order_manager, mut order_update_receiver) = MockOrderManager::new();
    
    // 创建策略配置
    let config = MarketMakingConfig {
        symbol: symbol.clone(),
        base_spread_bps: 20,
        order_size: Decimal::new(1, 1),
        max_position: Decimal::new(1, 0),
        refresh_interval_ms: 200,
        ..Default::default()
    };

    // 创建风险管理器
    let risk_manager = Arc::new(MockRiskManager::new());
    
    // 创建事件和订单通道
    let (event_sender, mut event_receiver) = mpsc::unbounded_channel();
    let (order_sender, mut order_receiver) = mpsc::unbounded_channel();

    // 创建策略
    let mut strategy = BasicMarketMaker::new(
        config,
        risk_manager,
        event_sender,
        order_sender,
    );

    // 初始化并启动策略
    assert!(strategy.initialize().await.is_ok());
    assert!(strategy.start().await.is_ok());

    // 启动数据模拟
    data_provider.start_simulation().await;

    // 启动订单处理任务
    let order_manager_clone = Arc::new(order_manager);
    let order_manager_task = order_manager_clone.clone();
    tokio::spawn(async move {
        while let Some(order) = order_receiver.recv().await {
            if let Err(e) = order_manager_task.submit_order(order).await {
                eprintln!("Failed to submit order: {}", e);
            }
        }
    });

    // 启动市场数据处理任务
    let mut strategy_clone = strategy;
    tokio::spawn(async move {
        while let Some(market_event) = market_data_receiver.recv().await {
            if let Err(e) = strategy_clone.on_market_data(market_event).await {
                eprintln!("Failed to process market data: {}", e);
            }
        }
    });

    // 启动订单更新处理任务
    tokio::spawn(async move {
        while let Some(order_update) = order_update_receiver.recv().await {
            // 这里应该将订单更新发送给策略
            println!("Order update: {:?}", order_update);
        }
    });

    // 运行测试一段时间
    let test_duration = TokioDuration::from_secs(2);
    let mut event_count = 0;
    let mut order_count = 0;

    let start_time = tokio::time::Instant::now();
    while start_time.elapsed() < test_duration {
        tokio::select! {
            Some(event) = event_receiver.recv() => {
                event_count += 1;
                match event {
                    StrategyEvent::Started => println!("Strategy started"),
                    StrategyEvent::QuoteUpdated(quote) => {
                        println!("Quote updated: bid={}, ask={}", quote.bid_price, quote.ask_price);
                    },
                    StrategyEvent::OrderSubmitted(_) => {
                        order_count += 1;
                        println!("Order submitted");
                    },
                    StrategyEvent::OrderFilled(_, quantity, price) => {
                        println!("Order filled: {} @ {}", quantity, price);
                    },
                    _ => {}
                }
            }
            _ = sleep(TokioDuration::from_millis(100)) => {
                // 定期检查
            }
        }
    }

    // 停止数据模拟
    data_provider.stop_simulation().await;

    // 验证测试结果
    assert!(event_count > 0, "Should have received strategy events");
    println!("Integration test completed: {} events, {} orders", event_count, order_count);
}

/// 测试高级做市策略集成
#[tokio::test]
async fn test_advanced_market_making_integration() {
    let symbol = Symbol::new("ETH", "USDT", Exchange::OKX);
    
    // 创建高级策略配置
    let config = AdvancedMarketMakingConfig {
        base_config: MarketMakingConfig {
            symbol: symbol.clone(),
            base_spread_bps: 15,
            order_size: Decimal::new(2, 1), // 0.2
            max_position: Decimal::new(2, 0),
            ..Default::default()
        },
        dynamic_spread_adjustment: true,
        order_book_imbalance_adjustment: true,
        max_order_levels: 2,
        adaptive_learning: true,
        ..Default::default()
    };

    let risk_manager = Arc::new(MockRiskManager::new());
    let (event_sender, mut event_receiver) = mpsc::unbounded_channel();
    let (order_sender, mut order_receiver) = mpsc::unbounded_channel();

    let mut strategy = AdvancedMarketMaker::new(
        config,
        risk_manager,
        event_sender,
        order_sender,
    );

    // 初始化并启动策略
    assert!(strategy.initialize().await.is_ok());
    assert!(strategy.start().await.is_ok());

    // 创建模拟市场数据
    let ticker = Ticker {
        symbol: symbol.clone(),
        last_price: Decimal::new(3000, 0),
        bid_price: Decimal::new(2995, 0),
        ask_price: Decimal::new(3005, 0),
        volume_24h: Decimal::new(5000, 0),
        price_change_24h: Decimal::new(50, 0),
        timestamp: Utc::now(),
    };

    // 处理市场数据
    assert!(strategy.on_market_data(MarketDataEvent::Ticker(ticker)).await.is_ok());

    // 检查是否有订单生成
    let mut order_count = 0;
    while let Ok(Some(_order)) = timeout(TokioDuration::from_millis(100), order_receiver.recv()).await {
        order_count += 1;
        if order_count >= 4 { // 2层 * 2边 = 4个订单
            break;
        }
    }

    // 验证多层订单
    assert!(order_count > 0, "Should have generated orders");
    println!("Advanced strategy generated {} orders", order_count);

    // 停止策略
    assert!(strategy.stop().await.is_ok());
}

/// 测试策略引擎集成
#[tokio::test]
async fn test_strategy_engine_integration() {
    let config = StrategyEngineConfig {
        max_concurrent_strategies: 5,
        update_interval_ms: 100,
        event_buffer_size: 1000,
    };

    let mut engine = StrategyEngine::new(config);

    // 启动策略引擎
    assert!(engine.start().await.is_ok());
    assert!(engine.is_running().await);

    // 创建测试策略
    let strategy_config = MarketMakingConfig::default();
    let risk_manager = Arc::new(MockRiskManager::new());
    let event_sender = engine.get_event_sender();
    let order_sender = engine.get_order_sender();

    let strategy = Arc::new(BasicMarketMaker::new(
        strategy_config,
        risk_manager,
        event_sender,
        order_sender,
    )) as Arc<dyn MarketMakingStrategy>;

    // 添加策略到引擎
    assert!(engine.add_strategy(
        "test_strategy_1".to_string(),
        "Test Market Making Strategy".to_string(),
        StrategyType::MarketMaking,
        strategy,
    ).await.is_ok());

    // 验证策略信息
    let strategies = engine.list_strategies().await;
    assert_eq!(strategies.len(), 1);
    assert_eq!(strategies[0].name, "Test Market Making Strategy");
    assert_eq!(strategies[0].strategy_type, StrategyType::MarketMaking);

    // 启动策略
    assert!(engine.start_strategy("test_strategy_1").await.is_ok());

    // 验证策略状态
    let strategy_info = engine.get_strategy_info("test_strategy_1").await;
    assert!(strategy_info.is_some());
    assert!(strategy_info.unwrap().is_active);

    // 停止策略
    assert!(engine.stop_strategy("test_strategy_1").await.is_ok());

    // 移除策略
    assert!(engine.remove_strategy("test_strategy_1").await.is_ok());

    // 停止引擎
    assert!(engine.stop().await.is_ok());
}

/// 测试风险控制集成
#[tokio::test]
async fn test_risk_control_integration() {
    let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
    
    // 创建严格的风险控制配置
    let risk_config = RiskControlConfig {
        max_position_limit: Decimal::new(1, 1), // 0.1 BTC
        max_daily_loss: Decimal::new(100, 0),   // 100 USDT
        max_order_size: Decimal::new(5, 2),     // 0.05 BTC
        max_order_count: 5,
        emergency_stop_enabled: true,
        ..Default::default()
    };

    let risk_controller = StrategyRiskController::new(risk_config);

    // 创建策略配置
    let strategy_config = MarketMakingConfig {
        symbol: symbol.clone(),
        order_size: Decimal::new(3, 2), // 0.03 BTC (在限制内)
        max_position: Decimal::new(8, 2), // 0.08 BTC (在限制内)
        ..Default::default()
    };

    let risk_manager = Arc::new(MockRiskManager::new());
    let (event_sender, mut event_receiver) = mpsc::unbounded_channel();
    let (order_sender, mut order_receiver) = mpsc::unbounded_channel();

    let mut strategy = BasicMarketMaker::new(
        strategy_config,
        risk_manager,
        event_sender,
        order_sender,
    );

    // 启动策略
    assert!(strategy.initialize().await.is_ok());
    assert!(strategy.start().await.is_ok());

    // 模拟市场数据触发订单生成
    let ticker = Ticker {
        symbol: symbol.clone(),
        last_price: Decimal::new(50000, 0),
        bid_price: Decimal::new(49995, 0),
        ask_price: Decimal::new(50005, 0),
        volume_24h: Decimal::new(1000, 0),
        price_change_24h: Decimal::new(100, 0),
        timestamp: Utc::now(),
    };

    assert!(strategy.on_market_data(MarketDataEvent::Ticker(ticker)).await.is_ok());

    // 检查生成的订单是否通过风险检查
    let mut passed_orders = 0;
    let mut failed_orders = 0;

    while let Ok(Some(order)) = timeout(TokioDuration::from_millis(100), order_receiver.recv()).await {
        let risk_result = risk_controller.check_order_risk(&order).await;
        
        if risk_result.passed {
            passed_orders += 1;
            println!("Order passed risk check: {} {}", order.side, order.quantity);
        } else {
            failed_orders += 1;
            println!("Order failed risk check: {:?}", risk_result.messages);
        }

        if passed_orders + failed_orders >= 2 { // 期望2个订单（买卖各一个）
            break;
        }
    }

    assert!(passed_orders > 0, "Should have some orders passing risk check");
    println!("Risk control test: {} passed, {} failed", passed_orders, failed_orders);

    // 测试紧急停止
    risk_controller.trigger_emergency_stop("Test emergency stop".to_string()).await;
    
    // 创建新订单测试紧急停止
    let test_order = Order {
        id: OrderId::new(),
        symbol: symbol.clone(),
        side: Side::Buy,
        order_type: OrderType::Limit,
        quantity: Decimal::new(1, 2),
        price: Some(Decimal::new(50000, 0)),
        status: OrderStatus::Pending,
        filled_quantity: Decimal::ZERO,
        average_fill_price: Decimal::ZERO,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        exchange: Exchange::Binance,
    };

    let emergency_result = risk_controller.check_order_risk(&test_order).await;
    assert!(!emergency_result.passed, "Orders should be blocked during emergency stop");

    // 解除紧急停止
    risk_controller.release_emergency_stop().await;
    let normal_result = risk_controller.check_order_risk(&test_order).await;
    assert!(normal_result.passed, "Orders should be allowed after emergency stop is released");
}

/// 测试多策略并发运行
#[tokio::test]
async fn test_multiple_strategies_integration() {
    let symbols = vec![
        Symbol::new("BTC", "USDT", Exchange::Binance),
        Symbol::new("ETH", "USDT", Exchange::OKX),
        Symbol::new("ADA", "USDT", Exchange::Gate),
    ];

    let mut strategies = Vec::new();
    let mut event_receivers = Vec::new();

    // 创建多个策略
    for (i, symbol) in symbols.iter().enumerate() {
        let config = MarketMakingConfig {
            symbol: symbol.clone(),
            base_spread_bps: 10 + (i as u32 * 5), // 不同的价差
            order_size: Decimal::new(1 + i as i64, 1), // 不同的订单大小
            ..Default::default()
        };

        let risk_manager = Arc::new(MockRiskManager::new());
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        let (order_sender, _order_receiver) = mpsc::unbounded_channel();

        let mut strategy = BasicMarketMaker::new(
            config,
            risk_manager,
            event_sender,
            order_sender,
        );

        assert!(strategy.initialize().await.is_ok());
        assert!(strategy.start().await.is_ok());

        strategies.push(strategy);
        event_receivers.push(event_receiver);
    }

    // 为每个策略发送市场数据
    for (i, (strategy, symbol)) in strategies.iter_mut().zip(symbols.iter()).enumerate() {
        let ticker = Ticker {
            symbol: symbol.clone(),
            last_price: Decimal::new(1000 * (i + 1) as i64, 0), // 不同的价格
            bid_price: Decimal::new(1000 * (i + 1) as i64 - 5, 0),
            ask_price: Decimal::new(1000 * (i + 1) as i64 + 5, 0),
            volume_24h: Decimal::new(1000, 0),
            price_change_24h: Decimal::new(10, 0),
            timestamp: Utc::now(),
        };

        assert!(strategy.on_market_data(MarketDataEvent::Ticker(ticker)).await.is_ok());
    }

    // 收集所有策略的事件
    let mut total_events = 0;
    for mut receiver in event_receivers {
        let mut strategy_events = 0;
        while let Ok(Some(_event)) = timeout(TokioDuration::from_millis(100), receiver.recv()).await {
            strategy_events += 1;
            total_events += 1;
            if strategy_events >= 3 { // 每个策略最多收集3个事件
                break;
            }
        }
    }

    assert!(total_events > 0, "Should have received events from multiple strategies");
    println!("Multiple strategies generated {} total events", total_events);

    // 停止所有策略
    for strategy in strategies.iter_mut() {
        assert!(strategy.stop().await.is_ok());
    }
}

/// 测试策略性能和延迟
#[tokio::test]
async fn test_strategy_performance() {
    let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
    let config = MarketMakingConfig {
        symbol: symbol.clone(),
        refresh_interval_ms: 10, // 高频更新
        ..Default::default()
    };

    let risk_manager = Arc::new(MockRiskManager::new());
    let (event_sender, mut event_receiver) = mpsc::unbounded_channel();
    let (order_sender, mut order_receiver) = mpsc::unbounded_channel();

    let mut strategy = BasicMarketMaker::new(
        config,
        risk_manager,
        event_sender,
        order_sender,
    );

    assert!(strategy.initialize().await.is_ok());
    assert!(strategy.start().await.is_ok());

    let start_time = tokio::time::Instant::now();
    let mut market_data_count = 0;
    let mut order_count = 0;

    // 发送大量市场数据
    for i in 0..100 {
        let ticker = Ticker {
            symbol: symbol.clone(),
            last_price: Decimal::new(50000 + i, 0),
            bid_price: Decimal::new(50000 + i - 5, 0),
            ask_price: Decimal::new(50000 + i + 5, 0),
            volume_24h: Decimal::new(1000, 0),
            price_change_24h: Decimal::new(i, 0),
            timestamp: Utc::now(),
        };

        let process_start = tokio::time::Instant::now();
        assert!(strategy.on_market_data(MarketDataEvent::Ticker(ticker)).await.is_ok());
        let process_time = process_start.elapsed();

        market_data_count += 1;

        // 检查处理延迟
        assert!(process_time < TokioDuration::from_millis(10), 
               "Market data processing should be fast");

        // 收集生成的订单
        while let Ok(Some(_order)) = timeout(TokioDuration::from_millis(1), order_receiver.recv()).await {
            order_count += 1;
        }
    }

    let total_time = start_time.elapsed();
    let avg_processing_time = total_time / market_data_count;

    println!("Performance test results:");
    println!("  Processed {} market data events in {:?}", market_data_count, total_time);
    println!("  Average processing time: {:?}", avg_processing_time);
    println!("  Generated {} orders", order_count);

    // 性能要求
    assert!(avg_processing_time < TokioDuration::from_millis(5), 
           "Average processing time should be under 5ms");
    assert!(order_count > 0, "Should have generated orders");

    assert!(strategy.stop().await.is_ok());
}

/// 测试错误恢复和容错性
#[tokio::test]
async fn test_error_recovery() {
    let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
    let config = MarketMakingConfig {
        symbol: symbol.clone(),
        ..Default::default()
    };

    let risk_manager = Arc::new(MockRiskManager::new());
    let (event_sender, _event_receiver) = mpsc::unbounded_channel();
    let (order_sender, _order_receiver) = mpsc::unbounded_channel();

    let mut strategy = BasicMarketMaker::new(
        config,
        risk_manager,
        event_sender,
        order_sender,
    );

    assert!(strategy.initialize().await.is_ok());
    assert!(strategy.start().await.is_ok());

    // 测试无效市场数据处理
    let invalid_ticker = Ticker {
        symbol: Symbol::new("INVALID", "PAIR", Exchange::Binance), // 不匹配的符号
        last_price: Decimal::new(50000, 0),
        bid_price: Decimal::new(49995, 0),
        ask_price: Decimal::new(50005, 0),
        volume_24h: Decimal::new(1000, 0),
        price_change_24h: Decimal::new(100, 0),
        timestamp: Utc::now(),
    };

    // 策略应该能够处理不匹配的符号而不崩溃
    assert!(strategy.on_market_data(MarketDataEvent::Ticker(invalid_ticker)).await.is_ok());

    // 测试异常订单更新
    let invalid_order = Order {
        id: OrderId::new(),
        symbol: Symbol::new("INVALID", "PAIR", Exchange::Binance),
        side: Side::Buy,
        order_type: OrderType::Limit,
        quantity: Decimal::new(1, 1),
        price: Some(Decimal::new(50000, 0)),
        status: OrderStatus::Filled,
        filled_quantity: Decimal::new(1, 1),
        average_fill_price: Decimal::new(50000, 0),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        exchange: Exchange::Binance,
    };

    // 策略应该能够处理无效订单而不崩溃
    assert!(strategy.on_order_update(invalid_order).await.is_ok());

    // 验证策略仍然正常运行
    assert!(matches!(strategy.get_state(), hft_system::strategy_engine::market_making::StrategyState::Running));

    // 测试正常数据处理仍然工作
    let valid_ticker = Ticker {
        symbol: symbol.clone(),
        last_price: Decimal::new(50000, 0),
        bid_price: Decimal::new(49995, 0),
        ask_price: Decimal::new(50005, 0),
        volume_24h: Decimal::new(1000, 0),
        price_change_24h: Decimal::new(100, 0),
        timestamp: Utc::now(),
    };

    assert!(strategy.on_market_data(MarketDataEvent::Ticker(valid_ticker)).await.is_ok());

    assert!(strategy.stop().await.is_ok());
}

/// 测试内存使用和资源清理
#[tokio::test]
async fn test_memory_management() {
    let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
    
    // 创建和销毁多个策略实例
    for i in 0..10 {
        let config = MarketMakingConfig {
            symbol: symbol.clone(),
            ..Default::default()
        };

        let risk_manager = Arc::new(MockRiskManager::new());
        let (event_sender, _event_receiver) = mpsc::unbounded_channel();
        let (order_sender, _order_receiver) = mpsc::unbounded_channel();

        let mut strategy = BasicMarketMaker::new(
            config,
            risk_manager,
            event_sender,
            order_sender,
        );

        assert!(strategy.initialize().await.is_ok());
        assert!(strategy.start().await.is_ok());

        // 发送一些数据
        let ticker = Ticker {
            symbol: symbol.clone(),
            last_price: Decimal::new(50000 + i, 0),
            bid_price: Decimal::new(50000 + i - 5, 0),
            ask_price: Decimal::new(50000 + i + 5, 0),
            volume_24h: Decimal::new(1000, 0),
            price_change_24h: Decimal::new(i, 0),
            timestamp: Utc::now(),
        };

        assert!(strategy.on_market_data(MarketDataEvent::Ticker(ticker)).await.is_ok());

        assert!(strategy.stop().await.is_ok());
        
        // 策略应该被正确清理
        drop(strategy);
    }

    println!("Memory management test completed - created and destroyed 10 strategy instances");
}

