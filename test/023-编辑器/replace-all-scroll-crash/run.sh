#!/usr/bin/env bash
# 023 编辑器「替换全部」滚动位下溢崩溃复现（bead pi-flash-2pyq）
# 修复后预期：实例存活 + 53 处 ORIGIN→mark 全替换 + 视图回顶。
# 修复前预期：进程 abort（crash-*.log 帧 = panic_const_sub_overflow @ scroll_to）。
# 用法：cargo build 后 bash test/023-编辑器/replace-all-scroll-crash/run.sh
set -e
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
DIR="$ROOT/tmp/ui-test-2pyq"
PUI="$ROOT/target/debug/pif-ui.exe"
APP="$ROOT/target/debug/pi-flash.exe"
MD="$DIR/替换崩溃复现.md"

[ -x "$APP" ] || { echo "先 cargo build"; exit 1; }
rm -rf "$DIR"; mkdir -p "$DIR/inst"

# 1) 测试文件：40 节 CJK 长段（软换行）+ ORIGIN-xx ASCII token（40 节头 +
#    13 处 i%3 行内 = 53 处，对齐 case.md 实录数）
P1='源质铸造的传说源自大崩坏之前的黄金纪元，那时的锻造师们掌握了将魔力注入金属的核心工艺，铸出的每一柄武器都拥有自己的灵魂与意志，这种工艺在后世被称为源金时代。'
P2='然而大崩坏降临之后，一切辉煌都随着源脉的枯竭而湮灭，只剩下散落各地的遗迹与残卷，供后来的探险者们去发掘那些被遗忘的技术，以及那些仍在低语的源金造物。'
P3='据说最深处的心脉炉至今仍在运转，它以整条源脉为燃料，将地底的源金矿脉熔炼成可以自我修复的合金，任何接近它的生物都会被那股灼热的魔力洪流灼伤甚至同化。'
{
  printf '# 源金与源质 —— 替换崩溃复现\n\n> 引子：源金即锈，源质即火。\n\n---\n\n'
  for i in $(seq -w 1 40); do
    printf '## 第%s节 源金残卷 ORIGIN-%s\n\n' "$i" "$i"
    if [ $((10#$i % 3)) = 0 ]; then
      printf '%s\n\n' "${P1/源金时代。/源金时代 ORIGIN-$i。}"
    else
      printf '%s\n\n' "$P1"
    fi
    printf '%s\n\n%s\n\n' "$P2" "$P3"
  done
} > "$MD"

# 2) 隔离实例
PI_FLASH_DIR="$DIR/inst" PI_FLASH_AUTOMATION=auto "$APP" > "$DIR/app.log" 2>&1 &
sleep 8
LINE=$(sed -n 's/.*automation: 127\.0\.0\.1:\([0-9]*\) pid=[0-9]* token=\([a-f0-9]*\).*/\1 \2/p' "$DIR/app.log" | head -1)
PORT=${LINE%% *}; TOK=${LINE##* }
export PI_FLASH_DIR="$DIR/inst"
$PUI --addr 127.0.0.1:$PORT --token $TOK wait --path app.booted --truthy --timeout 30

# 3) 手势：开文件 → 聚焦 → 滚 5 页 → ctrl-h 替换面板 → 查询/替换词 → 全部替换
$PUI --addr 127.0.0.1:$PORT --token $TOK exec file.open --arg path="$MD" >/dev/null
$PUI --addr 127.0.0.1:$PORT --token $TOK exec input.focus --arg target=editor >/dev/null
for i in 1 2 3 4 5; do
  $PUI --addr 127.0.0.1:$PORT --token $TOK exec input.keys --arg keys=pagedown >/dev/null; sleep 0.3
done
$PUI --addr 127.0.0.1:$PORT --token $TOK exec input.keys --arg keys=ctrl-h >/dev/null; sleep 1
for c in o r i g i n; do
  $PUI --addr 127.0.0.1:$PORT --token $TOK exec input.keys --arg keys=$c >/dev/null; sleep 0.15
done
$PUI --addr 127.0.0.1:$PORT --token $TOK exec input.mouse --arg action=click --arg rx=0.4948 --arg ry=0.1378 >/dev/null
for c in m a r k; do
  $PUI --addr 127.0.0.1:$PORT --token $TOK exec input.keys --arg keys=$c >/dev/null; sleep 0.15
done
sleep 0.5
$PUI --addr 127.0.0.1:$PORT --token $TOK exec input.mouse --arg action=click --arg rx=0.883 --arg ry=0.1378 >/dev/null
sleep 1.5

# 4) 断言：实例存活（此步连不上即崩）+ 盘上替换数
$PUI --addr 127.0.0.1:$PORT --token $TOK exec file.save --arg path="$MD" >/dev/null
N=$(grep -c "mark-" "$MD" || true)
O=$(grep -c "ORIGIN" "$MD" || true)
$PUI --addr 127.0.0.1:$PORT --token $TOK exec app.quit >/dev/null 2>&1 || true

if [ "$N" = "53" ] && [ "$O" = "0" ]; then
  echo "PASS: 存活 + 53 处全替换"
else
  echo "FAIL: mark-0xx=$N ORIGIN=$O（替换不完整或实例已崩）"
  exit 1
fi
