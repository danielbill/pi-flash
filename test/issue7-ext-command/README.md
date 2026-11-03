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

## 修复

- `pi-link protocol.rs`：`prompt_disposition(&Value)` 解析 disposition（带单测）。
- `app session/runtime/events.rs`：prompt response 臂新增 `handled` 分支——
  `!agent_running` 时清 `phase_waiting` + 摘「无回显替身」乐观气泡
  （`retire_stale_echo`，pi-web prompt_done → loadSession 磁盘真相口径的
  即时版）+ 撤伪标签；`agent_running`（handler 自己 sendMessage 开跑）时
  不动，等待行让给真轮。`queued`/`started` 维持原行为（轮事件族自理）。

## 自动化（wire 级回归）

```bash
node test/issue7-ext-command/driver.mjs
```

spawn vendored pi `--mode rpc` + 假 /usage 扩展（纯 UI，不 sendMessage），
发 `prompt "/usage"` 与抛错命令 `/usageboom`，断言：
- `response:prompt(success=true, disposition=handled)` 两连
- 除 `extension_ui_request` / `extension_error` 外**零 agent/消息事件**

修复前 PF 视角：handled 后 phase_waiting 无事件可清 → 卡等待行。

## 人工验收（pif-ui，隔离实例）

```bash
# ① 造一个带 /usage 扩展的隔离项目（或直接 pi install npm:@pi-plugins/usage）
#    把 ext-usage.ts 放进 <项目>/.pi/extensions/
PI_FLASH_DIR=<隔离目录> cargo run -p app
# ② 会话输入 /usage 发送 →
#    期望：等待行立即收场，无「正在等待模型...」常驻；扩展 notify/widget
#    照常出现（PF ext_ui 渲染面）；侧栏伪标签撤下
# ③ 对照：/usageboom → extension_error 进状态行，同样不卡等待行
# ④ 回归：普通消息发送 → 流式回复照旧（disposition=started 分支未动）；
#    运行中排队消息照旧（queued 分支未动）
```
