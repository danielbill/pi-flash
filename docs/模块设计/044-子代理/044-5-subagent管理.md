# 044-5 subagent 管理（扩展设计）

> 状态：**现行**，实现于 `extensions/pf-subagents`（pi package 扩展，CLI / pi-flash / 任何宿主都能加载；不做宿主内置、不做引擎内置）。
> 宿主侧配套见 `044-0`（产品/渲染）与 `044-1`（Settings 页）；检索层见 `044-3`。
> **构建门**：开发旗标 `dev-flags.json` `"subagents"` / env `PI_FLASH_SUBAGENTS`，**默认开**（发布包子代理可见），改动重启生效；用户面另有运行时开关（044-1「启用模块」，不挂扩展即四件工具不注册）。

## 1 框架骨架（七个组件）

1. **服务层（扩展内 SubagentManager）**：统一启动、消息准入、容量、父子所有权、结算与释放；"兑现即发布"——启动仅在子级已接受且可取消后返回。
2. **provider 注册表**：具名后端，按名称路由。两个进程内后端：`spawn`（全新上下文）/ `fork`（带父转录）；外部后端（ACP / Claude Code / Codex）未实现。**能力声明协商**：每个 provider 声明 `continuable` / `depthLimit` / `modelOptions` / `worktree`，请求不满足的能力 → 稳定分码显式拒绝，**绝不静默降级**。
3. **派发即返回**（唯一常规形态）：工具只准入子级并返回 childId，答案由结算消息送回，父轮**永远不会**被子代理占住。`caller`（await 即工具结果）只留给需要内联结果的程序化调用方（code-mode 聚合），必须显式 `blocking: true`——这是唯一会让父轮挂起的入口。
4. **容量⊥深度分离**：每父会话在线名额（默认 3，设置页可配 2-10），**耗尽即拒不排队**（`CAPACITY_REACHED`）；名额与 `maxTurns`、委派深度预算相互独立。深度默认 1（PF 恒定 1、禁套娃，设置页不暴露；构造期子会话 loader 不装配本扩展）。容量/深度/开关**事实源 = 投影 `<PF数据目录>/ext-context.json` 的 `subagents` 段**（001-2；`module-config.ts::readSubagentsProjection`，capacity / max_depth / enabled）——扩展每次委派入口重读（`syncModuleConfig`）所以改完即生效、不重启；`enabled=false` 时 `agent` 工具直接拒绝（`MODULE_DISABLED`）。回退链固定：投影（version 匹配）→ legacy `<profiles dir>/config.json`（`capacity` / `maxDepth` 各一组 `{default,min,max}`，`maxTurns` 仍以此文件为准；**模块级 `model` 已废除**——默认模型只有 profile frontmatter `model:` 与调用级 `model` 参数两条路，都没写 = 继承父会话模型；残留 `model` 键按未知键忽略，PF 启动/写容量时清除）→ 扩展内置 FALLBACK(8)；**投影坏档/版本不匹配 = 保持现值不覆盖在跑上限**，problem 上报。PF 设置页改动写 hub `subagents` section（投影为扩展契约源）并**双写一版** legacy `config.json`（`pi-link/agents/module_config.rs`，给未升级扩展留活路）。env 值传导已全退役（`PI_SUBAGENTS_CAPACITY` / `PI_SUBAGENTS_MAX_DEPTH` / `PI_FLASH_APP_SETTINGS` 不再由 spawn 传递；`PI_FLASH_APP_SETTINGS` 扩展侧仅保留 dirname 读取兼容），路径类只留 `PI_SUBAGENTS_PROFILES_DIR` / `PI_FLASH_DIR`（001-2 §5）。
5. **中断**：本地子级 = 停当前轮次、保留队列与后代；已结束（settling/settled）子级 = 空操作容忍；未知 childId = `PROVIDER_UNKNOWN`。父 turn 中止 = 子代理级联中止（signal 级联）。
6. **稳定失败分码**：`CAPACITY_REACHED` / `PROFILE_INVALID` / `DEPTH_LIMIT` / `NOT_RESUMABLE` / `MODULE_DISABLED` / `BACKEND_UNSUPPORTED` / `ABORTED` 等（全集见 `errors.ts` 与 044-0 错误管理）——宿主与 codemode 脚本按码编程，不解析自由文本。
7. **子代理目录（catalog）**：manager 内存记录（childId/provider/label/depth/phase/usage/stats，含已结算历史）；`subagent_list` 读它。宿主面板**不读** catalog——宿主观测从主会话消息推导（见 044-3）。

## 2 pi 核心机制映射

| 框架概念 | pi 原语实现 |
|---|---|
| activation 结算与结果 | 默认 `parent` 投递：派发只回 childId，终态走结算消息；`blocking: true` 时 await 即工具结果（nested-call，`parentToolCallId` 记账、`nestedCalls` 有界清单、usage 逐层上卷为 pi 原生） |
| 结构化输出 | `outputSchema` + `structuredContent` |
| 并行 fan-out | codemode：`Promise.all([tools.agent(…), …])`（direct 暴露，脚本可调）；分类汇总类任务只有结论进模型 |
| 驻留子会话 | 子会话是**一等公民 pi session**：独立 JSONL、`parentSession` header、自己的 codemode/tool_search/MCP（`extensionFactories` 显式装配） |
| 冷恢复与孤儿 | 子会话 `subagent/child` custom entry 记深度（`restoreChildDepth`）；重开会话单趟扫描（`scanSessionBranch`）兼做孤儿对账——无终态记录且不在跑的派发 ack 补一条 `aborted` 终态消息（不唤醒轮次） |
| 完成通知（parent） | `subagent-settled` custom entry 当帧落盘（宿主切卡）+ toast `ctx.ui.notify`（人看）+ `pi.sendMessage(customType: subagent-completion)` 进父会话（模型看）；空闲 `triggerTurn` 唤醒、流式 `steer` 注入当前轮；三段式结算见 §4 |
| persona / 工具面收敛 | profile 体系（§8）：`.md` frontmatter，形态走 pi package 资源 |
| 上下文继承措辞 | 工具描述按 profile 继承配置动态生成（fresh 类"看不到本对话"/继承类"看不到进行中的轮次"） |

## 3 工具面：一个工具，两个入口

```
agent(prompt, { profile?, model?, cwd?, label?, maxloop?, resumeFrom?, blocking? })
  → { childId, status: "started" }        # 默认：立即返回，终态走完成通知
  → { childId, text, stopReason, … }      # 仅 blocking: true

- label: 任务短标签(transcript/面板显示);maxloop: 委派级轮次预算
  (0=不限,省略=100;优先级 maxloop > profile maxTurns
   > config.json maxTurns > 默认 100)。
- resumeFrom: 冷恢复入口（§4.6）——续跑既有子会话，而不是重开一个。
- 工具描述注册期动态注入容量上限 +「CAPACITY_REACHED 等槽位释放重派」引导
  （注册期快照，PF 每会话独立 spawn pi，下次会话见到新值）。
```

- `exposure: "direct"` + `executionMode: "parallel"` + `namespace: "subagents"` + `outputSchema`；direct = 模型可直接声明调用，脚本也可经 `ctx.executeTool` 调用——不设 model-only。
- 错误结果用稳定分码（§1 第 6 条），不解析自由文本。

## 4 派发 / 回传 / 收尾契约

### 4.1 工具描述（`tool.ts` · `delegationDescription()`）

描述由以下成分拼装（注册期生成，静态字符串）：

- **异步契约**：`This tool starts an independently managed subagent and immediately returns its id: it never parks your turn. …`——点名派发不占父轮、完成通知自带最终答案；
- **反轮询指令**：`do not wait for the child and do not poll it (no sleep, no repeated subagent_list/status checks)`；
- **冷恢复指引**：`To continue a child whose run already ended … pass resumeFrom with its session file`；
- **profile 清单**：注册期扫盘逐条列 `name: description (tools: 档位)`，模型按意图点名；
- **容量注入**：`At most <N> subagents run concurrently … if a call is rejected with CAPACITY_REACHED, re-issue it once an earlier delegation settles`；
- **委派政策**（When to delegate）：何时该委派、何时不该。

参数描述：`blocking` = `Only for a programmatic caller that must read the answer inline: parks your turn…`；`label` = `Short (3-5 word) display name for the child.`。

### 4.2 派发结果（`tool.ts` · `runDelegation()` parent 分支）

```
started subagent <id> — it runs in the background. Do NOT poll for it (no `sleep`,
no repeated `subagent_list`): its result arrives as a notification on its own.
```

**反轮询契约**：每一条派发结果都必须点名 `sleep` / `subagent_list` 轮询为非法；工具描述与 `subagent_list` 描述里不得出现 `wait for the results` 类措辞（会被模型读成「去等」）。

`subagent_list` 同款约束：描述声明它是 **diagnostic snapshot, NOT a polling API**，
结果首行固定为 `Snapshot only — do not poll: finished children notify you on their own.`；
`manager.list()` 对已 `settled` 的记录强制归一到终态（`stopReason` → completed/
interrupted/failed），快照里不残留 `active`。

### 4.3 结算消息（`completion.ts` · `composeCompletion()`）

摘要按 `stop_reason` 分档（各档都接正文 `Its closing message:`）：

| stop_reason | 行 |
|---|---|
| `completed` | `Background subagent <childId> finished. Its closing message follows. It will do no further work unless you send it more.` |
| `aborted` | `Background subagent <childId> was stopped before it finished. Its closing message follows.` + 可续时附 `Resume it with agent(prompt, { resumeFrom: "<file>" }) …` |
| `max_turns` | `Background subagent <childId> ran out of turn budget before it finished. Its closing message follows.` + 同上续跑指引 |
| `error` / 其他 | `Background subagent <childId> failed before it finished. (<error_code>) Its closing message follows.` |

正文（不截断）：

```
<摘要行>
Its closing message:
<report>
Structured result: {"childId":…,"stopReason":…,"durationMs":…,"sessionFile":…}
```

- toast 保持中文短句（「子代理完成」/「子代理结束（<code>）」+ 400 字预览），不参与模型契约
- 正文 >128KB 时不截断正文，改为附一行 `The full report is in the child session: <session_file>`

### 4.4 投递（`index.ts` · `deliverParent()`）

终态三通道，依次：

1. `pi.appendEntry("subagent-settled", details)`——custom **entry** 不进消息顺序、当帧广播 `entry_appended`，宿主据此即时把派发卡切到终态（不受 steer 排队影响）；
2. toast（人看）；
3. 消息（模型回执）：

```ts
pi.sendMessage({ customType: COMPLETION_TYPE, content, display: true, details },
  ctx.isIdle() ? { triggerTurn: true } : { deliverAs: "steer" });
```

### 4.5 `subagent_list` 描述（`index.ts`）

```
List this session's child agents with id, phase, label, provider and depth. This is a
diagnostic snapshot, NOT a polling API: you will be notified when a subagent finishes,
so there is no need to check its status after dispatching.
```

### 4.6 冷恢复（`resumeFrom`）

```ts
agent("continue the survey", { resumeFrom: "…/2026-11-08T05-20-11-330Z_01a1b….jsonl" })
```

- **语义**：新 childId + **同一会话文件**。子代理带着完整历史续跑，父会话看到一张新卡、一条新结算；不复用旧 childId（卡片=一次运行，复用会让已结算卡二次结算，破坏 044-3 的「一个状态」不变量）。
- **准入**（先于容量，失败不花槽位）：`assertResumable()`（`backends/local.ts`）要求 `*.jsonl` + 文件存在 + 非目录；`tool.ts` 另拒「父会话自己的文件」（会把父 transcript 写成子会话）与 `manager.liveSessionFiles()` 里的会话（两个写者交错同一文件）。错误码统一 `NOT_RESUMABLE`。
- **构造**：`index.ts` 用 `SessionManager.open(file, parentSessionDir)`，cwd 取会话头的 `getCwd()`（历史工具结果都相对于它）；**不重打 `subagent/child` 标记**（文件里已有，重复 append 会污染侧栏过滤与深度恢复）。
- **发现入口**：完成通知（`completion.ts`）在 `max_turns`/`aborted` 且带 `session_file` 时附「Resume it with `agent(prompt, { resumeFrom: "<file>" })`」，卡片 details 里也一直有 `session_file`；对已结算子代理 `subagent_send` 的报错同样内嵌 `continue it with agent({ resumeFrom: … })` 指引（`ADMISSION_CLOSED`）。

### 4.7 外部委派面排除：billion-context 的 `acp_delegate*`

billion-context 扩展注册 `acp_delegate` / `acp_delegate_wait` / `acp_delegate_cancel`，与 `agent` 语义相反（`runId` vs `childId`），混用会导致模型轮询。排除手段：

- 它自带的 `delegate.forceEnable=false` 检测只认 npm 安装的 `pi-subagents`，对 PF 的 `-e` 仓库布局检测不到——**不可依赖**
- 其配置文件的 `pi.subagents.enabled: false` 关闭项会被该扩展的 web UI 整份重写丢失——**不可依赖**
- **唯一可靠手段**：`spawn.rs` 在所有档位发 `--exclude-tools acp_delegate,acp_delegate_wait,acp_delegate_cancel`（denylist 对扩展工具同样生效）

## 5 子会话构造

```
createAgentSession({
  resourceLoader: new DefaultResourceLoader({
    extensionFactories: [createCodemodeExtension(), createToolSearchExtension(), createMcpExtension()],
  }),
  sessionManager: SessionManager.create(childCwd, 父sessionDir, { parentSession: 父session文件 }),
  model: profile.model ?? 父当前模型,
  tools: profile 白名单（inherit = 不发,继承委派者装配;chat-only = noTools 全关）,
})
```

子会话首组自定义 entry = `subagent/child` 标记（provider + depth；**宿主侧栏过滤只认这条 marker，不用 `parentSession`**——pi 原生 fork 同样带 `parentSession`，须保留侧栏行）。委派元数据（parentToolCallId 等）只走内存 descriptor 与派发 result details，不落子会话转录。

## 6 可观测（RPC 宿主视角）

- onUpdate 分片两类：派发受理后 `running`（details 带 child_id / label / profile / session_file）与被拒 `failed`（status=failed + error_code）→ `tool_execution_update` 过 RPC → 卡片即时上屏（限 agent 工具）；逐轮进度不上报
- 完整过程：宿主直读子 JSONL（路径在 details/entry），点击跳转
- 聚合面板：**主会话自足**（caller=工具 result details、background=父会话 `subagent-settled` 条目 + `subagent-completion` custom 消息回灌，宿主零读盘推导，见 044-3）；被拒委派经 fail onUpdate 即时上卡
- watcher 守卫：每轮 stats/usage 上报的守卫须 **settled/failed 才拦**——caller 投递下 `wait()` 即置 settling，`state!=="active"` 会吞掉整个运行期的逐轮上报

## 7 持久化与孤儿恢复

- 子会话落 `subagent/child` 标记 entry（provider + 深度）；父会话只落终态两件（`subagent-settled` 条目 + `subagent-completion` 消息），descriptor 不落盘
- 孤儿对账：宿主重启/侧车重启后，子代理的 `subagent-settled` 条目永远写不出来——重开会话时 `scanSessionBranch` 单趟扫描找出「有派发 ack、无终态记录、manager 不认识」的孤儿，补一条 `aborted` 终态消息（`orphanCompletionMessage`，带 `resumeFrom` 指引；不带 `triggerTurn`，重开不烧轮次）
- 卡片上的「继续」按钮未做（当前由模型/用户经 `agent(resumeFrom)` 触发，路径来自卡片/结算的 `session_file`）

## 8 profile = Agent 定义（用户资产）

**概念分层**：profile 是**用户的 Agent 定义资产**（「类」）；委派产生的运行记录是「实例」。pf-subagents 是 manager（机制层），只消费目录约定，不拥有定义。

### 8.1 存放与解析顺序（name 优先，同名不覆盖内置）

| 来源 | 路径/载体 | 说明 |
|---|---|---|
| 自定义（全局） | `<PF数据目录>/agents/*.md`（env `PI_SUBAGENTS_PROFILES_DIR` 注入） | 用户完全所有；名称全局唯一（PF 强制查重） |
| 自定义（项目） | `<cwd>/.pi-flash/agents/*.md` | 与全局并列 |
| 内置 | 扩展代码内常量 | 本地探索者 / 通用子代理 / 摘要机 三枚，只读 |

解析（`loadProfile`）：按 key 先比 `name`（点名句柄）——内置 → 全局 → 项目；id 兑底（旧转录/手写文档）——项目 > 全局 > 内置；都无 → `PROFILE_INVALID`。**不存在「自定义覆盖内置」**（同名时内置先中）。

内置落点 = `profile.ts` `BUILTIN_PROFILE_DOCS` + pi-link `agents.rs` `builtin_agents`，两侧逐字一致（有对账测试）；委派工具描述注册期扫盘列清单（带 description + tools 档位）。

边界：①不做包分发；②PF 编辑走表单；③PF 未识别的新字段原样透传。

### 8.2 字段集 v1（frontmatter）

```yaml
name: 代码评审员              # 点名句柄+显示名(全局唯一);键省略 = 展示回落 id
description: 代码评审          # 列表展示
model: provider/id            # 路由;键省略 = 继承父会话模型
thinking: medium              # 思考强度;键省略 = auto = 不覆盖
tools: inherit                # 工具档位(044-0 六档):inherit / chat-only
                              #   / read-only / default / full / full+
maxTurns: 15                  # 轮次预算:软限(到限 steer 收尾,超限 abort);
                              #   键省略 = config.json maxTurns(再缺省 100);0=不限
systemPrompt: |               # 替换式人格(before_agent_start
  你是代码评审员…               #   systemPrompt override;优先于追加)
appendSystemPrompt: …         # 追加式(与替换并存;替换优先)= 文档正文
injectAgentsMd: true          # 项目 AGENTS.md 是否注入(默认 true)
cwd: pf-data                  # 工作目录;唯一合法显式值 pf-data = PF 数据目录;
                              #   键省略 = 继承委派者 cwd
extensions:                   # full+ 档:挂载的 pi-ext 清单
  - path/to/ext.ts
mcp:                          # full+ 档:允许的 mcp 服务名清单
  - server-name
skills:                       # 注入的 skills 三态:键省略 = 系统开启集;
  - skill-name                #   裸键 = 关闭;有清单 = 自定义
```

追加/替换语义对齐 pi 官方 SYSTEM.md / APPEND_SYSTEM.md 两档。schema 已识别、委派装配未消费：`inheritContextLimit`、`isolation`(worktree)。

### 8.3 生命周期

- 修改只影响**之后创建的**子代理；运行中的实例保持其已解析组合不受影响
- tools 为**档位枚举**（034 配方复用）：`inherit` = 不发白名单（子会话继承委派者装配）；其余档位展开为工具白名单（`TOOL_TIER_WHITELISTS`），`chat-only` = `noTools` 全关
- 损坏 profile fail-closed（`PROFILE_INVALID`），不静默降级
- profile 说明挂进 `namespace: "subagents"` instructions，codemode 脚本 `describeNamespace()` 可读

## 9 护栏

- 在线名额（§1 第 4 条）即并发护栏；`maxTurns` 软限：到限先 steer 要求收尾，无视（收尾轮后仍发 toolCall）才 abort
- 取消传播：工具 execute 的 signal 级联给子会话 abort；父 turn 中止 = 子代理中止（前台语义天然成立）；后台子代理按 §1 第 5 条中断语义

## 10 已知限制

- 外部 provider（ACP / Claude Code / Codex）未实现：能力映射、`continuable=false` 语义、深度预算外包均缺位
- worktree 隔离未实现（profile `isolation` 已识别、装配未消费）
- 子会话 MCP 为独立 `createMcpExtension()` 装配，未做共享父连接（borrowed 快照）选项
- 实时进度流未做（子代理运行中逐轮上卡）
