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
| **secondary+** | **ui(11.5)** | **行描述（set_row desc）、主题卡名、模型行副文**（2026-10-20 按 settings 实测收编） |
| **body** | **ui(12)** | 面板正文、按钮、输入框、菜单行、设置正文（基准档） |
| **body+** | **ui(12.5)** | **选项文字（语言行/字体列表）、misc 的值文字**（2026-10-20 收编） |
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
| ICON_XS | 12 | 行内关闭 ×、工具卡小图标、面包屑 chevron、**topbar 标签**（2026-10-22 定夺，原值 15 非法档收敛） | — |
| ICON_SM（默认） | 14 | **菜单项、列表行、按钮内、文件树** | — |
| ICON_MD | 16 | 独立强调图标、statusbar tab、空态图标 | — |
| ICON_LG | 18 | 图标按钮内部（psp 行操作钮、topbar 图标钮） | — |

规则：
- 图标↔文字 gap 8（SP4）；图标在按钮内默认 ICON_SM，图标按钮走 §5.2 的方形槽。
- 关闭 × 全项目统一 ICON_XS（12）——存量 8/11/13 三种杂值收敛掉。
- **「更多」图标全项目唯一 = lucide ellipsis（裸三点）**（2026-10-13 用户定夺，取代 2026-10-10 的 circle-ellipsis 带圈三点）；禁止再引入第二种 more 变体。
- iconfont「新建会话」19px 等**品牌特例**仅限 startup logo（88）与该处，不得扩散。
- icon hover 动效（scale 1.06 + 上抬 1px）按 v55 既有规格，见 `docs/UI设计/主界面UI设计说明.md` §14。

---

## 4. 圆角体系

| 令牌 | px | 用途 |
|---|---|---|
| R_XS | 3 | checkbox、徽标、高亮词底 |
| R_INLINE（行内小件） | 5 | **输入框、下拉触发器、列表行、菜单行、主题卡、键帽、行内小图标钮**（2026-10-20 收编 settings 实测：这一族全是 5，见 §10） |
| R_CTRL（默认控件） | 6 | **一切文字按钮、图标按钮、tab** |
| R_CARD | 8 | 菜单/弹层容器、会话/项目列表行、面板内卡片 |
| R_MODAL | 10 | 弹窗卡片（含大卡顶条与整体） |
| R_SHEET | 12 | 大卡片（设置/系统提示词）、hover 浮卡 |
| R_PILL | 16 / full | composer 胶囊、pill、开关、圆点（rounded_full） |

规则：同语义同值。"5 与 6 并存"不是随机：**能点的散件（按钮）走 6、被塞进行里的容器件走 5**，两档语义见 §10 实测表。"弹窗圆角"只有一个 10（存量 8/10/12 三档收敛：菜单壳 8、标准弹窗 10、大卡 12——这是三条语义，不是三档随机）。窗口装饰（CSD）10px 是系统特例。

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
- **设置页入口 `config_button` 只有两档：`small` = BTN_MD(28)，默认 = BTN_LG(32)**（表单主按钮取 LG；实测表见 §10.4）。
- Danger 三套红（widgets hsla 系 / overlay t.danger 系 / psp danger_alpha 系）**只保留 theme.rs 语义系**：`t.danger` 文字 + `danger_alpha(0x59)` 边 + `danger_wash()` 底。硬编码 0xd8626a 仅允许出现在窗口关闭钮（OS 惯例特例）。
- 弹窗按钮排布：右对齐、gap 8、次要（取消）在左、主要（确认）在右。危险确认钮用 Danger 变体，rest 底 6% → hover 12%。

### 5.2 图标按钮

方形、无边框，圆角 6，hover = bg_hover：

| 尺寸 | 方形槽 | 内图标 | 用途 |
|---|---|---|---|
| IB_SM | 22 | 14 | tab 关闭 ×、行内 ⋯ 触发钮 |
| IB_MD（默认） | 28 | 16 | 行操作钮、弹窗关闭 × |
| IB_LG | 32 | 18 | topbar、psp 标题行操作 |

特例（保留）：窗口控制钮 42×40 无圆角（OS 惯例）；composer 发送/停止 36×36 圆角 16（运行中收成 9）；大卡关闭钮 30×30 红底（破坏性区域语义）；topbar/statusbar 高 40（chrome 层，2026-10-10 定夺 36→38，2026-10-13 定夺 38→40 两条等高；两条 bar 上沿均不画 border_t，激活 tab 上沿不画线只留左右分隔）；**bar 内 tab 高 = bar 高 −4**（2026-10-13 定夺；topbar tab 底缘 mb(−1) 融入下方内容区，statusbar 激活卡顶缘贴 dock 连体）。

### 5.3 开关 Switch

| 尺寸 | 轨道 | 圆角 | knob | 位置（关/开） |
|---|---|---|---|---|
| SW_MD（默认） | 28×16 | 8（半高） | 10 | ml2 / ml14 |
| SW_SM | 24×14 | 7 | 8 | ml2 / ml10 |

- 三态（`SwitchMark`：Off / Mixed / On）：
  - **On（全选）** = accent 实底 + bg 色 knob，knob 在开位；
  - **Mixed（部分选中）** = accent 减淡铺底（`tokens::switch::MD_MIXED_ALPHA`）+ 同色调略深描边 + knob 在开位；
  - **Off** = bg_selected 底 + border，knob 在关位；
  - 用于「一组若干项」的总开关（技能分组 / 扩展全局 / MCP 全局）：一项没开不能装成一半，只开了一部分也不能装成全关——部分看轨道浓度，具体数字交给旁边的 `{enabled}/{total}`。禁用仍是整体 opacity 0.5（别拿减淡轨道当禁用）。
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
| 单行高 | 30（紧凑档 20 = 行内改名/过滤框，见 §10.4） |
| 多行高 | 170（约 8 行，可拖） |
| 圆角 | **5（R_INLINE）**——2026-10-20 按实现收编（原写 6，`ui/text_input.rs` 与 settings 全部输入框都是 5） |
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
| 子菜单 | 沿父容器右侧 +4 偏移，行对行；父卡向上弹时行位按卡底缘折算 |
| 弹出方向（统一算法） | 锚点在视口**上半 → 向下弹，反之向上弹；左半 → 向右弹，反之向左弹**（两轴独立判定，恰在中线按「反之」）。唯一实现 `ui::popup::PopupEdges`（边锚定制：向上/向左弹给 bottom/right，免量内容尺寸）；toast 与会话栏（psp）全套浮层已接入，新锚定浮层一律走它（2026-10-11 定夺）。**算法只裁方向，精准对齐归调用方**——如会话 hover 详情卡：向下弹卡顶=行顶、向上弹卡底=行底（行高由 prepaint 钩子实测），卡悬在行的背离侧不遮行（2026-10-11 二次定夺） |

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

### 5.10 悬浮提示 tooltip

唯一入口 `ui/tooltip.rs` 的 `WithTip`（2026-10-13 建），禁止直摸 `gpui_component::tooltip::Tooltip`：

| API | 文案 | 用途 |
|---|---|---|
| `.with_tip(key)` | i18n 键，每次弹出过 tr 随语言切换 | icon 钮/可交互元素的悬停提示 |
| `.with_tip_action(key, action)` | 同上 + 键位 chip（`Kbd` 右挂，样式「Ctrl+P」灰底键帽） | 有全局快捷键的钮；键位查 keymap 自动跟随绑定 |
| `.with_tip_text(text)` | 动态直排（快捷键拼接等），不走 i18n | 动态文案 |

机制：gpui 原生 tooltip 手势——悬停 **350ms** 自动弹出（vendored gpui 本地改动，上游 500ms，升级需重套）、移开即收，应用层不写 hover 状态机；手势只挂 stateful 元素（`.id()` 必须先行）。提示卡样式 = gpui-component `Tooltip` 默认盘（tip_view 收口），不另行定制。

不属 tooltip 的悬浮物：psp 项目路径悬浮条 / 会话详情卡（自绘 overlay，可交互、有宽限期，见 psp_overlays.rs）。

### 5.12 滑块（range slider）

设置页唯一的连续量控件（首用 = 界面页「会话内容宽度」）。代码落点
`settings/widgets.rs` 的 `width_slider`：

| 项 | 值 |
|---|---|
| 元素高（= 命中带） | 18（轨道只有 4px，命中带必须放大） |
| 轨道 | 4px 高、全圆角、`t.border` |
| 已选段 | 同轨道几何，`t.accent` |
| 旋钮 | 12px 圆，`t.bg_panel` 填充 + 1.5px `t.accent` 描边（中心行程 = 元素宽 − 旋钮径） |
| 默认宽 | 180 |
| 步进 | 数值域自定（宽度 = 20px/档，2026-10-29 定） |

交互：点轨道即跳值、按住连续拖（拖出元素范围仍跟手）；拖动中只写内存预览
并重绘（消息列与 composer 同帧同步变宽），**松手才落盘**。实现走
`canvas` + 窗口级 `on_mouse_event`（与 §5.9 滚动条同一机制），拖拽标志放
thread_local——paint 期每帧重注册闭包，局部变量活不过一次重绘。

### 5.11 面板控制栏（020-1 整合面板）

上下子面板（会话列表 / 文件树）的**控制栏统一 38px 高、一律无下 border**（2026-10-14 定夺，视觉统一清晰）；控制栏钮盒高 30、图标档位左二 LG18（icon+文字）/ 右三 MD16（纯 icon，搜索已降级并入右侧组）；分隔条总高 7px（视觉线 1px 居中，左右缩进 10，hover/拖拽变 accent，双击复位 50%）。代码落点 `tokens::panel_ctrl`（BAR_H=38 / DIV_H=7 / BTN_H=30）。

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
- 列表：行内左对齐起于 pl10（psp 一体列表除外，见下条图标对齐线）；时间/数字/快捷键列右对齐（右缘 pr10）。
- **图标对齐线（topbar 收放钮 ⇄ psp 列表 ⇄ 状态栏首 tab，2026-10-13 定稿）**：收放钮图标与 psp 列表行图标（项目 folder / 会话 / 伪标签）共用一条视觉左缘，状态栏首 tab 卡片左缘也落在线上，单一来源 `ui/tokens.rs::align::ICON_LINE`（现值 12）。两侧从它反推：收放钮 `ml = ICON_LINE − (钮盒30 − 图标18)/2 = 6`；psp 列容器 `px 10`（唯一水平留白），行内 `pl = ICON_LINE − 10 = 2`（对齐补偿，非留白）；状态栏 tabs 容器 `ml = ICON_LINE`。**改线只改 ICON_LINE 一个数**（规范本条 + tokens 常量 + 锁定测试断言三处记账），两侧边距与 session 渐隐量宽公式全部自动跟随。状态栏设置钮图标右缘对齐列表右线（容器 LIST_PX + 行 pr LIST_PX = 2×LIST_PX，右对齐 + pr 直取；居中会是 15px、多伸出 5px，勿改回）。
- 弹窗：标题左对齐、关闭钮右上；按钮行右对齐。
- 分栏容器：设置页**左列表列宽统一 400**（`settings/widgets.rs::LIST_W`，Providers/扩展/MCP 清单/技能四页，基准 = 扩展页；2026-10-09 定稿 350，082 因扩展页要容「↑ 可更新」箭头四页同步加宽到 **400**，2026-10-20 回写）；设置**主导航列 160**（mod.rs big_card：界面/模型/技能… 那列；2026-10-10 定夺，勿与 sidebar_shell 混淆）；子页清单基座 `sidebar_shell` 240（未走 LIST_W 的页用）；工具定义左列 200。

---

## 8. 状态与动效

| 状态 | 规则 |
|---|---|
| hover | bg_hover；按钮主色变 accent_hover；危险变 danger_wash |
| selected | bg_selected（唯一表达） |
| focus | accent 1px 边框（仅输入框类）；按钮不做 focus ring，键盘焦点走 hover 同款底 |
| disabled | opacity 0.5，去掉 hover 响应 |
| 图标 hover | scale 1.06 + 上抬 1px（v55 规格，paint 期矩阵实现） |
| toast | 全项目两处（status 居中 / ext notice 右上）统一主题配色：`bg_panel` 底 + 1px 边框（status 用 `t.border`，ext 用类型色）+ `t.text` 文字；尺寸自适应内容，max 600×200，超高滚动，自动换行，左右 padding 20（2026-10-10 定夺）；定位纵边走 §5.6 统一弹出算法（status 只取纵向判定、水平居中独立保留；ext 右上槽 = 上半向下+右半向左，输出与原写死值等价）（2026-10-11） |
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
| 11 | icon 钮 tooltip 只挂文件树四钮，psp 标题行/行内 ⋯＋ 从未有 | §5.10 `with_tip` 全量补挂 | function_panel/mod.rs（psp 标题行 4 + 项目行 2 + 会话行 ⋯） |

落地建议（四阶段滚动，每阶段独立 commit + pif-ui 截图走查）：

- **P0 地基（约半天）**：新建 `crates/app/src/ui/tokens.rs` 落全部令牌常量（SP*、ICON_*、R_*、BTN_*、SW_*、DLG_*）+ 单测锁定数值（仿 `theme.rs` 的不变量测试）。零视觉变化；此后禁止新增硬编码尺寸。
- **P1 低风险值替换（约 1 天，一个 commit）**：#8 checkbox 勾色（2 处 2 行）→ #9 次要钮描边（2 处）→ #7 滚动条容器 w10（4 处）→ #4 danger 唯一化（4 文件）。全是近等值替换，视觉几乎无感，先销 4 条。
- **P2 样板收敛（2–3 天）**：#1 文字按钮——把 `settings/widgets.rs` 的 config_button 升级为基于 tokens 的通用按钮组件（三高三变体），先在 043 MCP 设置页试点走查，再替换 overlay/dialogs/psp_overlays/git_panel 其余组合；随后 #5 弹窗圆角三语义、#3 图标档位（先统一"关闭×=12""菜单图标=14"两个高频语义，其余按模块滚动）。
- **P3 视觉敏感项 + 文档回写（约半天 + 走查）**：#6 行高四档、废 mb 补偿（项目行 33→32 有 1px 变化，需走查）；#2 裸字号——markdown/终端按规范本就是固定特例，值入册 tokens 即可，其余换 ui()；#10 文档漂移以本文为准回写实现。

优先级理由：#1/#4 是后续新页面（042/043 一类设置页正密集产出）会照抄的样板，最优先统一，否则每新一个页面多欠一笔债；#2 带 bug 性质（用户调界面字号时那 11 处不动）；icon 全量 66 处不做一次性大扫除，改到哪个模块顺手收敛哪个模块。`settings/widgets.rs` 的 Config* 系列在 P2 改造为基于 tokens 的通用组件（Button/Switch/Checkbox/NavRow）向全项目推广。

> **销账状态（2026-10-10）**：#1–#10 全部落地。#2 的 markdown 固定字号入册 `tokens::fixed`（含锁值单测），vendored TextInput 实测已继承环境字号（无硬编码 px(12)，自然合规）；#3 剩余低频图标按"改到即收敛"策略滚动；#10 发送钮实测已 36、statusbar 36 入册 §5.2、设置主导航列恢复 160（P3 曾误按 sidebar_shell 的 240 改宽，两处是不同的面，2026-10-10 纠正）。会话轨新增定夺同期入册：§5.4 选项/分项文字（text_muted 统一、选中只加粗）、§2 终端字体（JetBrains Mono）与字号（= 面板字号）。

---

## 10. Settings 已实现组件实测表（事实标准 · 新页面照抄）

> **立此存照（2026-10-20）**：040-1 扩展市场这个「简单页面」，因为没有一张能抄的尺寸表，
> 每个按钮、每段间距都现场推，来回改一整天仍对不齐。本章把 settings 里**已经落地并被
> 用户验收**的组件逐件量下来，作为可复制的清单。
>
> 使用规则：**新页面照本章抄数**；本章与 §1–§9 冲突处以本章为准（通用规则只约束本章未
> 覆盖的新组件）；抄到一半发现缺项，**补进本章再写代码**，不要现推。
>
> 量取基准 = 面板字号默认档（`ui(n)` 即 n）；凡走 `ui_size()` 的尺寸都随「界面字号」缩放，
> 本章只写基准值。括号里是代码落点，抄前读一眼那个函数。

### 10.1 大卡窗框（`ui/overlay.rs::big_card`，设置/系统提示词/工具定义共用）

| 部件 | 值 |
|---|---|
| 整卡 | 宽 `0.7×` 视口、高 `0.98×`、r**12**、1px `t.border`、`shadow_lg` |
| 顶条 | h**36**、`pl` **30**、`bg` chrome、`rounded_t` 12、下边 1px `border_alpha(0x73)` |
| 标题 | ui(13) SEMIBOLD `t.text`（空串 = 无标题，左导航即身份） |
| 关闭钮 | **30×30** r**7** 图标 **13** `text_muted`，hover = 红底白字（`0xd8626a`，OS 惯例特例） |
| 左导航列 | w**160** `bg=t.nav`、`p 8 / pt 12`、`rounded_bl 10`、右边 1px `border_alpha(0x66)` |
| 导航项 | `px 10 / py 7.5`、`mb 1`、r**8**、gap **9**、ui(13)、图标 **15**；激活 `bg_selected`+SEMIBOLD+`t.text`+icon accent，否则 `text_muted`+hover `bg_hover` |
| body 外衬 | `pt 14 / pb 16`、`px 20`（**分栏页 px = 0 全出血**：扩展/模型/技能/MCP 四页；**子代理页虽也分栏，body 仍留 px 20**） |

### 10.2 两栏骨架（`two_pane` + `sidebar_shell` / `sidebar_list` / `detail_shell`）

| 部件 | 值 |
|---|---|
| 侧栏基座 `sidebar_shell` | 默认 w**240**，但**现有五页（扩展/模型/技能/MCP/子代理）一律 `.w(LIST_W=400)` 覆盖**——新建分栏页照 **400** 抄，240 只当兜底默认值；`h_full`、右边 1px `t.border`、**不刷底色**（列表区与详情同底，2026-10-10 定稿） |
| 列表容器 `sidebar_list` | `px 6 / pt 8`，滚动容器；**列表页统一 `px 7`**（扩展/技能/MCP） |
| 列表行 `sidebar_item` | h**30**、`px 8`、`mb 1`、r**5**、gap **8**、ui(12)；激活 `bg_selected`+SEMIBOLD+`t.text`，否则 `bg_panel`+`text_muted`，hover `bg_hover` |
| 组头 `group_header` | `px 8 / pt 8 / pb 3`、ui(10) SEMIBOLD `text_dim`，右侧 aside（计数 + 三态开关） |
| 详情区 `detail_shell` | `p 20`、`gap 16`（`gap_4`）、ui(12)、纵向滚动 |
| 详情内区块 | 常见 `p 15`（技能/MCP/自定义模型/子代理表单）；区块内行距 **8**、次级行距 **12** |
| 列宽常量 | `LIST_W = 400`、主导航列 160、侧栏 240、工具定义左列 200 |

### 10.3 行与文本块

| 件 | 值 | 落点 |
|---|---|---|
| set-row（界面/其他页单列行） | `gap 14 / py 13`、下边 1px `border_alpha(0x40)`；左列 title ui(13) SEMIBOLD `text_muted` + desc ui(**11.5**) `text_dim`、左列内 gap 2；控件靠右 | `set_row` |
| field（label 上 / 控件下） | 纵向，`gap 5`；label 走 section_title | `field` |
| section_title | ui(11) SEMIBOLD `text_dim` | `section_title` |
| 说明 / 错误 | ui(11)，`text_dim` / `t.danger` | `note` / `error_note` |
| mono 值块 | ui(11) + `MONO_FAMILY`，`nowrap + ellipsis` | `mono_text` |
| 统计/计数 | mono ui(10) `text_dim`（如组头 `3/12`） | `group_switch` |

### 10.4 控件族

| 控件 | 值 | 落点 |
|---|---|---|
| 按钮 | **small = §5.1 MD(28/px12/ui12)、默认 = LG(32/px14/ui12)**；设置页表单主按钮一律 LG，行内/列表小按钮 MD | `config_button` → `ui::button` |
| 开关 | **28×16** r8、knob **10**（关 `ml 2` / 开 `ml 14`）；关 = `bg_selected`+border+`text_muted` knob，开 = `accent`+`t.bg` knob，**Mixed = accent×0.45 轨道 + knob 开位**；禁用整体 opacity 0.5 | `switch_base` / `config_switch` / `group_switch` |
| 输入框 | 单行 **30**、多行 **170**、**紧凑 20**（行内改名/过滤，内件 XSmall 零富余）；r**5**、1px（聚焦 `accent`）、底 `bg_panel`、ui(12)（compact ui(11)）、placeholder `text_faint` | `TextInput`（`ui/text_input.rs`） |
| 下拉/选择器触发器 | **250×30**（字体槽 **100×30**）×1px r**5**、`px 9`；开态边框 `accent`；标签真居中（绝对定位铺满，箭头贴右） | general 字体/字号、agents 下拉 |
| 菜单行（页内小菜单） | h**28**、`px 8~10`、r**5**、gap **6~8**、ui(12)；容器 `p 4~6` + r8 + `shadow_lg` | 排序/字体/语言菜单 |
| 单选 | **14** 圆 + **6** 内点（选中：accent 边框 + accent 点），标签 ui(12)、间距 7 | general 语言行 |
| 徽标 scope_tag | `px 5 / py 1`、r**3**、ui(10)；项目 = indigo `hsla(.63,.86,.62, .12/.85)`，其他 = 中性 12% 纱 | `scope_tag` |
| 状态点 | **7** 圆 `rounded_full` | `status_dot` |
| 行内小图标钮 | **24×24** r5（其他页）、垃圾桶 **28×28** r5（danger 35% 边 + 6% 红底） | misc / plugins |
| 工具条开关钮 `icon_toggle` | **24×24** r**6**、图标 **14**；按 = accent 16% 洗底 + accent 图标，否则透明 + `text_muted`，hover `bg_hover`，禁用 0.4 | kit/toggle.rs（083 搜索条） |
| 工具条文字开关 `text_toggle` | 同槽（h**24**、`px 6`、r**6**），文字 mono ui(11) BOLD；按 = accent 16% 洗底 + accent 字 | kit/toggle.rs（083 搜索条 Aa/ab\|/.*） |
| 键帽 Kbd | 高 **22**、`min-w 22`、`px 7`、r5、mono ui(11)；底 `cap_bg`、1px border、底部 1px 硬阴影（edge = border 混深） | shortcuts |
| 主题卡 | 高 **36**、r5、1px border（选中 → **2px** accent）、`px 8`、gap 6、底色 = 该主题 `bg`；**每行 5 槽等宽、gap 8、mt 8**；槽位外包无 padding 容器（否则 flex 不等宽） | general 外观页 |
| 滑块 | 180×18、轨道 4、旋钮 12（同 §5.12） | `width_slider` |
| 列表行高（可缩放页） | `max(ui(11) + 19, 30)`——随字号长个，不写死 30 | models 侧栏 |
| 表单（子代理页） | label 列 **w96**、控件列 **w250**（下拉/输入框 30 高）、textarea **170**、行距 **8**、区块 gap **12** | agents |

### 10.5 字号补充档（2026-10-20 收编，§2 的档位表 +2 档）

| 档 | 值 | 用途 |
|---|---|---|
| secondary | ui(11) | 说明/注释/键帽/mono 值 |
| **secondary+** | **ui(11.5)** | **行描述（set_row desc）、主题卡名、模型行副文** |
| body | ui(12) | 面板正文、列表行、输入框、徽标正文 |
| **body+** | **ui(12.5)** | **选项文字（语言行/字体列表）、misc 里的值文字** |
| emphasized | ui(13) | set_row 标题、导航项、弹窗标题 |

存量 `ui(9)`（provider 徽标）、`ui(10.5)`（字号后缀）两处按「改到即收敛」滚动（§9 策略），新增文案不许再取这两档。

### 10.6 新建设置页 checklist（照做就不会再对不齐）

1. **骨架**：分栏页 = `two_pane(sidebar_shell(...), detail_shell(...))`；单列页 = `set_row` 叠行。侧栏列宽走 `LIST_W`，不要自己量。
2. **行**：`sidebar_item` / `set_row` / `group_header` 三选一，禁止自拼行高与 padding。
3. **控件**：`config_button` / `config_switch` / `TextInput` / `width_slider` 四个入口，禁止自绘按钮与开关（市场页手搓分段控件就是反面教材：同页出现 gap 4/6/8 三种图标间距）。
4. **数字**：新尺寸先在 §10 找；找不到才新增，且**先落 `ui/tokens.rs` 常量 + 本章表格**，再写代码。
5. **抄骨架**：`docs/模板/新页面骨架.md`（一页可复制骨架，整页零字面量；含两栏/单列两种）。
6. **守卫**：`python scripts/check_ui_raw.py`——交互链（`on_mouse_down` / `cursor_pointer` /
   `on_click`）里出现裸几何字面量 = 红；存量冻结在 `scripts/ui-raw-baseline.txt`，只拦新增。
   确属特殊控件在该链写 `// ui-raw: ok <理由>` 并回来登记尺寸。
7. **对齐**：右缘对齐同一列时，宽度由同一个函数算（参见市场页 `side_col_w` 用 `text_system().layout_line` 实测标签宽，禁写死——英文标签比中文长一倍）。

### 10.7 本章收编时的两项判定（防再次反复）

- **圆角 5 vs 6**：不是漂移，是两条语义——**被塞进行里的容器件（输入框/触发器/列表行/菜单行/键帽）走 5**，**能点的散件（文字按钮/图标按钮/tab）走 6**。已入 §4 为 R_INLINE。
- **间距出现 7.5 / 9 / 13 / 14 这类非 2px 网格值**：settings 是 pi-web 逐像素复刻，这批值已全站验收，**归 10 章的「设置页专用密度」**，不改；新页面照抄含义（行 py13 / 行内 gap14 / 导航 py7.5 / 标签↔控件 gap5），不要搬去其他面（会话区仍按 §1 网格）。

### 10.8 ui-raw: ok 豁免登记（特殊控件 · 逐条可追溯）

| 位置 | 几何 | 理由 |
|---|---|---|
| `search/results.rs` `landing_row` | 行高 22 / px 4 / gap 4 / rounded 4 | Zed search landing 行逐值复刻，非 PF 档位 |
| `search/results.rs` `card_header` 外壳 | 块 p 4 / 内条 pl 4 pr 8 gap 6 rounded 4 | Zed `render_buffer_header` 逐值复刻（083 定案） |
| `search/results.rs` `card_header` 收展钮 | 28×28 / rounded 2 | Zed `rems_from_px(28)`；不套 `ui_size`（12 基准会放大成 31） |

---

## 附：参照出处

- Zed `crates/ui`（本地 D:\github\zed 实测）：IconSize `icon.rs:54-77`、ButtonSize `button/button_like.rs:451-472`、Switch `toggle.rs:495-541`、Checkbox `toggle.rs:178-252`、ContextMenu `context_menu.rs:2263-2319`、Modal `modal.rs:162-366`、Spacing `styles/spacing.rs:19-34`、字号 `styles/typography.rs:161-248`、圆角族 `gpui_macros rounded_*`、滚动条 `scrollbar.rs:27-376`
- [Fluent 2 Layout](https://fluent2.microsoft.design/layout)（4px 基网格 spacing ramp）、[Fluent 2 Design tokens](https://fluent2.microsoft.design/design-tokens)、[Fluent UI 主题 spacing](https://storybooks.fluentui.dev/react/?path=/docs/theme-spacing--docs)
- [Material 3 Buttons](https://m3.material.io/components/buttons/overview)、[M3 图标 20/24dp](https://m3.material.io/styles/icons/designing-icons)、[M3 触控目标](https://m3.material.io/foundations/designing/structure)（本文论证其不适用于键鼠桌面）
- [GitHub Primer](https://primer.style)（16px octicons、8px 间距节奏）、[primer/primitives](https://github.com/primer/primitives)
- [shadcn/ui Button](https://ui.shadcn.com/docs/components/radix/button)（h-8/h-9/h-10、icon 16、r8）、[Dialog max-w-lg=512](https://github.com/shadcn-ui/ui/issues/7188)
