//! 高频交易系统主程序
//! 
//! 启动和管理整个交易系统的生命周期

use hft_system::{HftSystem, Result};
use std::env;
use tokio::signal;
use tracing::{info, error};

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    init_logging()?;
    
    // 加载配置
    let config_path = env::args().nth(1).unwrap_or_else(|| "config.toml".to_string());
    info!("Loading configuration from: {}", config_path);
    
    // 创建系统实例
    let mut system = HftSystem::new().await?;
    info!("HFT System initialized");
    
    // 启动系统
    system.start().await?;
    info!("HFT System started, waiting for shutdown signal...");
    
    // 等待关闭信号
    wait_for_shutdown().await;
    info!("Shutdown signal received, stopping system...");
    
    // 优雅关闭
    system.stop().await?;
    info!("HFT System stopped successfully");
    
    Ok(())
}

/// 初始化日志系统
fn init_logging() -> Result<()> {
    let log_level = env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string());
    
    tracing_subscriber::fmt()
        .with_env_filter(log_level)
        .with_target(false)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .init();
    
    info!("Logging initialized");
    Ok(())
}

/// 等待关闭信号
async fn wait_for_shutdown() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C signal");
        },
        _ = terminate => {
            info!("Received terminate signal");
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_system_creation() {
        let result = HftSystem::new().await;
        assert!(result.is_ok(), "System creation should succeed");
    }
}

