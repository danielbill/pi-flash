# issue7-ext-command：扩展命令吃掉 prompt 的等待行卡死（GitHub #7）

用户报障（#7「装了扩展，使用的时候会话页面卡住了」）：装 /usage 类扩展后，
发送 `/usage` 会话页面永远停在「正在等待模型...」。

## 根因

pi 对**扩展命令 / 输入 handler 吃掉的 prompt 不开 run**（agent-session.js
`prompt()` → `_tryExecuteExtensionCommand` 命中即 return，在 `_startRun`
之前）：wire 上只有命令自己的 UI 事件（`extension_ui_request` 的
notify/setStatus/setWidget 火后即忘、报错时一条 `extension_error`）+
`response:prompt(success=true, disposition="handled")`——
**没有 agent_start / 同文回显 / agent_settled**。

PF 的 `optimistic_send` 已置 `phase_waiting=true`，而该旗标只有轮终事件族
（assistant 消息 / AgentEnd / AgentSettled）会清——扩展命令轮次一个都不来，
等待行永久转圈，观感即「会话页面卡住」。

pi 早已在 prompt 成功 response 的 `data.disposition` 里区分三种下场
（`handled` / `queued` / `started`，pi-web rpc-manager.ts 同源消费），
PF 此前没解析。

## 修复（2813b991）

- `pi-link protocol.rs`：`prompt_disposition(&Value)` 解析 disposition（带单测）。
- `app session/runtime/events.rs`：prompt response 臂新增 `handled` 分支——
  `!agent_running` 时清 `phase_waiting` + 摘「无回显替身」乐观气泡
  （`retire_stale_echo`，pi-web prompt_done → loadSession 磁盘真相口径的
  即时版）+ 撤伪标签；`agent_running`（handler 自己 sendMessage 开跑）时
  不动，等待行让给真轮。`queued`/`started` 维持原行为（轮事件族自理）。

## 自动化一：wire 级回归

```bash
node test/issue7-ext-command/driver.mjs
```

spawn vendored pi `--mode rpc` + 假 /usage 扩展（纯 UI，不 sendMessage），
发 `prompt "/usage"` 与抛错命令 `/usageboom`，断言：
- `response:prompt(success=true, disposition=handled)` 两连
- 除 `extension_ui_request` / `extension_error` 外**零 agent/消息事件**

## 自动化二：真实 UI 前后对照（pif-ui，已跑通 ✓）

`tmp/ui-test-issue7-run.sh`（临时脚本，跑法如下）：

1. `git worktree add tmp/wt-issue7-prefix 2813b991~1` 取修复前树
   （4c8796ea = v0.4.0），独立 `CARGO_TARGET_DIR` 冷构建修复前二进制；
   修复后二进制取 `2813b991` 构建（隔离 target，避免动主检出 target）。
2. 隔离项目 `tmp/ui-test-issue7/proj/.pi/extensions/ext-usage.ts` + 隔离
   `PI_FLASH_DIR`；扩展**不经项目信任**（RPC 无 UI 时项目级扩展默认不信任、
   不加载——实踩），改走 031 自定义档清单 `<PI_FLASH_DIR>/session-ext.json`
   映射 `draft-0/1 → [ext 绝对路径]`，spawn 拼 `-e` 显式挂载。
3. pif-ui：`project.switch` → `session.send text=/usage`（内部懒 spawn）→
   `snapshot session` 断言 + `PI_FLASH_RPC_LOG` 线缆取证。
4. **发送前必须 grep wire 日志确认 ext-usage 真挂上**，否则 /usage 漏成普通
   prompt 真调模型（实踩：烧了用户两条极短消息的配额）。

实测结果（2026-11-03 首修）：

| | 修复前（4c8796ea） | 修复后（2813b991） |
|---|---|---|
| wire | prompt → handled，agent_start=0 | 同左 |
| waiting | **true（永久卡住）** | **false（收场）** |
| 假气泡 "/usage" | 残留 | 摘除 |
| messages.count | 1 | 0（命令不落盘，UI=磁盘真相） |

> 上表 messages.count=0 是首修时的行为，已被下面的「二改」推翻：pi-web 的
> loadSession 在会话未落盘时走 404 分支，**不清消息列表**。

Git Bash 实踩：`pif-ui exec --arg text=/usage` 会被 MSYS 路径翻译成
`C:/Program Files/Git/usage`——必须 `MSYS2_ARG_CONV_EXCL='*'` 前缀。

## 三改（2026-11-03 深夜）：真正根因——flush 回执分支吞掉 handled

用户路径可直接复现（pi_work「这是测试」旧会话 → 进去打 `/usage` → 关弹窗 →
「执行扩展命令中」永久亮）。wire 日志里 **Close → handled 到了**，但 PF 状态
`waiting=true`、`phase_command=true`。加临时探针抓到：

```
[dbg] prompt resp id=cmd-11 success=true disp=Some("handled") flush=Some("cmd-11") ...
```

**这条 prompt 是本地队列 flush 发出的**（`flush_in_flight.id` 匹配），所以先命中
`events.rs` 的 flush 回执分支，handled 语义被当成普通 flush 回执吞掉，
后面的 handled 分支永不执行。修法：flush 分支里先按 disposition 收场命令相位，
再做队首/打断槽逻辑。单测：`handled_on_flush_path_settles_command_phase`。

同时替换阻塞弹窗外壳（旧版无高度限制）：

- 卡片 600×600（小窗口按视口收缩），通用标题栏（首行标题 + 排队数 + ✕ 关闭）
- 上：信息面板 ≈ 50% 高、独立滚动；下：操作面板 ≈ 50% 高、独立滚动（可见滚动条）
- 旧版长标题（/usage 的 «View all» 40+ 行）会比小窗口还高：卡片铺满视口、
  Close 在屏外，点背板又落在卡片上（stop_propagation）→ 弹窗根本关不掉

验证：同一复刻环境（完整复制 pi-flash-dev 配置 + 真实 pi_work cwd + 会话副本
+ zai/glm-5.3-flash）Close 后 `waiting=false`、`msgs` 回 11（已落盘会话按磁盘
真相摘气泡）、`ext_dialog=false`。

## 二改（2026-11-03 晚）：收场对齐 pi-web 磁盘真相 + Stop 取消弹窗


用户实测反馈：弹窗关闭后还挂「执行扩展命令中…」好几秒，退出后整屏清空
「像没执行过」。对照 pi-web（SDK 内嵌）逐行核：

1. **收场只认 pi 的 handled 回执**：rpc-mode 的 `preflightResult("handled")`
   在 `await command.handler()` 之后才发（wire 实测 Close→handled **2ms**），
   语义等于 pi-web wrapper 的 `prompt_done`。原先自造的「无弹窗 + 无 agent
   活动静默 5s」看门狗已删——它与真实在途状态无关，只会在弹窗关闭后多挂
   几秒 / 误杀后台 handler。pi-web 的 `waitForPromptSettlement` 兜底靠的是
   `isPromptRunning`（pi RPC 的 get_state 无此字段），PF 无法照抄，也不该
   用静默计时假装。
2. **收场后按磁盘真相**：pi-web `prompt_done → loadSession(sid)`——会话已
   落盘（200）用盘上消息覆盖；未落盘（纯 UI 命令，404）**不动消息列表**，
   transient 会话行保留。PF 旧版无条件 `retire_stale_echo` + 撤伪标签 →
   新会话 `/usage` 一关就空屏。现在只有 `file` 已绑定（盘上有会话）才摘
   气泡；未落盘保留气泡与伪标签。
3. **Stop 取消在途扩展 UI**（pi-web `extensionUiAbortController.abort()`
   parity）：`session.abort` 前 `Chat::ext_cancel_all` 对队头 / 队列 / 问卷卡
   逐个回 `cancelled`，否则 handler 永久阻塞在 `ctx.ui.select` 上。

自动化：`tmp/verify-ext-usage.sh`（pif-ui 隔离实例，真实 @narumitw/pi-usage）

| 场景 | 断言 | 结果 |
|---|---|---|
| A. 菜单 → Close | waiting=false、ext_dialog=false、**messages.count=1**（气泡保留）、wire disposition=handled | ✓ |
| B. 菜单 → session.abort | ext_dialog=false、wire `cancelled:true`、handler 收场 handled | ✓ |

单测锁：`session::runtime::turn_outcome_tests::command_phase_settle_follows_disk_truth`
（未落盘保留 / 已落盘摘除 + 补绑定）。

> RPC 模式能力缺口（非 bug、抄不过来）：pi-web 的 `/usage` 面板走
> `ctx.ui.custom`（TUI 组件渲染 + 按键注入）；vendored pi 的 rpc-mode
> `custom()` 直接 `return undefined`，@narumitw/pi-usage 的 pi-tui-kit 检测
> `mode==="rpc"` 自动降级为 `ctx.ui.select`（多行 title + options）。
> PF 里看到的面板就是降级 select。

## 人工验收（可选补充）

- 装任意 /usage 类扩展（如 `pi install npm:@pi-plugins/usage`）→ 发 /usage：
  等待行立即收场，扩展 notify/widget 照常出现，气泡保留（新会话未落盘，
  pi-web 404 语义），已落盘会话里气泡随磁盘真相消失。
- `/usageboom`（handler 抛错）→ extension_error 进状态行，同样不卡等待行。
- 菜单开着按 Stop → 弹窗立即消失，命令相位收场。
- 回归：普通消息流式照旧（started 分支未动）；运行中排队照旧（queued 分支未动）。
