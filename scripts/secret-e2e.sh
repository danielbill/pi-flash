#!/usr/bin/env bash
# 042-1 凭据取回链路端到端：pif-secret.exe 随包分发（用例：test/042-1/pif-secret-packaging/case.md）
#
# 验什么：用户拿到**发版包**后存 key → auth.json 标记指向**包内** pif-secret.exe
#   （不是 target/debug）→ 包内 helper 能从系统凭据库取回 → pi 经它发起真实请求
#   → 藏起 helper 即断、放回即通。全程隔离目录 + pif-ui 自动化，不抢真实屏幕，
#   不碰真实 ~/.pi 与 ~/.pi-flash。
#
# 夹具（tmp/secret-e2e/）：
#   pkg/     模拟发版包 = target/release/{pi-flash,pif-secret}.exe + node.exe + vendor/pi
#            （与 release.sh 第 3 步同构，不动版本号/CHANGELOG）
#   pf/      PI_FLASH_DIR（PF 自有文件）
#   agent/   PI_CODING_AGENT_DIR（pi 的 auth.json / models.json / settings.json 落这里；
#            PF 与 pi 子进程读同一个 env，pi 原生 getAgentDir() 同名）
#
# 用法：
#   scripts/secret-e2e.sh [--provider <id>] [--base <url>] [--model <id>] [--keep]
#   E2E_SECRET_KEY=<真 key> scripts/secret-e2e.sh --base https://api.deepseek.com --model deepseek-chat
# 不给 E2E_SECRET_KEY：用假 key 验 TC1/2a/2b/2c + TC3 的 helper 缺失分支，跳过真对话。
# 退出码 0 = 全绿。
set -uo pipefail
cd "$(dirname "$0")/.."
ROOT_W=$(pwd -W)                     # D:/ai_workspace/pi-flash（Windows 侧路径，给 app 用）
T=tmp/secret-e2e
T_WIN=${T//\//\\}
PUI=target/debug/pif-ui.exe
REL=target/release
PROVIDER=e2e-secret
BASE=https://api.deepseek.com
MODEL=deepseek-chat
KEEP=0
KEY=${E2E_SECRET_KEY:-}
REAL_KEY=1
[ -n "$KEY" ] || { KEY="sk-e2e-fake-$(date +%s)"; REAL_KEY=0; }

while [ $# -gt 0 ]; do
  case "$1" in
    --provider) PROVIDER=$2; shift 2 ;;
    --base) BASE=$2; shift 2 ;;
    --model) MODEL=$2; shift 2 ;;
    --keep) KEEP=1; shift ;;
    *) echo "未知参数：$1"; exit 2 ;;
  esac
done

pass=0; fail=0
ok()   { pass=$((pass+1)); echo "  ✓ $1"; }
bad()  { fail=$((fail+1)); echo "  ✗ $1${2:+  —— $2}"; }
step() { echo -e "\n$1"; }

# 只认本夹具目录下的 pi-flash 进程，绝不碰用户真实实例（upd-e2e.sh 同款）
test_pids() {
  powershell.exe -NoProfile -Command \
    "Get-CimInstance Win32_Process -Filter \"Name='pi-flash.exe'\" | Where-Object { \$_.ExecutablePath -like '*${ROOT_W//\//\\}\\$T_WIN*' } | Select-Object -ExpandProperty ProcessId" \
    2>/dev/null | tr -d '\r' | grep -E '^[0-9]+$' || true
}
kill_test_instances() {
  for pid in $(test_pids); do
    powershell.exe -NoProfile -Command "Stop-Process -Id $pid -Force" 2>/dev/null
  done
  sleep 1
}
cleanup_on_exit() {
  kill_test_instances
  [ "$KEEP" = 1 ] || rm -rf "$T"
}
trap cleanup_on_exit EXIT

# --- 前置 ---------------------------------------------------------------
[ -x "$PUI" ] || { echo "缺 $PUI —— 先跑 cargo build"; exit 2; }
[ -x "$REL/pi-flash.exe" ] || { echo "缺 $REL/pi-flash.exe —— 先跑 cargo build --release -p app"; exit 2; }
[ -x "$REL/pif-secret.exe" ] || { echo "缺 $REL/pif-secret.exe —— 先跑 cargo build --release -p pi-link --bin pif-secret"; exit 2; }
NODE_BIN=$(command -v node || true)
[ -n "$NODE_BIN" ] || { echo "找不到 node"; exit 2; }
[ -f vendor/pi/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js ] || {
  echo "vendor/pi 未安装（cd vendor/pi && npm ci）"; exit 2; }
echo "provider=$PROVIDER base=$BASE model=$MODEL 真key=$REAL_KEY"

# --- 装配模拟包（release.sh 第 3 步同构）---------------------------------
kill_test_instances
rm -rf "$T"
mkdir -p "$T/pkg/vendor" "$T/pf" "$T/agent" "$T/proj"
cp "$REL/pi-flash.exe" "$REL/pif-secret.exe" "$T/pkg/"
cp "$NODE_BIN" "$T/pkg/node.exe"
# vendor/pi 硬链接 = 秒级、不复制 300MB；跨卷失败回落复制
cp -al vendor/pi/. "$T/pkg/vendor/pi/" 2>/dev/null || cp -a vendor/pi/. "$T/pkg/vendor/pi/"
# 新会话默认落到测试商（pi 读 settings.json defaultProvider/defaultModel）
printf '{"defaultProvider":"%s","defaultModel":"%s"}\n' "$PROVIDER" "$MODEL" > "$T/agent/settings.json"

PKG_W="$ROOT_W/$T/pkg"                  # 正斜杠绝对路径（marker_key 的写入形态）
export PI_FLASH_DIR="$ROOT_W/$T/pf"
export PI_CODING_AGENT_DIR="$ROOT_W/$T/agent"
export PI_FLASH_AUTOMATION=auto
export PI_FLASH_UPDATE_CHECK=0

launch_pkg() {
  nohup "$T/pkg/pi-flash.exe" >> "$T/app.log" 2>&1 &
}
wait_booted() {
  for _ in $(seq 1 30); do
    if $PUI wait --path app.booted --truthy --timeout 5 >/dev/null 2>&1; then return 0; fi
    sleep 2
  done
  return 1
}
quit_pkg() {
  $PUI exec app.quit >/dev/null 2>&1
  for _ in $(seq 1 15); do [ -z "$(test_pids)" ] && break; sleep 1; done
  kill_test_instances
  $PUI clean >/dev/null 2>&1
}
# 等本轮结束 + 等首条消息（草稿懒起：pi 进程在首条 prompt 才 spawn，
# session.new 后 has_process=false 是正常的，不能等它）
wait_turn_done() {
  $PUI wait --path session.agent_running --eq false --timeout "${1:-120}" >/dev/null 2>&1
}
# 真回复 = assistant 且 text 非空。pi 请求失败/重试会落 text 为空的 assistant
# 条目（helper 缺失时实测 4 条空 assistant），只看 role 会误判成功
has_reply() {
  $PUI snapshot session --only messages 2>/dev/null \
    | grep -A1 '"role": "assistant"' | grep -q '"text": "[^"]'
}
# 发消息并轮询到真回复出现（或超时）。user 消息本地即落 transcript，
# count>0 不代表 pi 已响应；pi spawn（node 冷启）+ 模型解析 + 首 token 要十几秒
send_and_wait() { # send_and_wait <text> <超时秒> → 0 = 出现非空 assistant 回复
  $PUI exec session.send --arg text="$1" >/dev/null 2>&1
  local deadline=$((SECONDS+$2))
  while [ $SECONDS -lt $deadline ]; do
    if has_reply; then wait_turn_done 60; return 0; fi
    sleep 3
  done
  return 1
}
last_msgs() { $PUI snapshot session --only messages 2>/dev/null; }

# --- TC1 组装清单 ---------------------------------------------------------
step "TC1 组装：pif-secret.exe 与 pi-flash.exe 同目录"
[ -f "$T/pkg/pif-secret.exe" ] && [ -f "$T/pkg/pi-flash.exe" ] \
  && ok "pkg/ 下两 exe 同目录在位" || bad "pkg/ 缺 exe"

# --- 启动包内实例 ---------------------------------------------------------
step "启动包内 pi-flash.exe（隔离 PI_FLASH_DIR / PI_CODING_AGENT_DIR）"
: > "$T/app.log"
launch_pkg
wait_booted && ok "包内实例已启动" || { bad "包内实例起不来" "$(tail -20 "$T/app.log")"; exit 1; }

# --- TC2a 存 key 走真实保存链路 -------------------------------------------
step "TC2a 设置页新建自定义商并保存 key（settings.mj_* 直调真实链路）"
$PUI exec settings.open >/dev/null 2>&1
$PUI wait --path settings.open --truthy --timeout 10 >/dev/null 2>&1 || bad "设置页没开"
$PUI exec settings.mj_new >/dev/null 2>&1
$PUI exec settings.mj_form --arg name="$PROVIDER" --arg base="$BASE" --arg key="$KEY" >/dev/null 2>&1
snap=$($PUI snapshot settings 2>/dev/null)
echo "$snap" | grep -q "\"mj_name_value\": \"$PROVIDER\"" && ok "表单 name 已填" || bad "表单 name 没填上" "$snap"
$PUI exec settings.mj_save >/dev/null 2>&1
$PUI wait --path settings.mj_providers --contains "$PROVIDER" --timeout 15 >/dev/null 2>&1 \
  && ok "models.json 出现商 $PROVIDER" || bad "保存后 models.json 没有该商" "$($PUI snapshot settings --only error 2>/dev/null)"
err=$($PUI snapshot settings --only error 2>/dev/null)
echo "$err" | grep -q '"error": null' && ok "保存无报错" || bad "保存报错" "$err"
# 给该商加一个模型（商保存只写商条目不带模型；pi 解析 defaultModel 需要模型在册）
$PUI exec settings.mj_select --arg section="mj:p:$PROVIDER" >/dev/null 2>&1
$PUI exec settings.mj_add_model >/dev/null 2>&1
$PUI exec settings.mj_model_form --arg id="$MODEL" --arg ctx=128000 --arg max_tokens=8192 >/dev/null 2>&1
$PUI exec settings.mj_model_save >/dev/null 2>&1
grep -q "\"$MODEL\"" "$T/agent/models.json" 2>/dev/null \
  && ok "模型 $MODEL 已落 models.json" || bad "模型没落盘" "$($PUI snapshot settings --only error 2>/dev/null)"

# --- TC2b 标记指向包内 ----------------------------------------------------
step "TC2b auth.json 标记指向包内 helper（不是 target/debug）"
AUTH="$T/agent/auth.json"
[ -f "$AUTH" ] && ok "隔离 auth.json 已生成（真实 ~/.pi 未动）" || bad "隔离 auth.json 不存在"
marker=$(grep -o "!'[^']*' get $PROVIDER" "$AUTH" 2>/dev/null | head -1)
expect="!'$PKG_W/pif-secret.exe' get $PROVIDER"
# 盘符大小写不敏感比对（current_exe 可能回 D:/ 也可能 d:/）
if [ "${marker,,}" = "${expect,,}" ]; then ok "标记 = $marker"; else bad "标记不对" "got=[$marker] want=[$expect]"; fi
case "${marker,,}" in *target/debug*) bad "标记仍指向 target/debug";; *) ok "标记不含 target/debug";; esac
if grep -q "\"$PROVIDER\"" "$AUTH" && ! grep -qF "$KEY" "$AUTH"; then
  ok "auth.json 无明文 key"
else
  bad "auth.json 里出现明文 key 或缺该商条目"
fi

# --- TC2c 包内 helper 手动取回 --------------------------------------------
step "TC2c 包内 pif-secret.exe get → 凭据库取回"
got=$("$T/pkg/pif-secret.exe" get "$PROVIDER" 2>/dev/null); rc=$?
[ "$rc" = 0 ] && [ "$got" = "$KEY" ] && ok "helper 取回 == 保存值（exit 0）" || bad "helper 取回失败" "rc=$rc got=[$got]"
# 从任意 cwd 执行同样成立（pi 执行时 cwd 是项目目录）
got2=$(cd / && "$ROOT_W/$T/pkg/pif-secret.exe" get "$PROVIDER" 2>/dev/null)
[ "$got2" = "$KEY" ] && ok "任意 cwd 下取回一致" || bad "换 cwd 取回不一致"

# --- TC2d 端到端（真 key 才跑）-------------------------------------------
$PUI exec settings.close >/dev/null 2>&1
if [ "$REAL_KEY" = 1 ]; then
  step "TC2d 端到端：开项目发消息 → pi 经包内 helper 取 key → 真回复"
  $PUI exec project.new_session_in --arg cwd="$ROOT_W/$T/proj" >/dev/null 2>&1
  if send_and_wait "Reply with the single word: pong" 180; then
    msgs=$(last_msgs)
    ok "收到 assistant 回复"
    echo "$msgs" | grep -qi 'api key\|unauthorized\|401\|no credentials\|not configured' \
      && bad "回复里出现凭据错误" "$msgs" || ok "无凭据类错误"
  else
    bad "180s 内无 assistant 回复" "status=$($PUI snapshot session --only status 2>/dev/null | tr -d '\n') model=$($PUI snapshot session --only model 2>/dev/null | tr -d '\n ') msgs=$(last_msgs | tr -d '\n ')"
  fi
else
  step "TC2d 端到端：跳过（未给 E2E_SECRET_KEY）"
fi

# --- TC3 反向：藏起 helper → 断；放回 → 通 --------------------------------
step "TC3 反向：藏起包内 pif-secret.exe（pi 对 !cmd 按进程缓存，须重启实例）"
quit_pkg
mv "$T/pkg/pif-secret.exe" "$T/pkg/pif-secret.exe.bak"
if "$T/pkg/pif-secret.exe" get "$PROVIDER" >/dev/null 2>&1; then
  bad "helper 藏起后仍可执行？"
else
  ok "helper 藏起后标记命令不可执行"
fi
if [ "$REAL_KEY" = 1 ]; then
  launch_pkg
  wait_booted || bad "藏 helper 后实例起不来" "$(tail -20 "$T/app.log")"
  $PUI exec project.new_session_in --arg cwd="$ROOT_W/$T/proj" >/dev/null 2>&1
  if send_and_wait "ping" 60; then
    bad "helper 缺失却仍收到非空 assistant 回复（桥没被用到？）" "$(last_msgs | tr -d '\n ')"
  else
    ok "helper 缺失 → 60s 无真回复（符合用户机缺包时的表现）"
  fi
  quit_pkg
fi
mv "$T/pkg/pif-secret.exe.bak" "$T/pkg/pif-secret.exe"
got3=$("$T/pkg/pif-secret.exe" get "$PROVIDER" 2>/dev/null)
[ "$got3" = "$KEY" ] && ok "helper 放回 → 取回恢复" || bad "放回后取回失败"
if [ "$REAL_KEY" = 1 ]; then
  launch_pkg
  wait_booted || bad "放回后实例起不来"
  $PUI exec project.new_session_in --arg cwd="$ROOT_W/$T/proj" >/dev/null 2>&1
  if send_and_wait "Reply with the single word: pong" 180; then
    ok "放回后端到端恢复"
  else
    bad "放回后 180s 仍无回复" "status=$($PUI snapshot session --only status 2>/dev/null | tr -d '\n') model=$($PUI snapshot session --only model 2>/dev/null | tr -d '\n ')"
  fi
fi

# --- 清理：走真实删除链路（删标记 + 删钥匙库条目）--------------------------
step "清理：settings.mj_delete 删商（auth.json 标记 + 凭据库条目同步清）"
[ -n "$(test_pids)" ] || { launch_pkg; wait_booted; }
$PUI exec settings.open >/dev/null 2>&1
$PUI exec settings.mj_delete --arg name="$PROVIDER" >/dev/null 2>&1
sleep 1
grep -q "get $PROVIDER" "$AUTH" 2>/dev/null && bad "auth.json 标记没删干净" || ok "auth.json 标记已删"
if "$T/pkg/pif-secret.exe" get "$PROVIDER" >/dev/null 2>&1; then
  bad "凭据库条目没删干净（PF_KEY_* 残留）"
else
  ok "凭据库条目已删"
fi
quit_pkg

echo
echo "通过 $pass / 失败 $fail"
[ "$fail" = 0 ]
