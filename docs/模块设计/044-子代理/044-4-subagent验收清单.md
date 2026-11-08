# 044-4 subagent 验收清单（测试口径 + CLI 排查手册）

> 对象：`extensions/pf-subagents`。界面形态见 `044-0` 验收①-⑥；扩展机制见 `044-5`。
> **验收主路径就是跑 PF**：`cargo run -p app`，新会话里让模型委派一次，界面上看到 `agent` 工具卡与子代理结果。下面的 CLI 步骤是排查用的下位手段。

## A. PF 内验收（主路径）

集成方式：PF 起 vendored pi 时追加 `-e extensions/pf-subagents/src/index.ts`（`pi-link/src/subagents.rs` 定位、`client.rs` 挂载；`PI_FLASH_SUBAGENTS=0` 关闭，`PI_FLASH_SUBAGENTS_EXT` 覆盖路径）。发版时 `scripts/release.sh` 把扩展一并拷到 `dist/pi-flash/extensions/`，查找顺序与开发布局一致。

```bash
cargo run -p app
```

新会话里发送：

```
用 agent 工具委派一次，prompt 是 'Reply with exactly: PF-SUBAGENT-OK'，
然后把子代理的回复原样告诉我。
```

期望：

- 出现「工作详情 · N 条消息 · 1 次工具调用」，折叠内是 `agent` 工具卡（入参为那条 prompt）
- 最终回答含子代理返回的 `PF-SUBAGENT-OK`
- 子会话文件落在**父会话同目录**（`~/.pi/agent/sessions/<encoded-cwd>/`），header 带 `parentSession`；父会话 jsonl 有 `subagent-completion` custom 消息（默认派发必有）；`blocking: true` 时终态走 agent 工具 result details（child_id/session_file）
- **侧栏不该多出行**：子会话自体落 `subagent/child` custom entry（扩展在子会话 `session_start` 时经 inline extension `appendEntry`），索引层据此排除（`sessions.rs` `IndexEntry::child_marker`）；仍可按路径打开检视
- **pi 原生 fork 分支必须正常上行**：fork 的 header 同样带 `parentSession`，但无 marker——**禁止**用 `parentSession` 判定子会话（误判会把用户 fork 的置顶会话从侧栏全杀）

同轮快照 `messages` 为 user → assistant(tool_calls=1) → assistant("PF-SUBAGENT-OK")；子会话文件与描述符齐备。

**冷恢复排查**：让子代理撞 `maxloop`（如 `maxloop: 2`），完成通知里应出现
`Resume it with agent(prompt, { resumeFrom: "<子会话文件>" })`；再发「用 `agent` 带 `resumeFrom` 续跑刚才那个子代理」，
期望出现第二张卡、子会话文件**同一个**（时间戳与 id 不变、条目继续追加），且父会话里能看到它引用了中断前的结论。

## B. 机制层测试（改扩展后必跑）

```bash
cd extensions/pf-subagents && node --test "test/*.test.ts"
```

期望：全绿（现行 168 条；`npm test` = `node --test`）。**跑之前清环境**：`PI_FLASH_DIR= PI_SUBAGENTS_PROFILES_DIR= node --test test/*.test.ts`——否则 `syncModuleConfig` 会先读 PF 数据目录里的 `ext-context.json`（评审机上是 capacity=3），把 `module-config.test.ts` 的容量用例顶成假失败。

覆盖：容量（拒绝不排队/降低上限不杀已在跑）、结算（清理后才交付结果/关准入/迟到输入被拒）、注册表（能力协商/重复注册）、深度、描述符（版本/对账）、工具面（caller/parent 双投递/分码/结算消息三段式）、冷恢复（`resumeFrom` 透传 / 缺文件 / 非 jsonl / 父会话 / 活会话 → `NOT_RESUMABLE`、`max_turns` 通知带续跑指引）、profile（两级/fail-closed）、本地后端（答案/继续输入/中断/失败/max_loop 软限）。

## C. CLI 级排查（可选）

`PI` 指 `node vendor/pi/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js`。零安装：Node 24 原生跑 TS，SDK 由扩展运行时 lazy import（157 导出）。

### C1 单代理冒烟

**必须带 `--session-dir tmp/smoke` 隔离**：`--no-session` 只隔离父会话，子会话仍按 cwd 落进用户真实 sessions 目录，无法自动识别，会污染侧栏。

```bash
mkdir -p tmp/smoke
$PI -e extensions/pf-subagents/src/index.ts --session-dir tmp/smoke \
  --model deepseek/deepseek-flash --no-session \
  -p "Use the agent tool exactly once with prompt 'Reply with exactly: PONG-7'. Then report it."
```

期望：输出出现 `PONG-7`。

### C2 并行两路

```bash
rm -rf tmp/subagent-smoke
$PI -e extensions/pf-subagents/src/index.ts --model deepseek/deepseek-flash \
  --session-dir tmp/subagent-smoke --mode json -p \
  "Call the agent tool twice in parallel: one child must reply 'AAA', the other 'BBB'. Then report both."
```

期望：两条 `agent` 的 `tool_execution_end`，`isError=false`，`structuredContent.childId` 为 `sub-1`/`sub-2`，`text` 分别 `AAA`/`BBB`，`durationMs` 有值（证明并发）。

### C3 落盘与父子链接

```bash
find tmp/subagent-smoke -name "*.jsonl" | sort
head -c 300 <子会话文件>
```

期望：子会话与父同目录；header 含 `"parentSession":"…"`；跑完后 `~/.pi/agent/sessions/` 不新增本次子会话文件。

### C4 观测数据（pi-flash-9eck：descriptor 落盘已删）

```bash
grep -c '"subagent-completion"' tmp/subagent-smoke/*.jsonl
```

期望：默认派发的父会话有 `subagent-completion` 消息（childId/stopReason 齐全）；`blocking: true` 走工具 result details，无落盘条目。父会话**不再有** `subagent/descriptor` 条目。

### C5 容量耗尽（可选，约 3 次调用）

`PI_SUBAGENTS_CAPACITY=2` + 并行三次 → 第三次返回 `CAPACITY_REACHED`（不排队、不挂起）。默认 3，设置页可配 2-5 且实时生效。

### C6 Profile

`<PF数据目录>/agents/probe.md`：

```markdown
---
description: probe
tools:
  - read
---
You are a probe. Answer in one word.
```

用 `profile: 'probe'` 委派 → 正常回答；把 `tools` 写成非列表 → `PROFILE_INVALID`（fail-closed，不静默降级）。

### C7 深度护栏

子会话由 `createAgentSession` 创建，默认资源发现不含 `-e` 路径 → 孙代没有 `agent` 工具；若强行加载，运行期 `childDepths` 会把 `depth+1` 交 manager 并以 `DEPTH_LIMIT` 拒绝。

## D. 已知边界

- 子会话 loader 带 marker inline extension（不注册工具），与"子会话不装配 pf-subagents"的防孙代约束不冲突；自定义 loader 必须显式传 `agentDir`，否则 pi 的 `resolvePath(undefined)` 崩（`undefined.startsWith`）
- CLI 用 `--no-session` 时父会话无文件 → 子会话拿不到 `parentSession`，会作为普通会话出现（仅影响 CLI 冒烟，PF 恒有会话文件）
- 派发即返回（无参数，默认）：派发结果回裸句 `started subagent <id>`，完成经 `subagent-completion` 自定义消息投递（空闲 `triggerTurn` / 流式 `steer`，见 044-5 §4）；`blocking: true` 才等结果（code-mode 聚合）；实时进度流未做
- 未做：codemode fan-out / usage 上卷展示、worktree、外部后端（ACP/CC/Codex）
