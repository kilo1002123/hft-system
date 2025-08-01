//! 做市策略演示

use hft_system::*;
use rust_decimal::Decimal;
use std::time::Duration;
use tokio::time;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    println!("🚀 启动做市策略演示");

    // 创建系统实例
    let mut system = HftSystem::new().await?;
    system.start().await?;

    // 创建交易对
    let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
    
    // 模拟市场价格
    let mut market_price = Decimal::new(50000, 0); // $50,000
    
    println!("📊 开始做市演示 (交易对: {})", symbol);
    
    for i in 1..=10 {
        println!("\n--- 第 {} 轮报价 ---", i);
        
        // 计算买卖价格 (价差 0.1%)
        let spread = market_price * Decimal::new(1, 3); // 0.1%
        let bid_price = market_price - spread / Decimal::new(2, 0);
        let ask_price = market_price + spread / Decimal::new(2, 0);
        
        // 创建双边订单
        let buy_order = OrderRequest::new_limit(
            symbol.clone(),
            OrderSide::Buy,
            Decimal::new(1, 3), // 0.001 BTC
            bid_price
        );
        
        let sell_order = OrderRequest::new_limit(
            symbol.clone(),
            OrderSide::Sell,
            Decimal::new(1, 3), // 0.001 BTC
            ask_price
        );
        
        println!("📈 买单: {} @ ${}", buy_order.quantity, bid_price);
        println!("📉 卖单: {} @ ${}", sell_order.quantity, ask_price);
        println!("💰 价差: ${} ({:.2}%)", 
            ask_price - bid_price, 
            (ask_price - bid_price) / market_price * Decimal::new(100, 0)
        );
        
        // 模拟价格变动
        let price_change = (i % 3) as i64 - 1; // -1, 0, 1
        market_price += Decimal::new(price_change * 100, 0);
        
        // 等待一段时间
        time::sleep(Duration::from_secs(2)).await;
    }
    
    println!("\n✅ 做市策略演示完成");
    
    system.stop().await?;
    Ok(())
}

