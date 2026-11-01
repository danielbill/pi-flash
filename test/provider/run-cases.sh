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
    RESULTS+=("PASS  $case  $desc"); PASS=$((PASS+1))
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
# 三模块重构：/anthropic 无 models 端点 → messages 握手 400 = 协议可达
# → 明确 PassedEmpty（不再是含糊的 Passed 前缀匹配）
check CASE2 "测速通过（PassedEmpty：连通但无目录）" "$PUI" wait --path settings.mj_speed --contains "PassedEmpty" --timeout 60
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
check CASE2B "无重复商（唯一 pf-test-ds）" node -e "
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
check CASE3 "手加模型落盘（id=pf-test-manual）" node -e "
const d=JSON.parse(require('fs').readFileSync('$MODELS','utf8'));
process.exit(((((d.providers||{})['pf-test-ds']||{}).models)||[]).some(x=>x.id==='pf-test-manual')?0:1)"
mj settings.mj_model_close

# ---------- CASE4 自动填参（OpenRouter，在线）----------
# 自造一行真实 id（deepseek-chat 在 OpenRouter 有条目；pf-test-manual 查不到
# 是 NoMatch 常态，不是 bug）。查目录优先按 author 推断（baseUrl host → deepseek）
mj settings.mj_add_model
TARGET_IX=$(node -e "
const d=JSON.parse(require('fs').readFileSync('$MODELS','utf8'));
console.log(((((d.providers||{})['pf-test-ds']||{}).models)||[]).length)")
mj settings.mj_model_form --arg id=deepseek-chat
mj settings.mj_model_save
if node -e "
const d=JSON.parse(require('fs').readFileSync('$MODELS','utf8'));
process.exit(((((d.providers||{})['pf-test-ds']||{}).models)||[]).some(x=>x.id==='deepseek-chat')?0:1)" 2>/dev/null; then
  mj settings.mj_select --arg section="mj:m:pf-test-ds:$TARGET_IX"
  mj settings.mj_autofill
  # 断言落盘而非 mj_fill==Done：回填成功设 Done 后立即 mj_select 重选回显，
  # 状态被「换选中项即清」的设计瞬时清回 Idle，Done 不可观测
  FILL_OK=0
  for i in $(seq 1 30); do
    if node -e "
const d=JSON.parse(require('fs').readFileSync('$MODELS','utf8'));
const m=((((d.providers||{})['pf-test-ds']||{}).models)||[]).find(x=>x.id==='deepseek-chat')||{};
process.exit(m.name&&m.cost?0:1)" 2>/dev/null; then FILL_OK=1; break; fi
    sleep 2
  done
  check CASE4 "目录参数落盘（deepseek-chat 行有 name/cost）" test "$FILL_OK" = 1
  mj settings.mj_model_close
else
  skip CASE4 "自动填参（deepseek-chat 行未落盘：CASE3 追加分支失效）"
fi

# ---------- CASE4B 家族名 id → 版本名检索键（051：具体型号优先于别名聚合）----------
# 官方 /models 的 id 常是**家族名**（DeepSeek 的 deepseek-flash，以后每个 flash
# 版本都叫它），版本信息只在 name 里。自动填参必须用 name 派生的检索键拿到
# 具体型号参数，而不是「…-latest」别名聚合值（实测 ctx 1048576 /
# maxTokens 393216 vs 别名 943718）；落盘 id/name 一律保持官方列表值。
node -e "
const fs=require('fs');const p='$MODELS';
const d=JSON.parse(fs.readFileSync(p,'utf8'));
const e=d.providers['pf-test-ds'];
e.models=(e.models||[]).filter(m=>m.id!=='deepseek-flash');
e.models.push({id:'deepseek-flash',name:'DeepSeek-V4.1-Flash',reasoning:true,input:['text']});
fs.writeFileSync(p,JSON.stringify(d,null,2));" 2>/dev/null
FAMILY_IX=$(ds_models)
FAMILY_IX=$((FAMILY_IX-1))
mj settings.mj_select --arg section="mj:p:pf-test-ds"
"$PUI" wait --path settings.section --eq "mj:p:pf-test-ds" --timeout 10
mj settings.mj_select --arg section="mj:m:pf-test-ds:$FAMILY_IX"
if "$PUI" wait --path settings.mj_id_value --eq deepseek-flash --timeout 10 >/dev/null 2>&1; then
  mj settings.mj_autofill
  FILL_IX=0
  for i in $(seq 1 30); do
    if node -e "
const d=JSON.parse(require('fs').readFileSync('$MODELS','utf8'));
const m=(((d.providers||{})['pf-test-ds']||{}).models||[]).find(x=>x.id==='deepseek-flash')||{};
process.exit(m.maxTokens===393216?0:1)" 2>/dev/null; then FILL_IX=1; break; fi
    sleep 2
  done
  check CASE4B "填参取具体型号参数（maxTokens=393216 非别名 943718）" test "$FILL_IX" = 1
  check CASE4B "保存的 id 仍是官方家族名（不被查询键替换）" test "$(jm_get '.providers["pf-test-ds"].models.find(m=>m.id==="deepseek-flash").id')" = '"deepseek-flash"'
  check CASE4B "官方 name 不被泛化标签冲掉" test "$(jm_get '.providers["pf-test-ds"].models.find(m=>m.id==="deepseek-flash").name')" = '"DeepSeek-V4.1-Flash"'
  mj settings.mj_model_close
  # 收尾：等异步回填彻底静止（section 回到商详情、填参态 Idle），后续 CASE
  # 不受本段的网络尾巴影响
  "$PUI" wait --path settings.section --eq "mj:p:pf-test-ds" --timeout 10 >/dev/null 2>&1 || true
else
  skip CASE4B "家族名 id 填参（mj_id_value 未回显 deepseek-flash）"
fi

# ---------- CASE4C 列表行 → 模型详情（内置商 / 自定义商同一行为）----------
# 两个「可用模型」列表（内置详情 / 自定义详情）共用同一行实现：点行 = 模型
# 详情，关闭退回**来源 section**（内置模型属于 section = 商 id，不能把底图
# 换成不存在的自定义商草稿）。ops 与行点击是同一个 Chat::mj_open_model。
mj settings.mj_select --arg section=deepseek
"$PUI" wait --path settings.section --eq deepseek --timeout 10 >/dev/null 2>&1 || true
mj settings.mj_open_model --arg provider=deepseek --arg id=deepseek-flash
check CASE4C "内置商模型行可点开详情（只读覆盖视图）" "$PUI" wait --path settings.section --eq "mj:o:deepseek:deepseek-flash" --timeout 10
# 参数回显依赖内存目录已装该商（真实链路里行就来自目录）；沙箱 PI_CODING_AGENT_DIR
# 隔离、目录冷启动，此处只断言必然成立的部分：id 回显（参数空了不算 bug）
check CASE4C "只读视图回显该模型 id" node -e "
const j=JSON.parse(process.argv[1]);
process.exit(j.mj_id_value==='deepseek-flash'?0:1)" "$(snap)"
mj settings.mj_model_close
check CASE4C "关闭退回来源 section（内置商 → 商 id，不是 mj:p:）" "$PUI" wait --path settings.section --eq deepseek --timeout 10
mj settings.mj_select --arg section="mj:p:pf-test-ds"
"$PUI" wait --path settings.section --eq "mj:p:pf-test-ds" --timeout 10 >/dev/null 2>&1 || true
mj settings.mj_open_model --arg provider=pf-test-ds --arg id=pf-test-manual
check CASE4C "自定义商模型行可点开详情（账本条目）" node -e "
const j=JSON.parse(process.argv[1]);
process.exit(j.section.startsWith('mj:m:pf-test-ds:')&&j.mj_id_value==='pf-test-manual'?0:1)" "$(snap)"
mj settings.mj_model_close
check CASE4C "关闭退回自定义商详情" "$PUI" wait --path settings.section --eq "mj:p:pf-test-ds" --timeout 10

# ---------- CASE4D 点模型行**只认 id**，名称不得参与匹配 ----------
# 教训（2026-11-01 用户实测）：账本里 id=glm-5.3-flash-free、name=GLM-4.6V 的
# 行，曾让「点 GLM-4.6V」弹出那个莫名其妙的 free 模型。名称是显示字符串，
# 绝不能当匹配维度；命中不唯一也一律不猜（退回只读的 pi 定义视图）。
node -e "
const fs=require('fs');const p='$MODELS';
const d=JSON.parse(fs.readFileSync(p,'utf8'));
d.providers['pf-test-mism']={name:'pf-test-mism',baseUrl:'https://api.deepseek.com',api:'openai-completions',
  models:[{id:'glm-5.3-flash-free',name:'GLM-4.6V',contextWindow:1000000,reasoning:true,input:['text']}]};
fs.writeFileSync(p,JSON.stringify(d,null,2));" 2>/dev/null
mj settings.mj_select --arg section="mj:p:pf-test-mism"
"$PUI" wait --path settings.section --eq "mj:p:pf-test-mism" --timeout 10 >/dev/null 2>&1 || true
mj settings.mj_open_model --arg provider=pf-test-mism --arg id=glm-4.6v
check CASE4D "名称再像也不串行（落只读覆盖视图，不碰那条 free 账本行）" node -e "
const j=JSON.parse(process.argv[1]);
process.exit(j.section==='mj:o:pf-test-mism:glm-4.6v'&&j.mj_id_value==='glm-4.6v'?0:1)" "$(snap)"
mj settings.mj_model_close
node -e "
const fs=require('fs');const p='$MODELS';
const d=JSON.parse(fs.readFileSync(p,'utf8'));delete d.providers['pf-test-mism'];
fs.writeFileSync(p,JSON.stringify(d,null,2));" 2>/dev/null

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
check CASE6 "刷新无鉴权错误（error 条为空）" node -e "
process.exit(JSON.parse(process.argv[1]).error?1:0)" "$(snap)"
check CASE6 "刷新后目录仍可用（模型数>0）" node -e "process.exit(Number(process.argv[1])>0?0:1)" "$(zg_models)"

# ---------- CASE7 改名（P2-4：内存目录同步）----------
# 前置等待：上一段刷新目录是异步写盘（PF 现读现写），刚写完就改名会撞在
# 「表单被重建 → save 拿到旧名」的窗口上（本套件原有的偶发护栏即此原因）。
# 改为确定性重试：每次重填 name 后再存，最多三次。
SETTLED=0
for i in $(seq 1 10); do
  if node -e "
const j=JSON.parse(process.argv[1]);
process.exit(j.mj_fill==='Idle'&&!String(j.mj_speed||'').includes('Running')&&!j.error?0:1)" "$(snap)" 2>/dev/null; then SETTLED=1; break; fi
  sleep 2
done
RENAMED=0
for attempt in 1 2 3; do
  mj settings.mj_form --arg name=pf-test-zg-rn
  mj settings.mj_save
  if "$PUI" wait --path settings.section --eq "mj:p:pf-test-zg-rn" --timeout 10 >/dev/null 2>&1; then
    RENAMED=1; break
  fi
  sleep 2
done
check CASE7 "section 切到新名" test "$RENAMED" = 1
check CASE7 "磁盘改名完成" node -e "
const d=JSON.parse(require('fs').readFileSync('$MODELS','utf8'));
process.exit((d.providers||{})['pf-test-zg-rn']&&!(d.providers||{})['pf-test-zg']?0:1)"
check CASE7 "内存目录换名（旧名消失、新名出现）" node -e "
const s=JSON.parse(process.argv[1]).mj_catalog_providers;
process.exit(!s.includes('pf-test-zg')&&s.includes('pf-test-zg-rn')?0:1)" "$(snap)"
check CASE7 "凭据跟名搬家（auth.json 新名有标记）" node -e "
const d=JSON.parse(require('fs').readFileSync('$AUTH','utf8'));
process.exit(d['pf-test-zg-rn']&&d['pf-test-zg-rn'].key.startsWith('!')?0:1)"

# ---------- CASE9 手写条目落盘分流（2026-10-31 定稿：auth.json 无 PF 标记 =
# 非托管。UI 照常编辑；落盘只写表单拥有的键——显式 api 不被推断覆盖，
# 明文 apiKey 不转存凭据库、不写标记。判据 = pif-secret 命令引用形态）----------
node -e "
const fs=require('fs');
const d=JSON.parse(fs.readFileSync('$MODELS','utf8'));
d.providers['hand-made-ds']={name:'hand-made-ds',baseUrl:'https://api.deepseek.com',api:'openai-responses',apiKey:'$DS_KEY'};
fs.writeFileSync('$MODELS',JSON.stringify(d,null,2));"
mj settings.mj_select --arg section="mj:p:hand-made-ds"
"$PUI" wait --path settings.section --eq "mj:p:hand-made-ds" --timeout 10
mj settings.mj_form --arg base="https://api.deepseek.com/v1"
mj settings.mj_save
check CASE9 "表单改动落盘（baseUrl 已更新）" test "$(jm_get '.providers["hand-made-ds"].baseUrl')" = '"https://api.deepseek.com/v1"'
check CASE9 "显式 api 不被推断覆盖（仍 openai-responses）" test "$(jm_get '.providers["hand-made-ds"].api')" = '"openai-responses"'
check CASE9 "明文 apiKey 原样（不转存不删除）" test "$(jm_get '.providers["hand-made-ds"].apiKey')" = "\"$DS_KEY\""
check CASE9 "未写凭据标记（auth.json 无该商）" node -e "
const d=JSON.parse(require('fs').readFileSync('$AUTH','utf8'));
process.exit(d['hand-made-ds']?1:0)"
check CASE9 "钥匙库无收编（无 PF_KEY_HAND_MADE 条目）" bash -c '! cmdkey /list 2>/dev/null | grep -q PF_KEY_HAND_MADE'
mj settings.mj_speedtest
# 偶发护栏：套件前段已多次打 DeepSeek，限流/瞬断重试一次
if ! "$PUI" wait --path settings.mj_speed --contains "Passed" --timeout 40 >/dev/null 2>&1; then
  sleep 5
  mj settings.mj_speedtest
fi
check CASE9 "手写商测速可用（探测不写凭据）" "$PUI" wait --path settings.mj_speed --contains "Passed" --timeout 40
check CASE9 "测速后 apiKey 仍原样明文" test "$(jm_get '.providers["hand-made-ds"].apiKey')" = "\"$DS_KEY\""
check CASE9 "测速后显式 api 仍保留" test "$(jm_get '.providers["hand-made-ds"].api')" = '"openai-responses"'
mj settings.mj_select --arg section="mj:p:pf-test-zg-rn"
# 清理：测试自己摘手写条目
node -e "
const fs=require('fs');
const d=JSON.parse(fs.readFileSync('$MODELS','utf8'));
delete d.providers['hand-made-ds'];
fs.writeFileSync('$MODELS',JSON.stringify(d,null,2));"

# ---------- CASE10 内置商端点管道（2026-11-01：注册表 dump 携带 baseUrl，
# 内置商「获取模型」端点 SDK 派生——27 家白名单缺口根治的管道级证明）----------
REG="$T/registry-cache.json"
REG_OK=0
for i in $(seq 1 30); do
  if node -e "
const d=JSON.parse(require('fs').readFileSync(process.argv[1],'utf8'));
const ps=d.providers||[];
const withUrl=ps.filter(p=>p.baseUrl);
const zai=ps.find(p=>p.id==='zai-coding-cn');
process.exit(ps.length>=40&&withUrl.length===ps.length&&zai&&zai.baseUrl==='https://open.bigmodel.cn/api/coding/paas/v4'?0:1)" "$REG" 2>/dev/null; then REG_OK=1; break; fi
  sleep 2
done
check CASE10 "注册表缓存就绪且全员带 baseUrl（含 zai-coding-cn 实值）" test "$REG_OK" = 1

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
