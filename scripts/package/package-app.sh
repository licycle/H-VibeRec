#!/bin/bash
set -euo pipefail

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)

cd "$REPO_ROOT"

echo "Building hit-vvc Application..."
echo "========================================"

# 检查是否安装了必要的工具
check_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "ERROR: $1 is not installed. Please install it first."
        exit 1
    else
        echo "OK: $1 is available"
    fi
}

if [ "$(uname -s)" != "Darwin" ] || [ "$(uname -m)" != "arm64" ]; then
    echo "ERROR: bundled offline ASR runtime packaging currently supports macOS Apple Silicon only."
    echo "       Other platforms would install without a usable bundled ASR runtime."
    exit 1
fi

echo "Checking prerequisites..."
check_command "npm"
check_command "cargo"
check_command "rustc"

# 安装前端依赖
echo "Installing frontend dependencies..."
if npm install; then
    echo "OK: frontend dependencies installed successfully"
else
    echo "ERROR: failed to install frontend dependencies"
    exit 1
fi

# 准备内置 ASR runtime
echo "Preparing bundled ASR runtime..."
if npm run runtime:ensure && npm run runtime:check; then
    echo "OK: bundled ASR runtime is ready"
else
    echo "ERROR: failed to prepare bundled ASR runtime"
    exit 1
fi

echo "Checking package readiness..."
npm run package:check

# 清理旧 bundle，避免过时 sidecar 或资源目录残留进检查结果
echo "Cleaning previous bundle output..."
rm -rf src-tauri/target/release/bundle src-tauri/target/release/_up_

# 构建 Tauri 应用
echo "Building Tauri application..."
if npm run tauri build; then
    echo "OK: Tauri application built successfully"
    echo ""
    echo "Build completed."
    echo "Built applications can be found in:"
    echo "   - src-tauri/target/release/bundle/"
    echo ""
    ls -la src-tauri/target/release/bundle/ 2>/dev/null || echo "   (Bundle directory not found - check build output above)"
else
    echo "ERROR: failed to build Tauri application"
    echo "Try running: npm run tauri dev (for development mode)"
    exit 1
fi
