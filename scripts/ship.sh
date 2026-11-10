#!/usr/bin/env bash
# pi-flash 发版收尾一条龙：release.sh 八步全绿 + RELEASE.md §3.2 人工验收通过之后
# 跑这个，替代手动尾部块（RELEASE.md §3.3 的等价手动序列保留作排查/降级路径）。
#
# 用法：
#   scripts/ship.sh <版本号> <说明文件.md>
#   例：scripts/ship.sh 0.2.0 tmp/release-notes-0.2.0.md
#
# 步骤：前置校验（版本三处一致、资产三件在位、tag 未占用、人工验收标记在位）→ 发版提交
# （pathspec 限定五文件，不卷并行改动）→ tag → 推 main + tags（HTTPS 被墙
# 自动降级 SSH over 443）→ gh release create 挂三资产（紧跟推送，压缩薄壳
# 用户的 404 竞态窗口，080 §4）。
# npm 薄壳不在此发：publish-npm.yml 随 tag 自动 OIDC 发布（RELEASE.md §4）。

set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=${1:-}
NOTES_FILE=${2:-}
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "用法: scripts/ship.sh <x.y.z> <说明文件.md>"; exit 1; }
[[ -f "$NOTES_FILE" ]] || { echo "说明文件不存在: $NOTES_FILE"; exit 1; }
ZIP_NAME="pi-flash-${VERSION}-win32-x64.zip"

# --- 前置校验 ---------------------------------------------------------------
grep -q "^version = \"${VERSION}\"" crates/app/Cargo.toml \
  || { echo "✗ crates/app/Cargo.toml 版本不是 ${VERSION}（先跑 scripts/release.sh ${VERSION} ${NOTES_FILE}）"; exit 1; }
grep -q "\"version\": \"${VERSION}\"" npm/package.json \
  || { echo "✗ npm/package.json 版本不是 ${VERSION}"; exit 1; }
for f in "dist/${ZIP_NAME}" "dist/${ZIP_NAME}.sha256" dist/SHA256SUMS; do
  [[ -f "$f" ]] || { echo "✗ 缺资产 dist/$f（先跑 scripts/release.sh）"; exit 1; }
done
# RELEASE.md §3.2：人工验收门禁——标记由所有者验过 zip 后手工 touch；release.sh 重打包会清掉它
[[ -f "dist/MANUAL-OK-${VERSION}" ]] \
  || { echo "✗ 缺人工验收标记 dist/MANUAL-OK-${VERSION}：先按 RELEASE.md §3.2 解压 dist/${ZIP_NAME} 跑包内 exe 验收，通过后 touch 该文件再 ship"; exit 1; }
git rev-parse -q --verify "refs/tags/v${VERSION}" >/dev/null \
  && { echo "✗ tag v${VERSION} 已存在——版本号永不复用（RELEASE.md §0）；若指向旧提交是测试残留，处置见 RELEASE.md §3.3"; exit 1; } || true

echo "==> 1/3 发版提交 + tag"
FILES=(crates/app/Cargo.toml npm/package.json Cargo.lock docs/CHANGELOG.md docs/CHANGELOG_EN.md)
git add "${FILES[@]}"
git commit -m "release v${VERSION}" -- "${FILES[@]}"
git tag "v${VERSION}"

echo "==> 2/3 推送 main + tags（直连失败自动走 SSH over 443，AGENTS.md 铁律；两路都断才停）"
# owner/repo.git 从任意 origin 形式提取（https / scp 别名 / ssh URL 四种都认）
REPO_PATH=$(git remote get-url origin | grep -oE '[^/:]+/[^/:]+\.git$') || true
[[ -n "$REPO_PATH" ]] || { echo "✗ 无法从 origin 提取 owner/repo：$(git remote get-url origin)"; exit 1; }
SSH_URL="ssh://git@ssh.github.com:443/${REPO_PATH}"
push_with_fallback() {  # $1 = refspec（main / tag 名）；git push 单参数会被当远端名，必须 origin <refspec>
  local out
  if out=$(git push origin "$1" 2>&1); then return 0; fi
  echo "    origin 直推 $1 失败：$(echo "$out" | tail -1)"
  echo "    走 SSH over 443 重试…"
  GIT_SSH_COMMAND="ssh -i ~/.ssh/id_rsa_github -o StrictHostKeyChecking=no" git push "$SSH_URL" "$1"
}
push_with_fallback main || { echo "✗ 两条通道都推不动 main：本地提交与 tag 不丢，网络恢复后 'git push origin main && git push origin v${VERSION}'，再手动挂 Release（RELEASE.md §3.3）"; exit 1; }
push_with_fallback "v${VERSION}" || { echo "✗ 两条通道都推不动 tag：恢复后 'GIT_SSH_COMMAND=\"ssh -i ~/.ssh/id_rsa_github -o StrictHostKeyChecking=no\" git push $SSH_URL v${VERSION}'，再手动 gh release create（RELEASE.md §3.3）"; exit 1; }

echo "==> 3/3 挂 GitHub Release + 三资产"
gh release create "v${VERSION}" --verify-tag --title "v${VERSION}" \
  --notes-file "${NOTES_FILE}" \
  "dist/${ZIP_NAME}" "dist/${ZIP_NAME}.sha256" "dist/SHA256SUMS" \
  || { echo "✗ gh release create 失败：撞名多半是 release 已存在 → gh release upload v${VERSION} <资产> --clobber（RELEASE.md §6）"; exit 1; }

cat <<EOF
✓ v${VERSION} 已发布（npm 薄壳由 publish-npm.yml 自动发，分钟级）
验收（RELEASE.md §5）：
  gh release view v${VERSION}                          # 三资产 + 双语正文
  npm view pi-flash version --registry=https://registry.npmjs.org/   # CI 绿后 1-2 分钟内就绪，稍等再核
  真机：npm install -g pi-flash@latest --registry=https://registry.npmjs.org/
EOF
