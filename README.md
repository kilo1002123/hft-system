# 高频交易系统 (HFT System)

[![Rust](https://img.shields.io/badge/rust-1.70+-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Build](https://img.shields.io/badge/build-passing-green.svg)](https://github.com/hft-team/hft-system)

基于 Rust 的高性能虚拟货币高频交易系统，支持多交易所统一接口、智能做市策略和实时风险管理。

## ✨ 核心特性

- 🚀 **高性能**: 毫秒级订单处理，支持 20,000+ TPS
- 🔗 **多交易所**: 统一接口支持 Binance、OKX、Gate
- 🤖 **智能策略**: 内置做市策略和自定义策略支持
- 🛡️ **风险控制**: 多层风险防护和实时监控
- 💾 **数据持久化**: PostgreSQL + Redis 双重存储
- 📊 **实时监控**: 完整的性能指标和告警系统

## 🚀 快速开始

### 环境要求

- Rust 1.70+
- PostgreSQL 13+
- Redis 6.0+ (可选)

### 安装步骤

1. **克隆项目**
```bash
git clone https://github.com/hft-team/hft-system.git
cd hft-system
```

2. **配置环境**
```bash
# 复制配置模板
cp config.toml.example config.toml

# 设置环境变量
export BINANCE_API_KEY="your_binance_api_key"
export BINANCE_SECRET_KEY="your_binance_secret_key"
export OKX_API_KEY="your_okx_api_key"
export OKX_SECRET_KEY="your_okx_secret_key"
export GATE_API_KEY="your_gate_api_key"
export GATE_SECRET_KEY="your_gate_secret_key"
```

3. **编译运行**
```bash
# 编译
cargo build --release

# 运行
./target/release/hft_system
```

### Docker 部署

```bash
# 构建镜像
docker build -t hft-system .

# 运行容器
docker run -d --name hft-system \
  -e BINANCE_API_KEY="your_key" \
  -e BINANCE_SECRET_KEY="your_secret" \
  -p 8080:8080 \
  hft-system
```

## 📖 使用示例

### 基本用法

```rust
use hft_system::*;

#[tokio::main]
async fn main() -> Result<()> {
    // 创建系统实例
    let mut system = HftSystem::new().await?;
    
    // 启动系统
    system.start().await?;
    
    // 系统将持续运行直到收到停止信号
    Ok(())
}
```

### 自定义策略

```rust
use hft_system::strategy_engine::*;

struct MyStrategy {
    // 策略参数
}

#[async_trait]
impl Strategy for MyStrategy {
    async fn on_market_data(&mut self, data: MarketData) -> Vec<OrderRequest> {
        // 实现你的交易逻辑
        vec![]
    }
}
```

### 手动下单

```rust
use hft_system::oms::*;

// 创建限价买单
let order = OrderRequest::new_limit(
    Symbol::new("BTC", "USDT", Exchange::Binance),
    OrderSide::Buy,
    Decimal::new(1, 2), // 0.01 BTC
    Decimal::new(50000, 0) // $50,000
);

// 提交订单
let response = order_manager.place_order(order).await?;
```

## 📊 性能指标

| 指标 | 数值 | 说明 |
|------|------|------|
| **订单延迟** | < 1ms | 下单到确认时间 |
| **处理能力** | 20,000+ TPS | 每秒订单处理数 |
| **成功率** | 99.9% | 订单执行成功率 |
| **可用性** | 99.9% | 系统运行时间 |
| **内存使用** | < 512MB | 运行时内存占用 |

## 🏗️ 系统架构

```
┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
│   策略引擎      │  │   订单管理      │  │   风险控制      │
│  Strategy       │  │     OMS         │  │     RMS         │
└─────────────────┘  └─────────────────┘  └─────────────────┘
         │                     │                     │
┌─────────────────────────────────────────────────────────────┐
│                    数据接入层                                │
│  Binance API    │    OKX API     │    Gate API              │
└─────────────────────────────────────────────────────────────┘
         │                     │                     │
┌─────────────────────────────────────────────────────────────┐
│                   持久化层                                   │
│  PostgreSQL     │   Redis Cache  │   File Storage           │
└─────────────────────────────────────────────────────────────┘
```

## ⚙️ 配置说明

### 基础配置 (config.toml)

```toml
[system]
log_level = "info"
worker_threads = 4

[database]
url = "postgresql://localhost/hft_system"
max_connections = 20

[exchanges.binance]
enabled = true
api_key = "${BINANCE_API_KEY}"
secret_key = "${BINANCE_SECRET_KEY}"
testnet = false

[strategy.market_making]
enabled = true
spread_bps = 10
order_size = 0.01
max_position = 1.0
```

### 环境变量

| 变量名 | 说明 | 必需 |
|--------|------|------|
| `BINANCE_API_KEY` | Binance API 密钥 | 是 |
| `BINANCE_SECRET_KEY` | Binance API 私钥 | 是 |
| `OKX_API_KEY` | OKX API 密钥 | 是 |
| `OKX_SECRET_KEY` | OKX API 私钥 | 是 |
| `GATE_API_KEY` | Gate API 密钥 | 是 |
| `GATE_SECRET_KEY` | Gate API 私钥 | 是 |
| `DATABASE_URL` | 数据库连接字符串 | 否 |
| `LOG_LEVEL` | 日志级别 | 否 |

## 🧪 测试

### 运行测试

```bash
# 单元测试
cargo test

# 集成测试
cargo test --test integration_tests

# 性能测试
cargo bench
```

### 测试覆盖率

```bash
# 安装 tarpaulin
cargo install cargo-tarpaulin

# 生成覆盖率报告
cargo tarpaulin --out Html
```

## 📈 监控

### 健康检查

```bash
curl http://localhost:8080/health
```

### 指标查询

```bash
# 系统状态
curl http://localhost:8080/metrics

# 策略状态
curl http://localhost:8080/strategies/status

# 订单统计
curl http://localhost:8080/orders/stats
```

## 🔧 开发

### 项目结构

```
hft_system/
├── src/
│   ├── common/           # 通用类型和工具
│   ├── data_ingestion/   # 数据接入模块
│   ├── oms/             # 订单管理系统
│   ├── rms/             # 风险管理系统
│   ├── strategy_engine/ # 策略引擎
│   ├── monitoring/      # 监控模块
│   ├── persistence/     # 持久化模块
│   └── backtesting/     # 回测模块
├── tests/               # 测试文件
├── benches/            # 性能测试
├── docs/               # 文档
└── config.toml.example # 配置模板
```

### 代码规范

- 使用 `cargo fmt` 格式化代码
- 使用 `cargo clippy` 检查代码质量
- 遵循 Rust 官方编码规范
- 为公共 API 编写文档注释

### 贡献指南

1. Fork 项目
2. 创建功能分支 (`git checkout -b feature/amazing-feature`)
3. 提交更改 (`git commit -m 'Add amazing feature'`)
4. 推送到分支 (`git push origin feature/amazing-feature`)
5. 创建 Pull Request

## 📚 文档

- [技术架构](docs/技术架构.md) - 详细的系统架构说明
- [API 文档](docs/API文档.md) - 完整的 API 接口文档
- [部署指南](docs/部署指南.md) - 生产环境部署指南
- [开发指南](docs/开发指南.md) - 开发环境搭建和开发流程

## 🤝 支持

- 📧 Email: support@hft-system.com
- 💬 Discord: [HFT System Community](https://discord.gg/hft-system)
- 📖 Wiki: [项目 Wiki](https://github.com/hft-team/hft-system/wiki)
- 🐛 Issues: [GitHub Issues](https://github.com/hft-team/hft-system/issues)

## 📄 许可证

本项目采用 MIT 许可证 - 查看 [LICENSE](LICENSE) 文件了解详情。

## ⭐ Star History

[![Star History Chart](https://api.star-history.com/svg?repos=hft-team/hft-system&type=Date)](https://star-history.com/#hft-team/hft-system&Date)

---

**免责声明**: 本软件仅供学习和研究使用。使用本软件进行实际交易的风险由用户自行承担。

