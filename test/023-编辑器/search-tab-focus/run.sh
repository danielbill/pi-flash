#!/usr/bin/env bash
# 023 fv-search Tab 焦点环转复现（bead pi-flash-8rzn）
# 驱动：ctrl-h 展开直接打字 → 点查询框打词 → Tab 续打 → shift-tab 补打，
# 三张 shot 落 tmp/，焦点落点（哪框收到字）人工目检（面板不在 snapshot 面）。
# 修复后预期：step1 替换框=mark 查询框空；step3 替换框=markx 查询框=origin；
# step4 查询框收到 q。
# 用法：cargo build 后 bash test/023-编辑器/search-tab-focus/run.sh
set -e
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
DIR="$ROOT/tmp/ui-test-tab-focus"
PUI="$ROOT/target/debug/pif-ui.exe"
APP="$ROOT/target/debug/pi-flash.exe"
MD="$ROOT/tmp/ui-test-2pyq/替换崩溃复现.md"   # 复用 2pyq 的长文，无则提示先跑

[ -x "$APP" ] || { echo "先 cargo build"; exit 1; }
[ -f "$MD" ] || "$ROOT/test/023-编辑器/replace-all-scroll-crash/run.sh" >/dev/null 2>&1 || true
[ -f "$MD" ] || { echo "缺测试长文 $MD"; exit 1; }
rm -rf "$DIR"; mkdir -p "$DIR/inst"

PI_FLASH_DIR="$DIR/inst" PI_FLASH_AUTOMATION=auto "$APP" > "$DIR/app.log" 2>&1 &
sleep 8
LINE=$(sed -n 's/.*automation: 127\.0\.0\.1:\([0-9]*\) pid=[0-9]* token=\([a-f0-9]*\).*/\1 \2/p' "$DIR/app.log" | head -1)
PORT=${LINE%% *}; TOK=${LINE##* }
A="--addr 127.0.0.1:$PORT --token $TOK"
export PI_FLASH_DIR="$DIR/inst"
$PUI $A wait --path app.booted --truthy --timeout 30
$PUI $A exec file.open --arg path="$MD" >/dev/null
$PUI $A exec input.focus --arg target=editor >/dev/null

# 1) ctrl-h 展开 → 直接打 mark（预期落替换框）
$PUI $A exec input.keys --arg keys=ctrl-h >/dev/null; sleep 1
for c in m a r k; do $PUI $A exec input.keys --arg keys=$c >/dev/null; sleep 0.15; done
sleep 0.5
$PUI $A shot "$DIR/step1-replace-focused.png" >/dev/null

# 2) 点查询框打 origin → Tab → 打 x（预期 x 落替换框）
$PUI $A exec input.mouse --arg action=click --arg rx=0.4 --arg ry=0.101 >/dev/null; sleep 0.3
for c in o r i g i n; do $PUI $A exec input.keys --arg keys=$c >/dev/null; sleep 0.15; done
sleep 0.5
$PUI $A exec input.keys --arg keys=tab >/dev/null; sleep 0.4
$PUI $A exec input.keys --arg keys=x >/dev/null; sleep 0.5
$PUI $A shot "$DIR/step3-tab-cycled.png" >/dev/null

# 3) shift-tab → 打 q（预期 q 落查询框）
$PUI $A exec input.keys --arg keys=shift-tab >/dev/null; sleep 0.4
$PUI $A exec input.keys --arg keys=q >/dev/null; sleep 0.5
$PUI $A shot "$DIR/step4-shifttab.png" >/dev/null

$PUI $A exec app.quit >/dev/null 2>&1 || true
echo "DONE: 目检 $DIR/step{1,3,4}-*.png（判据见同目录 case.md 实录表）"
