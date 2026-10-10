#!/usr/bin/env bash
# pi-flash 打包发布标准程序（Windows x64 绿色包）。
#
# 用法：
#   scripts/release.sh <版本号> <说明文件.md>
#   例：scripts/release.sh 0.2.0 tmp/release-notes-0.2.0.md
#
# 步骤：校验工作区 → 更新版本号（Cargo + npm 薄壳同同步）→ release 构建 →
# 组装 dist/pi-flash（exe + node.exe + vendor/pi 裁非宿主 esbuild + 启动说明）
# → 包内烟测 → 更新 CHANGELOG.md → 压缩 dist/pi-flash-<版本>-win32-x64.zip
# → 生成 SHA256SUMS 与 <资产>.sha256（npm 薄壳下载校验用，080-软件分发 §4）。
#
# 结束后执行尾部「后续（手动执行）」块：提交 → tag 推送（触发 macOS CI）
# → gh release create 挂资产 → npm publish 发布薄壳包。

set -euo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)

VERSION=${1:-}
NOTES_FILE=${2:-}
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "用法: scripts/release.sh <x.y.z> <说明文件.md>"; exit 1; }
[[ -f "$NOTES_FILE" ]] || { echo "说明文件不存在: $NOTES_FILE"; exit 1; }
ZIP_NAME="pi-flash-${VERSION}-win32-x64.zip"

# --- 0. 前置校验 -----------------------------------------------------------
# only tracked changes block a release; untracked scratch files (e.g.
# buglist.md) never enter the build
if [[ -n "$(git status --porcelain --untracked-files=no)" ]]; then
  echo "✗ 工作区有未提交改动，先提交再发布："
  git status --short
  exit 1
fi
[[ -f vendor/pi/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js ]] || {
  echo "✗ vendor/pi 未安装（cd vendor/pi && npm ci）"; exit 1;
}

echo "==> 1/8 更新版本号 -> $VERSION"
sed -i "s/^version = \".*\"/version = \"$VERSION\"/" crates/app/Cargo.toml
grep -q "version = \"$VERSION\"" crates/app/Cargo.toml || { echo "✗ 版本号替换失败"; exit 1; }
# npm 薄壳包与 Cargo 同版本（080 §4：版本三处单点改写，禁止手改两处）
sed -i "s/\"version\": \".*\"/\"version\": \"$VERSION\"/" npm/package.json
grep -q "\"version\": \"$VERSION\"" npm/package.json || { echo "✗ npm 薄壳版本号替换失败"; exit 1; }

echo "==> 2/8 release 构建"
cargo build --release -p app

echo "==> 3/8 组装 dist/pi-flash"
rm -rf dist/pi-flash "dist/$ZIP_NAME" "dist/${ZIP_NAME}.sha256" dist/SHA256SUMS
mkdir -p dist/pi-flash/vendor
cp target/release/pi-flash.exe dist/pi-flash/
NODE_BIN=$(command -v node || true)
[[ -n "$NODE_BIN" ]] || { echo "✗ 找不到 node（PATH 或 PI_FLASH_NODE）"; exit 1; }
cp "$NODE_BIN" dist/pi-flash/node.exe
cp -r vendor/pi dist/pi-flash/vendor/

# 裁掉非宿主平台 esbuild 二进制（vendor/pi 416MB 里 284MB 是 26 个平台副本，
# 每包只用 win32-x64 一个 → 下载体积 200MB+ → ≈120MB；见 080 §1.2）
ESBUILD_DIR="dist/pi-flash/vendor/pi/node_modules/@earendil-works/pi-coding-agent/node_modules/@esbuild"
[[ -d "$ESBUILD_DIR/win32-x64" ]] || { echo "✗ 找不到 @esbuild/win32-x64"; exit 1; }
for d in "$ESBUILD_DIR"/*; do
  [[ "$(basename "$d")" == "win32-x64" ]] || rm -rf "$d"
done
echo "    esbuild 裁剪完成（保留 win32-x64）"

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
- 设置弹窗页签：界面 / 模型 / 技能 / 扩展 / MCP / 其他 / 远程控制
- 本版本更新内容见压缩包内 CHANGELOG 片段或仓库 docs/CHANGELOG.md

数据位置：pi 数据 ~/.pi/agent/（sessions、settings.json、auth.json、models 缓存）；
         pi-flash 自己的配置 ~/.pi-flash/（workspace / app-settings / session-index / recents / catalog-cache）
EOF
# 同时带上一份 CHANGELOG，方便离线看说明
cp docs/CHANGELOG.md dist/pi-flash/CHANGELOG.md

echo "==> 4/8 包内烟测（独立目录启动）"
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

echo "==> 5/8 更新 CHANGELOG.md"
{
  echo "## [$VERSION] - $(date +%F)"
  echo
  cat "$NOTES_FILE"
  echo
  tail -n +2 docs/CHANGELOG.md 2>/dev/null || true
} > docs/CHANGELOG.md.new
mv docs/CHANGELOG.md.new docs/CHANGELOG.md

echo "==> 6/8 压缩 $ZIP_NAME"
powershell -NoProfile -Command "Compress-Archive -Path 'dist\pi-flash' -DestinationPath 'dist\\${ZIP_NAME}' -Force"

echo "==> 7/8 生成校验和（SHA256SUMS + 资产 sidecar，供 npm 薄壳下载校验）"
(cd dist && sha256sum "$ZIP_NAME" | tee "${ZIP_NAME}.sha256" > SHA256SUMS)

echo "==> 8/8 完成"
ls -lh "dist/$ZIP_NAME" "dist/${ZIP_NAME}.sha256" dist/SHA256SUMS
cat <<EOF

后续（按序手动执行；gh release 紧跟 tag 推送、别等 CI——CI 遇到无 release 会自建）：
  git add crates/app/Cargo.toml npm/package.json Cargo.lock docs/CHANGELOG.md
  git commit -m "release v${VERSION}"
  git tag v${VERSION}
  git push && git push --tags   # 推送 tag 触发 macOS CI（darwin-arm64 资产 + .sha256 自动挂载）
  gh release create "v${VERSION}" --verify-tag --title "v${VERSION}" \\
    --notes-file "${NOTES_FILE}" \\
    "dist/${ZIP_NAME}" "dist/${ZIP_NAME}.sha256" "dist/SHA256SUMS"
  cd npm && npm publish         # 仅首发/回退需要（浏览器 2FA 一次性）；配好 trusted publisher 后由 publish-npm.yml 自动发，无需再跑
EOF
