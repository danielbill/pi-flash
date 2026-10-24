# 项目名
Pi-Flash 缩写 PF

# 项目目标
以PI-WEB为功能原型，ZED为性能原型，用Rust+GPUI编写极速Pi Windows桌面端。

# Quick Start

```bash
cd vendor/pi && npm ci && cd ../..              # 首次；node_modules 不入库
cargo run -p app                                # 运行（bin: pi-flash）
RUST_MIN_STACK=33554432 cargo test -p pi-link   # 协议/路径测试都在 pi-link
```

- 环境变量：`PI_FLASH_PI_BIN` 覆盖内置 pi；`PI_FLASH_THEME` 切主题（default/dark/rose）；`PI_FLASH_RPC_LOG` 开 RPC 线缆日志；**`PI_FLASH_DIR` 显式指定永远优先；未设时按构建档位取默认——debug/test（cargo run/cargo test）自动落 `./pi-flash-dev`，release 才落 `~/.pi-flash`**（2026-10-18 起 paths 层焊死，裸跑测试摸不到正式目录；dev home 已进 `.gitignore`）；**dev-flags（重型模块发版排除，2026-10-19 起）默认全关**：`<PF数据目录>/dev-flags.json`（支持 // 注释）按键控模块，false/缺省=关、改动重启生效；键住址 = pi-link `dev_flags::KNOWN`（现仅 `subagents`），env 覆盖 `PI_FLASH_<键大写>=1/0` 最高优先，启动 stderr 打 `[startup] dev-flags:` 状态行（仅 debug 构建——release 包零输出是烟测门禁）。子代理关 = 不加载扩展 + 隐藏设置页与面板按钮
- UI 验证一律走 pif-ui 自动化链路，禁止抢真实屏幕

# 铁律
- **快是第一卖点**：PF 的卖点就是快——启动/首帧/输入/渲染热路径**禁止同步重活**（wasm 编译、全仓扫描、网络、大文件哈希）；一律后台线程 + 过揭幕，能沉成缓存的必经缓存（编译产物/结果），缓存自校验、失配自动回退；改热路径要带实测数字（`Instant`/`[perf]` 计时进日志），不靠感觉
- **内置pi**：内置 `@earendil-works/pi-coding-agent`，vendor 进应用分发，运行时 spawn `node <app>/vendor/pi/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js --mode rpc`。**不读 PATH/系统 pi**；升级 = bump `vendor/pi/package.json` + VERSION + 跑 pi-link 符合性测试（wire 陷阱与升级复核点见 `docs/agents/pi-rpc.md`）。开发期 `PI_FLASH_PI_BIN` 可覆盖。
- 完成任务保证编译通过后提交git并push，验收交给用户，禁止UI测试
- **GitHub 推送**：origin 已直连 SSH over 443（`~/.ssh/config` 的 `Host github.com-443` 别名 → ssh.github.com:443，已配 IdentityFile），`git push`/`git fetch` 直接推拉即可。github HTTPS(443) 与 SSH(22) 间歇被墙，勿走 HTTPS 远端、勿手拼 GIT_SSH_COMMAND。
- UI绘制遵循 docs\UI设计\UI组件规范.md
- **发布产物不落本机**：构建产物只留在 `target/`（如 `target/release/pi-flash.exe`），禁止复制/替换本机已安装的 `D:\softwares\...\payload\pi-flash.exe`，也不要在 payload 目录生成 `.new` 之类的暂存文件——装不装、何时装由用户自己决定
- CHANGELOG发版驱动，平时勿碰！
- 禁止排版！pre-commit 自动对暂存的 crates/**.rs 跑 rustfmt
- 如非用户授权，禁止修改Settings模块！

# 路径
- vendored pi：`vendor/pi`（package.json+lock 入库；node_modules 走 `npm ci` 不入库）
- 架构设计：`docs/ARCHITECTURE.md`；复刻计划：`docs/PORT_PLAN.md`
- 模块设计：`docs/模块设计/`（编号即模块，正文里的 0xx/vNN 引用都指这里）
- 领域陷阱笔记：`docs/agents/`（见下方「领域笔记」节）
- 调研笔记：本地 `docs/调研/`（外部项目调研报告；**已 gitignore，不入库**，正文别再用链接引它）
- 软件分发：`docs/模块设计/080-软件分发.md`；发版唯一路径：`docs/RELEASE.md`
- bug 追踪：`docs/buglist.md`；
- 进度：`docs/progress.md`；
- 截图：`tmp/屏幕截图/`

# 架构
- workspace：`crates/pi-link`（钉版 pi RPC 协议层，测试在此）+ `crates/app`（GPUI 界面）
- gpui 0.2.2 / gpui-component **vendored**：`[patch.crates-io]` → `vendor/gpui`、`vendor/gpui-component`（workspace exclude；debug profile 保留行号表）
- 主题：`appearance/theme.rs`

# UI自动化测试
**agent 调试 UI 一律走 pif-ui 自动化链路，禁抢真实屏幕/鼠标/键盘，禁OS截图**
UI测试技能：`.agents/skills/pi-flash-ui-test/SKILL.md`

# File Map

一行一个职责；细粒度见各文件 `//!` 头注释。

```
crates/pi-link/src/     钉版 pi RPC 协议层（typed bridge，测试在此）
  protocol.rs             typed pi RPC（JSONL over stdio）
  client.rs               pi 侧车进程所有权与 JSONL 管线
  json.rs                 pi 返回 JSON 的解析入口（disable_recursion_limit 在此）
  transcript.rs           transcript 系统提示词 + 工具声明重放
  catalog.rs              启动期目录直读（get_commands 无内置命令的补偿）
  vendor.rs               vendored pi 运行时定位
  sessions.rs             会话发现（~/.pi/agent/sessions 索引扫描）
  recents.rs              最近活动会话列表（003）
  config.rs               pi 配置文件访问（Models 面板）
  models_json.rs          ~/.pi/agent/models.json（pi 的文件，PF 只读）
  registry.rs             全量 provider 注册表（042 新增 Provider 数据源）
  pf_auth.rs              ~/.pi-flash/pf-auth.json PF 密钥账本（051）
  pf_providers.rs         ~/.pi-flash/providers.json 自定义 provider（051）
  credentials.rs          OS 凭据库 keyring + $PF_KEY_* 引用（051）
  probe.rs                apikey 保存后通路探查（042）
  projects.rs             PF 项目注册表 ~/.pi-flash/projects.json（打开项目即注册，004）
  estimate.rs             token 估算（pi-web parity）
  mcp.rs                  mcp.json 全局/项目配置（043）
  skills.rs               技能发现 + SKILL.md frontmatter 编辑
  extensions.rs           扩展发现口径（复刻 pi core/extensions）
  session_ext.rs          会话级插件清单（031/034）
  agents.rs               Agent profile 目录读写（Settings-子代理页数据源，044-1）
  subagents.rs            内置子代理扩展定位（-e 挂载，同 pf_providers 模式）
  subagent_catalog.rs     子代理 catalog 流式读取（父会话 JSONL custom entry，044-1）
  dev_flags.rs            dev-flags.json 开发旗标（重型模块发版排除）
  paths.rs                PF 自有文件路径（不污染 pi，010）
  market/                 040-1 扩展市场 pi-link 侧（目录抓取/磁盘快照）
  automation.rs           pif-ui 自动化协议（NDJSON over TCP 127.0.0.1）
  bin/                    附属可执行（pif-ui CLI 等）

crates/app/src/         GPUI 界面（Chat = 全应用状态实体：直属字段只有 8 个
                        状态子结构 pool/composer/layout/psp/ws/dlg/cfg/aux——
                        跨域摸字段在代码里可见；域方法以 impl Chat 块形式住在
                        各领域文件——这是本仓的核心组织方式）
  main.rs                 只剩 mod 树 + 根 re-export（shell::{Chat,types::*}）+ main()
  shell/                  应用壳域（P3 自 main.rs 拆出）：types 界面类型簇
                          （Dialog/PanelTab/FileTab/Pill*，单域类型已下沉）/
                          chat Chat 实体本体（8 簇子结构+构造器）/ render Chat
                          渲染根（焦点仲裁+overlay 叠放）
  startup.rs              启动唯一入口（010）：Globals/ProjectCtx、boot 泵、splash
  agent_session.rs        AgentSession：RPC 进程所有权、事件泵、状态机
  content.rs              内容区状态机 chat / term / md（v54）
  dialogs.rs              弹窗域：模态渲染（ModelSelect/GitDiff/SessionSearch/…）+ 打开器
  ext_ui.rs               扩展 UI 面 + 协议动作 on_ext_ui/ext_respond（040）
  ask_card.rs             ask_user_question 整卷问卷卡（tab 分题+串行应答）
  terminal/               内置终端（alacritty+ConPTY）：mod 渲染/impl Chat +
                          palette + snapshot + input_encode（键盘/IME→PTY）
  i18n.rs                 三语（zh-CN 为源）
  i18n_audit.rs           i18n 审计（语义键源码级静态检查）
  models_config.rs        enabledModels 白名单编辑（pi-web port）
  assets.rs               图标编入二进制；status_bar.rs 状态栏；titlebar.rs topbar；
                          top_panels.rs 系统提示词/工具定义面板
  appearance/             界面域：theme.rs 七主题色板 + 注册表（registry 自 ALL 派生）
                          + 字体槽位 + 界面/多语言（006/007）
  automation/             pif-ui 自动化服务：snapshot / screenshot / op 分发
  editor/                 文件编辑器域：kind FileTypeHandler 注册表（按后缀注册，
                          未注册走默认文本 kind；打开路由/编辑器配置/主体渲染/
                          工具栏/右键菜单全由 handler 决定）+ view 编辑页（023）
                          + tabs 文件 tab 生命周期 + markdown/ 渲染与所见即所得（024/025）
  function_panel/         左 dock 域：psp 一体列表（015）+ psp_actions 项目动作 +
                          file_tree（021）+ file_actions 树文件动作 + git 面板（022）+ 悬浮层
  remote_control/         微信 ⇄ 会话桥（060）
  market/                 040-1 扩展市场页面（卡片/分页/预取）
  services/               无 UI 助手（pi-web lib/ parity）：file_icons / at_file /
                          file_index / file_tree / format / git / paths_sort / sound /
                          token_probe / updater / watcher / workspace / shell_open /
                          asset_evict（gpui 位图驱逐）/ plugin_updates（082）/
                          session_clean（052）
  session/                会话域：runtime/ 会话池实体（030；mod 生命周期+泵、
                          events 事件臂、send 发送+rpc 门面、nav 定位对账、
                          ingest 摄入）+ messages/（032；mod、model 类型与尾窗
                          解码、cards 工具/压缩卡、turn 行轮渲染、images 解码
                          缓存）+ input/（031 composer；mod 委托层、area 主输入
                          区、bar 控件行、ctx_tip 用量面板）+ pool 池 Chat 方法、
                          chat_list 滚屏、menu 补全菜单 + attachments 附件、
                          fork、diff、model/plugin/mcp picker、new_session（012）、
                          tools_recipe 档位配方（034）、subagent_panel 子代理
                          面板（044-1）、actions_bar 消息操作栏
  settings/               设置面板六页（界面/模型/技能/扩展/MCP/其他）；widgets 只剩
                          select_section，公共件全部转发 ui/kit（存量调用点零改动）
  ui/                     通用件：tokens（UI组件规范唯一代码落点）、button、text_input、
                          composer_input、dropdown、overlay、popup、vlist、psp_scrollbar、
                          tooltip、list_handle
  ui/kit/                 页面级组件库（**新页面只从这里取控件，禁就地手搓**）：
                          button（Btn+config_button）/ switch（三态开关）/ row（set_row、
                          field、文本块）/ layout（两栅骨架、LIST_W）/ badge（黊标·
                          状态点）/ slider（width_slider）；尺寸见 UI组件规范 §10
```

# 领域笔记（docs/agents/）

**改哪片代码，先读相关笔记；新陷阱写进所属笔记，不堆回本文件。**

- [pi-rpc.md](docs/agents/pi-rpc.md)：pi RPC wire 陷阱 + vendor 升级复核点。覆盖 `crates/pi-link/**` 与 app 侧消费方（agent_session、session/runtime、messages、ext_ui）。
- [gpui.md](docs/agents/gpui.md)：gpui 0.2.2（vendored）/ gpui-component 渲染与交互陷阱。覆盖 `crates/app/src/**`。
- [engineering.md](docs/agents/engineering.md)：Rust/工具链工程杂项（补丁方式、CRLF、Windows 路径、测试栈、真实按键链）。
- [market.md](docs/agents/market.md)：040-1 Pi 扩展市场（入口开关、页状态机、磁盘快照、布局踩点、与设计稿/规范的三方漂移）。覆盖 `crates/app/src/market/**` + `crates/pi-link/src/market/**`。



<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:970c3bf2 -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/SYNC_CONCEPTS.md for details and anti-patterns.

## Agent Context Profiles

The managed Beads block is task-tracking guidance, not permission to override repository, user, or orchestrator instructions.

- **Conservative (default)**: Use `bd` for task tracking. Do not run git commits, git pushes, or Dolt remote sync unless explicitly asked. At handoff, report changed files, validation, and suggested next commands.
- **Minimal**: Keep tool instruction files as pointers to `bd prime`; use the same conservative git policy unless active instructions say otherwise.
- **Team-maintainer**: Only when the repository explicitly opts in, agents may close beads, run quality gates, commit, and push as part of session close. A current "do not commit" or "do not push" instruction still wins.

## Session Completion

This protocol applies when ending a Beads implementation workflow. It is subordinate to explicit user, repository, and orchestrator instructions.

1. **File issues for remaining work** - Create beads for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **Handle git/sync by active profile**:
   ```bash
   # Conservative/minimal/default: report status and proposed commands; wait for approval.
   git status

   # Team-maintainer opt-in only, unless current instructions forbid it:
   git pull --rebase
   bd dolt push
   git push
   git status
   ```
5. **Hand off** - Summarize changes, validation, issue status, and any blocked sync/commit/push step

**Critical rules:**
- Explicit user or orchestrator instructions override this Beads block.
- Do not commit or push without clear authority from the active profile or the current user request.
- If a required sync or push is blocked, stop and report the exact command and error.
<!-- END BEADS INTEGRATION -->

<!-- BEGIN BEADS CODEX SETUP: generated by bd setup codex -->
## Beads Issue Tracker

Use Beads (`bd`) for durable task tracking in repositories that include it. Use the `beads` skill at `.agents/skills/beads/SKILL.md` (project install) or `~/.agents/skills/beads/SKILL.md` (global install) for Beads workflow guidance, then use the `bd` CLI for issue operations.

### Quick Reference

```bash
bd ready                # Find available work
bd show <id>            # View issue details
bd update <id> --claim  # Claim work
bd close <id>           # Complete work
bd prime                # Refresh Beads context
```

### Rules

- Use `bd` for all task tracking; do not create markdown TODO lists.
- Run `bd prime` when Beads context is missing or stale. Codex 0.129.0+ can load Beads context automatically through native hooks; use `/hooks` to inspect or toggle them.
- Keep persistent project memory in Beads via `bd remember`; do not create ad hoc memory files.

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/SYNC_CONCEPTS.md for details and anti-patterns.
<!-- END BEADS CODEX SETUP -->
