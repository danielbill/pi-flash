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

# ---------------------------------------------------------------- 3. 徽标
echo "== 3. 会话列表 badge"
POLL_PAT='"icon"'
poll 30 "$PUI" exec plugin.badge --arg "path=$SESSION_FILE" >/dev/null   # 先等徽标态成立
B=$("$PUI" exec plugin.badge --arg "path=$SESSION_FILE" 2>&1); echo "  $B"
N=$(node -e 'try{console.log(JSON.parse(process.argv[1]).items.length)}catch{console.log(0)}' "$B")
[ "${N:-0}" -ge 1 ] && ok "会话行画出 badge（$N 条）" || bad "会话行没有 badge（期望 ≥1）"

# ---------------------------------------------------------------- 4. fn icon
echo "== 4. fn_bar 微信 icon"
# ip 会话文件落进运行时才算「进会话完成」，此前 fn_bar 的 env.session.key 是空串
for _ in $(seq 1 40); do
  SF=$("$PUI" snapshot session --only file 2>&1)
  case "$SF" in *".jsonl"*) break ;; esac
  sleep 1
done
POLL_PAT='accent'
poll 30 "$PUI" exec plugin.fnbar >/dev/null
F=$("$PUI" exec plugin.fnbar 2>&1); echo "  $F"
TONE=$(node -e 'try{const i=JSON.parse(process.argv[1]).items;console.log(i.length?i[0].tone:"(none)")}catch{console.log("(parse)")}' "$F")
[ "$TONE" = "accent" ] && ok "微信 icon 点亮（tone=accent）" || bad "微信 icon 未点亮（tone=$TONE，期望 accent）"

# 口径回归：壳归档的绑定指针必须是宿主注册表规范形
UI="$DEST/data/ui.json"
if [ -f "$UI" ]; then
  GOT=$(node -e 'try{console.log(JSON.parse(require("fs").readFileSync(process.argv[1],"utf8")).boundKey||"")}catch{console.log("")}' "$UI")
  [ "$GOT" = "$NORM_KEY" ] && ok "ui.boundKey = 注册表口径" || bad "ui.boundKey 口径不符：$GOT ≠ $NORM_KEY"
else
  bad "壳未写 ui.json（sync→bound 链未跑通）"
fi

# ---------------------------------------------------------------- 5. 点 icon → 二维码
echo "== 5. 点微信 icon → 二维码显示（TC2）"
"$PUI" exec plugin.fnbar_click >/dev/null 2>&1 || bad "fn icon 点击失败"
PAINT=0; OPENED=0
for _ in $(seq 1 40); do
  "$PUI" snapshot app --only plugin_popup > "$WORK/pop.json" 2>/dev/null
  PAINT=$(node -e '
try {
  const j=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8"));
  const b=j.plugin_popup && j.plugin_popup.body;
  if (j.plugin_popup && j.plugin_popup.open) console.log("OPEN");
  const walk=(n)=>!n?null:(n.type==="canvas"?n:((n.children||[]).map(walk).find(Boolean)||null));
  const c=walk(b);
  const k=require("fs").statSync(process.argv[1]).size;
  console.log(c && Array.isArray(c.paint) ? c.paint.length : 0);
} catch(e) { console.log(0) }' "$WORK/pop.json" 2>/dev/null | tail -1)
  OPENED=$(node -e '
try{const j=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8"));console.log(j.plugin_popup&&j.plugin_popup.open?1:0)}catch(e){console.log(0)}' "$WORK/pop.json" 2>/dev/null)
  [ "${PAINT:-0}" -gt 0 ] 2>/dev/null && break
  sleep 1
done
[ "${OPENED:-0}" = "1" ] && ok "二维码弹窗已打开" || bad "二维码弹窗未打开"
[ "${PAINT:-0}" -gt 0 ] 2>/dev/null && ok "二维码已绘制（paint 段数 $PAINT）" || bad "二维码空白（canvas paint=${PAINT:-0}）"

echo; echo "== 结果：$pass passed, $fail failed"
[ "$KEEP" = "1" ] && echo "（现场 $WORK；日志 $WORK/app.log）"
cleanup
[ "$fail" -eq 0 ] && exit 0 || exit 1
