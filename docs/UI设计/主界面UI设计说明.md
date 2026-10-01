# pi-flash 主界面设计说明

> 现行定稿：[主界面UI设计-2.html](./主界面UI设计-2.html)（psp 一体列表版，可交互设计稿，双击浏览器打开）
> 旧版（v53 plb 版）已删除，演进过程见 git 历史
> 状态：设计封稿 · 2026-09-29 · mist 主题
> 文件约定：HTML 内**不写注释、不写页面说明文字**；补丁脚本存于 `tmp/design/patch_*.py`
> 演示哈希：`#collapsed` 收起侧栏 · `#files` / `#git` 切面板 · `#term` / `#md` / `#both` 内容区视图 · `#history` git 历史页 · `#nav` 会话导航展开 · `#settings` 设置弹窗

## 1. 布局总览

```
┌────────────────────────┬──────────────────────────────────┐
│ topbar 左段（chrome）    │ topbar 右段（chrome）              │
│ [收放]                  │ [终端tab][md tab]   [设置]│─□×    │
├────────────────────────┴─────┬────────────────────────────┤
│ psp 250–500px                │ 内容区（聊天/终端/md 预览）   │
│ 项目+会话一体列表|文件树|git    │                            │
├──────────────────────────────┴───┐                        │
│ statusbar（仅面板段）[3 tabs]      │ ← 内容区直通窗口底      │
└──────────────────────────────────┴────────────────────────┘
```

- **plb（66px 项目竖栏）已废除**：项目与会话合并为 psp 单列一体列表（ChatGPT 式，设计核心=隐藏复杂度、减少第一层信息量）
- topbar 左段只剩收放钮；原工具钮全部下沉 psp title 行
- slp|内容一条全高竖线；statusbar 只在面板段，内容区所有视图直通窗口底（内容列下探 25px）

## 2. 尺寸系统（CSS 变量）

| 项 | 值 |
|---|---|
| topbar 高 | **36px**（两段同高；窗口控制钮高度跟随） |
| statusbar 高 | 30px，宽度 = slp；**收起态整个隐藏**（v54 前 statusbar 残留 bug 的修正） |
| psp 宽 | **可拖拽 250–500px**，默认 282px（分隔线 6px 命中区 + 双击复位） |
| composer | 宽 **内容区 75%**、min 500px；内边距 5/7/3 |
| 内容列 | 无条件 `margin-bottom: -25px` 下探 |
| 内容区 tab | 常态 36px、激活连体态 33px（随 topbar -2px 同步） |

变量化硬约束不变：`--slp-w` 派生一切（panel-col/贯穿线/statusbar 宽）。

## 3. 三层色阶

| 层 | token | 色值 | 区域 |
|---|---|---|---|
| chrome | `--chrome` | `#dfe9e5` | topbar 两段 + statusbar |
| nav | `--nav` | `#eef4f1` | psp |
| content | `--bg` | `#f4f8f7` | 聊天 / md 预览 |

- 选中语言：**会话选中 = accent 10% soft-tint**（色相偏移，非明度差）；**活动项目 = 名字提亮加粗**（v60 起无 accent bar）；tab 激活 = 填充色连体（状态栏 tab 向上顶穿、内容区 tab 向下凸起、git 头部 tab 同）
- 淡灰辅助色 `#a3b0aa`（placeholder / 非操作信息 / 导航工具列表）；title 淡色 `#8a9d95`

## 4. psp（项目会话面板，单列）

**title 行**（操作行，灰字 `#8a9d95`、行高 24px）：
- 左：文案随列表模式切换——分组列表=「项目」，平铺=「会话」
- 右：**4 个常显钮**（18px 图标，gap 10px + per-icon margin 配平，无 tip）——打开项目（iconfont 实底）/ 新建会话（iconfont 实底，19px）/ 会话查询（lucide search）/ 排序（ellipsis，无 tip）

**⋯ 排序菜单**（一级菜单**左对齐**触发钮左缘；二级菜单右侧弹出）：
- 列表方式 › **项目分组列表** ✓ / **最近会话列表**（iconfont 图标 + 选中勾，切换后 title 文案跟随）
- 排序方式 › **按更新时间排序** ✓ / **手动排序**（iconfont 图标 + 勾；手动=恢复初始序，真拖拽待实现）

**分组列表**（默认）——v55 对齐 ZCode TaskListItem 规格（源码实测）：
- **列表两侧 12px 留白**（p-3）：选中/悬停高亮块不满宽，右侧留白给 slp 分隔线与拖拽区
- **会话行**：`pl 10 / pr 4 / py 4` + **行距 2px**（space-y-0.5）+ 圆角 8px，行高 32px——上下不再黏连
- **选中 / 悬停 = 中性前景薄纱**（text 10% / 5%，**非 accent**——accent 底会与毛玻璃互相染色）
- **三区结构**：**图标区** 15px 状态槽（运行=旋转圈 / 未读=绿点 / 空槽占位对齐）+ **标题区**（flex 撑满）+ **时间区**（11px text-faint、固定 38px 右对齐成列）；**标题与项目名同一对齐列**
- 标题溢出**不用省略号**：尾部 **24px** 渐变淡出（mask，ZCode 1.5rem 同款）；**量宽确认溢出才挂**，短标题不糊尾
- 时间区格式（`fmtAgo` 规格）：最近活动距今 **<5 分钟=「刚刚」；<60 分钟=N分钟；<24 小时=N小时；否则=N天**；详情卡内非「刚刚」加「前」
- 项目行 hover：深色 tooltip（短目录名 + 全路径 mono）+ 右侧 **⋯ / ＋** 显现（22px 槽、图标 18px、gap 2px）
  - ⋯ 菜单（左对齐）：在文件浏览器中打开 / 在终端中打开（开终端 + slp 切文件树）/ 删除项目及所有会话（红字，iconfont 图标）
  - ＋ = 新会话入组即选中并激活项目
- **≤5 个项目**按最近更新倒序（settings-其他「默认加载项目数」可调）；上限由 10 改 5

**平铺列表**：全部会话按最近顺序平铺、无项目头，title 切「会话」；点会话仍记选中态与项目归属

**会话行 hover 详情卡**（浅色浮层，258px，锚定侧栏右缘外 8px）：
- 标题（hover 蒙 `--bg-hover` 引导点击 → 变输入框改名，Enter/blur 双路提交）
- folder-closed 项目名 / clock 最后活动（"N小时前"） / message N 条消息 / 通宽分隔线
- 底部右对齐：**删除**（红 ghost 钮）→ 点击原地换 **取消/确认**（确认执行、组即时刷新）
- 交互三坑（已修）：settings-mask 漏 `</div>` 吞浮层、离行 300ms 宽限 + 进卡取消隐藏、卡片内点击 stopPropagation（否则改名即闪退）

## 5. 文件树面板（不变）

Zed 式：根项目行、每级缩进 guide 线、类型图标（folder-open/folder、md=book、html=file-code、json=braces、sh=file）、选中行全宽色带；点 .md 行打开内容区预览 tab

## 6. git 面板

- **头部单行**：`项目title（淡色 #8a9d95，min 100px / max 50%，截断） + Changes(N)/History tabs 右靠`（tab 11.5px，激活连体；项目名随选中会话切换）
- Changes：View Diff（iconfont 加号底线图标）+ Stage All ∨；Untracked 变更树（开合文件夹、绿 + 徽标、行尾复选框、选中 accent 描边）；底部 = `⎇ main + ↑N Push ∨` / commit message 大区（无边框融入式 96px）/ `Commit Tracked ∨`；最近提交条 + uncommit
- History：提交列表（○ 多多有鱼 · 时间 · hash + ↑ 推送小钮）
- 明确去掉：窗口操作图标、AI 写描述按钮

## 7. topbar（36px 两段）

- 左段（chrome）：**仅收放钮**（贴左 5px）——打开项目/新建会话/搜索/终端四钮已下沉或删除
- 右段：内容区 tabs + 设置（sliders-horizontal）+ 竖线 + 窗口控制（42×36，关闭悬停红）
- 收起（Obsidian 式）：面板 + statusbar **全部隐藏**；收放钮跳到右段起点（竖线镜像），4px 等距

## 8. 内容区 tab（不变，Obsidian 式）

激活=凸起卡片（bg 色、顶圆角、压底线、× 可见）；非激活=平铺文字；状态机 chat/term/md；term|md tab 距分隔线 10px

## 9. composer（一体式）

- 单容器 16px 圆角 1.5px 边框；**宽 75% / min 500px**，居中，整体高出窗口底 10px
- **悬浮胶囊（独立性的表达）**：上浮 18px 叠在聊天区上（消息从胶囊后滚过），双层软影 `0 8px 24px / 0 2px 6px rgba(20,40,34,…)`；wrapper `pointer-events:none`、胶囊 `auto`（叠住区不吞点击）；聊天区底部 padding 12→30px 让位
- placeholder：`/使用命令，shift回车换行`，淡灰 `#a3b0aa`
- 控件行：左 = 图片 + 工具预设「默认∨」；右 = 上下文用量环 + 模型∨ + 思考∨ + **圆形发送 ↑**（28px、上提 3px、accent 底）
- 操作栏距下边框 3px；无压缩/铃声/AI 按钮

## 10. statusbar（30px，仅面板段）

三个面板 tab（psp/文件树/Git）46px 宽 icon 即标签；**收起态整个隐藏**

## 11. 设置弹窗

70%×98% 居中，自带 **36px** topbar；左导航 200px 六页签带图标；界面页含「默认加载项目数·5」；其余同前（无页标题无说明文字）

## 12. 会话导航（比例尺）

- **垂直居中**于内容区（上下等距，v27 的"top 下 100px/70% 高"废止）；**高 65%**；节点 ≤10、灰点灰线分段连接、当前位 accent
- 悬停展开 326px 发言列表覆盖层（不挤压布局）

## 13. 对话区消息

**用户气泡**（右对齐）：
- 操作栏**默认隐藏**，hover 消息行（整行感应区）淡入 0.15s
- 内容：⧉复制 / ✎编辑 / ⑂新分支（lucide 图标 + 文字）+ 时间；操作项 `--text-dim` hover 提亮，时间淡灰

**等待动画**（发送后 → agent 首个 token 之前）：
- 结构：模型名（11px `--text-dim`）+ 旋转 spark（14px，1.4s/圈）+「正在思考…」（省略号 1.4s/4 步循环，accent 色）。~~下方 260×10 shimmer 骨架条~~ **已删**（已有文案，骨架条是纯视觉噪音）
- 生命周期：`SessionRuntime.phase_waiting` 发送成功置位；assistant `message_start`/`message_update`、`agent_settled`/`agent_end`、`prompt` 失败、用户中止（Esc）清零
- 实现坑：回显去重标记 `pending_echo` 必须与 `phase_waiting` 解耦 —— pi 回显 user 消息是瞬时的，两者共用一个标记会让等待行「一闪即灭」

**agent 回复**（左对齐）：
- 开头**工作详情折叠行**（pi-web 特色，无框、与正文左对齐、chevron 旋转 90° 展开）：`工作详情 · N 条消息 · N 次工具调用`；展开 = 左细线缩进的工具调用列表（tool · 对象）；仅**已完成**回复有（工作中无）
- 操作栏**默认隐藏**、hover 消息块显示；左对齐三块：⧉复制 / 用时（"用时1分55秒"）/ 时间；**支出信息已删**（冗余，可下沉 hover 或会话级统计）
- **时间格式统一**「X月X日 HH:MM」；非操作信息淡灰 `#a3b0aa`

## 14. 图标系统

- **Lucide 24 栅格 2px 描边**：search、image、wrench、lightbulb、copy、pencil、git-branch、check、chevron、undo-2、sliders-horizontal、wallpaper/cpu/wand-sparkles/bot/plug/ellipsis-vertical、folder-tree、messages-square、file 系
- **iconfont 实底**（用户提供）：打开项目（1099 箱）、新建会话（1024 箱）、文件浏览器、终端、垃圾桶、列表方式、分组/平铺列表、时钟、手动排序、View Diff 加号底线
- **混排三规则**：① fill 转 `currentColor` 继承；② iconfont 轮廓字线宽写死在视箱单位里，需 svg 根加 stroke 配重（project=31/new chat=24/菜单组=40/时钟=40，按显示尺寸折算）；③ **viewBox 收紧到墨迹边界**（~88% 填充率）消除"外围束缚"，否则图形本体比 Lucide 小一圈

## 15. GPUI 实现注记

- plb 已废：项目列表=psp 树状结构（项目组+会话子行），展开态持久化
- psp 交互映射：组收起/展开、hover 浮层（tooltip/详情卡/两级菜单）用 gpui overlay；详情卡改名=行内编辑态
- 工作详情折叠行 = 消息元数据（消息数/工具调用数 + 工具流），SessionRuntime 聚合
- 消息操作栏 hover 显隐 = 消息块 hover 状态；复制/编辑/新分支接会话分支机制
- 手动排序需会话顺序持久化（`_manual` 索引示意）；跨项目预载、启动恢复照旧
- 拖拽/色阶/覆盖层注记同前版

