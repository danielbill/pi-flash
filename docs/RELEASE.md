# pi-flash 发版规范（RELEASE）

> 本文是发版的**唯一操作规范**。机制与设计见 `docs/模块设计/080-软件分发.md`
> 用户可见变更史见 `docs/CHANGELOG.md`。
> 实操命令均已在 v0.1.0 发版中验证通过（2026-10-10）。

---

## 0. 版本纪律（铁律）

1. **版本号唯一决定人 = 仓库所有者。**
2. **版本号唯一写入入口 = `scripts/release.sh`**：入参 → `crates/app/Cargo.toml` +
   `npm/package.json` + `Cargo.lock` 三处单点同步。禁止手工改版本字段。
3. **已发布版本永不复用**：npm 不允许重发同版本，git tag 不动。同一版本号发布后即封存。
4. **发错只能前向修复**：由所有者拍板下一个版本号后走完整流程；禁止修改已发布产物、
   禁止改写 CHANGELOG 已有版本段、禁止删改已发布 tag/release。
5. CHANGELOG 只增不改：`[Unreleased]` 段在发版时由 release.sh 收编进新版本段，历史段永不回头编辑。

## 1. 产物与通道

  **平台范围：只发 Windows x64。不发布 macOS 版本**
`release-macos.yml` 已 `disabled_manually`，不做 darwin 资产，npm 薄壳在 macOS 上会报「暂无发布资产」。

| 产物 | 通道 | 产生方式 | 备注 |
|---|---|---|---|
| `pi-flash-<v>-win32-x64.zip` + `.sha256` + `SHA256SUMS` | GitHub Release | 本地 `scripts/release.sh` | ≈86MB；解压 299MB（esbuild 已裁非宿主平台） |
| ~~`pi-flash-<v>-darwin-arm64.zip`~~ | — | **不发布** | macOS 不在支持范围；薄壳代码仍保留 darwin 分支，只是不出资产 |
| `pi-flash` npm 薄壳（7.4kB，仅 bin/） | npm 官方源 | **首发手动一次（0.1.0 已完成）→ 此后 `publish-npm.yml` OIDC 自动** | 薄壳安装时从 Release 拉载荷并 SHA-256 校验 |
| `docs/CHANGELOG.md` + `docs/CHANGELOG_EN.md` 新版本段 | 仓库 | release.sh 5/8：说明文件中文段收编中文版、`<!-- EN -->` 后英文段收编英文版 | 说明文件全文（双语）作 GitHub Release 正文，同一 tag 下中英同页 |
| provenance 签名 | npm 包页 | OIDC 发布自动生成 | 供应链徽章，免配置 |

**依赖关系**：npm 包本身不装载荷，安装成败取决于 Release 资产是否已挂 → 资产先行。

## 2. 前置条件（checklist）

- [ ] 所有者已拍板本次版本号
- [ ] git 工作区干净（release.sh 门禁拒绝 tracked 改动；**并行会话的活必须先由其本人提交**，
      勿 stash、勿代提交；若按规范流程**预写**了 CHANGELOG 新版本段，同样要先提交——
      收编幂等，内容一致则零改动）
- [ ] CHANGELOG写完交所有者审批，审批通过方可继续
- [ ] 门禁测试全绿：`cargo check --workspace`、`cargo test -p pi-link`、`node npm/test/e2e.js`（19 项）
- [ ] 架构守卫 PASS：`scripts/check_arch.sh`（行数红线按**非测试行数**计；超线 = 先拆分或口径复核再发版）
- [ ] `pi-flash.exe` 未运行（烟测会 `taskkill` 全部实例；且 debug 实例正在运行会让 debug 构建撞 exe 锁）
- [ ] 发布说明就绪：`tmp/release-notes-<v>.md`（安装/更新命令 + 本版要点；**双语**：中文段在上、
      单独一行 `<!-- EN -->`、下接英文段，见 `docs/CHANGELOG规范.md`；中文段收编 CHANGELOG.md、
      英文段收编 CHANGELOG_EN.md、全文作 Release 正文。纯中文可发版，英文后补走 `gh release edit`）
- [ ] 宿主证明私钥在位（090-5 L3）：插件仓 `pi-flash-plugins/inner/keys/priv/<kid>.pk8.pem`
      存在（kid 由 `PF_RELEASE_KID` 指定，默认 `dev-2026`；私钥永不入库，缺失 =
      release.sh **硬失败**，绝不静默出占位符包——占位符包的内层 wasm 插件全部拒载）
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
2. 签宿主证明（插件仓 proof.mjs，`PF_BUILD_PROOF/KID` → build.rs 烙常量）+
   `cargo build --release -p app` + **exe 烙入自检**（proof b64 前缀必须命中，未烙入 = 拒打包）
3. 组装 `dist/pi-flash`：exe + node.exe + vendor/pi **裁非宿主 esbuild** + 启动说明
4. **GUI 烟测**：独立目录启动，**stdout/stderr 必须零输出**（`[perf]` 行 = 门控回归，
   查 `PI_FLASH_PERF`）；结束 taskkill
5. CHANGELOG：中文段收编 `docs/CHANGELOG.md`、英文段收编 `docs/CHANGELOG_EN.md`
   （双语以单独一行 `<!-- EN -->` 分隔；无英文段则只收中文，英文版不动）
6. 压缩 `pi-flash-<v>-win32-x64.zip`（PowerShell Compress-Archive，根带 `pi-flash/` 包装目录）
7. 生成 `SHA256SUMS` + `<资产>.sha256`（薄壳校验取用顺序：sidecar → SHA256SUMS）
8. 完成，打印尾部块

### 3.2 发版收尾（一条命令；排查时按手动序列逐步走）

```bash
scripts/ship.sh <v> tmp/release-notes-<v>.md
```

前置校验（版本三处一致、dist 三资产在位、tag 未占用）→ 发版提交（pathspec 限六文件）→
tag → 推 main + tags → `gh release create` 挂三资产。等价手动序列（顺序敏感）：

```bash
git add crates/app/Cargo.toml npm/package.json Cargo.lock docs/CHANGELOG.md docs/CHANGELOG_EN.md
git commit -m "release v<v>" -- crates/app/Cargo.toml npm/package.json Cargo.lock docs/CHANGELOG.md docs/CHANGELOG_EN.md
git tag v<v>
git push && git push --tags                       # ① 先推 tag（触发 publish-npm OIDC job）
gh release create "v<v>" --verify-tag --title "v<v>" \
  --notes-file "tmp/release-notes-<v>.md" \
  "dist/pi-flash-<v>-win32-x64.zip" \
  "dist/pi-flash-<v>-win32-x64.zip.sha256" \
  "dist/SHA256SUMS"                               # ② 紧跟其后
cd npm && npm publish                             # ③ 仅首发/回退需要；配好 OIDC 后由 CI 自动，跳过
```

- **commit 必须带 pathspec**（如上）：工作区可能有并行会话 staged 的内容，裸 `git commit` 会把它卷进发版提交
- **不发布 macOS**（§1 定夺）：`release-macos` workflow 已停用，不会自建 release，①→② 无需抢时间
- 尾部块内的 `npm publish` 与 CI 的 `publish-npm.yml` **二选一**，勿双跑（重复发版报 EPUBLISHCONFLICT，job 红）
- tag 已存在且指向旧提交（测试期残留）时：发版前 `git tag -f v<v>` + 强制推 tag。`gh release create --verify-tag` 只校核tag 存在，不校核指向哪个 commit

### 3.3 等 publish-npm

tag 推送后 `publish-npm.yml` 自动 OIDC 发薄壳（分钟级），无需干预；失败看 §6。

## 4. npm 发布的自动化边界

| 环节 | 归属 | 说明 |
|---|---|---|
| 0.1.0 首发 | 手动（已完成） | npm 不支持 OIDC 发首版（npm/cli#8544），浏览器 2FA 仅此一次 |
| Trusted Publisher 配置 | 手动，npmjs 网页 | `danielbill/pi-flash` + workflow **`publish-npm.yml`**（逐字）+ **勾 Allowed action `npm publish`**；Environment name 留空；**配置后 2 天内须有一次成功发布完成绑定**，过期删除重建（不可编辑） |
| 此后每次发版 npm publish | 自动 | tag 推送 → `publish-npm.yml`：`id-token: write` OIDC 换票（零长效 token）、npm ≥11.5.1、自动 provenance |
| 终态加固 | 手动，一次性 | OIDC 验证成功后：Package Settings → Publishing access → **Require 2FA and disallow tokens**（此后长效 token 全禁，2027-01 bypass-token 直发移除前完成过渡） |

## 5. 发版后验收

- [ ] `gh release view v<v>`：win zip + sidecar + SHA256SUMS 三资产在
- [ ] `gh release view v<v>` 正文双语（`<!-- EN -->` 为 HTML 注释不渲染；纯中文发版的用
      `gh release edit v<v> --notes-file <双语文件>` 补挂英文）
- [ ] `npm view pi-flash version --registry=https://registry.npmjs.org/` == `<v>`
      （OIDC 自动发时另看 Actions `publish-npm` 绿。CI 绿 ≠ registry 立即可查：
      npm "being processed" 有 **1–2 分钟**传播延迟，日志见 `+ pi-flash@<v>` 即发成功，稍等再核。
      npmjs.com 版本页显示 **Validating** = 已收件、校验中，registry 404 属预期——
      **agent 不轮询等待，通知用户即可**，处置见 §6 校验滞留行）
- [ ] 真机：`npm install -g pi-flash@latest --registry=https://registry.npmjs.org/`
      → 载荷下载/校验/落位成功 → `pi-flash` 启动正常
      （**本机 github.com 不通时用镜像验**：`PI_FLASH_MIRROR=http://127.0.0.1:<port>` +
      `python -m http.server`，把 dist 产物摆成 `<base>/v<版本>/` 布局，见 docs/agents/engineering.md）
- [ ] 真机更新链路（**仅当本版改动涉及 updater / npm 薄壳 / 载荷结构**；19 项断言）：
      `cargo build && scripts/upd-e2e.sh`
      （081 §7 的夹具：真 release zip + 本地假 API/镜像，隔离目录 + pif-ui 驱动，
      不碰真实安装、不改版本号；github 直连/镜像与 release 构建需有网环境另验）
- [ ] macOS：不在支持范围，**无 darwin 资产可验**
- [ ] 通知测试者；镜像站（npmmirror）同步有延迟，**首装带 `--registry` 官方源**

## 6. 异常处置

| 症状 | 处置 |
|---|---|
| 烟测 ✗ 启动有输出 | 看 `dist/smoke.log`；perf 行 = `PI_FLASH_PERF` 门控回归，修码重跑 |
| 门禁 ✗ 工作区不干净 | 等并行会话自行提交；勿 stash / 勿代提交。预写的 CHANGELOG 段自行提交后再整脚重跑 |
| ship.sh ✗ tag 已存在 / push 断网 / release 撞名 | 分别见 §0 永不复用、§6 git push 断网、§6 release 撞名；脚本各步失败信息已带对应处置 |
| git push 断网 | 网络恢复重推，本地提交不丢；勿强推 |
| 资产漏挂 / 传坏 | `gh release upload v<v> <文件> --clobber` |
| `gh release create` 撞名 | 多半是同名 release 已存在 → `gh release upload v<v> <资产> --clobber` 补挂 |
| npm publish ENEEDAUTH/EOTP | 检查 trusted publisher 是否配好（Actions 里看 OIDC 日志）；未配则手动浏览器授权；勿新建长效 token（除非走 bypass-token 过渡方案） |
| Trusted Publisher 过期 | npmjs 删除重建，2 天窗口内再发一版完成绑定 |
| npm 版本发错 | 禁止重发同版本 → 前向修复，版本号由所有者拍板（§0） |
| `publish-npm` 报 cannot publish over the previously published versions | 测试期手发过同号：先 `npm unpublish pi-flash@<v>`（**需登录 + 2FA，注意 72h 窗口**），再 `gh run rerun <publish-npm run id>` 让 OIDC 重发。本机 npmjs token 可能已失效（写操作 exit 0 但 registry 无变化）→ unpublish 后必须回 registry 核对 `npm view pi-flash versions` |
| npmjs.com 版本页显示 **Validating**、registry 一直 404（0.1.6 实录：CI exit 0 + provenance 已签，滞留 1h+，status 页全绿） | 请求已被 npm 收件、正跑校验队列，非内容被拒。**禁重发、禁重跑 CI job**（实体化后必撞重号）；**agent 不轮询等待，通知用户即可，后续由用户定夺**。参考阈值：滞留 12h 可提 npm support 工单、24h 必提（材料：包名/版本/publish 时刻/CI run 链接/shasum/provenance 透明日志链接）。版本未实体化 ≠ 已发布（§0.3 不烧号），若最终被弃，经所有者点头可重发同号 |
| 薄壳安装 404（包已上线、资产未挂） | 竞态窗口：等资产挂完，`npm rebuild pi-flash` 重试 |
| 薄壳下载 SHA-256 不符 | 自动删除并失败；确认资产与 sidecar 同批上传，重装 |

## 7. 速查

```bash
# 版本现状（三处 + tag）
grep -m1 '^version' crates/app/Cargo.toml && grep -m1 '"version"' npm/package.json && git tag
# 门禁三件套
cargo check --workspace && cargo test -p pi-link && node npm/test/e2e.js
# 架构守卫（行数/视图函数红线，非测试行数口径）
scripts/check_arch.sh
# 官方源核对
npm view pi-flash version dist-tags --registry=https://registry.npmjs.org/
```

## 8. 关联

- 设计：`docs/模块设计/080-软件分发.md`（方案、包结构、运行机制、限制）
- 变更史：`docs/CHANGELOG.md` 、 `docs/CHANGELOG_EN.md`
- CHANGELOG编写规范：`docs/CHANGELOG规范.md`
