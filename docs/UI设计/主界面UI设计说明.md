# pi-flash 主界面设计说明

> 设计稿：[主界面UI设计-2.html](./主界面UI设计-2.html)（psp 一体列表版，可交互设计稿，双击浏览器打开）
> 状态：已随实现落地的现行设计 · mist 主题（色值取自 `appearance/theme.rs`）
> 文件约定：HTML 内**不写注释、不写页面说明文字**
> 演示哈希：`#collapsed` 收起侧栏 · `#files` / `#git` 切面板 · `#term` / `#md` / `#both` 内容区视图 · `#history` git 历史页 · `#nav` 会话导航展开 · `#settings` 设置弹窗
> 尺寸/令牌的权威出处 = [UI组件规范.md](./UI组件规范.md)（`crates/app/src/ui/tokens.rs`）；本文与代码冲突处以代码为准

## 1. 布局总览

```
┌────────────────────────┬──────────────────────────────────┐
│ topbar 左段（chrome）    │ topbar 右段（chrome）              │
│ [收放]                  │ [会话tab][自由标签区…]      ─ □ ×  │
├────────────────────────┴─────┬────────────────────────────┤
│ psp 300–500px                │ 内容区（聊天/终端/文件/…）    │
│ 项目+会话一体列表|文件树|git    │                            │
├──────────────────────────────┴───┐                        │
│ statusbar（仅面板段）[项目tab…]    │ ← 内容区直通窗口底      │
└──────────────────────────────────┴────────────────────────┘
```

- 项目与会话合并为 psp 单列一体列表（ChatGPT 式，设计核心=隐藏复杂度、减少第一层信息量）
- topbar 左段仅收放钮（「菜单」三横钮藏在 dev-flags 后，默认不渲染）；工具钮全部在 psp 控制栏
- slp|内容一条全高竖线；statusbar 只在面板段，内容区所有视图直通窗口底（statusbar 不渲染在内容列下，无补偿负边距）

## 2. 尺寸系统

| 项 | 值 |
|---|---|
| topbar 高 | **40px**（两段同高；窗口控制钮高度跟随） |
| statusbar 高 | 40px，宽度 = slp；**收起态整个隐藏** |
| psp 宽 | **可拖拽 300–500px**，首次默认 340px（分隔线感应 12px + 双击复位 300） |
| composer | 宽随「会话内容宽度」设置封顶（默认 920px，小屏自动取可用宽）；操作栏距胶囊下边框 7px |
| 内容列 | 直通窗口底（statusbar 只渲染在面板段） |
| 内容区 tab | 高 = topbar − 4 = 36px，激活连体卡底缘 mb(−1) 融入下方内容区 |

变量化硬约束不变：`slp_w` 派生一切（panel-col/贯穿线/statusbar 宽）。

## 3. 三层色阶

| 层 | token | mist 色值 | 区域 |
|---|---|---|---|
| chrome | `t.chrome` | `#dfe9e5` | topbar 两段 + statusbar |
| nav | `t.nav` | `#eef4f1` | psp |
| content | `t.bg` | `#f4f8f7` | 聊天 / md 预览 |

- 选中语言：**会话选中 = 中性前景薄纱（text 10%，悬停同色，非 accent）**；**活动项目 = 名字提亮加粗**；tab 激活 = 填充色连体（状态栏 tab 向上顶穿、内容区 tab 向下凸起、git 头部 tab 同）
- 辅助灰：`text_faint #a3b0aa`（placeholder / 非操作信息 / 导航工具列表）；title 淡色 `text_soft #8a9d95`

## 4. psp（项目会话面板，单列）

**控制栏**（38px 高、无下 border，`tokens::panel_ctrl`）：
- 左二 icon+文字钮（钮盒高 30、大图标 LG18）——打开项目（iconfont 实底）/ 新建会话（iconfont 实底 19px）
- 右三纯 icon 钮（定宽 26、图标 MD16、gap 2）——搜索 / 列表切换（项目分组 ↔ 最近会话直切，图标随模式换）/ 最大化；**三颗常态不可见**（`autohide_icons` 总开关，hover 本控制栏才显影）
- 列表方式不再走菜单（⋯ 两级排序菜单已撤；`sort_mode` 仅存档读写）

**分组列表**（默认）：
- **列表两侧 10px 留白**（`LIST_PX`）：选中/悬停高亮块不满宽，右侧留白给 slp 分隔线与拖拽区
- **会话行**：`pl 2 / pr 10` + 圆角 8px，**真行高 34px**（选中/悬停色条 32，上下各内缩 1px，相邻条间留 2px）
- **选中 / 悬停 = 中性前景薄纱**（text 10%，同色，**非 accent**）
- **三区结构**：**图标区** 15px 状态槽（运行=spinner(accent) / 通知点 7px 三色（完成=青蓝 NOTICE、内部中断=黄 WARN、外部错误=红 danger） / 空槽占位对齐）+ **标题区**（flex 撑满）+ **时间区**（面板字号 −2 `ui(10)` text_faint、固定 38px 右对齐成列）；**标题与项目名同一对齐列**
- 标题溢出**不用省略号**：尾部 **24px** 渐变淡出（mask，ZCode 1.5rem 同款）；**量宽确认溢出才挂**
- 时间区格式（`fmt_ago`）：最近活动距今 **<5 分钟=「刚刚」；<60 分钟=N分钟；<24 小时=N小时；否则=N天**；详情卡内用 `time_ago`（非「刚刚」带「前」，走 i18n）
- 会话置顶（pin）：置顶会话收进「置顶」伪项目组（不收折/不拖拽），有序表跨重启保留
- 项目行 hover：深色 tooltip（短目录名 + 全路径 mono）+ 右侧 **＋** 显影（26×26 圆角 6、图标 MD16、gap 2、负右距 10 对齐列表右线）= 新会话入组即选中并激活项目
- 项目行**右键** = 项目详情卡（删除确认流）；会话行**右键** = 会话详情卡（见下）；项目行支持**拖拽排序**（用户排序持久化；「手动排序」= 初始序）
- 项目行的打开入口 = 控制栏「打开项目」与文件树标题项目下拉（树根跨项目快切）

**平铺列表**：全部会话按最近顺序平铺、无项目头；点会话仍记选中态与项目归属

**会话行右键详情卡**（浅色浮层，**338px**，锚右键点四象限、上下 12px 让出光标）：
- 标题（点击=打开并改名，Enter/blur 双路提交）
- folder-closed 项目名 / clock 最后活动（"N小时前"） / message N 条消息 / 置顶 pin 钮 / 通宽分隔线
- 底部右对齐：**删除**（红 ghost 钮）→ 点击原地换 **取消/确认**（确认执行、组即时刷新）
- 交互规格：点卡外即收（再右键别行 = 换卡）；卡片内点击 stopPropagation

## 5. 文件树面板

Zed 式：根项目行（标题项目下拉跨项目快切）、每级缩进 guide 线、类型图标（folder-open/folder、md=book、html=file-code、json=braces、sh=file）+ 名称 + git 徽标，选中行全宽色带；点 .md 行打开内容区预览 tab

## 6. git 面板

- **头部单行**：`项目title（淡色 text_soft，min 100px / max 50%，截断） + Changes(N)/History tabs 右靠`（激活连体；项目名随选中会话切换；git 面板全部文字 = 面板字号 −1）
- Changes：View Diff（iconfont 加号底线图标）+ Stage All ∨；Untracked 变更树（开合文件夹、绿 + 徽标、行尾复选框、选中 accent 描边）；底部 = `⎇ main + ↑N Push ∨` / commit message 输入框（无边框融入式，单行 30 高）/ `Commit Tracked ∨`；最近提交条 + uncommit
- History：提交列表（标题 + ↑ 推送小钮 + 元信息）
- 无窗口操作图标、无 AI 写描述按钮

## 7. topbar（40px 两段）

- 左段（chrome）：**仅收放钮**（图标左缘落 ICON_LINE 对齐线）；「菜单」三横钮仅 dev-flags `pi_ext_market` 开启时渲染
- 右段：中段标签栏（置顶会话 tab 固定段 + 自由标签区：终端/文件/更新日志/市场/任务/搜索/插件页，距条左缘 10px，自由区独立横向滑动）+ 20px 拖拽区 + 窗口控制（42×40，关闭悬停红；**设置钮不在此段**——已下放 statusbar 右端）
- 收起（Obsidian 式）：面板 + statusbar **全部隐藏**；收放钮跳到右段起点（竖线镜像位，4px 等距，后随 1px 竖线分隔）

## 8. 内容区 tab

激活=凸起卡片（bg 色、顶圆角、压底线、× 可见）；非激活=平铺文字；内容视图状态机 `ContentView`（chat / term / file / changelog / 双市场 / tasks / search / session_search / plugin 页）；自由标签区距条左缘 10px

## 9. composer（一体式）

- 单容器 16px 圆角 1px 边框（焦点态 accent、`!` shell 模式工具绿、否则 t.border）；**宽随「会话内容宽度」封顶**（默认 920px），居中
- **悬浮胶囊（独立性的表达）**：叠在聊天区上（消息从胶囊后滚过），`shadow_lg` 双层软影；wrapper `pointer-events:none`、胶囊 `auto` + `occlude`（叠住区不吞下层 hover/滚轮）
- placeholder：淡灰 `text_faint`（按面板字号）；输入文字随「会话字体」（`sess_size(0)`）
- 编辑区 AutoGrow 3–10 行，超出出纵向滚动条
- 控件行：左 = 图片 + 工具预设「默认∨」；右 = 上下文用量环 + 模型∨ + 思考∨ + **圆形发送 ↑**（36×36 正圆、accent 底；运行中变停止、圆角收成 9）
- 操作栏距下边框 7px；无压缩/铃声/AI 按钮

## 10. statusbar（40px，仅面板段）

「项目」tab（46px 宽 briefcase icon 连体卡，accent 常亮）+ PF 插件 tab（清单声明 `status_bar` 时出现，46px 同形；未激活融入 chrome 背景）；tab 高 = bar 高 −4；右端 = 设置钮（018 下放）；**收起态整个隐藏**

## 11. 设置弹窗

80%×98% 居中（`big_card` 0.8×0.98），自带 **36px** chrome 顶条 + 30×30 关闭钮（hover 红底白字）；左导航 160px（界面/模型/技能/扩展/子代理/MCP/其他/关于/快捷键，图标 15px；子代理页随功能总开关隐藏）；页面清单与规格见 [设置页面UI.md](./设置页面UI.md) 与 UI组件规范 §10（无页标题无说明文字）

## 12. 会话导航（比例尺）

- 右缘 10px 竖条 rail，**屏高 75%**、按「屏高 − composer 高×50%」居中；轮次均分切割整条（1 轮整条弱主题色、N 轮切 N 段 flex_1 等分 + 2px 缝），**>10 轮走百分比桶映射**（段数封顶 10，轮 k → 段 k×10/N，点击回桶首）
- 悬停展开 **500px** 发言列表覆盖层（accent 1px 描边 + 5px 圆角，不挤压布局；下缘与输入面板底线对齐）

## 13. 对话区消息

**用户气泡**（右对齐）：
- 操作栏**默认隐藏**（opacity 0），hover 消息行（整行感应区）显影（opacity 1）
- 内容：⧉复制 / ✎编辑 / ⑂新分支（lucide 图标 + 文字）+ 时间；操作项 `text_dim` hover 提亮，时间淡灰

**等待动画**（发送后 → agent 首个 token 之前）：
- 结构：模型名 + 旋转 spark +「正在思考…」（省略号循环，accent 色）
- 生命周期：`SessionRuntime.phase_waiting` 发送成功置位；assistant `message_start`/`message_update`、`agent_settled`/`agent_end`、`prompt` 失败、用户中止（Esc）清零
- 实现坑：回显去重标记 `pending_echo` 必须与 `phase_waiting` 解耦 —— pi 回显 user 消息是瞬时的，两者共用一个标记会让等待行「一闪即灭」

**agent 回复**（左对齐）：
- 思考 + 工具调用**轮末整形收进「工作详情」折叠行**（pi-web TurnGroup parity：有最终回答时默认折叠、流式中/无最终回答时展开；chevron 旋转 90° 展开）：`工作详情 · N 条消息 · N 次工具调用`；展开 = 工具调用列表（tool · 对象）；运行中无折叠行（平铺就地展开）
- 操作栏**默认隐藏**、hover 消息块显示；左对齐三块：⧉复制（整轮）/ 用时（"用时1分55秒"）/ 时间
- **时间格式统一**「X月X日 HH:MM」（跨年带年份）；非操作信息淡灰 `text_faint`

## 14. 图标系统

- **Lucide 24 栅格 2px 描边**：search、image、wrench、lightbulb、copy、pencil、git-branch、check、chevron、undo-2、sliders-horizontal、wallpaper/cpu/wand-sparkles/bot/plug/ellipsis-vertical、folder-tree、messages-square、file 系
- **iconfont 实底**（用户提供）：打开项目（1099 箱）、新建会话（1024 箱）、文件浏览器、终端、垃圾桶、列表方式、分组/平铺列表、时钟、手动排序、View Diff 加号底线
- **混排三规则**：① fill 转 `currentColor` 继承；② iconfont 轮廓字线宽写死在视箱单位里，需 svg 根加 stroke 配重（project=31/new chat=24/菜单组=40/时钟=40，按显示尺寸折算）；③ **viewBox 收紧到墨迹边界**（~88% 填充率）消除"外围束缚"，否则图形本体比 Lucide 小一圈
- **icon hover 统一规格**：hover 时**图标自身动效**——**上抬 1px + 放大约 1px（scale 1.06，0.12s ease）**，图标色提亮为 `text`，**无底色**。适用：topbar 图标钮 / 项目行 ＋ / statusbar tab / git 钮与 tab / tab 关闭 × / composer 图标 / 模型选择钮 / 消息操作行 act。**豁免**：窗口控制三钮（Windows 惯例底色 hover）、关闭/删除/确认（danger 红）、发送钮（accent-hover）、文字 pill 与菜单行（cmp-pill/ghost-btn/set-nav/pm-item/hc-cancel 保留底色）、slp resizer、导航比例尺节点——语义色不参与统一
- **项目行交互**：悬停**不给背景色**；＋ 钮 hover 显影，交互中不消失

## 15. GPUI 实现注记

- 项目列表=psp 树状结构（项目组+会话子行），展开态持久化
- psp 交互映射：组收起/展开、hover 浮层（tooltip/两级菜单）用 gpui overlay；详情卡改名=行内编辑态
- 工作详情折叠行 = 消息元数据（消息数/工具调用数 + 工具流），SessionRuntime 聚合
- 消息操作栏 hover 显隐 = 消息块 hover 状态；复制/编辑/新分支接会话分支机制
- 项目拖拽排序持久化（`proj_order` 用户排序表）；会话置顶有序表（`session_pins`）；跨项目预载、启动恢复照旧
