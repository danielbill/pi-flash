# 一条安装命令完成下载与更新（对标 pi-web）——调研

> **定稿设计见 `docs/模块设计/080-软件分发.md`（冲突以该文为准）**；本文为调研过程与先例数据存档。
>
> 结论先行：**推荐「npm 薄壳 + GitHub Releases 载荷」方案**——
> `npm install -g pi-flash@latest` 一条命令既是首次安装也是更新，
> 与 pi-web 的体验逐字相同，但把 ~120MB 载荷放在 GitHub Releases 而非 npm tarball 里。

---

## 1. pi-web 为什么能做到"一条命令装 + 更"

pi-web（功能原型，`D:\github\---ai-tools---\pi-web`）的做法：

```bash
npm install -g @agegr/pi-web@latest   # 首次安装
pi-web                                 # 启动
npm install -g @agegr/pi-web@latest   # 更新 = 重跑同一条命令
npm uninstall -g @agegr/pi-web        # 卸载
npx @agegr/pi-web                      # 不安装直接跑
```

拆解其机制（实测数据）：

| 要素 | pi-web 的做法 | 实测 |
|---|---|---|
| 发行通道 | npm registry | `@agegr/pi-web` 当前 0.11.0 |
| 载荷 | 预构建 `.next` 产物直接进 tarball | `dist.unpackedSize = 33,510,701` 字节 / 740 文件 |
| 入口 | `bin: pi-web.js`（校验 Node 版本 → 启动 next） | `node-version.js` 要求 Node ≥ 22.19 |
| 安装钩子 | `postinstall: node bin/prepare-terminal.js` | 装完即配 |
| 更新语义 | npm 自带：装新版本 → 重跑全部生命周期脚本 | README 原文"再次执行同一条安装命令" |
| 回滚 | `npm i -g @agegr/pi-web@旧版本` | 免费获得 |

**本质：npm registry 天然提供 install / update / uninstall / rollback / 缓存 / 镜像 全套语义，
pi-web 只是把"预构建产物"当作包内容发上去。** 要"像 pi-web 一样"，就是复用这套语义。

---

## 2. PF 现状（实测体量）

- 发布形态：`scripts/release.sh` 产 Windows x64 绿色 zip（exe + node.exe + vendor/pi + 启动说明）；
  macOS 走 `.github/workflows/release-macos.yml` 产 `pi-flash-macos-arm64.zip`。
- 组成与体积：

  | 组成 | 解压 | 说明 |
  |---|---|---|
  | `pi-flash.exe` | 70 MB | release 构建 |
  | `node.exe` | 93 MB | 内置 node（铁律：不依赖系统 node） |
  | `vendor/pi` | **416 MB** | 其中 **284MB 是 `@esbuild/*` 的 26 个平台二进制**，实际只用 1 个 |
  | 合计 | **≈580 MB** | tar+gzip 实测整包 **138 MB** |

- **关键发现：vendor/pi 只需宿主平台的 esbuild**（Windows 包留 `win32-x64` 12MB，macOS 包留 `darwin-arm64` 11MB），
  裁剪后解压 ≈310 MB、压缩 ≈120 MB —— 直接决定下载体验，应进 release.sh（见 §6-P0）。
- GitHub Releases：**目前没有任何已发布资产**（`gh release list` 为空），Windows zip 只存在本地。
- npm 包名：`pi-flash` **未被占用**（`npm view pi-flash` → 404）。
- 版本号现况：`crates/app` 0.1.1；npm 包将引入第三处版本（Cargo.toml / git tag / npm）。

---

## 3. 候选方案对比

### 方案 A（推荐）：npm 薄壳 + GitHub Releases 下载载荷

npm 包只含启动器（几十 KB）：`bin/pi-flash.js` + `postinstall` 下载脚本。
`npm install -g pi-flash@latest` 触发 postinstall → 按平台从 GitHub Releases 拉对应 zip →
SHA-256 校验 → 解压到包目录内 → 写版本戳 → 启动 exe。

- **先例**：electron（主包 1.17MB，`install.js` 从 GitHub 下载 zip）、playwright/puppeteer（postinstall 下浏览器）、MagicBell CLI 等（社区通式，见 §7）。
- **一条命令装/更/卸/回滚全部成立**（npm 语义原样继承）。
- 载荷不占 npm tarball：避开 base64 上传内存上限、镜像同步大包慢、发布超时等问题。
- 载荷落包目录内 → `npm uninstall` 自动清干净。
- 首次安装需要网络两次（npm 一次 + GitHub 一次）；GitHub Releases 资产走 Azure CDN，国内可达性尚可但不保证——需提供镜像回退（见 §6 风险）。

### 方案 B：载荷全塞 npm tarball（pi-web 的放大版）

pi-web 方式原样照搬；或用 codex 的"同名包平台版本 + optionalDependencies"变体。

- **先例**：`@openai/codex` 主包 13KB、平台载荷包单平台 **457MB unpacked** 也在 npm 上正常分发；`@biomejs/cli-win32-x64` 79MB。
- 优点：一次下载、离线缓存、`npm ci`/lockfile 可复现、走 npmmirror 对国内用户反而快；**没有第二跳**。
- 缺点：`npm publish` 要 base64 序列化 ~150-200MB tarball（吃内存、易超时）；npmmirror 同步大包有延迟；回滚要保留旧版本大包（npm 不删旧版本，packument 会膨胀——drizzle 曾因 packument 100MB 上限翻车）。
- 变体 B'（最省流量）：不发 node.exe、不发 vendor/pi —— 依赖系统 Node ≥22（pi-web 用户群本来就有）+ 把 pi 钉版作为精确版本 npm 依赖（`@earendil-works/pi-coding-agent@1.0.0`）。**但这改了"内置 node、vendor 进应用分发"的铁律**，仅作备选记录。

### 方案 C：幂等安装脚本（Zed 式）+ 应用内自更新

```powershell
irm https://github.com/danielbill/pi-flash/releases/latest/download/install.ps1 | iex   # Windows
curl -fsSL https://.../install.sh | sh                                                   # macOS/Linux
```

- 脚本幂等：装过=更新（Zed 的 `script/install.sh` 就是下载→解压→建软链，重跑即换新）。
- 应用内自更新（Zed：后台检查、下载、重启生效）可作为补充（PORT_PLAN M6 的"自动更新"）。
- 优点：**不需要用户有 Node**；下载源只有 GitHub 一处。
- 缺点：脚本要自己实现 平台判断/校验/断点重试/PATH/卸载；PowerShell 执行策略（`irm|iex` 在受限机器被禁）；"更新"对用户来说要么重跑脚本、要么等应用内提示——没有 npm 那种"一条命令"的统一心智。
- Rust 侧现成轮子：`self_update` / `self_replace` crate（应用内自更新用），`update-informer`（仅查版本提示）。

### 方案 D：包管理器

- Windows：`winget install` + `winget upgrade`（需向 microsoft/winget-pkgs 提 manifest PR，每版跟进或挂自动化）；scoop（自有 bucket，绿色包形态天然契合）。
- macOS：`brew install --cask`（GitHub Releases 做下载源，需提 cask token）。
- 优点：用户群习惯、系统级更新管理。缺点：**更新命令 ≠ 安装命令的"同一条"体验要靠各家升级语义**；发布流程多一个维护面；国内 winget 源覆盖不全。

### 方案 E：`cargo install --git ...`（否决）

vendored gpui（`[patch.crates-io]`）、wry/node 载荷、编译耗时数分钟——与"极速"和"一条命令"目标相悖，直接排除。

### 对比总表

| | 一条命令装+更 | 需要 Node | 载荷通道 | 卸载/回滚 | 发布改造量 | 国内可用性 |
|---|---|---|---|---|---|---|
| **A npm 薄壳+GH 载荷** | ✅（同 pi-web） | ✅ 需要（pi-web 用户本就有） | GitHub Releases | npm 免费 | 小 | GitHub 可达性需回退方案 |
| B npm 全量 tarball | ✅ | ✅ 需要 | npm 镜像 | npm 免费 | 小 | npmmirror 镜像反而快 |
| C 安装脚本+自更新 | ⚠️ 重跑脚本/应用内 | ❌ 不需要 | GitHub Releases | 自己写 | 中 | 同 A |
| D winget/brew | ⚠️ 各家升级命令 | ❌ 不需要 | 各家机制 | 各家管理 | 中（持续维护 manifest） | ⚠️ |
| E cargo install | ❌ | ❌ | 源码编译 | — | — | — |

---

## 4. 推荐落地设计（方案 A 详细设计）

### 4.1 用户侧体验

```bash
npm install -g pi-flash@latest    # 首次安装：下薄壳 → 下载载荷 → 校验解压 → 可启动
pi-flash                          # 启动（发现载荷缺失/版本不符会自愈补齐）
npm install -g pi-flash@latest    # 更新 = 同一条命令
npm uninstall -g pi-flash         # 卸载（载荷在包目录内，一并清除）
npx pi-flash@latest               # 不常驻安装的体验也保留
```

### 4.2 包结构（`npm/` 目录入库，随 release 发布）

```
npm/
├── package.json          # name: pi-flash, version: = Cargo 版本, bin: {pi-flash: bin/pi-flash.js}
│                         # scripts: { postinstall: "node bin/fetch-payload.js" }
├── bin/
│   ├── pi-flash.js       # 启动器：Node 版本检查 → 确保载荷存在且版本吻合 → spawn exe（常驻/转发信号）
│   └── fetch-payload.js  # 下载器：平台探测 → 下载 → SHA-256 校验 → 解压 → 写 .payload-version
└── README.md
```

要点：

1. **平台探测**：`process.platform` + `process.arch` → `win32-x64` / `darwin-arm64`（暂无 win32-arm64/linux 资产则明确报错）。
2. **下载源**：`https://github.com/danielbill/pi-flash/releases/download/v<ver>/pi-flash-<ver>-<platform>.zip`；
   可选环境变量 `PI_FLASH_MIRROR` 覆盖（国内镜像/自建 CDN 回退口）。
3. **校验**：release 上传 `SHA256SUMS`，下载后必须比对通过才解压；失败删临时文件退出非零（让 npm 报错，不装半截）。
4. **原子落位**：解压到 `payload.tmp-<pid>` → rename 成 `payload/`；版本戳文件写最后一步——崩溃自愈靠"无版本戳=重下"。
5. **首启自愈（关键兜底）**：postinstall 可能被 `--ignore-scripts`、公司策略、镜像剥掉——`bin/pi-flash.js`
   每次启动检查 `payload/.payload-version === package.json.version`，不符/缺失就先跑同一下载逻辑再启动。
   （electron 同款思路：装的时候没下成功，跑的时候补。）
6. **正在运行时更新**：Windows 下运行中的 exe 锁目录 → 启动器启动前检测已有实例（或下载失败报
   `EBUSY`）时提示"请先退出正在运行的 pi-flash 再执行安装"——pi-web README 同款提示。
7. **Node 版本门槛**：复用 pi-web `node-version.js` 的写法，`engines: { node: ">=22.19.0" }` + 启动时友好报错。

### 4.3 载荷瘦身（无论选哪个方案都该做）

`scripts/release.sh` 组装 dist 前裁掉非宿主平台 esbuild：

```bash
# Windows 包只留 win32-x64；macOS CI 同理只留 darwin-arm64
ESBUILD_DIR="dist/pi-flash/vendor/pi/node_modules/@earendil-works/pi-coding-agent/node_modules/@esbuild"
for d in "$ESBUILD_DIR"/*; do
  [[ "$(basename "$d")" == "win32-x64" ]] || rm -rf "$d"
done
```

> 风险核对：pi 运行时若在**用户机器**上重装/重解析 esbuild（如插件 bundling 触发 npm install），
> 会按宿主平台重新拉取，不受裁剪影响；裁剪只动"随包分发的静态副本"。上线前跑一遍 pi-link 符合性
> 测试 + 手动触发一次依赖 esbuild 的插件路径验证。
> 收益：解压 580MB → 310MB，下载压缩包 ≈200MB+ → **≈120MB**。

### 4.4 发布流水线改造

1. `scripts/release.sh` 在烟测通过后追加：
   ```bash
   gh release create "v$VERSION" --verify-tag --title "v$VERSION" \
     --notes-file "$NOTES_FILE" \
     "dist/$ZIP_NAME" "dist/SHA256SUMS"        # 资产 = 载荷（现状：从未上传过）
   ```
2. npm 版本与 Cargo 对齐：release.sh 里 `npm version $VERSION --no-git-tag-version --prefix npm`
   （三处同步：`crates/app/Cargo.toml`、git tag、`npm/package.json`——同一脚本内完成，禁止手改两处）。
3. 发布 npm：`cd npm && npm publish --access public`（`NPM_TOKEN` 进 repo secrets；pi-web 的
   `release` script 同款，可照抄）。
4. macOS workflow 产的 zip + SHA256 同样挂到该 tag 的 release（薄壳靠资产名解析，命名要带版本与平台）。
5. CHANGELOG、tag、README 安装段（中英日俄四语 README 同步——pi-web 先例）。

### 4.5 分阶段实施

| 阶段 | 内容 | 验收 |
|---|---|---|
| **P0** | release.sh 裁 esbuild + 上传 GitHub Release 资产（SHA256SUMS） | 重跑 release：release 页可见 zip 与校验文件；下载解压烟测通过 |
| **P1** | `npm/` 薄壳包（下载器 + 启动器 + 自愈）；release.sh 接 npm publish | 全新机器：`npm i -g pi-flash@latest && pi-flash` 可用；再发 0.0.x+1 版本，重跑同命令完成更新；`npm uninstall` 清干净；`--ignore-scripts` 下首启自愈 |
| **P2** | （可选，对应 M6）应用内"检查更新"：查 `npm view pi-flash version` 或 GitHub `/releases/latest`，提示后走薄壳同一下载器后台更新、重启生效 | 旧版本启动收到更新提示；重启后版本号变化 |

---

## 5. 风险与坑（逐条预案）

| 风险 | 预案 |
|---|---|
| `npm i -g` 需要 Node；Windows 上若 Node 装在 `Program Files` 全局目录要管理员权限 | 文档标注"需要 Node ≥22.19"（pi-web 同款门槛）；权限问题提示用户 `npm config set prefix` 或提权重试；后续可补 winget/安装脚本通道给无 Node 用户 |
| `--ignore-scripts` / 镜像剥掉 postinstall | 启动器首启自愈兜底（§4.2-5），装完不启动也能被 npm rebuild 恢复 |
| GitHub Releases 国内访问慢/失败 | 下载器实现重试 + `PI_FLASH_MIRROR` 环境变量回退；必要时把镜像地址写进默认候选列表 |
| npm publish 大包（若误选方案 B）base64 上传超时 | 方案 A 根本不把载荷进 tarball，天然规避 |
| 载荷 SHA-256 不符（传输损坏/被投毒） | 校验不过即失败重下；`SHA256SUMS` 随 release 资产走 |
| 更新时旧版 pi-flash.exe 正在运行（目录锁） | 启动器/下载器捕获 EBUSY → 提示"先退出运行中的实例"（pi-web 同款文案） |
| SmartScreen/杀软对未签名 exe 报警 | 现状绿色包同样存在；长期解 = 代码签名（Windows 证书），与安装方式正交，单列技术债 |
| 版本三处不一致（Cargo/tag/npm） | 全部收敛进 release.sh 单点改写，脚本内 grep 校验一致才继续 |
| 裁剪 esbuild 后运行时缺二进制 | P0 阶段跑 pi-link 符合性测试 + 真机走一遍插件安装/bundling 路径 |
| npm 包名被抢注 | `pi-flash` 当前 404 未占用——P1 落地前先 `npm publish` 占位（0.0.1） |

---

## 6. 结论

1. **pi-web 的"一条命令"= npm registry 的安装/更新/卸载/回滚语义 + 预构建产物进包**，不是什么私有机制。
2. PF 载荷（≈120MB 压缩后）比 pi-web（33.5MB）大一个量级，**照抄"全进 tarball"能跑但不优雅**；
   业界对大载荷的标准答案是 **npm 薄壳 + GitHub Releases 下载**（electron/codex/biome 先例）。
3. 推荐 **方案 A**：`npm install -g pi-flash@latest` 一条命令完成下载与更新，与 pi-web 心智完全一致；
   载荷从 GitHub Releases 拉、SHA-256 校验、包内落位、启动自愈兜底。
4. **两件事与方案解耦、应先做**：① release 裁 esbuild（下载体积 200MB+ → 120MB）；
   ② release.sh 上传 GitHub Release 资产（目前根本没上传，任何"从网上下载"的方案都无源可用）。
5. 无 Node 用户的通道（PowerShell/安装脚本、winget、brew）列为 P2 之后的增量，不阻塞主线。

## 7. 主要资料

- pi-web：`D:\github\---ai-tools---\pi-web`（README 安装段、`bin/pi-web.js`、`docs/release.md`、npm 实测 33.5MB）
- npm 大包先例：`npm view @openai/codex`（主包 13KB / 平台包 457MB）、`@biomejs/biome`（optionalDependencies 平台包 79MB）、`electron`（postinstall 下载 1.17MB 主包）
- npm 体积限制：npm/npm#12750（无硬上限、base64 上传吃内存）；vlt.io：drizzle 遭遇 packument 100MB 上限
- Zed：`zed-industries/zed/script/install.sh`（幂等下载解压）+ zed.dev/docs/update（后台自更新、重启生效）
- 社区通式：MagicBell《Distributing Platform-Specific Binaries with npm》（postinstall 模式）、mssql-mcp ADR-0028（optionalDeps + shim 自愈）
- Rust 自更新轮子：`self_update`、`self_replace`、`update-informer`（P2 备选）
