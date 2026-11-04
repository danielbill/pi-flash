#!/bin/bash
# test/ext-ui：扩展 UI 调用链路自动化台架（bead pi-flash-zi2s.1）。
#
# 一条命令跑完全部扩展 UI 用例，人工不点：
#   bash test/ext-ui/run.sh            # 全量
#   bash test/ext-ui/run.sh C1 C5      # 只跑指定用例
#
# 载体两类：
#   real — 真实 vendored pi + fake-ext.ts（session-ext.json 按 active key
#          挂载，spawn 前写台账即可：mount 路径是 spawn 时现读的）；
#   mock — PI_FLASH_VENDOR_DIR 指向 mock-pi.mjs 生成的假 vendor 树，
#          注入协议边界事件（未知 method / 进程崩溃）。
#
# 断言全部走 pif-ui snapshot / wire 日志（PI_FLASH_RPC_LOG），不碰像素，
# 不抢真实屏幕。baseline（2026-11-04，修复前）期望结果固化在下方各 case
# 的 EXPECT 注释与 README.md。
set -u

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TESTDIR="$ROOT/test/ext-ui"
EXE="$ROOT/target/debug/pi-flash.exe"
PUI="$ROOT/target/debug/pif-ui.exe"
OUTBASE="$ROOT/tmp/ext-ui"
FAKE_EXT="$TESTDIR/fake-ext.ts"
NODE="${PI_FLASH_NODE:-node}"

[ -x "$EXE" ] || { echo "FATAL: $EXE 不存在，先 cargo build"; exit 2; }
[ -x "$PUI" ] || { echo "FATAL: $PUI 不存在，先 cargo build"; exit 2; }
command -v "$NODE" >/dev/null 2>&1 || { echo "FATAL: node 不可用"; exit 2; }

# ---------------------------------------------------------------- helpers --
PASS=0; FAIL=0; XFAIL=0; XPASS=0
RESULTS=()
# expect <期望标记 expect-fail|expect-pass> <用例名> <说明> <退出码0/1>
expect() {
  local kind="$1" name="$2" desc="$3" rc="$4"
  if [ "$rc" -eq 0 ]; then
    if [ "$kind" = "expect-fail" ]; then XPASS=$((XPASS+1)); RESULTS+=("XPASS  $name  $desc（基线已翻案：更新本台架与 bead）")
    else PASS=$((PASS+1)); RESULTS+=("PASS   $name  $desc"); fi
  else
    if [ "$kind" = "expect-fail" ]; then XFAIL=$((XFAIL+1)); RESULTS+=("XFAIL  $name  $desc（基线已知缺陷，见 README）")
    else FAIL=$((FAIL+1)); RESULTS+=("FAIL   $name  $desc"); fi
  fi
  return 0
}

# 每用例独立实例与目录
CUR=""
CUR_OUT=""; CUR_PFDATA=""; CUR_WIRE=""; CUR_APP_PID=""
P() { PI_FLASH_DIR="$CUR_PFDATA" "$PUI" "$@"; }

start_app() { # start_app <vendor_mode real|mock> <scenario>
  # 目录可能被上一轮残留句柄锁住（Windows）：换 .PID 后缀目录兜底
  rm -rf "$CUR_OUT" 2>/dev/null
  mkdir -p "$CUR_PFDATA" "$CUR_OUT/cwd" 2>/dev/null || {
    CUR_OUT="$CUR_OUT.$$"; CUR_PFDATA="$CUR_OUT/pfdata"; CUR_WIRE="$CUR_OUT/wire.log"
    mkdir -p "$CUR_PFDATA" "$CUR_OUT/cwd"
  }
  local envs="PI_FLASH_DIR=$CUR_PFDATA PI_FLASH_AUTOMATION=auto PI_FLASH_RPC_LOG=$CUR_WIRE"
  if [ "$1" = "mock" ]; then
    "$NODE" "$TESTDIR/mock-pi.mjs" emit "$CUR_OUT/mockvendor" "$2" >/dev/null || return 1
    envs="$envs PI_FLASH_VENDOR_DIR=$CUR_OUT/mockvendor/pi"
  fi
  env $envs "$EXE" > "$CUR_OUT/app.log" 2>&1 &
  CUR_APP_PID=$!
  local ok=0
  for i in $(seq 1 80); do ls "$CUR_PFDATA"/automation/*.json >/dev/null 2>&1 && ok=1 && break; sleep 0.25; done
  [ "$ok" = 1 ] || { echo "  [boot] automation 登记未出现"; return 1; }
  P wait --path app.booted --truthy --timeout 60 >/dev/null || { echo "  [boot] app.booted 超时"; return 1; }
  P exec project.switch --arg cwd="$CUR_OUT/cwd" >/dev/null || return 1
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

# 写挂载台账 + session.open 预热：台账 key = 会话文件路径（mount 路径是
# spawn 时现读的；open_session 发现台账非空会立即拉起进程——pool.rs 005
# 预热），wire spawn 行由此在发送前出现，守门才有得看。
mount_fake_ext() {
  : > "$CUR_OUT/cwd/ext-sess.jsonl"
  "$NODE" -e 'const fs=require("fs");const[p,k,ext]=process.argv.slice(1);let j={};try{j=JSON.parse(fs.readFileSync(p,"utf8"))}catch{};j[k]=[ext];fs.writeFileSync(p,JSON.stringify(j,null,1));' \
    "$CUR_PFDATA/session-ext.json" "$CUR_OUT/cwd/ext-sess.jsonl" "$FAKE_EXT" || return 1
  P exec session.open --arg path="$CUR_OUT/cwd/ext-sess.jsonl" >/dev/null || return 1
  return 0
}

# 发送前守门：wire spawn 行必须已含 fake-ext 路径，否则 /ext-* 会漏成
# 普通 prompt 真调模型（issue7 实踩：烧配额）。没挂上 = FAIL 不发送。
assert_ext_mounted() {
  for i in $(seq 1 40); do
    grep -q "fake-ext.ts" "$CUR_WIRE" 2>/dev/null && return 0
    sleep 0.25
  done
  echo "  [guard] wire spawn 行未含 fake-ext.ts —— 扩展没挂上，拒绝发送（防烧配额）"
  return 1
}

await_wire() { # await_wire <pattern> <seconds>
  local pat="$1" secs="${2:-10}"
  for i in $(seq 1 $((secs*10))); do
    grep -q "$pat" "$CUR_WIRE" 2>/dev/null && return 0
    sleep 0.1
  done
  return 1
}

# json 判言：jcheck <surface> <node 表达式（读到 const d）> → 0=真
jcheck() { # jcheck <surface> <expr>
  P snapshot "$1" 2>/dev/null | "$NODE" -e 'let raw="";process.stdin.on("data",c=>raw+=c).on("end",()=>{try{const d=JSON.parse(raw);const v=eval("("+process.argv[1]+")");process.exit(v?0:1)}catch(e){process.exit(1)}})' "$2"
}

# ------------------------------------------------------------------ cases --
case_c1() { # notify 冒烟：命令收场链路（基线 PASS）
  start_app real || return 1
  mount_fake_ext && assert_ext_mounted || { quit_app; return 1; }
  MSYS2_ARG_CONV_EXCL='*' P exec session.send --arg text=/ext-notify >/dev/null
  local rc=0
  await_wire 'ext-notify: hello' 8 || rc=1
  # notice 4s 自动消失：先断言它，再等收场
  jcheck session 'd.ext_notice && d.ext_notice.message.includes("hello")' || rc=1
  P wait --path session.waiting --eq false --timeout 15 >/dev/null || rc=1
  P wait --path dialogs.ext_dialog --eq false --timeout 5 >/dev/null || rc=1
  await_wire 'disposition":"handled' 5 || rc=1
  quit_app
  return $rc
}

case_c2() { # setStatus 数据面（渲染 parity = zi2s.4，此处锁数据链路）
  start_app real || return 1
  mount_fake_ext && assert_ext_mounted || { quit_app; return 1; }
  local rc=0
  MSYS2_ARG_CONV_EXCL='*' P exec session.send --arg text=/ext-status >/dev/null
  P wait --path session.waiting --eq false --timeout 15 >/dev/null || rc=1
  jcheck session 'JSON.stringify(d.ext_status).includes("usage") && JSON.stringify(d.ext_status).includes("61%")' || rc=1
  P shot "$CUR_OUT/status.png" >/dev/null 2>&1 || true   # 人工复核渲染位（composer 上方状态行，zi2s.4）
  MSYS2_ARG_CONV_EXCL='*' P exec session.send --arg text=/ext-status-clear >/dev/null
  P wait --path session.waiting --eq false --timeout 15 >/dev/null || rc=1
  sleep 1
  # usage 键摘除即达标（内置 mcp 扩展也会 setStatus，其他键不属于本用例）
  jcheck session 'Array.isArray(d.ext_status) && !JSON.stringify(d.ext_status).includes("usage")' || rc=1
  quit_app
  return $rc
}

case_c3() { # setWidget ANSI 剥离（zi2s.8；基线=原样带 \x1b，XFAIL）
  start_app real || return 1
  mount_fake_ext && assert_ext_mounted || { quit_app; return 1; }
  local rc=0
  MSYS2_ARG_CONV_EXCL='*' P exec session.send --arg text=/ext-widget-ansi >/dev/null
  P wait --path session.waiting --eq false --timeout 15 >/dev/null || rc=1
  # parity 口径：widget 文本不得含 ESC（PlainTextTheme parity）
  jcheck session 'Array.isArray(d.ext_widgets) && d.ext_widgets.length===1 && d.ext_widgets[0].lines.join("").indexOf("\x1b")===-1 && d.ext_widgets[0].lines.join("").includes("RED")' || rc=1
  quit_app
  return $rc
}

case_c4() { # editor 多行弹窗（zi2s.6）：prefill 原样多行 + 开合链路
  start_app real || return 1
  mount_fake_ext && assert_ext_mounted || { quit_app; return 1; }
  local rc=0
  MSYS2_ARG_CONV_EXCL='*' P exec session.send --arg text=/ext-editor >/dev/null
  P wait --path dialogs.ext_dialog_head.method --eq editor --timeout 15 >/dev/null || rc=1
  # prefill 两个 \n 原样进多行输入（折行/丢失/换空格即回归 zi2s.6）
  jcheck dialogs 'd.ext_dialog_head.input === "prefill L1\nprefill L2"' || rc=1
  P exec dialog.ext_respond >/dev/null 2>&1 || rc=1   # 无 value = 取消
  P wait --path dialogs.ext_dialog --eq false --timeout 5 >/dev/null || rc=1
  await_wire 'cancelled":true' 5 || rc=1
  quit_app
  return $rc
}

case_c5() { # select 键盘导航（zi2s.7）
  start_app real || return 1
  mount_fake_ext && assert_ext_mounted || { quit_app; return 1; }
  local rc=0
  MSYS2_ARG_CONV_EXCL='*' P exec session.send --arg text=/ext-select >/dev/null
  P wait --path dialogs.ext_dialog_head.method --eq select --timeout 15 >/dev/null || rc=1
  P keys "down" >/dev/null 2>&1; P keys "down" >/dev/null 2>&1
  # ↓↓ 后高亮 = 第 3 行（index 2，zi2s.7 导航数据出口）
  jcheck dialogs 'd.ext_dialog_head.sel===2' || rc=1
  P keys "enter" >/dev/null 2>&1
  P wait --path dialogs.ext_dialog --eq false --timeout 5 >/dev/null || rc=1
  # ↓↓+Enter = 第三项 gamma（handler 回 notify 带选择值）
  jcheck session '(d.ext_notice||{}).message && d.ext_notice.message.includes("gamma")' || rc=1
  quit_app
  return $rc
}

case_c6() { # 长标题 select（600×600 双面板壳回归，b9e2edce）
  start_app real || return 1
  mount_fake_ext && assert_ext_mounted || { quit_app; return 1; }
  local rc=0
  MSYS2_ARG_CONV_EXCL='*' P exec session.send --arg text=/ext-bigselect >/dev/null
  P wait --path dialogs.ext_dialog_head.method --eq select --timeout 15 >/dev/null || rc=1
  P wait --path dialogs.ext_dialog --eq true --timeout 3 >/dev/null || rc=1
  P shot "$CUR_OUT/bigselect.png" >/dev/null 2>&1 || rc=1
  [ -f "$CUR_OUT/bigselect.png" ] || rc=1
  P exec dialog.ext_respond >/dev/null 2>&1
  quit_app
  return $rc
}

case_c7() { # 未知 extension_ui_method → cancelled + 留痕（zi2s.2；基线整条丢弃，XFAIL）
  start_app mock unknown-method || return 1
  local rc=0
  P exec session.send --arg text=hello >/dev/null
  # mock 注入 method=futureWidget 后等 PF 的 cancelled 回执；mock 收到后回 handled
  await_wire 'extension_ui_response' 8 || rc=1
  await_wire 'cancelled":true' 3 || rc=1
  jcheck dialogs 'd.ext_dialog===false' || rc=1
  P wait --path session.waiting --eq false --timeout 12 >/dev/null || rc=1
  quit_app
  return $rc
}

case_c8() { # pi 进程死亡清弹窗（zi2s.3；基线弹窗残留，XFAIL）
  start_app mock crash-with-dialog || return 1
  local rc=0
  P exec session.send --arg text=hello >/dev/null
  P wait --path dialogs.ext_dialog --eq true --timeout 10 >/dev/null || rc=1
  # mock 4s 后 exit(1) 模拟崩溃 → 弹窗必须被清理
  P wait --path dialogs.ext_dialog --eq false --timeout 12 >/dev/null || rc=1
  P wait --path session.waiting --eq false --timeout 5 >/dev/null || rc=1
  quit_app
  return $rc
}

case_c9() { # 内置斜杠命令客户端拦截（zi2s.5）：wire 断言无 prompt 帧
  start_app mock plain || return 1
  local rc=0
  # /name <name> → set_session_name 命令 + 输入清空 + 不发 prompt。
  # （侧栏名不在此断言：mock 的 get_state 无 sessionName 回显，异步会覆写
  # 乐观写；真机的侧栏名走既有 rename 链 pending_rename → RenameReady）
  MSYS2_ARG_CONV_EXCL='*' P exec session.send --arg text="/name mockname" >/dev/null
  sleep 1
  await_wire '"type":"set_session_name"' 5 || rc=1
  jcheck session 'd.input===""' || rc=1
  # /compact /auto-compact → 对应 RPC 命令
  MSYS2_ARG_CONV_EXCL='*' P exec session.send --arg text=/compact >/dev/null
  sleep 1
  await_wire '"type":"compact"' 5 || rc=1
  MSYS2_ARG_CONV_EXCL='*' P exec session.send --arg text=/auto-compact >/dev/null
  sleep 1
  await_wire '"type":"set_auto_compaction"' 5 || rc=1
  # /session /mcp → 客户端面板（无 wire）
  MSYS2_ARG_CONV_EXCL='*' P exec session.send --arg text=/session >/dev/null
  sleep 1
  jcheck dialogs 'd.dialog==="session_info"' || rc=1
  P exec dialog.close >/dev/null 2>&1
  MSYS2_ARG_CONV_EXCL='*' P exec session.send --arg text=/mcp >/dev/null
  sleep 1
  jcheck dialogs 'd.settings_open===true' || rc=1
  P exec settings.close >/dev/null 2>&1
  # 关键负断言：以上内置命令一个 prompt 都没发过
  if grep -q '"type":"prompt"' "$CUR_WIRE"; then
    echo "  [wire] 内置命令泄漏成 prompt 帧（字面文本进了模型上下文）"
    rc=1
  fi
  # 对照组：普通文本照旧发 prompt
  P exec session.send --arg text=hello >/dev/null
  await_wire '"type":"prompt"' 5 || rc=1
  quit_app
  return $rc
}

# ------------------------------------------------------------------ main --
desc_of() {
  case "$1" in
    C1) echo "notify 冒烟：命令收场链路" ;;
    C2) echo "setStatus 渲染（zi2s.4）" ;;
    C3) echo "setWidget ANSI 剥离（zi2s.8）" ;;
    C4) echo "editor 多行弹窗（zi2s.6）" ;;
    C5) echo "select 键盘导航（zi2s.7）" ;;
    C6) echo "长标题 select 600x600 壳回归" ;;
    C7) echo "未知 method → cancelled 留痕（zi2s.2）" ;;
    C8) echo "pi 崩溃清弹窗（zi2s.3）" ;;
    C9) echo "内置斜杠命令拦截（zi2s.5）" ;;
  esac
}

CASES="${*:-C1 C2 C3 C4 C5 C6 C7 C8 C9}"
mkdir -p "$OUTBASE"
for C in $CASES; do
  CUR="$C"; CUR_OUT="$OUTBASE/$C"; CUR_PFDATA="$CUR_OUT/pfdata"; CUR_WIRE="$CUR_OUT/wire.log"
  echo "== $C =="
  "case_$(printf '%s' "$C" | tr 'A-Z' 'a-z')"
  local_rc=$?
  case "$C" in
    C5) expect expect-pass "$C" "$(desc_of "$C")" "$local_rc" ;;
    *)           expect expect-pass "$C" "$(desc_of "$C")" "$local_rc" ;;
  esac
done

echo
echo "==================== test/ext-ui 结果 ===================="
for r in "${RESULTS[@]}"; do echo "  $r"; done
echo "----------------------------------------------------------"
echo "  PASS=$PASS XFAIL=$XFAIL FAIL=$FAIL XPASS=$XPASS（XFAIL=基线已知缺陷；XPASS=缺陷已修，请翻案基线）"
[ "$FAIL" -eq 0 ] || exit 1
exit 0
