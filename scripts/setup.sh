#!/bin/bash

# 高频交易系统安装脚本

set -e

echo "🚀 开始安装高频交易系统..."

# 检查 Rust 是否已安装
if ! command -v rustc &> /dev/null; then
    echo "📦 安装 Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source ~/.cargo/env
else
    echo "✅ Rust 已安装"
fi

# 检查 PostgreSQL 是否已安装
if ! command -v psql &> /dev/null; then
    echo "📦 安装 PostgreSQL..."
    if [[ "$OSTYPE" == "linux-gnu"* ]]; then
        sudo apt update
        sudo apt install -y postgresql postgresql-contrib
    elif [[ "$OSTYPE" == "darwin"* ]]; then
        brew install postgresql
    else
        echo "⚠️  请手动安装 PostgreSQL"
    fi
else
    echo "✅ PostgreSQL 已安装"
fi

# 创建数据库
echo "🗄️  创建数据库..."
sudo -u postgres createdb hft_system 2>/dev/null || echo "数据库已存在"

# 复制配置文件
if [ ! -f config.toml ]; then
    echo "⚙️  创建配置文件..."
    cp config.toml.example config.toml
    echo "请编辑 config.toml 文件设置 API 密钥"
fi

# 编译项目
echo "🔨 编译项目..."
cargo build --release

echo "✅ 安装完成！"
echo ""
echo "下一步："
echo "1. 编辑 config.toml 设置 API 密钥"
echo "2. 设置环境变量："
echo "   export BINANCE_API_KEY='your_key'"
echo "   export BINANCE_SECRET_KEY='your_secret'"
echo "3. 运行系统："
echo "   ./target/release/hft_system"

