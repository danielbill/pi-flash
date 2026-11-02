#!/usr/bin/env bash
# 042 模型菜单：新会话点行出 ✓ / pi 启动即死后仍可选可发（2026-11-02 真机回归锁）
# 离线、确定性：不消费网络（pi 启动即死走不到 prompt；重 spawn 只断言 get_state
# 回包；auth 是假 key，即便真发 prompt 也是 401）。
# 隔离：PI_FLASH_DIR + PI_CODING_AGENT_DIR 指进 tmp；settings.json 不带 packages，
# 避免 pi 首启 npm 装扩展（60s+）。
#
# 背景：pi 1.5.0（5f754ed0）models.json 校验变严 → 商不存在时 `--model` 直接
# `Error: Model not found` 退出。暴露 PF 两处老缺口：① 退出臂不清 agent.session →
# 点行 SetModel 写死管道、发送不重 spawn（列表选不中、消息转圈）；② 菜单 ✓ 只读
# state.model，草稿态 pending_model 永远不出 ✓。
# 已知行为（fixture 依赖）：已知商下的未知模型 id pi 只警告并合成，不退出——
# 所以死亡场景用「不存在的商」。
set -u
ROOT="D:/ai_workspace/pi-flash"
PUI="$ROOT/target/debug/pif-ui.exe"
EXE="$ROOT/target/debug/pi-flash.exe"
T="$ROOT/tmp/ui-test-picker-current"

PASS=0; FAIL=0
declare -a RESULTS
check() { local case="$1" desc="$2"; shift 2
  if "$@" >/dev/null 2>&1; then RESULTS+=("PASS  $case  $desc"); PASS=$((PASS+1))
  else RESULTS+=("FAIL  $case  $desc"); FAIL=$((FAIL+1)); fi; }
jget() { node -e "const j=JSON.parse(process.argv[1]);let v;try{v=eval('j'+process.argv[2])}catch{v=undefined};console.log(JSON.stringify(v===undefined?null:v))" "$("$PUI" snapshot "$1" 2>/dev/null)" "$2"; }

rm -rf "$T"; mkdir -p "$T/pi-agent"
export PI_FLASH_DIR="$T" PI_CODING_AGENT_DIR="$T/pi-agent"
"$PUI" clean >/dev/null 2>&1 || true
cat > "$PI_CODING_AGENT_DIR/models.json" <<'J'
{"providers":{"myds":{"api":"openai-completions","name":"myds","baseUrl":"https://api.deepseek.com","models":[
  {"id":"deepseek-flash","name":"DeepSeek-V4.1-Flash","contextWindow":1048576},
  {"id":"deepseek-v4-pro","name":"DeepSeek-V4-Pro","contextWindow":1048576}]}}}
J
echo '{"enabledModels":["myds/*"]}' > "$PI_CODING_AGENT_DIR/settings.json"
echo '{"myds":{"type":"api_key","key":"sk-test-not-real"}}' > "$PI_CODING_AGENT_DIR/auth.json"
# 星标指向 pi 不认识的**商**：全新草稿 spawn 带 --model nope/ghost → pi 启动即退出
echo '{"default_provider":"nope","default_model":"ghost"}' > "$T/app-settings.json"

PI_FLASH_AUTOMATION=auto PI_FLASH_RPC_LOG="$T/rpc.log" "$EXE" > "$T/app.log" 2>&1 &
APP_PID=$!
cleanup() { "$PUI" exec app.quit >/dev/null 2>&1 || kill "$APP_PID" >/dev/null 2>&1 || true; }
trap cleanup EXIT
BOOTED=0
for i in $(seq 1 15); do
  "$PUI" wait --path app.booted --truthy --timeout 10 >/dev/null 2>&1 && { BOOTED=1; break; }; sleep 2
done
[ "$BOOTED" = 1 ] || { echo "FATAL: app 未启动"; tail -20 "$T/app.log"; exit 1; }

# ---------- CASE1 pi 启动即死 → 死因进状态条 → 死会话仍可选 → 发送重 spawn ----------
"$PUI" exec session.send --arg text=hi >/dev/null
check CASE1 "pi 退出原因挂进 status（Model not found）" "$PUI" wait --path session.status --contains "not found" --timeout 30
check CASE1 "死进程不再转圈（无 streaming）" node -e "process.exit(process.argv[1]==='true'?1:0)" "$(jget session .state.is_streaming)"
"$PUI" exec model.picker_open >/dev/null
check CASE1 "死会话菜单：✓ 槽为空（state 未到过模型）" test "$(jget dialogs .model_current_ref)" = null
"$PUI" exec model.pick --arg provider=myds --arg id=deepseek-v4-pro >/dev/null; sleep 1
check CASE1 "死会话里点行仍生效（pending 记下、pill 变）" test "$(jget session .draft_model.id)" = '"deepseek-v4-pro"'
"$PUI" exec model.picker_open >/dev/null
check CASE1 "✓ 在新选的行" test "$(jget dialogs .model_current_ref)" = '"myds/deepseek-v4-pro"'
"$PUI" exec model.pick --arg provider=myds --arg id=deepseek-v4-pro >/dev/null
"$PUI" exec session.send --arg text=hi >/dev/null
check CASE1 "发送 → 重 spawn 带 --model → pi 回 get_state（model 就位）" "$PUI" wait --path session.model.id --eq deepseek-v4-pro --timeout 60
check CASE1 "rpc：首次 spawn nope/ghost，重 spawn myds/deepseek-v4-pro" node -e "
const s=require('fs').readFileSync(process.argv[1],'utf8');const sp=s.split('\n').filter(l=>l.startsWith('=== spawn'));
process.exit(sp.length>=2&&sp[0].includes('nope/ghost')&&sp[sp.length-1].includes('myds/deepseek-v4-pro')?0:1)" "$T/rpc.log"

# ---------- CASE2 新会话草稿：点行 → ✓ 在该行、pill 同步 ----------
# 在已重 spawn 的活会话上开草稿态流程：新 draft 无 state，✓ 只能来自 pending
"$PUI" exec session.new >/dev/null; sleep 1
"$PUI" exec model.picker_open >/dev/null
check CASE2 "新草稿菜单有行" node -e "process.exit(Number(process.argv[1])>=2?0:1)" "$(jget dialogs .model_rows)"
check CASE2 "草稿未手选：无 ✓" test "$(jget dialogs .model_current_ref)" = null
"$PUI" exec model.pick --arg provider=myds --arg id=deepseek-flash >/dev/null; sleep 1
check CASE2 "点行即收菜单" test "$(jget dialogs .model_rows)" = 0
check CASE2 "pill = 所选行（pending 优先于兜底）" test "$(jget session .draft_model.id)" = '"deepseek-flash"'
"$PUI" exec model.picker_open >/dev/null
check CASE2 "重开菜单：✓ 在所选行" test "$(jget dialogs .model_current_ref)" = '"myds/deepseek-flash"'
"$PUI" exec model.pick --arg provider=myds --arg id=deepseek-v4-pro >/dev/null; sleep 1
"$PUI" exec model.picker_open >/dev/null
check CASE2 "再点另一行：✓ 跟着走" test "$(jget dialogs .model_current_ref)" = '"myds/deepseek-v4-pro"'

printf '%s\n' "${RESULTS[@]}"
echo "---- PASS=$PASS FAIL=$FAIL"
[ "$FAIL" = 0 ]
