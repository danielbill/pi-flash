#!/usr/bin/env bash
# L3（pif-ui 自动化）：旧 schema 2 包在新 PF 下的迁移体验——090-10
#
# 场景 = 本机 ~/.pi-flash 的真实现状复刻：台账登记 quick-panel 0.1.4（schema 2，
# 本版 PF 拒载），版本目录下有用户数据 0.1.4/data/pins.lst。
# 断言：
#   L1 data 从 <pkg>/0.1.4/data 迁到 <pkg>/data（内容原样，源目录搬空）
#   L2 揭幕后 toast 播报「不兼容 / 需更新」并带上插件名+版本
#   L3 市场状态 pf_market_stale 含该包，kind = schema:2（头部通知行 / 已安装页标签的数据源）
#
# 用法：bash test/090-10/插件更新/l3-stale-migrate.sh   （先 cargo build）
set -euo pipefail

ROOT=D:/ai_workspace/pi-flash
APP=$ROOT/target/debug/pi-flash.exe
PUI=$ROOT/target/debug/pif-ui.exe
FEED=$ROOT/test/090-10/dev-market/feed/pi-flash-plugins-quick-panel-0.1.4.tgz
T=$ROOT/tmp/ui-l3-stale-$$
PKG=$T/plugins/@pi-flash-plugins__quick-panel

# ---- 播种 ----
mkdir -p "$PKG/0.1.4/data"
tar --force-local -xzf "$FEED" -C "$PKG/0.1.4" --strip-components=1  # Git Bash：D: 会被当远程主机
printf 'D:\\x\\a.exe\nD:\\y\\b.md\n' > "$PKG/0.1.4/data/pins.lst"
cat > "$T/plugins.json" <<'EOF'
[{"name":"@pi-flash-plugins/quick-panel","version":"0.1.4","integrity":"","installed_at":"epoch:0","source":"market","disabled":false}]
EOF
grep -q '^schema = 2' "$PKG/0.1.4/pf-plugin.toml" || { echo "FAIL 夹具不是 schema 2"; exit 1; }
# mda8：再放一个非台账版本的残留目录（模拟上次更新被占用没删掉），启动期应被 prune
mkdir -p "$PKG/0.1.3"; printf 'schema = 2\n' > "$PKG/0.1.3/pf-plugin.toml"

# ---- 起隔离实例 ----
export PI_FLASH_DIR=$T
PI_FLASH_AUTOMATION=auto "$APP" > "$T/app.log" 2>&1 &
APP_PID=$!
cleanup() { "$PUI" exec app.quit >/dev/null 2>&1 || kill "$APP_PID" 2>/dev/null || true; }
trap cleanup EXIT

# pif-ui 实例发现读 <PI_FLASH_DIR>/automation/<pid>.json，app 写它要几百毫秒；
# 先等登记文件出现再发第一条命令（否则「没有找到自动化实例」直接退出）
for _ in $(seq 1 60); do ls "$T"/automation/*.json >/dev/null 2>&1 && break; sleep 0.5; done
"$PUI" wait --path app.booted --truthy --timeout 60 >/dev/null
echo "booted"

# ---- L2 toast（揭幕后播，2.5s 窗口）----
"$PUI" wait --surface app --path status_toast --contains "0.1.4" --timeout 10 \
  && echo "PASS L2 toast 含插件版本" || { echo "FAIL L2 toast"; exit 1; }

# ---- L3 市场失效状态 ----
"$PUI" wait --surface app --path "pf_market_stale[0].kind" --eq "schema:2" --timeout 10 >/dev/null \
  && echo "PASS L3 pf_market_stale kind=schema:2" || { echo "FAIL L3 stale"; exit 1; }
"$PUI" snapshot app --only pf_market_stale

# ---- L1 data 迁移 ----
test -f "$PKG/data/pins.lst" && echo "PASS L1 data 迁到 <pkg>/data" || { echo "FAIL L1 目标缺失"; exit 1; }
test ! -e "$PKG/0.1.4/data" && echo "PASS L1 版本目录 data 已搬空" || { echo "FAIL L1 源未搬"; exit 1; }
[ "$(grep -c '' "$PKG/data/pins.lst")" = "2" ] && echo "PASS L1 内容两行原样" || { echo "FAIL L1 内容"; exit 1; }

# ---- L5 启动期清非台账版本目录（mda8）----
test ! -e "$PKG/0.1.3" && echo "PASS L5 残留 0.1.3 已清" || { echo "FAIL L5 残留未清"; exit 1; }
test -d "$PKG/0.1.4" && echo "PASS L5 台账版本 0.1.4 保留" || { echo "FAIL L5 台账版本被误删"; exit 1; }

# ---- L4 市场页头部常驻通知行（像素级：开页 → 等视图 → 截帧，人眼/视觉复核）----
"$PUI" exec content.view --arg view=pf-market >/dev/null
"$PUI" wait --surface app --path content_view --eq "pf-market" --timeout 10 >/dev/null \
  && echo "PASS L4 市场页已打开" || { echo "FAIL L4 开页"; exit 1; }
sleep 1  # 等一帧把索引加载态/通知行画完
"$PUI" shot "$T/pf-market-stale.png"

grep -E "data 目录已迁出|跳过" "$T/app.log" || true
echo "ALL PASS ($T)"
