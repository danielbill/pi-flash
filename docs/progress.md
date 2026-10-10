# pi-flash 进度记录

> 唯一进度台账（AGENTS.md 只留铁律与路径）。**维护规则**：
> ① 每轮工作更新「当前状态」，并在「里程碑索引」顶部加**一条一行**纪要；
> ② 实现细节不进本文件——commit message 即详录（本仓库惯例：提交信息自带
>    根因/方案/验证），检索用 `git log --oneline --grep=<关键词>`；
> ③ 陷阱与用户定案沉淀进下方「持久参考」两节，长期保留、随踩随补。
>
> 2026-10-10 压缩：2716 行 → 现行规模。详版全文在 git 历史——压缩提交的
> 父提交是最后全量版（`git log -p -- docs/progress.md`）。旧文件「当前状态
> （2025-09，…）」两节的 2025 日期为早期 agent 幻觉，已随压缩删除——
> **项目实际始于 2026-09-24**（首次提交 80d80d2）。

## 当前状态（2026-10-10）

- workspace：`crates/pi-link`（协议层，127+3 测试）+ `crates/app`（GPUI 界面，
  ~173 测试）+ `vendor/pi`（**钉版 1.1.0**，2026-10-10）；gpui 0.2.2 vendored。
- 发版：**v0.1.0 已上线**（GitHub Releases 三资产 + npm `pi-flash@0.1.0`）；
  发版操作唯一路径 `docs/RELEASE.md`；OIDC trusted publishing 已就绪，待
  npmjs 配 Trusted Publisher 后发版不碰 npm。
- 功能已通：流式聊天/steer/中断/图片、富渲染（KaTeX/mermaid/raw HTML/表格）、
  工具卡片与会话分支（fork/clone + agent 轮锚点）、模型/思考 7 档/工具预设、
  @ 文件检索、! shell、内置终端、文件树（Zed 移植）+ 文件编辑器、git 面板、
  设置页签×6（「远程控制」页代码保留、入口已隐藏）、三语 i18n、七主题、
  pif-ui UI 自动化测试链路。
- 待用户实机验收：pi 1.1.0 升级、065 拖选手感、080 发布链真机复验。
- 主线文档：`docs/PORT_PLAN.md`（复刻计划）、`docs/模块设计/*`（分模块设计）、
  `docs/bugs.md`（bug 追踪）。beads 清空，新工作 `bd create`。

## 里程碑索引（新 → 旧；一行一条，细节看对应提交）

- 2026-10-10 **060 远程控制入口隐藏**（用户定夺）：topbar 手机钮删除 + 设置
  「远程控制」页签撤出导航；060 管线/扫码弹窗/自动化 WX_QR_OPEN 原样保留，
  重开 = 导航数组加回一行（或 git revert）。

- 2026-10-10 **vendor pi 1.0.0→1.1.0**（eeaa351）：RPC 面纯增量；22 命令名 +
  CORE_TOOLS 核对无缺口；get_commands 仍无内置命令；live 探针 488 模型同形。
- 2026-10-10 **080-2 发版收口**：v0.1.0 全链路上线；publish-npm.yml（OIDC）；
  RELEASE.md 规范入库；`[perf]` 门控/CHANGELOG 路径/pathspec 提交三坑修入铁律。
- 2026-10-10 **080 软件分发**（xnl）：npm 薄壳 + GH Releases 载荷；esbuild 裁剪
  284MB→下载 ~120MB；e2e 19/19（安装/自愈/SUMS 兜底/更新同一条命令）。
- 2026-10-10 **064/065 编辑卡顿治理**（zr8）：根因 = paint 尾无条件 notify 连坐 +
  syntect 零缓存；LRU 128（280 行块 92ms→0.017ms）+ notify 门控 + PI_FLASH_PERF
  仪器；选区改逐行 quad 去 lyon 曲面细分（对齐 Zed）。
- 2026-10-08 **062/063 大文件提速**（5hf/5z6）：已开 tab 判重提前；set_value
  专用 reset 路径 + TextWrapper 零物化（4.6MB 152→26ms）；>256KB tree-sitter
  全量 parse 后台化 + 纪元作废。
- 2026-10-07 **031 @检索 + !shell**（s49）：pi-web file-fuzzy 移植；bash RPC +
  乐观卡 + 增量渲染；get_messages 携带 bashExecution 的去重。
- 2026-10-07 **034 档位改版**：四条定稿（见定案节）；full = `-ne`+显式 `-e`
  结构性零注入；`pi_link::extensions`/`session_ext`；full+plugin 选择面板。
- 2026-10-07 **023 文件编辑展示页**：CodeEditor 底座 + topbar 标签混排 + 外部
  改动检测 + 面包屑；废弃 GitDiff 旧入口。
- 2026-10-07 **pif-ui 自动化框架**（2kq）+ v1.1 六修：快照/操作/wait/keys，
  隔离实例；误删活登记判死收紧、--arg 转义、input.focus、wait 三断言。
- 2026-10-06 **010 启动装载**：`~/.pi-flash` 自有目录 + 迁移；磁盘目录层
  `catalog.rs`（无进程冷启动可用）；startup.rs 收拢后台任务与闸门。
- 2026-10-06 **012 新会话页**（1bk）+ **004 打开项目菜单**（fresh 语义：新会话页
  选项目 = 强制新草稿，不恢复旧会话）。
- 2026-10-06 **文件树 Zed 移植**：96 图标主题 + natural_sort + gitignore 压栈 +
  vlist 虚拟化 + watcher 去抖；auto_fold 链式折叠；vlist 句柄每帧 new 修复。
- 2026-10-05 **fork 三层根因修复**：serde_json 128 层递归上限吃掉 get_tree →
  `parse_value`；pi 侧深树溢出 → `get_entries` 扁平锚点；`follow_session_file`
  统一 draft/fork/clone 跟随。
- 2026-10-05 **「新分支」改挂 agent 轮**（用户定案）+ 尾部走 clone + 分支自动
  改名（截 20 字）。
- 2026-10-04~05 **v63 会话导航**：刻度条（轮次均分切条 + 20 上限百分比桶，
  逆映射公式两版修正终定）+ 400px 悬浮摘要面板 + 虚拟化 + 总结栏。
- 2026-10-04 **v59 滚屏七修定稿**：`chat_list.rs` 收拢全部滚屏状态；钉顶垫片
  常数化（与测量精度解耦）；快流判定抖动回归锁；PAD_BOTTOM=180。
- 2026-10-04 **v61/v62 性能与字号**：LineLayoutCache 存档层 + markdown 解析
  缓存 + 图片解码缓存；字号改绝对像素差体系；StyledText 字号只认容器继承的
  根因修复（sized_text）。
- 2026-10-04 **设置改版 + v60 最近会话**：主题持久化去污染（app_settings.json，
  严禁写 pi settings.json）；Dropdown 组件；加载改时间窗口 + 每组 10 条分页。
- 2026-10-04 **思考 7 档补全**；模型目录上收 Chat 层项目级共享 + 草稿 lazy
  connect；agent 输出样式对齐 pi-web（间距/margin 折叠/strong 700/JetBrains
  Mono 打包/VS Dark+ 高亮）。
- 2026-10-03 **模型弹窗六项** + Enter 双重租约 defer 修复 + set_model 异步
  响应回填；**PI_FLASH_RPC_LOG** 线缆日志；**vendor 0.87.1→1.0.0**。
- 2026-10-02 **v56 消息渲染对齐 pi-web**（48u，30 commits）：轮结构/工作详情组/
  工具卡/diff 双栏/token 估算/compaction；**v57** KaTeX（RaTeX）/mermaid
  （mermaid-rs）/raw HTML（scraper 安全子集）；实测五修（-ne 隔离、pill_anchor、
  粘贴崩溃两连、长文本折行）。
- 2026-09-30 **v54 主界面按设计稿全量重构**（布局骨架/psp 一体列表/composer
  胶囊/消息区/设置弹窗/七主题）；收尾验收；10-01 v54.6 IME panic 修复 +
  v54.7 主题一致性。
- 2026-09-24~28 **M1-M6 PORT_PLAN 全交付**：pi-link 协议层；分支导航；文件树；
  git 面板；内置终端（alacritty 0.26 + ConPTY）；模型/Provider 面板；插件/技能/
  工具面板；扩展 UI 协议；子代理面板；主题运行时切换；i18n 三语；LLM 生成
  标题；check_arch 收官（a1bfff0/b68faca，09-27）。
- 2026-09-24 **项目起点**：GPUI smoke + pi rpc 桥接 spike（80d80d2）→ 复刻
  pi-web 策略 + 钉版 vendor 决策 + PORT_PLAN（d0c4a87）→ M1 骨架 workspace +
  pi-link（typed RPC + fixture 测试）+ vendor 0.87.1 + markdown 渲染（0b7a9c2）。

## 产品定案与已知行为（勿当 bug 修）

- **滚屏**（pi-web useAgentSession 语义）：发言翻页 = 用户消息钉视口顶；
  「回复只到屏幕中央 + 下方空白」= 钉顶态本身（垫片是真实列表条目）；
  「一滑掉屏底、空白消失」= 滚轮退役锚点（pi-web 同款）。均非 bug。
- **fork**：分支点挂 **agent 轮**操作栏（语义 = 保留到本轮回复为止）；尾部无
  下一条用户消息时走 `clone`；分支自动改名 = 截 20 字不加后缀；用户消息栏
  只剩 复制/编辑。
- **档位四条定稿**：full 禁任何插件注入（个人扩展照常）；自定义 = full + 插件
  清单；对话中途不可改（运行中工具预设置灰）；每会话保存一份；默认 = 自定义；
  configured 档已删。
- **扩展装载**：主会话默认加载扩展（`load_extensions=true`，v70.3 拍板，
  pi-web parity）；`-ne` 隔离降级为设置页逃生口。
- **composer 控件分类**：工具预设 = spawn 参数 → 运行中禁换；思考强度/模型 =
  实时 RPC → 随时可换；fork 与压缩运行中均禁。
- **渲染规则**：用户气泡不渲染 HTML（发出内容凭证）；用户消息时间戳
  「9月17日 16:20」hover 淡入；无计费行、无「已复制」反馈；流式当轮平铺
  渲染（isLiveTail），轮末才折叠「工作详情」。
- **psp 加载**：时间窗口 7/14/30 天 + 每组 10 条「显示更多」分页（zcode 式）；
  窗口即归档——显示层隐藏，数据不删。
- **其它**：md 默认打开源码态（用户二改）；html 文件走系统浏览器（wry 内嵌
  三轮失败定案绕行）；主题持久化只写 app_settings.json；模型弹窗短名/思考
  off = 预期（非推理模型）；新会话页判据 = isEmptyNew。

## 持久参考：陷阱库

### pi 协议（wire，AGENTS.md 摘要版的扩集）

- 内容块 camelCase `toolCall`；流式 args 起始 `partialJson`；工具结果
  `role:"toolResult"` 独立回灌（按 toolCallId 挂回卡片）；`set_model` 字段是
  `modelId`；client 不得硬编码 `--no-session`（压掉 `--session`）。
- 无 `navigate_tree`/`systemPrompt`/`get_tools`（后者是 pi-web 进程内 SDK 专属）。
  系统提示词与工具声明在 transcript 的 `role:"system"` 消息里（content 追加 +
  sections 补丁 + toolsAdded/Removed），重放对齐 pi-ai `utils/transcript.js`；
  serde_json 开 `preserve_order` 保 sections 顺序。turn_start 后的 system
  message 全量携带 transcript 补丁（app 靠它 live 重放）；1.1.0 起多 `toolsAdded`
  键、`agent_settled` 多 `aborted` 字段。
- `get_tree` 深树双炸：客户端 serde_json 默认 128 层递归上限（→ `parse_value`
  disable_recursion_limit + reader 线程 16MB 栈）；pi 侧超长会话自己
  `Maximum call stack`。fork 锚点正解 = `get_entries` 扁平列表走 parentId。
- `get_commands` 只回扩展 + skill 命令，**无内置**（1.0.0/1.1.0 实测，vendor
  升级时复核 catalog.rs）；`extension_ui_response` 的 id 必须是请求 id
  （pi 在 raw-line 层按 id 关联）；RPC 模式不支持 custom 扩展 UI。
- `get_messages` 快照携带 bashExecution（与乐观卡并轨会双卡）；`set_model`
  异步：立刻 get_state 拿旧值，响应 data 里才有切换后的完整模型对象。
- `-ne` 只关扩展，个人 skills 照旧（关要 `-ns/-np/--no-themes`）；`--tools`
  是注册级硬 allowlist（连 getAllTools 都挡）；重复 `-ne` 无害。

### gpui 0.2.2 vendored / gpui-component

- `overflow_y_scroll`/`track_scroll` 只在 `Stateful<Div>`（先 `.id()`）；
  `visible_on_hover` 不存在 → `.group()` + `group_hover`。
- `list()` 虚拟列表条目**只认上下 padding**，左右无效 → 外层容器包 `.px()`；
  item 宽度语义不可靠（psp 弃用改全量 div）。
- `uniform_list` 契约 = 容器 fixed/max 高（Fill 需 relative+flex_1 外包 +
  absolute 定死四角，否则画全部内容）；**UniformListScrollHandle 必须跨帧
  复用**（每帧 `new()` = 滚动全丢）→ `ui::vlist` thread_local 句柄表按 id 复用。
- overlay 必须 `absolute().inset_0()` + `occlude()`（防 0 高容器裁掉子元素 /
  鼠标穿透）；但 `occlude()` 会**截断祖先 hitbox**（hit_test 从后往前 break）
  → 行级 hover 失效、滚轮困死——消息气泡禁止加 occlude。
- 滚轮派发给**所有**命中可滚动 hitbox 含祖先（嵌套 = 内外同时滚，非先内后外）。
- 滚屏铁律：Bottom 对齐下内容填不满视口 → 强制 logical=None 贴底（钉顶必须
  靠 spacer 垫片，常数化最稳）；`ListState::reset()` 清滚动位（notify 前存、
  后恢复）；`scroll_to*` 不触发 notify（程序化滚动自补通知 + window.refresh）；
  滚动回调内**严禁**触碰 ListState（BorrowMutError → 置标记渲染帧补账）。
- hover 事件按绘制逆序派发：向下移动时上一行 leave 晚于下一行 enter →
  enter-only 置位。
- **StyledText runs 的 font_size/font_weight 无效**（shape_text 只认容器继承
  链）→ 字号/加粗挂容器 div（markdown sized_text/weight 参数即此）。
- `WindowBounds::Maximized` 创建路径无效 → `zoom_window()`；gpui 无逐元素
  backdrop blur → 遮罩用 bg 色 80% 透明近似柔雾。
- 字节/字符边界：gpui highlight/切片 range 按字节且必须落字符边界（粘贴
  panic 0xc0000409 两连的根源）。
- 双重租约：事件派发期间同步 update/drop 正被派发的实体 → `cx.defer`；
  自动化 keys 同理（INPUT_KEYS 走 window.defer）。
- `icon_hover` 自定义元素在 list 虚拟条目里占位不绘制 → 用 `icon()`/
  `icon_current`；svg 未登记进 `assets!()` 会**静默画空白**（有双向一致性
  测试守卫）。
- `window.update`/`chat.update` 不 flatten（catch_unwind 四层嵌套）；vendored
  `WeakEntity::update` 签名与 Zed 上游不同，别照抄；渲染期读取的实体会注册
  window invalidator（InputState notify 即重渲染）。
- 杂项：prepaint/paint 第 5 参是 prepaint state 非 hitbox；Pixels 字段私有
  （f32::from/除法）；move 闭包捕获 `&WeakEntity` 参数 E0521（闭包外 clone）；
  `unbounded()` 返回值不能别解构。

### 工程/工具链

- python 脚本整文件重写会把 CRLF 翻成 LF：改前探 `\r`、写回保原行尾；子串
  替换锚点带行首 `\n` 防缩进误匹配（AGENTS.md：大补丁用脚本文件，勿 heredoc）。
- gpui-component Input `set_value` 全文走 utf16 编辑管线（4.6MB ≈48ms）→
  大文本走 vendor state.rs 的 reset 路径（换 Rope + wrapper.reset）。
- Windows 路径：`Path::components()` RootDir 保留原始分隔符（不可做路径 key）；
  盘符大小写双写（`d:\` vs `D:\`）跨写方真实存在 → 归一化键小写化
  （pi-link `path_key`）。
- 深递归 JSON 测试（deep_tree）在 Windows 默认测试栈溢出 → `RUST_MIN_STACK`
  ≥32MB（bead 在案）。
- UI 自动化断言交互 bug 必须走**真实按键链**（`input.keys`）：直调 setter
  测不出 on_change 类缺陷（菜单 latch 泄漏即漏测案例）。

## 截图索引（tmp/屏幕截图/）

| 文件 | 内容 |
|---|---|
| 图标版-全貌.png | SVG 图标版主界面（当前）|
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
