#!/usr/bin/env bash
# 快速验证：debug 构建 + 直接启动（不打包、不 zip）。
# 用法：scripts/dev.sh        （改完代码后跑这个即可测试）
#       scripts/dev.sh --test （顺带跑一遍测试）

set -euo pipefail
cd "$(dirname "$0")/.."

if [[ "${1:-}" == "--test" ]]; then
  cargo test
fi

# RPC wire log (pi-link client reads PI_FLASH_RPC_LOG): records spawn args,
# every JSONL line both ways, and the child stderr. Delete to silence.
export PI_FLASH_RPC_LOG="${PI_FLASH_RPC_LOG:-$PWD/tmp/rpc-last.log}"
mkdir -p "$(dirname "$PI_FLASH_RPC_LOG")"
: > "$PI_FLASH_RPC_LOG"

cargo build -p app
./target/debug/pi-flash.exe &
echo "已启动 target/debug/pi-flash.exe（RPC 日志 tmp/rpc-last.log）"
