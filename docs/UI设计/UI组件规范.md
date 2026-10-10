# PF UI 组件规范

> 定稿：2026-10-09。适用范围：`crates/app` 全部界面。
> 本文是**唯一尺寸来源**：界面里出现的一切高度 / 内边距 / 间距 / 图标 / 圆角 / 字号，必须能对到本文的令牌上；对不上的视为 bug。
>
> 配色不在本文范围——色板语义见 `crates/app/src/theme.rs`（其头部注释即色彩规则）。

---

## 0. 参照体系（为什么是这些数）

规范不拍脑袋，按下表四家收敛。**首选参照是 Zed**：同为 GPUI、同为桌面开发者工具、本项目本就以 Zed 为性能原型，其 `crates/ui` 的令牌直接实测提取；Fluent 2（Windows 平台规范）与 Material 3 / GitHub Primer / shadcn 用于交叉校验。

| 维度 | Zed（实测源码） | Fluent 2 | Material 3 | Primer / shadcn | **PF 取值** |
|---|---|---|---|---|---|
| 间距网格 | 2/4/6/8/12/16/20/24/32 | 4px 基网格：2/4/8/12/16/20/24/32/40/48 | 4dp 基网格 | 4/8/16/24 | **2px 半步进，≥12 走 4 的倍数** |
| 按钮高度 | 18/22/28/32 | 24/32/40 | 40 | shadcn 32/36/40 | **24 / 28 / 32** |
| 图标 | 10/12/14/16/48（菜单默认 14） | 12/16/20/24 | 20（密）/24 | Primer 16/24；shadcn 16 | **10/12/14/16/18** |
| 圆角 | 2/4/6/8/12/16/24 | 2/4/8 | — | shadcn 4/8/12 | **3/6/8/10/12** |
| Switch | 32×20，knob 12 | 40×20，thumb 12 | 52×32（触屏） | — | **28×16，knob 10**（小档 24×14） |
| Checkbox | 16，r2，勾 14 | 20 | — | Primer 16 | **14，r3，勾 10**（12px 密度取小一档） |
| 弹窗 | 宽 544/640，r8，标题 16 | r8 | — | shadcn max-w 512，r8 | **宽 380/500/620/760，r10** |
| 菜单 | r8，min-w 200，图标 14 | — | — | shadcn 行 py6 | **r8，p4，行 px10·py7，图标 14** |
| 滚动条 | 宽 6，悬停加宽 | — | — | — | **宽 6→10（与 Zed 相同）** |
| UI 字号 | 10/12/14/16（默认 14） | — | — | VS Code 13 一档 | **10/11/12/13/14（默认 12）** |

密度取向说明：PF 基准字号 12px（用户可调 10–17），比 Zed（14）/VS Code（13）密一档，但控件高度取 28 档而不是 Zed 的 22 档——留呼吸感，也是 pi-web 原型的既定观感。触屏目标尺寸（M3 的 48dp、7–10mm）不适用：PF 是键鼠桌面应用。

---

## 1. 间距体系

所有 `padding / gap / margin` 必须取自下表（单位 px）：

| 令牌 | 值 | 用途 |
|---|---|---|
| SP1 | 2 | 微调：开关滑块内缩、徽标描边外隙 |
| SP2 | 4 | 图标组间隙、菜单容器内衬、行内紧凑 gap |
| SP3 | 6 | 紧凑 gap、卡片行内 padding、分隔线行距 |
| SP4 | 8 | **标准 gap**（图标↔文字、控件↔控件）、卡片内 padding |
| SP5 | 10 | **列表/菜单行水平 padding**、行内稍宽 gap |
| SP6 | 12 | hover 卡 / 工具卡内 padding、表单行距 |
| SP7 | 16 | 弹窗内 padding、区块间距 |
| SP8 | 20 | 大区块间距、分栏 padding |
| SP9 | 24 | 页级区块间距 |
| SP10 | 32 | 容器级留白 |

规则：
- **2px 步进**；**≥12 后必须是 4 的倍数**（12/16/20/24/28/32）。
- 禁止 5/7/9/11/13/15 及一切 `.5` 步进。存量 `py(7)`、`gap(9)`、`px(3.5)` 等按就近收敛（7→6 或 8，9→8，3.5→4）。
- 行高（列表行、按钮、顶栏）是**控件高度**，走 §5 的档位表，不受 4 倍数约束（30、33 等合法）。

场景映射（现状已达标，写下来防止回退）：

| 场景 | 值 |
|---|---|
| 图标↔文字（行/菜单/按钮内） | gap 8 |
| 卡片（工具卡/气泡/hover 卡） | px10 py6～p12 |
| 列表行水平 padding | px10 |
| 文件树缩进 | 20/级 |
| 表单行距 / 区块间距 | 12 / 20 |
| 设置页 body | pt22 px30 pb30（大卡特例，保留） |

---

## 2. 字号体系

沿用已定稿的双轨模型（详见 `docs/UI设计/字体大小设置.md`），本文只固定档位语义：

**面板轨 `ui_size(base)`**（随"界面字号"设置缩放，设置值钳 10–20，档位 小15/中16/大17/特大18）：

| 档 | 值（默认设置下） | 用途 |
|---|---|---|
| caption | ui(10) | 徽标、scope 标签、stat 标签 |
| secondary | ui(11) | 辅助说明、时间列、git 面板正文、footer |
| **body** | **ui(12)** | 面板正文、按钮、输入框、菜单行、设置正文（基准档） |
| emphasized | ui(13) | 设置导航、富菜单主行、列表强调行 |
| title | ui(14) | 弹窗标题、pill 菜单、选中行主文字 |

**会话轨 `sess_size(delta)`**：正文 0、操作栏 −1、气泡/工具卡 −2、bash/meta −3；markdown 内部规则不变。

禁止：
- 绕过双轨的裸 `px(n)` 字号、tailwind `text_sm/text_xs`（存量 11 处待清，见 §9）。
- markdown / 终端 / TextInput 的内部字号是**渲染特例**，不经 ui_size，但值必须按本文档记载的固定值来（markdown 表头 11 等）。

**终端（派生特例）**：字体 = JetBrains Mono（全应用唯一内置 mono，Regular/SemiBold/Bold/Italic 四字重，终端显式关 calt 连字保持 xterm 语义）；字号 = 面板字号（档 14/15/16/17 即终端值），行高 1.25×；面板字号变化后终端下一帧自动重测 cell 并重排（含 PTY resize），无独立设置项。

---

## 3. 图标体系

入口 `ui::icon(name, size, color)`。size 只允许 5 档：

| 令牌 | px | 用途 | 禁止用途 |
|---|---|---|---|
| ICON_INDICATOR | 10 | checkbox 勾、状态点、行内 meta 小图标 | — |
| ICON_XS | 12 | 行内关闭 ×、工具卡小图标、面包屑 chevron | — |
| ICON_SM（默认） | 14 | **菜单项、列表行、按钮内、文件树** | — |
| ICON_MD | 16 | 独立强调图标、statusbar tab、空态图标 | — |
| ICON_LG | 18 | 图标按钮内部（psp 行操作钮、topbar 图标钮） | — |

规则：
- 图标↔文字 gap 8（SP4）；图标在按钮内默认 ICON_SM，图标按钮走 §5.2 的方形槽。
- 关闭 × 全项目统一 ICON_XS（12）——存量 8/11/13 三种杂值收敛掉。
- **「更多」图标全项目唯一 = lucide circle-ellipsis（带圈三点）**，横/竖 ellipsis 两枚已废除（2026-10-10 定夺）；禁止再引入第二种 more 变体。
- iconfont「新建会话」19px 等**品牌特例**仅限 startup logo（88）与该处，不得扩散。
- icon hover 动效（scale 1.06 + 上抬 1px）按 v55 既有规格，见 `docs/UI设计/主界面UI设计说明.md` §14。

---

## 4. 圆角体系

| 令牌 | px | 用途 |
|---|---|---|
| R_XS | 3 | checkbox、徽标、高亮词底 |
| R_CTRL（默认控件） | 6 | **一切按钮、输入框、图标按钮、菜单行、tab** |
| R_CARD | 8 | 菜单/弹层容器、会话/项目列表行、面板内卡片 |
| R_MODAL | 10 | 弹窗卡片（含大卡顶条与整体） |
| R_SHEET | 12 | 大卡片（设置/系统提示词）、hover 浮卡 |
| R_PILL | 16 / full | composer 胶囊、pill、开关、圆点（rounded_full） |

规则：同语义同值。"弹窗圆角"只有一个 10（存量 8/10/12 三档收敛：菜单壳 8、标准弹窗 10、大卡 12——这是三条语义，不是三档随机）。窗口装饰（CSD）10px 是系统特例。

---

## 5. 控件规范

### 5.1 文字按钮

三种高度档，变体三种色：

| 尺寸 | 高 | 水平 padding | 字号 | 圆角 |
|---|---|---|---|---|
| BTN_SM | 24 | 8 | ui(11) | 6 |
| BTN_MD（默认） | 28 | 12 | ui(12) | 6 |
| BTN_LG | 32 | 14 | ui(12) | 6 |

| 变体 | 底 | 文字 | 边框 | hover |
|---|---|---|---|---|
| Primary | accent | accent_contrast | accent | accent_hover |
| Secondary | 透明 | text | **t.border（1px）** | bg_hover |
| Danger | 透明 | t.danger | danger_alpha(t, 0x59)（35%） | danger_wash()（红 12%） |

规则：
- 全项目只有这一张按钮表。存量 6 种文字按钮组合（overlay confirm / dialogs / git 小钮 / psp ghost 钮…）全部向此收敛：git 面板小钮 → BTN_SM；弹窗按钮 → BTN_MD；`rounded(7)` 文字钮 → 6。
- Danger 三套红（widgets hsla 系 / overlay t.danger 系 / psp danger_alpha 系）**只保留 theme.rs 语义系**：`t.danger` 文字 + `danger_alpha(0x59)` 边 + `danger_wash()` 底。硬编码 0xd8626a 仅允许出现在窗口关闭钮（OS 惯例特例）。
- 弹窗按钮排布：右对齐、gap 8、次要（取消）在左、主要（确认）在右。危险确认钮用 Danger 变体，rest 底 6% → hover 12%。

### 5.2 图标按钮

方形、无边框，圆角 6，hover = bg_hover：

| 尺寸 | 方形槽 | 内图标 | 用途 |
|---|---|---|---|
| IB_SM | 22 | 14 | tab 关闭 ×、行内 ⋯ 触发钮 |
| IB_MD（默认） | 28 | 16 | 行操作钮、弹窗关闭 × |
| IB_LG | 32 | 18 | topbar、psp 标题行操作 |

特例（保留）：窗口控制钮 42×38 无圆角（OS 惯例）；composer 发送/停止 36×36 圆角 16（运行中收成 9）；大卡关闭钮 30×30 红底（破坏性区域语义）；topbar/statusbar 高 38（chrome 层，2026-10-10 定夺 36→38；两条 bar 上沿均不画 border_t，激活 tab 上沿不画线只留左右分隔）。

### 5.3 开关 Switch

| 尺寸 | 轨道 | 圆角 | knob | 位置（关/开） |
|---|---|---|---|---|
| SW_MD（默认） | 28×16 | 8（半高） | 10 | ml2 / ml14 |
| SW_SM | 24×14 | 7 | 8 | ml2 / ml10 |

- 选中 = accent 底 + bg 色 knob；未选 = bg_selected 底 + border；禁用整体 opacity 0.5。
- 与 Zed（32×20/knob12）、Fluent（40×20/thumb12）同族，按 12px 密度再收半档（2026-10-09 定夺）。存量 config_switch 32×18、misc 34×19、组头 24×14 全部收敛到 28×16（`settings/widgets.rs::switch_el` 为唯一画法，组头行内开关同尺寸）。SW_SM 24×14 为**预留档**（当前无消费方；需要更小开关必须用它，禁止自造第三种尺寸）。状态直切、无动画——GPUI 现状即无插值，规范不虚构动画。

### 5.4 复选框 / 单选

| 部件 | 值 |
|---|---|
| 方框 | 14×14，圆角 3，边框 1px（选中 accent 底，未选 border） |
| 勾 | icon CHECK @ ICON_INDICATOR(10)，色 **accent_contrast**（统一；清掉 mcp/plugin picker 里硬编码 0xffffff） |
| 单选 | 14 圆圈 + 6 内点，选中描边 accent |
| 选项/分项文字 | 统一 `text_muted`，**选中只加粗（SEMIBOLD）不变色**；分项标签比节标题小一档（ui12 常规，节标题 ui13 semibold），层级靠字号字重表达（2026-10-10 定夺） |

### 5.5 输入框

| 项 | 值 |
|---|---|
| 单行高 | 30 |
| 多行高 | 170（约 8 行，可拖） |
| 圆角 | 6（R_CTRL） |
| 边框 | 1px；未焦 = t.border，**聚焦 = accent**（不做双圈 focus ring） |
| 底色 | bg_panel |
| 字号 | ui(12)（随面板字号缩放；清掉现在硬编码的 px(12)） |
| placeholder | text_faint |

composer 胶囊（rounded16、pt10 pb12、AutoGrow 3–10 行）为既有特例，内部行框契约不变。

### 5.6 菜单 / 下拉弹层

| 部件 | 值 |
|---|---|
| 容器 | min_w 200，p 4，圆角 8，border 1px，shadow_lg，max_h 75% 视口 |
| 行 | 水平 padding 10，垂直 padding 7，图标↔文字 gap 8 |
| 行图标 | ICON_SM(14)（存量 13/15 并存收敛到 14） |
| 行文字 | ui(12)；副行/描述 ui(11) text_faint |
| 快捷键/右注 | ui(11) text_faint，`ml_auto` 右对齐 |
| 分隔线 | 1px t.border，上下留白 6 |
| 子菜单 | 沿父容器右侧 +4 偏移，行对行 |

锚定与防抖沿用 `ui/dropdown.rs`（弹层 mt4、窗口收口 margin 8、防抖 300ms），不在本文重复。

### 5.7 弹窗

| 部件 | 值 |
|---|---|
| 遮罩 | 黑 35%（模态）；非遮挡菜单 dim=false 走 `ui/overlay.rs::layer` |
| 宽度档 | **380 / 500 / 620 / 760**；大卡 = 相对 0.7×0.98 |
| 内 padding | 16（存量 12/14/18 收敛到 16；大卡 body px30 保留） |
| 圆角 | 10 |
| 标题 | ui(14) Semibold；标题行高 36 + 关闭钮 IB_SM/ICON_XS |
| 按钮行 | 见 §5.1：右对齐 gap 8，mt 16 |

宽度档映射：小确认 380（存量 260/300 收敛）；表单/中型 500；模型选择/会话搜索 620；diff/大查看器 760；项目选择维持 500×500。

### 5.8 列表行

| 列表类型 | 行高 | 选中态 | hover 态 |
|---|---|---|---|
| 紧凑（文件树） | 24 | 无 | bg_hover |
| 标准（设置侧栏、picker、字体列表） | 30 | bg_selected + Semibold | bg_hover |
| 主列表（psp 会话行） | 32 | bg_selected + Semibold | bg_selected 同色（更浅不变） |
| 弹窗选择行（项目选择） | 40 | 行尾 check | bg_hover |

- **选中态统一 bg_selected**：psp 会话行现用的 `rgba(text,0x1a)` 中性薄纱与 bg_selected 视觉几乎等效，收敛为 token，消除特例。
- 项目行 33（=32+mb2 补偿）改回真行高 32，用 mb 补间距的写法废除。
- 行内结构：pl10 pr10 gap8；图标 ICON_SM(14/15→统一)；时间/数字列定宽右对齐。

### 5.9 滚动条

`ui/psp_scrollbar.rs` 既有常量即标准：thumb 宽 6、内缩 4、hover/拖拽 10、min thumb 25、最大不透明 0.7、autohide 3s + 1s 淡出、全圆角。**容器统一 w10**（存量 w8/w12 收敛）。

---

## 6. 颜色使用规则（交互态）

只引用 theme.rs 语义 token，禁止裸 hex（除 §5.2 特例与 bucket_color 固定类别色）：

| 语义 | token |
|---|---|
| hover | bg_hover |
| selected | bg_selected（列表/菜单选中） |
| 主色底 | accent / accent_hover |
| 破坏性 | danger 系（见 §5.1，唯一表达） |
| 占位/最弱文字 | text_faint |
| 通知点三色 | NOTICE(0x3AA6D0 青蓝=完成) / WARN(0xFACC15 黄=内部中断) / danger(红=外部错误)——语义见 §8 会话状态槽 |
| 工具卡/代码底 | tool_bg（bg ±3%） |
| 细微洗色 | bg_subtle |

主题一致性不变量（border 对比、hover<selected 亮度递进、text 五级阶梯单调）由 `theme.rs::semantic_ladder_holds_for_all_themes` 测试守护，新增主题必须过该测试。

---

## 7. 对齐规则

- 图标与文字一律垂直居中（items_center），禁止基线对齐。
- 表单：标签左对齐；标签↔控件 gap 8，控件间 gap 8，行距 12，区块间距 20。控件列起点对齐（不同表单页同一列宽节奏）。
- 列表：行内左对齐起于 pl10；时间/数字/快捷键列右对齐（右缘 pr10）。
- 弹窗：标题左对齐、关闭钮右上；按钮行右对齐。
- 分栏容器：设置页**左列表列宽统一 350**（`settings/widgets.rs::LIST_W`，Providers/扩展/MCP 清单/技能四页，基准 = 扩展页，2026-10-09 定稿）；设置**导航列** 240（`sidebar_shell`）；工具定义左列 200。

---

## 8. 状态与动效

| 状态 | 规则 |
|---|---|
| hover | bg_hover；按钮主色变 accent_hover；危险变 danger_wash |
| selected | bg_selected（唯一表达） |
| focus | accent 1px 边框（仅输入框类）；按钮不做 focus ring，键盘焦点走 hover 同款底 |
| disabled | opacity 0.5，去掉 hover 响应 |
| 图标 hover | scale 1.06 + 上抬 1px（v55 规格，paint 期矩阵实现） |
| toast | 全项目两处（status 居中 / ext notice 右上）统一主题配色：`bg_panel` 底 + 1px 边框（status 用 `t.border`，ext 用类型色）+ `t.text` 文字；尺寸自适应内容，max 600×200，超高滚动，自动换行，左右 padding 20（2026-10-10 定夺） |
| 会话状态槽 | 15px 槽：运行 = spinner(accent)；agent 结束时**窗口无焦点或本会话已转后台** → 通知点三色（2026-10-10 定夺）：**外部错误/崩溃 = 7px 红 `t.danger`**、**内部中断（stopReason length/aborted，超时/上限停止）= 7px 黄 `WARN`**、**正常完成 = 7px 青蓝 `NOTICE`**（只是"跑完了回来看"，不承载对错）；绿色不作通知点（绿色只表达正确性）；点径统一 7px，切入会话即清 |
| psp 会话行局部字号 | 标题 = 面板设置值（ui12 基准）；时间 = **面板−2** `ui(10)` text_faint 右对齐列；「显示更多」= **面板−1** `ui(11)` **text_faint（placeholder 色）**，hover 回正文（2026-10-10 定夺） |

---

## 9. 存量收敛清单（按热点排序，落地时逐条销账）

| # | 问题 | 收敛目标 | 主要位置 |
|---|---|---|---|
| 1 | 文字按钮 6 种尺寸组合 | 全部对 §5.1（SM24/MD28/LG32，r6） | overlay.rs / dialogs.rs / psp_overlays.rs / git_panel.rs |
| 2 | 裸 px 字号绕过双轨（text_sm/xs ×11、TextInput px(12)、markdown ×5） | 换 ui()/固定特例入册 | dialogs.rs、titlebar.rs、content.rs、ext_ui.rs、text_input.rs、markdown.rs |
| 3 | 图标尺寸散点（关闭× 8/11/12/13；菜单图标 13/15） | §3 五档 | ui 调用点全量 |
| 4 | Danger 三套红 | theme.rs danger 系唯一表达 | widgets.rs、overlay.rs、psp_overlays.rs、titlebar.rs |
| 5 | 弹窗圆角 8/10/12 混用 | 8=菜单壳 / 10=弹窗 / 12=大卡 | dialogs.rs、overlay.rs、actions_menu.rs |
| 6 | 列表行高 5 种 + mb 补偿 | §5.8 四档；废除 mb 补偿 | function_panel、file_tree、mcp_picker、dialogs |
| 7 | 滚动条容器 8/10/12 | 统一 w10 | function_panel、actions_menu、top_panels、dialogs |
| 8 | checkbox 勾色 白/accent_contrast 分裂 | accent_contrast | mcp_picker、plugin_picker |
| 9 | 次要按钮描边 border_alpha(0x8c) 与 t.border 并存 | t.border | dialogs.rs、overlay.rs |
| 10 | 文档-实现漂移（发送钮 28↔36、设置导航 200↔160、statusbar 30↔36） | 以本文为准回写实现 | input.rs、settings/mod.rs、status_bar.rs |

落地建议（四阶段滚动，每阶段独立 commit + pif-ui 截图走查）：

- **P0 地基（约半天）**：新建 `crates/app/src/ui/tokens.rs` 落全部令牌常量（SP*、ICON_*、R_*、BTN_*、SW_*、DLG_*）+ 单测锁定数值（仿 `theme.rs` 的不变量测试）。零视觉变化；此后禁止新增硬编码尺寸。
- **P1 低风险值替换（约 1 天，一个 commit）**：#8 checkbox 勾色（2 处 2 行）→ #9 次要钮描边（2 处）→ #7 滚动条容器 w10（4 处）→ #4 danger 唯一化（4 文件）。全是近等值替换，视觉几乎无感，先销 4 条。
- **P2 样板收敛（2–3 天）**：#1 文字按钮——把 `settings/widgets.rs` 的 config_button 升级为基于 tokens 的通用按钮组件（三高三变体），先在 043 MCP 设置页试点走查，再替换 overlay/dialogs/psp_overlays/git_panel 其余组合；随后 #5 弹窗圆角三语义、#3 图标档位（先统一"关闭×=12""菜单图标=14"两个高频语义，其余按模块滚动）。
- **P3 视觉敏感项 + 文档回写（约半天 + 走查）**：#6 行高四档、废 mb 补偿（项目行 33→32 有 1px 变化，需走查）；#2 裸字号——markdown/终端按规范本就是固定特例，值入册 tokens 即可，其余换 ui()；#10 文档漂移以本文为准回写实现。

优先级理由：#1/#4 是后续新页面（042/043 一类设置页正密集产出）会照抄的样板，最优先统一，否则每新一个页面多欠一笔债；#2 带 bug 性质（用户调界面字号时那 11 处不动）；icon 全量 66 处不做一次性大扫除，改到哪个模块顺手收敛哪个模块。`settings/widgets.rs` 的 Config* 系列在 P2 改造为基于 tokens 的通用组件（Button/Switch/Checkbox/NavRow）向全项目推广。

> **销账状态（2026-10-10）**：#1–#10 全部落地。#2 的 markdown 固定字号入册 `tokens::fixed`（含锁值单测），vendored TextInput 实测已继承环境字号（无硬编码 px(12)，自然合规）；#3 剩余低频图标按"改到即收敛"策略滚动；#10 发送钮实测已 36、statusbar 36 入册 §5.2、设置导航列 160→240 已回写。会话轨新增定夺同期入册：§5.4 选项/分项文字（text_muted 统一、选中只加粗）、§2 终端字体（JetBrains Mono）与字号（= 面板字号）。

---

## 附：参照出处

- Zed `crates/ui`（本地 D:\github\zed 实测）：IconSize `icon.rs:54-77`、ButtonSize `button/button_like.rs:451-472`、Switch `toggle.rs:495-541`、Checkbox `toggle.rs:178-252`、ContextMenu `context_menu.rs:2263-2319`、Modal `modal.rs:162-366`、Spacing `styles/spacing.rs:19-34`、字号 `styles/typography.rs:161-248`、圆角族 `gpui_macros rounded_*`、滚动条 `scrollbar.rs:27-376`
- [Fluent 2 Layout](https://fluent2.microsoft.design/layout)（4px 基网格 spacing ramp）、[Fluent 2 Design tokens](https://fluent2.microsoft.design/design-tokens)、[Fluent UI 主题 spacing](https://storybooks.fluentui.dev/react/?path=/docs/theme-spacing--docs)
- [Material 3 Buttons](https://m3.material.io/components/buttons/overview)、[M3 图标 20/24dp](https://m3.material.io/styles/icons/designing-icons)、[M3 触控目标](https://m3.material.io/foundations/designing/structure)（本文论证其不适用于键鼠桌面）
- [GitHub Primer](https://primer.style)（16px octicons、8px 间距节奏）、[primer/primitives](https://github.com/primer/primitives)
- [shadcn/ui Button](https://ui.shadcn.com/docs/components/radix/button)（h-8/h-9/h-10、icon 16、r8）、[Dialog max-w-lg=512](https://github.com/shadcn-ui/ui/issues/7188)
