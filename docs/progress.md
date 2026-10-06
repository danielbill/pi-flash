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
## 状态（2026-09-30）— v54 UI 重构全量落地（7 beads 关闭）

以 `docs/UI设计/主界面UI设计-2.html` + 设计说明为唯一蓝本的全量重构（用户确认两决策：全量一轮推进、按设计删除旧功能）：

- **布局骨架**：topbar 两段 36px（左段仅收放钮 / 右段内容 tabs+设置+窗口控制 42×36、关闭悬停红 #d8626a）；statusbar 30px 仅面板段三 tab（激活 nav 连体卡）；收起态面板+statusbar 全隐、收放钮跳右段；psp 宽 250–500 拖拽（282 默认、双击复位、`__ui` 持久化）
- **psp 一体列表**：跨项目扫描按会话 mtime 聚类 ≤N 项目（当前钉顶，设置-其他「默认加载项目数」）；title 行 4 钮（iconfont 打开项目/新建会话 + search + ⋯）；⋯ 两级菜单（列表方式 分组/平铺 + 排序方式 时间/手动，勾选态+持久化）；项目行收合（持久化）+ hover tooltip（深色 全路径 mono）+ ⋯/＋（资源管理器/终端/删除项目确认流）；会话行 15px 状态槽（旋转圈/未读绿点 `running_files`+`unread` 集合）；**hover 详情卡**（锚侧栏右缘外 8px、标题原地改名输入框 Enter/Esc、删除→取消/确认、300ms 离行宽限 + `card_hovered` 竞争修复）
- **内容区**：chat/term/md 状态机（ContentView），term/md tab 在 topbar-r（Obsidian 连体凸起卡）；终端从 dock 移入内容区（psp 项目菜单「在终端中打开」入口）；.md 点开走 md 预览 tab（760px 页）
- **composer**：悬浮胶囊（75%/min500、16px 圆角、0 高 wrapper、底 padding 135px 让位）；控件行 图片+工具预设 | 上下文环（ring-25/50/75/100 分桶）+模型+思考+圆形发送↑（运行中变停止红）
- **消息区**：用户气泡 62%/r14 无边框 + hover 操作行（复制/编辑/新分支+「X月X日 HH:MM」）；agent 回复「工作详情 · N 条消息 · N 次工具调用」折叠行（工具·对象列表、chevron、工作中不显示）；等待动画 spark 旋转（1.4s）+ shimmer 滑动条（1.6s）；hover 操作行（复制/用时/时间）
- **git 面板**：头部单行（项目名 #8a9d95 + Changes(N)/History 连体 tab）；Changes：View Diff/Stage All/变更树（目录嵌套+绿+徽标+复选）/底部 ⎇+↑N Push(git_ahead_count)/融入式提交区/最近提交条+uncommit(git reset --soft)
- **设置弹窗**：70%×98% 自带 36px topbar、左导航 200px 六页签（界面/模型/技能/子代理/插件/其他）+「默认加载项目数」「启动恢复」新增
- **启动**：每次默认最大化（`window.zoom_window()` 显式调用——gpui 0.2.2 Windows 的 `WindowBounds::Maximized` 创建路径不生效）；**位置不再持久化**（外框/客户区坐标存取不对称导致每周期漂移一个边框宽——用户报告的"每次打开下移"）
- **删除（按设计）**：TopPanel 系统提示词/工具定义、statusbar 状态文本、dock 终端视图、BranchTree/内容搜索对话框、title 生成（services/title.rs）、services/branch.rs（collect_path_user_ids/parse_export_html 迁入 runtime.rs）、tools 设置页、pages/welcome、DockPanel::Terminal、pill()
- **主题**：三层色阶 chrome/nav/content + text_soft/text_faint/danger 七主题全配；30 新图标（lucide 补齐 + iconfont 实底提取自设计稿 + ring 分桶）
- **实测通过（用户+工具驱动）**：菜单两级/删除项目/详情卡改名删除/状态栏切换/收起展开/宽拖拽/发送/等待动画；设置弹窗钮待用户复测（其屏幕坐标疑有外部悬浮窗干扰）
- **测试**：81 全绿（pi-link 47 + app 34）

**gpui 0.2.2 新陷阱（钉死）**：① `list()` 虚拟列表 item 宽度语义不可靠（psp 弃用改全量 div 渲染）；② overlay 容器必须是 `absolute().inset_0()`（流内 0 高容器裁掉 absolute 子元素——菜单曾不可见）；③ `WindowBounds::Maximized` 创建路径无效需 `zoom_window()`；④ hover 竞争：行 on_hover 退出事件可能晚于卡进入，用 `card_hovered` 标志门控；⑤ 调试 hitbox：patch gpui `on_mouse_down` 打印 `is_hovered+bounds` 最快定位

## 状态（2026-09-30 晚）— v54 迭代收尾（用户验收通过）

主体重构后多轮小步迭代（均已提交、用户验收）：
- **绑定域定案**：statusbar 会话标签↔会话内容区(chat)、文件树标签↔浏览操作区(终端/文件预览 browse_last)、git 标签仅切面板；启动固定会话界面
- **topbar 双态**：会话视图左对齐会话标题（≤15 字，pi 名优先/首条消息回落）；终端/文件 tabs 仅浏览操作区显示（切回会话即消失）
- **文件预览 tab 化**：全部文件走内容区 tab（无弹窗）；md 渲染（复用 agent 渲染器）、图片 gpui img()、源码单文本块+行号（修逐行 div 卡顿）+ 垂直滚动条（gpui-component Scrollbar）；横向溢出修复
- **html 打开**：webview 内嵌尝试 3 轮失败（Foreground/FindWindow HWND 挂错应用、gpui Window 直接传→消息重入 panic、三段式仍不显示）→ 定案绕行：系统默认浏览器打开，wry 全链路移除，等 gpui 生态成熟再评估
- **会话导航修复**：比例尺常显（单轮也渲染）、激活位随滚动追踪、flyout 自持 hover+250ms 宽限（修无法点击）、单行摘要（首条有正文回复首句，非会话副本）、选择框/比例尺亮点跟随鼠标+四面完整边框、点击定位 scroll_to_reveal_item
- **鼠标穿透**：全部 overlay（flyout/菜单/确认层/对话框遮罩/设置遮罩/pill 菜单/详情卡容器）挂 occlude() 截断 hit-test
- **杂项**：statusbar 30→36px、消息时间「X月X日 HH:MM」、调试日志清理
- **验收通过清单**（用户确认）：双绑定、topbar 双态、文件滚动条、html 浏览器打开、导航定位、菜单/二级菜单/详情卡改名删除/收起态/拖宽/等待动画/发送
- **遗留**：beads = check_arch 复跑、手动排序真拖拽；wry 内嵌 GPUI 的重入问题已记录（绕行）；未 push

## 状态（2026-10-01）— v54.6 修复 + v54.7 UI 主题一致性（待用户验收）

- **v54.6（2d87783，用户已确认修复）**：IME 中文输入 panic（char 下标当字节下标 → utf16_to_byte_offset + safe_replace_range）；等待动画与乐观回显解耦（phase_waiting 拆 pending_echo，模型名+省略号补齐）；shimmer 骨架条删除（视觉噪音）
- **v54.7（d82a5f8，UI 一致性专项）**：
  - **markdown 渲染对齐 pi-web 规格**（MarkdownBody + CodeBlock parity）：正文 14px/行高 1.7；标题克制放大（1.16/1.08/0.98em、margin 10/5、h3 混色）；列表 marker accent 72% 混色 600 字重；代码块完整结构（外框圆角 7 + 头部语言名/复制按钮 + 行号 gutter + 12.5px/1.62 + bg 92%混 panel）；引用块 3px 混色边 + bg-subtle 底 + muted 文字；**表格渲染（此前表格内容直接丢失）**：外框 + th bg_panel 650 + 斑马纹 + 行分隔线；独立图片段落 gpui img() 真渲染（http/缺失降级 alt 占位）；strong 混 accent、em muted、删除线支持
  - **语法高亮随主题明暗切换**（pi-web Prism vs/vscDarkPlus parity）：浅色 InspiredGitHub / 深色 base16-ocean.dark——修复固定 base16-ocean.dark 在浅色主题下浅底亮字对比崩坏（本轮根因之一）
  - **zed 系五主题语义校准**（Theme 增 dark 标记 + 语义阶梯不变量测试）：one-dark/nord-dark border 提亮到 bg 之上（原更暗 → 分隔线不可见）；one-light/ayu tool_bg 回 bg+3%（原=selected 过深）；nord-light border/hover/selected 加深可见；nord-dark selected 降饱和（6c99a6→4d5a72）；one-dark/nord-dark text 阶梯拉开（muted 过暗）；ayu accent_hover 加深（原反向变浅）
  - **mist 专属硬编码色全量语义化**：0xafc4baXX 系（20+ 处）→ border_alpha(t, a)、0xd8626a → danger_alpha/danger_hover、0x2e8b57 → UNREAD；设计意图保留：tooltip/toast 恒深底、终端 ANSI、thinking 金灯、行内 code 底
  - **sync_gpui_tokens 补 danger 系映射**（gpui-component 部件删除态随主题）
  - 测试 85 全绿（pi-link 47 + app 38：新增语义阶梯不变量/表格/图片/删除线/双主题高亮）

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

## 状态（2026-09-28）— check_arch 全绿，bead pi-flash-719 关闭

**架构重构 v4 收官（a1bfff0 + b68faca）**：check_arch 四项全 PASS——
单文件 ≤1500 ✓、全部视图函数 ≤300 ✓、无手搓字符输入 ✓、警告 0 + 测试
84 全绿 ✓。三阶段拆分：phase 1 超标视图（dialogs/general/subagents/
plugins/settings/models/main_column）→ phase 2 main.rs 2364→1418（7 个
根模块 actions_*）→ phase 3 function_panel sidebar（565 行）拆出
sidebar_lists + session_row_confirming + session_row_actions。

phase 3 顺手清零 54 条警告：删除死代码（export_html、AgentSession::spawn、
is_markdown/load_project_files_pub、cancel_rename、close_terminal〔与
close_panel_tab 重复〕、icon_theme、LANGS/lang_name、clear_scope、theme
DEFAULT/DARK/c()、fmt_hhmm/cwd_tail/top_level_entries、terminal cell_at、
TextInput::on_submit builder、runtime send_follow_up/abort〔Chat 自有〕、
FileTab.path/truncated、MenuItem.title/desc、ThemeEntry.family/source +
Family 枚举、op_seq、drop(&ref) 空操作）；拆分残留参数下划线化；
SessionEvent 的 RunningChanged/AgentFinished/FileBound 保留
`#[allow(dead_code)]`（会话池重构预接线，订阅臂已在 actions_runtime.rs）。

UI 实测（真机）：sessions 面板渲染/行点击打开会话/hover 改名删除按钮/
confirming 行内确认+取消 全部正常（tmp/shots/sessions_final.png 等）。

陷阱补充：python 脚本整文件重写会把 CRLF 库文件翻成 LF（i18n/
models_config/terminal 中招，已还原）——改文件前先探 `\r`，写回保持
原行尾；子串替换锚点要带行首 `\n` 防缩进子串误匹配；move 闭包捕获
`&WeakEntity` 参数会 E0521（'static 要求 owned），须在闭包外 clone。

## 待办

beads 任务跟踪已清空。后续工作 → 新建 beads（`bd create`）。
计划全量见 PORT_PLAN.md。下一主线：用户提出重新设计 UI；
会话池重构计划见 .zcode/plans/plan-sess_1c4e46f7（每会话常驻
SessionRuntime，pi-web 多开模型对齐）。

## v56（2026-10-02）消息渲染全面对齐 pi-web

epic pi-flash-48u（7 子任务全关，c1-c30 共 13 提交）。目标=session 渲染与
pi-web 完全对齐，唯一豁免=消息末尾操作栏（复制/编辑/分支 hover 栏自定义保留）。

- c1-c4 数据层：stopReason/errorMessage/Usage.cache_write 透传；Block::Image
  +ToolCall is_error/images/duration_s/details/args_partial/result_arrived；
  toolResult 按 toolCallId 关联合并（merge_tool_result 收敛三路）；
  sessions::renderable_message 把 compaction/custom_message/branch_summary
  映射为 role=custom（branch_summary 不映射 user，保 fork 锚点对齐）。
- c5-c7 轮结构：thinking+toolCall 全收「工作详情」组（splitFinalAssistantBlocks
  parity：最终回答=末条 assistant 尾部 text/image 连续段，前置块入组）；组默认
  折叠=有最终回答/error/length，流式中展开（collapsed 改 HashMap 显式覆盖）；
  相位行 pi-web 语义（agentRunning&&!hasStreamingContent，Running {tool} 三档
  文案，13px 脉冲）；时间戳静态 10px。
- c8-c11 工具卡：ToolCallBlock 卡片（状态色/摘要 120 键序/apply_patch 路径
  摘要/耗时/旋转箭头）+参数 pre（流式原始/完成 pretty）+结果 pre maxH400+
  （无输出）斜体+错误红字；diff.rs 解析器（unified+V4A+preview，带测试），
  SplitFilesView 双栏/PatchText 单栏；结果图片 b64 直渲 maxW720/maxH520。
- c12-c16 markdown：任务列表自绘复选框+GFM 裸 URL linkify（自研 split_links
  补 pulldown 缺口）+frontmatter 吞掉；代码块横向滚动（nowrap+overflow_x_scroll
  替换裁剪）；流式跳过 syntect 高亮与行号；100k 字符守卫。
- c18-c22 用户气泡：85%+蓝边框+圆角12+pad 8x12+走 markdown+图片 240+300px 内滚。
- c23-c25：流式估算 token+四档 t/s 徽章（0.5s 延迟）；written files chips
  （appliedFiles>preview>输入解析，剔 delete，点击 open_file_tab）；cache W。
- c26-c28：错误红框/截断黄框；compaction 卡片（parse_compaction_summary 剥
  尾部文件段）；custom 三态（hidden 暗卡 140 预览/branch_summary 斜体引言/
  generic 卡）。
- c29/c30 核实无改动：pi-web 工具结果亦原样渲染 ANSI；空块过滤语义一致。

已知偏差：KaTeX/mermaid/raw HTML 降级（GPUI 平台限制）；c17 syntect 主题校准
暂缓（可选）；branch_summary 渲为斜体引言（非 user 气泡）；compaction 文件
清单常显（pi-web <details> 默认折叠）；模型名每消息一条（Msg.model 三路解析）。
测试 97 全绿（app 45 + pi-link 52）。

## v57（2026-10-02）富渲染三组件：KaTeX / mermaid / raw HTML

epic pi-flash-w9w（3 子任务全关）。v56 已知偏差三项全部落地，代码独立存放于
crates/app/src/render/（html.rs / math.rs / mermaid.rs / mod.rs），markdown.rs
只留薄胶水（事件接线 + MdBlock 分支）。

- v57-1 raw HTML：scraper（html5ever）解析 Event::Html/InlineHtml（v56 前直接
  丢弃）→ 安全子集映射到 MdBlock/Run。行内 b/i/code/a/del/br→Run，块级
  pre（language-x）/img/标题/ul-ol-li/table/blockquote/hr/details（平铺）→块，
  script/style/iframe/svg 剥离，HTML5 容错+实体解码。8 测试。
- v57-2 KaTeX：RaTeX 0.1.14 管线（ratex-parser→ratex-layout→ratex-render，
  embed-fonts 内嵌 19 字体），pulldown ENABLE_MATH；多行 $$…$$ 的 DisplayMath
  实测发在 Paragraph 事件流内 → Style::Math/DisplayMath 标记 run，段落分段
  flex（文本段 StyledText 换行 + 行内公式 img + 块级公式整行图）；透明底 PNG
  2x 超采样、颜色=主题文字色；进程级 LRU（latex+display+color 哈希）防流式
  重渲；失败降级等宽文本。6 测试。
- v57-3 mermaid：mermaid-rs-renderer 0.3.1（纯 Rust，--no-default-features
  库模式）→ SVG → gpui img（usvg 系统字体，图内 text 可渲——源码已核）。
  深色 Theme::dark/浅色 mermaid_default；(源码+主题) LRU；流式期间与失败
  均回退源码块（pi-web MermaidBlock parity）。4 测试。

陷阱：rcdom 0.39 是不受维护的测试 DOM（README 明确警告）→ 换 scraper 0.27；
ego-tree 0.11 把 Node 私有化，公共类型=NodeRef<'a,T>（scraper 不 re-export，
需直接依赖 ego-tree 0.11 同版对齐）；bash heredoc 反斜杠转义会静默损坏
（ 变 formfeed 0x0C 进文件）——python 补丁一律 chr(92) 构造或写脚本文件。
测试 116 全绿（app 64 + pi-link 52）。

## v57 实测轮（2026-10-02 下午）真机验证 + 顺手修复

三组件真机验证全过：mermaid（复杂 subgraph 流程图 SVG 渲染成功）、KaTeX
（行内/块级公式印刷体，气泡与回复两侧）、raw HTML（b/i/del 真样式 ✓）。
测试期间揪出并修复 5 个问题：

- RPC 全断（7393014）：系统 pi 升 1.0 后 ~/.pi/agent 扩展 fatal 掉 0.87.1
  内核 → spawn 加 -ne；live_rpc_probe 探针入库。
- tools/thinking 菜单错位（b53d208）：写死窗口右下角 → pill_anchor 动态锚定。
- composer 粘贴缺失：key_down 无 Ctrl+V 分支（v54 起缺失）→ 补剪贴板追加。
- 粘贴即崩 0xc0000409（两连修）：caret 高亮 range 字节/字符错位（+1 切进
  3 字节光标字符中间，gpui str 切片 panic）→ range 精确覆盖全字符；CRLF
  归一。教训：gpui highlight range 永远按字节且必须落在字符边界。
- 长文本不折行：编辑区文本在 flex 行内溢出裁剪 → 块级全宽容器 + caret
  改追加着色字符（随折行）。
- 用户气泡 HTML 按原文显示（产品规则，用户拍板）：用户消息=发出内容凭证，
  气泡吞标签无法核对 agent 收到什么 → markdown 管线加 html 开关
  （render_user 字面路径）；顺带修 assistant 行内 HTML 配对标签跨事件
  样式丢失（fragment_effect StylePush/Pop/Runs 状态机）。

与 pi-web 的既定偏差新增：用户气泡不渲染 HTML（pi-web 渲染）。
测试 118 全绿（app 66 + pi-link 52）。

## vendor pi 0.87.1 → 1.0.0（2026-10-03）

背景：用户报「系统 pi 升 1.0 后应用启动找不到 model 列表」。诊断结论：

- vendored 0.87.1 的 RPC 链路实测**并未断**（live_rpc_probe 在仓库 cwd 与
  pi-web cwd 均返回 450+ 模型，~/.pi/agent 被 1.0 迁移后 0.87.1 也能读）；
  故障应为 1.0 迁移配置期间的瞬态，或来自系统 pi 的测试路径。
- npm 上 1.x 仅 1.0.0（=latest=用户系统版本），无版本选择问题。

升级动作（调用方式维持 RPC，不引入 SDK——RPC 是官方进程边界、可用
pi-link 测试钉住；SDK 需嵌 Node 宿主、耦合 pi 内部 API，已论证否决）：

- vendor/pi：package.json + lock 0.87.1→1.0.0，npm install；VERSION 同步；
  crates/pi-link PI_VENDOR_VERSION 同步。vendored 与系统 1.0.0 chunks 逐字节一致。
- 协议面核对：Command 枚举全部命令名（abort/compact/steer/follow_up/
  get_available_models/set_model/get_tree/…）逐一 grep 1.0 bundle 全部在；
  「toolcall」仅是 protocol.rs 防御别名，wire 上是 toolCall（camelCase，在）。
- 实机探针（1.0.0 + -ne）：get_state/get_available_models/get_commands 三连
  success；模型列表结构与 0.87 同形（data.models 数组），447 个解析成功。
- `-ne` 维持：钉版分发自包含原则不变，注释更新为隔离原则表述（不再依赖
  版本错位这个具体案例）。
- 测试 123 全绿（app 71 + pi-link 52，fixtures 为 0.87.1 真实报文，1.0.0
  下照样通过=wire 兼容）。待用户启动实测模型列表/会话/工具调用。

## RPC 线缆日志（2026-10-03，诊断「应用内模型列表空」）

1.0.0 升级后用户复测仍报模型列表空，但所有离线模拟全通：live probe
（repo/pi-web cwd）447/450 模型；手工 spawn vendored 1.0.0 + `-ne` +
`--session <0.87 写的会话文件>` + cwd=ghzw 项目——get_state 与
get_available_models 均 success（会话恢复模型=space-bunny-alpha，即用户
过滤框输入 "space" 的由来）。排除：协议面、vendor 版本、会话恢复、
项目 cwd、enabledModels 白名单（全局 settings.json 无该键→不过滤；
启动时 reload_settings_panel 已把 all_enabled 置 true）。

剩余盲区=应用进程内实际 spawn 的参数/响应/stderr（stderr 此前被
Stdio::null() 丢弃，pi fatal 完全不可见）。落地：pi-link client 支持
`PI_FLASH_RPC_LOG=<path>` 线缆日志（spawn 头+双向 JSONL+stderr+EOF，
父目录自动创建）；dev.sh 每次启动导出绝对路径 tmp/rpc-last.log 并截断。
待用户：完全退出旧实例→dev.sh 重启→复现→读日志定位。

## 模型列表弹窗优化（2026-10-03，六项）

- 宽度 520→620；列表行字号 12→14px（标签+上下文列+空态）；标题接 i18n
  （tr("选择模型")，表内已有条目）；过滤占位符入表（过滤模型...）。
- 统一关闭机制：dialogs.rs 新增 dialog_shell（遮罩 occlude + 点外关闭 +
  ESC + 居中 + 面板 stop_propagation），ModelSelect/GitDiff/SessionSearch
  三弹窗全走 shell（GitDiff 此前无点外关闭；SessionSearch 原实现并入）。
- 行点击=切换模型+关窗；键盘：↑/↓=ComposerUp/Down action 冒泡到 overlay
  （app 级 Input 上下文覆盖绑定，单行输入无 cursor-up handler 必冒泡），
  Enter=门面 on_submit；过滤变化重置选中；选中行常亮 bg_selected。
- Dialog::ModelSelect 加 sel 状态；Chat 增 filtered_models（渲染与键盘共
  用同一白名单+过滤逻辑）/move_model_sel/apply_model_sel（actions_rename.rs）。
- 测试 123 全绿零警告。待用户实测。

### 修复：Enter 选模型崩溃（同日）

键盘 ↑↓+Enter 崩 0xc0000409=记忆中的双重租约（事件派发期间同步 update/
drop 正被派发的实体）：on_submit/on_escape 回调在过滤输入（及其内部
InputState）被派发链租用期间同步 weak.update(Chat)，apply_model_sel 的
dialog=None 把正在派发的两个实体 drop 掉。修=两个回调内改走 cx.defer
（App::defer，派发周期结束后再执行；composer 事故同款解法）。on_change
只改 sel 不 drop 实体，维持同步。123 测试全绿零警告。

### 修复：Enter 选模型无反应（同日续）

上一轮 defer 修了崩溃但 Enter 仍不切模型。复盘链路：ESC 好使走的是门面
渲染 div 的 on_key_down 冒泡（text_input.rs:265），不经过 InputEvent 订
阅——所以「ESC 正常」证明不了订阅链路。Enter 原设计走 PressEnter 订阅
→on_submit→defer，实测未生效（具体断点未定位，疑似订阅/emit 时机）。
改法=Enter 接到已被验证的层：模型弹窗 overlay 上加 on_key_down("enter")
→stop_propagation+defer apply_model_sel（与 ESC/箭头同层；gpui 派发顺
序=action 先、key_down 后，InputState::enter 单行模式显式 propagate，
事件必达 overlay）。on_submit 路径保留（双触发被 dialog=None 早退守卫）。
apply_model_sel 加 debug eprintln 探针（sel/pick/session），dev.sh 控制台
可见；若再失效一轮定位。123 测试全绿零警告。

### 修复：模型切换滞后一拍（同日续，实测定位）

用户报：点击模型标签不更新、下次点击才换成上一个；误判标签短名/思考
off 为 bug（短名=预期行为用户确认；off=pi 真实状态，gpt-4.1 非推理模型，
pi-web 同样显示，切回推理模型自动恢复 high）。
实测（ghzw cwd 真 1.0 进程）：set_model 后立刻 get_state 回的是**旧模型**
（swap 异步，set_model 响应最后到）；set_model 响应 data 里带切换后的
完整模型对象；响应到达后 state 即新值，thinkingLevel 同步（gpt-4.1=off，
deepseek-flash 恢复 high=按模型记忆）。
修：select_model 去掉立刻 refresh_state（必拿旧值）；响应处理新增
set_model 臂=用响应 data 直接更新 state.model（pi-link 加 parse_model_info）
+ 此时再 refresh_state（swap 已落地，thinkingLevel 正确）。鼠标/键盘同
路径修复。check+123 测试绿；exe 被运行中应用锁定未链接，用户关应用后
dev.sh 自动重链。

### 弹窗遮罩：灰→应用底色柔雾（2026-10-03）

用户要求弹窗背景不用灰色、用底层界面模糊。实测确认 gpui 0.2.2 渲染层
（Windows DirectX/Metal）无逐元素 backdrop blur——着色器仅阴影高斯，无
离屏 pass；真模糊需给双平台渲染器各写 blur pass（大手术，暂不做）。
落地近似：三处遮罩（dialog_shell/ext_ui/settings）从 35% 黑改为应用
bg 色 80% 透明（rgba((t.bg<<8)|0xcc)）——底层界面以 20% 幽灵度透出，
观感为「界面退隐成同色柔雾」而非灰膜；真毛玻璃留作渲染器级后续项。

## agent 输出样式对齐 pi-web（2026-10-04，P1 五项 + P2 扫尾）

与 pi-web globals.css/MessageView/MermaidBlock 逐项比较后的对齐批次：

- **块间距统一 8px**（P1 最大观感项）：messages.rs 组内 item/最终回答/防御
  分支的块容器全部加 gap(8)，thinking 去掉 my_1（原 4px）、工具卡原 0px
  贴死——现在 text/thinking/工具卡间一律 8px（pi-web 块容器 gap:8 parity）。
  messages 间距 22→16、写文件 chips 文字色 text_muted→text、usage 行/两处
  时间戳/用时 text_faint→text_dim、工作详情折叠行 text_dim→text_muted。
- **markdown 块间距 = CSS margin 折叠**：render_blocks 按
  max(前块mb, 本块mt) 挂间距（此前逐块挂 margin 不折叠，段→代码 8+6=14px
  而 CSS 取 8，整体系统性偏大）；末块不再挂 mb = p:last-child 归零。
  各块 margin 值集中在 block_margins()（heading 10/5、p 0/8、code/quote
  6/6、列表 5/8、table/img 8/8、rule 12/12）。
- **strong 700**：FontWeight SEMIBOLD(600)→BOLD(700)（pi-web strong:700；
  标题/工具名/th 表头维持 600/650 对应 SEMIBOLD）。链接下划线补 45% 透明色
  （offset gpui 无对应）。
- **行内 code 等宽盒**：gpui highlight 换不了字体家族——含 Code run 的段落
  拆 flex-wrap 段（paragraph_element，与 math 拆段同路），code = JetBrains
  Mono 0.92em + bg-subtle + 圆角5 + padding 1/5 + 70% 边框（pi-web
  .markdown-inline-code）；表格/标题内的 code 仍是 bg 兜底高亮。
- **混色真正落到 run**：base_style 增加 color 参数（StyledText 自带 base
  style 会覆盖容器 text_color）——引用块正文现在真是 text_muted、h3 真是
  88% 混色；标题色按 pi-web 规则恒 text（引用块内也是），经参数传入。
- **JetBrains Mono 打包**：assets/fonts 三档字重（Regular/SemiBold/Bold +
  OFL.txt）include_bytes 进二进制，main.rs text_system().add_fonts 注册；
  MONO_FAMILY 常量改 "JetBrains Mono"，全仓库 41 处 .font_family("Consolas")
  统一切换（终端 FONT_FAMILY 与字体选择列表保留 Consolas；选择列表加
  JetBrains Mono 项）。
- **深色高亮 = VS Dark+**：syntect 默认主题集无 vscDarkPlus，按 dark_plus
  调色板用代码构建 Theme（22 条选择器：keyword 蓝/control 紫/string 橙/
  fn 黄/type 青/comment 绿…），替换 base16-ocean.dark；浅色 InspiredGitHub
  不变。
- **其它 pi-web parity**：表格字号 14→13px、行高 1.6→1.7（th 色也走参数）；
  任务框选中态 = accent 10% 淡底 + 55% 边框 + accent 对勾 + 圆角4（原实心
  accent 白勾）；hr = 底线 + 两端 18% 渐变遮罩复现 linear-gradient 淡出
  （linear_color_stop）；代码块补 box-shadow 0 1px 0 border 42%；
  thinking 块补右侧时长 Ns（快照按消息首尾时间差，流式中无）。
- **消息列默认边距 10px**：session_list pl34/pr30 → px(10)（920 列宽不动，
  宽窗居中、窄窗不贴边）；composer 去掉 min_w 500（窄窗溢出另一来源）。
- **已知未做**：代码块复制按钮无"已复制"反馈（读全局需要渲染期 cx，gpui
  渲染函数拿不到，defer）；工具参数 pre 的 break-all（gpui 按词折行，
  超长 token 可能溢出）；链接下划线 offset（无 API）。

测试 126 全绿零警告（app 73 + pi-link 53；新增 render 冒烟测试覆盖明暗两
主题全路径 incl. vs_dark_plus）。待用户实测。

### 补丁：marker 对齐与尺寸（同日，用户实测反馈）

- 列表 marker 槽 items_center→items_start + 行盒 1.7：pi-web outside
  marker 与内容第一行对齐，原实现在多行 item 上圆点垂直居中漂到中部。
- 无序圆点弃用 "•" 字形（14px 下 ~4px 过小），画 0.45em 实心圆
  （Chrome disc 尺寸），随 markdown 字号槽缩放；任务框 mt5 =
  pi-web checkbox top:0.35em 首行定位。
- compaction「文件上下文」三角：10px text_faint "▾/▸" 字形小到不可见，
  换 12px text_muted chevron 图标（与工作详情折叠行同款）。

### 补丁：消息列两侧 10px 边距真正生效（同日，用户二次反馈）

根因在 gpui List 实现（vendor/gpui/src/elements/list.rs prepaint_items）：
`item_origin = bounds.origin + (0, padding.top)`——**只应用垂直 padding，
左右 padding 对条目完全无效**，条目恒从列表 bounds 左缘画起。此前
pl(34)/pr(30) 与改后的 px(10) 挂在 list() 上均从未生效（垂直的 pt/pb
一直正常，掩盖了问题）。修：list 包进 `.px(px(10.))` 外层 flex_col 容器，
920 列宽与居中不动。全仓库仅此一处 list()，无同类隐患。

### 左侧栏最小宽度 300（同日，用户要求）

slp_w 四处对齐：拖拽 clamp(250,500)→clamp(300,500)、双击复位 282→300、
workspace.rs UiState 默认值 282→300、持久化加载 clamp 同步 300（旧存档
282 载入时抬到 300）。

### 设置-其他新增「展示思考」开关（同日，用户要求）

AppSettings.show_thinking（默认 false=不展示）+ 加载/保存 + show_thinking()
getter；其他页新增 set_row + 34×19 switch（展示思考 / 在消息中显示模型的
思考块（开启时默认收起））。消息层双处生效：render_block Thinking 臂先
门控（关=整块不渲染）、默认态 unwrap_or(true)→false（开启时默认收起，
用户展开后记忆在该块）；block_displayable 同步门控，纯思考消息在「工作
详情」组不再渲染光杆模型标签。存储键 show_thinking。

### 回到最新按钮 + 发送即滚到最新（同日，用户要求；pi-web parity）

- `SessionRuntime.list_at_bottom: Rc<Cell<bool>>`——list 滚动不通知实体，
  `set_scroll_handler` 里按 `ListScrollEvent.visible_range.end >= count`
  判定贴底，翻转时 `window.refresh()` 重绘按钮显隐。
- 悬浮按钮（session/mod.rs `scroll_to_bottom_button`）：32px 圆钮、
  border/bg_panel、常态 opacity .28 hover 全亮、0 2px 8px 阴影，absolute
  bottom 96 居中于 composer 上方；点击 `scroll_to_latest()`。新增
  assets/icons/arrow-down.svg（arrow-up 翻转版）并入 assets! 清单。
- `scroll_to_latest()` = `list.scroll_to_reveal_item(末条)` + 置位贴底；
  两个调用点：send_input 乐观气泡 push 后（发送即滚）、live ingest 的
  user 臂（steer/回显消息到达即滚）。贴底时 ListAlignment::Bottom 天然
  跟随新消息，无需额外锚定。

### 修正：「展示思考」语义（同日，用户纠错）

上一版把开关做成了「关=整块不渲染」——错。正确语义：思考块**始终渲染**，
开关只控制新思考块的默认展开态：关（默认）=收成一行（灯泡+单行预览，
点击可展开）；开=默认展开全文。`unwrap_or(show_thinking())`，撤销
block_displayable 的 thinking 门控（纯思考消息照常计入工作详情组），
设置描述更正。用户手动开合过的块仍以显式状态为准。

### 修正：「回到最新」按钮位置（同日，用户纠错）

原实现 absolute bottom(96) 固定定位——输入面板（胶囊）增高（多行/带图）
后按钮叠进面板。改为把按钮挂进 composer-wrap 的悬浮容器：容器改
flex_col + items_center + gap(20)，按钮（若有）叠在胶囊正上方 20px，
随面板高度自动上移，永不叠进面板；builder 去 absolute 包装改 pub(crate)，
main_column 旧挂载点移除。

### 修正：发送即「清屏」（同日，用户纠错，pi-web scrollUserMsgToTop 语义）

此前用 `scroll_to_reveal_item`（最小滚动，新消息落在视口底部）=错。
pi-web 的 `scrollUserMsgToTop` 是把最后一条用户消息滚到视口**顶部**
（elAbsTop-16，clamp 到 maxScrollTop）。pi-flash 对应实现
`clear_to_sent_message()`：`list.scroll_to(ListOffset{item_ix: 末条,
offset_in_item: 0})` —— vendored gpui 的 scroll_to 直接设置
logical_scroll_top、不经过 wheel 路径的 scroll_max 钳制，末条消息也能
钉在第一行，历史全部滚出屏幕上方，下方留白等回复；list_at_bottom 置
false（↓ 按钮出现，响应流入后由此跟进）。两个发送点（send_input 乐观
气泡、live ingest 回显）都走清屏；↓ 按钮单独走 `scroll_to_bottom()`
（reveal 末条 + 贴底标记）。

### 修正：清屏被 `ListState::reset()` 抹掉（同日，用户实测截图定位）

三处时序问题叠加导致发送后消息仍在底部：
1. gpui `ListState::reset()` 会把 `logical_scroll_top` 置 None（回贴底），
   `notify_list` 内部就是 reset——send_input 里 clear 在 notify 之后执行，
   钉顶当场被抹。修：先 notify 再 clear。
2. 活回显 `Event::MessageStart`(user) 走 `on_event` 尾部的 notify_list，
   同样抹掉乐观路径的钉顶。修：on_event 加 `user_arrived` 局部标志，
   尾部 notify 后重钉（覆盖回显升级与 steer 回显推入两分支）。
3. `ingest_message` 是 get_messages 快照重建路径（打开会话/刷新循环），
   撤销其中的清屏——打开会话保持贴底。

### 修正：清屏钉顶在流式期间保不住（同日，用户实测截图定位）

钉顶后，流式增量 / phase 行切换 / 状态事件每次都触发 `notify_list` →
`reset()`（logical_scroll_top=None 回贴底），视口立刻被拽回底部——截图
表现为新消息停在半屏。修：`notify_list` 在 reset 前保存
`logical_scroll_top()`，当 `list_at_bottom == false`（非贴底）时
`scroll_to(prev)` 恢复原位；贴底时维持 None 继续跟随新内容。此后清屏
钉顶、用户上翻位置都能在整轮流式期间存活。

### 滚屏整体重写：pi-web useAgentSession 全机制移植（同日，用户要求）

通读 pi-web hooks/useAgentSession.ts + lib/chat-lazy-load.ts 后推倒重写，
替换此前的三轮补丁：

pi-web 机制 → pi-flash 移植：
- `promptAnchorActive` + spacer（钉顶=贴底的关键：spacer 使 scrollHeight
  = 用户消息顶+视口高，scrollToBottom 与钉顶位重合）→ `prompt_anchor:
  Rc<Cell<Option<usize>>>`（锚定用户消息的列表条目号）；gpui 无 DOM 布局
  后量测，钉顶直接用 `scroll_to(ListOffset{ix, 0})` 达到同一位置语义。
- `isNearBottomRef` + rAF 跟随循环（"scrolled up, leave them there"）→
  gpui Bottom 对齐原生：logical=None=跟随、Some=手动；`notify_list` 单点
  状态机：跟随哨兵（`logical_scroll_top().item_ix == item_count`，Bottom
  默认值物化）/manual reset 后恢复/锚点重钉。
- spacer 归零后 scrollToBottom 跟随尾部 → 锚点条目到末条内容（含 phase
  行，`content_below` 用上轮布局 bounds）≥ 视口高时锚点退役，切回跟随；
  边界处两位置重合，无缝。
- 用户在锚点中上翻 → 检测 prev 偏离钉位，锚点让位 manual。
- 按钮显隐 `shouldShowScrollToLatest` → scroll handler
  `at_bottom = 锚点激活 || !is_scrolled`（锚点期钉顶即贴底，不显按钮）。
- `!agentRunning → setPromptAnchorActive(false) + scrollToBottom` →
  AgentSettled/AgentEnd 清锚点回尾部跟随（展示回复结尾）；发送失败、
  快照重建（messages.clear）同样清锚点。
- 打开会话 scrollToBottom("instant") → reset 默认跟随。✓（原有）

126 测试全绿零警告。

### 钉顶失效的真根因与 spacer 完整移植（同日）

通读 vendor/gpui list.rs layout_items 全文，定位铁律：**滚动位到列表末尾
的内容填不满视口 → Bottom 对齐强制 logical=None（贴底跟随）**——"能看到
末尾=在底部"。发送后锚下只有等待行，必然填不满，所以任何 scroll_to 钉顶
都会在下一轮布局被抹（此前三轮失败的共同根因）。

这正是 pi-web `PromptAnchorSpacer`（getPromptAnchorSpacerHeight）存在的
原因——之前只移植了锚点概念没移植 spacer，等于没抄完。完整移植：

- 列表条目结构恒为 [msgs | phase 行 | spacer]；spacer 是真实列表条目，
  高度 = 视口 − 锚下内容（bounds 上一轮布局，滞后一帧≈pi-web rAF），
  把「跟随位」精确垫到用户消息顶——列表全程保持原生跟随（None），
  钉顶不与元素对抗；内容长过视口后 spacer 归零，跟随自然滑向尾部。
- 弃每帧 reset()（全量重测 + Bottom 归 None 的元凶），改外科手术式
  splice：listed_msgs/listed_phase/listed_spacer 三段跟踪，消息插入
  phase/spacer 之前；仅整体重排（快照重建）才 reset（恰好要贴底）。
- 渲染闭包 None 臂按 [phase?|spacer?] 分解渲染 spacer 条目；
  scroll_to_bottom reveal 末条（含 spacer，锚点期=回钉顶位，pi-web
  scrollToBottom 落 spacer 底同款）。

## 状态（2026-10-04）— v59 滚屏算法抽离 `session/chat_list.rs` + 发送帧翻页修正

用户裁定「每次发言必须翻页：历史滚出屏、最新发言钉视口顶」，且原实现全是错的。
先定位真根因，再整体重构（bead pi-flash-73k）：

### 原钉顶失效的直接原因（在 spacer 移植版之上）

`notify_list` 的垫片高度 = 视口 − 锚下内容，取自**上一帧** `bounds_for_item`；
发送帧刚 splice 的用户消息是 Unmeasured → `content_below` 返回 None →
`spacer_ready=false` → 垫片不挂载 → Bottom 铁律（list.rs 674-716 填不满视口
强制 logical=None 贴底）生效 → 气泡停屏底。发送路径从未 `scroll_to`，修正
只能等下一个 pi 事件，甚至整轮不落位。

### 新架构：`crates/app/src/session/chat_list.rs`（滚屏唯一归属）

- `ChatList` 收拢原 SessionRuntime 六个散字段（list/list_at_bottom/
  prompt_anchor/prompt_spacer_px/listed_*）→ `pager: ChatList` 单字段；
  gpui ListState + 锚点 + 垫片 + splice 记账 + scroll handler 全内建
- 函数级 API：`page_turn`（翻页）/ `sync`（结构手术，内部 settle_spacer/
  splice 编排）/ `release`（锚点退役）/ `reload`（会话切换 reset+记账）/
  `jump_to_bottom` / `reveal` / `is_at_bottom` / `anchor_active` /
  `spacer_px` / `scroll_top_ix`；7 个记账单测（ListState splice/reset 可
  无窗口跑）
- runtime 侧 `notify_list` 缩成薄封装（pager.sync + cx.notify）

### 发送帧翻页算法（核心修正）

1. 发送帧：锚定新消息，垫片按视口高**过估**挂载（新消息未测量，精确高度
   算不出）→ `scroll_to(anchor_ix, 0)` 硬置顶——逻辑位 Some 直接绕开
   Bottom 贴底铁律，**不等回显、不等 bounds**
2. 下一帧（回显/任意事件）：消息已测量 → `settle_spacer` 收缩垫片到精确
   高度（视口 − 锚下内容），逻辑位交还贴底胶水（scroll_to(count) ≡ None，
   同一几何同帧落地无跳变）；此后流式增长由胶水自然下滑（spacer 退役同款）
3. 同锚点重入（回显升级）不重钉不重估；滚轮 is_scrolled 即退役锚点并就地
   卸垫片（尊重用户滚动，按钮出现）；AgentEnd/Settled、快照重建、发送失败
   → release；steer 回显同样翻页

外部三处裸 `list.reset`（actions_sessions/actions_panels/main 恢复会话）
改 `pager.reload`（reset + 记账同步，消除记账失配隐患）。测试 87 全绿
（app 80，其中 chat_list 7）。

### v59 首轮验收修正（同日，用户实测三症状全根因定位）

- **崩溃**：卸垫片的 splice 原放在 scroll handler 里——gpui `scroll()` 持有
  ListState 的 RefCell 可变借用期间回调 handler，再 splice 即 BorrowMutError。
  铁律：滚动回调里严禁触碰 ListState。退役改为只置 `pending_release` 标记，
  渲染层每帧经 `take_frame_sync()` 补一次 sync（结算尝试带 12 次上限防病态循环）
- **白屏**：过估垫片（整视口高）在未结算窗口内一旦落到贴底胶水 = 整屏只剩
  垫片。sync 尾部加守卫：锚点未结算期间任何胶水位弹回 `scroll_to(锚, 0)`；
  结算成功后交还胶水（同一几何）。相位脉冲动画每帧驱动渲染 → 结算在发送后
  ~2 帧内完成，窗口可忽略
- **steer 发送不翻页**：composer 回车在 agent_running 时走 steer_input，原只
  等回显。抽出 `optimistic_send()`（pending_echo + 乐观气泡 + phase_waiting +
  page_turn），prompt/steer 共用，发送帧即上屏翻页
- app 测试 82 全绿（chat_list 9：+滚动延迟卸载、+帧补账上限）

## 状态（2026-10-04）— v59 滚屏三修：钉顶几何真根因（内边距）+ 端点几何锁

用户口径不变：**每次发言，用户消息刷新到屏幕顶部，把整屏留给 agent 回复**
（pi-web 为示意图）。首轮（上面那节）虽修了崩溃/白屏窗口/steer 翻页，实测仍不对。

### 真根因一：垫片公式漏减列表上下内边距（157px）

`settle_spacer` 算的是 `spacer = 视口高 − content_below`，但 gpui 贴底胶水
（Bottom 对齐、`logical_scroll_top = None`）把**末条底边钉在
`viewport.bottom − padding.bottom`**，而聊天列元素自带 `.pt(22)/.pb(135)`：

```
painted(anchor).top = viewport.bottom − padding.bottom − spacer − content_below
⇒ 要让锚点顶落在 viewport.top + padding.top：spacer = (视口高 − pt − pb) − content_below
```

少减 157px 的后果正好是首轮那个「白屏」：锚点停在 `viewport.top − 135`（消息
整条滚出屏顶），可见区只剩那片空白 spacer 自己。首轮把白屏只当成「未结算窗口」
的过估问题，其实结算后的公式本身也是错的——这才是白屏的真正归宿。

### 真根因二：垫片只结算一次，之后流式增长全靠一帧前的旧高度

`content_below` 原用 `bounds_for_item`，该 API 开头就是「条目索引 < 逻辑滚动位
→ 返回 None」，而胶水态逻辑滚动位恒为 `item_count`（锚点在上方）→ 结算只能
成功一次；此后锚点被冻结的垫片吊着，内容越长锚点越往屏顶外漂（实测症状：
发完消息锚点立刻消失/下方留一大片空白）。

修法：vendor/gpui `ListState::measured_height_in(range)`（区间高度和，未测量
条目按 0 计 = 下界），胶水态下每帧可重算；下界偏小 ⇒ 垫片偏大 ⇒ 钉顶更稳，
且「下界已填满内容区」足以判定转跟随。

### 真根因三：轮末退役锚点 = 立刻撤销钉顶

`AgentEnd`/`AgentSettled` 原来调 `pager.release()`（就地卸垫片）。gpui Bottom
对齐下内容短于视口时「贴底胶水」会把内容拽到**屏底** → 短回复一结束消息就从
屏顶跳到屏底。pi-web 只把 `promptAnchorActive` 置 false（垫片收敛），容器
scrollTop 保持：短回复留在屏顶、长回复由胶水跟尾。现锚点只由用户滚轮 / 发送
失败 / 快照重建 / 外部整表重读 / 会话切换退役。

### 配套

- `chat_list.rs`：`spacer_target(avail, content_below) -> (px, follow)`；垫片
  高度扣 `PAD_TOP`/`PAD_BOTTOM`（与 `session::session_list` 的 `.pt()/.pb()`
  同源常量，改一处即改滚屏数学）；sync 第 6 步「内容未长过内容区 → 逻辑位硬
  停锚点顶」；结算前守卫「内容条目必须已进树」（sync 先结算后 splice，发送帧
  区间求和越界会把不存在的区间算成别的区间 → 偏小垫片 → 钉顶被铁律顶掉）
- `jump_to_bottom` 改 `scroll_to(item_count)`（交还胶水 = 继续跟随，pi-web
  scrollToBottom + isNearBottom 同款），不再是一次性 reveal
- 测试：`scripts/test_gpui_glue.sh`（vendor/gpui 不是 workspace 成员——它的
  examples 进 members 会编不过；脚本临时挂成员只跑 `--lib`，trap 还原）
  - gpui 侧：`test_bottom_glue_pins_last_item_above_bottom_padding`（canvas 抓
    真实绘制 bounds：正确公式锚点顶 = padding.top、垫片底 = 屏底 − pb；旧公式
    复现「可见区只剩垫片」）、`test_measured_height_in_counts_only_measured_items`
  - app 侧 `chat_list::glue_geometry`（真 gpui 布局 + 真 ChatList，模拟发送帧 →
    结算 → 回复逐帧增长 → 长过内容区跟随 → 滚轮退役；断言用的也是真实绘制位置）
  - app 84 测试、pi-link 53 测试全绿；`cargo test` 未新增警告
- 待用户肉眼验收（bead `pi-flash-fkh`）

### 已知行为（不是 bug，勿再改）

「回复只到屏幕中央 + 下方一大片空白」= 钉顶态本身：用户消息钉在内容区顶、
回复在其下方，剩余空间由 spacer（**真实列表条目**，不是 padding）填满。

「一滑就掉到屏底、下方空白消失」= 滚轮退役锚点（pi-web 同款）：

- 退役 → spacer 从内容里摘掉 → 内容高度 < 视口高 → gpui Bottom 对齐铁律把
  末条底边拉回屏底 → 整块内容下落
- 落下后没有滚动余量（内容比视口短 ⇒ 最大滚动位就是尾部位），滑不回去；
  重回钉顶只有再发一条消息（pi-web 的 `promptAnchorActive` 也只由发送置位）
- 长回复不受影响（内容高于视口时胶水没接管，滚到哪停在哪儿）

用户实测确认与 pi-web 一致，故保留；若日后要「滚动不塌空白」，得让滚轮只退役
**钉顶**而保留垫片——那会偏离 pi-web，属产品决策不是修 bug。

### 追加修复：消息区「流式中」也用了过期快照（思考框/徽章不显示）

用户对比 pi-web 截图指出：pi-flash 流式期间既不显示模型行的 `↓token 估算 + t/s
徽章`，也看不到思考框（thinking 条）。

根因同一类：`session/mod.rs` 的消息渲染闭包用
`rt_view.state.as_ref().is_some_and(|s| s.is_streaming)` 判「工作中」，而
`get_state` 快照在一轮内没人重拉 → 恒 false（composer 当初正因此改用事件驱动的
`agent_running`，消息区漏改）。后果两连：

- `stream_est = None` → `is_working = false` → 模型行只渲染模型名，↓token 与
  t/s 徽章永不出场（pi-web 这两项是 `isStreaming && est > 0` 才渲染）
- 「工作详情」组按 `default_open = is_working || !has_final_answer` 折叠 →
  思考/工具块全被折起来，看不见思考框

修法（4 处同类漏改一起收）：

- `session/mod.rs` 消息区 `streaming = rt_view.agent_running`
- `session_hero` 空态判据改 `agent_running`
- `runtime.rs` `send_input` 的 steer/prompt 分流、`fork_from_entry` 的
  「运行中禁止 fork」守卫都改成 `agent_running || 快照 is_streaming`

`actions_runtime.rs` 里本来就是 OR 关系，无需改。app 84 测试全绿，无警告。

### 追加修复：流式期间平铺渲染（pi-web `isLiveTail`）+ 垫片帧自检

用户对照 pi-web 列出五步：①等待模型应答 ②模型思考（出计速徽章）③模型工作
④模型输出 ⑤折叠思考/工作进「处理详情」。pi-flash 却是②就直接给出折叠的
「工作详情」，模型还在干活，接着翻页乱掉。

根因一（渲染结构）：pi-web `ChatWindow` 有 `isLiveTail = (sessionBusy ||
isStreaming) && endIdx === messages.length && userIdx === lastAnchorIdx`——
**运行中的这一轮直接平铺渲染**（每条 assistant 一个模型名行，thinking/toolCall/
text 就地展开），`ProcessDetailsGroup` 折叠行与最终回答分区只在轮末整形时才生成
（`defaultExpanded={!finalAnswerMessage}`）。pi-flash 把「分组折叠」过早套上：
思考/工具被折起来（看不见思考框），内容高度忽大忽小。

修法：`render_assistant_turn` 里 `is_working` 时直接 `return col.child(
live_turn_body(...))`——新增 `live_turn_body()` 平铺渲染（每条消息：模型名行 +
全部块；流式那条的模型名行带 ↓token 估算 + t/s 徽章）。usage 行 / 复制栏照
pi-web `MessageView` 的 `!isStreaming` 规则留给轮末；写文件 chips 同理。顺带把
模型名行抽成 `model_label_div(label, est, tps, t)`（徽章四档配色同 pi-web）。

根因二（翻页乱）：内容**形状**一变（轮末折叠、手动展开收合、chips/usage 行出现），
垫片假设就过期，而贴底胶水会把锚点摆到错误位置（内容变短 → 锚点坠到屏幕中段）。
旧实现只在「未结算」时补账，形状变化只能等下一条 pi 事件才自愈。

修法：`ChatList::take_frame_sync` 增加第三条触发——每帧自检
`spacer_lagging()`：实测锚下高度推出的目标垫片与当前值差 > 0.5px 就补一次 sync
（末条内容条目用「条目数 − 垫片」推得，无需 msgs/phase 拆分）。顺带消掉流式
增长的垫片一帧滞后；静止时自检安静（不空转）。

测试：`chat_list::glue_geometry` 加 `shape_change_triggers_frame_sync`（整形后
帧自检必须补账、锚点回到 padding.top，静止后不再请求）；app 85 测试、pi-link 53
全绿；真机启动无 panic。

### v59 滚屏四修：垫片改成常数（钉顶与测量精度解耦）——用户截图错位状态的真正机制

用户实测（截图四）：agent 内容尾巴贴在屏顶、下方一整片空白，用户消息整个不见——
「每次 agent 消息都顶到顶部去」，而需求是**只有用户消息刷到顶部**。

真机制（前三次修都没打中的那层）：钉顶靠「逻辑位 = 锚点」+ 精算垫片撑着，而
gpui `layout_items` 在 `pt + below + spacer + pb < H`（锚下填不满视口）时会
**丢掉逻辑位、强制改成贴底胶水**（`logical_scroll_top = None`），胶水定位用的是
列表里**缓存的条目高度**。内容在两帧之间变矮（轮末把思考/工具折进「工作详情」、
等待行/思考块收起、下一条消息开始；尤其**模型派发后的静默期没有任何 pi 事件**，
没人补账）时：

1. 精算垫片（= 内容区高 − 锚下内容）相对偏大 → 覆盖条件成立
2. 逻辑位被夺走 → 胶水按偏大的旧垫片算出更靠上的起点
3. **锚点条目不在绘制范围里**（第一段绘制从更靠上的条目开始）→ 用户消息消失
4. 帧自检读的是同一份缓存高度 → 认为垫片没问题 → 不补账 → 静默期一直停在错位状态

修法（结构性）：

- `spacer_target` 只给两个取值：**钉顶期 = 一整屏内容区高（常数）**、跟尾期 = 0。
  这样一来覆盖条件 `pt + below + spacer + pb ≥ H` 恒成立（below ≥ 0）⇒ 逻辑位
  永远不被夺走 ⇒ 锚点位置与测量精度、条目缓存新鲜度**彻底解耦**。垫片偏大只是
  屏下空白多一点（看不见）。
- `settle_spacer` 只做一件事：定「钉顶 or 跟尾」（`below ≥ 内容区高` 即交还胶水，
  只在翻过去那次 `scroll_to(count)`；翻回来由 sync 第 6 步重新钉顶）。夹紧末条
  内容条目为「已存在的最后一条内容条目」——结算发生在 splice 之前，否则会把垫片
  自己算进锚下内容（凭空多一整屏 → 误判跟尾，单测抓到过）。
- 帧自检（`take_frame_sync` 第 3 条触发）改为**只在钉顶/跟尾判定翻转时**请求补账
  （形状变化不经过 pi 事件时用），静止时完全不空转。
- `jump_to_bottom`（回到最新）：钉顶期就是 `scroll_to(锚点, 0)`；只有锚点已退役
  才交还胶水——**别在垫片还是正数时用胶水**，那正是把内容末尾推到屏顶的错位状态。

测试：新增 `glue_geometry::shape_shrink_without_event_keeps_pin`（内容变矮且无
pi 事件时锚点必须纹丝不动；长过内容区后判定翻转补账跟尾）；`spacer_target_is_a
_constant_while_pinned`；app 86 测试 + pi-link 53 全绿，真机启动无 panic。

### v59 滚屏五修：快流下「钉顶/跟尾」判定每事件翻转（= 永不自动上滚）+ 下边距改 150

用户实测：agent 输出一路顶出屏幕、**到输出结束都不自动上滚**；要求「输出距
input panel 上沿 20px 就该开始滚」。

根因（快流下的判定抖动）：`sync` 第 5 步「外科重测尾部」的区间是
`msgs-1 .. target`——**单条回复的回合里，`msgs-1` 正是承载整轮内容的「轮首
条目」**。它被打回 `Unmeasured` 后，同一帧里第二个 pi 事件结算时 ListState 按 0
计它的高度 ⇒ `below` 只剩锚点高度 ⇒ 判定翻回「钉顶」⇒ sync 第 6 步重新钉顶、
垫片弹回一整屏。230 tok/s 时一帧 2~4 个 delta，于是判定**每个事件翻一次**，
表现就是钉顶压着不动、内容一直长出屏幕（用户看到的「不自动上滚」）。

修法：第 5 步避开「轮首条目」（`from = max(msgs-1, 锚点+2)`）——它本来就每帧
被布局重测（钉顶期在首屏内、跟尾期在胶水 walk-up 里都会被渲染），不需要也不该
被 splice 打回未测量。

另外按用户口径把下边距 135 → **150**：内容区高 = 视口高 − 22 − 150，跟尾时内容
末条停在「胶囊上沿 + 20px」（胶囊高 ~110 + 底距 20）。「回到最新」按钮本来就悬浮
在胶囊上方 20px 处，现在与内容末条正好落在同一条线上。

回归锁：`glue_geometry::burst_events_do_not_flip_follow_decision`（一帧内两个
事件：判定必须保持跟尾、末尾贴屏底；改动前该测试失败，正好复现用户现象）。
app 87 测试 + pi-link 53 全绿，真机启动无 panic。

### v59 滚屏六修：换工具预设导致消息回落 + 审计 composer 各控件

用户实测：发送后等待 agent 响应期间改「工具选项」，「等待模型响应」连同刚上翻的
历史一起**掉回屏底**。

根因：工具预设是 **spawn 参数**（`--tools` / `--no-tools`，见 `SessionRuntime::spawn`），
换它要 `mc_set_tools_preset` 重绑会话进程（kill + 新开）并 `GetMessages` 整表重读；
而整表重读的处理里无条件 `pager.release()` —— 锚点退役、垫片卸掉，Bottom 对齐
随即把「等待模型响应」这种短内容整块拽到屏底。运行中重绑还会**直接掐掉正在跑
的这一轮**。

修法两层：

1. **禁止**：agent 跑动期间工具预设置灰不可点（点击给一句提示），
   `mc_set_tools_preset` 里也加 `agent_running` 守卫（绕过 UI 的兜底）。
   思考强度/模型**不用禁**——它们是实时 RPC（`SetThinkingLevel` / `SetModel`），
   不动会话进程也不重读消息表（已逐条核对代码）。
2. **兜底**：整表重读（换绑的 `get_messages`、压缩后重拉、会话文件重读）后调
   `ChatList::reanchor`——只挪锚点索引、保留垫片与跟尾状态、重钉一次；锚点确实
   失效时按「最后一条用户消息」重锚，找不到才退役。fork（换会话）仍显式退役。

composer 控件审计（是否重绑进程 / 重读消息表）：

| 控件 | 行为 | 结论 |
|---|---|---|
| 工具预设 | spawn 参数 → 重绑 + GetMessages | **运行中禁止**（唯一一个） |
| 思考强度 | 实时 RPC SetThinkingLevel | 安全，不禁 |
| 模型 | 实时 RPC SetModel + get_state | 安全，不禁 |
| 压缩（用量详情里） | 中止当前轮 + GetMessages | 早已锁死（can_compact） |
| 图片附件 / / 菜单 / @ / 声音 / 用量详情 | 纯本地 | 安全 |
| 发送 / 停止 / 排队 / 引导 | prompt/steer/follow_up | 正常（steer 本就要重钉） |
| 消息 hover「新分支」 | fork → 重绑 + GetMessages | 早已有「运行中禁止 fork」守卫 |

顺带修了 i18n 表里 8 条 en/zh-TW 顺序写反的条目（都是压缩卡片/写文件那组），
新增一条「运行中不能更换工具预设」三语文案。

测试：`glue_geometry::reload_keeps_pin_or_falls_to_bottom`（整表重读后钉顶纹丝
不动；反例退役锚点则回落到屏下半部）。app 88 + pi-link 53 全绿。

## 思考强度菜单补全 pi 1.0 全 7 档（2026-10-04）

pi 1.0 的合法思考档位为 `off / minimal / low / medium / high / xhigh / max`
（`VALID_THINKING_LEVELS`，vendor 1.0.0 已含；模型 schema `thinkingLevelMap`
按 7 键映射到 provider 端参数，未映射档有内建回退，如
`thinkingLevelMap?.[level] ?? "medium"`，透传安全）。composer 思考菜单原来只给
4 项（auto/low/high/max），现补全为 auto + 7 档共 8 行，文案与 pi-web zh-CN
逐字对齐（关闭推理/最低限度推理/中等强度推理/超高强度推理/最高强度推理，
`max` 由「最强推理」改「最高强度推理」），en/zh-TW 同步补 4 条、改 1 条。
runtime `set_thinking_level` 本就字符串透传（auto→None），pill 标签显示 pi
回报的原始档位串，均无需改动。pi-web 无菜单星标（★），截图中的星标非 pi-web
功能，不复刻。npm 上 pi 最新 1.0.2（vendor 钉 1.0.0），与档位无关，暂不 bump。

## 模型目录上收 Chat 层项目级共享 + 草稿 lazy connect（2026-10-04）

**bug**：已有会话能切换模型，新开对话模型选择器空白。根因是架构问题：模型列表
`available_models` 存在每个 `SessionRuntime` 上，只有活跃会话的 RPC 响应能填，
而「新会话」是 lazy draft（无 pi 进程），`refresh_state` 的 `if let Some(session)`
短路 → 列表永远为空；pill 兜底补拉同样被短路。且 new_session 注释承诺的
「首条 prompt 时 spawn」从未实现——草稿发消息会直接「未连接」。

**pi-web parity**（读了源码）：`/api/models` 由服务端配置直接枚举、按 cwd 缓存
（`lib/models-cache.ts` loadModelsWithCache：60s TTL + in-flight 去重），
`useAgentSession.loadModels` 用 `newSessionCwd ?? session?.cwd` 拉——草稿也走
全局接口，与会话进程零耦合。

**改动**：
- 模型目录上收 Chat 层：`models_by_cwd: HashMap<cwd, Vec<ModelInfo>>` 项目级
  共享；`SessionRuntime` 删字段，`get_available_models` 响应只转发
  （新 `SessionEvent::Models`，各 runtime 写各自 cwd 条目）；选择器
  （filtered_models）、设置页（mc_provider_ids/两处渲染）、mc_refs 全部读共享
  目录；pill 兜底改 `ensure_models_requested`——只向同 cwd 的带进程 runtime
  补拉，绝不为选模型 spawn 进程。
- 补 lazy connect（pi-web ensureNewSession parity）：`send_input` 发现无进程时
  spawn + attach_pump + refresh_state 再发 prompt。
- 补草稿提升（pi-web promoteNewSession parity）：get_session_stats 首次带回
  sessionFile（None→Some）发 `SessionEvent::FileBound`，池 key 从 draft-N 迁移
  到会话路径 + last_open + 侧栏刷新，否则重开该会话会起重复 runtime。
- 修 key 碰撞：启动无 last_open 时初始 runtime 占用 "draft-0"，draft_seq 原置 0
  会让第一次「新会话」覆盖已连接的 runtime，恒置 1。

测试：app 88 全绿。pi-link 52/53——`parses_session_info_name_from_real_file_tail`
失败为 pre-existing 机器态依赖（测试注释自述 machine-dependent by design，
本机该 2026-09-25 会话文件的 sessionInfo 名已不存在），与本次无关。

## 新会话页加载 pi 默认设置（2026-10-04）

**问题**：新会话（草稿）三枚 pill 全是假值——模型「选择模型」、思考硬编码
"medium"、工具显示 "default"，pi 配置的默认模型/思考档/工具完全没进 UI。
pi-web 的做法（读源码）：新会话初始 = `CONFIGURED_TOOL_PRESET`（不钉名单）；
默认模型/思考档来自 `/api/models` 的 `defaultModel`/`defaultThinkingLevel`
（服务端 selectInitialModelScope：settings 默认模型在 scope 内 → 用它，否则
scope 首个；思考档 = `enabledModels :level` pin > per-model
modelThinkingLevels > 全局 defaultThinkingLevel）；用户显式选择才随
ensure_session 透传，其余让 pi 端按 settings 解析。

**改动**：
- pi-link config.rs：`read_default_model`（defaultProvider+defaultModel）、
  `read_default_thinking_level`、`read_model_thinking_levels` 三个 reader
  （+2 测试）。
- Chat 层：`reload_model_defaults()`（启动 + 设置面板都调）读 settings.json
  默认值并重算 mc_state；`new_session_default()` = selectInitialModelScope
  parity（scope 用现成的 models_config glob + pin 解析）；`model_display_name`。
  Models 事件到达时重算 mc_state（scope 的 refs 此时才可知）。
- Runtime：`pending_model`（pi-web newSessionModelOverrideRef parity）——草稿
  选模型无进程时暂存，spawn 带 `--model provider/id`；`thinking_override` 同样
  以 `--thinking <level>` 上 spawn（重绑场景不再丢）；工具映射补 default
  （read,bash,edit,write）/ full（bash,read,edit,write,grep,find,ls）两档，
  初始预设改 "configured"（无参数 = pi 按 defaultTools 解析，CLI parity）。
- composer pill：草稿态 model = pending_model ?? new_session_default；thinking
  = override ?? draft 默认 ?? "auto"（删除硬编码 "medium"）；有进程后仍由 pi
  get_state 接管。工具 pill 菜单本就是 pi-web 5 档，现在初始值与语义对齐。

pi CLI flag 依据：`--model <pattern>`（支持 provider/id）、`--thinking <level>`。
测试：app 88 全绿；pi-link 54 绿 + 1 机器态失败（pre-existing，同上）。

## 设置「界面」页改版 + 主题持久化去污染（2026-10-04）

**背景**：设计文档 docs/UI设计/设置页面UI.md——标题「外观」→「界面」、删版本
号；主题淡色5/深色2 各一排（去掉冗余 (id) 注释与"主题写入…"说明）；字体族
换 zed 式下拉（250×400，顶部筛选行，参考 zed字体菜单.bmp）；字号改动即时
反映；语言三个一排宽 100px；提示音挪「其他」页。**且主题配置此前被写进 pi
的 settings.json（theme 键值域是 dark/light，写入 pi-flash 主题 id 会让 pi
每次启动报错）——彻底停止污染。**

- **去污染（根）**：删 pi-link `read_theme`/`write_theme`；main.rs 主题启动
  链 = PI_FLASH_THEME > app_settings.json（不再回退读 pi settings.json）；
  general.rs 切主题只走 `persist_theme`（app_settings.json）+ token 重映射。
  已清理用户机上被污染的 `~/.pi/agent/settings.json`（移除 "theme":"rose"，
  备份 .bak-piflash）。
- **主题**：两排卡片（淡5/深2，`theme::ALL.dark` 分组），swatch=主题底色+
  accent/muted 圆点，选中 accent 描边；显示名仅 entry.name。图标主题行随
  本版式移除（单选项死 UI，ICON_THEMES/icon_theme_id 一并删）。
- **字体（v60-2 WYSIWYG；v60-3 三修；v60-4 会话直连；v60-5 终版语义收敛；
  v60-6 封装 Dropdown 组件）**：
  v60-6（用户反馈：标签折行 + 弹层不贴合按钮正下方 + 要求封装组件）：
  **新增 `ui/dropdown.rs` 通用下拉组件**——锚定照搬 vendored gpui-component
  dropdown 的成熟机制：弹层 `deferred(anchored().snap_to_window_with_margin)`
  ——deferred 保留块流内布局（absolute 静态位置 = 触发按钮正下方，无需捕获
  /计算任何坐标）并逃出滚动容器裁剪，anchored 负责窗口内收口/溢出翻转；
  `on_mouse_down_out` 外点收起 + DropdownState 300ms 防抖守卫（挡外点收起
  与触发点击的双触发竞态）；popup 惰性构建（open 才建）。字体三槽位全部
  改走该组件：删 font_anchor/font_trigger_bounds/font_popup_layer/overlay
  弹层分支（快照减负），open 状态经快照传入 mc_general_view。"Markdown 字
  体"标签折行 = 110px 列宽在面板 1.33× 下不够 → 130px + whitespace_nowrap。
  v60-5（沿用）：
  v60-5（用户定义终版）：**会话字体 = 用户/agent 输出正文的族+字号，不含
  meta 等 chrome**——撤掉 chat_size 缩放体系（会话区 chrome/meta/composer
  全部回归 ui_size 面板缩放，51+2 处），气泡正文走 v60-4 的 MD_SPEC 直连
  （session_font 默认回 14）。**弹层"弹不出"根因 = gpui div() 默认
  position:relative**——v60-3 给背板/卡片包的 wrapper div 成了 absolute
  子节点的定位容器（0×0 → 背板消失、卡片锚到弹窗中心）；修复 = 背板/卡片
  作为 overlay 的直接兄弟子节点（font_popup_layer 返回 tuple）。
  v60-4：
  v60-4：用户实测"会话字体一点没变"的根因 = 气泡正文的族+字号都挂在
  markdown 槽位上（会话字号只以缩放系数叠上去，族完全无关）——改为
  **thread_local MD_SPEC 按渲染上下文分流**：render/render_user（聊天）=
  会话字体族+字号直连（气泡=用户设的族和号），render_themed（文件预览）=
  Markdown 字体槽位；代码块 12.5→12.5/BASE×spec 等比；chat_scale 乘法删除
  （chat_size 仍管 composer/会话区 chrome）。弹层"弹不出来"：回退 v60-3 在
  开启路径上加的 ensure_ready+事件期 window.focus（改回 focus_soon 渲染期
  聚焦），保留 per-slot bounds 与背板/卡片兄弟结构两个修复——开启机制与
  v60-2（已验证能弹）完全一致。
  v60-3 三修（沿用）：
  三槽位每行 = 110px 标签 + 250px 下拉按钮 + 字号步进，无预览行。双缩放轴
  模型（AtomicU32 缓存，save_font/startup 同步）：**面板字号 = 全局 UI 缩放**
  （`ui_size(base)`=base×scale，scale=面板字号/12，钳 10–16——固定高度行
  >1.33× 裁字）——173+3 处 `.text_size(px(N))` 脚本替换（tmp/sweep_ui_size.py；
  排除 markdown.rs/terminal.rs/render/math.rs），含设置页自身=调整时眼前即变；
  **会话字号 = 会话区缩放**（`chat_size(base)`，基准 15px=1.0，钳 10–24）：
  composer（15/chip 14）、会话区 chrome（session/* 51 处）、markdown 正文
  推导尺寸（base_style/行内 code/列表圆点 ×chat_scale()）全部跟随；
  **Markdown 字号 = 正文基准字号**（原有 spec.size 语义不变）。
  下拉弹层（v60-3 修）：canvas **按槽位**捕获触发按钮 bounds（原单字段被
  三个按钮每帧覆写 → 永远开在 Markdown 行位置）；**背板与卡片改兄弟节点**
  （原卡片嵌在背板里，点输入框冒泡触发背板"点击关闭"→ 弹窗秒收、无法输入
  过滤）；打开即 `ensure_ready` 建态 + `window.focus` 聚焦筛选框（不再依赖
  渲染期 focus，TextInput 的 focus_soon/want_focus 已回退删除）。250×400
  卡片 = 顶部筛选 TextInput（清空/Escape 收起）+ `uniform_list` 虚拟化列表
  （系统字体全量 `all_font_names`，startup 排序去重缓存 OnceLock；每项以
  自身字体渲染，active accent 高亮），点选写 app_settings.json。
  FONT_CHOICES/cycle_family 弃用删除。
- **语言**：三个按钮一排（各 100px、文字左对齐、active 描边）。
- **其他页**：新增「提示音」set_row（从界面页挪入）；`misc::stepper` 转
  pub(crate) 供复用。
- **实现注**：弹层 = render_settings 内 overlay 的兄弟分支（全窗口透明
  catcher 收起 + occlude 卡片），不进滚动容器；SettingsFormData 快照增
  font_popup/font_anchor/font_filter/font_filter_value（渲染期无 cx，筛选值
  快照时读）；TextInput 增 `focus_soon`（want_focus 渲染期聚焦，弹层打开即
  可打字）；Chat 无需 observe——gpui 渲染期读取的实体会注册 window
  invalidator（app.rs record_entities_accessed），InputState notify 即重渲染。

测试：app 88 全绿零警告；pi-link 54 绿 + 1 机器态失败（pre-existing，
parses_session_info_name_from_real_file_tail 依赖的特定会话文件已被 pi 重写，
与本次无关）。构建被运行中的 pi-flash.exe 锁定（os error 5），check 已过，
待关闭实例后 build + 实测。

### v60-3 最近活动会话清单（docs/模块设计/003-session管理.md 全量落地）

启动只加载最近 N 个 session 的机制层。三层职责：
- **摘要索引**（既有 `pi-flash-session-index.json`，不动）：每文件
  id/cwd/preview/name/条数，`(mtime,size)` 指纹失效。
- **活动清单**（新 `pi-link/recents.rs` → `pi-flash-session-recents.json`）：
  top-100 条目只存 `(path, last_active)`，按活动时间降序；摘要渲染时查
  索引不冗余。活动 = max(最近被打开, 最近有更新)：`touch`（打开/消息，
  恒提顶置 now）+ `record`（外部 mtime，max 语义不倒退）。
- **30s 轮询**（main.rs 常驻任务）：`poll_recent_sessions` 对账外部写入
  （pi-web/cli）→ 记录变更 + 清死路径 + 节流落盘；**排除所有活跃 runtime
  的会话文件**（pi 持续追加时轮询扫它们 = 每轮整文件重扫；它们的提顶走
  事件路径）。Scanner 增 `list_excluding(max, exclude)` 入口（对齐项：
  枚举+截断+指纹服务收敛）。

三条写入路径：`open_session`/`on_file_bound` 打开提顶；
`subscribe_runtime` Changed 分支消息级提顶（任何 runtime，非仅活跃）；
30s 轮询外部变更。`delete_session` 同步摘除条目。

启动路径：尾部预载数据源从 `list_sessions(n)`（全枚举排序）换成
`recent_preload_paths(n)`（读清单前 N，只 stat + 查索引）；清单为空
（首次安装到有历史会话的机器）内部用 mtime 序种子生成初版清单并落盘
——干净机器/存量机器统一为一个入口。跨项目 400 条 rebuild_projects
保持现状（全量摘要缓存）。

**顺手修存量 bug**（上一轮标 pre-existing 的
`parses_session_info_name_from_real_file_tail`，根因本次定位）：重命名
记录 `session_info` 停在写入时刻的位置，会话后续追加把它推出 64KB 固定
尾窗（实例：153KB 文件 09-25 改名聊到 10-04）→ name 解析失败退回
preview。`scan_file` 改用 `read_latest_session_info`：尾窗 64KB 起倍增
搜索至 1MB 封顶（`SESSION_INFO_MAX_WINDOW`），普通会话单次 64KB 读、
病态大文件也有界。

测试：pi-link 60 全绿（含 recents 5 项：提顶/容量、max 语义、prune/
remove、持久化 roundtrip、scanner 排除跳过）；app 88 全绿。构建仍被
运行中的 pi-flash.exe 锁定，待关闭实例后 build 实测。

### v60-4 「默认加载会话数」落地（用户反馈 v60-3 看不出效果）

根因：v60-3 只换了预载数据源，psp 列表仍走全量 `list_sessions(400)`，
界面无可见差异。本版把加载单位真正切到会话：
- **设置-其他**：「默认加载项目数」（1–10，钳 ps p组数）退役 →
  「默认加载会话数」（5–20 步进，默认 10），绑定 key `preload_sessions`；
  `project_count()`/settings.projects 字段删除（旧 key 值不再读写）。
- **启动 psp 数据源**：`list_sessions(400)`+排序 → 清单前 N 路径 +
  `sessions_for_paths`（纯 stat + 指纹索引查，种子后必命中，无文件读）
  ——全盘枚举彻底退出启动路径；列表可见效果 = 只显示这 N 个会话
  （按项目分组，组不再按项目数截断）。
- 尾部预载与 psp 共用同一批 `recent_preload_paths(n)` 路径（一次取得）。
- `sessions_for_paths`（pi-link）：显式路径列表取摘要，输入序保持，
  死路径丢弃。i18n 补两行（繁/英）。

测试：app 88 + pi-link 60 全绿；build 成功（实例已关），待实测启动
加载条数与设置页钳位。

### v60-5 修：改名后所有会话冲出列表（N 语义未贯彻刷新路径）

用户实测：改名单个会话名 → psp 当前项目组冒出全部会话。根因：改名完成
emit `ListDirty` → `refresh_sessions`，该函数沿用 v60 前"当前项目全量"
语义——`list_sessions_for_cwd(100)` 整列回灌，冲破「默认加载会话数」。
修复：refresh 结果裁到 mtime 最近 N 条 + 当前激活会话兜底（改名的会话
即激活会话，保证改名标题可见；打开的旧会话同理）。003 文档「运行中
边界」补记该行为。cargo check 0 警告；build 因运行中实例锁定 exe，
待关闭后重启实测。

### v60-6 修：列表重复会话条目（跨写方路径表示分裂）

用户实测：同一会话在 psp 列表出现两次（时间戳相同）。定位：清单文件里
同一文件存了两条路径串，唯一差异是 group 目录盘符大小写
`--d--ai_workspace-pi_work--` vs `--D--…`。成因：pi（Node）回传的会话
文件路径与 Rust 扫描器枚举的存储名大小写不同（Win32 两条串指向同一物理
文件，磁盘真实目录只有一个，大写 D）；`PathBuf` 严格相等把两条当两个
条目，启动 `sessions_for_paths` 双双返回 → 列表重复。

修复（pi-link/recents.rs）：路径身份键 `path_key`（Windows 小写化、
其他平台原样）——`promote` 按键合并、时间取 max（touch 恒 now、record
不回拨）、最新路径表示胜出（清单串收敛回枚举形态）、无实质变化不置
dirty（保住"仅变更时落盘"）；`load` 归并存量重复（按活动时间降序取
首个）并标记 dirty 洗盘。回归测试
`case_insensitive_identity_converges_cross_writer_paths`。

测试：pi-link 61 全绿（recents 6 项）、app 88 全绿。待用户重启实测
重复消失。时间窗口方案（3天/7天/2周/1月）讨论中，待拍板另做。

### v60-7 加载语义改版：时间窗口 + 每项目分页（用户拍板，学 zcode）

设置从「默认加载会话数」（数量 5–20）改为**「加载时间窗口」档位
7/14/30 天**（默认 7，key `load_window_days`；preload_sessions/settings.
preload 退役，尾部预载内部化为常量 TAIL_PRELOAD=10）。

- pi-link：清单容量 100→500（30 天重度硬上限）、POLL_SCAN_WINDOW 550；
  `recent_load_paths(days)` 替换 recent_preload_paths（空清单种子 + 窗口
  过滤，recency 序）。窗口测试 window_filters_by_recency。
- app 启动：psp 数据源 = 清单窗口内会话（sessions_for_paths 查索引）；
  预载取活动序前 10。refresh_sessions 改窗口过滤（mtime 近似）+ 激活
  会话兜底（不再裁数量）。
- **psp 分页**（学 zcode）：每组默认显示 10 个会话标签，底部「显示更多」
  点击续 10 条；Flat 模式全局键同样分页。PspRow 增 More 变体（去 Copy），
  Chat.psp_shown 跨刷新保持。窗口即归档：pi 无归档机制（append-only），
  滑出窗口 = 显示层隐藏，数据不删，搜索可找回。
- misc.rs：stepper 换三档位按钮组（仿语言按钮）。

验证：pi-link 62 全绿；app 因用户并行进行中的 general.rs 字体改版
（800+ 行未完成 diff）暂无法整链 build，本版改动文件零错误（check
错误全部位于 general.rs/mod.rs）。待其收敛后统一 build 实测。

### v61 滚动体验对齐 pi-web：平滑减速滚轮 + 卡顿三源治理（bead pi-flash-52q）

用户诉求：会话区内容一多上下翻页卡顿；pi-web 滚动带减速缓停效果极好。
研究结论：pi-web **没有自己写滚动物理**——减速缓停是 Chrome 合成器原生
ScrollAnimator（贝塞尔缓出 + ~250ms 收敛），pi-web 自做的是 scroll anchoring/
懒加载/`scroll-behavior: smooth`。pi-flash 的 gpui `List` 则是滚轮 delta
**直接跳变**（无动画），且每帧对可见条目全量重建元素树。

- **① vendored gpui List 平滑滚轮**（elements/list.rs）：非 precise delta
  （notched 滚轮）走动画路径——`begin_smooth_scroll` 把本次事件像素量累计进
  绝对像素 target，`advance_smooth` 在 `List::prepaint`（prepaint_items 之前，
  本帧即按推进后位置绘制）按 `current += (target-current)·(1-e^(-dt/τ))` 指数
  逼近（τ=70ms ≈ Chrome 手感；SNAP 0.5px 截尾；dt 上限 0.1s 防挂起跳变）。
  推进复用 `StateInner::scroll()` 全语义（钳制/贴底胶水/handler/notify）。
  precise delta（触摸板）保持原生直滚。程序化定位（scroll_to/scroll_to_reveal/
  set_offset_from_scrollbar/reset）取消进行中动画。动画未结束由 RAF 自续；
  事件派发期只 refresh() 起步（`request_animation_frame` 内 `current_view`
  断言仅绘制期成立，事件期调用会 panic——踩过）。平滑路径行高换算单独
  33px/行（系统 3 行/格 ≈ 100px/格，对齐 Chrome；触摸板仍 20px/行）。
  钉顶期滚轮退役锚点 → 垫片卸载 → target 重新钳到新 scroll_max →
  「内容下落」从瞬跳变平滑滑落。
- **② LineLayoutCache 存档层**（text_system/line_layout.rs）：原为两帧滑动
  窗口——滚出视口的条目两帧后 shaped 布局即被清，滚动中每条重进视口 =
  数百行重新 shape（大 markdown 轮块 5-30ms/帧，卡顿主因；最新 Zed 也是
  两帧设计，编辑器靠自有 wrap map 不受害）。新增 archive（48MB 字节预算、
  插入序驱逐）：finish_frame 沉降掉出两帧窗口的布局而非丢弃；查找路径
  current → previous → archive 三级回退，命中搬回 current（自然近似 LRU）。
  字节估算 16B/字符 + 512B 常数；重复沉降/陈旧 order key 均防御处理。
- **③ markdown 解析缓存**（app/markdown.rs）：`render_impl` 每帧重跑
  pulldown-cmark（大消息数百 µs～ms）。新增 `cached_blocks`：key=(FNV-1a
  8B 块哈希, len, html 标志)、命中全等校验防碰撞、线程局部 128 项 LRU
  （Rc<Vec<MdBlock>> 共享，渲染期零拷贝）。流式末条每 delta 一变 → 命不中
  也只是队头轮换。
- **④ 图片解码缓存**（app/session/messages.rs）：用户图/工具结果图每帧重走
  base64 解码 + `Image::from_bytes` 内容哈希（几百 KB = ms 级）。
  `decode_image_cached`：key=(哈希, len, 格式)、Err 同样入缓存防反复重试、
  64 项 LRU；`Image.bytes` 公开字段保住 MAX_IMAGE_BYTES 校验。
- 测试：app 88 + pi-link 62 + gpui 内部 4（scripts/test_gpui_glue.sh）全绿；
  check 0 警告。build 因运行中实例锁 exe 待重启实测（同 v60-5）。

### v61-1 调参：减速滑行延长（用户实测反馈"不卡了"）

`SMOOTH_TAU_SECS` 70→110ms（收手后 ~330ms 走 95%、~500ms 完全缓停），
`SMOOTH_SNAP_PX` 0.5→0.25（尾巴多走几帧再吸附）。其余不动；app 15 项
chat_list 测试复跑全绿。exe 被运行中实例锁定，待关闭后 build 实测。

### v61-2 字号体系整理：docs/UI设计/字体大小设置.md 落地

核心换算从「比例缩放 base×(panel/12)」改「**绝对像素差**」：任何设置档下
「设置值±N」精确成立——`ui_size(base)` = 面板设置值 + (base-12)（appearance
PANEL_PX 缓存钳 10–16）；markdown MD_SPEC 同理 `spec.size + (size-14)`。
新增 `sess_size(delta)` = 会话设置值 + delta，会话区文字从面板缩放体系迁到
会话体系。文档未列的元素基值一律不动（默认档视觉不变）。

- **面板侧**：topbar 标题 12.5→12；项目/会话标签 13→12；目录树 text_xs
  （固定 12 不缩放，bug）→ ui_size(11)；git 面板 14 处统一 12/12.5/11.5→11
  （含 text_xs 错误行）；导航面板 用户 12.5→11 / agent 12→10；操作栏
  BAR_FONT 15→12；placeholder 15→12（输入文字改跟会话字体，Q: 文档未提
  输入正文）；上下文用量弹窗正文 14→12（title 与其他相等 ✓）。
- **会话侧**：thinking ui_size(11)等宽 → sess(-2)；工具卡全套（头/名/参/
  耗时/结果/diff/patch/chips）→ sess(-2)，内部相对差 -1 保留；代码块
  12.5 比例式 → spec-1；表头 13→spec（表体 -1 不变）；agent 名/费用/
  操作栏/时间戳 → sess(-1)；工作详情行 → sess(0)；list mark 槽固定 14 →
  spec+2，disc 直径 0.45×spec → 0.45×(spec+2)。
- **文件字体**：设置标签「Markdown 字体」→「文件字体」（i18n 同步；
  存储键 markdown_font 不变保兼容，FontSlot::Markdown→File）。md 预览
  原本就走该槽位；源码预览（txt/json/py…）ui_size(12.5) → 文件字体字号
  （族固定 JetBrains Mono 等宽）。
- **设置页**：面板槽字号三档 14/15/16 → 11/12/13（原默认 12 不可选，bug；
  就近档位映射改通用最近邻）；settings 各页 SEMIBOLD 条目名 15→13
  （标题=设置值+1 档）。
- 决策记录（提问未复，按推荐执行）：①±N=绝对像素差；②输入文字跟会话
  字体；③标号字号=会话+2 且圆点同步放大；④源码只取文件字体字号、族固定
  等宽。文档未列的钉死值（代码块头 11、任务对勾 10、超长兜底 12、statusbar
  无文字）保持原状。app 88 测试全绿。

### v61-3 字号档位统一：三槽共用 14/15/16/17，默认 15（用户定案）

用户反馈三槽字号下拉必须同一组值：`size_trigger` 去掉面板槽特判，统一
小 14 / 中 15 / 大 16 / 特大 17（新增「特大」档，i18n 补繁/英）。默认值
对齐 15：session/panel/file 三槽默认全改 15（面板原 12 在 14 起步的档位
里永远不可选），PANEL_PX 初值 15、钳位 10–16 → 10–17（否则特大被钳）。
绝对像素差模型不变——ui_size 仍 = 面板设置值 + (base-12)，文档
「设置值±N」继续精确成立；默认从 12→15 只是整窗 chrome 统一 +3px。
app 88 测试复跑全绿。

### v61-2 滚动算法重写：Zed gestures 同源闭式样条（用户反馈"不丝滑、总有点卡"）

研究 D:\github\zed（gpui/gpui_windows）：Zed 的滚动物理在 `gpui/src/
gestures.rs`（AOSP `OverScroller.SplineOverScroller` Apache-2.0 转录）——
**轨迹闭式化**（位置 = f(速度, 已过时间)，与帧节奏解耦）+ 定长减速精确归零。
平台层与 present（`Present(0,0)`）均无平滑，vsync 线程仅做 GPU 掉线检测
（vendored 同构）。

旧指数逼近法（τ 时间常数）两因不丝滑：①起步速度 ∝ 剩余距离，每格滚轮首帧
只走 ~13%（发闷）②收尾亚像素爬行百余 ms（150% DPI 下肉眼可见顿挫）。

重写（elements/list.rs）：`SmoothScroll{target, segment_start, started_at,
duration}`——滚轮事件累计绝对像素 target 并**重启轨迹段**（Chrome
ScrollAnimator 同款），位置闭式求值 `start + 距离·spline(t/duration)`，
时长 = 剩余距离/巡航速度（钳 0.18~0.7s）。新常量：SMOOTH_CRUISE_PX_PER_SEC
=1400、SMOOTH_MIN/MAX_DURATION_SECS=0.18/0.7（τ/SNAP/MAX_DT 废弃）。
新增缓出样条几何锁单测（端点/单调/值域/起步快于线性）。gpui 内部 5 +
app 88 + pi-link 62 全绿；build 成功（exe 未被锁）。设计细节归档
docs/模块设计/050-滚动优化.md。

### v62 会话字号「改了不变」根因修复：gpui StyledText 排版字号只认容器继承

**症状**：改会话字体档位，聊天正文/代码块纹丝不动，只有列表标号联动
（用户 A/B 截图 + 像素行高测量实锤：小14 与特大17 两态正文行高完全相同）。

**根因**（gpui 0.2.2 text_system）：`shape_text` 只吃一个 font_size，来源
是 `window.text_style()`（容器继承链）；runs 里的 `TextStyle.font_size`
只决定字族/字重/颜色/装饰，**对字形尺寸无效**。markdown 里靠 base_style
（run 字号）传尺寸的元素——正文段落、代码块体、行内 code、表格 th/td——
从未按任何设置渲染过，一直画容器继承的默认 ~16px；只有标题、列表标号槽
这类把 `.text_size()` 挂在容器上的元素真正随设置变——正是「标号联动、
正文不动」的原因。字族（runs.font）不受影响，换字体立刻生效，更具迷惑性。

**诊断路径**（避免重走）：设置 JSON 写入 ✓ → 渲染层 set_md_spec 每次
render 都拿到新值 ✓（临时文件日志）→ 像素行高测量（DPI 150% 校准）→
vendored gpui shape_text 探针拿到决定性证据：h2 排版字号 = 会话值+1.12
（挂容器的标题 ✓）而正文恒为继承默认。

**修复**（markdown.rs）：新增 `sized_text()` —— 字号 = 槽位值 + (size-14)
与行高挂在容器 div 上，正文段落（含 flex_wrap 富段）与表格 th/td 改走
sized_text；代码块体容器挂 text_size(spec-1)+行高 1.62；行内 code 容器挂
spec-1.12。标题/标号原本就是容器挂法，不动。诊断探针与日志已全部移除。
app 88 测试全绿，正式版重启验证。

### v62-1 字号微调（用户定案）：表头加粗落进 runs、list mark 回归设置值

- 表格头：字号已是设置值，但**加粗从未生效**——容器的 font_weight 同样
  传不进 StyledText runs（与 font_size 同一个 gpui 0.2.2 限制）。base_style /
  styled_text / sized_text 增加 weight 参数：th = SEMIBOLD，正文/td =
  NORMAL；标题 runs 同步补 SEMIBOLD（此前标题也是容器加粗、runs 常规）。
- list mark：设置值+2 → **设置值**（标号槽字号与圆点直径 0.45em 同步回
  归 spec 基准）。
- 表格内容 = 设置值-1 核实无误。app 88 测试全绿。

### v62-2 消息操作栏统一（主界面UI设计-2.html）：图标修复 + 双栏对齐设计稿

**图标丢失根因**：agent/user 操作栏图标走 icon_hover 自定义元素（v55），
在消息列表（list 虚拟化条目）里占位不绘制——占位宽在、paint 无输出
（同款元素在 titlebar 正常，列表内异常；未深究）。改用 icon_current：
普通 gpui Svg 不设色、继承容器文字色（style.text.color），等价设计稿
svg 的 currentColor——act hover 提亮时图标+文字一起变色。工具卡图标
（同列表、同元素类型）一直正常渲染，机制可靠。

**Agent 栏 .as-stats**：gap 14 / 字号 ui_size(11.5)（回归先前调好的值，
废弃 v61-2 的 sess(-1)）；复制 act = 图标12+文字 gap4，hover 提亮；用时、
时间戳 = 普通 span 浅一档（text_faint ≈ 设计稿 #a3b0aa）；时间戳从
「独立右对齐行」移入栏内（设计稿结构）；已复制态 pill 整体 accent。
**User 栏 .msg-actions**：gap 12 / 字号 ui_size(11.5)；复制/编辑/新分支
三 act 图标+文字；.when 时间戳入栏（ml 4、text_faint），随栏 hover 淡入
（设计稿如此，废弃 v56-1 的时间戳常显右对齐）。底行 pr 4 保留。
icon_hover 从 messages.rs 移除（titlebar 等处保留）。app 88 测试全绿。

### v62-3 操作栏收敛（用户定案）：icon() 化、删已复制、新分支恒显、时间格式

- **图标**：操作栏 icon 全部改 `icon()`（工具卡同款 gpui::svg+显式
  text_dim）。svg 上挂 group_hover 会让 copy.svg 不渲染（check/pencil 同
  构造却正常，未深究）；icon hover 提亮暂只作用于文字。pencil/git-branch
  资产更新为用户给的 lucide 源（copy 原本就一致）。
- **删「已复制」**：设计稿无此反馈态。copy_flash 全链路移除（runtime
  字段/spawn_flash_clear/render_msg 与 render_assistant_turn 的 copied 参
  数及调用点）。点击复制只写剪贴板，栏不变。
- **新分支**：用户栏恒显示（设计稿三 act 齐全）；历史消息无 entry_id 时
  点击不动作。
- **时间格式**：fmt_msg_time 改「9月17日 16:20」（设计稿 .when；跨年补
  年份），废弃 MM-DD HH:MM。
- **计费/用量行**：设计稿无展示 → 不再渲染（usage_line 及调用点删除）。
- 悬停淡入沿用 group/usermsg·astat 机制（用户截图证实悬停时栏可见）。
  app 88 测试全绿。

### v63 会话导航面板落地（033 设计稿）：12px 刻度条 + 350px 悬浮摘要面板，修三 bug

**刻度条重做（v54 比例尺 → 033）**：右缘 26px 灰点比例尺改为 12px 刻度条，
屏高 75% 垂直居中；每轮一枚 2px 高 × 8px 宽刻度（行高 5px = 刻度 2 + 间隔
3，整行命中可点，group_hover 行悬停增亮），新增刻度 justify_center 整体重
新居中；当前轮 accent 全宽 12px，其余 0xa9beb5。编号不再截最近 10 轮——全
部轮次 01、02… 顺排（033：01-99）。>150 轮居中裁剪（overflow_hidden），
分页/缓存池仍留 033 待讨论。

**悬浮面板（350px）**：hover 刻度条向左弹出，topbar 之下 100% 高，occlude
+ overflow_y_scroll + track_scroll(nav_flyout_scroll，跨开合保持)；滚动条
按 user 气泡同款挂滚动容器平级 absolute 兄弟（right 3..11、宽 8、上下缩
8），仅 max_offset>0 渲染；pl 6/pr 14 给滚动条让位。摘要 = 用户前 50 字 +
agent 首条有正文回复首行前 50 字（新增 Msg::plain_text_prefix(N) 只拼前缀
——nav 每帧取数，旧 plain_text 全量 join 的成本随长回复线性涨）。卡片
hover 选择框只在 enter 置位、leave 不清（flyout 离开统一清）。

**三 bug 根因**：
- 向下划过选中框不显示：gpui 鼠标事件 bubble 阶段按绘制逆序派发，向下移动
  时上一张卡的 leave 在下一张卡 enter 之后触发，把 nav_hover_turn 清回
  None——enter-only 置位即修（向上恰好顺序相反故一直正常）。
- 没有滚动条：v54 压根没实现，非显示 bug。
- agent 行没左对齐：A 行多挂 ml(27) 缩进；去掉后与用户行同构（18px 右对
  齐标号列 + gap 9），A 对齐编号、正文对齐用户正文。

结构：nav_gutter 摘要数据（turns/turn_user/turn_agent/scroll_top_ix）帧首
一次性算完落 owned 值再拼元素，替代旧实现 listener 之间反复 read。app 88
测试全绿。附带发现：icon_current 未使用警告来自工作区未提交的 v62-2 后续
重构（messages.rs 已改用 icon()），非本次引入。

### v63-2 会话下边距 150 → 180：内容距 input panel 上沿 20px → 50px

用户口径：跟尾时消息末条离 input panel 太近，从「胶囊上沿 + 20px」抬到
「+ 50px」。只改一个常数 `chat_list::PAD_BOTTOM`（150 → 180，= 胶囊 ~110 +
底距 20 + 内容让位 50）——内容区高（视口 − 22 − PAD_BOTTOM）、钉顶/跟尾判定、
垫片一整屏高、列表 `.pb()`、`glue_geometry` 测试常量全部由它推导，无第二处
硬编码（仅 `spacer_target_is_a_constant_while_pinned` 的断言字面量同步改 180）。

「回到最新」按钮位置**不动**（仍悬浮在胶囊上方 20px）：它只在脱离尾部时出现，
钉顶/跟尾两种状态下都不会与内容末条重叠，之前「与内容末条同一条线」的巧合
不再是设计约束。

### v63-1 导航刻度条定稿参数 + 选中语义解耦（033 用户反馈）

**样式定稿**：刻度 2px 高 × 10px 宽（行 = 刻度 + 左右 padding 4 = 18×6 命中
区），间隔 4px；悬浮面板 350→400px。居中规则改为「屏高 − inputpanel 高
度×50% 后居中」：rail 挂 mb(0.5×composer高)——flex justify_center 连
margin 盒一起居中，视觉中心正好上移 composer/4，无需测量刻度条自身高度；
composer 高度用新原语 ui::measure_height（透明 Element，prepaint 把子元素
布局高度写入 Rc<Cell<f32>>，包在胶囊本体上、不含回底按钮以免显隐 ±52px
抖动）每帧实测，读上一帧值一帧滞后无感。

**选中语义 bug**（用户：被选中刻度不应跟卡片悬浮框走，点击定位后的轮次才
是选中刻度，常态对话在末轮所以末刻度常选中）：根因是刻度与卡片共用
sel = nav_hover_turn.or(active_turn)。解耦：刻度只看 active_turn = 贴底
（pager.is_at_bottom，含未滚动短会话）→ 最后一轮，否则视口顶之上最近轮；
卡片选中框只看 nav_hover_turn。点击卡片/刻度 → scroll_to_reveal 改写滚动
位置 → active_turn 下一帧跟随。

app 88 测试全绿（主 exe 链接被运行中的实例占用跳过，关闭后重跑 build）。

### v63-2 导航点击后刻度不动的根因：程序化定位是「三不管」路径

用户实测：点击卡片/刻度重定位后，选中刻度没有相应变化。三个坑叠加：
1. **gpui ListState.scroll_to_reveal_item / scroll_to 不触发任何 notify**——
   逻辑位同步改了，但没有重渲染就没人在读；列表本身靠悬浮等杂散事件
   重绘，刻度（nav_gutter 同帧重建）就停在旧值。
2. **scroll 回调只在滚轮路径触发**（list.rs apply delta 处）：pager 的
   at_bottom Cell 程序化定位不更新。v63-1 让 active_turn 优先信它 →
   点击导航后它仍是 true → 刻度永远错停末轮。修：去掉该分支，纯
   rposition(视口顶之上最近轮)——贴底时逻辑位 None，scroll_top_ix 回
   退成总条目数，末轮自然选中，语义不回退。
3. **reveal 向下跳是「目标底边贴视口底」**：目标消息停在视口底，视口顶
   落进上一轮内容，active 判定（视口顶之上最近轮）算到前一轮。修：
   pager 新增 nav_goto(ix) = scroll_to 目标置顶 + at_bottom 补正 false
   （jump_to_bottom 同款显式维护；跳离贴底后「回到最新」按钮随之出现），
   导航三处点击改用 nav_goto + window.refresh() 当帧重渲染。

结论：程序化滚动要自己补「通知 + 标记位」，gpui 只管滚轮路径。app 90
测试全绿（exe 链接被运行中实例占用，关闭后重跑 build）。

### v63-3 操作栏悬停统一：用户气泡的 occlude() 吞掉了行级 hover + 两套写法并成一处

用户口径：鼠标停在用户消息上，底部操作栏（复制/编辑/新分支 + 时间）不显影，
agent 轮块却正常——同一个行为不该有两套写法。

根因（gpui hit-test 语义，非「忘接线」）：v57 给用户气泡加了
`div().occlude() // 禁止鼠标透传到下层消息`。`Window::hit_test` 是**从后往前**
收集 hitbox，命中 `HitboxBehavior::BlockMouse` 就 `break`——被它挡住的是**先插入
的** hitbox，也就是该气泡的**全部祖先**（行级 `on_hover` 所在的 `msgrow-*`、
列表项、列表本身）连 `ids` 都进不去，`hitbox.is_hovered()` 恒 false。于是：

- 鼠标停在气泡上 → 行级 on_hover 永不触发 → `Chat.bar_hover` 不置位 → 操作栏
  `opacity` 停在 0（鼠标滑到气泡**左侧空白**才显影，用户看到的就是「时灵时不灵」）；
- agent 轮块没有遮挡后代，所以「同一个功能两处表现不同」。

同一处还夹带一个滚轮 bug：`ids` 被截断后外层会话列表不在命中链里
（`should_handle_scroll` 也读 `ids`），气泡内滚到底后滚轮没处可去。删掉 occlude
两件事一起好。

实证（新增 `messages::bar_hover_hit_test`，真 hit-test + 模拟鼠标移动，不是读代码
猜的）：气泡无遮挡 → 行 on_hover=1；气泡 `occlude()` → 行 on_hover=0（气泡自身
仍是 1）。正反两侧都留成测试：前者锁线上行为，后者锁上面这条 gpui 语义——上游
若改语义，这条会先炸，提示回来复核。

修法（统一实现，两处写法并成一处）：

- 删掉气泡的 `occlude()`，就地留注释说明**不得**再加（并写明滚轮副作用）；
- 新增 `messages::bar_hover_wired(el, weak, ix)`：用户行与 agent 轮的进出
  on_hover 都走它，注释写明「勿各写一段内联」（两套写法必然分叉，本次即此）；
- 状态从 `Chat.user_bar_hover` + `Chat.turn_bar_hover` 两字段合并为
  `Chat.bar_hover: Option<usize>`（用户行 = msg_ix，agent 轮 = 轮首 msg_ix；
  角色不同故索引永不撞车），`session_list` 两处 `bar_revealed` 同源。

验证：`cargo test -p app` 90 全绿（+2 新测试）；`cargo check -p app` 0 警告。
真机待复验：鼠标停在用户气泡任意位置（文字上、长气泡滚动区上）操作栏都显影，
与 agent 轮块同款淡入。

顺带记录（未处理）：工作区另有若干文件的行尾是历史遗留的 CRLF 污染
（`session/mod.rs` `dialogs.rs` `function_panel/*` 等，`git diff --ignore-cr-at-eol`
才看得见真实改动）——本轮把 messages.rs 自己写坏的部分已还原成 LF，其余未动。

### v63-4 复制图标不显示：copy.svg 从未登记进 assets!()（gpui svg 取不到资产就静默画空白）

用户口径：操作栏里「编辑」「新分支」的图标都在，只有「复制」前面空着。

根因：`ui::icon(name)` 走 `gpui::svg().path("icons/{name}.svg")`，资产由 `assets.rs`
里手写的 `assets!()` 列表 + `include_str!` 编进二进制。`copy.svg` 在磁盘上
（`assets/icons/copy.svg`，lucide 双矩形）但**没写进那个列表** → `Assets::load`
返回 `None` → gpui 对取不到的 svg **静默画空白**（不报错、不 warn、不落日志）。
用户行与 agent 轮两处复制图标同源同写法，所以一起消失，其余图标全部正常。

全量审计（不是只挑 copy 看）：`icon()`/`icon_hover()` 调用点共 22 个不同名字，
只有 `copy` 是「有调用点、没登记」；磁盘 30 个 svg 里也只有 `copy.svg` 漏登
（原先 29/30 都登记了，正好漏掉这一个）。

修法：

- `assets!()` 补 `"icons/copy.svg"`；
- 新增 `assets::tests::every_icon_file_and_call_site_is_registered`，双向锁：
  ① `assets/icons/*.svg` 必须全部登记（漏登即报出文件名）；
  ② 源码里每个 `icon("x")` / `icon_hover("x")` 调用点的名字必须有登记资产
  （注释行不算）。旧测试 `all_icons_load` 只遍历宏列表，**漏登的文件它天然看不见**
  ——本次这个 bug 正是从这个盲区漏出去的。
- 反证：临时删掉登记行，测试立刻报
  `assets/icons 下这些 svg 没登记进 assets!()（渲染为空白）：["copy.svg"]`。

验证：`cargo test -p app` 91 全绿（+1）；`cargo check -p app` 0 警告。
真机待复验：操作栏「复制」前出现双矩形图标（用户行与 agent 轮两处）。

### v63-3 导航性能定稿：摘要缓存进 runtime + 刻度上限 50 百分比定位

**摘要缓存（方案1 落地）**：gpui 即时渲染下 nav_gutter 原先每次重绘都从
messages 现拼一遍摘要（用户/agent 前缀）——数据一轮一变，计算却每帧重复。
改为 TurnSummary（user_ix/user_prefix/agent）存 SessionRuntime：notify_list
（一切消息变化的必经点，含流式 delta）置脏，nav_summary() 惰性重建并返回
Rc<Vec<_>>，渲染帧零重算；切会话回切直接命中缓存。

**刻度上限 50（方案2 用户定案替代压缩/采样）**：轮次 ≤50 逐轮一枚；超出
改百分比定位——50 枚均分轮次区间，轮 k → 刻度 k×50/N（用户例：100/1000
→ 第 5 枚），点击刻度回桶首轮 i×N/50，active 轮映射回桶高亮；飞出卡片仍
逐轮全列。飞出面板虚拟化明确不做（用户：先不考虑）。

顺带：刻度/卡片元素 id 从消息索引改为刻度序号（量化后消息索引不再唯一）。
app 91 测试全绿。033「待讨论」两条全部销项：跨会话缓存池在现架构下无的放
矢（后台会话不产生导航开销），页内分页被百分比定位替代。

### v63-4 导航总结栏：30px 固定头 + 自洽口径统计

pi-web 固有统计考据（lib/session-stats.ts）：totalMessages/user/assistant
按消息条目数、toolCalls 从 assistant content 块数、toolResults 是独立条
目，custom 不计——严格口径 N = 用户+助手+工具结果+其他，≠ 用户公式。
本工程 toolResult 入库即合并进 ToolCall 块（不占消息位）、system 跳过，
两边数字本就对不上。按用户定案采用自洽口径 N = x+y+z。

实现：NavData（turns + users/assistants/tool_calls）并入既有导航缓存
（notify_list 置脏、build_nav_summary 同遍历数出，零额外每帧成本）；面板
改 wrap(flex 列) = 总结栏 30px 固定 + 卡片滚动区 flex_1，总结栏文案走
i18n 三语表；悬停保持打开的 on_hover 上移到整包 wrap——鼠标挪到总结栏
不算离开面板；滚动条锚 top 38 让出总结栏。app 91 测试全绿 + 完整链接过
（期间撞上另一会话对 runtime.rs 的进行中重构，等它落盘后复验通过）。

## 状态（2026-10-05）— 「新分支」按钮修复：两层根因 + pi-web fork 语义对齐

用户报：用户消息下的「新分支」点了没反应。三层问题，按发现顺序：

### 1）serde_json 递归上限吃掉整条 `get_tree`（真正的死按钮）

`parse_line` 用 `serde_json::from_str`，默认递归上限 128 层。pi 的
`get_tree` 按 children 逐层嵌套整条会话（每条 entry ≈ 4~5 层 JSON）：实测
85 条消息的会话树深 95 → 整行 `recursion limit exceeded` → **响应被
parse_line 丢弃、无任何事件**（wire log 里明明有那条 402KB 的 `<` 行）。
没有 get_tree → 没有 entry id → 所有用户消息 `entry_id: None` → 按钮
根本不挂 handler（`if let Some(eid) = entry` 的 else 分支）。

修法：新增 `pi_link::json::parse_value`（`Deserializer::disable_recursion_limit`
+ Cargo feature `unbounded_depth`），`protocol::parse_line` 与
`sessions.rs` 的 JSONL 读取统一走它；顺带把 reader 线程栈从默认 2MiB 提到
16MB（`Value` 析构递归，3000 条 entry 的会话在 2MiB 上有溢出风险），
以及 reader 遇非 UTF-8 行不再 `break` 整条流（原来是「一行坏、后面全静默」）。

### 2）长会话上 pi 自己的 `get_tree` 会炸（更深的问题）

2970 条 entry 的会话（实测）：`get_tree` → `success:false,
"Maximum call stack size exceeded"`（**pi 侧**溢出，不是我们）。也就是说
第 1 点修好后，超长会话里按钮仍然是死的。

对齐 pi-web 的正解：pi-web 从不用 get_tree 算 fork 锚点——
`lib/session-reader.ts` 的 `sliceActiveBranch` 在**扁平 entry 列表**上走
parentId（rpc 侧的对应命令就是 `get_entries`）。于是：

- pi-link 新增 `Command::GetEntries`、`SessionEntry`、`parse_entries`、
  `active_user_entry_ids(entries, leaf)`（leaf→root 回溯 + user 过滤 +
  parentId 环保护）
- runtime 新增 `refresh_anchors()`：发 GetEntries（权威）+ GetTree（033
  导航面板用，失败即降级为空，不再影响「新分支」）；`apply_entry_ids()`
  统一回填
- 实测同一会话：get_tree 失败 / get_entries 2970 条、38 个 user 锚点 ✅

### 3）fork 之后 shell 身份没跟着换（分支生效但界面还挂在父会话）

pi 的 fork 是**进程内 rebind**：`runtimeHost.fork` → `createBranchedSession`
→ 同一进程换绑新文件。旧实现只清消息重载，`self.file`/pool key/侧栏高亮/
recents 全留在父会话上。

- `SessionState` 补 `session_file` / `session_id`
- runtime 新增 `follow_session_file()`：get_state / get_session_stats 里
  发现 sessionFile 变了就跟随（draft 首条 prompt 落盘、fork、clone 三个
  场景统一走这一条路），重取磁盘探针并发 `FileBound` → Chat 的
  `on_file_bound` 迁移 pool key + 侧栏 + last-open

### 4）UI 对齐 pi-web UserMessageView

- in-flight 态：`runtime.forking` → 按钮文案「创建中…」、主题色、
  `cursor_not_allowed`、点击不挂 handler（防连点发第二个 fork）
- 操作栏 `opacity: hovered || forking`（pi-web 同式），创建中不许消失
- i18n 三语补 `创建中…`
- `fork_from_entry` 补前置条件：进程必须在、`forking` 去重

### 实测证据（crates/pi-link/tests/live_fork_probe.rs，新增）

短会话（167 条消息）：`fork` → `success:true`，
sessionFile 从 `…T05-41-06…01a10a94` 变成 `…T06-02-29…01a10aa8`，
sessionId 同步变更，messageCount 归 1（fork 点之前的路径被复制过去）——
**clone 新分支 + 开新会话**这条链路 pi 侧本来就通，问题全在客户端。

回归锁：pi-link 65 测试（新增 deep_tree 400 层 parse、flat entries 锚点、
parentId 环 3 条）。app 91 测试。check_arch 违规数与基线一致（4+3，全存量）。

### 顺带的架构收敛（不给 check_arch 留新账）

- 新增 `session/fork.rs`：fork_from_entry / follow_session_file /
  refresh_anchors / apply_entry_ids / on_fork_response / collect_path_user_ids
  全部搬出 runtime.rs（1701 → 1566 行）
- 新增 `session/actions_bar.rs`：用户消息操作栏（复制/编辑/新分支/时间戳）
  搬出 messages.rs，`render_msg` 371 → ≤300 行（该函数不再是违规项），
  messages.rs 2649 → 2545 行

### 待真机验证（需先关闭正在运行的 pi-flash.exe 才能覆盖 target/debug）

1. 打开一条 85+ 条消息的老会话 → hover 用户消息 → 「新分支」可点
2. 点击 → 状态栏 forked，侧栏高亮切到新分支会话，标题变新分支内容
3. 新分支里发一条消息正常流转
4. 1000+ 轮的超长会话里「新分支」同样可用（get_tree 在那里已被 pi 拒绝）

### v63-5 刻度条生成算法定稿：轮次均分切割整条（用户方案）

替代 v63-1 的固定 2px 小横条：竖条按轮次均分切割——1 轮整条弱主题色、
N 轮切 N 段（flex_1 等分 + 2px 缝），20 轮后段高不再缩小；>20 走既有百
分比桶映射（TICK_MAX 50→20，轮 k → 段 k×20/N，点击回桶首轮）。**定位/
选中算法一行未动**：active 段跟随真实滚动位置（贴底=末段）、卡片悬浮框
不回写、nav_goto 置顶跳转——只换了「段怎么画」。段色从灰绿 0xa9beb5 改
为弱主题色（accent α0x3d），hover 增亮 accent 实色；删掉每刻度的
group/group_hover 双层结构（段本体即视觉即命中区），元素数 3×N→N。
app 91 测试全绿 + 完整链接过。

### v63-6 导航面板虚拟化 + 段→面板联动（「方案3」落地）

卡片列表从 overflow_y_scroll 全量 div 换成 gpui `list()`：ListState 存
Chat（跨开合保持滚动位，轮次数变化时 reset），每帧只建可视卡片 ~20 张——
千轮会话原先是 1.2 万+ div/帧 + 1200 个 hover 监听。要点：
- **水平 padding 挂外层容器**：List 条目只认上下 padding（session_list
  同款结论），pl 6/pr 14 留滚动条位，pt/pb 8 挂 list 本体。
- **卡片构建抽出 nav_card()**：list 闭包只有 &mut App，hover 改走
  WeakEntity::upgrade + update（语义不变：enter 置位、leave 不清）；选中
  框状态经 weak 升级 + read 读取。
- **滚动条**：gpui-component 不认 ListState——新增 ui/list_handle.rs 的
  ListStateHandle 适配 ScrollHandleOffsetable：offset 直接转接 list.rs 的
  scroll_px_offset_for_scrollbar（负 y 约定与 ScrollHandle 一致），
  content_size = max_offset_for_scrollbar + viewport_bounds，组件滚动条
  （含拖拽）原样可用。
- **段→面板联动**：点段 = 消息 nav_goto + 面板 scroll_to_reveal_item 到
  该段区域起始卡（卡序号=轮序号）。区域分页方案讨论后不做：虚拟化把每
  帧元素数打到 ~20（分页是 60），且零窗口管理逻辑。

app 93 测试全绿 + 完整链接过（期间两度撞上 fork 会话的 pi-link/fork.rs
中间态，等落盘复验通过）。

## 状态（2026-10-05）— 「新分支」改挂 agent 回复 + 分支自动改名（用户定案）

### 1）入口从用户消息移到 agent 轮操作栏

用户判定原设计（pi-web：按钮在用户消息上）语义反了：`fork` 的 position 只能是
`before`，分支点 = 该用户消息的 parentId，**这条用户消息本身不会带过去**——
点「自己这条消息」却从它之前开始，很反直觉。正确语义是「一切从 clone 的地方
开始」：按钮挂在 **agent 回复** 上，分支保留到本轮回复为止。

- `ForkAnchor { next_user: Option<String>, tail: bool, forking: bool }`
  （`session/fork.rs`）：分支点 = **下一条用户消息之前** → 从 agent 轮下按钮点
  下去，带过去的是「本轮及之前全部」，其后的用户消息与内容丢弃
- 本轮就是尾部（没有下一条用户消息）时 pi 没有可用的 before 目标 → 走
  **rpc `clone`**（`runtimeHost.fork(leafId, {position:"at"})`，整段复制）。
  pi-link 新增 `Command::Clone`（wire `{"type":"clone"}`）；`fork`/`clone`
  响应共用 `on_fork_response`
- 锚点没回来（`active_user_entry_ids` 为空）或中途某条用户消息缺 entry id →
  `ForkAnchor::clickable()` 为假，按钮渲染但不接点击（宁可不点，不能开错地方）
- 用户消息栏回归「复制 / 编辑」两 act

### 2）分支自动改名：原名截取 20 字

克隆出来的会话与原会话同名，侧栏里分不出谁是谁（用户实测反馈）。

- 初版规则是「前 15 字 + "2"」；用户连做两次 clone 后实测后缀叠成 `…22`，
  判定很蠢 → 改为**只截断、不加任何后缀**
- `branch_name(base)`：`chars().take(BRANCH_NAME_CHARS = 20)`，超长补 `…`；
  按**字**截断（中文不能切半个），空标题返回空串（调用方跳过
  `set_session_name`，不发明名字）；单测覆盖 20 字边界 / 中文 / 连续 clone
  同名不叠后缀
- 原 title 必须在 fork **之前**取：`state.session_name` 优先，缺省首条用户消息
  前 50 字（与重命名弹窗预填同口径），存 `fork_source_title`；
  `on_fork_response` 里 `set_session_name` 发到 pi 已 rebind 的新文件上
- 实测（`live_fork_probe` + PROBE_MODE=clone/RENAME）：`clone` 返回
  `{cancelled:false}`，sessionFile/sessionId 换成新分支（621 条消息整段带过去），
  `set_session_name` 后 `get_state.sessionName` 与新文件的 `session_info`
  条目都已是新名字 → 侧栏改名生效

### 顺带：render 函数违规清零（除既有 input_area）

agent 轮操作栏整体搬进 `session/actions_bar.rs`（`assistant_action_bar`），
`render_assistant_turn` 316 → 249 行；`render_msg` 已在上一步达标。
check_arch：文件行数违规 4 条（全存量）／render 函数违规仅剩既有
`input_area (367)`（我这两轮新引入的两条已消除）。

测试：app 93（新增 branch_name ×2）、pi-link 65 全绿。

### v63-7 点击选中 -1：贴底钳制改写视口顶 + 桶映射舍入；UI 微调

**点击段后选中段总在点击处上方**：非索引算错。nav_goto 置顶后，若目标靠
近会话尾部（常态——对话在末轮），下方内容不足以填满视口，gpui list 的
layout_items 向上补条目并**改写 logical_scroll_top**（贴底钳制，vendor
list.rs 878-907），视口顶落到目标轮之前——active 按「视口顶之上最近轮」
推导就错选上一段。修（符合 v63-1 定稿语义「定位后的轮次才是选中的」）：
pager Core 加 nav_pinned Cell，nav_goto 记录被点轮次，active 优先取钉住
值；物理滚轮（scroll handler is_scrolled）清除交还位置推导。
顺带修映射舍入：tick_of_turn 朴素 floor(t·k/N) 对桶起始轮（i·N mod k≠0，
如 N=45/k=20 的段 5 → 轮 11 → 段 4）会 floor 回上一桶；改桶包含式
floor((t+1)·k/N)（末端 clamp k−1）。

**UI**：gutter 右呼吸位 2→4px（gutter 16、面板锚 right 16）；选中段改弱
主题色常驻 + accent 1px 边框（不整段覆盖）；总结栏分隔线 0x40→0x80。
app 94 测试全绿。

### v63-8 段选中偏下一桶：v63-7 逆映射公式少减一

v63-7 的「桶包含式」floor((t+1)·k/N) 推导有误——恒等映射（N≤20，k=N）下
它等于 t+1，**点哪段亮下一段**（用户实测「跑下面去了」）；>20 时对部分轮
同样 +1。与正向边界 floor(i·N/k) 自洽的逆映射是
i = max{i : floor(i·N/k) ≤ t} = floor(((t+1)·k − 1)/N)（ceil(x)−1 恒等式
的整数形式，末端 clamp k−1）：
- 恒等映射：floor((t+1)N−1)/N = t ✓
- 桶起始（N=45,k=20，t=11）：floor(239/45) = 5 = turn_of_tick(5) ✓
两版错例（floor(t·k/N) 偏上一桶、floor((t+1)·k/N) 偏下一桶）都记录在案，
此为终版。app 94 测试全绿。

### v64 topbar 会话视图优化 + ⋯ 更多菜单（打开终端 / 系统 / 工具）

用户口径：① 标题左 padding 15；② 标题前 `message-square-more`；③ 标题 30 字
截断；④ 标题后 ⋯ 更多菜单；⑤ 菜单三项 = 打开终端 / 此会话系统提示词 / 此会话
加载工具；⑥⑦⑧ 图标口径（打开终端用现有；系统提示词 `file-sliders`；加载工具
`wrench`）；⑨「打开终端、系统、工具直接抄 pi-web」。

**成果 = pi-web 的三个能力，数据源与面板都对齐 pi-web。**

- **数据源（关键发现）**：pi 0.86+ 把系统提示词与工具声明**写进 transcript**：
  每次运行追加一条 `role:"system"` 消息，带 `content`（追加到基础提示词）、
  `sections`（按名打补丁，`null` 删除）、`toolsAdded`/`toolsRemoved`（声明增删）。
  pi 自己的 `agent.state.systemPrompt` 就是这份重放（pi-web 的
  `lib/exact-system-prompt.ts` 注释也这么写），而 CLI RPC **既没有
  `systemPrompt` 字段也没有 `get_tools`**（pi-web 能用 `get_tools` 是因为它把
  SDK 跑在自己的 server 进程里）。所以：
  - 新增 `crates/pi-link/src/transcript.rs`：`transcript_system(&[Value])`，
    逐行对齐 pi-ai `utils/transcript.js`（`getCurrentSystemMessage`）与
    `utils/text.js`（`getSystemMessageText`）——prompt = 非空 content 段 +
    全部存活 section 值，`\n\n` 连接；tools = 按 `toolsRemoved`/`toolsAdded`
    重放、重名替换保值（JS `Map.set` 语义）。
  - `serde_json` 开 `preserve_order`：sections 顺序 = 首次插入顺序，默认
    BTreeMap 会按字母重排（preamble/tools/rules/docs/… → 字母序），面板里就不
    是 pi 实际发送的那份提示词了。
  - `SessionRuntime` 在 get_messages 时 replay 进 `sys_prompt` /
    `session_tools`（原来的 `parse_export_html` + export_html 那条死路删除：
    数据本就在每次拉的消息里，无需导出 HTML、无临时文件、无阻塞）。
- **面板（照抄 pi-web 组件）**：新增 `crates/app/src/top_panels.rs` =
  `activeTopPanel` 机制本身——挂在 topbar 正下方的整幅面板（`top(HEIGHT)`、
  内容区宽、`bg_panel + border_b`、高 `min(600px,75dvh)`、`shadow_lg`），
  一次只开一个：
  - `TopPanel::System` ← SystemPromptPanel.tsx：单滚动区，等宽 12px / 行高 1.6 /
    muted / pre-wrap；空态文案用 pi-web 原文（「系统提示词为空（工具已禁用）」
    「系统提示词尚未加载」）。
  - `TopPanel::Tools` ← ToolDefinitionsPanel.tsx：左 `clamp(112px,26%,220px)`
    工具名列表（单选、选中 = bg-selected + 左侧 2px accent 竖条）、右详情
    （描述 / 参数：类型 + 说明书 + 必填-可选 + 可选值 + 默认值，`formatSchemaType`
    的 anyOf/oneOf/enum/array/$ref 规则逐条移植）；`param_fields` 对应
    `getToolParameterFields`。选中态存 `Chat.tool_sel`（pi-web 的
    selectedToolName）。
  - 开关 = pi-web `toggleTopPanel`（同面再点收起、换面直切）；menu 行用 check
    图标显示当前开着哪个。菜单项与面板文案按你的口径（「此会话系统提示词 /
    此会话加载工具」），面板内文案保持 pi-web 原文。
  - **打开终端**无需改动：pi-web 的 `handleOpenTerminal` = 聚焦同 cwd 的终端
    tab、否则新建并切到内容区，pi-flash 的 `open_terminal(None, …)`（v54
    内容区 tab）本来就是同构实现。
- **布局**：会话视图分支 `pl(15) + gap(7) + [message-square-more 15px]
  [标题 ≤30 字 semibold] [⋯ 22px]`，`max_w(460)`；30 字截断落在
  `Chat::session_title`。菜单入口 = 既有 `ellipsis`。
- **图标**：新增 `message-square-more.svg` / `file-sliders.svg`；`wrench.svg`
  换成你给的新 lucide 变体（**全局**，输入框「工具」pill 同步改画法）；
  曾按首版口径加的 `bot-message-square.svg` 已删（未用即删，进回收站）。
  三个新图标都登记进 `assets!()`——`every_icon_file_and_call_site_is_registered`
  先报红后转绿（正是当初「复制」图标静默消失那个坑的守卫）。
- **i18n**：新增 15 条（菜单 3 + 系统面板 2 + 工具面板 10，三语，措辞取
  pi-web 的 system.* / tools.* 译文）。
- **死代码清理**：`on_event` 里第二个 `command == "export_html" && success`
  （同一 if/else 链、永不可达）；`parse_export_html` + `html_unescape`。
- **验证**：
  - 单测：pi-link 68（+3 transcript）、app 94 全绿，`cargo check -p app` 零警告。
  - **真机一致性**：取一条真实会话（560 条消息 / 62,522 字提示词 / 26 个工具），
    我们的 replay 与 pi-ai `getCurrentSystemPrompt` + `getCurrentTools`
    **逐字节相同**（prompt 与 tools 两个文件 diff 均 empty）。探针留为
    `crates/pi-link/tests/live_transcript_probe.rs`（ignored，用法见文件头）。
  - 已确认 RPC `get_messages` 会把 system 消息原样带回（用 vendored pi 手工
    发一次 get_messages 验证：sections 顺序 preamble→tools→…→agent_browser、
    toolsAdded 26 条）。
  - `scripts/check_arch.sh`：文件行数违规仍是**存量 4 条**（main/markdown/
    messages/runtime）+ 既有 `input_area` render 违规；新增的 transcript 逻辑
    放进独立模块正是为了不把 pi-link/protocol.rs 顶过 1500 行。
- **真机待复验**：构建时 `pi-flash.exe` 正被占用（未擅自关窗），exe 未更新。

#### v64.1 浮层基类（用户：鼠标穿透 + 关不掉 + 「这是所有弹窗的规则」）

用户实测两点：① 面板显示时鼠标**穿透**到下面的内容；② 两个面板**关不掉**
（没关闭钮、点外无效）。并要求「所有弹窗都按同一套规则，为什么还没做基类」。

- 新增 `crates/app/src/ui/overlay.rs` = **浮层唯一外壳**，把此前散落 6 处的
  手写壳子收成一套规则：
  1. `layer()`：`absolute inset_0 + occlude()`（鼠标**永不穿透**；裸挂 absolute
     不 occlude 会让点击/滚轮漏进底下的消息列表——`input.rs` 胶囊、
     `psp_overlays` 都单独踩过这个坑，现在统一由基座保证）；
  2. 点浮层外任意处关闭；
  3. ESC 关闭（`track_focus` 到 `chat.dialog_focus`，由调用方传 handle）；
  4. `stop_click()`：卡片套一层，点卡片本身不关；
  5. `close_btn()` / `panel_header()`：**看得见的 × 关闭钮**（22px，hover 变亮）。
  `dismiss` 要求 `Clone` —— 它同时挂在外点与 ESC 两个 handler 上。
  文件头写明了**两处故意例外**：`/` `@` 补全菜单（composer 的补全 UI，ESC 语义
  是「取消补全」，点外收起故意让点击继续落到下层）与悬停提示（tooltip/hover 卡，
  由 hover 状态机收起）。
- 改造接入基座（原先各自手写「遮挡 + 点外 + ESC」的地方）：
  - `dialogs.rs::dialog_shell`（5 个弹窗：模型选择 / Git 差异 / 会话搜索 /
    图片预览 / ……）—— 图片预览原来**没有关闭钮**，补右上角 ×（规则 5）；
  - `settings/mod.rs` 设置面板；
  - `ext_ui.rs` 扩展弹窗（外点/ESC 都等于「取消」）；
  - `function_panel/psp_overlays.rs`：psp ⋯ 菜单层 + 删除项目确认层（顺带得到
    ESC 关闭；确认层需要焦点，签名加 `focus` 参数）；
  - `main.rs` 胶囊下拉菜单层；
  - `top_panels.rs` 两个面板（见下）。
- **top panel 重做外壳**：从「content-col 里裸挂 absolute 的卡片」改为
  `overlay::layer` + 卡片 → 鼠标不再穿透、点侧栏/内容区任意处关闭、ESC 关闭，
  头部件加标题（系统提示词 / 工具定义）+ × 关闭钮（pi-web 那边靠 topbar 按钮的
  active 态表示，这里按用户口径补可见关闭钮）。挂载点从 content-col 移到 **root**
  （浮层要盖住侧栏才能做到「点外部关」），浮层从 topbar 下缘起铺 —— 窗口控制钮
  与设置钮不被吞，卡片左缘跟 `slp_w`（侧栏收起 = 0）和内容区对齐。
- ESC 的焦点前提：composer 的 escape 会 `stop_propagation`，抢不到 —— 焦点策略
  链（`Chat::render`）补一支 `else if self.top_panel.is_some()` → 把焦点交给
  `dialog_focus`（与 dialog/settings 同款），面板关闭后自动回落 composer。
- i18n：+2（系统提示词 / 工具定义）。
- 验证：app 94 + pi-link 68 全绿，`cargo check` 零警告；check_arch 违规仍为存量
  4 文件 + 既有 `input_area`。真机待复验（exe 被运行中的 pi-flash 占用）。

#### v64.2 两个面板改用设置弹窗窗体（用户：不要发明新窗体）

用户口径：系统和工具**直接用 settings 弹窗**那套窗体，删掉之前那幅「遮挡半屏
的弹窗」，不要自造。

- 删除：v64/v64.1 那幅挂在 topbar 之下的整幅面板（`top_panels::top_panel`
  + `Chat.top_panel` 字段 + root 挂载 + 焦点策略链里那支 + `panel_height` /
  `sidebar_width` 计算）。
- 抽出 **设置弹窗窗框** `ui::overlay::big_card(title, nav, body, t, close)`：
  0.7×0.98 卡片、`rounded(10)`、chrome 36px 顶条（左标题、右 × 红 hover）、
  可选左导航列（200px，nav 底 + 左下倒角 + 右边线）——**设置弹窗自己改用它**，
  所以是「同一套窗体」，不是我另画一个。设置弹窗的标题传空串（它用左导航当
  身份），两个面板传「系统提示词 / 工具定义」。
- 两个面板回归 `Dialog::SessionInfo { kind }`（与其它弹窗同一条挂载/关闭链路）：
  - 系统提示词：无左导航，正文走 mc-body 同款内边距（pt22/pl30/pr30/pb30）+ 滚动；
  - 工具定义：左导航位 = 工具名列表（设置弹窗 nav_items 同款行样式），右侧 =
    描述 / 参数详情；
  - ⋯ 菜单项 = `open_session_info`（同面再点收起）+ 打勾态 `session_info_open`。
- 浮层三条全局规则仍由 `ui::overlay::layer` 统一提供（遮挡不穿透 / 点外关闭 /
  ESC 关闭），窗框自带 ×（规则 5）。
- 验证：app 94 全绿、零警告；check_arch 违规仍为存量 4 文件 + 既有 input_area；
  exe 已重建并启动（真机复验）。
