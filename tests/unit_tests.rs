use hft_system::common::*;
use hft_system::data_ingestion::*;
use chrono::Utc;
use rust_decimal::Decimal;
use std::str::FromStr;
use tokio_test;

#[cfg(test)]
mod common_tests {
    use super::*;

    #[test]
    fn test_symbol_creation() {
        let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
        assert_eq!(symbol.base, "BTC");
        assert_eq!(symbol.quote, "USDT");
        assert_eq!(symbol.exchange, Exchange::Binance);
    }

    #[test]
    fn test_symbol_to_exchange_format() {
        let binance_symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
        assert_eq!(binance_symbol.to_exchange_format(), "BTCUSDT");

        let okx_symbol = Symbol::new("BTC", "USDT", Exchange::OKX);
        assert_eq!(okx_symbol.to_exchange_format(), "BTC-USDT");

        let gate_symbol = Symbol::new("BTC", "USDT", Exchange::Gate);
        assert_eq!(gate_symbol.to_exchange_format(), "BTC_USDT");
    }

    #[test]
    fn test_order_creation() {
        let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
        let order = Order::new(
            symbol,
            Side::Buy,
            OrderType::Limit,
            Decimal::from_str("1.0").unwrap(),
            Some(Decimal::from_str("50000.0").unwrap()),
        );

        assert_eq!(order.side, Side::Buy);
        assert_eq!(order.order_type, OrderType::Limit);
        assert_eq!(order.quantity, Decimal::from_str("1.0").unwrap());
        assert_eq!(order.price, Some(Decimal::from_str("50000.0").unwrap()));
        assert_eq!(order.status, OrderStatus::New);
        assert_eq!(order.filled_quantity, Decimal::ZERO);
    }

    #[test]
    fn test_ticker_creation() {
        let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
        let ticker = Ticker {
            symbol,
            bid_price: Decimal::from_str("49999.0").unwrap(),
            bid_quantity: Decimal::from_str("1.5").unwrap(),
            ask_price: Decimal::from_str("50001.0").unwrap(),
            ask_quantity: Decimal::from_str("2.0").unwrap(),
            last_price: Decimal::from_str("50000.0").unwrap(),
            volume_24h: Decimal::from_str("1000.0").unwrap(),
            price_change_24h: Decimal::from_str("500.0").unwrap(),
            timestamp: Utc::now(),
        };

        assert!(ticker.bid_price < ticker.ask_price);
        assert!(ticker.volume_24h > Decimal::ZERO);
    }

    #[test]
    fn test_order_book_creation() {
        let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
        let bids = vec![
            OrderBookLevel {
                price: Decimal::from_str("49999.0").unwrap(),
                quantity: Decimal::from_str("1.0").unwrap(),
            },
            OrderBookLevel {
                price: Decimal::from_str("49998.0").unwrap(),
                quantity: Decimal::from_str("2.0").unwrap(),
            },
        ];
        let asks = vec![
            OrderBookLevel {
                price: Decimal::from_str("50001.0").unwrap(),
                quantity: Decimal::from_str("1.5").unwrap(),
            },
            OrderBookLevel {
                price: Decimal::from_str("50002.0").unwrap(),
                quantity: Decimal::from_str("2.5").unwrap(),
            },
        ];

        let order_book = OrderBook {
            symbol,
            bids,
            asks,
            timestamp: Utc::now(),
        };

        assert_eq!(order_book.bids.len(), 2);
        assert_eq!(order_book.asks.len(), 2);
        assert!(order_book.bids[0].price > order_book.bids[1].price);
        assert!(order_book.asks[0].price < order_book.asks[1].price);
    }
}

#[cfg(test)]
mod orderbook_tests {
    use super::*;

    #[tokio::test]
    async fn test_orderbook_manager_creation() {
        let manager = OrderBookManager::new();
        assert_eq!(manager.get_active_symbols().len(), 0);
    }

    #[tokio::test]
    async fn test_orderbook_update() {
        let manager = OrderBookManager::new();
        let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
        
        let order_book = OrderBook {
            symbol: symbol.clone(),
            bids: vec![
                OrderBookLevel {
                    price: Decimal::from_str("50000.0").unwrap(),
                    quantity: Decimal::from_str("1.0").unwrap(),
                },
            ],
            asks: vec![
                OrderBookLevel {
                    price: Decimal::from_str("50001.0").unwrap(),
                    quantity: Decimal::from_str("1.5").unwrap(),
                },
            ],
            timestamp: Utc::now(),
        };

        let result = manager.update_order_book(order_book).await;
        assert!(result.is_ok());

        let snapshot = manager.get_snapshot(&symbol).await;
        assert!(snapshot.is_some());
        
        let snapshot = snapshot.unwrap();
        assert_eq!(snapshot.bids.len(), 1);
        assert_eq!(snapshot.asks.len(), 1);
    }

    #[tokio::test]
    async fn test_best_bid_ask() {
        let manager = OrderBookManager::new();
        let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
        
        let order_book = OrderBook {
            symbol: symbol.clone(),
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

        manager.update_order_book(order_book).await.unwrap();

        let (best_bid, best_ask) = manager.get_best_bid_ask(&symbol).await.unwrap();
        assert_eq!(best_bid, Some(Decimal::from_str("50000.0").unwrap()));
        assert_eq!(best_ask, Some(Decimal::from_str("50001.0").unwrap()));
    }

    #[tokio::test]
    async fn test_market_impact_calculation() {
        let manager = OrderBookManager::new();
        let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
        
        let order_book = OrderBook {
            symbol: symbol.clone(),
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

        manager.update_order_book(order_book).await.unwrap();

        // 测试买单市场冲击
        let (avg_price, filled_qty) = manager
            .calculate_market_impact(&symbol, Side::Buy, Decimal::from_str("2.0").unwrap())
            .await
            .unwrap();
        
        assert!(avg_price > Decimal::from_str("50001.0").unwrap());
        assert_eq!(filled_qty, Decimal::from_str("2.0").unwrap());

        // 测试卖单市场冲击
        let (avg_price, filled_qty) = manager
            .calculate_market_impact(&symbol, Side::Sell, Decimal::from_str("2.0").unwrap())
            .await
            .unwrap();
        
        assert!(avg_price < Decimal::from_str("50000.0").unwrap());
        assert_eq!(filled_qty, Decimal::from_str("2.0").unwrap());
    }
}

#[cfg(test)]
mod data_normalizer_tests {
    use super::*;

    #[test]
    fn test_data_normalizer_creation() {
        let normalizer = DataNormalizer::new();
        // 验证符号映射已初始化
        assert!(!normalizer.symbol_mappings.is_empty());
    }

    #[test]
    fn test_symbol_normalization() {
        let normalizer = DataNormalizer::new();
        let symbol = Symbol::new("btc", "usdt", Exchange::Binance);
        let normalized = normalizer.normalize_symbol(&symbol, Exchange::Binance).unwrap();
        
        assert_eq!(normalized.base, "BTC");
        assert_eq!(normalized.quote, "USDT");
        assert_eq!(normalized.exchange, Exchange::Binance);
    }

    #[test]
    fn test_ticker_normalization() {
        let normalizer = DataNormalizer::new();
        let ticker = Ticker {
            symbol: Symbol::new("btc", "usdt", Exchange::Binance),
            bid_price: Decimal::from_str("50000.123456789").unwrap(),
            bid_quantity: Decimal::from_str("1.123456789").unwrap(),
            ask_price: Decimal::from_str("50001.123456789").unwrap(),
            ask_quantity: Decimal::from_str("2.123456789").unwrap(),
            last_price: Decimal::from_str("50000.5").unwrap(),
            volume_24h: Decimal::from_str("1000.123456789").unwrap(),
            price_change_24h: Decimal::from_str("100.5").unwrap(),
            timestamp: Utc::now(),
        };

        let normalized = normalizer.normalize_ticker(ticker, Exchange::Binance).unwrap();
        assert_eq!(normalized.symbol.base, "BTC");
        assert_eq!(normalized.symbol.quote, "USDT");
        assert!(normalized.bid_price < normalized.ask_price);
    }

    #[test]
    fn test_order_book_normalization() {
        let normalizer = DataNormalizer::new();
        let order_book = OrderBook {
            symbol: Symbol::new("btc", "usdt", Exchange::Binance),
            bids: vec![
                OrderBookLevel {
                    price: Decimal::from_str("49999.123456789").unwrap(),
                    quantity: Decimal::from_str("1.123456789").unwrap(),
                },
                OrderBookLevel {
                    price: Decimal::from_str("50000.123456789").unwrap(),
                    quantity: Decimal::from_str("2.123456789").unwrap(),
                },
            ],
            asks: vec![
                OrderBookLevel {
                    price: Decimal::from_str("50001.123456789").unwrap(),
                    quantity: Decimal::from_str("1.123456789").unwrap(),
                },
                OrderBookLevel {
                    price: Decimal::from_str("50002.123456789").unwrap(),
                    quantity: Decimal::from_str("2.123456789").unwrap(),
                },
            ],
            timestamp: Utc::now(),
        };

        let normalized = normalizer.normalize_order_book(order_book, Exchange::Binance).unwrap();
        assert_eq!(normalized.symbol.base, "BTC");
        assert_eq!(normalized.symbol.quote, "USDT");
        
        // 验证买单按价格降序排列
        assert!(normalized.bids[0].price > normalized.bids[1].price);
        
        // 验证卖单按价格升序排列
        assert!(normalized.asks[0].price < normalized.asks[1].price);
    }

    #[test]
    fn test_quality_score_calculation() {
        let normalizer = DataNormalizer::new();
        
        // 测试正常的 ticker 数据
        let good_ticker = Ticker {
            symbol: Symbol::new("BTC", "USDT", Exchange::Binance),
            bid_price: Decimal::from_str("49999.0").unwrap(),
            bid_quantity: Decimal::from_str("1.0").unwrap(),
            ask_price: Decimal::from_str("50001.0").unwrap(),
            ask_quantity: Decimal::from_str("2.0").unwrap(),
            last_price: Decimal::from_str("50000.0").unwrap(),
            volume_24h: Decimal::from_str("1000.0").unwrap(),
            price_change_24h: Decimal::from_str("100.0").unwrap(),
            timestamp: Utc::now(),
        };

        let good_event = MarketDataEvent::Ticker(good_ticker);
        let good_score = normalizer.calculate_quality_score(&good_event, Exchange::Binance);
        assert!(good_score > 0.9);

        // 测试异常的 ticker 数据（买价高于卖价）
        let bad_ticker = Ticker {
            symbol: Symbol::new("BTC", "USDT", Exchange::Binance),
            bid_price: Decimal::from_str("50001.0").unwrap(),
            bid_quantity: Decimal::from_str("1.0").unwrap(),
            ask_price: Decimal::from_str("49999.0").unwrap(),
            ask_quantity: Decimal::from_str("2.0").unwrap(),
            last_price: Decimal::from_str("50000.0").unwrap(),
            volume_24h: Decimal::from_str("1000.0").unwrap(),
            price_change_24h: Decimal::from_str("100.0").unwrap(),
            timestamp: Utc::now(),
        };

        let bad_event = MarketDataEvent::Ticker(bad_ticker);
        let bad_score = normalizer.calculate_quality_score(&bad_event, Exchange::Binance);
        assert!(bad_score < 0.5);
    }

    #[test]
    fn test_aggregated_price_calculation() {
        let normalizer = DataNormalizer::new();
        
        let tickers = vec![
            Ticker {
                symbol: Symbol::new("BTC", "USDT", Exchange::Binance),
                bid_price: Decimal::from_str("49999.0").unwrap(),
                bid_quantity: Decimal::from_str("1.0").unwrap(),
                ask_price: Decimal::from_str("50001.0").unwrap(),
                ask_quantity: Decimal::from_str("2.0").unwrap(),
                last_price: Decimal::from_str("50000.0").unwrap(),
                volume_24h: Decimal::from_str("1000.0").unwrap(),
                price_change_24h: Decimal::from_str("100.0").unwrap(),
                timestamp: Utc::now(),
            },
            Ticker {
                symbol: Symbol::new("BTC", "USDT", Exchange::OKX),
                bid_price: Decimal::from_str("50000.0").unwrap(),
                bid_quantity: Decimal::from_str("1.5").unwrap(),
                ask_price: Decimal::from_str("50002.0").unwrap(),
                ask_quantity: Decimal::from_str("2.5").unwrap(),
                last_price: Decimal::from_str("50001.0").unwrap(),
                volume_24h: Decimal::from_str("800.0").unwrap(),
                price_change_24h: Decimal::from_str("80.0").unwrap(),
                timestamp: Utc::now(),
            },
        ];

        let aggregated = normalizer.calculate_aggregated_price(tickers).unwrap();
        assert_eq!(aggregated.source_exchanges.len(), 2);
        assert!(aggregated.weighted_price > Decimal::ZERO);
        assert!(aggregated.spread > Decimal::ZERO);
        assert_eq!(aggregated.best_bid, Decimal::from_str("50000.0").unwrap());
        assert_eq!(aggregated.best_ask, Decimal::from_str("50001.0").unwrap());
    }
}

#[cfg(test)]
mod data_provider_tests {
    use super::*;

    #[test]
    fn test_create_binance_provider() {
        let config = ExchangeConfig {
            api_key: "test_key".to_string(),
            secret_key: "test_secret".to_string(),
            passphrase: None,
            sandbox: true,
            rate_limit: 1000,
        };

        let provider = create_provider(Exchange::Binance, &config);
        assert!(provider.is_ok());
        
        let provider = provider.unwrap();
        assert_eq!(provider.exchange(), Exchange::Binance);
        assert!(!provider.is_connected());
    }

    #[test]
    fn test_create_okx_provider() {
        let config = ExchangeConfig {
            api_key: "test_key".to_string(),
            secret_key: "test_secret".to_string(),
            passphrase: Some("test_passphrase".to_string()),
            sandbox: true,
            rate_limit: 1000,
        };

        let provider = create_provider(Exchange::OKX, &config);
        assert!(provider.is_ok());
        
        let provider = provider.unwrap();
        assert_eq!(provider.exchange(), Exchange::OKX);
        assert!(!provider.is_connected());
    }

    #[test]
    fn test_create_gate_provider() {
        let config = ExchangeConfig {
            api_key: "test_key".to_string(),
            secret_key: "test_secret".to_string(),
            passphrase: None,
            sandbox: true,
            rate_limit: 1000,
        };

        let provider = create_provider(Exchange::Gate, &config);
        assert!(provider.is_ok());
        
        let provider = provider.unwrap();
        assert_eq!(provider.exchange(), Exchange::Gate);
        assert!(!provider.is_connected());
    }

    #[tokio::test]
    async fn test_data_distributor() {
        let mut distributor = DataDistributor::new();
        assert_eq!(distributor.providers.len(), 0);
        assert_eq!(distributor.subscribers.len(), 0);

        let config = ExchangeConfig {
            api_key: "test_key".to_string(),
            secret_key: "test_secret".to_string(),
            passphrase: None,
            sandbox: true,
            rate_limit: 1000,
        };

        let provider = create_provider(Exchange::Binance, &config).unwrap();
        distributor.add_provider(provider);
        assert_eq!(distributor.providers.len(), 1);
    }
}

#[cfg(test)]
mod performance_tests {
    use super::*;
    use std::time::Instant;

    #[tokio::test]
    async fn test_orderbook_update_performance() {
        let manager = OrderBookManager::new();
        let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
        
        let start = Instant::now();
        
        // 执行1000次订单簿更新
        for i in 0..1000 {
            let order_book = OrderBook {
                symbol: symbol.clone(),
                bids: vec![
                    OrderBookLevel {
                        price: Decimal::from_str(&format!("{}.0", 50000 - i)).unwrap(),
                        quantity: Decimal::from_str("1.0").unwrap(),
                    },
                ],
                asks: vec![
                    OrderBookLevel {
                        price: Decimal::from_str(&format!("{}.0", 50001 + i)).unwrap(),
                        quantity: Decimal::from_str("1.0").unwrap(),
                    },
                ],
                timestamp: Utc::now(),
            };

            manager.update_order_book(order_book).await.unwrap();
        }
        
        let duration = start.elapsed();
        println!("1000 order book updates took: {:?}", duration);
        
        // 验证性能：1000次更新应该在1秒内完成
        assert!(duration.as_secs() < 1);
    }

    #[test]
    fn test_data_normalization_performance() {
        let normalizer = DataNormalizer::new();
        let start = Instant::now();
        
        // 执行1000次数据标准化
        for i in 0..1000 {
            let ticker = Ticker {
                symbol: Symbol::new("BTC", "USDT", Exchange::Binance),
                bid_price: Decimal::from_str(&format!("{}.123456789", 50000 - i)).unwrap(),
                bid_quantity: Decimal::from_str("1.123456789").unwrap(),
                ask_price: Decimal::from_str(&format!("{}.123456789", 50001 + i)).unwrap(),
                ask_quantity: Decimal::from_str("2.123456789").unwrap(),
                last_price: Decimal::from_str(&format!("{}.5", 50000)).unwrap(),
                volume_24h: Decimal::from_str("1000.123456789").unwrap(),
                price_change_24h: Decimal::from_str("100.5").unwrap(),
                timestamp: Utc::now(),
            };

            let _normalized = normalizer.normalize_ticker(ticker, Exchange::Binance).unwrap();
        }
        
        let duration = start.elapsed();
        println!("1000 ticker normalizations took: {:?}", duration);
        
        // 验证性能：1000次标准化应该在100ms内完成
        assert!(duration.as_millis() < 100);
    }
}

