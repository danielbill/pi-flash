# Rust / 工具链工程杂项

覆盖全仓（crates/**、脚本、手工流程），不专属于某个模块。随踩随补。

- **Windows PowerShell 5.1 读无 BOM 的 UTF-8 `.ps1` 会按 ANSI（本机 GBK）解**：脚本里的中文字符串会炸成
  `TerminatorExpectedAtEndOfString`（解析期就挂）。`.ps1` 要么带 UTF-8 BOM，要么内部只用 ASCII（`-File` 跑时；
  `-Command` 走另一条路，不受影响）。
- **删除须进回收站**（仓库规则）：MSYS `rm` 不进回收站；用 PowerShell 的
  `[Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile/DeleteDirectory($p,'OnlyErrorDialogs','SendToRecycleBin')`
  （先 `Add-Type -AssemblyName Microsoft.VisualBasic`）。一批小文件先 `Move-Item` 打进一个临时目录再整包回收，
  回收站里只留一条记录。
- 大补丁用脚本文件，勿用 bash heredoc（会静默截断）；方法链中间插入语句后漏/多 `),` → 结构性编译错误。
- `gen` 是 edition 2024 保留字。
- python 脚本整文件重写会把 CRLF 翻成 LF：改前探 `\r`、写回保原行尾；子串替换锚点带行首 `\n` 防缩进误匹配。
- Windows 路径：`Path::components()` RootDir 保留原始分隔符（不可做路径 key）；盘符大小写双写（`d:\` vs `D:\`）跨写方真实存在 → 归一化键小写化（pi-link `path_key`）。
- 深递归 JSON 测试（deep_tree）在 Windows 默认测试栈溢出 → `RUST_MIN_STACK` ≥32MB（Quick Start 的测试命令已带）。
- UI 自动化断言交互 bug 必须走**真实按键链**（`input.keys`）：直调 setter 测不出 on_change 类缺陷（菜单 latch 泄漏即漏测案例）。
- **git-bash 的 `grep -i -F --` 会 core dump**（GNU grep 3.0 / MSYS，SIGABRT，退出码 134）：`grep -qiF -- pat` / `grep -iF -- pat` 必崩（`-qF --`、`-qi`、`-q` 都正常）。脚本里做「路径子串包含」判断不要用 grep，用 bash `case "${line,,}" in *"${pat,,}"*)`——轮询里顺带省掉反复起进程的开销（`scripts/upd-e2e.sh` 实录）。
- **改脚本/文档别把行尾换行吃了**：`edit` 的 oldText 带 `\n` 而 newText 不带，会把两行并成一行。`scripts/upd-e2e.sh` 里 `$PUI shot …` 那行曾被并进前一行 `… || bad …` 的尾部，而 `A && B || C && D` 的链式求值照样打印 ✓ ——**假绿**。测试脚本改完必须 `bash -n` + 真跑一遍数断言（本次就是靠「断言数 19/19 但 PNG 不在」才发现）。

## target/ 磁盘占用（实测 26G 膨胀的教训）

- **deps/ 与 incremental/ 会无限囤积**：同一 crate 按指纹（feature/参数不同即换 hash）囤好几份副本，cargo 只偶尔清旧的。长期开发后 `target/debug` 膨胀到 26G（deps 15G + incremental 7.4G），大头是陈旧指纹而非当前构建。定期 `cargo clean`（或直接删这两个子目录）即可，全部可再生，无损源码。
- **多会话共用同一 target/**：agent 会话、编辑器 rust-analyzer、手动 `CARGO_INCREMENTAL=1` 的构建都会往里写。清理后“又长出来”不奇怪——先查有没有活跃 cargo/rustc/rust-analyzer 进程再删，别删正在写的缓存。
  **症状**：另一会话正在跑 `cargo test` 时，本会话重新链接会以 `LNK1104: cannot open file '…\deps\pi_flash-<hash>.exe'` 失败——同名测试二进制正被对方执行中。看到 LNK1104（而非编译错误）就是撞车：等对方跑完重试，别改代码。
- **2026-10 配置决策**：`[profile.dev] incremental = false`（根 Cargo.toml），incremental/ 常驻从 ~7G 砍到 0；代价是改动所在 crate 重编译慢 20~40%，快迭代临时 `CARGO_INCREMENTAL=1`（env 覆盖 profile），用完记得会重新囤。deps/ 瘦身早有铺垫（`[profile.dev.package."*"] debug = false`），勿动 gpui/gpui-component 的 `debug = 1`（断点进 vendor 内部用）。

## 发版脚本（scripts/release.sh）踩过的坑

- **esbuild 路径会随 npm hoist 变**：断言原写死 `node_modules/@earendil-works/pi-coding-agent/node_modules/@esbuild`，新版 pi 把 `@esbuild/win32-x64` 提升到 `vendor/pi/node_modules/@esbuild` → 裁非宿主平台那步整脚失败。两处都认（先试嵌套再回落顶层）。
- **烟测「启动零输出」与更新探查鸡生蛋**：release 构建默认跑更新检查（`PI_FLASH_UPDATE_CHECK` 未设时 `!cfg!(debug_assertions)` 为真），而 GitHub Release 还没建时 `/releases/latest` 必 404 → `eprintln!("[updater] 检查更新失败…")` 把门禁打红。烟测须 `export PI_FLASH_UPDATE_CHECK=0`（门禁只验启动，不验更新）。
- **CHANGELOG 收编不能假设首行是版本段**：文件顶部的 `# 更新日志` 标题 + 说明段会被原 `tail -n +2` 吃掉，且同版本段已存在时会写出两份 → 已改成幂等 `upsert_section`（同版本段原地替换，否则插到首个 `## [` 之前，前言原样保留）。收编前还要去**段首空行**，否则 `<!-- EN -->` 前后空行会在英文段标题下多出一行。
- 烟测会 `taskkill` 全部 `pi-flash.exe`：发版前必须让用户自己优雅关掉实例（草稿/回合会丢），别替他 kill。

## npm 相关（本机环境事实）

- 本机 `~/.npmrc` 里 `//registry.npmjs.org/:_authToken` **已失效**（`/-/whoami` 返401）→ `npm unpublish/publish` 这类写操作会静默 no-op：`npm unpublish` 竟 exit 0 并打印 `- pkg@version`，但 registry 的 `versions` 毫无变化（内部 PUT `/-rev/<rev>` 返 404）。**写操作后必须回 registry 核对 versions**，别信退出码。
- 同一版本号不可重发：测试期手发过的版本，正式发版前必须先 unpublish（注意 72h 窗口），否则 `publish-npm` OIDC job 报 “cannot publish over the previously published versions”。
- **npm 版本号一经使用永不可复用**（docs.npmjs.com/policies/unpublish：“Once `package@version` has been used, you can never use it again”，unpublish 不可撤销）。测试期手发过 x.y.z → 正式发版只能换新号，别指望 unpublish 后重发同一号（packument 的 `time` 墓碑会让 PUT 持续 400）。
- **本机 github.com 是「间歇不通」，不是永久不通**（2026-10-17 复测：真 0.1.9 通过 `api.github.com` 认到 0.1.10，且从 `github.com/.../releases/download` 直连下载成功跑到 95%）。所以薄壳安装链路可在通的时候真机验；不通时才走本地镜像（`PI_FLASH_MIRROR=http://127.0.0.1:8765`，把 `dist/` 里的 zip + `.sha256` + `SHA256SUMS` 摆成 `<base>/v<版本>/` 布局用 `python -m http.server` 挂上）。

## 标准 debug home 与 0.1.9 升级测试台

- **标准 debug home：`./pi-flash-dev`**（已进 `.gitignore`）。**2026-10-18 起 paths 层焊死双 home：debug/test 构建（cargo run / cargo test）默认自动落它，release 才落 `~/.pi-flash`；`PI_FLASH_DIR` 仍可显式覆盖**。历史命令带不带前缀都等价：`cargo run -p app`。它跨轮保留（设置 / 更新状态 / 缓存），所以测试实例不用重新登录。注意：`cargo run --release` 走正式 home，想隔离请显式 `PI_FLASH_DIR`。
- **测试实例与真实 pi 的边界**：`PI_FLASH_DIR` 只搬 app 自有文件；pi 侧车仍用真实 `~/.pi/agent`（凭据/会话/设置）——测试实例产生的会话会出现在正常会话列表里，这是故意的（免登录）。
- **0.1.9 升级测试台**（人工复现升级/打断；2026-10-17 搭）：`D:\pf-test-019\`
  - `payload\` = 真 0.1.9 载荷（`dist/pi-flash-0.1.9-win32-x64.zip` 解压）；`baseline\payload\` = 母本（硬链接克隆，还原用）
  - `① 启动 0.1.9 测试实例.cmd`（`PI_FLASH_DIR` → `./pi-flash-dev`）/ `② 查看现状.cmd`（只读清单：残留条目名 + 载荷 exe 的 sha256 是否还等于干净 0.1.9）/ `③ 一键恢复干净的 0.1.9.cmd`（杀本目录进程 → 打印残留清单 → 清空 → 母本硬链接还原 + 校验）
  - 真源可用，无需假源（见上一条）
  - 打断实测（下载完成、待重启时杀进程）：留下 `payload.stage-0.1.10`（263MB）+ `update-staged.json`，`payload\` 本身没被碰（老方案 stage 阶段不动在用目录）
  - `③` 的还原是**硬删**（工作副本可反复重建，不进回收站）；母本 `baseline\` 才是真副本，别手删
  - **怎么让新代码被测到**（真源下新代码永远"已是最新"，因为自我版本 = 编译进 exe 的 0.1.10 = 真源最新发布）：临时降版窗口——`crates/app/Cargo.toml` 的 version 改 0.1.9 → `cargo build -p app` → `python tools/rig.py rebuild` → 立刻改回 + `git checkout -- Cargo.lock`。实测全程 2 分钟、`git status`/`git diff` 零残留（做不到时先确认没有别的 cargo/rustc 在跑）。改完 updater 想再测一次安装链路，就得重走一遍
  - **新代码档启动必须带 `PI_FLASH_UPDATE_CHECK=1`**（debug 构建默认不检查更新，真实发版包是 release 默认查）：`rig.py start` 已代设
  - 实测（2026-10-17，真源 0.1.10 资产 + 新代码）：下载走 `github.com/.../releases/download` → 解压 → 落位 → **`安装收尾：读过 15033/15033 个文件，301 ms`**（8 线程）→ `update.state=ready`。即"安装期把每个文件读一遍"对 15k 文件几乎不增加安装耗时
- npm ≥11.5.1 的 `allowScripts` 门控会对薄壳 postinstall 报“not yet covered by allowScripts”警告（实测安装仍会跑脚本，失败则 npm 回滚）；命中拦截时的兜底是 `pi-flash.js` 启动时 `payloadOk()` 自愈补齐。

## 启动时凭空多一个终端窗口（Windows 子系统/spawn flags）

**症状**：PF 主窗口出现前先弹一个终端窗口（Windows Terminal，tab 名 = `pi-flash`，
里面只有一行 `libpng warning: iCCP: known incorrect sRGB profile`，之后无提示符）。

**根因**：`crates/app/src/main.rs` 缺 `#![windows_subsystem = "windows"]` →
链接出的 exe 是 **console subsystem** → 进程一启动 Windows 就给它分配控制台 →
机器若以 Windows Terminal 作默认终端宿主，系统替它开一个 WT 窗口（那行 iCCP 警告
是 WT 自己加载 PNG 图标的诊断，与 PF 无关）。GUI 程序还会白挂一个
`conhost.exe 0x4` 子进程。

**判定姿势（可复用）**：

- `Get-CimInstance Win32_Process` 看进程树：谁名下挂着 `conhost.exe`（父进程即
  `AllocConsole` 的那个）；`conhost.exe --headless …` 则是内置终端的 ConPTY
  服务端（不显示窗口，正常）。
- 进程内自查：`GetConsoleProcessList` + `GetConsoleWindow`，`attached=true` 即
  本进程已在控制台里（临时 extern "system" 打两个点，诊断完删）。
- 只想验证 gpui 是否有责：写个 `#![windows_subsystem = "windows"]` 的最小 bin
  链 gpui 开空窗——**实测 gpui 无罪**（无 conhost 子进程），病根在 app 自身。

**铁律**：

1. `main.rs` 的 `#![windows_subsystem = "windows"]` **勿删**（文件头注释已写缘由）。
2. **任何** `std::process::Command` spawn Windows 子进程都要带
   `CREATE_NO_WINDOW`（0x0800_0000）。GUI 宿主 spawn console 程序（git /
   powershell / node）时，子进程默认新建控制台 → 又一个 WT 窗口。已带 flag 的：
   `pi-link/{client,vendor,registry}.rs`、`services/token_probe.rs`；
   本轮补上：`services/git.rs`（新增 `git_cmd()` 统一口径）、`services/file_index.rs`。
   **测试代码也不例外**：updater.rs 测试里的 powershell（Compress-Archive 造
   zip 夹具）漏 flag，每跑一轮 `cargo test -p app updater::` 就弹一个带进度条
   的 PS 窗抢屏幕（2026-10-18 实锤，agent 代跑测试时尤甚）——已加
   `hidden_powershell()` 帮手统一口径。**并发 spawn 也不例外**：测试里裸
   `Command::new("git")` 多线程同时 spawn 会撞 conhost 分配竞态——code=-1073741502
   （0xC0000142 STATUS_DLL_INIT_FAILED）、stderr 为空、单跑又必过（2026-10-19
   status_tests 实锤）——测试也要走 `git_cmd()`，别自建 Command。
3. 读 PE Subsystem 别手撸偏移——本地 python 按 `pe+24+68` 读 debug exe 得出 2、
   release 得出 3，与实测行为矛盾；**以「进程树里有没有 conhost / 探针
   attached」为准**，别信自己算的字段。

**实测（debug exe，隔离 home，启动后 15s 采样）**：修复前 PF 名下有
`conhost.exe 0x4` + `GetConsoleProcessList` 在 `main` 第一行就 `attached=true`；
修复后同一探针 `attached=false`、进程树只剩 `node.exe`（pi 侧车）与 `git.exe`
（都 CREATE_NO_WINDOW），无 conhost、无 WT 新进程。

## git 子进程语义三坑：pathspec / porcelain 路径基准 / -z rename（2026-10-19 实锤）

外部用户实测报告（`docs/pi-flash-file-tree-freeze-20261019/`，含复现脚本与
微基准）钉死，`services/git.rs` 的坑，全是「cwd==repo_root 时看不出问题」型：

1. **`git status` 无 pathspec = 扫整仓**：cwd 是仓库子目录时照扫仓库根，
   `-uall` 还递归展开全部未跟踪目录——家目录遗留仓库 + Windows junction
   实测 270 万条 / 414MB / 99.9s。PF 把结果全存 `ws.git_files`，点文件夹时
   UI 线程 `flatten()` 全量重建 HashMap → 冻死数秒。status/numstat 一律带
   `-- .` 钉在项目内（同环境 0.095s / 3289 条）。
2. **porcelain 输出是仓库根相对路径，不是 cwd 相对路径**：`cwd.join()` 在
   cwd==repo_root 时碰巧对，子目录项目全拼成假路径——徽标永远不亮、
   `changed_ancestor_dirs` 造出几十万个假祖先目录。修法：`git rev-parse
   --show-toplevel` 定根，重定基到 cwd（与 read_dir 同形态，git_map 才命中）。
3. **`-z` 下 rename/copy 是两条 NUL 记录**（`R  new\0old\0`），没有 TAB 可切；
   按 TAB 解析永远不命中，old path 还会被当成独立条目。修法：X/Y 为 R/C 时
   消费紧跟的下一条记录。

## 格式化：全自动（pre-commit 机械处理，2026-10-20 转向）

**现状**：本仓是 rustfmt-clean 仓。**排版不需要记任何规则，也不需要主动跑任何命令**：

- `.githooks/pre-commit` 对本次暂存的 `crates/**.rs` 跑 rustfmt，改了就自动
  `git add` 回暂存区 → 提交里看到的已是格式化后的版本（有未暂存改动的文件会跳过，
  留到下次提交处理）。
- 配置在仓库根 `rustfmt.toml`（`edition = "2024"`）。
- **`vendor/**` 永不格式化**：gpui / gpui-component 是 vendored 上游副本（workspace
  exclude），格式化会把本地补丁面炸开。钩子与 `scripts/fmt.sh` 都用 `crates/*.rs`
  的 pathspec 限死。
- `crates/app/src/i18n.rs` 的 TABLE 由文件内 `#[rustfmt::skip]` **局部**豁免（该文件
  其余部分照常格式化）。**那个属性不能删**：`i18n_audit` 按行解析表行（行 trim 后
  以 `(` 起 + 切 4 列），折行即 panic（实测 812 → 2546 行）。
- 基线提交 `b36f9b2`（121 个文件）登记在根 `.git-blame-ignore-revs`。
- 手工入口（平时用不到）：`scripts/fmt.sh <file.rs>` / `--all` / `--check --all`。

### 为什么现在能放开（此前 2026-10-18 曾全面禁用 rustfmt）

当时禁 fmt 不是口味问题，而是**「没有基线」**导致的三件事：

1. 整包或单文件格式化都会把无关行重排 → 功能提交掺进大片纯排版 diff
   （`1f66410` 实录：`markdown/*`、`shell/render.rs` 的噪音）。
2. `rustfmt mod.rs` 递归洗**整棵子树**（点名 2 个文件，实际脏了 33 个）。
3. `i18n.rs` TABLE 被按行解析，折行即爆。

有基线之后三条同时消失：格式化**幂等**（只动你新写的行）、子树已 clean（递归 =
空操作）、表行被 `#[rustfmt::skip]` 冻结。于是「格式化」从**风险动作**变成
**机械动作**，可以放心交给钩子——这也正是「排版问题用脚本解决、别变成 agent
的记忆负担」这条原则的落地方式。


### 事故处置

| 症状 | 处置 |
|---|---|
| 有人手工跑了整包 rustfmt，工作区一片脏 | `scripts/fmt-flood-clean.sh`（.rs 封进 stash，可恢复） |
| 只想回滚某个被洗的文件 | `git checkout HEAD -- <path>` |
| 基线本身要撤掉 | `git revert b36f9b2`，并把该行从 `.git-blame-ignore-revs` 删掉，然后 `git config --unset blame.ignoreRevsFile` |
| 想确认基线是否完好 | `scripts/fmt.sh --check --all`（期望：`0 个不符合`） |

## 任务栏固定随更新失效（081 布局 × Windows pin 语义，2026-10-23）

机制与修复主体在 `services/win_pin.rs` 的 `//!` 头注释：pin lnk 记录**固定那
一刻**窗口进程的 exe 绝对路径，而 081 启动链让窗口常年跑在 `payload-<版本>\`，
cleanup 删旧版本目录 → pin 死。修复三件套 = 显式 AUMID + 开始菜单稳定入口
lnk + `User Pinned\TaskBar` 死链自愈。这里只记工程陷阱：

- **`SetCurrentProcessExplicitAppModelID` 静态链接必炸**：windows 0.61 与
  windows-sys 0.52 的元数据都没生成它，且**SDK 的 shell32.lib 符号表里根本没有**
  （MSDN 标的导入库名有误；`grep` 过 10.0.26100 的 lib 实锤）→ 手写
  `#[link(name="shell32")]` 声明必报 `LNK2019: __imp_SetCurrentProcessExplicitAppModelID`。
  唯一稳路 = windows-sys `GetModuleHandleA` + `GetProcAddress` 运行时绑定
  （`win_pin::set_process_aumid`，HMODULE 在 0.52 里是 `isize`，判 `== 0`）。
- **AUMID（`dev.piflash.app`）发布后永不可改**：用户的 pin lnk / 跳转列表里
  记着它，改 = 全体用户固定全失效。
- **explorer「固定运行中窗口」的身份认领机制**：窗口带显式 AUMID 时，优先
  认领开始菜单里**同 AUMID** 的现成 lnk（Chrome 跨版本固定不坏的同款机制）；
  没有才现场造一个指向当前 exe 的 lnk——所以「开始菜单稳定入口」是修复的支点，
  只设 AUMID 不建 lnk 没用。
- **PROPVARIANT 手构造的释放陷阱**：windows 0.61 的 `PROPVARIANT` 实现 Drop
  （`PropVariantClear` 按 VT_LPWSTR 释放 `pwszVal`）。`IPropertyStore::SetValue`
  内部已拷贝 → 构造后必须 `mem::forget`，否则 Drop 会对 Vec 缓冲区 `CoTaskMemFree`
  二次释放（UB）。读取侧（`GetValue` 返回系统分配的）则走 Drop 正常清。
- **`PKEY_AppUserModel_ID` 在 windows 0.61 挪到了 `Win32::Storage::EnhancedStorage`**
  （元数据错位；去 PropertiesSystem 找不到）。
- **CLSID_ShellLink 无 CoClass 包装**：0.61 没生成 `ShellLink` 类，`CoCreateInstance`
  直接喂 GUID `00021401-0000-0000-C000-000000000046`。


