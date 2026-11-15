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

## 陷阱回填

- `pif-ui exec` 位置参数 `k=v` 不解析，必须 `--arg k=v`；
- `snapshot --only` 输出 pretty JSON，grep 键要带冒号后空格；
- `wait` 断言只有 `--eq/--contains/--truthy`（无 `--null`——null 用 snapshot+grep）；
- **dev_flags::summary 原版只看 dev-flags.json**，env 开的旗标日志显 off
  误导排障——已修为 enabled 同语义（env 覆盖计入）。
