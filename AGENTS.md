# Pi-Flash（PF）

以 PI-WEB 为功能原型、ZED 为性能原型，用 Rust+GPUI 编写极速 Pi Windows 桌面端。

# Quick Start

```bash
cd vendor/pi && npm ci && cd ../..              # 首次；node_modules 不入库
cargo run -p app                                # 运行（bin: pi-flash）
cargo test -p pi-link                           # 协议/路径测试都在 pi-link（deep_tree 用例自抬 32MB 栈，无需配 env）
```

- 环境变量：`PI_FLASH_PI_BIN` 覆盖内置 pi；`PI_FLASH_THEME` 切主题（default/dark/rose）；`PI_FLASH_RPC_LOG` 开 RPC 线缆日志；`PI_FLASH_DIR` 数据目录显式指定永远优先（未设时 debug/test 落 `./pi-flash-dev`、release 落 `~/.pi-flash`）；dev-flags 开发旗标（键表 = pi-link `dev_flags::KNOWN`，env `PI_FLASH_<键大写>=1/0` 最高优先）——机制详见 `docs/agents/engineering.md`「dev-flags」节
- UI 测试：agent 调试 UI 一律走 pif-ui 自动化链路（`.agents/skills/pi-flash-ui-test/SKILL.md`），禁抢真实屏幕/鼠标/键盘，禁 OS 截图

# 铁律
- **快是第一卖点**：启动/首帧/输入/渲染热路径**禁止同步重活**，一律后台线程 + 过揭幕，能沉成缓存的必经缓存（自校验、失配自动回退）；改热路径要带实测数字（`Instant`/`[perf]` 计时进日志）
- **内置pi**：vendored `@earendil-works/pi-coding-agent`，**不读 PATH/系统 pi**；升级 = bump `vendor/pi/package.json` + VERSION + 跑 pi-link 符合性测试（wire 陷阱与升级复核点见 `docs/agents/pi-rpc.md`）；开发期 `PI_FLASH_PI_BIN` 可覆盖
- 完成任务保证编译通过后提交git并push，验收交给用户，禁非授权UI测试
- 复杂需求编写testcase，存 `test/` 目录，按功能模块编号分目录存放
- **GitHub 推送**：origin 已直连 SSH，`git push`/`git fetch` 直接推拉即可；勿走 HTTPS 远端、勿手拼 GIT_SSH_COMMAND
- UI绘制遵循 docs\UI设计\UI组件规范.md，禁手写参数造新组件
- **插件唯一开发载体 = Rust→wasm**：禁止先写 `main.js` 再移植 wasm，新能力先落 WIT；提交前 L1（编译+契约）+L2（wasm 逻辑）+L3（pif-ui 接线）全绿（`docs/模块设计/90-插件系统/90-8-插件开发闭环.md`）
- 构建产物只留在 `target/`，禁替换本机安装的 `D:\softwares\...\payload\pi-flash.exe`
- CHANGELOG 发版驱动，勿碰！
- 禁止排版！pre-commit 自动对暂存的 crates/**.rs 跑 rustfmt
- 勿修改“Settings”模块
- **models.json / auth.json 是 pi 的文件，但 042-1 起 PF 会写**：整份草稿保存、**不可读拒写**、proper-lockfile 同款锁（`fs_lock.rs`）；用户手写的明文/oauth 条目一律只读不改；apikey 只进系统钥匙库——规则详见 `docs/模块设计/042-1-模型管理理清对齐.md`

# 路径
- vendored pi：`vendor/pi`（package.json+lock 入库；node_modules 走 `npm ci` 不入库）
- 架构设计：`docs/ARCHITECTURE.md`；模块设计：`docs/模块设计/`（编号即模块）；领域笔记：`docs/agents/`
- 调研笔记：本地 `docs/调研/`（ignore 不入库，正文勿引用）
- 软件分发：`docs/模块设计/080-软件分发.md`；发版唯一路径：`docs/RELEASE.md`
- bug 追踪：`docs/buglist.md`；进度：`docs/progress.md`；测试用例：`test/`；临时文件入 `tmp/`

# 架构
- workspace：`crates/pi-link`（钉版 pi RPC 协议层，测试在此）+ `crates/app`（GPUI 界面）；分层与状态所有权详见 `docs/ARCHITECTURE.md`
- gpui 0.2.2 / gpui-component **vendored**（`[patch.crates-io]` → `vendor/`，workspace exclude）——渲染陷阱见 `docs/agents/gpui.md`

# File Map

```
crates/pi-link/src/     钉版 pi RPC 协议层（typed bridge，测试在此）
  protocol/               typed pi RPC：command/blocks/events/session/wire + tests/
                          （mod 全量 re-export 保公 API 路径）
  client.rs json.rs       pi 侧车进程所有权与 JSONL 管线 / pi JSON 解析入口
  transcript.rs catalog.rs  transcript 系统提示词重放 / 启动期目录直读
  vendor.rs               vendored pi 运行时定位
  sessions/ recents.rs    会话发现（info/scan/read/search）/ 最近活动会话列表（003）
  config.rs               pi 配置文件访问（Models 面板）
  models_json.rs auth_json.rs  models.json + auth.json 的 PF 写入视图（042-1）
  register_to_sys.rs      向系统注册：pif-secret + 全局 pi shim（051/081）
  fs_lock.rs migrate_0421.rs  文件锁 / 042-1 一次性迁移
  registry.rs credentials.rs probe.rs  Provider 注册表（弹窗元数据）/ OS 凭据库 keyring / 通路探查
  projects.rs estimate.rs mcp.rs skills.rs  项目注册表（004）/ token 估算 / mcp.json（043）/ 技能发现
  extensions.rs session_ext.rs agents.rs  扩展发现 / 会话级插件清单（031/034）/ Agent profile（044-1）
  subagents.rs subagent_catalog.rs  内置子代理扩展定位 / catalog 流式读取（044-1）
  dev_flags.rs paths.rs   dev-flags 开发旗标 / PF 自有文件路径（010）
  market/ automation.rs   扩展市场 pi-link 侧（040-1）/ pif-ui 自动化协议
  bin/                    附属可执行（pif-ui CLI、pif-secret 取 key、pi 全局前置 shim）

crates/app/src/         GPUI 界面（组织方式：Chat = 全应用状态实体，直属字段只有 8 簇
                        子结构 pool/composer/layout/psp/ws/dlg/cfg/aux；域方法以 impl Chat
                        块形式住在各领域文件——本仓核心组织方式，详见 ARCHITECTURE.md §3）
  shell/ main.rs startup.rs  应用壳（types/chat/render）/ mod 树 / 启动唯一入口（010）
  agent_session.rs content.rs  RPC 进程所有权、事件泵、状态机 / 内容区状态机 chat/term/md
  dialogs/                弹窗域（090）：provider/project picker、ask_card、ext_ui
  terminal/               内置终端（alacritty+ConPTY）
  appearance/ i18n.rs     界面域：七主题色板+字体槽位（006/007）/ 三语（zh-CN 为源）+ 审计
  automation/ assets.rs status_bar.rs titlebar.rs top_panels.rs  pif-ui 服务 / 图标编入 / 状态栏 / topbar / 系统提示词面板
  editor/                 文件编辑器域：kind 注册表 + view/tabs + markdown（023/024/025）
  function_panel/         左 dock 域：psp/file_tree/git 面板（015/020/021/022）
  remote_control/         微信 ⇄ 会话桥（060）
  market/                 扩展市场页面 + 三市场分段外壳（040-1/090-4）
  pf_plugin/              PF 插件宿主（090-4；02 契约）
  services/               无 UI 助手（pi-web lib/ parity）：updater（081）/workspace/models_config/
                          file_icons/at_file/file_index/file_tree/format/git/paths_sort/sound/
                          token_probe/watcher/shell_open/asset_evict/pi_extension_updates（082）/session_clean（052）
  session/                会话域：runtime（030）/messages（032）/input（031）/nav_panel（033）/
                          picker_shell/chat_list/menu/attachments/fork/diff/model·plugin·mcp picker/
                          new_session（012）/tools_recipe（034）/subagent_panel（044-1）/actions_bar
  settings/               设置面板六页（界面/模型/技能/扩展/MCP/其他）；公共件全部转发 ui/kit
  ui/  ui/kit/            通用件（tokens = UI组件规范唯一代码落点）/ 页面级组件库（新页面从这里取控件，禁手搓）
```

# 领域笔记（docs/agents/）

**改哪片代码，先读相关笔记；新陷阱写进所属笔记，不堆回本文件。**

- [pi-rpc.md](docs/agents/pi-rpc.md)：pi RPC wire 陷阱 + vendor 升级复核点。覆盖 `crates/pi-link/**` 与 app 侧消费方（agent_session、session/runtime、messages、ext_ui）。
- [gpui.md](docs/agents/gpui.md)：gpui 0.2.2（vendored）/ gpui-component 渲染与交互陷阱。覆盖 `crates/app/src/**`。
- [engineering.md](docs/agents/engineering.md)：Rust/工具链工程杂项（补丁方式、CRLF、Windows 路径、测试栈、真实按键链、dev-flags）。
- [market.md](docs/agents/market.md)：040-1 Pi 扩展市场（页状态机、磁盘快照、布局踩点）。覆盖 `crates/app/src/market/**` + `crates/pi-link/src/market/**`。

任务跟踪用 bd（beads），勿用 TodoWrite/markdown TODO 清单；协议跑 `bd prime`。
