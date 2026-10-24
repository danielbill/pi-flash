#!/usr/bin/env bash
# pi-flash 打包发布标准程序（Windows x64 绿色包）。
#
# 用法：
#   scripts/release.sh <版本号> <说明文件.md>
#   例：scripts/release.sh 0.2.0 tmp/release-notes-0.2.0.md
#
# 步骤：校验工作区 → 更新版本号（Cargo + npm 薄壳同同步）→ release 构建 →
# 组装 dist/pi-flash（exe + node.exe + vendor/pi 裁非宿主 esbuild + 启动说明）
# → 包内烟测 → 更新 CHANGELOG.md / CHANGELOG_EN.md → 压缩 dist/pi-flash-<版本>-win32-x64.zip
# → 生成 SHA256SUMS 与 <资产>.sha256（npm 薄壳下载校验用，080-软件分发 §4）。
#
# 结束后跑 scripts/ship.sh <版本> <说明文件> 收尾（发版提交 → tag 推送触发
# publish-npm.yml 自动发薄壳 → gh release create 挂资产）。
# macOS 不发布（RELEASE.md §1，2026-10-12 所有者定夺）。
#
# ⛔ 本脚本绝不跑格式化：不在这里插任何 `cargo fmt` / rustfmt 步骤。发版阶段只做
# 校验/构建/打包，源码必须保持「跑过测试的那一份」（理由与实测代价见
# docs/agents/engineering.md §多 agent 风格不一致）。

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

echo "==> 2/8 release 构建（先签宿主证明，再构建）"
# 090-5 L3：官方构建必须带 PF_BUILD_PROOF——占位符包的内层 wasm 插件全部拒载。
# 签名端在插件仓（inner/scripts/proof.mjs，Node 内置 crypto 零依赖）；
# build.rs 只做 env→常量（rerun-if-env-changed 已声明，env 从无到有必触发重编）。
# kid 默认 dev-2026（当前唯一密钥，公钥已随插件编译）；生成正式密钥后
# PF_RELEASE_KID=<new-kid> 覆盖，私钥缺失 = 硬失败，绝不静默出占位符包。
PF_PLUGINS_DIR="${PF_PLUGINS_DIR:-$(cd "$ROOT/.." && pwd)/pi-flash-plugins}"
PROOF_KID="${PF_RELEASE_KID:-dev-2026}"
PROOF_KEY="${PF_BUILD_KEY:-$PF_PLUGINS_DIR/inner/keys/priv/$PROOF_KID.pk8.pem}"
PROOF_SCRIPT="$PF_PLUGINS_DIR/inner/scripts/proof.mjs"
[[ -f "$PROOF_SCRIPT" ]] || { echo "✗ 找不到 proof.mjs: $PROOF_SCRIPT（PF_PLUGINS_DIR 可覆盖，默认仓旁 pi-flash-plugins）"; exit 1; }
[[ -f "$PROOF_KEY" ]] || { echo "✗ 宿主证明私钥缺失: $PROOF_KEY（插件仓跑 proof.mjs keygen --kid $PROOF_KID；流程见 pi-flash-plugins/inner/README.md）"; exit 1; }
# proof.mjs 输出两行 export（PF_BUILD_PROOF=<b64(payload).b64(sig)> / PF_BUILD_KID=<kid>；
# 纯 base64 字符集，eval 安全）。签名失败时 substitution 为空 → 下方非空校验拦截。
eval "$(PF_BUILD_KEY="$PROOF_KEY" node "$PROOF_SCRIPT" sign --kid "$PROOF_KID" --build-id "v$VERSION" --version "$VERSION")"
[[ -n "${PF_BUILD_PROOF:-}" && -n "${PF_BUILD_KID:-}" ]] || { echo "✗ 宿主证明签名失败（proof.mjs 无输出）"; exit 1; }
export PF_BUILD_PROOF PF_BUILD_KID
echo "    宿主证明已签：kid=$PF_BUILD_KID build-id=v$VERSION"
cargo build --release -p app
# 产物自检：b64("pf-official|") = cGYtb2ZmaWNpYWx8，注入成功则 proof 常量必在
# exe 字符串段（占位符字面量散布多处回退路径，不能作阴性信号，只验阳性）。
grep -aq "cGYtb2ZmaWNpYWx8" target/release/pi-flash.exe \
  || { echo "✗ exe 中未找到宿主证明（env 未生效或构建缓存异常），拒绝打包"; exit 1; }
echo "    宿主证明已烙入 exe（自检通过）"

echo "==> 3/8 组装 dist/pi-flash"
rm -rf dist/pi-flash "dist/$ZIP_NAME" "dist/${ZIP_NAME}.sha256" dist/SHA256SUMS
mkdir -p dist/pi-flash/vendor
cp target/release/pi-flash.exe dist/pi-flash/
NODE_BIN=$(command -v node || true)
[[ -n "$NODE_BIN" ]] || { echo "✗ 找不到 node（PATH 或 PI_FLASH_NODE）"; exit 1; }
cp "$NODE_BIN" dist/pi-flash/node.exe
cp -r vendor/pi dist/pi-flash/vendor/

# 045：subagent 扩展随包分发（PF 用 `-e <path>` 挂载，运行时 lazy import SDK，
# 无需构建）。放在 exe 旁的 extensions/，与 subagents.rs 的查找顺序一致。
if [[ -d extensions/pi-subagents ]]; then
  mkdir -p dist/pi-flash/extensions
  cp -r extensions/pi-subagents dist/pi-flash/extensions/
  echo "    subagent 扩展已随包分发"
else
  echo "    ⚠ extensions/pi-subagents 缺失，本包将没有 agent 工具"
fi

# 裁掉非宿主平台 esbuild 二进制（vendor/pi 416MB 里 284MB 是 26 个平台副本，
# 每包只用 win32-x64 一个 → 下载体积 200MB+ → ≈120MB；见 080 §1.2）。
# pi 1.2.0 起不再依赖 esbuild（bundle 零引用、依赖树无平台包）——目录不存在
# 即跳过裁剪，包里本来也没有可裁的东西。
ESBUILD_DIR="dist/pi-flash/vendor/pi/node_modules/@earendil-works/pi-coding-agent/node_modules/@esbuild"
# npm 提升（hoist）后 @esbuild 可能落在 vendor/pi/node_modules 顶层，两处都认
[[ -d "$ESBUILD_DIR/win32-x64" ]] || ESBUILD_DIR="dist/pi-flash/vendor/pi/node_modules/@esbuild"
if [[ -d "$ESBUILD_DIR/win32-x64" ]]; then
  for d in "$ESBUILD_DIR"/*; do
    [[ "$(basename "$d")" == "win32-x64" ]] || rm -rf "$d"
  done
  echo "    esbuild 裁剪完成（保留 win32-x64）"
else
  echo "    无 @esbuild 平台目录（pi 1.2.0+ 已不依赖 esbuild），跳过裁剪"
fi

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
# 同时带上一份 CHANGELOG，方便离线看说明（英文版存在且非空才带上）
cp docs/CHANGELOG.md dist/pi-flash/CHANGELOG.md
if [[ -s docs/CHANGELOG_EN.md ]]; then
  cp docs/CHANGELOG_EN.md dist/pi-flash/CHANGELOG_EN.md
fi

echo "==> 4/8 包内烟测（独立目录启动）"
SMOKE_LOG="$ROOT/dist/smoke.log"
# 烟测只验「启动无输出」，更新探查不属于启动门禁：release 构建默认跑更新检查，而
# GitHub Release 尚未创建时 /releases/latest 必404 → eprintln 污染日志（首发鸡生蛋）。
PI_FLASH_UPDATE_CHECK=0
export PI_FLASH_UPDATE_CHECK
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

TODAY=$(date +%F)
echo "==> 5/8 更新 CHANGELOG.md / CHANGELOG_EN.md"
# 说明文件双语约定：中文段在上，单独一行 <!-- EN --> 分隔，下接英文段。
# 中文段收编 docs/CHANGELOG.md（权威版），英文段收编 docs/CHANGELOG_EN.md，
# 全文作 GitHub Release 正文（<!-- EN --> 为 HTML 注释，渲染不可见）→ 同一 tag 下中英同页对齐。
# upsert_section <目标文件> <新版本段临时文件>：把新版本段写进 CHANGELOG——
# 同版本段已存在则原地替换（幂等，整脚重跑不会写出两份），否则插到首个 `## [` 段之前；
# 文件顶部的标题与说明段（# 更新日志 / 格式指路）原样保留。
upsert_section() {
  local target="$1" newsec="$2"
  [[ -f "$target" ]] || return 0
  awk -v vf="$newsec" -v ver="$VERSION" '
    BEGIN { while ((getline l < vf) > 0) sec = sec l ORS }
    skip && !/^## \[/ { next }
    skip { skip = 0 }   # 跳到下一段：标题行不消费，落回下方规则照常输出（0.1.2 实发曾吞掉 0.1.1 标题）
    /^## \[/ {
      if ($0 ~ "^## \\[" ver "\\]") { skip = 1; if (!inserted) { printf "%s", sec; inserted = 1 } next }
      if (!inserted) { printf "%s", sec; inserted = 1 }
    }
    { print }
    END { if (!inserted) printf "%s", sec }
  ' "$target" > "$target.new"
  mv "$target.new" "$target"
}

# 去段首空行（说明文件里 <!-- EN --> 前后常带空行，收编后会多出空行）
drop_lead_blank() {
  awk 'BEGIN{seen=0} { if (seen==0 && $0 ~ /^[[:space:]]*$/) next; seen=1; print }'
}

ZH_SEC=$(mktemp)
zh_body=$(sed '/^<!-- EN -->\r\?$/,$d' "$NOTES_FILE" | drop_lead_blank)
# 说明文件可自带 ## [v] 标题首行（规范示例款式，0.1.2 实发曾因此双标题）——
# 带了就剥掉，标题统一由这里 echo（日期归一为当天）
if head -n 1 <<<"$zh_body" | grep -qF "## [$VERSION]"; then
  zh_body=$(tail -n +2 <<<"$zh_body" | drop_lead_blank)
fi
{ echo "## [$VERSION] - $TODAY"; echo; printf '%s\n' "$zh_body"; } > "$ZH_SEC"
upsert_section docs/CHANGELOG.md "$ZH_SEC"
rm -f "$ZH_SEC"
if grep -q '^<!-- EN -->\r\?$' "$NOTES_FILE"; then
  EN_SEC=$(mktemp)
  en_body=$(sed -n '/^<!-- EN -->\r\?$/,$p' "$NOTES_FILE" | tail -n +2 | drop_lead_blank)
  if head -n 1 <<<"$en_body" | grep -qF "## [$VERSION]"; then
    en_body=$(tail -n +2 <<<"$en_body" | drop_lead_blank)
  fi
  { echo "## [$VERSION] - $TODAY"; echo; printf '%s\n' "$en_body"; } > "$EN_SEC"
  upsert_section docs/CHANGELOG_EN.md "$EN_SEC"
  rm -f "$EN_SEC"
  echo "    英文段已收编 CHANGELOG_EN.md"
else
  echo "    说明文件无 <!-- EN --> 英文段，CHANGELOG_EN.md 本次不收编"
fi

echo "==> 6/8 压缩 $ZIP_NAME"
powershell -NoProfile -Command "Compress-Archive -Path 'dist\pi-flash' -DestinationPath 'dist\\${ZIP_NAME}' -Force"

echo "==> 7/8 生成校验和（SHA256SUMS + 资产 sidecar，供 npm 薄壳下载校验）"
(cd dist && sha256sum "$ZIP_NAME" | tee "${ZIP_NAME}.sha256" > SHA256SUMS)

echo "==> 8/8 完成"
ls -lh "dist/$ZIP_NAME" "dist/${ZIP_NAME}.sha256" dist/SHA256SUMS
cat <<EOF

后续（一条命令收尾；等价手动序列见 RELEASE.md §3.2）：
  scripts/ship.sh ${VERSION} "${NOTES_FILE}"
npm 薄壳无需手动发——publish-npm.yml 随 tag 推送自动 OIDC 发布（RELEASE.md §4）。
EOF
