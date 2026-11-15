#!/bin/bash
# test/047-内存削减 bench.sh：047 §4 度量验收——同一洪泛会话负载下的
# 进程工作集对照（A/B 二进制由调用方给）。
#
# 用法：
#   bash test/047-内存削减/bench.sh <label> <pi-flash.exe> [PI_FLASH_VENDOR_DIR]
#
# 流程：生成洪泛夹具（bench-fixture.mjs，~16MB 真实形态会话）→ 隔离实例
# （PI_CODING_AGENT_SESSION_DIR 指夹具根，不污染真实 ~/.pi）→ session.open
# 装载 → 收场后采样 WorkingSet64/PrivateMemorySize64 ×5 取中位 → 单行
# BENCH 结果（工作集 MB + sessions 快照 mem 探针）。
#
# 装载路不发模型请求（session.open 只 spawn + get_state/get_messages），
# 真实 vendored pi 零 API 成本。
set -u

ROOT="$(cd "$(dirname "$0")/../.." && { pwd -W 2>/dev/null || pwd; })"
FIXGEN="$ROOT/test/047-内存削减/bench-fixture.mjs"
OUTBASE="$ROOT/tmp/mem-047-bench"

LABEL="${1:?usage: bench.sh <label> <exe> [vendor_dir]}"
EXE="${2:?usage: bench.sh <label> <exe> [vendor_dir]}"
VENDOR="${3:-}"

[ -f "$EXE" ] || { echo "FATAL: $EXE 不存在"; exit 2; }
command -v node >/dev/null 2>&1 || { echo "FATAL: node 不可用"; exit 2; }

# ------------------------------------------------------------- fixture --
FIXROOT="$OUTBASE/fixture"
if [ ! -f "$FIXROOT/sessions/bench-flood.jsonl" ] && [ ! -f "$(ls "$FIXROOT"/sessions/*/bench-flood.jsonl 2>/dev/null | head -1)" ]; then
  rm -rf "$FIXROOT"
  mkdir -p "$FIXROOT"
  node "$FIXGEN" "$FIXROOT" || { echo "FATAL: 夹具生成失败"; exit 2; }
fi
FIXSESSION=$(ls "$FIXROOT"/sessions/*/bench-flood.jsonl | head -1)
FILE_MB=$(node -e 'const fs=require("fs");const s=fs.statSync(process.argv[1]).size;console.log((s/1048576).toFixed(1))' "$FIXSESSION")
echo "[bench] 夹具 $FIXSESSION (${FILE_MB} MB)"

# ---------------------------------------------------------------- boot --
CUR_OUT="$OUTBASE/run-$LABEL"
CUR_PFDATA="$CUR_OUT/pfdata"
P() { PI_FLASH_DIR="$CUR_PFDATA" "$ROOT/target/debug/pif-ui.exe" "$@"; }
rm -rf "$CUR_OUT"
mkdir -p "$CUR_PFDATA"

# shellcheck disable=SC2086
env PI_CODING_AGENT_SESSION_DIR="$FIXROOT/sessions" \
    ${VENDOR:+PI_FLASH_VENDOR_DIR="$VENDOR"} \
    PI_FLASH_DIR="$CUR_PFDATA" PI_FLASH_AUTOMATION=auto \
    "$EXE" > "$CUR_OUT/app.log" 2>&1 &
APP_PID=$!
ok=0
for i in $(seq 1 80); do ls "$CUR_PFDATA"/automation/*.json >/dev/null 2>&1 && ok=1 && break; sleep 0.25; done
[ "$ok" = 1 ] || { echo "FATAL: automation 登记未出现"; kill $APP_PID 2>/dev/null; exit 2; }
P wait --path app.booted --truthy --timeout 60 >/dev/null || { echo "FATAL: app.booted 超时"; P exec app.quit >/dev/null 2>&1; kill $APP_PID 2>/dev/null; exit 2; }
APPWIN=$(basename "$CUR_PFDATA"/automation/*.json | head -1 | sed 's/\.json//')

MSYS2_ARG_CONV_EXCL='*' P exec project.switch --arg cwd="$FIXROOT/cwd" >/dev/null || { echo "FATAL: project.switch 失败"; P exec app.quit >/dev/null 2>&1; kill $APP_PID 2>/dev/null; exit 2; }

# ------------------------------------------------------- load session --
FIXPATH=$(P snapshot sessions 2>/dev/null | node -e 'let raw="";process.stdin.on("data",c=>raw+=c).on("end",()=>{try{const d=JSON.parse(raw);const s=(d.sessions||[]).find(x=>x.path.replace(/\\/g,"/").includes("bench-flood"));console.log(s?s.path:"")}catch(e){console.log("")}})')
[ -n "$FIXPATH" ] || { echo "FATAL: 会话未被发现（sessions 根/slug 不符）"; P exec app.quit >/dev/null 2>&1; kill $APP_PID 2>/dev/null; exit 2; }
MSYS2_ARG_CONV_EXCL='*' P exec session.open --arg path="$FIXPATH" >/dev/null || { echo "FATAL: session.open 失败"; P exec app.quit >/dev/null 2>&1; kill $APP_PID 2>/dev/null; exit 2; }

# 等装载完成：active_msgs > 0（get_messages 投影 + 叶链重建落位）。
# 同时一个 PowerShell 采样器在后台跑 15s（200ms/次）：装载期解析峰值
# （047-P1-1 move 语义收益形态）+ 收场稳态一起拿。
(
  powershell -NoProfile -Command "
\$ws=@()
for (\$i=0; \$i -lt 200; \$i++) {
  \$p = Get-Process -Id $APPWIN -ErrorAction SilentlyContinue
  if (\$p) { \$ws += \$p.WorkingSet64 }
  Start-Sleep -Milliseconds 200
}
if (\$ws.Count -gt 0) {
  Write-Output ('peak_ws=' + [math]::Round((\$ws | Measure-Object -Maximum).Maximum/1MB,1))
  Write-Output ('last_ws=' + [math]::Round(\$ws[\$ws.Count-1]/1MB,1))
}
" > "$CUR_OUT/samples.txt" 2>/dev/null
) &
SAMPLER=$!

loaded=0
for i in $(seq 1 150); do
  n=$(P snapshot sessions --only mem 2>/dev/null | node -e 'let raw="";process.stdin.on("data",c=>raw+=c).on("end",()=>{try{const d=JSON.parse(raw);console.log(d.mem&&d.mem.active_msgs||0)}catch(e){console.log(0)}})')
  [ "$n" -gt 0 ] 2>/dev/null && loaded=1 && break
  sleep 0.5
done
[ "$loaded" = 1 ] || { echo "FATAL: 会话装载超时（active_msgs=0）"; kill $SAMPLER 2>/dev/null; P exec app.quit >/dev/null 2>&1; kill $APP_PID 2>/dev/null; exit 2; }
sleep 3   # 缓存收尾（tool_args/markdown 等惰性结构落定）

# ------------------------------------------------------------ sampling --
wait $SAMPLER 2>/dev/null   # 采样器固定 15s 窗，必须等它写完 samples.txt
PRIV=$(powershell -NoProfile -Command "
\$p = Get-Process -Id $APPWIN -ErrorAction SilentlyContinue
if (\$p) { [math]::Round(\$p.PrivateMemorySize64/1MB,1) } else { Write-Output '-1' }
" 2>/dev/null | tr -d '\r' | tail -1)
PEAK=$(grep -oE "peak_ws=[0-9.]+" "$CUR_OUT/samples.txt" 2>/dev/null | cut -d= -f2)
LASTWS=$(grep -oE "last_ws=[0-9.]+" "$CUR_OUT/samples.txt" 2>/dev/null | cut -d= -f2)

MEMPROBE=$(P snapshot sessions --only mem 2>/dev/null | tr -d '\n' | head -c 400)
echo "BENCH label=$LABEL exe=$EXE file_mb=$FILE_MB peak_ws_mb=$PEAK ws_mb=$LASTWS priv_mb=$PRIV"
echo "BENCHMEM $MEMPROBE"

P exec app.quit >/dev/null 2>&1
sleep 0.5
kill $APP_PID >/dev/null 2>&1
wait $APP_PID 2>/dev/null
exit 0
