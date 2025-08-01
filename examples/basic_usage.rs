//! 基本使用示例

use hft_system::*;
use rust_decimal::Decimal;

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    tracing_subscriber::fmt::init();

    // 创建系统实例
    let mut system = HftSystem::new().await?;
    println!("✅ 系统初始化完成");

    // 启动系统
    system.start().await?;
    println!("✅ 系统启动成功");

    // 创建交易对
    let symbol = Symbol::new("BTC", "USDT", Exchange::Binance);
    
    // 创建限价买单
    let buy_order = OrderRequest::new_limit(
        symbol.clone(),
        OrderSide::Buy,
        Decimal::new(1, 3), // 0.001 BTC
        Decimal::new(50000, 0) // $50,000
    );

    println!("📝 创建买单: {} {} {} @ {}",
        buy_order.side,
        buy_order.quantity,
        symbol,
        buy_order.price.unwrap()
    );

    // 创建限价卖单
    let sell_order = OrderRequest::new_limit(
        symbol.clone(),
        OrderSide::Sell,
        Decimal::new(1, 3), // 0.001 BTC
        Decimal::new(51000, 0) // $51,000
    );

    println!("📝 创建卖单: {} {} {} @ {}",
        sell_order.side,
        sell_order.quantity,
        symbol,
        sell_order.price.unwrap()
    );

    // 注意：在实际环境中，这里会提交订单到交易所
    // let buy_response = system.place_order(buy_order).await?;
    // let sell_response = system.place_order(sell_order).await?;

    println!("✅ 示例运行完成");

    // 停止系统
    system.stop().await?;
    println!("✅ 系统已停止");

    Ok(())
}

