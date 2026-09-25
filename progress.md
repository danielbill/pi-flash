# pi-flash 进度记录

> 本文件是唯一进度台账（AGENTS.md 只保留铁律与路径）。
> 每轮工作后更新「当前状态」与「里程碑历史」。

## 当前状态（2025-09，提交 816fad6）
## 当前状态（2025-09，分支导航 fork 完成）

- workspace：`crates/pi-link`（协议层，25 测试）+ `crates/app`（GPUI 界面，7 markdown 测试）+ `vendor/pi`（钉版 0.87.1）
- **分支导航（pi-flash-kzw 已关闭）**：
  - pi-link：Command::GetTree / Command::Fork{entry_id}（wire `entryId`）、TreeNode 递归解析（80 字预览）、parse_tree fixture 测试
  - 入口①：工具栏「分支」pill → BranchTree 面板（BranchNavigator 令牌级：24px 行、16px 缩进参考线、7px 圆点 accent/path/border、U/A 徽章、+N skipped、40 字标签、无会话/暂无分支空态）
  - 入口②：用户消息 hover → 「新分支」按钮（group+group_hover opacity 0→1，11px git-branch）
  - 链路：fork(entryId) → pi 创建 branched session（position before：复制到该消息之前）+ 进程内 rebind → UI 清空消息 + GetState/GetMessages/GetTree 重载 + list_sessions 刷新；新会话文件首条消息时落盘（pi 行为）
  - entryId 映射：get_tree 响应收集 active path 上的 user entry ids 回填 Msg.entry_id；AgentEnd 后刷新 tree（新消息也能 fork）
  - helpers：tree_has_branches / build_active_path / compress_chain / message_label / select_top_level_branches / collect_path_user_ids（BranchNavigator.tsx 迭代实现 parity）
- 会话行已对齐 pi-web SessionItem（54px、ellipsis、spinner、hover ✏/🗑）；弹窗统一 ESC 关闭
- 功能已通：流式聊天/steer/中断/图片发送、Markdown+高亮、thinking 折叠、工具卡片、会话管理/改名/删除、模型切换、thinking 循环、斜杠/@ 菜单、文件预览、**fork 分支**
- 测试：32 全绿（pi-link 25 + markdown 7）
## 协议陷阱（实测钉进 fixture）

- pi RPC 无 navigate_tree（原地切 leaf 是 pi-web 服务端概念）；fork(entryId) position
  before 要求 entry 是**用户消息**，target = parentId，branched session 文件首条消息时才落盘
- gpui 0.2.2：`overflow_y_scroll`/`track_scroll` 只在 Stateful<Div>（需先 .id()）；
  `visible_on_hover` 不存在，用 `.group("x")` + 子元素 `.group_hover("x", |s| s.opacity(1.))`
- Windows `Path::components()` 的 RootDir 保留原始分隔符（/ 或 \），component 拼接
  不可用于路径 key；用字符串级规范化（/ → \、去尾、case-fold）
- 桌面版持久化用文件（~/.pi/agent/pi-flash-workspace.json），localStorage 不可用

- 内容块类型是 camelCase `"toolCall"`（message.content 数组）
- 流式 args 起始来自 `partialJson`（message_start 阶段 arguments 为空对象）
- 工具结果以 `role:"toolResult"` 独立消息回灌（需挂接回工具卡片）
- set_model 字段是 **`modelId`**（pi 报 `Model not found: x/undefined` 即此）
- client 不得硬编码 `--no-session`（会压掉 `--session`，恢复为空）
- GPUI：on_mouse_down/listener 漏 `cx.notify()` = 状态变 UI 不动

## 截图索引（tmp/屏幕截图/）

| 文件 | 内容 |
| 图标版-全貌.png | SVG 图标版主界面（当前）|
|---|---|
| 冒烟-hello-gpui.png | GPUI 首窗口（150% DPI）|
| spike-pi桥接.png | spike 端到端首轮对话 |
| 布局改版-全貌.png | pi-web 化布局全貌 |
| 斜杠菜单.png | 42 命令菜单 |
| 模型切换-ok.png | set_model ok 状态栏 |
| 模型弹窗.png | select model 弹窗（过滤+ctx 窗口宽）|
| 用户气泡-mist主题.png | 用户气泡+thinking 卡片 |
| 会话行-hover按钮.png | 单行截断+hover ✏/🗑 |
| 图片选择器.png | 🖼 attach_images 文件选择器 |
| 文件预览.png | Cargo.toml 预览弹窗 |
## 当前迭代纪要（M2 收尾 + M3 前两项）

- pi-flash-kzw 分支导航：fork/tree 面板 + 消息 hover「新分支」（已关闭）
- pi-flash-4su 文件树：collect_tree_rows 递归展开、24px 行/14px 缩进、
  chevron 状态、懒加载 read_dir（300/目录 cap）、滚动容器（已关闭）
- pi-flash-bnh git status/diff：porcelain=v1 -z 解析分类 M/A/D/R/U/C、
  numstat 汇总 +a -d header、文件徽章（11px bold pi-web 色）、目录含改动黄点、
  改动文件点击 → GitDiff 弹窗（untracked 合成 patch）（已关闭）
- workspace 记忆：~/.pi/agent/pi-flash-workspace.json（per-workspace last open
  + __last 全局指针），列表按项目过滤，项目选择弹窗，启动恢复最后 workspace+会话
- **pi-flash-oqg M3 内置终端**（已关闭）：
  - 依赖：alacritty_terminal 0.26.0（crates.io，非 zed git fork）；tty::new
    Windows 走 ConPTY，shell = ComSpec ?? cmd.exe（terminal-manager parity）
  - 架构：`terminal.rs` 自定义 gpui Element（Interactivity 内嵌 →
    track_focus/on_key_down）+ paint 阶段 shape_line 逐行绘制；Term 存
    `Arc<FairMutex>`，EventLoop spawn io 线程，Proxy(EventListener) →
    futures unbounded channel → Chat 泵任务（tab_id, Event）
  - pi-web 面板 parity：右侧面板 560px、TabBar 36px（terminal 图标+标题+X、
    中键关闭、active bg/font-weight）、header 38px（状态点 7px 黄绿红 + cwd
    11px mono + Restart）、exit/error 横幅、固定深色面 #111318（任意主题）、
    Consolas 13px/1.25、scrollback 8000、cursor #60a5fa、选区 #365b8a、
    xterm 16 色 = pi-web theme、padding 10/8/22/12
  - 输入：keystroke_to_pty 表（ctrl 字符/alt 前缀/APP_CURSOR 方向键/F1-F12/
    CSI 1;m 修饰）、bracketed paste、Ctrl+C 有选区=复制否则 ^C、Ctrl+V 粘贴
  - 交互：左键拖选（网格坐标 + display_offset 钳制）、滚轮 scroll_display、
    ALT_SCREEN 时滚轮→方向键（zed alt_scroll parity）、首帧 prepaint 适配
    cols/rows → term.resize + Msg::Resize
  - 测试 15 个：palette/snapshot(SGR/inverse/选区)/selection_text/键表/paste
    + **conpty_echo_smoke**（真 ConPTY round-trip，无需 gpui）
  - 偏差记录：无 Reconnect（进程内无 SSE 断连概念）；MOUSE_MODE 应用只做
    滚轮→方向键（无鼠标上报）；IME 仅 key_char 路径；渲染逐行 shape（CJK
    宽字符列对齐受回退字体影响，同 xterm.js 回退行为）
- **pi-flash-d3i M4 模型/Provider 配置面板**（已关闭）：
  - pi-link `config.rs`：getAgentDir() parity（PI_CODING_AGENT_DIR ?? ~/.pi/agent）、
    settings.json enabledModels 读写（保留其他键）、auth.json 凭据（api_key/oauth、
    OAuth 拒删 parity）、lenient JSON（BOM/行注释/尾逗号 = pi loader 行为）、原子写
  - app `models_config.rs`：pi-web lib/enabled-models.ts 纯逻辑移植——
    PatternResolution/Entry、materialize（空 scope → 验证过的 provider glob）、
    collapseProvider/normalizeProviderGlobs（自愈改名）、serialize（覆盖全部且无
    stale/pin 时删键）、last-model 守卫；模式匹配子集：精确 ref（大小写不敏感）、
    裸 provider、provider/*（不跨 /）、provider/**、*、:level pin
  - UI（900px 弹窗，底栏「模型」打开）：50px header + 240px 侧栏（provider 行
    h30、绿点=已配置、N/M 徽章）+ 详情页：provider 头（状态点）、API Key 编辑
    （input 6/9 圆角5、显示/隐藏、保存/删除 ConfigButton h28、auth.json 说明）、
    已启用模型区（13px/600 标题 + N/M mono + 全部启用/停用 h28 + 列表 max360
    圆角6 bg_panel、行 36px name 11px/id mono 10px/pin 芯片 + ConfigSwitch
    32×18 knub12、最后一启用模型禁用开关）
  - 联动：ModelSelect 选择器按 enabled 白名单过滤（resolveVisibleModels parity）；
    项目级 .pi/settings.json 覆盖时面板只读（editable=false parity）
  - 测试 +9：匹配/材质化/守卫/glob 归一化/pin 保留/stale 不动 + lenient 解析、
    settings/auth CRUD（app 23 + pi-link 29 = 52 全绿）
  - 偏差：OAuth 登录流不做（仅显示状态+退出=删凭据，无吊销）；models.json 自定义
    provider 编辑后置（pi-flash-68h）；测试按钮（completeSimple）后置；新 provider
    的模型需重启 pi-flash 才进 available 列表（configuredProviders 启动快照）
- **pi-flash-68h M4 插件/技能/工具面板**（已关闭）：
  - ModelsConfig 弹窗升级为 SettingsPanel：1080px、tab 条（96px 标签、24×2
    accent 下划线）——模型 / 技能 / 插件 / 工具；底栏 模型/技能/插件 三入口
  - pi-link `skills.rs`：DefaultResourceLoader 目录子集发现（项目 .pi/skills、
    .agents/skills、全局 agent skills、~/.agents/skills、settings.skills 路径）、
    SKILL.md frontmatter 解析（name/description/disable-model-invocation）、
    set_disable_invocation 前言编辑（插入/替换/删除行、缺块创建）=
    pi-web PATCH /api/skills parity；PackageSource 条目助手（entry_source/
    entry_disabled=四资源数组全空/资源计数/normalize_source）
  - config.rs 增 packages 读写 + defaultTools 读写（保留其他键）
  - vendor.rs：node_bin() 提取 + run_cli()（CREATE_NO_WINDOW，一次-off CLI）；
    插件 安装/移除 走 vendored `pi install/remove [-l]` 后台线程 → op 泵任务
    刷状态行 + 面板（source 归一化 "$ pi install " 前缀）
  - 技能 tab：项目/全局分组侧栏（绿点=可见）、详情 scope 标签 + 路径 + 描述 +
    「对模型可见」开关
  - 插件 tab：侧栏（状态点 accent=加载/dim=停用、项目徽章）+ 底部「添加插件」
    ConfigListAction；详情=来源/范围/资源计数/ext·skills·prompts·themes/
    启停开关（停用=资源数组清零 parity，启用=恢复 plain source）/移除；
    安装表单（来源输入 + 全局/项目分段 + 安装按钮）
  - 工具 tab：defaultTools 预设（全部/默认 read-bash-edit-write/只读
    read-grep-find-ls/无）写 settings.json，新会话生效（与 CLI --tools 一致）
  - 测试 +4（发现/无前言回退/前言开关 roundtrip/包条目助手），共 56 全绿
  - 偏差：SystemPromptPanel 后置——钉版 pi RPC get_state 不含 systemPrompt
    （pi-web 是进程内 SDK 直取，RPC 面无此命令）；skills.sh 搜索/安装后置；
    插件 update 后置；项目信任(trust)检查后置
- **pi-flash-fhf M5 扩展 UI 协议**（已关闭）：
  - 协议层：Event::ExtensionUi(Value) → typed ExtensionUiRequest{id, method}
    —— select/confirm/input/editor（阻塞）/notify/setStatus/setWidget/setTitle/
    set_editor_text；RPC 模式 **不支持 custom**（rpc-mode.js custom()=undefined，
    无需 custom-ui 假终端）；Command::ExtensionUiResponse{id,value,confirmed,
    cancelled}——**id 必须是请求 id**（pi 在 raw-line 层按 id 关联，to_record
    早返回绕过 cmd 序号 id 注入）
  - app：setStatus → 状态栏右侧 key: text 项（空文本=移除）；setWidget →
    编辑器上/下 widget 块（mono 行块，空行数组=移除，belowEditor 置底）；
    notify → 右上角 toast（info/warning/error 色）4s 自动消失（spawn+timer）；
    set_editor_text → 直接写入聊天输入框；setTitle 记录为 no-op（桌面窗口
    标题固定，pi-web 是 document.title）；阻塞类 → ext_dialog 弹窗
    （select=选项按钮、confirm=是/否、input/editor=文本输入+提交），
    ESC=cancelled，应答经 session.send 发 extension_ui_response
  - 焦点协调：ext_dialog 与 dialog 共用 dialog_focus（同时存在时 ext 在顶层）
  - 测试：协议 8 变体解析 + 响应记录形状（cancelled/confirmed/value 组合）
  - 偏差：expiresAt 超时由 pi 侧 resolve(defaultValue) 兜底，客户端不做倒计时；
    editor 单行输入（多行编辑器后置）
- **pi-flash-vxc M5 子代理面板**（已关闭）：
  - pi-link `subagents.rs`：SubagentProfile 模型（pi-web subagents.ts 对齐）、
    三个内置 profile（general-purpose 全工具 / explore、plan 只读）、发现目录
    （agentDir/agents、cwd/.agents/agents、cwd/.pi/agents，shadowing 顺序
    builtin<global<workspace<project，同名小写 last-write-wins）、markdown
    frontmatter（snake_case）round-trip、agents/settings.json（builtInEnabled/
    disabledBuiltIns/maxConcurrent 1-32，默认 10）
  - Settings 弹窗第 5 个 tab「子代理」：侧栏=运行区 + 内置/全局/工作区/项目
    分组（绿点=启用、「覆盖」徽章）；详情=scope 标签+路径+描述+工具+模型/
    思考/最大轮数 + 启用开关（内置→disabledBuiltIns，文件→frontmatter）+
    删除（文件 scope）；全局设置区=内置开关 + maxConcurrent 输入
  - 运行/控制：按 profile 以 CLI flags spawn 子 RPC 会话（--system-prompt/
    --tools/--model/--thinking），sa_runs 列表（运行/已完成/失败/已中止），
    子会话事件泵至 AgentSettled（AgentEnd→get_last_assistant_text 回读输出），
    运行详情=状态 + 输出文本 + 中止（Abort）
  - Command::GetLastAssistantText 新增
  - 测试 +5（内置/禁用、roundtrip+shadowing、settings、frontmatter 容错）
    = 61 全绿
  - 偏差：运行由面板手动触发（pi-web 由模型经 Agent 工具触发——该机制在
    pi-web 服务端层，不在 RPC 面）；profile 编辑器后置（markdown 本就是
    pi 的手编格式）；worktree 隔离/队列/父会话通知属 pi-web 服务端运行时，
    不在 RPC 面
- **pi-flash-4ok M6 主题运行时切换**（已关闭）：
  - theme.rs：THEME_IX AtomicUsize 全局索引，set_by_name/theme_name 运行时
    切换（UI 每帧重读，切换即全局重绘）；PI_FLASH_THEME 仍为 dev 覆盖
  - 持久化：settings.json 的 theme 键（config::read_theme/write_theme），
    与 pi TUI 共用同一配置；启动顺序 = env 覆盖 > settings.json > mist
  - Settings 弹窗第 6 个 tab「通用」：4 主题选择行（色板预览 bg/accent/muted
    圆点 + 当前标记），点击切换并写盘；显示 vendored pi 版本
  - 测试：theme 切换 roundtrip + 未知名拒绝 + 4 主题字段完整性（62 全绿）
- **pi-flash-04k M6 i18n 三语**（已关闭）：
  - `i18n.rs`：t(源字符串) 恒等映射方案——zh-CN 为源（恒等返回），zh-TW/en 走
    ~120 项 TABLE（(zh, zh-TW, en) 三元组线性查找）；未命中的字符串原样透传
    （新 UI 优雅降级）；tf({key} 模板) 处理带参消息
  - 语言运行时切换（LANG_IX AtomicUsize）+ 持久化（workspace 记忆文件
    __lang 键）；通用 tab 三语按钮（简体中文/繁體中文/English）
  - 全部 117 处 UI 字面量包裹 tr()/tf()（含状态栏、设置面板、弹窗、消息列表
    相对时间、错误消息、退出横幅）
  - 测试：恒等/翻译/未命中透传/模板替换（65 全绿）
  - 陷阱：源码里 `t(` 后缀重命名 t→tr 时误伤 .expect(/.format(（已修）；
    match 恒等返回借用输入生命周期、翻译返回 'static——统一 'a 签名
- **pi-flash-43o LLM 生成标题**（已关闭，最后一个 beads 任务）：
  - 方案 = bead 预案的「独立小会话」：一次性 `pi --no-session --print
    --no-tools --thinking off [--provider/--model 当前会话模型]` 后台线程
    生成，stdout 即标题（vendor::run_cli_stdout 只收 stdout，stderr 仅报错）
  - pi-web session-title.ts 移植：TITLE_SYSTEM_PROMPT/TITLE_PROMPT 原文、
    transcript 裁剪（用户轮 800 字、中间回复 300、最后回复 600、总预算 6000
    头部优先 40%、中段省略）、标题清洗（首行/剥引号反引号/markdown 符号/
    80 字符钳制）
  - 完成后走既有 SetSessionName RPC + refresh_state；titling 防重入；
    结果经 title_tx 泵任务回主线程
  - 实测：真机 deepseek --print 通道 stdout 干净（无工具噪声）
  - 测试 +3：裁剪/预算省略/清洗（68 全绿）
  - 与启发式差异：heuristic（首条用户消息截断）已移除，统一走 LLM
  - 陷阱：unbounded() 返回 (Sender, Receiver) 别解构反；Pixels 字段私有
    （f32::from / 除法）；paint 闭包要 move 自持数据（interactivity 可变借）；
    prepaint/paint 第 5 参是 prepaint state 非 hitbox（PrepaintState=Option<Hitbox>）

## 状态（2026-09-25）

**PORT_PLAN M1-M6 全部交付**：9 个 beads 任务按里程碑顺序完成并关闭
（M3 终端 → M4 配置面×2 → M5 扩展UI+子代理 → M6 CI/主题/i18n + 标题），
`bd list` = No issues found；测试 68 全绿（app 30 + pi-link 38）。
各任务实现要点与偏差记录见上方迭代纪要。

## 待办

beads 任务跟踪已清空。后续工作 → 新建 beads（`bd create`）。
计划全量见 PORT_PLAN.md。
