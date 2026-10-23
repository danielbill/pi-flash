# pi RPC wire 陷阱

改动会碰到的文件（先读完本文再动）：
- `crates/pi-link/**`：协议层本体——`protocol.rs` 类型、`client.rs` 侧车进程、`json.rs` 解析、`transcript.rs` 重放、`catalog.rs` 启动目录、`vendor.rs` 定位
- `crates/app/src/agent_session.rs`、`crates/app/src/session/runtime.rs`、`crates/app/src/session/messages.rs`、`crates/app/src/ext_ui.rs`：app 侧主要消费方

随踩随补。vendor 升级（bump `vendor/pi/package.json` + VERSION）必跑 pi-link 符合性测试，并复核下方标了版本号的条目。

## wire 字段与形态

- 内容块类型 camelCase `toolCall`；流式 args 起始走 `partialJson`。
- 工具结果 `role:"toolResult"` 独立回灌，按 toolCallId 挂回工具卡。
- `set_model` 字段是 `modelId`，且**异步**：立刻 `get_state` 拿旧值，响应 data 里才有切换后的完整模型对象。
- client 不得硬编码 `--no-session`（会压掉 `--session`）。
- `get_messages` 快照携带 bashExecution——与乐观卡并轨会出双卡。
- **`agent_end{willRetry:true}` 不是轮末**（pi-flash-7ep）：可重试错误（429/5xx/网络断，默认 maxRetries=3、退避 2s/4s/8s 上限 60s）后 pi 在**同一轮内**退避并重发 `agent_start`。轮的真正终点是 `agent_end{willRetry:false}` + `agent_settled`；pi 语义 `isStreaming=_isAgentRunActive` 持续到 settled（`isIdle` 明列 retry 不算 idle）。伴随事件 `auto_retry_start{attempt,maxAttempts,delayMs,errorMessage}` / `auto_retry_end{success,attempt,finalError?}`（success=true 不带 finalError）。注意 `FreeUsageLimitError` 类账号限额在 pi 的 NON-RETRYABLE 名单里，不会重试（`@earendil-works/pi-ai` utils/retry.js）。1.3.0 起可重试名单扩大：gateway `Unable to forward upstream stream`、`The service is temporarily unavailable` 也会走重试。
- **排队发送恒走 `prompt` + `streamingBehavior`，禁用裸 `follow_up`/`steer`**（2026-10-17 真机 bug：排队内容几分钟后「重复发送」）。根因：RPC `follow_up` → `session.followUp()` → `_queueUserInput` **不校验 isStreaming**，且入队前还要 `await _runInputHandlers`（扩展 handler 有 latency）——客户端猜「在跑」而 pi 恰好收尾时，消息入到**空闲 agent 的 followUpQueue**，无人消费；下一轮 `prompt` 跑完时 agent-loop 在「would stop」处 drain 队列，陈旧消息被当新轮冲出来（实证：滞留消息的落盘时间 = 下一轮 assistant 终条同一秒）。`prompt(text, {streamingBehavior:"followUp"|"steer"})` 由 pi 以自身 isStreaming **原子**判定：在跑→入队，空闲→立即开跑（失败方向安全）。pi-web parity：rpc-manager.ts 恒带 `streamingBehavior`，从不发裸 `follow_up`。
- **abort 必须配对 `clear_queue`**（2026-10-20 第二轮真机 bug：停止后排队消息又自己执行）。pi 的 `Abort` 只掐当前 run **不清 followUp/steering 队列**（`_handlePostAgentRun` 因 `_agentRunAbortRequested` return false 绝不消费队列）——运行中入队的消息按停止后滞留队列，下一条消息跑完时被 would-stop drain 冲出来当新轮执行。`clear_queue`（`session.clearQueue()`，设计语义即 "restoring to editor when user aborts"）清双队列并回传被撤原文 `{steering:[text..], followUp:[text..]}`；PF 的 abort_stream / 微信远程 `/停止` 都配对发送，response 臂删乐观气泡 + 摘 pending_echoes + 文本回填输入框。prompt 被拒仍**不清 pending_echoes**（pi 侧队列原封不动，滞留回显靠它去重）。
- **`queue_update` 已消费（2026-10-20）**：`{steering:[text..], followUp:[text..]}` 全量快照，入队/投递/清空都发；typed `Event::QueueUpdate`，runtime 持镜像（spawn 时清）供「已排队 N 条」状态行。
- **`thinking_level_changed` 已消费（2026-10-20，真机 bug「思考强度怎么换都是 high」）**：`{level}` 携带的是 pi **截断后的真值**——`setThinkingLevel` 按 `getSupportedThinkingLevels(model)` 收敛（deepseek-flash 只有 `off/low/high/max`，请求 `minimal` 实际落 `low`、`medium` 落 `high`；无 `reasoning` 的模型一律 `off`），且**只有真变了才广播**（请求档恰好落在当前档时一声不响）。`set_thinking_level` 的 response **不带** level，`get_state` 又只在轮末/换模型时才问一次——所以标签与菜单打勾必须同时吃这份事件：runtime 收到即回填 `state.thinking_level` 并把 `thinking_override` 归一到真值（override 还是 respawn 的 `--thinking` 入参，留着没被支持的档会原样写回去），`set_thinking_level` 回执再补一次 `get_state` 兜「无变化」那一支。另一个坑：「使用 pi 默认设置」在 pi 端**没有对应档位**，只清本地 override = 点了没反应，必须把设置里解出的默认档（`defaultThinkingLevel` → per-model → `PI_DEFAULT_THINKING_LEVEL`）显式下发。
- 剩余不消费的杂项仍在 WireEvent 忽略臂（tool_execution_* / entry_appended / session_info_changed）。

## 方法存在性（1.0.0 / 1.1.0 / 1.2.0 / 1.3.0 实测，vendor 升级时复核）

- 无 `navigate_tree` / `systemPrompt` / `get_tools`（`get_tools` 是 pi-web 进程内 SDK 专属）。
- `get_commands` 只回扩展 + skill 命令，**无内置**——升级时复核 `catalog.rs`。
- RPC 模式不支持 custom 扩展 UI；`extension_ui_response` 的 id 必须是请求 id（pi 在 raw-line 层按 id 关联）。
- 1.2.0 起 `getAllTools` 输出多可选键 `namespace` / `reachesTools`（codemode/tool_search 声明用）；pi-link 无 `deny_unknown_fields`，直接容忍。

## 1.3.0 升级复核（2026-10-16，实测 + dist diff）

- **wire 三件套逐字节一致**：`dist/modes/rpc/{rpc-mode,rpc-types,jsonl,rpc-client}.js` 与 1.2.0 完全相同；`agent-session.js` 字符串集（事件名/工具名/键）零增删。protocol.rs 无需动。
- 内置工具名仍 8 个（bash/read/write/edit/find/grep/ls/powershell）；`get_commands` 仍只回扩展+skill。
- 行为变化（不破 wire，但 app 可见）：模型切换丢弃他模型的 thinking 块（旧模型思考不再以纯文本漏给新模型）；resume 保留会话激活的工具（`--tools +name/-name` 在其上叠加）；工具参数含非法 `\u` 转义不再丢字段；尾参 malformed JSON 不再静默半跑而是校验失败回给模型（用户会看到以前「能跑」的工具报错）。
- pi 侧新 settings 键 `sessionShareDestination`（auto/gist/radius，/share 用）——PF 设置页不消费，无需动。
- `kimi-coding` provider 改走 Chat Completions，`models.json` 里它的 baseUrl 覆盖**必须带 `/v1`**（PF 只读透传该文件，用户自己改）。

## 1.4.0 升级复核（2026-10-24，npm tarball dist diff）

- **wire 层零变化**：RPC 三件套 + `jsonl.js` 与 1.3.0 **逐字节一致**；`agent-session.js` 字符串集零增删；`AgentSessionEvent` 联合类型无新事件（pi-rpc 上述口径全部沿用）；内置工具名仍 8 个；`formatDimensionNote` 未变（image-resize.js 逐字节一致，PF 剥除照旧）。protocol.rs / catalog.rs 未动。
- 行为变化（不破 wire，app 可见）：压缩/分支摘要改用模型输出上限，"generation hit the token cap" 失败消除；输出上限截断后恢复的会话不再因孤儿 toolResult 被 provider 拒；resume/分支导航的扩展工具不再失活（`#11062`）；grep 工具改跑 `rg --no-config`（不再吃 `RIPGREP_CONFIG_PATH`）。
- Windows 相关修复利好本仓用户：shell 检测接受 MS Store 别名 + 非 `C:\Windows` 安装下找 PowerShell/WSL（`#9504`/`#9501`）；`mcp.json` 带 UTF-8 BOM 可解析（`#11191`）。
- pi 侧新键（PF 只读透传，无需动）：classifier `images` 输入、provider `compat.imageInput`/`sendImageOptInHeader`、`MiniMax-M3.1-Flash`、`PI_CODEMODE_MAX_WORKERS`；Copilot 默认模型 `gpt-5.4`→`gpt-6.1-sol`。

## transcript 重放

- 系统提示词与工具声明在 transcript 的 `role:"system"` 消息里（content 追加 + sections 补丁 + toolsAdded/Removed），重放对齐 pi-ai `utils/transcript.js`。
- serde_json 开 `preserve_order` 保 sections 顺序。
- turn_start 后的 system message 全量携带 transcript 补丁（app 靠它 live 重放）；1.1.0 起多 `toolsAdded` 键、`agent_settled` 多 `aborted` 字段。

## fork 与深树

- `get_tree` 深树双炸：客户端 serde_json 默认 128 层递归上限（→ `parse_value` disable_recursion_limit + reader 线程 16MB 栈）；pi 侧超长会话自己 `Maximum call stack`。
- fork 锚点正解 = `get_entries` 扁平列表走 parentId，不用 `get_tree`。

## spawn 开关

- `-ne` 只关扩展，个人 skills 照旧（全关要 `-ns`/`-np`/`--no-themes`）；`--tools` 是注册级硬 allowlist（连 getAllTools 都挡）；重复 `-ne` 无害。

## 进程死亡与轮终语义（EOF 三件套，2026-10-19）

- pi 进程死亡不发任何「轮终」事件——只有 stdout EOF。app 侧 pump 尾部（`runtime/mod.rs` 的 `consume_runtime_events` 通道关闭分支）统一收尾：轮中死亡 ⇒ `turn_error=true` + `fail_dangling_tool_calls()`（悬空工具卡就地落红）+ `settle_turn()`（响铃）+ `TurnSettled`（标签栏红点）；空闲死亡只复位不冒泡。此前这里只留一条静默的 "pi exited"——标签栏三色点只认 TurnSettled，用户看到的就是「跑一半悄无声息停了」。
- pi-link 合成 [`Event::ProcessExited`]：exit-waiter 线程 300ms 轮询 `try_wait` reap 退出码（std 无法跨线程 wait，child 住 `Arc<Mutex<Option<Child>>>`，Drop take 走后 waiter 见 None 即退）。stderr 恒 piped + 16KB 尾缓冲（`PiSession::stderr_tail`，`STDERR_TAIL_CAP`）——node OOM / unhandled rejection / 扩展 fatal 只出现在 stderr，死亡时 eprintln 留痕。
- 排查「跑一半停了」：先看 wire log 尾部有无 `=== stdout EOF`——有 = 进程死（本节语义收尾，状态行带退出码）；无 EOF 也无 agent_end = 流挂起（另一类问题）。

## 会话事件解析口径（1.3.0 实测，升级复核点）

`AgentSessionEvent`（`dist/core/agent-session.d.ts`）全量对齐：除轮终三件套（`agent_end`/`agent_settled`/`auto_retry_*`）外，`compaction_start/end`（end 带 `errorMessage`/`willRetry`）、`summarization_retry_scheduled/attempt_start/finished`（与 auto_retry 同构的退避窗口，轮未结束）、`extension_error`（rpc-mode 对扩展错误的输出）、`tool_execution_*`/`queue_update`/`entry_appended`/`session_info_changed`/`thinking_level_changed`（前两者之外归 `Event::WireEvent`，app 暂不消费；`queue_update`/`thinking_level_changed` 已有 typed 臂）均已 typed。真正未知 type 落 `Event::Unparsed`：按 type 去重 eprintln 留痕，不再完全静默。升级 pi 时 grep 新版 agent-session.d.ts 的 `AgentSessionEvent` 核对增量。

## 图片坐标标注（formatDimensionNote，升级复核点）

图片超限缩放展示时，pi 把 `[Image: original WxH, displayed at WxH. Multiply coordinates by N to map to original image.]` **无开关**拼进消息 text 块尾（生成于 `dist/utils/image-resize.js formatDimensionNote`；user 附件拼 `\n\n` + 标注、read 工具图结果拼 `Read image file [mime]\n` + 标注，多图多条连排）。会话 jsonl 存的就是带标注的最终文本，RPC 回显同文。这是给模型的元数据、非消息原文：PF 在 wire 解码出口统一剥除（`protocol.rs content_blocks → strip_image_dimension_note`，尾部连续标注、形状严格校验）。升级 pi 若改了该格式，剥除静默失效（不误剥），以新版 image-resize.js 为准复核。

## 调试

- `PI_FLASH_RPC_LOG` 开线缆日志。
