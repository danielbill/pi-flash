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

## 陷阱回填

- `pif-ui exec` 位置参数 `k=v` 不解析，必须 `--arg k=v`；
- `snapshot --only` 输出 pretty JSON，grep 键要带冒号后空格；
- `wait` 断言只有 `--eq/--contains/--truthy`（无 `--null`——null 用 snapshot+grep）；
- **dev_flags::summary 原版只看 dev-flags.json**，env 开的旗标日志显 off
  误导排障——已修为 enabled 同语义（env 覆盖计入）；
- `--arg keys=t` 单字符到不了 handler（bad_params 需 `{"keys":…}`）——用
  `--params-file` 传合法 JSON（本记录 type_key helper 即此法）。
