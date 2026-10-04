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
