# 047-内存削减：P2 三项验收自动化

> bead pi-flash-uo0f ｜ 2026-11-15 ｜ 模块：047-P2（client.rs spawn / editor tabs / terminal host）

## 背景

047-P2 三票（`lt36` node 堆上限 / `riev` 编辑器 tab soft cap 64 / `2aby` 终端
tab cap 16）的验收项原本写的是 pif-ui 人工跑。用户裁定人工验收不合适，全
部转成本目录的自动化台架（`run.sh`，事务式断言范式同 030）。

为此补了两个**只读断言载体**（生产代码的最小增量）：

- `client.rs` 的 `=== spawn` RPC 日志行新增 `heap=<mb>` 字段——spawn 组装
  对自动化不可见，日志行是唯一进程外观测点（047-P2-1）；
- `app` 快照面新增 `tabs` 键（`panel_tabs` 全序对象数组，kind/path/id）——
  编辑器逐出的「最旧被逐 / 脏保留」需要 tab 带序，`file_cache` HashMap 无
  序不可用（047-P2-2）。

toast 断言读既有 `app.status_toast` 键（2.5s 自动清窗，exec 同步返回后立
即 snapshot 即可命中）。

## 用例

### T1 node-heap（047-P2-1）

mock vendor（复用 030 台架 `mock-pi-stream.mjs`，exact 场景）：确定性回合
收场 + 零真实模型调用。注意 **lazy draft**：boot 不 spawn，spawn 触发 =
首条消息。

- env=256：首条消息拉起 spawn #1；**强杀 node 子进程**（按命令行里的
  `--max-old-space-size` 定位 Windows PID，`$!` 是 MSYS 包装进程不可靠）→
  宿主 `ensure_spawned` 兜底重启 = 真重启路径，spawn #2；再以第二项目发送
  保底 spawn（同 spawn 组装函数）。断言：全部 `=== spawn` 行 `heap=256`，
  无 3072 混入，`panel.dock` 仍可交互（env=256 不崩的自动化等价断言）；
- 缺省对照：全部 `heap=3072`；
- env=0：`heap=0`（显式禁用），无其他值。

### T2 editor-cap（047-P2-2）

67 文件 fixture（f00..f66），`file.open` ×64 开满 → 逐出步骤断言：

- 第 65 个：逐出最旧非脏 **f01**，冲突态 f00 保留，新 tab f64 在带，
  toast `已关闭最早打开的标签页「f01.rs」以释放内存`；
- 第 66 个：逐 f02，f00 继续豁免，toast 换 f02；
- 全脏放行：f03..f65 全部 stick 成冲突态脏 → 开 f66 无逐出对象，soft cap
  不硬拒，file tab count=65。

**置脏口径（关键坑）**：023 自动保存恒开（2026-10-10 用户拍板），纯
`set_text` 1s 静默后就被写盘洗干净。台架的「持续脏」= `dirty_stick`：复
激活（编辑器实体懒创建，仅可见 tab 有）→ `set_text` → 紧跟外部落盘
（<<1s）→ watcher 泵 150ms/轮标 `Changed` 冲突——冲突态 autosave 沉默、
`file_tab_dirty` 恒真。这同时覆盖了逐出跳过口径的 conflict 分支。

### T3 term-cap（047-P2-3）

`terminal.open` ×17：前 16 次返回 count 1..16 递增，第 17 个被拒（count
仍 16）+ toast 含 16（三语文案都带 16，语言无关）；`app.terminals===16`、
`term` 面 terminals 数组长 16。

## 运行

```bash
bash test/047-内存削减/run.sh          # 全量（T1+T2+T3）
bash test/047-内存削减/run.sh T1 T2   # 指定用例
```

前置：`cargo build`（debug 二进制）。隔离实例 + `PI_FLASH_RPC_LOG` 全程
走 pif-ui 数据面，不碰像素不抢屏幕。

## 已知口径

- **真 OOM 崩溃→重启**（pi 实际吃满堆限）不确定性太高，不入自动化；T1 的
  强杀重启路径与 spawn 组装同函数，旗标再现已可断言，OOM 场景留真机观察；
- T1 用 mock vendor 后，spawn 行的 `cli=` 指向 mock 树——旗标断言只看
  `heap=` 字段，不受影响；
- T3 建 16 个真实 ConPTY（cmd.exe 子进程），app 退出时随 ConPTY 句柄收尾；
- 结构断言合并进单次 `jcheck`（一次 snapshot 多断言），一是省往返，二是
  toast 2.5s 窗内的时序余量。
