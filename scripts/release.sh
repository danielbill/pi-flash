#!/usr/bin/env bash
# pi-flash 打包发布标准程序（Windows x64 绿色包）。
#
# 用法：
#   scripts/release.sh <版本号> <说明文件.md>
#   例：scripts/release.sh 0.2.0 tmp/release-notes-0.2.0.md
#
# 步骤：校验工作区 → 更新 crates/app 版本号 → release 构建 → 组装
# dist/pi-flash（exe + node.exe + vendor/pi + 启动说明）→ 包内烟测 →
# 更新 CHANGELOG.md → 产出 dist/pi-flash-<版本>-windows-x64.zip。
#
# 结束后自行提交版本号与 CHANGELOG，并打 tag v<版本号> 推送以触发
# macOS CI（.github/workflows/release-macos.yml）。

set -euo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)

VERSION=${1:-}
NOTES_FILE=${2:-}
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "用法: scripts/release.sh <x.y.z> <说明文件.md>"; exit 1; }
[[ -f "$NOTES_FILE" ]] || { echo "说明文件不存在: $NOTES_FILE"; exit 1; }
ZIP_NAME="pi-flash-${VERSION}-windows-x64.zip"

# --- 0. 前置校验 -----------------------------------------------------------
if [[ -n "$(git status --porcelain)" ]]; then
  echo "✗ 工作区有未提交改动，先提交再发布："
  git status --short
  exit 1
fi
[[ -f vendor/pi/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js ]] || {
  echo "✗ vendor/pi 未安装（cd vendor/pi && npm ci）"; exit 1;
}

echo "==> 1/7 更新版本号 -> $VERSION"
sed -i "s/^version = \".*\"/version = \"$VERSION\"/" crates/app/Cargo.toml
grep -q "version = \"$VERSION\"" crates/app/Cargo.toml || { echo "✗ 版本号替换失败"; exit 1; }

echo "==> 2/7 release 构建"
cargo build --release -p app

echo "==> 3/7 组装 dist/pi-flash"
rm -rf dist/pi-flash "dist/$ZIP_NAME"
mkdir -p dist/pi-flash/vendor
cp target/release/pi-flash.exe dist/pi-flash/
NODE_BIN=$(command -v node || true)
[[ -n "$NODE_BIN" ]] || { echo "✗ 找不到 node（PATH 或 PI_FLASH_NODE）"; exit 1; }
cp "$NODE_BIN" dist/pi-flash/node.exe
cp -r vendor/pi dist/pi-flash/vendor/

cat > dist/pi-flash/启动说明.txt <<EOF
pi-flash v${VERSION} — pi coding agent 的桌面壳（Windows x64）

启动：双击 pi-flash.exe（无需安装，node 已内置，vendor/pi 已内置）。

首次使用：
1. 配置模型 API Key：底部「模型」-> 选择 provider -> 填入 API Key 保存
   （密钥写入 ~/.pi/agent/auth.json，与 pi CLI 共用）
2. 顶部状态栏连接成功后即可对话；工具栏「生成标题」可让模型为会话起名

功能入口：
- 底部导航：模型 / 技能 / 插件
- 侧栏底部：文件浏览器（>_ 图标打开内置终端）
- 工具栏：完整历史 / 分支 / 系统提示词 / 工具
- 设置弹窗六个页签：模型 / 技能 / 插件 / 工具 / 子代理 / 通用（主题+语言）
- 本版本更新内容见压缩包内 CHANGELOG 片段或仓库 CHANGELOG.md

数据位置：~/.pi/agent/（sessions、settings.json、auth.json、models 缓存）
EOF
# 同时带上一份 CHANGELOG，方便离线看更新说明
cp CHANGELOG.md dist/pi-flash/CHANGELOG.md

echo "==> 4/7 包内烟测（独立目录启动）"
SMOKE_LOG="$ROOT/dist/smoke.log"
(cd / && "$ROOT/dist/pi-flash/pi-flash.exe" > "$SMOKE_LOG" 2>&1 &)
sleep 6
if ! tasklist //FI "IMAGENAME eq pi-flash.exe" 2>/dev/null | grep -qi pi-flash; then
  echo "✗ 打包后启动失败："; cat "$SMOKE_LOG"; exit 1
fi
taskkill //IM pi-flash.exe //F > /dev/null 2>&1 || true
sleep 1
if [[ -s "$SMOKE_LOG" ]]; then
  echo "✗ 启动有输出（视为异常）："; cat "$SMOKE_LOG"; exit 1
fi
rm -f "$SMOKE_LOG"
echo "    启动正常"

echo "==> 5/7 更新 CHANGELOG.md"
{
  echo "## [$VERSION] - $(date +%F)"
  echo
  cat "$NOTES_FILE"
  echo
  tail -n +2 CHANGELOG.md 2>/dev/null || true
} > CHANGELOG.md.new
mv CHANGELOG.md.new CHANGELOG.md

echo "==> 6/7 压缩 $ZIP_NAME"
powershell -NoProfile -Command "Compress-Archive -Path 'dist\pi-flash' -DestinationPath 'dist\\${ZIP_NAME}' -Force"

echo "==> 7/7 完成"
ls -lh "dist/$ZIP_NAME"
cat <<EOF

后续（手动执行）：
  git add crates/app/Cargo.toml Cargo.lock CHANGELOG.md
  git commit -m "release v${VERSION}"
  git tag v${VERSION}
  git push && git push --tags   # 推送 tag 触发 macOS CI 构建包
EOF
