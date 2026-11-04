#!/usr/bin/env bash
# 042-2 模型价格管理：价格编辑器（手填显式覆盖 / 空沿用 / 0 显示为空）+
# priceSource 记账在仓、models.json 投影剥除 + 离线填参安全失败。
# 离线、确定性：预置 models.json / custom-provider.json，不联网（填参走
# 降级链全 miss → Err 态，正合离线预期）。运行前提：target/debug 下有
# pi-flash.exe + pif-ui.exe。隔离：PI_FLASH_DIR + PI_CODING_AGENT_DIR 指 tmp。
#
# 背景（2026-11-04）：自定义模型价格恒 {0,0,0,0} → ctx 面板累计费用恒 $0.0000；
# 价格源定稿 models.dev 仓库单文件 TOML（见 docs/模块设计/042-2）。
set -u
ROOT="D:/ai_workspace/pi-flash"
PUI="$ROOT/target/debug/pif-ui.exe"
EXE="$ROOT/target/debug/pi-flash.exe"
T="$ROOT/tmp/ui-test-mj-price"

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
snap_field() { node -e "console.log(JSON.stringify(JSON.parse(process.argv[1])[process.argv[2]]))" "$(snap)" "$1"; }
ledger_cost() { node -e "
const d=JSON.parse(require('fs').readFileSync(process.argv[1],'utf8'));
const m=((((d.providers||{}).myds||{}).models)||[]).find(x=>x.id==='deepseek-flash');
console.log(JSON.stringify(m===undefined?null:m.cost))" "$MODELS"; }
ledger_has_source() { node -e "
const d=JSON.parse(require('fs').readFileSync(process.argv[1],'utf8'));
const m=((((d.providers||{}).myds||{}).models)||[]).find(x=>x.id==='deepseek-flash');
console.log(m&&m.priceSource!==undefined)" "$MODELS"; }
store_field() { node -e "
const d=JSON.parse(require('fs').readFileSync(process.argv[1],'utf8'));
const m=(((d.myds||{}).models)||{})['deepseek-flash']||{};
const v=m[process.argv[2]];
console.log(JSON.stringify(v===undefined?null:v))" "$STORE" "$1"; }

# ---------- 环境准备 ----------
rm -rf "$T"; mkdir -p "$T/pi-agent"
export PI_FLASH_DIR="$T"
export PI_CODING_AGENT_DIR="$T/pi-agent"
MODELS="$PI_CODING_AGENT_DIR/models.json"
STORE="$T/custom-provider.json"
SETTINGS="$PI_CODING_AGENT_DIR/settings.json"
"$PUI" clean >/dev/null 2>&1 || true

# 预置：骨架商 + 仓里一条 cost 全 0 的模型（042-2 典型存量）+ 显式勾选
cat > "$MODELS" <<'EOF'
{"providers":{"myds":{"api":"openai-completions","name":"myds","baseUrl":"https://api.deepseek.com"}}}
EOF
cat > "$STORE" <<'EOF'
{"myds":{"fetchedAt":1,"models":{
  "deepseek-flash":{"id":"deepseek-flash","name":"DeepSeek-V4.1-Flash","contextWindow":1048576,
   "cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0}}}}}
EOF
cat > "$SETTINGS" <<'EOF'
{"enabledModels":["myds/deepseek-flash"]}
EOF

PI_FLASH_AUTOMATION=auto "$EXE" > "$T/app.log" 2>&1 &
APP_PID=$!
cleanup() { "$PUI" exec app.quit >/dev/null 2>&1 || kill "$APP_PID" >/dev/null 2>&1 || true; }
trap cleanup EXIT
BOOTED=0
for i in $(seq 1 15); do
  if "$PUI" wait --path app.booted --truthy --timeout 10 >/dev/null 2>&1; then BOOTED=1; break; fi
  sleep 2
done
[ "$BOOTED" = 1 ] || { echo "FATAL: app 未启动"; tail -20 "$T/app.log"; exit 1; }
sleep 1

# ---------- CASE1 投影剥除：仓定义带 cost，物化进 models.json 后不夹带 PF 私有键 ----------
check CASE1 "勾选物化：deepseek-flash 进 models.json" test "$(ledger_cost)" != null
check CASE1 "models.json 无 priceSource（PF 私有键被 normalize 剥除）" test "$(ledger_has_source)" = false

# ---------- CASE2 编辑器回显：0 显示为空（0 = 没定价，别让缺省冒充真价） ----------
mj settings.open --arg tab=1 >/dev/null
mj settings.mj_select --arg section=mj:p:myds >/dev/null
mj settings.mj_open_model --arg provider=myds --arg id=deepseek-flash >/dev/null
"$PUI" wait --path settings.section --eq "mj:m:myds:0" --timeout 5 >/dev/null 2>&1
check CASE2 "cost input 框显示空（存量 0 → 空）" test "$(snap_field mj_cost_in_value)" = '""'
check CASE2 "cost cacheRead 框显示空" test "$(snap_field mj_cost_cr_value)" = '""'

# ---------- CASE3 手填价格：显式覆盖 + priceSource=manual 落仓、账本同步且剥私有键 ----------
mj settings.mj_model_form --arg cost_in='"0.15"' --arg cost_out='"0.6"' >/dev/null
mj settings.mj_model_save >/dev/null
sleep 1
check CASE3 "仓 cost.input=0.15" test "$(store_field cost)" != null
node -e "
const c=JSON.parse(process.argv[1]);
process.exit(c.input===0.15&&c.output===0.6?0:1)" "$(store_field cost)" \
  && RESULTS+=("PASS  CASE3  仓 cost 四键按手填覆盖") PASS=$((PASS+1)) \
  || { RESULTS+=("FAIL  CASE3  仓 cost 四键按手填覆盖"); FAIL=$((FAIL+1)); }
check CASE3 "仓 priceSource=manual" test "$(store_field priceSource)" = '"manual"'
check CASE3 "已勾选 → models.json cost 同步手填值" node -e "
const c=JSON.parse(process.argv[1]);
process.exit(c.input===0.15&&c.output===0.6?0:1)" "$(ledger_cost)"
check CASE3 "models.json 仍无 priceSource" test "$(ledger_has_source)" = false

# ---------- CASE4 空框 = 沿用：只改一个键不冲掉其它 ----------
mj settings.mj_open_model --arg provider=myds --arg id=deepseek-flash >/dev/null
"$PUI" wait --path settings.section --eq "mj:m:myds:0" --timeout 5 >/dev/null 2>&1
check CASE4 "重开回填 cost.input=0.15（非零不显示为空）" test "$(snap_field mj_cost_in_value)" = '"0.15"'
check CASE4 "重开回填 cost.cacheWrite 空（0 → 空）" test "$(snap_field mj_cost_cw_value)" = '""'
mj settings.mj_model_form --arg cost_cw='"0.02"' >/dev/null
mj settings.mj_model_save >/dev/null
sleep 1
check CASE4 "只写 cacheWrite：input/output 保留" node -e "
const c=JSON.parse(process.argv[1]);
process.exit(c.input===0.15&&c.output===0.6&&c.cacheWrite===0.02?0:1)" "$(ledger_cost)"

# ---------- CASE5 非数字拒落盘（cost_all_required 文案） ----------
mj settings.mj_open_model --arg provider=myds --arg id=deepseek-flash >/dev/null
"$PUI" wait --path settings.section --eq "mj:m:myds:0" --timeout 5 >/dev/null 2>&1
mj settings.mj_model_form --arg cost_in='"abc"' >/dev/null
mj settings.mj_model_save >/dev/null
sleep 1
check CASE5 "非数字 → 报错不关弹窗（仍在编辑器）" "$PUI" wait --path settings.section --eq "mj:m:myds:0" --timeout 5
check CASE5 "报错不落盘：input 仍 0.15" node -e "
const c=JSON.parse(process.argv[1]);
process.exit(c.input===0.15?0:1)" "$(ledger_cost)"

# ---------- CASE6 自动填参：手填价（manual）是最高优先级，目录价不冲 ----------
# （有网时 jsdelivr 通道可能真拉到 models.dev 价 → fill Done；断网时全 miss →
# Err。两态都合法，断言只认「跑完了」+ 手填价分毫不动）
mj settings.mj_open_model --arg provider=myds --arg id=deepseek-flash >/dev/null
"$PUI" wait --path settings.section --eq "mj:m:myds:0" --timeout 5 >/dev/null 2>&1
mj settings.mj_autofill >/dev/null
FILLED=0
for i in $(seq 1 15); do
  s=$(snap_field mj_fill)
  if [ "$s" = '"Done"' ] || [ "$s" = '"Err"' ]; then FILLED=1; break; fi
  sleep 2
done
check CASE6 "填参已出结果（Done/Err，非 Running/Idle）" test "$FILLED" = 1
check CASE6 "手填价不被目录价冲掉" node -e "
const c=JSON.parse(process.argv[1]);
process.exit(c.input===0.15&&c.output===0.6&&c.cacheWrite===0.02?0:1)" "$(ledger_cost)"
check CASE6 "仓 priceSource 仍 manual" test "$(store_field priceSource)" = '"manual"'

printf '%s\n' "${RESULTS[@]}"
echo "---- PASS=$PASS FAIL=$FAIL"
[ "$FAIL" = 0 ]
