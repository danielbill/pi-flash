#!/usr/bin/env bash
# 自定义 Provider 端到端用例（真实端点：DeepSeek /anthropic 兼容 + 智谱 coding v4）。
# 运行前提：target/debug 下有 pi-flash.exe + pif-ui.exe（cargo build -p app）。
# 隔离：PI_FLASH_DIR（PF 自有文件）+ PI_CODING_AGENT_DIR（models.json/auth.json）
#       都指进 tmp，不碰用户真实 ~/.pi。钥匙库是真实 Credential Manager——
#       统一用 pf-test-* 商名，末尾删除用例顺带清 keyring，另做 best-effort 兜底。
# 端点分工（2026-10-31 curl 实测）：
#   DeepSeek /anthropic：/v1/models 404 → 走 messages POST 兜底 = PassedEmpty，
#                        压 compat 探测 + P1-1（+模型）+ P2-3（改名后测速）
#   智谱 coding /v4：    /models 有真目录 → 压 openai 形状目录合并 +
#                        P1-2（刷新空 key）+ P2-4（改名内存目录同步）
# 已知失败（bug 未修，跑出来 KNOWN = 复现成功；PASS = bug 已修）
set -u
ROOT="D:/ai_workspace/pi-flash"
PUI="$ROOT/target/debug/pif-ui.exe"
EXE="$ROOT/target/debug/pi-flash.exe"
T="$ROOT/tmp/ui-test-mj"
INFO="$ROOT/test/provider/real-provider-info.txt"

DS_KEY=$(grep -oE 'sk-[0-9a-f]+' "$INFO" | head -1)
ZG_KEY=$(grep -oE '[0-9a-f]{32}\.[A-Za-z0-9]+' "$INFO" | head -1)
[ -z "$DS_KEY" ] && [ -z "$ZG_KEY" ] && { echo "FATAL: key 解析失败"; exit 1; }

PASS=0; FAIL=0; KNOWN=0; SKIP=0
declare -a RESULTS
check() { # check <case> <描述> <条件命令...>
  local case="$1" desc="$2"; shift 2
  if "$@" >/dev/null 2>&1; then
    RESULTS+=("PASS  $case  $desc"); PASS=$((PASS+1))
  else
    RESULTS+=("FAIL  $case  $desc"); FAIL=$((FAIL+1))
  fi
}
known_fail() { # 期望当前是 bug 态：断言失败 = 复现（KNOWN）；断言通过 = bug 已修（PASS）
  local case="$1" desc="$2"; shift 2
  if "$@" >/dev/null 2>&1; then
    RESULTS+=("PASS  $case  $desc（bug 已修复！）"); PASS=$((PASS+1))
  else
    RESULTS+=("KNOWN $case  $desc（复现已知 bug）"); KNOWN=$((KNOWN+1))
  fi
}
skip() { RESULTS+=("SKIP  $1  $2"); SKIP=$((SKIP+1)); }
jm_get() { node -e "const d=JSON.parse(require('fs').readFileSync(process.argv[1],'utf8'));try{const v=eval('d'+process.argv[2]);console.log(JSON.stringify(v===undefined?null:v))}catch(e){console.log('null')}" "$MODELS" "$1"; }
snap() { "$PUI" snapshot settings 2>/dev/null; }
mj() { "$PUI" exec "$@"; }
ds_models() { node -e "const d=JSON.parse(require('fs').readFileSync('$MODELS','utf8'));console.log(((((d.providers||{})['pf-test-ds']||{}).models)||[]).length)"; }
zg_models() { node -e "const d=JSON.parse(require('fs').readFileSync('$MODELS','utf8'));console.log(((((d.providers||{})['pf-test-zg']||{}).models)||[]).length)"; }

# ---------- 环境准备 ----------
rm -rf "$T"; mkdir -p "$T"
export PI_FLASH_DIR="$T"
export PI_CODING_AGENT_DIR="$T/pi-agent"
MODELS="$PI_CODING_AGENT_DIR/models.json"
AUTH="$PI_CODING_AGENT_DIR/auth.json"
"$PUI" clean >/dev/null 2>&1 || true
PI_FLASH_AUTOMATION=auto "$EXE" > "$T/app.log" 2>&1 &
APP_PID=$!
trap 'kill $APP_PID 2>/dev/null' EXIT
BOOTED=0
for i in $(seq 1 15); do
  if "$PUI" wait --path app.booted --truthy --timeout 10 >/dev/null 2>&1; then BOOTED=1; break; fi
  sleep 2
done
[ "$BOOTED" = 1 ] || { echo "FATAL: app 未启动"; tail -20 "$T/app.log"; exit 1; }

"$PUI" exec settings.open --arg tab=1
DS_BASE="https://api.deepseek.com/anthropic"

# ---------- CASE1 新增自定义 Provider（DeepSeek /anthropic）----------
mj settings.mj_new
"$PUI" wait --path settings.section --eq "mj:new" --timeout 10
mj settings.mj_form --arg name=pf-test-ds --arg base="$DS_BASE" --arg key="$DS_KEY"
mj settings.mj_save
check CASE1 "保存后 section 切到新商" "$PUI" wait --path settings.section --eq "mj:p:pf-test-ds" --timeout 10
check CASE1 "models.json 有条目" test "$(jm_get '.providers["pf-test-ds"]')" != null
check CASE1 "api=anthropic-messages（baseUrl 推断）" test "$(jm_get '.providers["pf-test-ds"].api')" = '"anthropic-messages"'
check CASE1 "models.json 不写 apiKey（key 走凭据路径）" test "$(jm_get '.providers["pf-test-ds"].apiKey')" = null
check CASE1 "auth.json 有 PF 标记条目" node -e "const d=JSON.parse(require('fs').readFileSync('$AUTH','utf8'));const c=d['pf-test-ds'];process.exit(c&&c.type==='api_key'&&c.key.startsWith('!')&&c.key.includes('pif-secret')?0:1)"

# ---------- CASE2 测速（compat：models 404 → messages 兜底 = PassedEmpty）----------
mj settings.mj_speedtest
check CASE2 "测速通过" "$PUI" wait --path settings.mj_speed --contains "Passed" --timeout 40
if [ "$(ds_models)" -gt 0 ]; then
  check CASE2 "目录合并进账本" test "$(ds_models)" -gt 0
  check CASE2 "内存目录出现该商" node -e "process.exit(JSON.parse(process.argv[1]).mj_catalog_providers.includes('pf-test-ds')?0:1)" "$(snap)"
else
  skip CASE2 "目录合并/内存目录（/anthropic 无 models 端点，PassedEmpty 路径符合预期）"
fi

# ---------- CASE2B 改名后测速（P2-3：不应按表单新名另建重复商）----------
mj settings.mj_form --arg name=pf-test-ds-renamed
mj settings.mj_speedtest
"$PUI" wait --path settings.mj_speed --contains "Passed" --timeout 40
known_fail CASE2B "无重复商（唯一 pf-test-ds）" node -e "
const d=JSON.parse(require('fs').readFileSync('$MODELS','utf8'));
const ks=Object.keys(d.providers||{}).filter(k=>k.startsWith('pf-test'));
process.exit(ks.length===1&&ks[0]==='pf-test-ds'?0:1)"
# 收拾 2B 残留（bug 态会多出 pf-test-ds-renamed；删除对不存在名也安全）
mj settings.mj_delete --arg name=pf-test-ds-renamed
mj settings.mj_select --arg section="mj:p:pf-test-ds"
"$PUI" wait --path settings.section --eq "mj:p:pf-test-ds" --timeout 10

# ---------- CASE3 +模型（P1-1）----------
mj settings.mj_add_model
N=$(ds_models)
check CASE3 "模型行 section 打开（ix=$N）" "$PUI" wait --path settings.section --eq "mj:m:pf-test-ds:$N" --timeout 10
mj settings.mj_model_form --arg id=pf-test-manual --arg ctx=8192 --arg max_tokens=4096 --arg reasoning=true
mj settings.mj_model_save
known_fail CASE3 "手加模型落盘（id=pf-test-manual）" node -e "
const d=JSON.parse(require('fs').readFileSync('$MODELS','utf8'));
process.exit(((((d.providers||{})['pf-test-ds']||{}).models)||[]).some(x=>x.id==='pf-test-manual')?0:1)"
mj settings.mj_model_close

# ---------- CASE4 自动填参（OpenRouter，在线；目标行自适应）----------
TARGET_IX=$(node -e "
const d=JSON.parse(require('fs').readFileSync('$MODELS','utf8'));
const ms=(((d.providers||{})['pf-test-ds']||{}).models)||[];
const i=ms.findIndex(m=>m.id==='pf-test-manual');
console.log(i>=0?i:(ms.length>0?0:-1))")
if [ "$TARGET_IX" -ge 0 ] 2>/dev/null; then
  mj settings.mj_select --arg section="mj:m:pf-test-ds:$TARGET_IX"
  mj settings.mj_autofill
  check CASE4 "填参完成（mj_fill=Done）" "$PUI" wait --path settings.mj_fill --eq "Done" --timeout 40
  check CASE4 "目录参数落盘（该行有 name/cost）" node -e "
const d=JSON.parse(require('fs').readFileSync('$MODELS','utf8'));
const m=((((d.providers||{})['pf-test-ds']||{}).models)||[])[process.argv[1]]||{};
process.exit(m.name&&m.cost?0:1)" "$TARGET_IX"
  mj settings.mj_model_close
else
  skip CASE4 "自动填参（无可用模型行：目录空且 CASE3 未落盘）"
fi

# ---------- CASE5 新增智谱商（openai 形状 /v4，有真目录）----------
mj settings.mj_new
mj settings.mj_form --arg name=pf-test-zg --arg base="https://open.bigmodel.cn/api/coding/paas/v4" --arg key="$ZG_KEY"
mj settings.mj_save
check CASE5 "保存成功" "$PUI" wait --path settings.section --eq "mj:p:pf-test-zg" --timeout 10
check CASE5 "api=openai-completions" test "$(jm_get '.providers["pf-test-zg"].api')" = '"openai-completions"'
mj settings.mj_speedtest
check CASE5 "测速通过（/v4 直接接 /models）" "$PUI" wait --path settings.mj_speed --contains "Passed" --timeout 40
check CASE5 "目录合并进账本（模型数>0）" node -e "process.exit(Number(process.argv[1])>0?0:1)" "$(zg_models)"
check CASE5 "内存目录出现该商" node -e "process.exit(JSON.parse(process.argv[1]).mj_catalog_providers.includes('pf-test-zg')?0:1)" "$(snap)"

# ---------- CASE6 刷新目录（P1-2：PF 管的商 key 不在 models.json）----------
mj settings.mj_select --arg section="mj:p:pf-test-zg"
sleep 1
mj settings.mj_fetch
sleep 12   # 真网络探测（20s 超时内）；成功=无错误条，失败=error 条
known_fail CASE6 "刷新无鉴权错误（error 条为空）" node -e "
process.exit(JSON.parse(process.argv[1]).error?1:0)" "$(snap)"
check CASE6 "刷新后目录仍可用（模型数>0）" node -e "process.exit(Number(process.argv[1])>0?0:1)" "$(zg_models)"

# ---------- CASE7 改名（P2-4：内存目录同步）----------
mj settings.mj_form --arg name=pf-test-zg-rn
mj settings.mj_save
# 偶发护栏：save 极偶尔不生效（疑似与前一网络 op 的异步回填竞态），重试一次
if ! "$PUI" wait --path settings.section --eq "mj:p:pf-test-zg-rn" --timeout 10 >/dev/null 2>&1; then
  mj settings.mj_save
fi
check CASE7 "section 切到新名" "$PUI" wait --path settings.section --eq "mj:p:pf-test-zg-rn" --timeout 10
check CASE7 "磁盘改名完成" node -e "
const d=JSON.parse(require('fs').readFileSync('$MODELS','utf8'));
process.exit((d.providers||{})['pf-test-zg-rn']&&!(d.providers||{})['pf-test-zg']?0:1)"
known_fail CASE7 "内存目录换名（旧名消失、新名出现）" node -e "
const s=JSON.parse(process.argv[1]).mj_catalog_providers;
process.exit(!s.includes('pf-test-zg')&&s.includes('pf-test-zg-rn')?0:1)" "$(snap)"
check CASE7 "凭据跟名搬家（auth.json 新名有标记）" node -e "
const d=JSON.parse(require('fs').readFileSync('$AUTH','utf8'));
process.exit(d['pf-test-zg-rn']&&d['pf-test-zg-rn'].key.startsWith('!')?0:1)"

# ---------- CASE8 删除 Provider（teardown：盘+auth.json+keyring 三清）----------
for P in $(node -e "const d=JSON.parse(require('fs').readFileSync('$MODELS','utf8'));console.log(Object.keys(d.providers||{}).filter(k=>k.startsWith('pf-test')).join(' '))"); do
  mj settings.mj_select --arg section="mj:p:$P"
  mj settings.mj_delete --arg name="$P"
done
sleep 1
check CASE8 "models.json 无残留" node -e "
const d=JSON.parse(require('fs').readFileSync('$MODELS','utf8'));
process.exit(Object.keys(d.providers||{}).some(k=>k.startsWith('pf-test'))?1:0)"
check CASE8 "auth.json 无残留" node -e "
const d=JSON.parse(require('fs').readFileSync('$AUTH','utf8'));
process.exit(Object.keys(d).some(k=>k.startsWith('pf-test'))?1:0)"
# keyring 兜底清（keyring 条目 PiFlash:PF_KEY_PF_TEST_*；删除用例本应清掉）
cmdkey /list 2>/dev/null | grep -io "PF_KEY_PF_TEST[A-Z0-9_]*" | sort -u | while read -r K; do
  cmdkey "/delete:PiFlash:$K" >/dev/null 2>&1 || true
done

# ---------- 收尾 ----------
"$PUI" exec app.quit >/dev/null 2>&1
kill $APP_PID 2>/dev/null
echo "==================== 结果 ===================="
printf '%s\n' "${RESULTS[@]}"
echo "=============================================="
echo "PASS=$PASS KNOWN(复现bug)=$KNOWN FAIL=$FAIL SKIP=$SKIP"
[ "$FAIL" -eq 0 ]
