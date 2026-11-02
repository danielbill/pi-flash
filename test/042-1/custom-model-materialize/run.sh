#!/usr/bin/env bash
# 042-1 自定义商：勾选物化落盘 / 详情页一律可编辑 / 存量自愈（0.3.2 回归锁）
# 离线、确定性：预置 models.json / custom-provider.json / settings.json，不走网络、
# 不要 key。运行前提：target/debug 下有 pi-flash.exe + pif-ui.exe（cargo build）。
# 隔离：PI_FLASH_DIR + PI_CODING_AGENT_DIR 都指进 tmp，不碰用户真实 ~/.pi。
#
# 背景（2026-11-02）：253ca038 清单仓分支 push 完直接 return 没落盘 → 勾选的探测
# 模型从未进 models.json → pi 不认识（对话出错）+ 详情页落只读覆盖视图。
set -u
ROOT="D:/ai_workspace/pi-flash"
PUI="$ROOT/target/debug/pif-ui.exe"
EXE="$ROOT/target/debug/pi-flash.exe"
T="$ROOT/tmp/ui-test-mj-materialize"

PASS=0; FAIL=0
declare -a RESULTS
check() { # check <case> <描述> <条件命令...>
  local case="$1" desc="$2"; shift 2
  if "$@" >/dev/null 2>&1; then
    RESULTS+=("PASS  $case  $desc"); PASS=$((PASS+1))
  else
    RESULTS+=("FAIL  $case  $desc"); FAIL=$((FAIL+1))
  fi
}
mj() { "$PUI" exec "$@"; }
snap() { "$PUI" snapshot settings 2>/dev/null; }
# 账本 myds.models[] 里某 id 的条目（JSON 或 null）
ledger_model() { node -e "
const d=JSON.parse(require('fs').readFileSync(process.argv[1],'utf8'));
const m=((((d.providers||{}).myds||{}).models)||[]).find(x=>x.id===process.argv[2]);
console.log(JSON.stringify(m===undefined?null:m))" "$MODELS" "$1"; }
ledger_len() { node -e "
const d=JSON.parse(require('fs').readFileSync(process.argv[1],'utf8'));
console.log(((((d.providers||{}).myds||{}).models)||[]).length)" "$MODELS"; }
store_field() { node -e "
const d=JSON.parse(require('fs').readFileSync(process.argv[1],'utf8'));
const m=(((d.myds||{}).models)||{})[process.argv[2]]||{};
console.log(JSON.stringify(m[process.argv[3]]===undefined?null:m[process.argv[3]]))" "$STORE" "$1" "$2"; }
snap_field() { node -e "console.log(JSON.stringify(JSON.parse(process.argv[1])[process.argv[2]]))" "$(snap)" "$1"; }
has_full_shape() { node -e "
const m=JSON.parse(process.argv[1]);
process.exit(m&&m.api==='openai-completions'&&m.baseUrl==='https://api.deepseek.com'&&Array.isArray(m.input)&&m.cost&&typeof m.reasoning==='boolean'?0:1)" "$1"; }

# ---------- 环境准备 ----------
rm -rf "$T"; mkdir -p "$T/pi-agent"
export PI_FLASH_DIR="$T"
export PI_CODING_AGENT_DIR="$T/pi-agent"
MODELS="$PI_CODING_AGENT_DIR/models.json"
STORE="$T/custom-provider.json"
SETTINGS="$PI_CODING_AGENT_DIR/settings.json"
"$PUI" clean >/dev/null 2>&1 || true

# 预置：骨架商（无 models[]）+ 仓里两条探测定义 + 显式勾选其中一条
# （= 0.3.2 真机故障现场：勾了却没物化）
cat > "$MODELS" <<'EOF'
{"providers":{"myds":{"api":"openai-completions","name":"myds","baseUrl":"https://api.deepseek.com"}}}
EOF
cat > "$STORE" <<'EOF'
{"myds":{"fetchedAt":1,"models":{
  "deepseek-flash":{"id":"deepseek-flash","name":"DeepSeek-V4.1-Flash","contextWindow":1048576},
  "deepseek-v4-pro":{"id":"deepseek-v4-pro","name":"DeepSeek-V4-Pro","contextWindow":1048576}}}}
EOF
cat > "$SETTINGS" <<'EOF'
{"enabledModels":["myds/deepseek-flash"]}
EOF

check CASE0 "预置：账本只有骨架、无 models[]" test "$(ledger_len)" = 0

PI_FLASH_AUTOMATION=auto "$EXE" > "$T/app.log" 2>&1 &
APP_PID=$!
cleanup() { "$PUI" exec app.quit >/dev/null 2>&1 || kill "$APP_PID" >/dev/null 2>&1 || true; }
trap cleanup EXIT
# 登记文件要等 app 起到自动化服务才出现，wait 在那之前会直接失败 → 重试
BOOTED=0
for i in $(seq 1 15); do
  if "$PUI" wait --path app.booted --truthy --timeout 10 >/dev/null 2>&1; then BOOTED=1; break; fi
  sleep 2
done
[ "$BOOTED" = 1 ] || { echo "FATAL: app 未启动"; tail -20 "$T/app.log"; exit 1; }

# ---------- CASE1 存量自愈：启动即对账（Chat::new → reload_settings_panel）→
# 显式勾选却缺账本的模型补物化；未勾选的绝不整批写入 ----------
sleep 1
check CASE1 "启动自愈：deepseek-flash 进 models.json（不必打开设置页）" test "$(ledger_model deepseek-flash)" != null
check CASE1 "自愈条目带 api/baseUrl/input/cost/reasoning（pi 硬要求）" has_full_shape "$(ledger_model deepseek-flash)"
check CASE1 "未勾选的 v4-pro 不被整批写入" test "$(ledger_model deepseek-v4-pro)" = null
check CASE1 "账本恰 1 条" test "$(ledger_len)" = 1
mj settings.open --arg tab=1 >/dev/null
"$PUI" wait --path settings.section --truthy --timeout 10 >/dev/null 2>&1
check CASE1 "打开设置页再对账：幂等不重复" test "$(ledger_len)" = 1

# ---------- CASE2 自定义商详情一律可编辑：仓里有、账本没有 → 可编辑仓草稿 ----------
# 从商详情页点行（真实路径；关闭/保存后退回 mj:p:myds）
mj settings.mj_select --arg section=mj:p:myds >/dev/null
mj settings.mj_open_model --arg provider=myds --arg id=deepseek-v4-pro >/dev/null
check CASE2 "不走只读覆盖视图：section = mj:m:myds:1（追加槽）" "$PUI" wait --path settings.section --eq "mj:m:myds:1" --timeout 5
check CASE2 "表单预填仓定义 id" test "$(snap_field mj_id_value)" = '"deepseek-v4-pro"'
check CASE2 "表单预填仓定义 contextWindow" test "$(snap_field mj_ctx_value)" = '"1048576"'

# ---------- CASE3 仓草稿保存：只进仓，不进 models.json（投影规则） ----------
mj settings.mj_model_form --arg max_tokens='"65536"' --arg reasoning=false >/dev/null
mj settings.mj_model_save >/dev/null
sleep 1
check CASE3 "保存后弹窗收起回商页" "$PUI" wait --path settings.section --eq "mj:p:myds" --timeout 5
check CASE3 "仓定义更新 maxTokens=65536" test "$(store_field deepseek-v4-pro maxTokens)" = 65536
check CASE3 "仓定义 reasoning 显式 false" test "$(store_field deepseek-v4-pro reasoning)" = false
check CASE3 "未勾选 → 不进 models.json" test "$(ledger_model deepseek-v4-pro)" = null
check CASE3 "账本仍 1 条" test "$(ledger_len)" = 1

# ---------- CASE4 勾选物化落盘（根因回归锁：仓命中分支必须写盘） ----------
mj settings.mc_toggle --arg ref=myds/deepseek-v4-pro --arg enable=true >/dev/null
sleep 1
check CASE4 "勾选 → v4-pro 进 models.json" test "$(ledger_model deepseek-v4-pro)" != null
check CASE4 "物化带完整形状" has_full_shape "$(ledger_model deepseek-v4-pro)"
check CASE4 "仓里手调的 maxTokens 随物化带入" node -e "
process.exit(JSON.parse(process.argv[1]).maxTokens===65536?0:1)" "$(ledger_model deepseek-v4-pro)"
check CASE4 "仓里显式 reasoning=false 不被缺省改回 true" node -e "
process.exit(JSON.parse(process.argv[1]).reasoning===false?0:1)" "$(ledger_model deepseek-v4-pro)"
check CASE4 "账本 2 条" test "$(ledger_len)" = 2

# ---------- CASE5 取消勾选 → 移除；定义留仓 ----------
mj settings.mc_toggle --arg ref=myds/deepseek-v4-pro --arg enable=false >/dev/null
sleep 1
check CASE5 "取消 → 账本移除" test "$(ledger_model deepseek-v4-pro)" = null
check CASE5 "定义仍在仓（off/on 往返不丢）" test "$(store_field deepseek-v4-pro maxTokens)" = 65536
check CASE5 "另一条不受影响" test "$(ledger_model deepseek-flash)" != null

# ---------- CASE6 账本行详情 = 索引模式（原行为不变） ----------
mj settings.mj_select --arg section=mj:p:myds >/dev/null
mj settings.mj_open_model --arg provider=myds --arg id=deepseek-flash >/dev/null
check CASE6 "账本行 → mj:m:myds:0" "$PUI" wait --path settings.section --eq "mj:m:myds:0" --timeout 5
check CASE6 "表单 id 正确" test "$(snap_field mj_id_value)" = '"deepseek-flash"'
mj settings.mj_model_close >/dev/null
check CASE6 "关闭退回商页" "$PUI" wait --path settings.section --eq "mj:p:myds" --timeout 5

# ---------- CASE7 仓草稿关闭后，+模型空白草稿不串值 ----------
mj settings.mj_open_model --arg provider=myds --arg id=deepseek-v4-pro >/dev/null
"$PUI" wait --path settings.section --eq "mj:m:myds:1" --timeout 5 >/dev/null 2>&1
mj settings.mj_model_close >/dev/null
"$PUI" wait --path settings.section --eq "mj:p:myds" --timeout 5 >/dev/null 2>&1
mj settings.mj_add_model >/dev/null
check CASE7 "+模型 = 追加槽 mj:m:myds:1" "$PUI" wait --path settings.section --eq "mj:m:myds:1" --timeout 5
check CASE7 "+模型 = 空白草稿（不预填刚关掉的仓模型）" test "$(snap_field mj_id_value)" = '""'
mj settings.mj_model_close >/dev/null

printf '%s\n' "${RESULTS[@]}"
echo "---- PASS=$PASS FAIL=$FAIL"
[ "$FAIL" = 0 ]
