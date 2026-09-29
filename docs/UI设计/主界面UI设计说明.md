# pi-flash 主界面设计说明

> 配套文件：[主界面UI设计.html](./主界面UI设计.html)（可交互设计稿，双击浏览器打开）
> 状态：设计定稿 · 2026-09-28 · mist 主题 · 共 35 轮迭代
> 文件约定：HTML 内**不写注释、不写页面说明文字**；补丁脚本存于 `tmp/design/patch_*.py`
> 演示哈希：`#collapsed` 收起侧栏 · `#files` / `#git` 切面板 · `#term` / `#md` / `#both` 内容区视图 · `#history` git 历史页 · `#nav` 会话导航展开 · `#settings` 设置弹窗

## 1. 布局总览

```
┌──────────────────────────────┬────────────────────────────────┐
│ topbar 左段（chrome）         │ topbar 右段（chrome）           │
│ [收放][项目][会话][搜索][终端] │ [终端tab][md tab]   [设置]│─□× │
├──────┬──────────────┬────────┴────────────────────────────────┤
│ plb  │ slp          │ 内容区（聊天 / 终端 / md 预览）           │
│ 66px │ 250–500px    │                                        │
│ 项目 │ 会话|文件树|git│                                        │
│ 标签 │ 三面板        │                                        │
├──────┴──────────────┴────────┐                               │
│ statusbar（仅面板段）[3 tabs] │ ← 内容区直通窗口底              │
└──────────────────────────────┴────────────────────────────────┘
```

- topbar 完全自绘（无系统边框），分左右两段：左段属面板区（chrome 底），右段属内容区（内容底）——Obsidian 式
- plb|slp 之间**无分隔线**（色差分层）；slp|内容一条全高竖线
- **statusbar 只存在于面板段**（宽度 = plb + slp）：内容区在所有视图下都直通窗口底，内容列下探 25px，composer 贴近底缘
- git 面板没有独立内容区——它的"内容区"就是会话内容区

## 2. 尺寸系统（全部走 CSS 变量）

| 项 | 值 |
|---|---|
| topbar 高 | **38px**（两段同高；图标 17px，窗口控制 glyph 14px） |
| statusbar 高 | **30px**，宽度恒为面板段 |
| plb 宽 | **66px**，padding 上 0 下 5 左右 3（首标签贴 topbar 下沿，无 gap） |
| slp 宽 | **可拖拽 250–500px**，默认 282px |
| composer 边距 | 上 2px、左右 34px、下 5px |
| 内容列 | 无条件 `margin-bottom: -25px` 下探 |
| 对齐锚点 | topbar 工具簇、状态栏 tabs、slp 会话行同起点 = `plb + 16px`；topbar tools 实际 margin = `plb/2 + 1px`（因前方有收放钮） |

**变量化是硬约束**：`--plb-w` / `--slp-w` 定义在 `:root`，rail/dock/panel-col/贯穿线/对齐锚点全部 `calc()` 派生——改动任何宽度，其余自动跟随（曾因写死 350px 与实际 348px 差 2px 出过"分隔线变宽"的 bug）。

**slp 拖拽**（参考 zed panel resize）：分隔线上 6px 隐形命中区，pointer capture 拖拽 + clamp(250,500) + 双击复位 282px；悬停/拖拽显 accent 线。

## 3. 三层色阶

| 层 | token | 色值 | 区域 |
|---|---|---|---|
| chrome | `--chrome` | `#dfe9e5` | topbar 两段 + statusbar + plb |
| nav | `--nav` | `#eef4f1` | slp |
| content | `--bg` | `#f4f8f7` | 聊天 / 终端标签页 / md 预览 |

- `--chrome-hover: #e7efec`（chrome 区悬停）；悬停通则 = 所在层按下一档
- **选中语言一句话：填充色 = 它打开的目标区域；accent bar 只归项目标签**
  - 项目标签选中：满幅出血色带（nav 色）连体 slp + 左缘 3px accent 竖条（贴窗口边、全高、方形）
  - 会话行选中：accent 10% soft-tint 圆角 pill（色相偏移，非明度差——浅色主题上纯明度差不可见的教训）
  - 状态栏面板 tab 激活：nav 色向上顶穿状态栏顶线连体 slp
  - 内容区 tab 激活：bg 色凸起卡片，下缘压过 topbar 底线连体内容

## 4. plb（projectListBar，66px）

- 标签**统一 46px 等高**：第一行项目名，第二行恒存在
- 第二行语义二分：**活跃** = 旋转圈（有 session 在跑）/ 绿点（全停有未读）+ 未读回执数（session 个数 ≠ 消息数）；**空闲** = 距今时间（"1小时前"）
- 空闲项目整体降灰：名字 `--text-dim` + 字重 500（只降色不降字重看不出差别）
- 排序：活跃/未读在前，空闲按时间倒序；**硬上限 10 个**（`slice(0,10)`），更多只能「打开项目」手动打开
- 选中：满幅出血 `width: calc(100%+8px); margin-left: -4px; padding-left: 12px`，文字与他签同位；出血必须显式加宽——纵向 flex 里负 margin-right 撑不宽盒子（踩过）

## 5. slp（sessionListPanel，三面板）

状态栏 tabs 切换，会话默认：

1. **会话**：session list；选中 = soft-tint pill
2. **文件树**（Zed 式）：根项目行（folder-open + 加粗）、每级缩进 guide 线（1px 通长）、按类型图标（folder-open/folder、md=book、html=file-code、json=braces、sh=file）、选中行全宽色带；点 .md 行打开内容区预览 tab
3. **git**（Zed 式，不做 diff 详情页）：
   - 页签 `Changes (N) / History`
   - Changes：View Diff + Stage All ∨ 操作行；Untracked 变更树（文件夹开合图标、新增文件绿色 + 徽标、行尾复选框、选中行 accent 描边）；底部固定区 = `⎇ main + ↑N Push ∨` / **commit message 大区**（无边框无卡片、与面板同底、min-height 96px，靠留白视觉扩容）/ `Commit Tracked ∨`；最底最近提交条 = 标题 + 单个 uncommit（undo-2 图标）
   - **明确去掉**：窗口操作图标（放大/弹出）、AI 写描述按钮
   - History：提交列表（标题 + ↑ 推送小钮；`○ 多多有鱼 · 时间 · hash` 元行），底部 commit 区不显示

## 6. topbar（38px 两段）

- **左段**（chrome）：收放钮（贴左 5px）+ 工具簇：打开项目（folder-open）/ 新建会话（message-square-plus）/ 会话搜索（search）/ 打开终端（terminal）——纯图标 30×30，图标 17px，悬停 tip 气泡
- **右段**（内容底，padding-left 4px）：内容区 tabs + 弹性空档 + 设置（sliders-horizontal）+ 竖线 + 窗口控制（42×38，关闭悬停红）
- **收起**（Obsidian 式）：面板整体归零隐藏（非留 46px 竖条）、内容区竖线与状态栏一并消失；收放钮跳到内容区 topbar 起点（图标**镜像**：竖线在右），点击展开
- 收起态间隙 **4px 等距**（上下左右一致，38px 行内 30px 按钮）

## 7. 内容区 tab（Obsidian 式）

- **激活 tab** = 凸起卡片：bg 色、顶部圆角、下缘压线连体内容、× 可见（× 关闭）
- **非激活 tab** = 平铺文字：无底无框、muted 灰、条带内垂直居中、无 ×；点文字切换，凸起随之转移
- 视图状态机：`chat`（默认，无 tab）→ `term`（终端，暗底 + bash tab）→ `md`（markdown 预览：居中 760px 版式、h1/h2/p/ul/pre 样式，代码块 panel 底）
- 终端标签与分隔线相隔 10px；tab 间 4px

## 8. composer（一体式，ZCode 式）

单容器（16px 圆角、1.5px 边框）：上部多行文本区 + 底部控件行

- 左：**图片**（lucide image）+ **工具预设**（wrench「默认 ∨」）——没有 + 按钮
- 右：**上下文用量环**（donut 弧线 = 已用比例，tip 显示"15.2万 / 1.0M（15%），点击查看明细"；点击出分项弹窗：消息/系统工具/技能/系统提示词/MCP + 缓存命中率）→ **模型 ∨** → **思考强度 ∨**（lightbulb「high」）→ **发送 ↑**（accent 方形圆角键，运行中变停止）
- 无压缩、无铃声、无 AI 按钮

## 9. statusbar（30px，仅面板段）

- 永远只有三个面板 tab（会话/文件树/Git），46px 宽、icon 即标签
- 右段状态信息**已删除**（所有视图下内容区都无 statusbar）

## 10. 设置弹窗

- 居中 **70% 宽 × 98% 高**，遮罩点击关闭，右上 × 关闭（悬停红）；弹窗自带 38px topbar（chrome 底、发丝线）
- 左导航 **200px**（nav 底）：六页签带图标——界面(wallpaper)/模型(cpu)/技能(wand-sparkles)/子代理(bot)/插件(plug)/其他(ellipsis-vertical)；激活项选中底 + accent 图标
- 右内容：**无页标题、无页面说明**，直接设置行（标题+描述居左、控件居右、发丝线分行）
- 行控件：下拉（sel-btn 单行 + 10px chev）、toggle（34×19 accent pill）、只读文本、列表卡（set-list）
- 各页内容按真实配置 mock：界面（主题/图标/三字体/语言/提示音/预载数）、模型（默认模型/思考强度/可用列表）、技能开关、子代理档案、npm 插件、其他（pi 版本/启动恢复/数据目录）

## 11. 会话导航（minimap）

- 位置：聊天滚动区**外侧**（聊天区滚动条隐藏），透明底常驻
- 几何硬性规定：**topbar 下 100px 起；最高内容区 70%；节点硬上限 10 个**
- 节点 = **比例尺**：不对应单条对话；≤10 轮一轮一点，超过不增长只细分；当前阅读位 = accent 实心
- 样式：pi-web 式**灰点（7px）+ 灰线分段连接，线不穿过圆点**（弃用 ZCode 式小横条）
- 悬停展开：326px 发言列表**覆盖层**（绝对定位、不挤压布局）：`01` 编号 + 用户发言（加粗 ≤3 行）/ `A` + agent 回复首行，轮间分隔线，当前轮 accent 左条；圆点列保持覆盖层右缘可见

## 12. 图标系统

统一 **Lucide 24 栅格 2px 描边**，SVG path 内嵌。已用：messages-square（会话 tab）、message-square-plus（新建会话）、folder-open（打开项目/展开目录）、folder、search、terminal、sliders-horizontal（设置）、file-code、book（md）、braces（json）、file、image、wrench、lightbulb、git-branch、undo-2（uncommit）、wallpaper/cpu/wand-sparkles/bot/plug/ellipsis-vertical（设置导航）。

## 13. GPUI 实现注记

- 拖拽区：`WindowControlArea::Drag` 挂 topbar 背景，子按钮命中豁免（已验证的坑）
- slp 宽度拖拽 → dock state 持久化（position/panel/width 已有 schema）
- statusbar 语境化 = 内容列 `margin-bottom: -25px` + 面板段独立条；GPUI 里直接给 content-col 换算高度
- 会话导航：节点位置按轮数等分 + 滚动比例点亮；跳转 scroll_to 消息锚点；未读回执数需 SessionRuntime 状态 + 持久化（配合 project 管理）
- 色阶：chrome/nav 需在 theme.rs 新增两个派生档（或 mist 常量直落）
- 覆盖层（导航 flyout / 设置弹窗）：gpui overlay + z 顺序，不参与布局

## 14. 迭代史要点（v1→v35）

100px 居中标签 → 左对齐+状态行 → 无分组贯穿线 → 三按钮上 topbar → Obsidian 自绘 topbar → 图标化两段 topbar → 对齐系统 → 三层色阶 → 连体 tab 出血修正 ×4 → plb 66px → 会话选中 soft-tint → 三面板 tabs → 内容区终端 → 面板 tab 连体 → Lucide 图标 → 一体 composer → 会话导航（minimap 比例尺 → pi-web 灰点灰线）→ 画板自适应视口 → 文档转正（docs/UI设计/）→ topbar 38/statusbar 30 → 首标签贴顶 → 等高标签+空闲时间行 → 空闲降灰+10 上限 → slp 可拖拽+变量化 → 对齐锚点修正 → Obsidian 收起（全隐+镜像钮）→ 4px 等距 → statusbar 仅面板段 → 内容区 tab 化（terminal/md）→ Obsidian tab 凸起/平铺 → 设置弹窗 → 注释清零。
