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

实测结果（2026-11-03）：

| | 修复前（4c8796ea） | 修复后（2813b991） |
|---|---|---|
| wire | prompt → handled，agent_start=0 | 同左 |
| waiting | **true（永久卡住）** | **false（收场）** |
| 假气泡 "/usage" | 残留 | 摘除 |
| messages.count | 1 | 0（命令不落盘，UI=磁盘真相） |

Git Bash 实踩：`pif-ui exec --arg text=/usage` 会被 MSYS 路径翻译成
`C:/Program Files/Git/usage`——必须 `MSYS2_ARG_CONV_EXCL='*'` 前缀。

## 人工验收（可选补充）

- 装任意 /usage 类扩展（如 `pi install npm:@pi-plugins/usage`）→ 发 /usage：
  等待行立即收场，扩展 notify/widget 照常出现，侧栏伪标签撤下。
- `/usageboom`（handler 抛错）→ extension_error 进状态行，同样不卡等待行。
- 回归：普通消息流式照旧（started 分支未动）；运行中排队照旧（queued 分支未动）。
