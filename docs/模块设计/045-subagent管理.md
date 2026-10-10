# subagent 管理（扩展设计）

> 状态：**设计思考阶段，未实现**。本文沉淀 pi-web / ZCode 对比研究与 pi 0.99→1.1 版本变迁的结论，给出优选后的设计方向。
>
> 结论先行：**subagent 必须做成 pi 扩展（pi package），不做宿主内置功能，也不做引擎内置。**
> 原 `045-subagent.md` 占位（"管理还是插件没想好"）由此篇回答并替代：做成扩展；pi-flash 侧只保留薄薄的"管理"面（profile CRUD + 开关 + 运行观测 UI）。

## 背景与决策

- pi-web 的 subagent 实现已拆除（2026-10-09）。不是打磨问题，是地基错位（见下）。
- 定位：**pi package 扩展**——CLI、pi-flash、任何宿主都能加载；宿主只通过 pi 的标准事件面（`tool_execution_update` / `extension_ui` 子协议 / 自定义 entry）获得观测与交互能力。
- pi-flash 的"管理"边界：profile 管理（读写扩展的配置文件）、扩展开关（走 pi config extensions 机制）、会话界面里的运行观测与子会话跳转。**编排逻辑一行都不进宿主。**

## pi 0.99 → 1.1 相关版本事实

subagent 设计必须建立在这些原语之上，先记录事实：

| 版本 | 对 subagent 有意义的变更 |
|---|---|
| **0.99.0** (9/29) | **codemode 诞生**：模型写 JS 跑在 QuickJS 沙箱调工具，只有脚本输出进模型（并行调用、大结果先过滤）。同批落地编排 API：`exposure` 五级暴露（direct / model-only / codemode / deferred / hidden）、`namespace`、`annotations`、`outputSchema`+`structuredContent`、`prepareLoadout()`、**`ctx.executeTool()` 嵌套调用**（事件带 `parentToolCallId`，toolCallId 形如 `<parent>/<n>`，结果记入有界 `nestedCalls`，usage 逐层上卷到调用方工具结果）。另有虚拟模型（按请求路由物理模型）、分类器模型 |
| **0.99.2** | MCP 默认 `codemode` 暴露：服务器不进系统提示、不阻塞首个 prompt，脚本用 `searchTools()` / `describeNamespace()` 发现。codemode 与 MCP 合流 |
| **1.0.0** (10/1) | codemode 减 40% prompt token、支持生图。**关键：SDK 创建的 session 默认不加载 codemode/tool_search/MCP 内建扩展**，宿主必须显式把 `createCodemodeExtension()` 等加进 `extensionFactories` |
| **1.0.4** | `--tools` 支持 `*` 通配；**沙箱内建对象冻结**（防脚本改原型打崩宿主）；codemode 里 `read()` 可返回图片块 |
| **1.1.0** (10/7) | `agent_settled` 事件增加 **`aborted` 字段**（区分取消与正常结束，孤儿判定基础）；工具事件带 `durationMs` |

codemode 能力面（docs/codemode.md）：`tools.<name>(args)` 调任意可调用工具、`Promise.allSettled` 并行、结果过滤后进模型（bash 最大 1MiB）、`store()/load()` 跨调用小状态、`models.classify()/generateImages()`。唯一禁令：**"Scripts cannot start other codemode scripts"**——同一沙箱内的规则（单线程 script worker 会死锁），跨 session 不受限。

## pi-web subagent 与 codemode 互斥的根因

三个结构性冲突，解释了"为什么拆"：

1. **子会话天生没有 codemode**。pi-web 用 `createAgentSessionFromServices` 造子会话，但没把 `createCodemodeExtension()` 加进子会话 loader（SDK 默认不加载内建扩展）。主会话用 pi 王牌能力，子会话停留在裸工具调用。
2. **父会话的 codemode 也摸不到 subagent**。pi-web 把 `Agent` / `get_subagent_result` / `steer_subagent` 全设 `exposure: "model-only"`（永不 callable），自建注释写明原因：脚本发起的 run 会把嵌套 call id 记成 `parentToolCallId`，**丢失它与子会话的自建关联**（它自己维护 toolCallId→子会话 map）。自建关联与 pi 0.99 nested-call 语义打架，只能禁调保平安。
3. **重复造了 pi 已原生化的轮子**。nestedCalls 记录、usage 上卷、exposure 门禁、structuredContent——pi-web 约 2000 行里三分之二（队列、consumed 标记防双投递、通知去重）都在补偿"结果不是工具结果"这个模型。且长在 Next.js 宿主里（`globalThis.__piSessions` 扛热重载），其他宿主用不了。

## 对比研究沉淀（ZCode vs pi-web）

之前对 ZCode（`D:\github\---harness-tools---\ZCode`）与 pi-web 的 subagent 做过全量对比，对新设计有效的结论：

**五条收敛定律**（两套独立演化的设计完全一致的地方，视为物理定律）：
1. 子代理 transcript 不进父上下文；
2. 父只拿回子代理的最终文本；
3. 硬禁孙代（递归深度 1~2 层封顶）；
4. 子代理独立持久化（可检视、可恢复）；
5. profile 化工具白名单（模型/系统提示/工具面按任务类型收敛）。

**值得互借**：
- 向 ZCode 借：事件水位投影（无轮询观察子代理）、后台任务存活判定以单一 registry 为权威、审批类反向请求路由回根会话（子代理的权限问题必须回到父/UI 会话上问）。
- 向 pi-web 借：结构化持久化恢复（子会话状态自描述、重启后 interrupted 可续）、git worktree 文件级隔离、profile 的 frontmatter 格式。

**ZCode 的教训**：一个 child 启动要穿过十余个环节，大量正确性依赖注释级不变式而非类型约束（runner.ts 2100 行、product-projection.ts 5500 行）。**我们的设计目标之一就是不积攒这种复杂度。**

## 优选设计：pi-subagents 扩展

### 设计原则

**不绕开 pi 的原语，站上去。** pi 官方文档已写明编排工具的姿势："codemode uses only this hook (`prepareLoadout`), `exposure`, and `ctx.executeTool()`, so another tool can implement the same behavior under a different name"。subagent 就是这句话的一个实例。

### 工具面：一个工具，两个入口

```
agent(prompt, { profile?, model?, cwd?, background? }) → { sessionId, text, usage, durationMs }
```

- `exposure: "direct"` + `executionMode: "parallel"` + `namespace: "subagents"` + `outputSchema`。direct = 模型可直接声明调用，**脚本也可经 `ctx.executeTool` 调用**。这是与 pi-web 的本质分歧：不设 model-only。
- 一个工具同时点亮两条路：
  - 模型在同一轮并行发多个 `agent` 调用（pi 本来就并行执行同一批工具调用）；
  - codemode 脚本里 `Promise.all([tools.agent(…), tools.agent(…)])` 做**带结果聚合的 fan-out**——分类汇总、多案比对、投票，脚本拿 structuredContent 直接加工，只有结论进模型。
- 生命周期结果即工具结果：await 返回即完成；脚本结束未 settle 的子代理被 pi 原生取消；usage 经 nested-call 逐层上卷进会话成本；`nestedCalls` 自动进 compaction 文件清单。pi-web 的 `get_subagent_result` / consumed 标记 / 通知去重**整体不需要存在**。

### 子会话构造：把 codemode 还给 subagent

```
createAgentSessionFromServices({
  resourceLoader: new DefaultResourceLoader({
    extensionFactories: [createCodemodeExtension(), createToolSearchExtension(), createMcpExtension()],
  }),
  sessionManager: SessionManager.create(childCwd, …, { parentSession: 父session文件 }),
  modelRuntime: 父的 ModelRuntime（共享凭据）,
  model: profile.model ?? 父当前模型,
  tools / excludeTools: 按 profile 白名单裁剪,
})
```

- 子会话是**一等公民 pi session**：独立 JSONL、`parentSession` header、自己的 codemode / tool_search / MCP。
- 首组自定义 entry（`pi.appendEntry`）记录：profile 名、parentToolCallId、深度、resourceSnapshot（模型/系统提示/工具面快照，resume 时还原）。

### 递归与深度

- **构造期保证**：子会话 loader 不装配 pi-subagents 包（孙代无从注册）。
- **运行期兜底**：扩展在 `session_start` 读到自己的 child-marker entry 就不注册工具（防用户全局配置把包穿透进子会话）。
- 深度 1 写死；如未来要 2 层，必须是 profile 显式 opt-in。

### 后台模式分期

- **v1 只做前台**。并行 batch + 脚本 fan-out 已覆盖绝大多数场景，不造后台机器。
- **v2 加 `background: true`**：完成通知走 `ctx.ui.notify`（RPC 下是 fire-and-forget 请求）+ 自定义 entry，**由宿主**（pi-flash）决定何时 `followUp()` 触发父 turn。通知的消费权归宿主——这是 RPC 边界的干净切法。

### 可观测（RPC 宿主视角，pi-flash 零协议成本）

- 子会话进度经 spawn 工具自己的 `onUpdate` 流出（details 带 sessionId、状态、最后活动、token 计数）→ `tool_execution_update` 天然流过 RPC → 会话界面渲染嵌套卡片。
- 看完整过程：pi-flash 直接读子 JSONL 文件（路径在 details / entry 里），点击跳转。
- 聚合面板（多子代理并行状态）：扩展 `setWidget`，宿主渲染。

### 持久化与孤儿恢复

- `session_start` 扫自定义 entry，非终态子会话标记 `interrupted`（用 1.1.0 的 `agent_settled.aborted` 区分取消与崩溃）。
- 按 entry 里持久化的 resourceSnapshot 可 resume（复用同一子 session 文件续跑）。

### profile

保留 pi-web 的好东西，改为 pi package 资源：
- `.md` frontmatter：model / appendSystemPrompt / 工具白名单（含 `ext:` 选择器）/ skills / 上下文继承（截断上限，pi-web 用 50k 字符）/ `isolation: worktree`。
- 全局（`~/.pi/agent/agents/`）与项目（`.pi/agents/`）两级；内置 profile 只读可开关，损坏 fail-closed。
- profile 的使用说明挂进 `namespace: "subagents"` 的 instructions，codemode 脚本 `describeNamespace()` 可读。

### 护栏

- 每父会话并发上限（默认 4~8，FIFO）。
- `maxTurns` 软限：到限先 steer 要求收尾，再超一 turn 才 abort。
- 成本预算可选项：父会话 codemode 里 `models.classify()` 判断"子代理该不该继续"——分类器本身是 0.99 送的新武器。
- 取消传播：工具 execute 拿到的 signal 级联给子会话 abort；父 turn 中止 = 子代理中止（前台语义，天然成立）。

### 与 pi-web 版的增删对比

| 删除 | 保留 | 新增 |
|---|---|---|
| 队列与 consumed 标记 | profile 系统 | 子会话 codemode / tool_search / MCP |
| 通知双投递防护 | resourceSnapshot | 脚本 fan-out（direct 暴露） |
| 自建 toolCallId→子会话关联 map | 孤儿判定与 resume | nested-call 原生记账（usage/嵌套清单） |
| exposure 舞蹈（model-only 禁调） | worktree 隔离 | namespace 组织与 prepareLoadout 适配 |
| Next.js 宿主耦合 | — | RPC 观测契约（onUpdate/setWidget/entry） |

净代码量预计减半，能力面反而更大。

## 待研究（下一步）

1. **成熟 subagent 扩展走访**（每家回答三个问题：spawn 工具暴露等级选了什么？子会话给了哪些内建扩展？后台完成通知如何回流父会话？）：
   - pi.dev/packages 画廊里的 subagent / orchestrator 类 package；
   - pi 官方 examples（`examples/extensions/`、`examples/sdk/14-codemode-mcp.ts`）里与编排相关的写法；
   - claude-code 的 Task/subagent 语义（作为交互范式参照，非代码）；
   - ZCode 的 Agent 工具描述文本（spawn 工具的 prompt 写法范本——"final message is returned as tool result" 这类契约句式）。
2. **待定问题**：
   - 子会话 MCP 是共享父连接（borrowed 快照）还是独立 `createMcpExtension()` 实例？（凭据共享 vs 生命周期隔离）
   - `agent` 工具要不要在 `codemode.mode: "only"` 下自动隐藏声明（prepareLoadout 处理）？
   - v2 后台的宿主契约：pi-flash 侧 followUp 触发策略（自动 vs 用户确认）？
   - worktree 隔离在 Windows 上的代价（pi-flash 双平台铁律）。
