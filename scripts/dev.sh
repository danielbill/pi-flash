#!/usr/bin/env bash
# 快速验证：debug 构建 + 直接启动（不打包、不 zip）。
# 用法：scripts/dev.sh        （改完代码后跑这个即可测试）
#       scripts/dev.sh --test （顺带跑一遍测试）

set -euo pipefail
cd "$(dirname "$0")/.."

if [[ "${1:-}" == "--test" ]]; then
  cargo test
fi

cargo build -p app
./target/debug/pi-flash.exe &
echo "已启动 target/debug/pi-flash.exe"
