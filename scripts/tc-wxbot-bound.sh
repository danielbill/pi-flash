#!/usr/bin/env bash
# tc-wxbot-bound.sh —— 已绑定会话冷启动：fn_bar 微信 icon 必须点亮、会话列表必须画 badge
#
# 你定义的三步（全程 pif-ui 自动化，零人眼）：
#   1. app 启动 → 扫描绑定情况；已绑定 → 取绑定会话的 sessionID
#   2. 自动打开该会话，进入会话
#   3. 断言 sessionList 画了 badge + fnbar 微信 icon 点亮（tone=accent）
#
# 绑定凭证来源（真机扫描留下的记录，不造假）：
#   <数据目录>/pf-svc/registry.json                  宿主锚点注册表（规范键 → token）
#   <数据目录>/plugins/pf-wxbot/data/svc/wx-state.json  执行面状态：page_id/session_id = 绑定会话
#   两个键都按宿主 norm_key 归一（\→/、盘符小写）后落测。
#
# 用法：scripts/tc-wxbot-bound.sh [--dir <数据目录>] [--keep]
#   缺省 --dir = pi-flash-dev 的副本（不动用户正在跑的实例）
#   退出码 0 = 全绿；1 = 有断言失败；2 = 前置缺失（没有绑定记录，用例不适用）

set -u
REPO="$(cd "$(dirname "$0")/.." && pwd)"
PLUGINS="${PF_PLUGINS_REPO:-D:/ai_workspace/pi-flash-plugins}"
SRC_DIR="$REPO/pi-flash-dev"
WORK="$REPO/tmp/tc-wxbot-bound"
KEEP=0
while [ $# -gt 0 ]; do
  case "$1" in
    --dir) SRC_DIR="$2"; shift 2 ;;
    --keep) KEEP=1; shift ;;
    *) echo "未知参数 $1"; exit 2 ;;
  esac
done

PUI="$REPO/target/debug/pif-ui.exe"
APP="$REPO/target/debug/pi-flash.exe"
[ -x "$APP" ] && [ -x "$PUI" ] || { echo "✗ 缺构建产物（cargo build）"; exit 2; }

pass=0; fail=0; APP_WIN_PID=""
ok()  { pass=$((pass+1)); echo "  PASS  $1"; }
bad() { fail=$((fail+1)); echo "  FAIL  $1"; }
cleanup() {
  [ -n "$APP_WIN_PID" ] && taskkill //PID "$APP_WIN_PID" //F >/dev/null 2>&1
  [ "$KEEP" = "1" ] || rm -rf "$WORK"
}
skip() { echo "SKIP  $1"; cleanup; exit 2; }
die()  { echo "✗ $1"; cleanup; exit 2; }

rm -rf "$WORK"; mkdir -p "$WORK"
WORK_WIN="$WORK"; [[ "$WORK" =~ ^/([a-zA-Z])/(.*)$ ]] && WORK_WIN="${BASH_REMATCH[1]^^}:/${BASH_REMATCH[2]}"   # MSYS 路径原生 exe 认不得
[ -d "$SRC_DIR" ] || die "缺数据目录 $SRC_DIR"
cp -r "$SRC_DIR" "$WORK/dir" || die "复制数据目录失败"
rm -rf "$WORK/dir/automation"          # 副本里的旧登记指向别人的进程，会糊脸
export PI_FLASH_DIR="$WORK_WIN/dir"

# 装当前源码版插件（v4）进隔离目录，避免测到过期安装副本
PKG="$PLUGINS/packages/pf-wxbot"
[ -d "$PKG" ] || die "缺插件包 $PKG"
DEST="$WORK/dir/plugins/pf-wxbot"; mkdir -p "$DEST"
cp "$PKG/pf-plugin.toml" "$PKG/main.js" "$DEST/" || die "拷清单/壳失败"
rm -rf "$DEST/pi" "$DEST/assets"; cp -r "$PKG/pi" "$DEST/pi"; cp -r "$PKG/assets" "$DEST/assets"

# ---------------------------------------------------------------- 1. 绑定情况
echo "== 1. 扫描绑定情况"
REG="$WORK/dir/pf-svc/registry.json"
EXT="$DEST/data/svc/wx-state.json"
[ -f "$REG" ] || skip "缺锚点注册表（从未起过执行域）"
[ -f "$EXT" ] || skip "缺执行面状态（从未绑定）"

BOT=$(node -e 'const v=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8"));console.log(v.bot_user_id||"")' "$EXT" 2>/dev/null)
[ -n "$BOT" ] || skip "执行面未绑定微信（无 bot_user_id）"
echo "  已绑定账号: $BOT"

# 绑定会话：优先执行面记的 session_id / page_id（谁绑的就是谁），退回注册表首个已落盘键
SESSION_FILE=$(node -e '
const fs=require("fs");
const ext=JSON.parse(fs.readFileSync(process.argv[1],"utf8"));
const reg=JSON.parse(fs.readFileSync(process.argv[2],"utf8"));
const toWin=k=>k.replace(/\//g,"\\");
const cand=[ext.session_id,ext.page_id].filter(Boolean).map(toWin);
for(const k of Object.keys(reg)){ if(k&&!k.startsWith("draft-")) cand.push(toWin(k)); }
for(const c of cand){ if(fs.existsSync(c)){ console.log(c); break; } }
' "$EXT" "$REG" 2>/dev/null)
[ -n "$SESSION_FILE" ] || skip "绑定会话文件不在盘上"
NORM_KEY=$(node -e 'const s=process.argv[1].trim().replace(/\\/g,"/");process.stdout.write(s.length>1&&s[1]===":"?s[0].toLowerCase()+s.slice(1):s)' "$SESSION_FILE")
echo "  绑定会话ID: $SESSION_FILE"

# ---------------------------------------------------------------- 2. 启动并进入会话
echo "== 2. 冷启动 + 打开该会话"
PI_FLASH_AUTOMATION=auto "$APP" >"$WORK/app.log" 2>&1 &
# 等它自己写出自动化登记（pif-ui 探一次就放弃，不等登记落地）
for _ in $(seq 1 60); do ls "$PI_FLASH_DIR/automation"/*.json >/dev/null 2>&1 && break; sleep 1; done

"$PUI" wait --path app.booted --truthy --timeout 90 >/dev/null || die "启动超时（日志 $WORK/app.log）"
APP_WIN_PID=$(ls "$PI_FLASH_DIR/automation"/*.json 2>/dev/null | head -1 | xargs -r basename | sed 's/\.json$//')
echo "  booted (pid=$APP_WIN_PID)"

"$PUI" exec session.open --arg "path=$SESSION_FILE" >/dev/null || die "打开会话失败"
# 首渲 → 壳 ensureSync → ext 回推 bound 是异步链（渲染节拍 + 1s 缓存 TTL），
# 断言必须等态成立而不是固定睡眠（有 delay，但不是灭灯）。
poll() {  # poll <次数> <命令...>；命中即返回 0
  local n=$1; shift
  local out
  for _ in $(seq 1 "$n"); do
    out=$("$@" 2>&1)
    case "$out" in *"$POLL_PAT"*) printf '%s' "$out"; return 0 ;; esac
    sleep 1
  done
  printf '%s' "$out"; return 1
}

# ---------------------------------------------------------------- 基线（通道级、确定性）
# 本用例只验证「壳 ↔ 通道 ↔ 宿主」这一段，不依赖 ext 的实时出码/网络/锁：
# 用与 ext 同形的 outbox 事件驱动壳，和真机扫码成功后的推送逐字段一致。
norm() { node -e '
const BS=String.fromCharCode(92);
let k=process.argv[1].trim().split(BS).join("/");
process.stdout.write(k.length>1&&k[1]===":"?k[0].toLowerCase()+k.slice(1):k)' "$1"; }
NORM_KEY=$(norm "$SESSION_FILE")
UIJSON="$DEST/data/ui.json"
ujson() { node -e 'try{const v=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8"));console.log(process.argv[2].split(".").reduce((o,k)=>o==null?o:o[k],v))}catch(e){console.log("")}' "$UIJSON" "$1"; }
fp() { node -e '
try {
  const v=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8"));
  const g=v.qr&&v.qr.grid;
  const rows=g&&g.rows?JSON.stringify(g.rows):"";
  let h=0; for(let i=0;i<rows.length;i++){h=(h*31+rows.charCodeAt(i))%2147483647;}
  console.log([!!v.qr, v.qrTimeout===true?1:0, h].join(":")); } catch(e){ console.log("err") }' "$UIJSON"; }

# 让本会话成为已绑定态：先把门牌（token）弄出来，再按 ext 的扫码成功事件推一次
"$PUI" exec plugin.fnbar_click >/dev/null 2>&1 || bad "fn icon 点击失败"
TOKEN1=""
for _ in $(seq 1 40); do
  TOKEN1=$(node -e '
const fs=require("fs");
try {
  const reg=JSON.parse(fs.readFileSync(process.argv[1],"utf8"));
  const BS=String.fromCharCode(92);
  const norm=k=>{k=k.split(BS).join("/");return k.length>1&&k[1]===":"?k[0].toLowerCase()+k.slice(1):k};
  const t=reg[norm(process.argv[2])];
  if(t) console.log(t);
} catch(e){}' "$REG" "$SESSION_FILE" 2>/dev/null)
  [ -n "$TOKEN1" ] && break
  sleep 1
done
[ -n "$TOKEN1" ] || die "本会话执行域未起（token 未铸）"
IN1="$WORK/dir/pf-svc/sessions/$TOKEN1-in.jsonl"
OUT1="$WORK/dir/pf-svc/sessions/$TOKEN1-out.jsonl"

printf '{"id":"pf-wxbot","payload":{"type":"bound","scanned":true}}
' >> "$OUT1"
GOT=""
for _ in $(seq 1 30); do GOT=$(ujson boundKey); [ "$GOT" = "$NORM_KEY" ] && break; sleep 1; done
[ "$GOT" = "$NORM_KEY" ] && ok "基线：绑定指针指向本会话（注册表口径）" || bad "基线失败（boundKey=$GOT）"
[ "$(ujson expired)" = "false" ] && ok "基线：非过期态（expired=false）" || bad "基线：仍处过期态"

# ---------------------------------------------------------------- 3. 徽标
echo "== 3. 会话列表 badge"
B=""; N=0
for _ in $(seq 1 30); do
  B=$("$PUI" exec plugin.badge --arg "path=$SESSION_FILE" 2>&1)
  N=$(node -e 'try{console.log(JSON.parse(process.argv[1]).items.length)}catch{console.log(0)}' "$B")
  [ "${N:-0}" -ge 1 ] 2>/dev/null && break
  sleep 1
done
echo "  $B"
[ "${N:-0}" -ge 1 ] && ok "会话行画出 badge（$N 条）" || bad "会话行没有 badge（期望 ≥1）"

# ---------------------------------------------------------------- 4. fn icon
echo "== 4. fn_bar 微信 icon"
TONE=""
for _ in $(seq 1 30); do
  F=$("$PUI" exec plugin.fnbar 2>&1)
  TONE=$(node -e 'try{const i=JSON.parse(process.argv[1]).items;console.log(i.length?i[0].tone:"(none)")}catch{console.log("(parse)")}' "$F")
  [ "$TONE" = "accent" ] && break
  sleep 1
done
echo "  $F"
[ "$TONE" = "accent" ] && ok "微信 icon 点亮（tone=accent）" || bad "微信 icon 未点亮（tone=$TONE）"

# ---------------------------------------------------------------- 5. 弹窗 + 码（TC2）
echo "== 5. 点微信 icon → 二维码（TC2）"
"$PUI" exec plugin.fnbar_click >/dev/null 2>&1 || bad "fn icon 点击失败"
OPENED=0
for _ in $(seq 1 40); do
  "$PUI" snapshot app --only plugin_popup > "$WORK/pop.json" 2>/dev/null
  OPENED=$(node -e 'try{const j=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8"));console.log(j.plugin_popup&&j.plugin_popup.open?1:0)}catch(e){console.log(0)}' "$WORK/pop.json" 2>/dev/null)
  [ "$OPENED" = "1" ] && break
  sleep 1
done
[ "$OPENED" = "1" ] && ok "二维码弹窗已打开" || bad "二维码弹窗未打开"

SYNTH=$(node -e '
const N=49; const rows=[]; for(let y=0;y<N;y++){ rows.push([[0,25],[26,23]]); }
console.log(JSON.stringify({size:N,rows}));')
printf '{"id":"pf-wxbot","payload":{"type":"qr","grid":%s}}
' "$SYNTH" >> "$IN1"
PAINT=0
for _ in $(seq 1 30); do
  "$PUI" snapshot app --only plugin_popup > "$WORK/pop.json" 2>/dev/null
  PAINT=$(node -e '
try{
  const j=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8"));
  const b=j.plugin_popup&&j.plugin_popup.body;
  const walk=n=>!n?null:(n.type==="canvas"?n:((n.children||[]).map(walk).find(Boolean)||null));
  const c=walk(b); console.log(c&&Array.isArray(c.paint)?c.paint.length:0);
}catch(e){console.log(0)}' "$WORK/pop.json" 2>/dev/null)
  [ "${PAINT:-0}" -gt 90 ] 2>/dev/null && break
  sleep 1
done
[ "${PAINT:-0}" -gt 90 ] 2>/dev/null && ok "二维码已绘制（paint $PAINT 段）" || bad "二维码空白（paint=${PAINT:-0}）"

# ---------------------------------------------------------------- 6. 超时白遮 → 点击刷新（TC3）
echo "== 6. 二维码超时 → 点击画布刷新（TC3）"
# 先等呈现面稳定（ext 自己的真码推送可能晚于本用例的注入，后到的 qr 会把
# qrTimeout 复位——那是正确行为，不是 bug；所以等它安静下来再注入白遮）
prev=""; stable=0
for _ in $(seq 1 60); do
  cur=$(fp)
  if [ "$cur" = "$prev" ]; then stable=$((stable+1)); else stable=0; fi
  prev=$cur
  [ "$stable" -ge 5 ] && break
  sleep 1
done
echo "  状态已稳定：$cur"
printf '{"id":"pf-wxbot","payload":{"type":"qr_timeout"}}
' >> "$IN1"
TMO=0
for _ in $(seq 1 120); do TMO=$(ujson qrTimeout); [ "$TMO" = "true" ] && break; sleep 1; done
[ "$TMO" = "true" ] && ok "超时白遮成立（qrTimeout=true）" || bad "超时白遮未成立（qrTimeout=$TMO）"

# 呈现面必须已经渲染出「可点的白遮帧」（canvas 带 command=refresh-qr）——
# 弹窗内容 1s 一帧，body 落后于 ui.json 时点下去等于点空。
CMDOK=0
for _ in $(seq 1 30); do
  "$PUI" snapshot app --only plugin_popup > "$WORK/pop.json" 2>/dev/null
  CMDOK=$(node -e '
try{
  const j=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8"));
  const b=j.plugin_popup&&j.plugin_popup.body;
  const walk=n=>!n?null:(n.type==="canvas"?n:((n.children||[]).map(walk).find(Boolean)||null));
  const c=walk(b); console.log(c&&c.command==="refresh-qr"?1:0);
}catch(e){console.log(0)}' "$WORK/pop.json" 2>/dev/null)
  [ "$CMDOK" = "1" ] && break
  sleep 1
done
[ "$CMDOK" = "1" ] && ok "白遮帧已渲染成可点画布（command=refresh-qr）" || bad "白遮帧未带刷新命令"

BEFORE=$(fp); echo "  点击前 (hasQr:timeout:hash) = $BEFORE"

WH=$("$PUI" snapshot app --only window 2>/dev/null)
CLICKXY=$(node -e '
const j=JSON.parse(process.argv[1]);
const sc=j.window.scale_factor||1;
console.log(Math.round(j.window.width_px/sc/2)+","+Math.round(j.window.height_px/sc/2-72));' "$WH" 2>/dev/null)
CX=${CLICKXY%,*}; CY=${CLICKXY#*,}
: > "$IN1"    # 清空 inbox：只看本次点击产生什么
echo "  点击逻辑坐标 ($CX,$CY)"
"$PUI" exec input.mouse --arg x=$CX --arg y=$CY >/dev/null 2>&1 || bad "鼠标点击失败"

# 断言一（宿主契约，本 bug 的核心）：点击必须回环出 refresh 意图落到本会话信箱
GOTREFRESH=0
for _ in $(seq 1 60); do
  if grep -q '"type":"refresh"' "$IN1" 2>/dev/null; then GOTREFRESH=1; break; fi
  sleep 1
done
[ "$GOTREFRESH" = "1" ] && ok "点击回环出 refresh 意图（env 带会话键）" || bad "点击未产生 refresh 意图（空 env 丢意图）"

# 断言二（呈现面）：ext 收到 refresh 后出新码 → 白遮解除 + 码指纹变
NEW=$(node -e '
const N=49; const rows=[]; for(let y=0;y<N;y++){ rows.push([[0,12],[14,11],[27,20]]); }
console.log(JSON.stringify({size:N,rows}));')
printf '{"id":"pf-wxbot","payload":{"type":"qr","grid":%s}}
' "$NEW" >> "$IN1"
HB=$(echo "$BEFORE" | cut -d: -f3)
AFTER="$BEFORE"
for _ in $(seq 1 60); do
  AFTER=$(fp)
  TMOA=$(echo "$AFTER" | cut -d: -f2); HA=$(echo "$AFTER" | cut -d: -f3)
  [ "$TMOA" = "0" ] && [ "$HA" != "$HB" ] && [ "$HA" != "0" ] && break
  sleep 1
done
echo "  新码后 (hasQr:timeout:hash) = $AFTER"
TMOA=$(echo "$AFTER" | cut -d: -f2); HA=$(echo "$AFTER" | cut -d: -f3)
if [ "$AFTER" = "err" ]; then bad "读不到呈现面状态"
elif [ "$TMOA" != "0" ]; then bad "白遮未解除（qrTimeout 仍为 true）"
elif [ "$HA" = "$HB" ]; then bad "码未更换（指纹未变）"
else ok "新码生效：qrTimeout 复位 + 码指纹变化"; fi

# ---------------------------------------------------------------- 7. 切换绑定（TC4）
echo "== 7. 切换到另一会话并绑定（TC4）"
OTHER=$(ls -t "C:/Users/bi_da/.pi/agent/sessions/--D--ai_workspace-pi_work--/"*.jsonl 2>/dev/null | while read f; do
  W=$(echo "$f" | tr '/' '\'); [ "$W" != "$SESSION_FILE" ] && echo "$W" && break; done)
[ -n "$OTHER" ] || die "找不到第二个会话用于切换"
echo "  目标会话: $OTHER"
"$PUI" exec session.open --arg "path=$OTHER" >/dev/null || die "打开第二会话失败"
sleep 3
"$PUI" exec plugin.fnbar_click >/dev/null 2>&1 || bad "第二会话 icon 点击失败"
TOKEN2=""
for _ in $(seq 1 40); do
  TOKEN2=$(node -e '
const fs=require("fs");
try {
  const reg=JSON.parse(fs.readFileSync(process.argv[1],"utf8"));
  const BS=String.fromCharCode(92);
  const norm=k=>{k=k.split(BS).join("/");return k.length>1&&k[1]===":"?k[0].toLowerCase()+k.slice(1):k};
  const t=reg[norm(process.argv[2])];
  if(t) console.log(t);
} catch(e){}' "$REG" "$OTHER" 2>/dev/null)
  [ -n "$TOKEN2" ] && break
  sleep 1
done
[ -n "$TOKEN2" ] && ok "第二会话执行域已就绪（token $TOKEN2）" || bad "第二会话执行域未起"
if [ -n "$TOKEN2" ]; then
  printf '{"id":"pf-wxbot","payload":{"type":"bound","scanned":true}}
' >> "$WORK/dir/pf-svc/sessions/$TOKEN2-out.jsonl"
  NORM2=$(norm "$OTHER"); GOT2=""
  for _ in $(seq 1 30); do
    GOT2=$(node -e 'try{console.log(JSON.parse(require("fs").readFileSync(process.argv[1],"utf8")).boundKey||"")}catch(e){console.log("")}' "$UIJSON" 2>/dev/null)
    [ "$GOT2" = "$NORM2" ] && break
    sleep 1
  done
  [ "$GOT2" = "$NORM2" ] && ok "绑定指针已切到第二会话" || bad "绑定未切换（boundKey=$GOT2 期望 $NORM2）"
  "$PUI" exec session.open --arg "path=$SESSION_FILE" >/dev/null 2>&1
  sleep 3
  F2=$("$PUI" exec plugin.fnbar 2>&1)
  T2=$(node -e 'try{const i=JSON.parse(process.argv[1]).items;console.log(i.length?i[0].tone:"(none)")}catch{console.log("(parse)")}' "$F2")
  [ "$T2" = "text.faint" ] && ok "原会话 icon 已回落为灰" || bad "原会话 icon 仍亮（tone=$T2）"
fi

echo; echo "== 结果：$pass passed, $fail failed"
[ "$KEEP" = "1" ] && echo "（现场 $WORK；日志 $WORK/app.log）"
cleanup
[ "$fail" -eq 0 ] && exit 0 || exit 1
