#!/usr/bin/env bash
# pi-flash 打包发布标准程序（Windows x64 绿色包）。
#
# 用法：
#   scripts/release.sh <版本号> <说明文件.md>
#   例：scripts/release.sh 0.2.0 tmp/release-notes-0.2.0.md
#
# 步骤：校验工作区 → 更新版本号（Cargo + npm 薄壳同同步）→ release 构建 →
# 组装 dist/pi-flash（exe + pif-secret.exe + node.exe + vendor/pi 裁非宿主
# esbuild + 启动说明）
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
# 同版本残迹放行（0.5.0 实录：上一次中止的 step1 已改版本号，重跑被自家残迹卡门禁，
# 每轮都要手工还原三件套）：脏文件仅限版本三件套、且 Cargo.toml / package.json
# 内容已是目标版本 = 确定性残迹，放行（step1 的 sed 幂等重打）；Cargo.lock 由
# step2 构建确定性重生成，同批放行。版本不符 = 人工误动，照旧硬失败。
DIRTY=$(git status --porcelain --untracked-files=no)
if [[ -n "$DIRTY" ]]; then
  RESIDUE_OK=1
  while IFS= read -r line; do
    f=${line#?? }; f=${f#\"}; f=${f%\"}
    case "$f" in
      Cargo.lock) ;;
      crates/app/Cargo.toml) grep -q "^version = \"$VERSION\"" "$f" || RESIDUE_OK=0 ;;
      npm/package.json)      grep -q "\"version\": \"$VERSION\"" "$f" || RESIDUE_OK=0 ;;
      *) RESIDUE_OK=0 ;;
    esac
  done <<< "$DIRTY"
  if [[ "$RESIDUE_OK" == 1 ]]; then
    echo "    检测到同版本残迹（三件套已是 $VERSION），放行整脚重跑"
  else
    echo "✗ 工作区有未提交改动，先提交再发布："
    git status --short
    exit 1
  fi
fi
[[ -f vendor/pi/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js ]] || {
  echo "✗ vendor/pi 未安装（cd vendor/pi && npm ci）"; exit 1;
}

# --- 0.5 打包现场预清理 ----------------------------------------------------
# 0.5.0 实录：extensions/pf-subagents/target-check（CARGO_TARGET_DIR 检查缓存，
# 4.3GB）随 cp -r 进包，90MB → 1.4GB，靠人眼在第 8 步才发现。git 已忽略的
# cargo 检查/构建缓存不属发版现场——仓库侧直接清，杜绝「dist 侧剥离漏网」与拷贝耗时。
for junk in extensions/*/target-check extensions/*/target; do
  [[ -d "$junk" ]] || continue
  echo "    预清理开发缓存：$junk"
  rm -rf "$junk"
done

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
# app 与两个独立 bin 合并为一次 cargo 调用：pif-secret/pi 不在 app 依赖图里，必须
# 显式 --bin（实测 cargo build -p app 不产出它们，2026-11-02）；合并后共享依赖只编
# 一遍，三目标由 cargo jobserver 并行调度（原三条串行命令要等 app 全部落地才轮到
# 两个小 bin，白占尾部几分钟）
BUILD_T0=$SECONDS
cargo build --release -p app -p pi-link --bin pi-flash --bin pif-secret --bin pi
echo "    [perf] release 构建（app+pif-secret+pi 并行）耗时 $((SECONDS - BUILD_T0))s"
# 产物自检：b64("pf-official|") = cGYtb2ZmaWNpYWx8，注入成功则 proof 常量必在
# exe 字符串段（占位符字面量散布多处回退路径，不能作阴性信号，只验阳性）。
grep -aq "cGYtb2ZmaWNpYWx8" target/release/pi-flash.exe \
  || { echo "✗ exe 中未找到宿主证明（env 未生效或构建缓存异常），拒绝打包"; exit 1; }
echo "    宿主证明已烙入 exe（自检通过）"

echo "==> 3/8 组装 dist/pi-flash"
rm -rf dist/pi-flash "dist/$ZIP_NAME" "dist/${ZIP_NAME}.sha256" dist/SHA256SUMS "dist/MANUAL-OK-${VERSION}"  # 标记随产物清：重打包即重验（RELEASE.md §3.2）
mkdir -p dist/pi-flash/vendor
cp target/release/pi-flash.exe dist/pi-flash/
# 042-1：pif-secret.exe 必须与 pi-flash.exe 同目录随包分发——auth.json 标记条
# 目是 pi 从系统凭据库取回 key 的唯一桥（pi 无 keyring 能力，靠 `!cmd` 执行它），
# helper_exe() 按 current_exe 同目录写绝对路径，包里缺它 = 用户侧全断。
[[ -f target/release/pif-secret.exe ]] || {
  echo "✗ target/release/pif-secret.exe 缺失（第 2 步构建应已产出）"; exit 1;
}
cp target/release/pif-secret.exe dist/pi-flash/
# 081：pi.exe 是全局 pi 前置 shim——PF 启动/更新时注册到
# %LOCALAPPDATA%\pi-flash\bin 并写 pi-target.json 指针；包内缺席则注册静默跳过。
[[ -f target/release/pi.exe ]] || {
  echo "✗ target/release/pi.exe 缺失（第 2 步构建应已产出）"; exit 1;
}
cp target/release/pi.exe dist/pi-flash/
NODE_BIN=$(command -v node || true)
[[ -n "$NODE_BIN" ]] || { echo "✗ 找不到 node（PATH 或 PI_FLASH_NODE）"; exit 1; }
cp "$NODE_BIN" dist/pi-flash/node.exe
cp -r vendor/pi dist/pi-flash/vendor/

# 045 / 001-4 / 090-6：随包扩展清单（唯一事实源）——PF 用 `-e <path>` 挂载，
# 运行时 lazy import SDK，无需构建，放 exe 旁 extensions/，与 subagents.rs /
# pfctx_manager.rs / service_ext.rs 的查找顺序一致。缺件 = 静默缺席：
#   pf-subagents    缺 = 没有 agent 工具（044）
#   pfctx-manager   缺 = 自我调试工具不注入（001，真机 2026-11-11 实锤）
#   pf-svc-runtime  缺 = ctx.service 执行域 shim 落空、service 声明告警（090-6）
# 新增/下线随包扩展：改此清单 + extensions/ 仓库目录 + 对应 *_rs 查找链，三者同步。
SHIP_EXTS=(pf-subagents pfctx-manager pf-svc-runtime)

# 清点①：仓内不得有清单外目录（0.5.0 实录：pfctx-manager、pf-svc-runtime 两连漏，
# 都是「目录在、清单没有」的静默缺席——新扩展先登记清单再入库，或移出 extensions/）
if [[ -d extensions ]]; then
  for d in extensions/*/; do
    name=$(basename "$d")
    [[ " ${SHIP_EXTS[*]} " == *" $name "* ]] || {
      echo "✗ extensions/$name 不在随包清单 SHIP_EXTS：先登记发版清单并接好查找链，或移出 extensions/"; exit 1;
    }
  done
fi

# 清点② + 分发：清单已登记但目录缺失同样硬失败（防清单与仓库反向漂移）
mkdir -p dist/pi-flash/extensions
for ext in "${SHIP_EXTS[@]}"; do
  [[ -d "extensions/$ext" ]] || { echo "✗ extensions/$ext 在随包清单中但目录缺失，对应能力将静默缺席"; exit 1; }
  cp -r "extensions/$ext" dist/pi-flash/extensions/
  echo "    $ext 扩展已随包分发"
done

# target-check/（CARGO_TARGET_DIR 检查缓存，git 已忽略）是开发期垃圾，cp -r 会照搬
# → 0.5.0 曾把包撑到 1.4GB（正常 ≈90MB），dist 侧剥掉，仓库侧不动。
for d in dist/pi-flash/extensions/*/target-check; do
  [[ -d "$d" ]] || continue
  rm -rf "$d"
  echo "    ⚠ 已从包内剥离 ${d#dist/}（开发缓存，勿入库勿随包）"
done

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
   （密钥本体存系统凭据库，auth.json 只留取回标记，与 pi CLI 共用）
   注意：包内 pif-secret.exe 是密钥取回通道，必须与 pi-flash.exe 同目录，勿删勿移。
2. 全局 pi 命令：pi-flash 首次运行/更新时把内置 pi 内核注册为系统默认
   （%LOCALAPPDATA%\pi-flash\bin\pi.exe 前置转发，新开终端生效）
3. 快捷方式请指向固定入口（安装版 payload\pi-flash.exe；解压版为解压目录内 pi-flash.exe），
   勿指向带版本号的目录，旧版本目录会被自动清理

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
SMOKE_FAIL=""
# 烟测只验「启动无输出」，更新探查不属于启动门禁：release 构建默认跑更新检查，而
# GitHub Release 尚未创建时 /releases/latest 必404 → eprintln 污染日志（首发鸡生蛋）。
PI_FLASH_UPDATE_CHECK=0
export PI_FLASH_UPDATE_CHECK
smoke_once() {
  : > "$SMOKE_LOG"
  (cd / && "$ROOT/dist/pi-flash/pi-flash.exe" > "$SMOKE_LOG" 2>&1 &)
  sleep 6
  if ! tasklist //FI "IMAGENAME eq pi-flash.exe" 2>/dev/null | grep -qi pi-flash; then
    SMOKE_FAIL="进程未起"; return 1
  fi
  taskkill //IM pi-flash.exe //F > /dev/null 2>&1 || true
  sleep 1
  if [[ -s "$SMOKE_LOG" ]]; then SMOKE_FAIL="启动有输出"; return 1; fi
  return 0
}
# Defender 实时扫描对刚拷出的 exe 会短暂持锁 → 首启 Permission denied（0.5.0 实录，
# 重跑即过）＝瞬时失败：歇 3 秒重试一轮，两轮都败才是真异常。
if ! smoke_once; then
  echo "    首轮烟测未过（$SMOKE_FAIL；多为 Defender 瞬时锁），3s 后重试…"
  taskkill //IM pi-flash.exe //F > /dev/null 2>&1 || true
  sleep 3
  if ! smoke_once; then
    echo "✗ 打包后烟测失败（两轮，$SMOKE_FAIL）："; cat "$SMOKE_LOG"; exit 1
  fi
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
# 压缩器择优（产物同为标准 deflate zip，根带 pi-flash/ 包装目录，npm 薄壳解压端
# System32 bsdtar -xf / Expand-Archive 回落两条路均实测可读、逐字节一致；
# 2026-11 实测 15379 文件/约300MB：bsdtar 24s·90.3MB，7z -mx=9 100s·85.9MB，
# Compress-Archive 分钟级最慢）：
# ⚠ bsdtar 必须带 --options zip:hdrcharset=UTF-8：libarchive 在 Windows 上默认
# 把非 ASCII 条目名按本机 ACP（zh-CN=GBK）写入且不置 EFS 标志，Rust zip crate
# 解压端按 CP437 解 → 落盘乱码名（0.5.0 实录：启动说明.txt → ╞⌠╢»╦╡├≈.txt）；
# EFS 名压缩后另有 zip_name_check.mjs 硬门禁兜底（任何压缩器产物不过即拒发）。
# ① System32 bsdtar（Win10 1803+ 自带，与薄壳解压端同一支，零依赖，最快）
# ② 7-Zip（PF_7Z 可覆盖路径；压得略小但慢 4 倍）
# ③ Compress-Archive 兜底（仅在前两者都缺时）
ZIP_T0=$SECONDS
ZIP_TOOL=""
SYS_TAR=""
if [[ -n "${SYSTEMROOT:-}" ]] && command -v cygpath >/dev/null 2>&1; then
  _p="$(cygpath -u "$SYSTEMROOT")/System32/tar.exe"
  [[ -f "$_p" ]] && SYS_TAR="$_p"
fi
[[ -n "$SYS_TAR" ]] || SYS_TAR="/c/Windows/System32/tar.exe"
if [[ -f "$SYS_TAR" ]] && (cd dist && "$SYS_TAR" -a --options zip:hdrcharset=UTF-8 -cf "$ZIP_NAME" pi-flash); then
  ZIP_TOOL="bsdtar"
else
  # bsdtar 失败可能留下半截产物——绝不留给后续 7z a 追加成混合包
  rm -f "dist/$ZIP_NAME"
  SEVENZIP=""
  if [[ -n "${PF_7Z:-}" ]] && [[ -f "${PF_7Z}" ]]; then
    SEVENZIP="$PF_7Z"
  else
    SEVENZIP=$(command -v 7z 2>/dev/null || true)
    if [[ -z "$SEVENZIP" ]]; then
      for c in "/d/softwares/7-Zip/7z.exe" "/c/Program Files/7-Zip/7z.exe" "/c/Program Files (x86)/7-Zip/7z.exe"; do
        if [[ -f "$c" ]]; then SEVENZIP="$c"; break; fi
      done
    fi
  fi
  if [[ -n "$SEVENZIP" ]] && (cd dist && "$SEVENZIP" a -tzip -y -mx=9 -bso0 -bsp0 "$ZIP_NAME" pi-flash); then
    ZIP_TOOL="7z"
  else
    echo "    （无 bsdtar/7-Zip，回落 Compress-Archive，较慢；建议确认 Win10 1803+ 或装 7-Zip）"
    powershell -NoProfile -Command "Compress-Archive -Path 'dist\pi-flash' -DestinationPath 'dist\\${ZIP_NAME}' -Force"
    ZIP_TOOL="Compress-Archive"
  fi
fi
echo "    压缩完成（$ZIP_TOOL，$((SECONDS - ZIP_T0))s）"

# 条目名编码硬门禁：非 ASCII 名必须 EFS/UTF-8（0.5.0 GBK 名乱码实录，机制见上）
node "$ROOT/scripts/zip_name_check.mjs" "dist/$ZIP_NAME"

# 体积硬守卫（0.5.0 实录：target-check 4.3GB 随包撑到 1.4GB，靠人眼在第 8 步才
# 发现——正常包 ≈90MB，守卫必须机器化）：绝对上限 250MB；能取到上一版资产体积
# （gh api，owner/repo 从 origin 解析）则动态比 1.8×；离线/gh 失效只降级跳过
# 动态对比，不阻塞打包。
CUR_MB=$(( $(stat -c%s "dist/$ZIP_NAME") / 1024 / 1024 ))
[[ "$CUR_MB" -le 250 ]] || {
  echo "✗ 包体积 ${CUR_MB}MB 超绝对上限 250MB（正常 ≈90MB）——打包现场混入异物，清完重跑"; exit 1;
}
REPO_SLUG=$(git remote get-url origin 2>/dev/null | sed -E 's#.*[:/]([^/]+/[^/.]+?)(\.git)?$#\1#')
PREV_TAG=$(git tag --sort=-v:refname 2>/dev/null | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | head -1)
if [[ -n "$REPO_SLUG" && -n "$PREV_TAG" && "$PREV_TAG" != "v$VERSION" ]]; then
  PREV_BYTES=$(gh api "repos/$REPO_SLUG/releases/tags/$PREV_TAG" --jq \
    ".assets[] | select(.name==\"pi-flash-${PREV_TAG#v}-win32-x64.zip\") | .size" 2>/dev/null || true)
  if [[ -n "$PREV_BYTES" && "$PREV_BYTES" -gt 0 ]]; then
    PREV_MB=$(( PREV_BYTES / 1024 / 1024 ))
    LIMIT_MB=$(( PREV_MB * 18 / 10 + 1 ))
    if [[ "$CUR_MB" -gt "$LIMIT_MB" ]]; then
      echo "✗ 包体积 ${CUR_MB}MB > 上一版（$PREV_TAG）${PREV_MB}MB × 1.8 = ${LIMIT_MB}MB——打包现场混入异物，清完重跑"; exit 1;
    fi
    echo "    体积 ${CUR_MB}MB / 上一版 ${PREV_MB}MB，正常"
  else
    echo "    ⚠ 取不到上一版资产体积（离线或 gh 失效），跳过动态对比（绝对上限已过）"
  fi
else
  echo "    ⚠ 无上一版语义化 tag 可比，跳过动态对比（绝对上限已过）"
fi

echo "==> 7/8 生成校验和（SHA256SUMS + 资产 sidecar，供 npm 薄壳下载校验）"
(cd dist && sha256sum "$ZIP_NAME" | tee "${ZIP_NAME}.sha256" > SHA256SUMS)

echo "==> 8/8 完成"
ls -lh "dist/$ZIP_NAME" "dist/${ZIP_NAME}.sha256" dist/SHA256SUMS
cat <<EOF

后续：
  1) 人工验收（RELEASE.md §3.2，所有者执行）：解压 dist/${ZIP_NAME} 到任意目录跑包内 exe，
     通过后  touch dist/MANUAL-OK-${VERSION}
  2) 一条命令收尾（缺上述标记拒发；等价手动序列见 RELEASE.md §3.3）：
     scripts/ship.sh ${VERSION} "${NOTES_FILE}"
npm 薄壳无需手动发——publish-npm.yml 随 tag 推送自动 OIDC 发布（RELEASE.md §4）。
EOF
