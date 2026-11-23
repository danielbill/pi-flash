# 046 pif-ui 自动化验收记录（2026-11-15，返工版）

前置：`cargo build`；实例用全新 tmp 目录 + `PI_FLASH_AUTOMATION=auto`。

## 载体拍板（同日返工）

用户对照 `docs/UI设计/任务管理-原型.html` 复核后拍板：**载体 = 内容区页签**
（PanelTab::Tasks / ContentView::Tasks，市场同款五处同步），页头横向
【任务】【日志】双 tab；日志行按用户截图（类型图标 + 任务名（可点下划线 /
停用·跳过灰名）+ 结果点/spinner + 右对齐 mono `MM-DD HH:MM:SS`，完成点
绿色；无 meta/耗时列——原稿 log-row 设计未拍板作废）。

## 自动化基建

- op：`tasks.open`（`--arg tab=0|1`）/ `tasks.close`（摘 PanelTab::Tasks，
  close_panel_tab 回退链路）；
- 断言面：`snapshot dialogs` 的 `tasks` 键 = {tab, sel, defs, running,
  runs, limit}（open = PanelTab 在签列 + content_view 命中）；
- `snapshot app` 的 `content_view` = "tasks"（页签载体直接可见）。

## 执行记录（run.sh，两实例，13 断言）

实例 A：`PI_FLASH_AUTO_TASKS=1`

```
✓ 实例 A 启动
✓ 初始未打开                 # "tasks": null
✓ 开页 = content_view tasks   # 页签载体
✓ 任务 tab（默认 0）
✓ 列表空态 defs=0
✓ 上限 20
✓ 日志 tab
✓ 日志空态 runs=0
✓ 关闭页签
✓ flag-on: source 注册留痕
```

实例 B：不设 flag（默认关）

```
✓ 实例 B 启动
✓ flag-off: source 不注册
✓ flag-off: 页签缺席
```

RESULT: fail=0。

## 执行记录 2（2026-11-15 新建栏 bug 修，单实例 11 断言）

用户截图报「新建栏全是 bug」：点工具 pill 把 ProjectPicker 模态也拉开了
（点击冒泡到 composer 操作栏行级 handler），工具菜单浮在模态顶上。
修：`trg_wrap` stop_propagation + 项目 pill 独立 handler + `menus_overlay`
挪到 dialogs 之前 + TIME 拾取补 notify + FREQ 菜单补五档 + INTERVAL 菜单
加档位切换 + name/desc 输入 flat 化对齐 HTML + 操作栏补 ext/MCP stat pill。

```
✓ 开页 content_view tasks；snapshot dialogs.tasks.popup 键新增（None）
✓ 点工具 pill (821,465) → popup=8 且 dialog=null   # 截图 bug 回归断言
✓ 点外 (1000,600) → popup=null                     # menu_layer 遮挡关
✓ 点项目 pill (750,464) → dialog=project_picker 且 popup=null
✓ dialog.close → 点调度 freq pill → popup=1
✓ freq 菜单五档（每 1 分钟/每小时/每天✓/每周/每月，像素复核）
✓ 点「每 1 分钟」→ unit 切 cmin，单钮「每 1 分」(82×28)
✓ 点单钮 → popup=2（间隔清单 + 分组线 + 四档切换，像素复核）
✓ 新建复位 → 点 09:00 pill → popup=6（tpick 双列锚定 pill 下方）
✓ 点小时 05 → 按钮/摘要即时 05:00                  # notify 修
✓ 全量 app+pi-link+pf-context 888 过 0 挂
```

RESULT: fail=0。

## 执行记录 3（2026-11-16 五条拍板改版，不再参考 HTML）

用户拍板：①列表/详情 = 40%/60%（列表最小宽 400）；②页头二级 tab = PF
插件市场简易 tab（MONO + `|` 分隔）；③详情 label/控件左右排（subagent
表单 w96/w250 同款，保存 = Primary）；④列表分两组：上【脚本任务】下
【Agent 任务】（subagent list 同款 group_header + 30px sidebar_item 行，
行 = 结果槽 + 名（下划线开话/停用灰）+ 右缘 mono 时间）；⑤空列表 =
「暂无任务」。

```
✓ 空库开页：左列居中「暂无任务」，40/60 分栏（分栏线 = 内容区 40%）
✓ 管道 tab「任务 | 日志」形制（弃融底卡）
✓ 新建表单：名称 w96/w250 + seg/调度/指令 label 左右式，保存 Primary
✓ 保存校验链：prompt 空拒 / project 空拒（toast）/ script path 空拒（toast）
✓ 项目 pill → ProjectPicker（task_form 路由回填，不切工作区）
✓ 建任务成功：defs=1 sel 回填，组头【脚本任务 0】/【Agent 任务 1】
✓ 行点击选中：bg_selected + semibold，表单回填（名称/指令/项目 pill 20观宏知微）
✓ 三仓测试全绿 + check_arch/ui-raw 守卫 OK
```

RESULT: fail=0。

## 执行记录 4（2026-11-16 续：表单四条拍板）

①Agent 字段序 = 名称/类型/定时/**项目**/指令；②脚本 = 名称/类型/定时/
脚本（不变）；③项目组件 = 新会话页 action_bar 项目 chip 同款（folder +
项目名/选择项目，h28 hover 洗底），挪到输入面板上方独立 field 行；
④指令框 = 新会话页 composer 胶囊复用：rounded16 + 焦点 accent 框 +
shadow、去发送钮、输入 10 行 ≈ +200px，操作栏 = +/工具档/扩展/MCP ……
模型/思考（原项目 pill 删除）；＋号 = 统一菜单技能页锚定弹层
（popup_slot::PLUS，点行插 `/skill:name ` 进指令，空态「暂无可选项」）。

```
✓ Agent 表单五行序（截图核对）
✓ 项目 chip 点击开 ProjectPicker → Enter 默认行 → chip 显「20观宏知微」
✓ + 钮 → popup=10 锚定弹层（无项目上下文 = 暂无可选项）
✓ composer 点击聚焦 → 胶囊边框 accent
✓ 脚本模式四行序 + note
✓ 三仓测试全绿 + check_ui_raw OK
```

RESULT: fail=0。

## 执行记录 5（2026-11-17 任务页五条拍板：icon/按钮/字段名/padding/+弹层）

①icon 全对齐任务管理-原型.html（新建钮去「＋」字符改子代理同款 Primary
纯文字钮；列表行加设计稿 tico——脚本=file-code（新资产）、Agent=bot；
folder 资产补设计稿横线 `M2 10h20`）；②按钮对齐子代理页：页头新建
Btn::Secondary→Primary（保存原已是）；③字段名 agent 侧 名称/类型/定时/
项目/指令，前三项两侧同时改（task.name/type/sched 三键 i18n 改值，脚本侧
共用同键）；④指令 textarea 内 padding 对齐 InputPanel（胶囊 pb 6→7 =
inputarea 同值；文本内缩 6→18 = wrapper 6 + 组件 input_px 12，flat 档
pl_0 由 wrapper 补足）；⑤「+」弹层彻底重做 = InputPanel slash_menu_view
同构复刻（搜索栏 + tab 条静态复刻[技能钉活/其余 dim] + 32px 条目卡
wand+mono 名+右说明，点行插 `/skill:name `；定位弃按钮居中锚改**贴指令
胶囊正上方同宽缝 8**——胶囊 bounds 新增 composer tracker 槽；搜索框实体
plus_search 懒创建镜像 plus_query，开菜单即清旧词；plus_row 豁免
ui-raw: ok，§10.8 登记）。

单实例（PI_FLASH_AUTO_TASKS=1，project.switch 注cwd 后 tasks 页会被切走
——重开即可）：

```
✓ 开页：字段序 名称/类型/定时/项目/指令（新建钮 Primary 纯文字，像素核对）
✓ 脚本 seg：名称/类型/定时/脚本文件 + note（前三项两侧同改）
✓ + 钮 (1035,499) → popup=10：搜索栏+tab 条（技能 active/命令·项目文件·会话
  dim 0.45）+ 暂无可选项，贴胶囊正上方与胶囊同宽（像素核对）
✓ 点外 (597,512) → popup=null
✓ 建任务（name+prompt 注入 → 保存）defs=1 sel 回填，行=tico(bot)+名，
  组头【脚本任务 0】【Agent 任务 1】
✓ 项目 chip folder 图标带横线（设计稿版），指令文本左缩进与 + 字形对齐
✓ 三仓 529/298/59 全绿 + check_arch/ui-raw OK（豁免 9）
```

RESULT: fail=0。

备注：条目卡实渲染（有技能清单时）未在本实例验到——隔离实例无 pi 会话
不出 skill: 清单；行几何 = menu.rs slash_menu_view card 逐值复刻，插入
链路与原 pill_row 版逐字同（set_value 拼接）。

## 执行记录 6（2026-11-17 续：icon 指认纠偏 + 按钮文案紧排）

用户截图纠偏上一条的第①项：「icon 用设计 html 的」指的是**任务入口
icon**（psp 控制栏「任务」钮 + 「任务管理」页签）——设计稿为日历时钟
（pc-btn/c-tab 同一枚，`M21 7.338…` 日历叠时钟），原用 clock-4 光秃钟面
不算数。另：按钮「不对齐子代理」实为文案——task.save「保 存」带全角
空格撑宽一圈，子代理 agents.save 是紧排「保存」。

- 新资产 `icons/calendar-clock.svg`（设计稿 path 逐字落资产），替换
  psp_view「任务」钮 + titlebar PanelTab::Tasks 页签两处（clock-4 仍留
  子代理时长行用）；task.create「创 建」/task.save「保 存」去空格紧排。

```
✓ psp「任务」钮 icon = 日历时钟（像素裁切核对 = 用户截图 3）
✓ 「任务管理」页签 icon = 日历时钟
✓ 保存钮紧排「保存」，与新建/子代理同宽同款
✓ app 测试 + check_arch/ui-raw 全绿
```

RESULT: fail=0。

## 执行记录 7（2026-11-17 四条拍板：InputPanel 直接搬 + 全局技能兜底 + 增高）

用户令「把 InputPanel 直接搬上来，只改高度/宽度、去发送钮」——原 tasks 侧
手搓操作栏/菜单整链退役（popup_slot::MODEL/TIER/THINKING/PLUS +
TaskAnchors 四槽 + composer 槽 + plus_menu 四件 + pill_row/model_menu_body
/tasks 接线），【指令】= session::input::input_area 嵌入态：

- 嵌入态仅四差异：统一菜单限高按胶囊 bounds 测距钳（列内净空 130，
  上限 620）、编辑区 min_h 300、宽度不吃内容列封顶、无发送/停止钮；
  Enter/Ctrl+Enter 在任务页 = 换行（不 send，compose.rs 实体回调按
  content_view 关门）；任务页打字不落会话草稿盘（drafts save_throttled
  同门）；
- 模型/思考/工具档菜单读写分支到 tasks.page（pill.rs / model_picker 三
  处 task_form 分支；工具档不走重绑链）；模型 pill 未选时按 InputPanel
  同链显示实际默认模型名（用户令：不出「继承新会话默认」字样）；
- ext/MCP = 真 picker（勾选语义同会话区：ext=会话清单、MCP=全局
  mcp.json）；「技能出不来」修 = Globals 增 global_skill_commands
  （pi-link discover_global_skills 全局目录启动一次性装载），
  slash_commands 兜底链无条件并入（项目同名让位）——未选项目
  （cwd=None）时 `/` 菜单此前给空表是根因；
- 按钮右缘对齐：页头 pr 12→15（与详情列 15 同值，新建/保存右缘一线）。

```
✓ 真 InputPanel 上墙（bar=真 pills/环，无发送钮；像素核对）
✓ 模型菜单 = 原菜单（无「继承」行；点行落 page.model，pill 显
  DeepSeek V4.1 Flash；新建复位回默认链显示）
✓ ext/MCP 菜单弹出（选择扩展合计 5,565 / 选择 MCP 已开启 1/1）
✓ + 菜单 = 真统一菜单（搜索栏+tab 条+打开文件+Tab 提示），贴胶囊
  上方限高不出屏（净空 130 修正后搜索栏完整）
✓ `/` 出全局技能（image-gen/jev/chart-visualization，cwd=None 兜底链）
✓ Enter 留在任务页不发送；建任务端到端 defs=1 sel 回填
✓ 胶囊 min_h 300 / 菜单上限 620（用户令各 +100）
✓ 新建/保存右缘齐（页头 pr 15）
✓ 三仓 529/298/59 全绿 + check_arch/ui-raw OK
```

RESULT: fail=0。

## 执行记录 8（2026-11-22 五条拍板回归：行尾动作钮/脚本框只读/浏览钮/删除链）

覆盖 11-22 五连提交（b8a5ce60/52f77f84/4c50c3f7/bb0fe759/6e0f9571）。
单实例（`PI_FLASH_AUTO_TASKS=1`，tmp/ui-test-046b）；坐标全部逻辑 px
（=物理/1.5），关键锚：脚本 seg (1037,157)、Agent seg (1096,157)、名称框
(1006,95,610×32)、保存 (1664,111)、指令胶囊 (1006,309,686×247)、列表行 y168、
行尾三钮 ≈x795–877（play/eye-off/trash 22px + gap）。

```
✓ 脚本 seg 表单：名称/类型/定时/脚本文件 + note「脚本执行超过30分钟将无条件终止」
✓ 脚本框只读：点脚本框 (1300,229) → tasks.popup=null 且无 dialog（b8a5ce60 回归）
✓ 浏览钮在位：像素裁切 =「浏览」两字紧排无省略号（6e0f9571 zh 档）
✓ en 档 lang.set name=English → 浏览钮像素裁切 =「Browse」（无省略号）
✓ 建任务链：名称/指令注入（ASCII 逐键）→ 项目 chip → ProjectPicker → Enter
  选中 → 保存 defs=1 sel 回填，组头【脚本任务 0】【Agent 任务 1】，项目 chip
  显「20观宏知微」
✓ 行尾动作钮 hover：行 (401,168) → 「立即运行」tip 弹出（with_tip 350ms）+
  trash 点击 (866,168) 命中 → 确认浮层「删除任务「1」?运行中的会先中止。」
✓ 确认删除 (960,475) → defs=0 sel 清空（4c50c3f7 移除链回归）
```

RESULT: 功能链 fail=0；**视觉层发现 1 个 bug（见下）**。

### ✗→✓ BUG：行尾动作钮显影失效（icon 恒 opacity 0）——当轮已修

- 现象：hover 列表行时 tip（立即运行）正常弹出、22px 钮可点击命中
  （trash 点击 → 确认浮层弹出），但三颗 icon **从不渲染**（play/eye-off/trash
  全程不可见，多帧多坐标像素裁切确认）。
- 根因：`task_row_actions`（tasks/view.rs）把 `.group(reveal)` 和
  `.group_hover(reveal)` 挂在**同一个 div** 上。gpui 求值顺序 = paint 里
  `compute_style_internal`（div.rs:1798，查 GroupHitboxes）先于本元素自身
  `GroupHitboxes::push`（div.rs:1846）→ 自引用 group 永远查不到 hitbox，
  opacity(1) refinement 永不生效。settings agents 行同款显影正常 = group 在
  **祖先行**上（agents/view.rs:104 group / :131 group_hover 分离两元素）。
- 修法：`.group("task-row-act-{def_id}")` 挪到 task_row 的 sidebar_item 行
  容器（when_some(select) 分支），icons div 只留 group_hover。
- 修后复测（实例 tmp/ui-test-046c）：建任务 defs=1 → hover 行 (866,168) →
  **三 icon 显影**（▷/眼斜杠/垃圾桶像素裁切）+ trash tip「移除」→ 点击
  确认删除 defs=0；app 534 全绿。陷阱落 docs/agents/gpui.md。

## 执行记录 9（2026-11-22 会话 type 属性：任务会话从用户会话口径隐身）

拍板：会话 type = user/sub/task 三态，事实源 = pi 会话文件 custom entry
`pf/session-type`（spawn env `PF_SESSION_TYPE=task` + PF 扩展
tasks/session_type.ts 落盘；sub 复用 subagent/child marker 派生；无 marker
= user）。psp 侧栏 / 恢复闸 / 全局搜索按 `SessionInfo.kind == User` 过滤；
052 过期删除不区分 type 全删（现状即然）。显示层 hack（⏱ 名字前缀 +
hidden 注入，执行记录 8 时代）废弃未提交即拆。

```
✓ 会话文件头含 pf/session-type entry（grep = 1；turn 落盘后即查）
✓ psp 侧栏无任务会话（新任务会话是组内最新文件，顶部行仍是旧用户会话）
✓ 每 1 分钟任务连跑 8 次全部正常（调度/台账不受 type 改动影响）
✓ 日志行点击 = content_view chat（会话以普通 tab 打开；session_path 台账）
✓ 搜索层 hit.kind 单测（task/sub/user 三态 + 搜索层不排除、消费侧过滤）
✓ pi-link 300 / app 536 全绿
```

RESULT: fail=0。

### 执行中修掉的盲区：cwd_for_session 对隐身会话失效

日志行点击最初不动：open_session 的 `cwd_for_session` 三路回退
（pool.sessions / psp.projects 组 / ws.cwd）对任务会话全落空——kind 过滤后
不在 psp 组、无项目态 ws.cwd=None → 静默 return（handler 有 eprintln 实证
触发但 switch_to 未达）。修：回退链尾加
`pi_link::sessions::session_header_cwd(path)`（读文件首行头部 cwd）——
「按路径开会话但不在任何已扫清单」的通用兜底（open.rs）。

## 陷阱回填（本轮新增 2 条）

- `pif-ui exec` 位置参数 `k=v` 不解析，必须 `--arg k=v`；
- `snapshot --only` 输出 pretty JSON，grep 键要带冒号后空格；
- `wait` 断言只有 `--eq/--contains/--truthy`（无 `--null`——null 用 snapshot+grep）；
- **dev_flags::summary 原版只看 dev-flags.json**，env 开的旗标日志显 off
  误导排障——已修为 enabled 同语义（env 覆盖计入）；
- `--arg keys=t` 单字符到不了 handler（bad_params 需 `{"keys":…}`）——用
  `--params-file` 传合法 JSON（本记录 type_key helper 即此法）；
- **文本注入坐标锚定**：`snapshot` 面没有 tasks 表单字段，坐标从 `shot` PNG
  读——pif-ui 坐标是逻辑 px（物理/scale_factor 1.5），从显示图直接量会偏
  （本记录显示图:逻辑 ≈ 1.269:1）；精确定位用 `ui.hit_probe` 找最小 hovered
  hitbox 的 bounds；
- **合成 click 不夺焦，必须拆 down/up**：`action=click` 点输入框焦点不动
  （上一框保持焦点、按键继续进旧框——名称框连吞 composer 的键）；改发
  `down`+sleep 0.1+`up` 后焦点才迁移。按钮/seg 等非焦点类用 click 正常；
- bash printf 写 `--params-file` JSON：`\a` 类转义会被 printf 吃掉（
  "ctrl-a" 变非法 JSON）——用 python `json.dumps` 写文件。

## 执行记录 10（2026-11-23 任务页扩展清单落任务 spec：pi-flash-jt44）

用户报障：任务页把扩展勾到只剩 web-access 并保存，每次点开该任务详情 plug
又回【4】（= 设置页全局启用的 4 个包）。根因 = 嵌入 InputPanel 的**扩展这
一路**没有 task_form 分支（执行记录 7 当时口径「ext = 会话清单」）：
plug 按钮 / `open_pi_extension_picker` / `apply_ext_selection` 全走
`self.rt()`，读写的是背后会话；`AgentSpec` 无扩展字段，spec 只存下
`tools=custom`。修：spec 增 `agent.extensions`（null = 沿用全局启用集）+
表单 / 面板 / 派发三处接任务清单（`docs/buglist.md` 同日条目）。

单实例两轮（同 `PI_FLASH_DIR`，`tmp/ui-test-jt44`，中间 `app.quit` 重启）；
逻辑窗口 1707×912（shot 2560×1368，scale 1.5）。**不触发真实任务运行**
（不点【立即运行】，零模型额度）——派发清单由单测
`dispatch_ext_sources_pins_task_list_and_clears_stale_draft` 固化。

```
✓ 基线：背后会话 ext_sources = 4（web-access / pi-usage /
  rpiv-ask-user-question / billion-context）；session 视图
  plugin_menu {available:true, count:4}
✓ tasks.open 新建态：tools=full → plug 置灰 plugin_menu.available=false，
  ext=null ext_count=4（档位读表单——修前读背后会话，恒 true）
✓ session.tools_preset custom（任务页分支）→ tools=custom、available=true、count=4
✓ plugin_picker.open（任务页分支）→ 种子 = 全局启用集（pending_sources 4 条）
✓ plugin_picker.toggle ×3（去 pi-usage / rpiv-ask-user-question /
  billion-context）→ 表单 ext=["npm:pi-web-access"]、ext_count=1、
  plugin_menu.count=1
✓ 背后会话 ext_sources 仍 4 条；隔离目录无 session-ext.json（任务页不写台账）
✓ 名称框 down/up (1300,71) + keys t、pif-ui type 指令 → 保存钮点亮 →
  保存 (1664,70) → defs=1 sel 回填
✓ 隔离库 context.db 直读：spec_json.agent.extensions=["npm:pi-web-access"]
✓ 新建 (845,70) → 表单复位 tools=full / ext=null / count=4
✓ 重选任务行 (500,115) → tools=custom、ext=["npm:pi-web-access"]、count=1   # 报障路径
✓ 存量兼容：直插旧格式 spec（无 extensions 键，停用）→ 选中 ext=null、count=4
✓ app.quit → 同目录重启 → tasks.open 首开选中该任务：ext=["npm:pi-web-access"]、
  count=1、保存钮灰（无假 dirty）；截图 plug【1】
✓ pf-context 61 / pi-link 300 / app（tasks+picker 过滤）27 全绿
```

RESULT: fail=0。

陷阱回填（本轮新增）：

- `ui.hit_probe` 的 `chain` 是**全量** hitbox 列表，不是该点命中链——按
  `hovered == true` 过滤后取面积最小者，才是该点上的元素；
- `wait --eq 2` 等数字值实测不成立（45s 超时，末帧快照里已是
  `"defs":2`）——数字断言用 `snapshot + grep`，`wait` 只留给字符串/布尔；
- `input.keys --arg keys=1` 这类纯数字键名被 `--arg` 按 JSON 数字解析，
  到不了 handler（同上一条「单字符」陷阱，需 `--params-file`）；
- `shot` 是物理像素：目测坐标 × (逻辑宽 1706.67 / 图宽) 才是逻辑坐标——
  精确定位仍以 hit_probe 的 bounds 为准。
