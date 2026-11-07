# 044-5 subagent 管理（扩展设计）

> 状态：**现行**，实现于 `extensions/pf-subagents`（pi package 扩展，CLI / pi-flash / 任何宿主都能加载；不做宿主内置、不做引擎内置）。
> 宿主侧配套见 `044-0`（产品/渲染）与 `044-1`（Settings 页）；检索层见 `044-3`。
> **总开关**：PF 侧默认**不加载**本扩展（`dev-flags.json` / `PI_FLASH_SUBAGENTS=1` 开，重启生效；发布包恒关）。

## 1 框架骨架（七个组件）

1. **服务层（扩展内 SubagentManager）**：统一启动、消息准入、容量、父子所有权、结算与释放；"兑现即发布"——启动仅在子级已接受且可取消后返回。
2. **provider 注册表**：具名后端，按名称路由。两个进程内后端：`spawn`（全新上下文）/ `fork`（带父转录）；外部后端（ACP / Claude Code / Codex）属未做。**能力声明协商**：每个 provider 声明 `continuable` / `depthLimit` / `modelOptions` / `worktree`，请求不满足的能力 → 稳定分码显式拒绝，**绝不静默降级**。
3. **delivery 二分**：`caller`（结果即工具结果，await 返回；`background` 缺省）⊥ `parent`（立即返回 childId，完成走通知；`background: true`）。同一框架，两种投递。
4. **容量⊥深度分离**：每父会话在线名额（默认 3，glm 服务器侧约束；设置页可配 2-5），**耗尽即拒不排队**（`CAPACITY_REACHED`；排队会造成"等待后代的父代理又等待自己占用名额"级联死锁）；名额与 `maxTurns`、委派深度预算相互独立。深度默认 1（可配 1-3；构造期子会话 loader 不装配本扩展；运行期读到 child-marker entry 不注册工具）。容量/深度由 PF 模块设置下发（env 初值 + `PI_FLASH_APP_SETTINGS` 每次委派实时重读）。
5. **中断三层**：本地子级 = 停当前轮次、保留队列与后代；外部后端 = 永久停止；对已结束子级 = 空操作容忍。父 turn 中止 = 前台子代理级联中止（signal 级联）。
6. **稳定失败分码**：`CAPACITY_REACHED` / `PROFILE_INVALID` / `DEPTH_LIMIT` / `BACKEND_UNSUPPORTED` / `ABORTED` 等——宿主与 codemode 脚本按码编程，不解析自由文本。
7. **子代理目录（catalog）**：父会话自有投影（`children` / 聚合状态），宿主面板与 `subagent_list` 共用同一数据源。

## 2 pi 核心机制映射

| 框架概念 | pi 原语实现 |
|---|---|
| activation 结算与结果 | **nested-call**：`caller` 投递下 await 即工具结果；`parentToolCallId` 记账、`nestedCalls` 有界清单、usage 逐层上卷为 pi 原生 |
| 结构化输出 | `outputSchema` + `structuredContent` |
| 并行 fan-out | codemode：`Promise.all([tools.agent(…), …])`（direct 暴露，脚本可调）；分类汇总类任务只有结论进模型 |
| 驻留子会话 | 子会话是**一等公民 pi session**：独立 JSONL、`parentSession` header、自己的 codemode/tool_search/MCP（`extensionFactories` 显式装配） |
| 冷恢复与孤儿 | 自定义 entry（`pi.appendEntry`：profile 名、parentToolCallId、深度、resourceSnapshot）+ `agent_settled.aborted` 区分取消/崩溃 |
| 完成通知（parent） | toast `ctx.ui.notify`（人看）+ `pi.sendMessage(customType: subagent-completion)` 进父会话（模型看）；空闲 `triggerTurn` 唤醒、流式 `steer` 注入当前轮；三段式结算见 §4 |
| persona / 工具面收敛 | profile 体系（§8）：`.md` frontmatter，语义对齐 dsh preset，形态走 pi package 资源 |
| 上下文继承措辞 | 工具描述按 profile 继承配置动态生成（fresh 类"看不到本对话"/继承类"看不到进行中的轮次"） |

## 3 工具面：一个工具，两个入口

```
agent(prompt, { profile?, model?, cwd?, label?, maxloop?, background? })
  → { sessionId, text, usage, durationMs }

- label: 任务短标签(transcript/面板显示);maxloop: 委派级轮次预算
  (0=不限,省略=100;优先级 maxloop > profile maxTurns > 默认 100)。
- 工具描述注册期动态注入容量上限 +「CAPACITY_REACHED 等槽位释放重派」引导。
```

- `exposure: "direct"` + `executionMode: "parallel"` + `namespace: "subagents"` + `outputSchema`；direct = 模型可直接声明调用，脚本也可经 `ctx.executeTool` 调用——不设 model-only。
- 错误结果用稳定分码（§1 第 6 条），不解析自由文本。

## 4 派发 / 回传 / 收尾契约

### 4.1 工具描述（`tool.ts` · `delegationDescription()`）

在 "The child's final answer is returned as this tool's result." 后插入：

```
This tool starts an independently managed subagent; with 'background: true' it returns
its id immediately and the runtime notifies you when it finishes, and the notice
includes its final answer. The child does not report mid-run; use 'subagent_send' to
steer it while running or continue its conversation after it finishes.
```

`background` 参数描述：`Return immediately with a child id; the runtime notifies you when it finishes.`
`label` 参数描述：`Short (3-5 word) display name for the child.`

### 4.2 派发结果（`tool.ts` · `runDelegation()` parent 分支）

```
content: [{ type: "text", text: `started subagent ${activation.childId}` }]
```

裸句，不含任何"结束本轮/不要轮询"指令——异步契约在描述里，不在结果里。

### 4.3 结算消息（`completion.ts` · `composeCompletion()`）

摘要按 `details.stop_reason` / `details.error_code` 分档：

| stop_reason | 行 |
|---|---|
| `completed` | `Background subagent <childId> finished. Its closing message follows.`；可续时追加 `It will do no further work unless you send it more.` |
| `aborted` | `Background subagent <childId> was stopped before it finished.` |
| `max_turns` | `Background subagent <childId> ran out of turn budget before it finished.` |
| `error` / 其他 | `Background subagent <childId> failed before it finished.` + `(<error_code>)` |

正文（不截断）：

```
<摘要行>
Its closing message:
<report>
Structured result: {"childId":…,"stopReason":…,"durationMs":…,"sessionFile":…}
```

- toast 保持中文短句（UI 面向人），不参与模型契约
- 正文 >128KB 时不截断正文，改为附一行 `The full report is in the child session: <session_file>`

### 4.4 投递（`index.ts` · `deliverParent()`）

```ts
pi.sendMessage({ customType: COMPLETION_TYPE, content, display: true, details },
  ctx.isIdle() ? { triggerTurn: true } : { deliverAs: "steer" });
```

只发 toast 时父模型拿不到回执，故双发。

### 4.5 `subagent_list` 描述（`index.ts`）

```
List this session's child agents with id, phase, label, provider and depth. You will be
notified when a background subagent finishes; there is no need to keep checking its status.
```

## 5 子会话构造

```
createAgentSessionFromServices({
  resourceLoader: new DefaultResourceLoader({
    extensionFactories: [createCodemodeExtension(), createToolSearchExtension(), createMcpExtension()],
  }),
  sessionManager: SessionManager.create(childCwd, …, { parentSession: 父session文件 }),
  modelRuntime: 父的 ModelRuntime(共享凭据),
  model: profile.model ?? 父当前模型,
  tools / excludeTools: 按 profile 白名单裁剪,
})
```

首组自定义 entry 记录：profile 名、parentToolCallId、深度、resourceSnapshot（模型/系统提示/工具面快照，resume 时还原）。

## 6 可观测（RPC 宿主视角）

- 进度经 spawn 工具 `onUpdate` 流出（details 带 sessionId、状态、最后活动、token 计数）→ `tool_execution_update` 过 RPC → 嵌套卡片；失败 onUpdate 分片（status=failed + error_code）即时上卡（限 agent 工具）
- 完整过程：宿主直读子 JSONL（路径在 details/entry），点击跳转
- 聚合面板：**descriptor 拉模型**（父会话 JSONL 的 descriptor entry 即事实源，宿主常驻轮询，见 044-3）；被拒委派也落 failed descriptor
- watcher 守卫陷阱：每轮 stats/usage 上报的守卫须 **settled/failed 才拦**——caller 投递下 `wait()` 即置 settling，`state!=="active"` 会吞掉整个运行期的逐轮上报

## 7 持久化与孤儿恢复

- `session_start` 扫自定义 entry，非终态子会话标 `interrupted`（`agent_settled.aborted` 区分取消与崩溃）
- 按 resourceSnapshot 可 resume（复用同一子 session 文件续跑）

## 8 profile = Agent 定义（用户资产）

**概念分层**：profile 是**用户的 Agent 定义资产**（「类」）；委派产生的运行记录是「实例」。pf-subagents 是 manager（机制层），只消费目录约定，不拥有定义。

### 8.1 存放与解析顺序（同名：项目 > 全局 > 内置）

| 来源 | 路径/载体 | 说明 |
|---|---|---|
| 自定义（全局） | `<PF数据目录>/agents/*.md`（env `PI_SUBAGENTS_PROFILES_DIR` 注入） | 用户完全所有；同名覆盖内置 |
| 自定义（项目） | `<cwd>/.pi-flash/agents/*.md` | 项目级覆盖全局 |
| 内置 | 扩展代码内常量 | explorer / general-purpose / summarizer 三枚，只读 |

内置落点 = `profile.ts` `BUILTIN_PROFILE_DOCS` + pi-link `agents.rs` `builtin_agents`，两侧逐字一致（有对账测试）；委派工具描述注册期扫盘列清单（带 description + tools 档位）。

边界：①不做包分发；②PF 编辑走表单；③PF 未识别的新字段原样透传。

### 8.2 字段集 v1（frontmatter）

```yaml
description: 代码评审          # 列表展示
model: provider/id            # 路由
tools: inherit                # 工具档位(044-0 六档):inherit / chat-only
                              #   / read-only / default / full / full+
maxTurns: 15                  # 轮次预算:软限(到限 steer 收尾,超限 abort),
                              #   覆盖全局默认;缺省 100(0=不限)
systemPrompt: |               # 替换式人格(before_agent_start
  你是代码评审员…               #   systemPrompt override;优先于追加)
appendSystemPrompt: …         # 追加式(与替换并存;替换优先)
injectAgentsMd: true          # 项目 AGENTS.md 是否注入(默认 true)
extensions:                   # full+ 档:挂载的 pi-ext 清单
  - path/to/ext.ts
mcp:                          # full+ 档:允许的 mcp 服务名清单
  - server-name
skills:                       # 注入的 skills(缺省 = 系统开启集)
  - skill-name
```

追加/替换语义对齐 pi 官方 SYSTEM.md / APPEND_SYSTEM.md 两档。

### 8.3 生命周期

- 修改只影响**之后创建的**子代理；运行中的实例保持其 resourceSnapshot
- tools 为**档位枚举**（034 配方复用）：spawn 时解析 inherit → 读委派者当前档位配方，转发对应 `--tools` / `-ne` 参数
- 损坏 profile fail-closed（`PROFILE_INVALID`），不静默降级
- profile 说明挂进 `namespace: "subagents"` instructions，codemode 脚本 `describeNamespace()` 可读

## 9 护栏

- 在线名额（§1 第 4 条）即并发护栏；`maxTurns` 软限：到限先 steer 要求收尾，再超一 turn 才 abort
- 成本预算可选项：父会话 codemode 里 `models.classify()` 判断"子代理该不该继续"
- 取消传播：工具 execute 的 signal 级联给子会话 abort；父 turn 中止 = 子代理中止（前台语义天然成立）；`background` 按中断三层（§1 第 5 条）

## 10 未做

- 外部 provider（ACP / Claude Code / Codex）：能力映射、continuable=false 语义、深度预算外包
- worktree 隔离（Windows 代价；不可用则按 `BACKEND_UNSUPPORTED` 拒绝）
- 子会话 MCP 共享父连接（borrowed 快照）vs 独立 `createMcpExtension()`（凭据共享 vs 生命周期隔离）
- `agent` 工具在 `codemode.mode: "only"` 下是否自动隐藏声明（`prepareLoadout`）
- 实时进度流（子代理运行中逐轮上卡）
