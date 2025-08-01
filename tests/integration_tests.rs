use hft_system::common::*;
use hft_system::data_ingestion::*;
use hft_system::strategy_engine::*;
use hft_system::oms::*;
use hft_system::rms::*;
use hft_system::monitoring::*;
use hft_system::persistence::*;
use hft_system::backtesting::*;
use chrono::Utc;
use rust_decimal::Decimal;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

#[cfg(test)]
mod integration_tests {
    use super::*;

    /// 测试完整的数据流：从数据接入到策略处理
    #[tokio::test]
    async fn test_complete_data_flow() {
        // 创建数据分发器
        let mut distributor = DataDistributor::new();
        
        // 创建市场数据接收器
        let (sender, mut receiver) = mpsc::unbounded_channel();
        distributor.add_subscriber(sender);
        
        // 创建测试用的市场数据
        let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
        let test_ticker = Ticker {
            symbol: symbol.clone(),
            bid_price: Decimal::from_str("49999.0").unwrap(),
            bid_quantity: Decimal::from_str("1.0").unwrap(),
            ask_price: Decimal::from_str("50001.0").unwrap(),
            ask_quantity: Decimal::from_str("2.0").unwrap(),
            last_price: Decimal::from_str("50000.0").unwrap(),
            volume_24h: Decimal::from_str("1000.0").unwrap(),
            price_change_24h: Decimal::from_str("100.0").unwrap(),
            timestamp: Utc::now(),
        };

        // 模拟数据发送
        let event = MarketDataEvent::Ticker(test_ticker);
        
        // 由于我们无法直接测试真实的 WebSocket 连接，
        // 这里我们测试数据处理逻辑
        let normalizer = DataNormalizer::new();
        let normalized = normalizer.normalize_market_data(
            event.clone(),
            Exchange::Binance,
            None,
        ).unwrap();
        
        assert!(normalized.quality_score > 0.9);
        assert_eq!(normalized.source_exchange, Exchange::Binance);
    }

    /// 测试订单簿管理和数据聚合
    #[tokio::test]
    async fn test_orderbook_aggregation() {
        let manager = OrderBookManager::new();
        let normalizer = DataNormalizer::new();
        let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
        
        // 创建多个交易所的订单簿数据
        let binance_book = OrderBook {
            symbol: Symbol::new("BTC", "USDT", Exchange::Binance),
            bids: vec![
                OrderBookLevel {
                    price: Decimal::from_str("50000.0").unwrap(),
                    quantity: Decimal::from_str("1.0").unwrap(),
                },
                OrderBookLevel {
                    price: Decimal::from_str("49999.0").unwrap(),
                    quantity: Decimal::from_str("2.0").unwrap(),
                },
            ],
            asks: vec![
                OrderBookLevel {
                    price: Decimal::from_str("50001.0").unwrap(),
                    quantity: Decimal::from_str("1.5").unwrap(),
                },
                OrderBookLevel {
                    price: Decimal::from_str("50002.0").unwrap(),
                    quantity: Decimal::from_str("2.5").unwrap(),
                },
            ],
            timestamp: Utc::now(),
        };

        let okx_book = OrderBook {
            symbol: Symbol::new("BTC", "USDT", Exchange::OKX),
            bids: vec![
                OrderBookLevel {
                    price: Decimal::from_str("50000.5").unwrap(),
                    quantity: Decimal::from_str("0.8").unwrap(),
                },
                OrderBookLevel {
                    price: Decimal::from_str("49999.5").unwrap(),
                    quantity: Decimal::from_str("1.8").unwrap(),
                },
            ],
            asks: vec![
                OrderBookLevel {
                    price: Decimal::from_str("50001.5").unwrap(),
                    quantity: Decimal::from_str("1.2").unwrap(),
                },
                OrderBookLevel {
                    price: Decimal::from_str("50002.5").unwrap(),
                    quantity: Decimal::from_str("2.2").unwrap(),
                },
            ],
            timestamp: Utc::now(),
        };

        // 更新订单簿
        manager.update_order_book(binance_book.clone()).await.unwrap();
        manager.update_order_book(okx_book.clone()).await.unwrap();

        // 测试聚合功能
        let aggregated = normalizer.aggregate_order_books(vec![binance_book, okx_book]).unwrap();
        assert_eq!(aggregated.source_count, 2);
        assert!(!aggregated.bids.is_empty());
        assert!(!aggregated.asks.is_empty());

        // 验证聚合后的数据
        for level in &aggregated.bids {
            assert!(level.total_quantity > Decimal::ZERO);
            assert!(!level.exchange_quantities.is_empty());
        }
    }

    /// 测试策略引擎和订单管理系统集成
    #[tokio::test]
    async fn test_strategy_oms_integration() {
        // 创建策略引擎
        let mut strategy_engine = StrategyEngine::new();
        
        // 创建订单管理系统
        let mut oms = SimpleOMS::new(Exchange::Binance);
        
        // 创建订单通道
        let (order_sender, mut order_receiver) = mpsc::unbounded_channel();
        strategy_engine.set_order_sender(order_sender);
        
        // 添加测试策略
        let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
        let strategy = SimpleMarketMakingStrategy::new(
            "test_strategy".to_string(),
            symbol.clone(),
            Decimal::from_str("1.0").unwrap(),
        );
        strategy_engine.add_strategy(Box::new(strategy));
        
        // 初始化策略
        strategy_engine.initialize_all().await.unwrap();
        
        // 创建测试市场数据
        let ticker = Ticker {
            symbol: symbol.clone(),
            bid_price: Decimal::from_str("49999.0").unwrap(),
            bid_quantity: Decimal::from_str("1.0").unwrap(),
            ask_price: Decimal::from_str("50001.0").unwrap(),
            ask_quantity: Decimal::from_str("2.0").unwrap(),
            last_price: Decimal::from_str("50000.0").unwrap(),
            volume_24h: Decimal::from_str("1000.0").unwrap(),
            price_change_24h: Decimal::from_str("100.0").unwrap(),
            timestamp: Utc::now(),
        };

        // 处理市场数据
        let event = MarketDataEvent::Ticker(ticker);
        strategy_engine.process_market_data(event).await.unwrap();
        
        // 检查是否有订单生成（在这个简单的测试策略中可能没有）
        // 这里主要验证系统没有崩溃
        
        // 停止策略
        strategy_engine.stop_all().await.unwrap();
    }

    /// 测试风险管理系统集成
    #[tokio::test]
    async fn test_risk_management_integration() {
        // 创建风险管理系统
        let risk_limits = RiskLimits {
            max_position_size: Decimal::from_str("10.0").unwrap(),
            max_daily_loss: Decimal::from_str("1000.0").unwrap(),
            max_order_size: Decimal::from_str("5.0").unwrap(),
            max_orders_per_second: 10,
        };
        let rms = SimpleRMS::new(risk_limits);
        
        // 创建测试订单
        let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
        
        // 测试正常订单
        let normal_order = Order::new(
            symbol.clone(),
            Side::Buy,
            OrderType::Limit,
            Decimal::from_str("1.0").unwrap(),
            Some(Decimal::from_str("50000.0").unwrap()),
        );
        
        let risk_check = rms.check_order_risk(&normal_order).await.unwrap();
        assert!(risk_check);
        
        // 测试超限订单
        let oversized_order = Order::new(
            symbol.clone(),
            Side::Buy,
            OrderType::Limit,
            Decimal::from_str("20.0").unwrap(), // 超过最大订单大小
            Some(Decimal::from_str("50000.0").unwrap()),
        );
        
        let risk_check = rms.check_order_risk(&oversized_order).await.unwrap();
        assert!(!risk_check);
    }

    /// 测试监控系统集成
    #[tokio::test]
    async fn test_monitoring_integration() {
        let monitoring = MonitoringSystem::new();
        
        // 测试延迟记录
        monitoring.record_latency("order_processing", 1500);
        monitoring.increment_counter("orders_processed");
        monitoring.record_throughput("market_data", 1000);
        
        // 测试运行时间
        let uptime = monitoring.get_uptime();
        assert!(uptime.as_millis() > 0);
        
        // 这里主要验证监控系统不会崩溃
    }

    /// 测试数据持久化集成
    #[tokio::test]
    async fn test_persistence_integration() {
        let persistence = MemoryPersistence::new();
        
        // 测试保存市场数据
        let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
        let ticker = Ticker {
            symbol: symbol.clone(),
            bid_price: Decimal::from_str("49999.0").unwrap(),
            bid_quantity: Decimal::from_str("1.0").unwrap(),
            ask_price: Decimal::from_str("50001.0").unwrap(),
            ask_quantity: Decimal::from_str("2.0").unwrap(),
            last_price: Decimal::from_str("50000.0").unwrap(),
            volume_24h: Decimal::from_str("1000.0").unwrap(),
            price_change_24h: Decimal::from_str("100.0").unwrap(),
            timestamp: Utc::now(),
        };
        
        let event = MarketDataEvent::Ticker(ticker);
        let result = persistence.save_market_data(&event).await;
        assert!(result.is_ok());
        
        // 测试保存订单
        let order = Order::new(
            symbol,
            Side::Buy,
            OrderType::Limit,
            Decimal::from_str("1.0").unwrap(),
            Some(Decimal::from_str("50000.0").unwrap()),
        );
        
        let result = persistence.save_order(&order).await;
        assert!(result.is_ok());
        
        // 测试查询历史数据
        let result = persistence.query_historical_data("SELECT * FROM market_data").await;
        assert!(result.is_ok());
    }

    /// 测试回测引擎集成
    #[tokio::test]
    async fn test_backtesting_integration() {
        let mut backtest_engine = BacktestEngine::new(Decimal::from_str("10000.0").unwrap());
        
        // 创建测试策略
        let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
        let mut strategy = SimpleMarketMakingStrategy::new(
            "backtest_strategy".to_string(),
            symbol.clone(),
            Decimal::from_str("1.0").unwrap(),
        );
        
        // 创建测试市场数据
        let market_data = vec![
            MarketDataEvent::Ticker(Ticker {
                symbol: symbol.clone(),
                bid_price: Decimal::from_str("49999.0").unwrap(),
                bid_quantity: Decimal::from_str("1.0").unwrap(),
                ask_price: Decimal::from_str("50001.0").unwrap(),
                ask_quantity: Decimal::from_str("2.0").unwrap(),
                last_price: Decimal::from_str("50000.0").unwrap(),
                volume_24h: Decimal::from_str("1000.0").unwrap(),
                price_change_24h: Decimal::from_str("100.0").unwrap(),
                timestamp: Utc::now(),
            }),
            MarketDataEvent::Ticker(Ticker {
                symbol: symbol.clone(),
                bid_price: Decimal::from_str("50099.0").unwrap(),
                bid_quantity: Decimal::from_str("1.0").unwrap(),
                ask_price: Decimal::from_str("50101.0").unwrap(),
                ask_quantity: Decimal::from_str("2.0").unwrap(),
                last_price: Decimal::from_str("50100.0").unwrap(),
                volume_24h: Decimal::from_str("1000.0").unwrap(),
                price_change_24h: Decimal::from_str("200.0").unwrap(),
                timestamp: Utc::now(),
            }),
        ];
        
        // 运行回测
        let result = backtest_engine.run_backtest(&mut strategy, market_data).await;
        assert!(result.is_ok());
        
        let backtest_result = result.unwrap();
        assert_eq!(backtest_result.total_trades, 0); // 简单策略不生成订单
        
        // 验证初始余额
        assert_eq!(backtest_engine.get_current_balance(), Decimal::from_str("10000.0").unwrap());
    }

    /// 测试多线程并发性能
    #[tokio::test]
    async fn test_concurrent_performance() {
        let manager = Arc::new(OrderBookManager::new());
        let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
        
        // 创建多个并发任务
        let mut handles = Vec::new();
        
        for i in 0..10 {
            let manager_clone = manager.clone();
            let symbol_clone = symbol.clone();
            
            let handle = tokio::spawn(async move {
                for j in 0..100 {
                    let order_book = OrderBook {
                        symbol: symbol_clone.clone(),
                        bids: vec![
                            OrderBookLevel {
                                price: Decimal::from_str(&format!("{}.0", 50000 - i * 100 - j)).unwrap(),
                                quantity: Decimal::from_str("1.0").unwrap(),
                            },
                        ],
                        asks: vec![
                            OrderBookLevel {
                                price: Decimal::from_str(&format!("{}.0", 50001 + i * 100 + j)).unwrap(),
                                quantity: Decimal::from_str("1.0").unwrap(),
                            },
                        ],
                        timestamp: Utc::now(),
                    };

                    manager_clone.update_order_book(order_book).await.unwrap();
                }
            });
            
            handles.push(handle);
        }
        
        // 等待所有任务完成
        for handle in handles {
            handle.await.unwrap();
        }
        
        // 验证最终状态
        let snapshot = manager.get_snapshot(&symbol).await;
        assert!(snapshot.is_some());
    }

    /// 测试错误处理和恢复
    #[tokio::test]
    async fn test_error_handling() {
        let normalizer = DataNormalizer::new();
        
        // 测试无效的 ticker 数据
        let invalid_ticker = Ticker {
            symbol: Symbol::new("INVALID", "SYMBOL", Exchange::Binance),
            bid_price: Decimal::from_str("-1.0").unwrap(), // 负价格
            bid_quantity: Decimal::from_str("1.0").unwrap(),
            ask_price: Decimal::from_str("0.0").unwrap(), // 零价格
            ask_quantity: Decimal::from_str("2.0").unwrap(),
            last_price: Decimal::from_str("50000.0").unwrap(),
            volume_24h: Decimal::from_str("1000.0").unwrap(),
            price_change_24h: Decimal::from_str("100.0").unwrap(),
            timestamp: Utc::now(),
        };

        let event = MarketDataEvent::Ticker(invalid_ticker);
        let normalized = normalizer.normalize_market_data(
            event,
            Exchange::Binance,
            None,
        );
        
        // 应该能够处理无效数据而不崩溃
        assert!(normalized.is_ok());
        
        // 质量评分应该很低
        let normalized = normalized.unwrap();
        assert!(normalized.quality_score < 0.5);
    }

    /// 测试系统负载能力
    #[tokio::test]
    async fn test_system_load() {
        let manager = OrderBookManager::new();
        let normalizer = DataNormalizer::new();
        
        let start = std::time::Instant::now();
        
        // 模拟高频数据更新
        for i in 0..1000 {
            let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
            
            // 创建订单簿更新
            let order_book = OrderBook {
                symbol: symbol.clone(),
                bids: vec![
                    OrderBookLevel {
                        price: Decimal::from_str(&format!("{}.{}", 50000 - i % 100, i % 100)).unwrap(),
                        quantity: Decimal::from_str(&format!("{}.0", 1 + i % 10)).unwrap(),
                    },
                ],
                asks: vec![
                    OrderBookLevel {
                        price: Decimal::from_str(&format!("{}.{}", 50001 + i % 100, i % 100)).unwrap(),
                        quantity: Decimal::from_str(&format!("{}.0", 1 + i % 10)).unwrap(),
                    },
                ],
                timestamp: Utc::now(),
            };

            // 标准化数据
            let normalized_book = normalizer.normalize_order_book(order_book.clone(), Exchange::Binance).unwrap();
            
            // 更新订单簿
            manager.update_order_book(normalized_book).await.unwrap();
            
            // 获取统计信息
            let _stats = manager.get_stats(&symbol).await;
        }
        
        let duration = start.elapsed();
        println!("1000 high-frequency updates took: {:?}", duration);
        
        // 验证性能：1000次高频更新应该在合理时间内完成
        assert!(duration.as_secs() < 5);
    }
}

