# pi-flash 发版规范（RELEASE）

> 本文是发版的**唯一操作规范**。机制与设计见 `docs/模块设计/080-软件分发.md`
> 用户可见变更史见 `docs/CHANGELOG.md`。
> 实操命令均已在 v0.1.0 发版中验证通过（2026-10-10）。

---

## 0. 版本纪律（铁律）

1. **版本号唯一决定人 = 仓库所有者。任何人（含 agent）不得擅自变更版本号，也不得提出版本号建议。**
2. **版本号唯一写入入口 = `scripts/release.sh`**：入参 → `crates/app/Cargo.toml` +
   `npm/package.json` + `Cargo.lock` 三处单点同步。禁止手工改版本字段。
3. **已发布版本永不复用**：npm 不允许重发同版本，git tag 不动。同一版本号发布后即封存。
4. **发错只能前向修复**：由所有者拍板下一个版本号后走完整流程；禁止修改已发布产物、
   禁止改写 CHANGELOG 已有版本段、禁止删改已发布 tag/release。
5. CHANGELOG 只增不改：`[Unreleased]` 段在发版时由 release.sh 收编进新版本段，历史段永不回头编辑。

## 1. 产物与通道

| 产物 | 通道 | 产生方式 | 备注 |
|---|---|---|---|
| `pi-flash-<v>-win32-x64.zip` + `.sha256` + `SHA256SUMS` | GitHub Release | 本地 `scripts/release.sh` | ≈86MB；解压 299MB（esbuild 已裁非宿主平台） |
| `pi-flash-<v>-darwin-arm64.zip` + `.sha256` | GitHub Release | macOS CI（tag 自动触发） | 资产名仅 tag 时带版本；sidecar 独立避免与 SHA256SUMS 互踩 |
| `pi-flash` npm 薄壳（7.4kB，仅 bin/） | npm 官方源 | **首发手动一次（0.1.0 已完成）→ 此后 `publish-npm.yml` OIDC 自动** | 薄壳安装时从 Release 拉载荷并 SHA-256 校验 |
| `docs/CHANGELOG.md` 新版本段 | 仓库 | release.sh 5/8：说明文件 + `[Unreleased]` 收编 | 说明文件同时作 GitHub Release 正文 |
| provenance 签名 | npm 包页 | OIDC 发布自动生成 | 供应链徽章，免配置 |

**依赖关系**：npm 包本身不装载荷，安装成败取决于 Release 资产是否已挂 → 资产先行。

## 2. 前置条件（checklist）

- [ ] 所有者已拍板本次版本号（见 §0，唯一来源）
- [ ] git 工作区干净（release.sh 门禁拒绝 tracked 改动；**并行会话的活必须先由其本人提交**，
      勿 stash、勿代提交）
- [ ] 门禁测试全绿：`cargo check --workspace`、`cargo test -p pi-link`、`node npm/test/e2e.js`（19 项）
- [ ] `pi-flash.exe` 未运行（烟测会 `taskkill` 全部实例；且 debug 实例正在运行会让 debug 构建撞 exe 锁）
- [ ] 发布说明就绪：`tmp/release-notes-<v>.md`（安装/更新命令 + 本版要点；并入 CHANGELOG 段与 Release 正文）
- [ ] 网络：github.com 可达（推送 + 资产上传）；npm 已登录（**仅首发/回退需要**）
- [ ] Trusted Publisher 若已配置：确认本次发版落在其 **2 天绑定窗口**内（见 §4）
- [ ] 说明文件在 `tmp/` 下（untracked，不触门禁）

## 3. 标准操作序列（唯一路径）

### 3.1 跑发布脚本

```bash
scripts/release.sh <v> tmp/release-notes-<v>.md
```

八步（任一失败即中止，修因后可整脚重跑，脚本自清 dist 幂等）：

1. 版本三处同步（Cargo + npm + lock）
2. `cargo build --release -p app`
3. 组装 `dist/pi-flash`：exe + node.exe + vendor/pi **裁非宿主 esbuild** + 启动说明
4. **GUI 烟测**：独立目录启动，**stdout/stderr 必须零输出**（`[perf]` 行 = 门控回归，
   查 `PI_FLASH_PERF`）；结束 taskkill
5. CHANGELOG：新版本段 = 说明文件 + `[Unreleased]` 收编
6. 压缩 `pi-flash-<v>-win32-x64.zip`（PowerShell Compress-Archive，根带 `pi-flash/` 包装目录）
7. 生成 `SHA256SUMS` + `<资产>.sha256`（薄壳校验取用顺序：sidecar → SHA256SUMS）
8. 完成，打印尾部块

### 3.2 按脚本尾部块依序执行（顺序敏感）

```bash
git add crates/app/Cargo.toml npm/package.json Cargo.lock docs/CHANGELOG.md
git commit -m "release v<v>" -- crates/app/Cargo.toml npm/package.json Cargo.lock docs/CHANGELOG.md
git tag v<v>
git push && git push --tags                       # ① 先推 tag（触发 macOS CI）
gh release create "v<v>" --verify-tag --title "v<v>" \
  --notes-file "tmp/release-notes-<v>.md" \
  "dist/pi-flash-<v>-win32-x64.zip" \
  "dist/pi-flash-<v>-win32-x64.zip.sha256" \
  "dist/SHA256SUMS"                               # ② 紧跟其后，别等 CI
cd npm && npm publish                             # ③ 仅首发/回退需要；配好 OIDC 后由 CI 自动，跳过
```

- **commit 必须带 pathspec**（如上）：工作区可能有并行会话 staged 的内容，裸 `git commit` 会把它卷进发版提交
- **①→② 间隔要短**：macOS CI 若先跑完而 release 不存在，`softprops/action-gh-release` 会自建 release，
  随后 `gh release create` 撞名失败（撞了就改用 `gh release upload v<v> <资产>` 补挂）
- 尾部块内的 `npm publish` 与 CI 的 `publish-npm.yml` **二选一**，勿双跑（重复发版报 EPUBLISHCONFLICT，job 红）

### 3.3 等 macOS CI

darwin 资产 + sidecar 由 CI 自动挂上同一 release（分钟级），无需干预；CI 失败见 §6。

## 4. npm 发布的自动化边界

| 环节 | 归属 | 说明 |
|---|---|---|
| 0.1.0 首发 | 手动（已完成） | npm 不支持 OIDC 发首版（npm/cli#8544），浏览器 2FA 仅此一次 |
| Trusted Publisher 配置 | 手动，npmjs 网页 | `danielbill/pi-flash` + workflow **`publish-npm.yml`**（逐字）+ **勾 Allowed action `npm publish`**；Environment name 留空；**配置后 2 天内须有一次成功发布完成绑定**，过期删除重建（不可编辑） |
| 此后每次发版 npm publish | 自动 | tag 推送 → `publish-npm.yml`：`id-token: write` OIDC 换票（零长效 token）、npm ≥11.5.1、自动 provenance |
| 终态加固 | 手动，一次性 | OIDC 验证成功后：Package Settings → Publishing access → **Require 2FA and disallow tokens**（此后长效 token 全禁，2027-01 bypass-token 直发移除前完成过渡） |

## 5. 发版后验收

- [ ] `gh release view v<v>`：win zip + sidecar + SHA256SUMS 三资产在
- [ ] `npm view pi-flash version --registry=https://registry.npmjs.org/` == `<v>`
      （OIDC 自动发时另看 Actions `publish-npm` 绿）
- [ ] 真机：`npm install -g pi-flash@latest --registry=https://registry.npmjs.org/`
      → 载荷下载/校验/落位成功 → `pi-flash` 启动正常
- [ ] macOS 资产稍后自动挂上（可后补验）
- [ ] 通知测试者；镜像站（npmmirror）同步有延迟，**首装带 `--registry` 官方源**

## 6. 异常处置

| 症状 | 处置 |
|---|---|
| 烟测 ✗ 启动有输出 | 看 `dist/smoke.log`；perf 行 = `PI_FLASH_PERF` 门控回归，修码重跑 |
| 门禁 ✗ 工作区不干净 | 等并行会话自行提交；勿 stash / 勿代提交 |
| git push 断网 | 网络恢复重推，本地提交不丢；勿强推 |
| 资产漏挂 / 传坏 | `gh release upload v<v> <文件> --clobber` |
| `gh release create` 撞名 | CI 已自建 → 改 `gh release upload` 补挂资产 |
| npm publish ENEEDAUTH/EOTP | 检查 trusted publisher 是否配好（Actions 里看 OIDC 日志）；未配则手动浏览器授权；勿新建长效 token（除非走 bypass-token 过渡方案） |
| Trusted Publisher 过期 | npmjs 删除重建，2 天窗口内再发一版完成绑定 |
| npm 版本发错 | 禁止重发同版本 → 前向修复，版本号由所有者拍板（§0） |
| 薄壳安装 404（包已上线、资产未挂） | 竞态窗口：等资产挂完，`npm rebuild pi-flash` 重试 |
| 薄壳下载 SHA-256 不符 | 自动删除并失败；确认资产与 sidecar 同批上传，重装 |

## 7. 速查

```bash
# 版本现状（三处 + tag）
grep -m1 '^version' crates/app/Cargo.toml && grep -m1 '"version"' npm/package.json && git tag
# 门禁三件套
cargo check --workspace && cargo test -p pi-link && node npm/test/e2e.js
# 官方源核对
npm view pi-flash version dist-tags --registry=https://registry.npmjs.org/
```

## 8. 关联

- 设计：`docs/模块设计/080-软件分发.md`（方案、包结构、风险预案、实施状态）
- 变更史：`docs/CHANGELOG.md`
- 先例参考：pi-web `docs/release.md`（npm 版发布清单）
