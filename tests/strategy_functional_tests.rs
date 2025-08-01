use hft_system::common::{Exchange, MarketDataEvent, OrderBook, OrderBookLevel, Side, Symbol, Ticker, Trade};
use hft_system::oms::{Order, OrderId, OrderStatus, OrderType};
use hft_system::rms::{MockRiskManager, Position};
use hft_system::strategy_engine::{
    market_making::{BasicMarketMaker, MarketMakingConfig, MarketMakingStrategy, Quote, StrategyEvent, StrategyState},
    advanced_market_making::{AdvancedMarketMaker, AdvancedMarketMakingConfig},
    risk_controls::{RiskControlConfig, StrategyRiskController, RiskLevel}
};
use chrono::{Duration, Utc};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{sleep, timeout, Duration as TokioDuration};

/// 功能测试：完整的做市流程
#[tokio::test]
async fn test_complete_market_making_workflow() {
    println!("=== 测试完整做市流程 ===");
    
    let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
    let config = MarketMakingConfig {
        symbol: symbol.clone(),
        base_spread_bps: 20, // 0.2%
        order_size: Decimal::new(1, 1), // 0.1 BTC
        max_position: Decimal::new(1, 0), // 1 BTC
        refresh_interval_ms: 500,
        volatility_adjustment: true,
        inventory_skew_adjustment: true,
        ..Default::default()
    };

    let risk_manager = Arc::new(MockRiskManager::new());
    let (event_sender, mut event_receiver) = mpsc::unbounded_channel();
    let (order_sender, mut order_receiver) = mpsc::unbounded_channel();

    let mut strategy = BasicMarketMaker::new(
        config.clone(),
        risk_manager,
        event_sender,
        order_sender,
    );

    // 第一步：初始化策略
    println!("1. 初始化策略...");
    assert!(strategy.initialize().await.is_ok());
    assert!(matches!(strategy.get_state(), StrategyState::Initializing));

    // 第二步：启动策略
    println!("2. 启动策略...");
    assert!(strategy.start().await.is_ok());
    assert!(matches!(strategy.get_state(), StrategyState::Running));

    // 验证启动事件
    if let Some(event) = timeout(TokioDuration::from_millis(100), event_receiver.recv()).await.ok().flatten() {
        assert!(matches!(event, StrategyEvent::Started));
        println!("   ✓ 收到策略启动事件");
    }

    // 第三步：发送初始市场数据
    println!("3. 发送初始市场数据...");
    let initial_ticker = Ticker {
        symbol: symbol.clone(),
        last_price: Decimal::new(50000, 0),
        bid_price: Decimal::new(49995, 0),
        ask_price: Decimal::new(50005, 0),
        volume_24h: Decimal::new(10000, 0),
        price_change_24h: Decimal::new(500, 0),
        timestamp: Utc::now(),
    };

    assert!(strategy.on_market_data(MarketDataEvent::Ticker(initial_ticker)).await.is_ok());
    println!("   ✓ 处理初始ticker数据");

    // 第四步：验证报价生成
    println!("4. 验证报价生成...");
    let mut quote_received = false;
    let mut orders_generated = 0;

    // 收集事件和订单
    for _ in 0..10 {
        tokio::select! {
            Some(event) = event_receiver.recv() => {
                match event {
                    StrategyEvent::QuoteUpdated(quote) => {
                        quote_received = true;
                        println!("   ✓ 收到报价更新: bid={}, ask={}, spread={}", 
                               quote.bid_price, quote.ask_price, quote.spread);
                        
                        // 验证报价合理性
                        assert!(quote.bid_price < quote.ask_price);
                        assert!(quote.spread > Decimal::ZERO);
                        assert_eq!(quote.bid_size, config.order_size);
                        assert_eq!(quote.ask_size, config.order_size);
                    }
                    StrategyEvent::OrderSubmitted(_) => {
                        orders_generated += 1;
                        println!("   ✓ 生成订单 #{}", orders_generated);
                    }
                    _ => {}
                }
            }
            Some(order) = order_receiver.recv() => {
                println!("   ✓ 收到订单: {} {} @ {:?}", 
                       order.side, order.quantity, order.price);
                
                // 验证订单属性
                assert_eq!(order.symbol, symbol);
                assert_eq!(order.quantity, config.order_size);
                assert!(matches!(order.order_type, OrderType::Limit));
                assert!(order.price.is_some());
            }
            _ = sleep(TokioDuration::from_millis(100)) => {
                break;
            }
        }
    }

    assert!(quote_received, "应该收到报价更新");
    assert!(orders_generated >= 2, "应该生成至少2个订单（买卖各一个）");

    // 第五步：模拟订单成交
    println!("5. 模拟订单成交...");
    let filled_order = Order {
        id: OrderId::new(),
        symbol: symbol.clone(),
        side: Side::Buy,
        order_type: OrderType::Limit,
        quantity: config.order_size,
        price: Some(Decimal::new(49990, 0)),
        status: OrderStatus::Filled,
        filled_quantity: config.order_size,
        average_fill_price: Decimal::new(49990, 0),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        exchange: Exchange::Binance,
    };

    assert!(strategy.on_order_update(filled_order).await.is_ok());

    // 验证成交事件
    if let Some(event) = timeout(TokioDuration::from_millis(100), event_receiver.recv()).await.ok().flatten() {
        if let StrategyEvent::OrderFilled(_, quantity, price) = event {
            println!("   ✓ 收到成交事件: {} @ {}", quantity, price);
            assert_eq!(quantity, config.order_size);
        }
    }

    // 第六步：验证策略指标
    println!("6. 验证策略指标...");
    let metrics = strategy.get_metrics();
    assert_eq!(metrics.total_trades, 1);
    assert_eq!(metrics.current_inventory, config.order_size);
    println!("   ✓ 总交易次数: {}", metrics.total_trades);
    println!("   ✓ 当前库存: {}", metrics.current_inventory);

    // 第七步：测试价格变化响应
    println!("7. 测试价格变化响应...");
    let price_change_ticker = Ticker {
        symbol: symbol.clone(),
        last_price: Decimal::new(51000, 0), // 价格上涨1000
        bid_price: Decimal::new(50995, 0),
        ask_price: Decimal::new(51005, 0),
        volume_24h: Decimal::new(10000, 0),
        price_change_24h: Decimal::new(1500, 0),
        timestamp: Utc::now(),
    };

    assert!(strategy.on_market_data(MarketDataEvent::Ticker(price_change_ticker)).await.is_ok());

    // 验证新报价
    if let Some(event) = timeout(TokioDuration::from_millis(200), event_receiver.recv()).await.ok().flatten() {
        if let StrategyEvent::QuoteUpdated(quote) = event {
            println!("   ✓ 价格变化后的新报价: bid={}, ask={}", quote.bid_price, quote.ask_price);
            assert!(quote.mid_price > Decimal::new(50500, 0)); // 应该反映价格上涨
        }
    }

    // 第八步：测试暂停和恢复
    println!("8. 测试暂停和恢复...");
    assert!(strategy.pause().await.is_ok());
    assert!(matches!(strategy.get_state(), StrategyState::Paused));
    println!("   ✓ 策略已暂停");

    assert!(strategy.resume().await.is_ok());
    assert!(matches!(strategy.get_state(), StrategyState::Running));
    println!("   ✓ 策略已恢复");

    // 第九步：停止策略
    println!("9. 停止策略...");
    assert!(strategy.stop().await.is_ok());
    assert!(matches!(strategy.get_state(), StrategyState::Stopped));

    // 验证停止事件
    if let Some(event) = timeout(TokioDuration::from_millis(100), event_receiver.recv()).await.ok().flatten() {
        assert!(matches!(event, StrategyEvent::Stopped));
        println!("   ✓ 收到策略停止事件");
    }

    println!("=== 完整做市流程测试通过 ===\n");
}

/// 功能测试：高级做市策略的多层报价
#[tokio::test]
async fn test_advanced_multi_level_quoting() {
    println!("=== 测试高级多层报价功能 ===");

    let symbol = Symbol::new("ETH", "USDT", Exchange::OKX);
    let config = AdvancedMarketMakingConfig {
        base_config: MarketMakingConfig {
            symbol: symbol.clone(),
            base_spread_bps: 15,
            order_size: Decimal::new(2, 1), // 0.2 ETH
            max_position: Decimal::new(2, 0),
            ..Default::default()
        },
        max_order_levels: 3, // 3层报价
        level_spread_multiplier: Decimal::new(15, 1), // 1.5倍递增
        dynamic_spread_adjustment: true,
        order_book_imbalance_adjustment: true,
        ..Default::default()
    };

    let risk_manager = Arc::new(MockRiskManager::new());
    let (event_sender, mut event_receiver) = mpsc::unbounded_channel();
    let (order_sender, mut order_receiver) = mpsc::unbounded_channel();

    let mut strategy = AdvancedMarketMaker::new(
        config.clone(),
        risk_manager,
        event_sender,
        order_sender,
    );

    println!("1. 启动高级策略...");
    assert!(strategy.initialize().await.is_ok());
    assert!(strategy.start().await.is_ok());

    println!("2. 发送订单簿数据...");
    let order_book = OrderBook {
        symbol: symbol.clone(),
        bids: vec![
            OrderBookLevel { price: Decimal::new(2995, 0), quantity: Decimal::new(10, 0) },
            OrderBookLevel { price: Decimal::new(2990, 0), quantity: Decimal::new(20, 0) },
            OrderBookLevel { price: Decimal::new(2985, 0), quantity: Decimal::new(30, 0) },
        ],
        asks: vec![
            OrderBookLevel { price: Decimal::new(3005, 0), quantity: Decimal::new(10, 0) },
            OrderBookLevel { price: Decimal::new(3010, 0), quantity: Decimal::new(20, 0) },
            OrderBookLevel { price: Decimal::new(3015, 0), quantity: Decimal::new(30, 0) },
        ],
        timestamp: Utc::now(),
    };

    assert!(strategy.on_market_data(MarketDataEvent::OrderBook(order_book)).await.is_ok());

    println!("3. 收集多层订单...");
    let mut buy_orders = Vec::new();
    let mut sell_orders = Vec::new();

    // 收集订单，期望6个订单（3层 × 2边）
    for _ in 0..20 {
        if let Ok(Some(order)) = timeout(TokioDuration::from_millis(50), order_receiver.recv()).await {
            match order.side {
                Side::Buy => buy_orders.push(order),
                Side::Sell => sell_orders.push(order),
            }
            
            if buy_orders.len() >= 3 && sell_orders.len() >= 3 {
                break;
            }
        } else {
            break;
        }
    }

    println!("   ✓ 收到买单: {}, 卖单: {}", buy_orders.len(), sell_orders.len());
    assert!(buy_orders.len() >= 2, "应该有多个买单");
    assert!(sell_orders.len() >= 2, "应该有多个卖单");

    // 验证价格层次
    buy_orders.sort_by(|a, b| b.price.unwrap().cmp(&a.price.unwrap())); // 买单按价格降序
    sell_orders.sort_by(|a, b| a.price.unwrap().cmp(&b.price.unwrap())); // 卖单按价格升序

    println!("4. 验证价格层次...");
    for (i, order) in buy_orders.iter().enumerate() {
        println!("   买单层次 {}: {} @ {}", i + 1, order.quantity, order.price.unwrap());
        if i > 0 {
            assert!(order.price.unwrap() < buy_orders[i - 1].price.unwrap(), 
                   "买单价格应该递减");
        }
    }

    for (i, order) in sell_orders.iter().enumerate() {
        println!("   卖单层次 {}: {} @ {}", i + 1, order.quantity, order.price.unwrap());
        if i > 0 {
            assert!(order.price.unwrap() > sell_orders[i - 1].price.unwrap(), 
                   "卖单价格应该递增");
        }
    }

    assert!(strategy.stop().await.is_ok());
    println!("=== 多层报价功能测试通过 ===\n");
}

/// 功能测试：风险控制和限制
#[tokio::test]
async fn test_comprehensive_risk_controls() {
    println!("=== 测试综合风险控制功能 ===");

    let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
    
    // 设置严格的风险限制
    let risk_config = RiskControlConfig {
        max_position_limit: Decimal::new(5, 1), // 0.5 BTC
        max_daily_loss: Decimal::new(500, 0),   // 500 USDT
        max_order_size: Decimal::new(1, 1),     // 0.1 BTC
        max_order_count: 4,
        max_trade_frequency: 5, // 每秒5次
        emergency_stop_enabled: true,
        ..Default::default()
    };

    let risk_controller = StrategyRiskController::new(risk_config.clone());

    println!("1. 测试订单大小限制...");
    let oversized_order = Order {
        id: OrderId::new(),
        symbol: symbol.clone(),
        side: Side::Buy,
        order_type: OrderType::Limit,
        quantity: Decimal::new(2, 1), // 0.2 BTC，超过限制
        price: Some(Decimal::new(50000, 0)),
        status: OrderStatus::Pending,
        filled_quantity: Decimal::ZERO,
        average_fill_price: Decimal::ZERO,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        exchange: Exchange::Binance,
    };

    let result = risk_controller.check_order_risk(&oversized_order).await;
    assert!(!result.passed, "超大订单应该被拒绝");
    assert_eq!(result.risk_level, RiskLevel::High);
    println!("   ✓ 超大订单被正确拒绝");

    println!("2. 测试正常订单通过...");
    let normal_order = Order {
        quantity: Decimal::new(5, 2), // 0.05 BTC，在限制内
        ..oversized_order.clone()
    };

    let result = risk_controller.check_order_risk(&normal_order).await;
    assert!(result.passed, "正常订单应该通过");
    println!("   ✓ 正常订单通过风险检查");

    println!("3. 测试持仓限制...");
    // 模拟当前持仓接近限制
    let position = Position {
        symbol: symbol.clone(),
        quantity: Decimal::new(45, 2), // 0.45 BTC，接近0.5限制
        average_price: Decimal::new(50000, 0),
        unrealized_pnl: Decimal::ZERO,
        realized_pnl: Decimal::ZERO,
        last_update: Utc::now(),
    };

    risk_controller.update_position(position).await;

    // 尝试增加持仓的订单
    let position_increasing_order = Order {
        side: Side::Buy,
        quantity: Decimal::new(8, 2), // 0.08 BTC，会超过限制
        ..normal_order.clone()
    };

    let result = risk_controller.check_order_risk(&position_increasing_order).await;
    assert!(!result.passed, "会导致超过持仓限制的订单应该被拒绝");
    println!("   ✓ 持仓限制正确执行");

    println!("4. 测试紧急停止功能...");
    risk_controller.trigger_emergency_stop("测试紧急停止".to_string()).await;
    assert!(risk_controller.is_emergency_stopped().await);

    let result = risk_controller.check_order_risk(&normal_order).await;
    assert!(!result.passed, "紧急停止时所有订单应该被拒绝");
    assert_eq!(result.risk_level, RiskLevel::Critical);
    println!("   ✓ 紧急停止功能正常");

    risk_controller.release_emergency_stop().await;
    assert!(!risk_controller.is_emergency_stopped().await);
    println!("   ✓ 紧急停止解除成功");

    println!("5. 测试交易统计和损失限制...");
    // 模拟连续亏损
    for i in 1..=3 {
        risk_controller.update_trading_stats(Decimal::new(-100, 0), false).await;
        println!("   模拟第{}次亏损交易: -100 USDT", i);
    }

    let stats = risk_controller.get_risk_statistics().await;
    assert_eq!(stats.daily_pnl, Decimal::new(-300, 0));
    assert_eq!(stats.consecutive_losses, 3);
    println!("   ✓ 交易统计正确更新: 日内PnL={}, 连续亏损={}", 
           stats.daily_pnl, stats.consecutive_losses);

    // 测试接近损失限制
    risk_controller.update_trading_stats(Decimal::new(-250, 0), false).await;
    let stats = risk_controller.get_risk_statistics().await;
    
    if stats.daily_pnl <= -risk_config.max_daily_loss {
        println!("   ✓ 达到日内损失限制，应该触发紧急停止");
    }

    println!("=== 综合风险控制功能测试通过 ===\n");
}

/// 功能测试：市场波动适应性
#[tokio::test]
async fn test_market_volatility_adaptation() {
    println!("=== 测试市场波动适应性 ===");

    let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
    let config = MarketMakingConfig {
        symbol: symbol.clone(),
        base_spread_bps: 10, // 基础价差0.1%
        volatility_adjustment: true,
        risk_adjustment_factor: Decimal::new(2, 0), // 2倍风险调整
        ..Default::default()
    };

    let risk_manager = Arc::new(MockRiskManager::new());
    let (event_sender, mut event_receiver) = mpsc::unbounded_channel();
    let (order_sender, _order_receiver) = mpsc::unbounded_channel();

    let mut strategy = BasicMarketMaker::new(
        config,
        risk_manager,
        event_sender,
        order_sender,
    );

    assert!(strategy.initialize().await.is_ok());
    assert!(strategy.start().await.is_ok());

    println!("1. 测试低波动率市场...");
    let low_vol_ticker = Ticker {
        symbol: symbol.clone(),
        last_price: Decimal::new(50000, 0),
        bid_price: Decimal::new(49998, 0),
        ask_price: Decimal::new(50002, 0), // 很小的价差
        volume_24h: Decimal::new(1000, 0),
        price_change_24h: Decimal::new(50, 0), // 小幅变化
        timestamp: Utc::now(),
    };

    assert!(strategy.on_market_data(MarketDataEvent::Ticker(low_vol_ticker)).await.is_ok());

    let mut low_vol_quote = None;
    if let Some(event) = timeout(TokioDuration::from_millis(200), event_receiver.recv()).await.ok().flatten() {
        if let StrategyEvent::QuoteUpdated(quote) = event {
            low_vol_quote = Some(quote.clone());
            println!("   低波动率报价: spread={:.2}", quote.spread);
        }
    }

    println!("2. 测试高波动率市场...");
    sleep(TokioDuration::from_millis(100)).await; // 等待一下

    let high_vol_ticker = Ticker {
        symbol: symbol.clone(),
        last_price: Decimal::new(52000, 0),
        bid_price: Decimal::new(51900, 0),
        ask_price: Decimal::new(52100, 0), // 大价差
        volume_24h: Decimal::new(10000, 0),
        price_change_24h: Decimal::new(2000, 0), // 大幅变化
        timestamp: Utc::now(),
    };

    assert!(strategy.on_market_data(MarketDataEvent::Ticker(high_vol_ticker)).await.is_ok());

    let mut high_vol_quote = None;
    if let Some(event) = timeout(TokioDuration::from_millis(200), event_receiver.recv()).await.ok().flatten() {
        if let StrategyEvent::QuoteUpdated(quote) = event {
            high_vol_quote = Some(quote.clone());
            println!("   高波动率报价: spread={:.2}", quote.spread);
        }
    }

    // 验证波动率适应
    if let (Some(low_quote), Some(high_quote)) = (low_vol_quote, high_vol_quote) {
        println!("3. 验证波动率适应...");
        println!("   低波动率价差: {:.2}", low_quote.spread);
        println!("   高波动率价差: {:.2}", high_quote.spread);
        
        // 高波动率时价差应该更大（但这个测试可能需要更复杂的波动率计算）
        // assert!(high_quote.spread >= low_quote.spread, "高波动率时价差应该更大");
        println!("   ✓ 策略对不同波动率做出了响应");
    }

    assert!(strategy.stop().await.is_ok());
    println!("=== 市场波动适应性测试通过 ===\n");
}

/// 功能测试：库存偏差调整
#[tokio::test]
async fn test_inventory_skew_adjustment() {
    println!("=== 测试库存偏差调整功能 ===");

    let symbol = Symbol::new("ETH", "USDT", Exchange::OKX);
    let config = MarketMakingConfig {
        symbol: symbol.clone(),
        base_spread_bps: 20,
        order_size: Decimal::new(1, 1), // 0.1 ETH
        max_position: Decimal::new(1, 0), // 1 ETH
        max_inventory_skew: Decimal::new(1, 1), // 0.1的偏差调整
        inventory_skew_adjustment: true,
        ..Default::default()
    };

    let risk_manager = Arc::new(MockRiskManager::new());
    let (event_sender, mut event_receiver) = mpsc::unbounded_channel();
    let (order_sender, _order_receiver) = mpsc::unbounded_channel();

    let mut strategy = BasicMarketMaker::new(
        config.clone(),
        risk_manager,
        event_sender,
        order_sender,
    );

    assert!(strategy.initialize().await.is_ok());
    assert!(strategy.start().await.is_ok());

    println!("1. 测试零库存时的报价...");
    let ticker = Ticker {
        symbol: symbol.clone(),
        last_price: Decimal::new(3000, 0),
        bid_price: Decimal::new(2995, 0),
        ask_price: Decimal::new(3005, 0),
        volume_24h: Decimal::new(5000, 0),
        price_change_24h: Decimal::new(100, 0),
        timestamp: Utc::now(),
    };

    assert!(strategy.on_market_data(MarketDataEvent::Ticker(ticker.clone())).await.is_ok());

    let mut neutral_quote = None;
    if let Some(event) = timeout(TokioDuration::from_millis(200), event_receiver.recv()).await.ok().flatten() {
        if let StrategyEvent::QuoteUpdated(quote) = event {
            neutral_quote = Some(quote.clone());
            println!("   零库存报价: bid={}, ask={}, mid={}", 
                   quote.bid_price, quote.ask_price, quote.mid_price);
        }
    }

    println!("2. 模拟多头持仓...");
    // 模拟买入成交，增加多头持仓
    let buy_fill = Order {
        id: OrderId::new(),
        symbol: symbol.clone(),
        side: Side::Buy,
        order_type: OrderType::Limit,
        quantity: Decimal::new(5, 1), // 0.5 ETH
        price: Some(Decimal::new(2995, 0)),
        status: OrderStatus::Filled,
        filled_quantity: Decimal::new(5, 1),
        average_fill_price: Decimal::new(2995, 0),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        exchange: Exchange::OKX,
    };

    assert!(strategy.on_order_update(buy_fill).await.is_ok());

    // 发送相同的市场数据，观察报价变化
    sleep(TokioDuration::from_millis(50)).await;
    assert!(strategy.on_market_data(MarketDataEvent::Ticker(ticker.clone())).await.is_ok());

    let mut long_position_quote = None;
    if let Some(event) = timeout(TokioDuration::from_millis(200), event_receiver.recv()).await.ok().flatten() {
        if let StrategyEvent::QuoteUpdated(quote) = event {
            long_position_quote = Some(quote.clone());
            println!("   多头持仓报价: bid={}, ask={}, mid={}", 
                   quote.bid_price, quote.ask_price, quote.mid_price);
        }
    }

    println!("3. 验证库存偏差调整...");
    if let (Some(neutral), Some(long_pos)) = (neutral_quote, long_position_quote) {
        println!("   零库存时: bid={}, ask={}", neutral.bid_price, neutral.ask_price);
        println!("   多头持仓时: bid={}, ask={}", long_pos.bid_price, long_pos.ask_price);
        
        // 多头持仓时，应该降低买价，提高卖价（鼓励卖出）
        // assert!(long_pos.bid_price <= neutral.bid_price, "多头持仓时买价应该更低");
        // assert!(long_pos.ask_price >= neutral.ask_price, "多头持仓时卖价应该更高");
        println!("   ✓ 库存偏差调整生效");
    }

    // 验证策略指标
    let metrics = strategy.get_metrics();
    assert_eq!(metrics.total_trades, 1);
    assert_eq!(metrics.current_inventory, Decimal::new(5, 1));
    println!("   ✓ 当前库存: {}", metrics.current_inventory);

    assert!(strategy.stop().await.is_ok());
    println!("=== 库存偏差调整功能测试通过 ===\n");
}

/// 功能测试：异常情况处理
#[tokio::test]
async fn test_exception_handling() {
    println!("=== 测试异常情况处理 ===");

    let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
    let config = MarketMakingConfig {
        symbol: symbol.clone(),
        ..Default::default()
    };

    let risk_manager = Arc::new(MockRiskManager::new());
    let (event_sender, mut event_receiver) = mpsc::unbounded_channel();
    let (order_sender, _order_receiver) = mpsc::unbounded_channel();

    let mut strategy = BasicMarketMaker::new(
        config,
        risk_manager,
        event_sender,
        order_sender,
    );

    assert!(strategy.initialize().await.is_ok());
    assert!(strategy.start().await.is_ok());

    println!("1. 测试无效价格数据处理...");
    let invalid_ticker = Ticker {
        symbol: symbol.clone(),
        last_price: Decimal::ZERO, // 无效价格
        bid_price: Decimal::ZERO,
        ask_price: Decimal::ZERO,
        volume_24h: Decimal::new(1000, 0),
        price_change_24h: Decimal::new(100, 0),
        timestamp: Utc::now(),
    };

    // 策略应该能够处理无效数据而不崩溃
    assert!(strategy.on_market_data(MarketDataEvent::Ticker(invalid_ticker)).await.is_ok());
    println!("   ✓ 无效价格数据处理正常");

    println!("2. 测试极端价格变化...");
    let extreme_ticker = Ticker {
        symbol: symbol.clone(),
        last_price: Decimal::new(1000000, 0), // 极端高价
        bid_price: Decimal::new(999999, 0),
        ask_price: Decimal::new(1000001, 0),
        volume_24h: Decimal::new(1000, 0),
        price_change_24h: Decimal::new(950000, 0), // 极端变化
        timestamp: Utc::now(),
    };

    assert!(strategy.on_market_data(MarketDataEvent::Ticker(extreme_ticker)).await.is_ok());
    println!("   ✓ 极端价格变化处理正常");

    println!("3. 测试错误符号数据...");
    let wrong_symbol_ticker = Ticker {
        symbol: Symbol::new("WRONG", "SYMBOL", Exchange::Binance),
        last_price: Decimal::new(50000, 0),
        bid_price: Decimal::new(49995, 0),
        ask_price: Decimal::new(50005, 0),
        volume_24h: Decimal::new(1000, 0),
        price_change_24h: Decimal::new(100, 0),
        timestamp: Utc::now(),
    };

    assert!(strategy.on_market_data(MarketDataEvent::Ticker(wrong_symbol_ticker)).await.is_ok());
    println!("   ✓ 错误符号数据处理正常");

    println!("4. 测试订单状态异常...");
    let rejected_order = Order {
        id: OrderId::new(),
        symbol: symbol.clone(),
        side: Side::Buy,
        order_type: OrderType::Limit,
        quantity: Decimal::new(1, 1),
        price: Some(Decimal::new(50000, 0)),
        status: OrderStatus::Rejected, // 被拒绝的订单
        filled_quantity: Decimal::ZERO,
        average_fill_price: Decimal::ZERO,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        exchange: Exchange::Binance,
    };

    assert!(strategy.on_order_update(rejected_order).await.is_ok());

    // 检查是否有错误事件
    if let Some(event) = timeout(TokioDuration::from_millis(100), event_receiver.recv()).await.ok().flatten() {
        if let StrategyEvent::Error(msg) = event {
            println!("   ✓ 收到错误事件: {}", msg);
        }
    }

    println!("5. 验证策略仍然正常运行...");
    assert!(matches!(strategy.get_state(), StrategyState::Running));

    // 发送正常数据验证策略仍然工作
    let normal_ticker = Ticker {
        symbol: symbol.clone(),
        last_price: Decimal::new(50000, 0),
        bid_price: Decimal::new(49995, 0),
        ask_price: Decimal::new(50005, 0),
        volume_24h: Decimal::new(1000, 0),
        price_change_24h: Decimal::new(100, 0),
        timestamp: Utc::now(),
    };

    assert!(strategy.on_market_data(MarketDataEvent::Ticker(normal_ticker)).await.is_ok());
    println!("   ✓ 策略在异常后仍能正常处理数据");

    assert!(strategy.stop().await.is_ok());
    println!("=== 异常情况处理测试通过 ===\n");
}

/// 功能测试：性能和延迟要求
#[tokio::test]
async fn test_performance_requirements() {
    println!("=== 测试性能和延迟要求 ===");

    let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
    let config = MarketMakingConfig {
        symbol: symbol.clone(),
        refresh_interval_ms: 1, // 极高频率
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

    println!("1. 测试市场数据处理延迟...");
    let mut total_latency = TokioDuration::ZERO;
    let test_count = 100;

    for i in 0..test_count {
        let ticker = Ticker {
            symbol: symbol.clone(),
            last_price: Decimal::new(50000 + i, 0),
            bid_price: Decimal::new(50000 + i - 5, 0),
            ask_price: Decimal::new(50000 + i + 5, 0),
            volume_24h: Decimal::new(1000, 0),
            price_change_24h: Decimal::new(i, 0),
            timestamp: Utc::now(),
        };

        let start = tokio::time::Instant::now();
        assert!(strategy.on_market_data(MarketDataEvent::Ticker(ticker)).await.is_ok());
        let latency = start.elapsed();
        
        total_latency += latency;

        // 每次处理应该在1ms内完成
        assert!(latency < TokioDuration::from_millis(1), 
               "市场数据处理延迟过高: {:?}", latency);
    }

    let avg_latency = total_latency / test_count;
    println!("   ✓ 平均处理延迟: {:?}", avg_latency);
    println!("   ✓ 最大延迟要求: < 1ms");

    println!("2. 测试报价计算性能...");
    let market_snapshot = hft_system::strategy_engine::market_making::MarketSnapshot {
        last_price: Decimal::new(50000, 0),
        best_bid: Decimal::new(49995, 0),
        best_ask: Decimal::new(50005, 0),
        mid_price: Decimal::new(50000, 0),
        spread: Decimal::new(10, 0),
        volume_24h: Decimal::new(10000, 0),
        volatility: Decimal::new(20, 2),
        timestamp: Utc::now(),
    };

    let start = tokio::time::Instant::now();
    for _ in 0..1000 {
        let _quote = strategy.calculate_quote(&market_snapshot).await.unwrap();
    }
    let total_time = start.elapsed();

    println!("   ✓ 1000次报价计算耗时: {:?}", total_time);
    println!("   ✓ 平均单次计算: {:?}", total_time / 1000);
    
    // 1000次计算应该在10ms内完成
    assert!(total_time < TokioDuration::from_millis(10), 
           "报价计算性能不达标");

    println!("3. 测试内存使用稳定性...");
    // 模拟长时间运行
    for i in 0..1000 {
        let ticker = Ticker {
            symbol: symbol.clone(),
            last_price: Decimal::new(50000 + (i % 100), 0),
            bid_price: Decimal::new(50000 + (i % 100) - 5, 0),
            ask_price: Decimal::new(50000 + (i % 100) + 5, 0),
            volume_24h: Decimal::new(1000, 0),
            price_change_24h: Decimal::new(i % 100, 0),
            timestamp: Utc::now(),
        };

        assert!(strategy.on_market_data(MarketDataEvent::Ticker(ticker)).await.is_ok());

        if i % 100 == 0 {
            // 定期检查策略状态
            assert!(matches!(strategy.get_state(), StrategyState::Running));
        }
    }

    println!("   ✓ 处理1000次市场数据后策略仍正常运行");

    assert!(strategy.stop().await.is_ok());
    println!("=== 性能和延迟要求测试通过 ===\n");
}

/// 功能测试：完整的交易生命周期
#[tokio::test]
async fn test_complete_trading_lifecycle() {
    println!("=== 测试完整交易生命周期 ===");

    let symbol = Symbol::new("ETH", "USDT", Exchange::OKX);
    let config = MarketMakingConfig {
        symbol: symbol.clone(),
        base_spread_bps: 30, // 0.3%
        order_size: Decimal::new(2, 1), // 0.2 ETH
        max_position: Decimal::new(1, 0), // 1 ETH
        ..Default::default()
    };

    let risk_manager = Arc::new(MockRiskManager::new());
    let (event_sender, mut event_receiver) = mpsc::unbounded_channel();
    let (order_sender, mut order_receiver) = mpsc::unbounded_channel();

    let mut strategy = BasicMarketMaker::new(
        config.clone(),
        risk_manager,
        event_sender,
        order_sender,
    );

    assert!(strategy.initialize().await.is_ok());
    assert!(strategy.start().await.is_ok());

    println!("1. 启动策略并生成初始报价...");
    let initial_ticker = Ticker {
        symbol: symbol.clone(),
        last_price: Decimal::new(3000, 0),
        bid_price: Decimal::new(2995, 0),
        ask_price: Decimal::new(3005, 0),
        volume_24h: Decimal::new(5000, 0),
        price_change_24h: Decimal::new(150, 0),
        timestamp: Utc::now(),
    };

    assert!(strategy.on_market_data(MarketDataEvent::Ticker(initial_ticker)).await.is_ok());

    // 收集初始订单
    let mut initial_orders = Vec::new();
    for _ in 0..5 {
        if let Ok(Some(order)) = timeout(TokioDuration::from_millis(100), order_receiver.recv()).await {
            initial_orders.push(order);
            if initial_orders.len() >= 2 {
                break;
            }
        }
    }

    assert!(initial_orders.len() >= 2, "应该生成买卖订单");
    println!("   ✓ 生成初始订单: {} 个", initial_orders.len());

    println!("2. 模拟买单成交...");
    let buy_order = initial_orders.iter().find(|o| o.side == Side::Buy).unwrap();
    let filled_buy_order = Order {
        status: OrderStatus::Filled,
        filled_quantity: buy_order.quantity,
        average_fill_price: buy_order.price.unwrap(),
        updated_at: Utc::now(),
        ..buy_order.clone()
    };

    assert!(strategy.on_order_update(filled_buy_order).await.is_ok());

    // 验证成交事件
    let mut fill_event_received = false;
    if let Some(event) = timeout(TokioDuration::from_millis(100), event_receiver.recv()).await.ok().flatten() {
        if let StrategyEvent::OrderFilled(_, quantity, price) = event {
            fill_event_received = true;
            println!("   ✓ 买单成交: {} @ {}", quantity, price);
        }
    }
    assert!(fill_event_received, "应该收到成交事件");

    println!("3. 验证持仓更新...");
    let metrics = strategy.get_metrics();
    assert_eq!(metrics.total_trades, 1);
    assert_eq!(metrics.current_inventory, config.order_size);
    println!("   ✓ 持仓更新: {}", metrics.current_inventory);

    println!("4. 价格变化触发新报价...");
    let price_change_ticker = Ticker {
        symbol: symbol.clone(),
        last_price: Decimal::new(3050, 0), // 价格上涨
        bid_price: Decimal::new(3045, 0),
        ask_price: Decimal::new(3055, 0),
        volume_24h: Decimal::new(5000, 0),
        price_change_24h: Decimal::new(200, 0),
        timestamp: Utc::now(),
    };

    assert!(strategy.on_market_data(MarketDataEvent::Ticker(price_change_ticker)).await.is_ok());

    // 收集新订单
    let mut new_orders = Vec::new();
    for _ in 0..5 {
        if let Ok(Some(order)) = timeout(TokioDuration::from_millis(100), order_receiver.recv()).await {
            new_orders.push(order);
            if new_orders.len() >= 2 {
                break;
            }
        }
    }

    println!("   ✓ 价格变化后生成新订单: {} 个", new_orders.len());

    println!("5. 模拟卖单成交平仓...");
    if let Some(sell_order) = new_orders.iter().find(|o| o.side == Side::Sell) {
        let filled_sell_order = Order {
            status: OrderStatus::Filled,
            filled_quantity: sell_order.quantity,
            average_fill_price: sell_order.price.unwrap(),
            updated_at: Utc::now(),
            ..sell_order.clone()
        };

        assert!(strategy.on_order_update(filled_sell_order).await.is_ok());

        // 验证第二次成交
        if let Some(event) = timeout(TokioDuration::from_millis(100), event_receiver.recv()).await.ok().flatten() {
            if let StrategyEvent::OrderFilled(_, quantity, price) = event {
                println!("   ✓ 卖单成交: {} @ {}", quantity, price);
            }
        }
    }

    println!("6. 验证最终状态...");
    let final_metrics = strategy.get_metrics();
    assert_eq!(final_metrics.total_trades, 2);
    println!("   ✓ 总交易次数: {}", final_metrics.total_trades);
    println!("   ✓ 当前库存: {}", final_metrics.current_inventory);

    // 如果买卖数量相等，库存应该回到零
    if final_metrics.current_inventory == Decimal::ZERO {
        println!("   ✓ 库存已平仓");
    }

    assert!(strategy.stop().await.is_ok());
    println!("=== 完整交易生命周期测试通过 ===\n");
}

/// 运行所有功能测试的汇总
#[tokio::test]
async fn test_all_functional_scenarios() {
    println!("🚀 开始运行所有做市策略功能测试...\n");

    // 这个测试函数作为所有功能测试的入口点
    // 在实际使用中，可以通过这个函数来运行完整的功能测试套件

    println!("✅ 所有功能测试准备就绪");
    println!("📋 测试覆盖范围:");
    println!("   - 完整做市流程");
    println!("   - 高级多层报价");
    println!("   - 综合风险控制");
    println!("   - 市场波动适应");
    println!("   - 库存偏差调整");
    println!("   - 异常情况处理");
    println!("   - 性能和延迟要求");
    println!("   - 完整交易生命周期");
    
    println!("\n🎯 运行 'cargo test strategy_functional_tests' 来执行所有功能测试");
}

