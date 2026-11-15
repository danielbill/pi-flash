#!/bin/bash
# test/047-内存削减：047-P2 三项验收自动化（bead pi-flash-uo0f）。
#
# 用户裁定人工验收不合适，全转 pif-ui 数据面断言（snapshot/exec），不碰
# 像素不抢屏幕。断言载体：
#   • app 快照新键 `tabs`（panel_tabs 全序，打开序）+ `status_toast`
#   • `=== spawn` RPC 日志行新字段 `heap=<mb>`（client.rs 047-P2-1）
#
# 用例：
#   T1 node-heap —— T1 用 **mock vendor**（复用 030-流式卡死台架）：确定性
#                  回合收场 + 零真实模型调用。spawn 触发 = 首条消息
#                  （lazy draft：boot 不 spawn）。第二 spawn 双保险 = 强杀
#                  node（真重启路径，best-effort）+ 第二项目发送（同 spawn
#                  组装函数）。断言：env=256 时每次 spawn 都带 heap=256，
#                  缺省 heap=3072，env=0 显式禁用 heap=0；全程可交互。
#   T2 editor-cap —— 66 文件 fixture：开满 64 后再开，最旧**非脏**被逐
#                  （toast 带 filename，2.5s 窗内断言）；脏文件豁免；全脏
#                  放行 soft cap（第 65 个照开，count=65）。置脏走「复激活
#                  + set_text」——编辑器实体懒创建（渲染帧补，仅可见 tab）。
#   T3 term-cap  —— terminal.open ×17：前 16 次 count 1..16 递增，第 17 个
#                  被拒（count 仍 16 + toast 带 16）。
#
# 一条命令跑完，人工不点：
#   bash test/047-内存削减/run.sh            # 全量
#   bash test/047-内存削减/run.sh T1 T2     # 只跑指定用例
#
# 路径纪律：ROOT 取 pwd -W（Windows 形态）——automation 的 --arg path/cwd
# 直通 handler，MSYS 形态（/d/...）会被 Windows 端 PathBuf 判非法。
set -u

ROOT="$(cd "$(dirname "$0")/../.." && { pwd -W 2>/dev/null || pwd; })"
EXE="$ROOT/target/debug/pi-flash.exe"
PUI="$ROOT/target/debug/pif-ui.exe"
MOCK="$ROOT/test/030-流式卡死/mock-pi-stream.mjs"
OUTBASE="$ROOT/tmp/mem-047"

[ -x "$EXE" ] || { echo "FATAL: $EXE 不存在，先 cargo build"; exit 2; }
[ -x "$PUI" ] || { echo "FATAL: $PUI 不存在，先 cargo build"; exit 2; }
[ -f "$MOCK" ] || { echo "FATAL: mock 台架缺失 $MOCK"; exit 2; }
command -v node >/dev/null 2>&1 || { echo "FATAL: node 不可用"; exit 2; }

# ---------------------------------------------------------------- helpers --
PASS=0; FAIL=0
RESULTS=()
expect() { # expect <用例> <说明> <退出码>
  if [ "$3" -eq 0 ]; then
    PASS=$((PASS+1)); RESULTS+=("PASS   $1  $2")
  else
    FAIL=$((FAIL+1)); RESULTS+=("FAIL   $1  $2")
  fi
  return 0
}

CUR=""; CUR_OUT=""; CUR_PFDATA=""; CUR_WIRE=""; CUR_APP_PID=""; CUR_ENV=""; CUR_VENDOR=""
P() { PI_FLASH_DIR="$CUR_PFDATA" "$PUI" "$@"; }

jcheck() { # jcheck <surface> <js 表达式（d = 快照 JSON）> → 0=真
  P snapshot "$1" 2>/dev/null | node -e 'let raw="";process.stdin.on("data",c=>raw+=c).on("end",()=>{try{const d=JSON.parse(raw);process.exit(eval("("+process.argv[1]+")")?0:1)}catch(e){process.exit(1)}})' "$2"
}

start_app() {
  rm -rf "$CUR_OUT" 2>/dev/null
  mkdir -p "$CUR_PFDATA" "$CUR_OUT/cwd" "$CUR_OUT/cwd2" || return 1
  printf 'fixture\n' > "$CUR_OUT/cwd2/d.txt"
  # mock vendor（030 台架）：清场后生成，零真实模型调用 + 确定性收场
  if [ "${MOCK_NEEDED:-}" = "1" ]; then
    node "$MOCK" emit "$CUR_OUT/mockvendor" exact >/dev/null 2>&1 || { echo "  [boot] mock vendor 生成失败"; return 1; }
    CUR_VENDOR="$CUR_OUT/mockvendor/pi"
  fi
  # shellcheck disable=SC2086
  env $CUR_ENV PI_FLASH_DIR="$CUR_PFDATA" PI_FLASH_AUTOMATION=auto \
      PI_FLASH_RPC_LOG="$CUR_WIRE" ${CUR_VENDOR:+PI_FLASH_VENDOR_DIR="$CUR_VENDOR"} \
      "$EXE" > "$CUR_OUT/app.log" 2>&1 &
  CUR_APP_PID=$!
  local ok=0
  for i in $(seq 1 80); do ls "$CUR_PFDATA"/automation/*.json >/dev/null 2>&1 && ok=1 && break; sleep 0.25; done
  [ "$ok" = 1 ] || { echo "  [boot] automation 登记未出现"; return 1; }
  P wait --path app.booted --truthy --timeout 60 >/dev/null || { echo "  [boot] app.booted 超时"; return 1; }
  return 0
}

quit_app() {
  [ -n "$CUR_APP_PID" ] || return 0
  P exec app.quit >/dev/null 2>&1
  sleep 0.5
  kill "$CUR_APP_PID" >/dev/null 2>&1
  wait "$CUR_APP_PID" 2>/dev/null
  CUR_APP_PID=""
}


spawn_wait() { # spawn_wait <n> —— 等 RPC 日志出现 ≥n 条 === spawn（异步 spawn 泵）
  local n="$1" i
  for i in $(seq 1 30); do
    [ "$(grep -c '=== spawn' "$CUR_WIRE" 2>/dev/null || echo 0)" -ge "$n" ] && return 0
    sleep 0.5
  done
  return 1
}

send_and_spawn() { # send_and_spawn <cwd> <text> —— 切项目 + 发消息（lazy draft 首发拉起 pi）
  MSYS2_ARG_CONV_EXCL='*' P exec project.switch --arg cwd="$1" >/dev/null 2>&1
  P exec session.send --arg text="$2" >/dev/null 2>&1
}

make_fixture() { # make_fixture <n> —— cwd 下 f00.rs .. f<n-1>.rs
  local i
  for i in $(seq 0 $(($1 - 1))); do
    printf 'fixture %d\n' "$i" > "$CUR_OUT/cwd/$(printf 'f%02d.rs' "$i")"
  done
}

abs() { echo "$CUR_OUT/cwd/$(printf 'f%02d.rs' "$1")"; }

open_n() { # open_n <from> <to> —— file.open（首次=开新 tab，重复=复激活）
  local i
  for i in $(seq "$1" "$2"); do
    MSYS2_ARG_CONV_EXCL='*' P exec file.open --arg path="$(abs "$i")" >/dev/null 2>&1 || return 1
  done
}

dirty_stick() { # dirty_stick <ix> —— 复激活 + set_text + 紧跟外部落盘 → Changed 冲突
  # 恒开自动保存（023 用户拍板）1s 静默就会把纯 set_text 洗干净；冲突态
  # autosave 沉默、file_tab_dirty 恒真——live 可造的「持续脏」前提。
  # touch 落在 set_text 后 ~60ms（<<1s），watcher 泵 150ms/轮落 Changed。
  local i="$1" k ok=""
  MSYS2_ARG_CONV_EXCL='*' P exec file.open --arg path="$(abs "$i")" >/dev/null 2>&1 || return 1
  for k in $(seq 1 30); do
    MSYS2_ARG_CONV_EXCL='*' P exec file.set_text --arg path="$(abs "$i")" --arg text="dirty $i" >/dev/null 2>&1 && { ok=1; break; }
    sleep 0.1
  done
  [ -n "$ok" ] || return 1
  printf 'external %d\n' "$i" > "$(abs "$i")"
  return 0
}



# ------------------------------------------------------------------ cases --
case_t1() { # node 堆旗标：env 覆盖 / 缺省 3072 / env=0 禁用；多 spawn 全量带旗
  local rc=0
  # (a) env=256
  CUR_OUT="$OUTBASE/T1-256"; CUR_PFDATA="$CUR_OUT/pfdata"; CUR_WIRE="$CUR_OUT/wire.log"; CUR_ENV="PI_FLASH_NODE_HEAP_MB=256"; CUR_VENDOR=""; MOCK_NEEDED=1
  start_app || return 1
  send_and_spawn "$CUR_OUT/cwd" hi
  spawn_wait 1 || { echo "  [t1] 首条消息未拉起 spawn"; quit_app; return 1; }
  P wait --path session.streaming --eq false --timeout 30 >/dev/null 2>&1
  # 真重启路径（best-effort）：杀 node → 宿主 ensure_spawned 兜底重启
  local child
  child=$(powershell -NoProfile -Command "(Get-CimInstance Win32_Process -Filter \"Name='node.exe'\" | Where-Object { \$_.CommandLine -like '*--max-old-space-size=*' } | Select-Object -First 1 -ExpandProperty ProcessId)" 2>/dev/null | tr -d '\r')
  if [ -n "$child" ]; then
    MSYS2_ARG_CONV_EXCL='*' taskkill /PID "$child" /F >/dev/null 2>&1
    echo "  [t1] 强杀 node pid=$child 触发重启路径"
    P wait --path session.streaming --eq false --timeout 25 >/dev/null 2>&1
    P exec session.send --arg text=hi2 >/dev/null 2>&1
  else
    echo "  [t1] 未定位 node 子进程，跳过强杀"
  fi
  # 第二 spawn 保底：第二项目发送（新 runtime = 同 spawn 组装函数再走一遍）
  send_and_spawn "$CUR_OUT/cwd2" hi3
  spawn_wait 2 || { echo "  [t1] 第二 spawn 未出现"; quit_app; return 1; }
  local sp n256
  sp=$(grep -c "=== spawn" "$CUR_WIRE")
  n256=$(grep -c "heap=256" "$CUR_WIRE")
  echo "  [t1] env=256：spawn $sp 条，heap=256 $n256 条"
  [ "$sp" -ge 2 ] || rc=1
  [ "$n256" -eq "$sp" ] || rc=1
  grep -q "heap=3072" "$CUR_WIRE" && rc=1
  # 多次 spawn 后应用仍可交互（env=256 不崩的自动化等价断言）
  P exec panel.dock --arg panel=default >/dev/null 2>&1 || rc=1
  quit_app
  # (b) 缺省对照：heap=3072
  CUR_OUT="$OUTBASE/T1-def"; CUR_PFDATA="$CUR_OUT/pfdata"; CUR_WIRE="$CUR_OUT/wire.log"; CUR_ENV=""; CUR_VENDOR=""; MOCK_NEEDED=1
  start_app || return 1
  send_and_spawn "$CUR_OUT/cwd" hi
  spawn_wait 1 || { echo "  [t1] 缺省实例无 spawn 行"; quit_app; return 1; }
  local sp2 nd
  sp2=$(grep -c "=== spawn" "$CUR_WIRE")
  nd=$(grep -c "heap=3072" "$CUR_WIRE")
  echo "  [t1] 缺省：spawn $sp2 条，heap=3072 $nd 条"
  [ "$sp2" -ge 1 ] || rc=1
  [ "$nd" -eq "$sp2" ] || rc=1
  grep -q "heap=256" "$CUR_WIRE" && rc=1
  quit_app
  # (c) env=0：显式禁用
  CUR_OUT="$OUTBASE/T1-0"; CUR_PFDATA="$CUR_OUT/pfdata"; CUR_WIRE="$CUR_OUT/wire.log"; CUR_ENV="PI_FLASH_NODE_HEAP_MB=0"; CUR_VENDOR=""; MOCK_NEEDED=1
  start_app || return 1
  send_and_spawn "$CUR_OUT/cwd" hi
  spawn_wait 1 || { echo "  [t1] env=0 实例无 spawn 行"; quit_app; return 1; }
  grep -q "heap=0" "$CUR_WIRE" || rc=1
  grep -q "heap=[1-9]" "$CUR_WIRE" && rc=1
  echo "  [t1] env=0：heap=0（禁用）确认"
  quit_app
  return $rc
}

case_t2() { # 编辑器 soft cap 64：最旧非脏逐出 / 脏豁免 / toast / 全脏放行
  local rc=0
  CUR_OUT="$OUTBASE/T2"; CUR_PFDATA="$CUR_OUT/pfdata"; CUR_WIRE="$CUR_OUT/wire.log"; CUR_ENV=""; CUR_VENDOR=""; MOCK_NEEDED=""
  start_app || return 1
  make_fixture 67
  # 开满 64（第 64 个开完恰在 cap 上，不逐出）
  open_n 0 63 || rc=1
  jcheck app 'd.tabs.filter(t=>t.kind==="file").length===64' || rc=1
  # 置脏 f00 为**冲突态**（复激活 + set_text + 外部落盘）：sticky 脏，
  # 后续逐出步骤断言无 autosave 竞态（file_tab_dirty 的 conflict 分支）
  dirty_stick 0 || { echo "  [t2] f00 stick 失败（编辑器未渲染？）"; quit_app; return 1; }
  sleep 0.4   # watcher 泵 150ms/轮：f00 的 Changed 落位再动下一个
  # 第 65 个：toast 先查（2.5s 窗最紧），结构随后（单快照合断言省往返）
  # 期望：逐出最旧非脏 f01；冲突态 f00 保留；新 tab f64 在带
  MSYS2_ARG_CONV_EXCL='*' P exec file.open --arg path="$(abs 64)" >/dev/null 2>&1 || rc=1
  jcheck app 'd.status_toast && d.status_toast.includes("f01.rs")' || rc=1
  jcheck app 'd.tabs.filter(t=>t.kind==="file").length===64&&d.tabs.some(t=>t.kind==="file"&&t.path.replace(/\\/g,"/").endsWith("/f00.rs"))&&d.tabs.some(t=>t.kind==="file"&&t.path.replace(/\\/g,"/").endsWith("/f64.rs"))&&!d.tabs.some(t=>t.kind==="file"&&t.path.replace(/\\/g,"/").endsWith("/f01.rs"))' || rc=1
  echo "  [t2] 第 65 个：逐 f01、留 f00(脏)、toast f01 ✓"
  # 第 66 个：逐 f02（f00 继续豁免）
  MSYS2_ARG_CONV_EXCL='*' P exec file.open --arg path="$(abs 65)" >/dev/null 2>&1 || rc=1
  jcheck app 'd.status_toast && d.status_toast.includes("f02.rs")' || rc=1
  jcheck app 'd.tabs.filter(t=>t.kind==="file").length===64&&d.tabs.some(t=>t.kind==="file"&&t.path.replace(/\\/g,"/").endsWith("/f00.rs"))&&d.tabs.some(t=>t.kind==="file"&&t.path.replace(/\\/g,"/").endsWith("/f65.rs"))&&!d.tabs.some(t=>t.kind==="file"&&t.path.replace(/\\/g,"/").endsWith("/f02.rs"))' || rc=1
  echo "  [t2] 第 66 个：逐 f02、f00 仍豁免、toast f02 ✓"
  # 全脏放行：f03..f65 全部 stick 成冲突态脏（+f00）→ 开 f66 无逐出对象，
  # soft cap 不硬拒，count=65
  local i
  for i in $(seq 3 65); do dirty_stick "$i" || { echo "  [t2] f$i stick 失败"; quit_app; return 1; }; done
  sleep 0.5   # watcher 泵收尾（末位 stick 的冲突落位）
  MSYS2_ARG_CONV_EXCL='*' P exec file.open --arg path="$(abs 66)" >/dev/null 2>&1 || rc=1
  jcheck app 'd.tabs.filter(t=>t.kind==="file").length===65&&d.tabs.some(t=>t.kind==="file"&&t.path.replace(/\\/g,"/").endsWith("/f00.rs"))' || rc=1
  echo "  [t2] 全脏放行：count=65 ✓"
  quit_app
  return $rc
}

case_t3() { # 终端 cap 16：前 16 递增，第 17 拒 + toast
  local rc=0
  CUR_OUT="$OUTBASE/T3"; CUR_PFDATA="$CUR_OUT/pfdata"; CUR_WIRE="$CUR_OUT/wire.log"; CUR_ENV=""; CUR_VENDOR=""; MOCK_NEEDED=""
  start_app || return 1
  # 终端要落在项目 cwd（无项目态不开）
  MSYS2_ARG_CONV_EXCL='*' P exec project.switch --arg cwd="$CUR_OUT/cwd" >/dev/null 2>&1 || rc=1
  local i n want=0
  for i in $(seq 1 17); do
    n=$(MSYS2_ARG_CONV_EXCL='*' P exec terminal.open 2>/dev/null | node -e 'let raw="";process.stdin.on("data",c=>raw+=c).on("end",()=>{try{const d=JSON.parse(raw);console.log(d.terminals==null?-1:d.terminals)}catch(e){console.log(-1)}})')
    if [ "$i" -le 16 ]; then want=$i; else want=16; fi
    [ "$n" = "$want" ] || { echo "  [t3] 第 $i 次 terminal.open count=$n（期望 $want）"; rc=1; }
  done
  echo "  [t3] 17 次开终端：16 建 1 拒 ✓"
  jcheck app 'd.terminals === 16' || rc=1
  jcheck term 'd.terminals.length === 16' || rc=1
  jcheck app 'd.status_toast && d.status_toast.includes("16")' || rc=1
  quit_app
  return $rc
}

# ------------------------------------------------------------------ main --
desc_of() {
  case "$1" in
    T1) echo "node 堆旗标：env=256/缺省 3072/env=0 三态全量落在每次 spawn，应用不崩" ;;
    T2) echo "编辑器 cap 64：最旧非脏逐出（toast 带 filename）、脏豁免、全脏放行 65" ;;
    T3) echo "终端 cap 16：17 次开终，第 17 拒 + toast" ;;
  esac
}

CASES="${*:-T1 T2 T3}"
mkdir -p "$OUTBASE"
for C in $CASES; do
  CUR="$C"
  echo "== $C =="
  "case_$(printf '%s' "$C" | tr 'A-Z' 'a-z')"
  local_rc=$?
  expect "$C" "$(desc_of "$C")" "$local_rc"
done

echo
echo "==================== test/047-内存削减 结果 ===================="
for r in "${RESULTS[@]}"; do echo "  $r"; done
echo "----------------------------------------------------------"
echo "  PASS=$PASS FAIL=$FAIL"
[ "$FAIL" -eq 0 ] || exit 1
exit 0
