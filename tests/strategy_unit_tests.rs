use hft_system::common::{Exchange, MarketDataEvent, OrderBook, OrderBookLevel, Side, Symbol, Ticker, Trade};
use hft_system::oms::{Order, OrderId, OrderStatus, OrderType};
use hft_system::rms::{MockRiskManager, Position};
use hft_system::strategy_engine::market_making::{
    BasicMarketMaker, MarketMakingConfig, MarketMakingStrategy, MarketSnapshot, Quote, StrategyState
};
use hft_system::strategy_engine::advanced_market_making::{
    AdvancedMarketMaker, AdvancedMarketMakingConfig
};
use hft_system::strategy_engine::risk_controls::{
    PositionController, RiskControlConfig, RiskLevel, StrategyRiskController
};
use chrono::Utc;
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::mpsc;

/// 测试做市策略配置
#[tokio::test]
async fn test_market_making_config() {
    let config = MarketMakingConfig::default();
    
    assert_eq!(config.base_spread_bps, 10);
    assert_eq!(config.min_spread_bps, 5);
    assert_eq!(config.max_spread_bps, 50);
    assert_eq!(config.order_size, Decimal::new(1, 2));
    assert_eq!(config.max_position, Decimal::new(1, 0));
    assert!(config.volatility_adjustment);
    assert!(config.inventory_skew_adjustment);
}

/// 测试基础做市策略创建
#[tokio::test]
async fn test_basic_market_maker_creation() {
    let config = MarketMakingConfig::default();
    let risk_manager = Arc::new(MockRiskManager::new());
    let (event_sender, _event_receiver) = mpsc::unbounded_channel();
    let (order_sender, _order_receiver) = mpsc::unbounded_channel();

    let strategy = BasicMarketMaker::new(
        config,
        risk_manager,
        event_sender,
        order_sender,
    );

    assert!(matches!(strategy.get_state(), StrategyState::Initializing));
    assert!(strategy.get_current_quote().is_none());
    
    let metrics = strategy.get_metrics();
    assert_eq!(metrics.total_trades, 0);
    assert_eq!(metrics.total_pnl, Decimal::ZERO);
}

/// 测试策略生命周期
#[tokio::test]
async fn test_strategy_lifecycle() {
    let config = MarketMakingConfig::default();
    let risk_manager = Arc::new(MockRiskManager::new());
    let (event_sender, mut event_receiver) = mpsc::unbounded_channel();
    let (order_sender, _order_receiver) = mpsc::unbounded_channel();

    let mut strategy = BasicMarketMaker::new(
        config,
        risk_manager,
        event_sender,
        order_sender,
    );

    // 测试初始化
    assert!(strategy.initialize().await.is_ok());
    
    // 测试启动
    assert!(strategy.start().await.is_ok());
    assert!(matches!(strategy.get_state(), StrategyState::Running));
    
    // 验证启动事件
    if let Some(event) = event_receiver.recv().await {
        assert!(matches!(event, hft_system::strategy_engine::market_making::StrategyEvent::Started));
    }

    // 测试暂停
    assert!(strategy.pause().await.is_ok());
    assert!(matches!(strategy.get_state(), StrategyState::Paused));

    // 测试恢复
    assert!(strategy.resume().await.is_ok());
    assert!(matches!(strategy.get_state(), StrategyState::Running));

    // 测试停止
    assert!(strategy.stop().await.is_ok());
    assert!(matches!(strategy.get_state(), StrategyState::Stopped));
    
    // 验证停止事件
    if let Some(event) = event_receiver.recv().await {
        assert!(matches!(event, hft_system::strategy_engine::market_making::StrategyEvent::Stopped));
    }
}

/// 测试报价计算
#[tokio::test]
async fn test_quote_calculation() {
    let config = MarketMakingConfig {
        base_spread_bps: 20, // 0.2%
        order_size: Decimal::new(1, 1), // 0.1
        ..Default::default()
    };
    let risk_manager = Arc::new(MockRiskManager::new());
    let (event_sender, _event_receiver) = mpsc::unbounded_channel();
    let (order_sender, _order_receiver) = mpsc::unbounded_channel();

    let strategy = BasicMarketMaker::new(
        config,
        risk_manager,
        event_sender,
        order_sender,
    );

    let market_snapshot = MarketSnapshot {
        last_price: Decimal::new(50000, 0),
        best_bid: Decimal::new(49990, 0),
        best_ask: Decimal::new(50010, 0),
        mid_price: Decimal::new(50000, 0),
        spread: Decimal::new(20, 0),
        volume_24h: Decimal::new(1000, 0),
        volatility: Decimal::new(20, 2), // 20%
        timestamp: Utc::now(),
    };

    let quote = strategy.calculate_quote(&market_snapshot).await.unwrap();
    
    // 验证报价结构
    assert!(quote.bid_price < quote.mid_price);
    assert!(quote.ask_price > quote.mid_price);
    assert_eq!(quote.mid_price, Decimal::new(50000, 0));
    assert_eq!(quote.bid_size, Decimal::new(1, 1));
    assert_eq!(quote.ask_size, Decimal::new(1, 1));
    
    // 验证价差大约为 0.2%
    let expected_spread = Decimal::new(50000, 0) * Decimal::new(20, 4); // 0.2% of 50000
    let actual_spread = quote.ask_price - quote.bid_price;
    let spread_diff = (actual_spread - expected_spread).abs();
    assert!(spread_diff < Decimal::new(1, 0)); // 允许1 USDT的误差
}

/// 测试市场数据处理
#[tokio::test]
async fn test_market_data_processing() {
    let config = MarketMakingConfig::default();
    let risk_manager = Arc::new(MockRiskManager::new());
    let (event_sender, _event_receiver) = mpsc::unbounded_channel();
    let (order_sender, _order_receiver) = mpsc::unbounded_channel();

    let mut strategy = BasicMarketMaker::new(
        config.clone(),
        risk_manager,
        event_sender,
        order_sender,
    );

    // 启动策略
    assert!(strategy.initialize().await.is_ok());
    assert!(strategy.start().await.is_ok());

    // 创建测试ticker数据
    let ticker = Ticker {
        symbol: config.symbol.clone(),
        last_price: Decimal::new(50000, 0),
        bid_price: Decimal::new(49990, 0),
        ask_price: Decimal::new(50010, 0),
        volume_24h: Decimal::new(1000, 0),
        price_change_24h: Decimal::new(100, 0),
        timestamp: Utc::now(),
    };

    // 处理市场数据
    let market_event = MarketDataEvent::Ticker(ticker);
    assert!(strategy.on_market_data(market_event).await.is_ok());

    // 验证策略状态仍然正常
    assert!(matches!(strategy.get_state(), StrategyState::Running));
}

/// 测试订单更新处理
#[tokio::test]
async fn test_order_update_processing() {
    let config = MarketMakingConfig::default();
    let risk_manager = Arc::new(MockRiskManager::new());
    let (event_sender, mut event_receiver) = mpsc::unbounded_channel();
    let (order_sender, _order_receiver) = mpsc::unbounded_channel();

    let mut strategy = BasicMarketMaker::new(
        config.clone(),
        risk_manager,
        event_sender,
        order_sender,
    );

    // 创建测试订单
    let order = Order {
        id: OrderId::new(),
        symbol: config.symbol.clone(),
        side: Side::Buy,
        order_type: OrderType::Limit,
        quantity: Decimal::new(1, 1),
        price: Some(Decimal::new(49990, 0)),
        status: OrderStatus::Filled,
        filled_quantity: Decimal::new(1, 1),
        average_fill_price: Decimal::new(49990, 0),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        exchange: config.symbol.exchange,
    };

    // 处理订单更新
    assert!(strategy.on_order_update(order.clone()).await.is_ok());

    // 验证成交事件
    if let Some(event) = event_receiver.recv().await {
        assert!(matches!(event, hft_system::strategy_engine::market_making::StrategyEvent::OrderFilled(_, _, _)));
    }

    // 验证指标更新
    let metrics = strategy.get_metrics();
    assert_eq!(metrics.total_trades, 1);
}

/// 测试高级做市策略
#[tokio::test]
async fn test_advanced_market_maker() {
    let config = AdvancedMarketMakingConfig::default();
    let risk_manager = Arc::new(MockRiskManager::new());
    let (event_sender, _event_receiver) = mpsc::unbounded_channel();
    let (order_sender, _order_receiver) = mpsc::unbounded_channel();

    let mut strategy = AdvancedMarketMaker::new(
        config,
        risk_manager,
        event_sender,
        order_sender,
    );

    // 测试初始化和启动
    assert!(strategy.initialize().await.is_ok());
    assert!(strategy.start().await.is_ok());
    assert!(matches!(strategy.get_state(), StrategyState::Running));

    // 测试停止
    assert!(strategy.stop().await.is_ok());
    assert!(matches!(strategy.get_state(), StrategyState::Stopped));
}

/// 测试风险控制器
#[tokio::test]
async fn test_risk_controller() {
    let config = RiskControlConfig {
        max_order_size: Decimal::new(1, 0),
        max_position_limit: Decimal::new(5, 0),
        max_daily_loss: Decimal::new(1000, 0),
        ..Default::default()
    };

    let controller = StrategyRiskController::new(config);

    // 测试正常订单
    let normal_order = Order {
        id: OrderId::new(),
        symbol: Symbol::new("BTC", "USDT", Exchange::Binance),
        side: Side::Buy,
        order_type: OrderType::Limit,
        quantity: Decimal::new(5, 1), // 0.5
        price: Some(Decimal::new(50000, 0)),
        status: OrderStatus::Pending,
        filled_quantity: Decimal::ZERO,
        average_fill_price: Decimal::ZERO,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        exchange: Exchange::Binance,
    };

    let result = controller.check_order_risk(&normal_order).await;
    assert!(result.passed);
    assert_eq!(result.risk_level, RiskLevel::Low);

    // 测试超大订单
    let large_order = Order {
        quantity: Decimal::new(2, 0), // 超过限制
        ..normal_order.clone()
    };

    let result = controller.check_order_risk(&large_order).await;
    assert!(!result.passed);
    assert_eq!(result.risk_level, RiskLevel::High);
}

/// 测试仓位控制器
#[tokio::test]
async fn test_position_controller() {
    let controller = PositionController::new(
        Decimal::new(10, 0), // max position: 10
        Decimal::new(2, 1),  // adjustment speed: 20%
    );

    // 设置目标仓位
    controller.set_target_position(Decimal::new(5, 0)).await;
    assert_eq!(controller.get_target_position().await, Decimal::new(5, 0));

    // 测试超出限制的目标仓位
    controller.set_target_position(Decimal::new(15, 0)).await;
    assert_eq!(controller.get_target_position().await, Decimal::new(10, 0)); // 应该被限制

    // 测试仓位调整计算
    controller.update_current_position(Decimal::ZERO).await;
    let adjustment = controller.calculate_position_adjustment().await;
    assert_eq!(adjustment, Decimal::new(2, 0)); // 20% of max position

    // 测试仓位限制检查
    assert!(controller.is_position_within_limits(Decimal::new(8, 0)).await);
    assert!(!controller.is_position_within_limits(Decimal::new(12, 0)).await);
}

/// 测试报价风险检查
#[tokio::test]
async fn test_quote_risk_check() {
    let config = RiskControlConfig {
        min_spread_bps: 5,  // 0.05%
        max_spread_bps: 50, // 0.5%
        ..Default::default()
    };

    let controller = StrategyRiskController::new(config);

    // 测试正常报价
    let normal_quote = Quote {
        bid_price: Decimal::new(49990, 0),
        bid_size: Decimal::new(1, 1),
        ask_price: Decimal::new(50010, 0),
        ask_size: Decimal::new(1, 1),
        mid_price: Decimal::new(50000, 0),
        spread: Decimal::new(20, 0), // 20 USDT = 0.04%
        timestamp: Utc::now(),
    };

    let result = controller.check_quote_risk(&normal_quote).await;
    assert!(result.passed);

    // 测试价差过小的报价
    let narrow_quote = Quote {
        spread: Decimal::new(1, 0), // 1 USDT = 0.002%
        ..normal_quote.clone()
    };

    let result = controller.check_quote_risk(&narrow_quote).await;
    assert!(!result.passed);
    assert_eq!(result.risk_level, RiskLevel::High);
}

/// 测试紧急停止功能
#[tokio::test]
async fn test_emergency_stop() {
    let config = RiskControlConfig::default();
    let controller = StrategyRiskController::new(config);

    // 初始状态
    assert!(!controller.is_emergency_stopped().await);

    // 触发紧急停止
    controller.trigger_emergency_stop("Test emergency".to_string()).await;
    assert!(controller.is_emergency_stopped().await);

    // 测试紧急停止状态下的订单检查
    let order = Order {
        id: OrderId::new(),
        symbol: Symbol::new("BTC", "USDT", Exchange::Binance),
        side: Side::Buy,
        order_type: OrderType::Limit,
        quantity: Decimal::new(1, 1),
        price: Some(Decimal::new(50000, 0)),
        status: OrderStatus::Pending,
        filled_quantity: Decimal::ZERO,
        average_fill_price: Decimal::ZERO,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        exchange: Exchange::Binance,
    };

    let result = controller.check_order_risk(&order).await;
    assert!(!result.passed);
    assert_eq!(result.risk_level, RiskLevel::Critical);

    // 解除紧急停止
    controller.release_emergency_stop().await;
    assert!(!controller.is_emergency_stopped().await);
}

/// 测试交易统计更新
#[tokio::test]
async fn test_trading_statistics() {
    let config = RiskControlConfig::default();
    let controller = StrategyRiskController::new(config);

    // 更新盈利交易
    controller.update_trading_stats(Decimal::new(100, 0), true).await;
    
    let stats = controller.get_risk_statistics().await;
    assert_eq!(stats.daily_pnl, Decimal::new(100, 0));

    // 更新亏损交易
    controller.update_trading_stats(Decimal::new(-50, 0), false).await;
    
    let stats = controller.get_risk_statistics().await;
    assert_eq!(stats.daily_pnl, Decimal::new(50, 0));
}

/// 测试订单簿数据处理
#[tokio::test]
async fn test_order_book_processing() {
    let config = MarketMakingConfig::default();
    let risk_manager = Arc::new(MockRiskManager::new());
    let (event_sender, _event_receiver) = mpsc::unbounded_channel();
    let (order_sender, _order_receiver) = mpsc::unbounded_channel();

    let mut strategy = BasicMarketMaker::new(
        config.clone(),
        risk_manager,
        event_sender,
        order_sender,
    );

    // 启动策略
    assert!(strategy.initialize().await.is_ok());
    assert!(strategy.start().await.is_ok());

    // 创建测试订单簿
    let order_book = OrderBook {
        symbol: config.symbol.clone(),
        bids: vec![
            OrderBookLevel { price: Decimal::new(49990, 0), quantity: Decimal::new(1, 0) },
            OrderBookLevel { price: Decimal::new(49980, 0), quantity: Decimal::new(2, 0) },
        ],
        asks: vec![
            OrderBookLevel { price: Decimal::new(50010, 0), quantity: Decimal::new(1, 0) },
            OrderBookLevel { price: Decimal::new(50020, 0), quantity: Decimal::new(2, 0) },
        ],
        timestamp: Utc::now(),
    };

    // 处理订单簿数据
    let market_event = MarketDataEvent::OrderBook(order_book);
    assert!(strategy.on_market_data(market_event).await.is_ok());

    // 验证策略状态
    assert!(matches!(strategy.get_state(), StrategyState::Running));
}

/// 测试交易数据处理
#[tokio::test]
async fn test_trade_processing() {
    let config = MarketMakingConfig::default();
    let risk_manager = Arc::new(MockRiskManager::new());
    let (event_sender, _event_receiver) = mpsc::unbounded_channel();
    let (order_sender, _order_receiver) = mpsc::unbounded_channel();

    let mut strategy = BasicMarketMaker::new(
        config.clone(),
        risk_manager,
        event_sender,
        order_sender,
    );

    // 创建测试交易数据
    let trade = Trade {
        symbol: config.symbol.clone(),
        price: Decimal::new(50000, 0),
        quantity: Decimal::new(1, 1),
        side: Side::Buy,
        timestamp: Utc::now(),
        trade_id: "test_trade_123".to_string(),
    };

    // 处理交易数据
    let market_event = MarketDataEvent::Trade(trade);
    assert!(strategy.on_market_data(market_event).await.is_ok());
}

/// 性能测试：报价计算
#[tokio::test]
async fn test_quote_calculation_performance() {
    let config = MarketMakingConfig::default();
    let risk_manager = Arc::new(MockRiskManager::new());
    let (event_sender, _event_receiver) = mpsc::unbounded_channel();
    let (order_sender, _order_receiver) = mpsc::unbounded_channel();

    let strategy = BasicMarketMaker::new(
        config,
        risk_manager,
        event_sender,
        order_sender,
    );

    let market_snapshot = MarketSnapshot {
        last_price: Decimal::new(50000, 0),
        best_bid: Decimal::new(49990, 0),
        best_ask: Decimal::new(50010, 0),
        mid_price: Decimal::new(50000, 0),
        spread: Decimal::new(20, 0),
        volume_24h: Decimal::new(1000, 0),
        volatility: Decimal::new(20, 2),
        timestamp: Utc::now(),
    };

    let start = std::time::Instant::now();
    
    // 计算1000次报价
    for _ in 0..1000 {
        let _quote = strategy.calculate_quote(&market_snapshot).await.unwrap();
    }
    
    let duration = start.elapsed();
    println!("1000 quote calculations took: {:?}", duration);
    
    // 性能要求：1000次计算应该在100ms内完成
    assert!(duration.as_millis() < 100);
}

/// 并发测试：多个策略实例
#[tokio::test]
async fn test_concurrent_strategies() {
    use tokio::task::JoinSet;
    
    let mut set = JoinSet::new();
    
    // 启动10个并发策略
    for i in 0..10 {
        set.spawn(async move {
            let config = MarketMakingConfig::default();
            let risk_manager = Arc::new(MockRiskManager::new());
            let (event_sender, _event_receiver) = mpsc::unbounded_channel();
            let (order_sender, _order_receiver) = mpsc::unbounded_channel();

            let mut strategy = BasicMarketMaker::new(
                config,
                risk_manager,
                event_sender,
                order_sender,
            );

            // 初始化并启动策略
            strategy.initialize().await.unwrap();
            strategy.start().await.unwrap();
            
            // 模拟一些操作
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            
            strategy.stop().await.unwrap();
            
            format!("Strategy {} completed", i)
        });
    }
    
    let mut results = Vec::new();
    while let Some(result) = set.join_next().await {
        results.push(result.unwrap());
    }
    
    assert_eq!(results.len(), 10);
    println!("Concurrent strategies test completed with {} results", results.len());
}

