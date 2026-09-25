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
  - 陷阱：unbounded() 返回 (Sender, Receiver) 别解构反；Pixels 字段私有
    （f32::from / 除法）；paint 闭包要 move 自持数据（interactivity 可变借）；
    prepaint/paint 第 5 参是 prepaint state 非 hitbox（PrepaintState=Option<Hitbox>）

## 待办

已迁移至 beads 任务跟踪（`bd list` / `bd ready` 查看剩余工作；完成后 `bd close <id>`）。
计划全量见 PORT_PLAN.md。
