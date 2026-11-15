# 046 pif-ui 自动化验收记录（2026-11-15）

前置：`cargo build`；实例用全新 tmp 目录 + `PI_FLASH_AUTOMATION=auto`。

## 自动化基建（随 S5 落码）

- op：`tasks.open`（`--arg tab=0|1|2` 直达 tab）/ `tasks.close`
  （`pi_link::automation::method` 常量 + handlers/tasks.rs 分支 + mod.rs
  `"tasks"` 前缀路由）；
- 断言面：`snapshot dialogs` 新增 `tasks` 键（开合/tab/editing/defs/
  running/runs/limit——独立槽 dlg.tasks 不占 dialog 枚举，随弹窗面一起
  上报）。

## 执行记录（bash，两实例）

实例 A：`PI_FLASH_AUTO_TASKS=1`（隔离 tmp 目录）

```
初始 tasks=null: 0
tab0 断言: 0        # dialogs.tasks.tab=0 / defs=0 / limit=20（空列表态）
tab1 断言: 0        # tab=1 / editing=false（新建表单态）
tab2 断言: 0        # tab=2 / runs=0（空历史态）
关闭断言: 0         # tasks.close → "tasks": null
flag-on: source registered OK   # app.log 含 [scheduler] source 注册: tasks
```

实例 B：不设 flag（默认关）

```
flag-off: no registration OK    # app.log 无注册行（共有泵照常，source 不注册）
"tasks": null                   # 弹窗键缺席
```

RESULT: fail=0（三 tab / 表单 / 列表断言 + dev-flag 三处同源两向验证）。

## 陷阱回填

- `pif-ui exec` 位置参数 `k=v` 不解析，必须 `--arg k=v`（或整段 JSON）；
- `snapshot --only tasks` 输出为 pretty JSON，grep 匹配键要带冒号后空格
 （`"tasks": null`）。
