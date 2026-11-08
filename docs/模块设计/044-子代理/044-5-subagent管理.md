# 044-5 subagent 管理（扩展设计）

> 状态：**现行**，实现于 `extensions/pf-subagents`（pi package 扩展，CLI / pi-flash / 任何宿主都能加载；不做宿主内置、不做引擎内置）。
> 宿主侧配套见 `044-0`（产品/渲染）与 `044-1`（Settings 页）；检索层见 `044-3`。
> **总开关**：PF 侧默认**不加载**本扩展（`dev-flags.json` / `PI_FLASH_SUBAGENTS=1` 开，重启生效；发布包恒关）。

## 1 框架骨架（七个组件）

1. **服务层（扩展内 SubagentManager）**：统一启动、消息准入、容量、父子所有权、结算与释放；"兑现即发布"——启动仅在子级已接受且可取消后返回。
2. **provider 注册表**：具名后端，按名称路由。两个进程内后端：`spawn`（全新上下文）/ `fork`（带父转录）；外部后端（ACP / Claude Code / Codex）属未做。**能力声明协商**：每个 provider 声明 `continuable` / `depthLimit` / `modelOptions` / `worktree`，请求不满足的能力 → 稳定分码显式拒绝，**绝不静默降级**。
3. **派发即返回**（对齐 dsh `tool-subagent/src/index.ts` 的唯一形态 `delivery: 'parent'`）：工具只准入子级并返回 childId，答案由结算消息送回，父轮**永远不会**被子代理占住。`caller`（await 即工具结果）只留给需要内联结果的程序化调用方（code-mode 聚合），必须显式 `blocking: true`——这是唯一会让父轮挂起的入口。
4. **容量⊥深度分离**：每父会话在线名额（默认 3，glm 服务器侧约束；设置页可配 2-10），**耗尽即拒不排队**（`CAPACITY_REACHED`；排队会造成"等待后代的父代理又等待自己占用名额"级联死锁）；名额与 `maxTurns`、委派深度预算相互独立。深度默认 1（可配 1-3；构造期子会话 loader 不装配本扩展；运行期读到 child-marker entry 不注册工具）。容量/深度/开关**事实源 = 投影 `<PF数据目录>/ext-context.json` 的 `subagents` 段**（001-2；`module-config.ts::readSubagentsProjection`，capacity / max_depth / enabled）——扩展每次委派入口重读（`syncModuleConfig`）所以改完即生效、不重启。回退链固定：投影（version 匹配）→ legacy `<profiles dir>/config.json`（`capacity` / `maxDepth` 各一组 `{default,min,max}`，`maxTurns` / `model` 仍以此文件为准）→ 扩展内置 FALLBACK(8)；**投影坏档/版本不匹配 = 保持现值不覆盖在跑上限**，problem 上报。PF 设置页改动写 hub `subagents` section（S3 起投影为扩展契约源）并**双写一版** legacy `config.json`（`pi-link/agents/module_config.rs`，给未升级扩展留活路）。env 值传导已全退役（`PI_SUBAGENTS_CAPACITY` / `PI_SUBAGENTS_MAX_DEPTH` / `PI_FLASH_APP_SETTINGS` 不再由 spawn 传递），路径类只留 `PI_SUBAGENTS_PROFILES_DIR` / `PI_FLASH_DIR`（001-2 §5）。
5. **中断三层**：本地子级 = 停当前轮次、保留队列与后代；外部后端 = 永久停止；对已结束子级 = 空操作容忍。父 turn 中止 = 前台子代理级联中止（signal 级联）。
6. **稳定失败分码**：`CAPACITY_REACHED` / `PROFILE_INVALID` / `DEPTH_LIMIT` / `BACKEND_UNSUPPORTED` / `ABORTED` 等——宿主与 codemode 脚本按码编程，不解析自由文本。
7. **子代理目录（catalog）**：父会话自有投影（`children` / 聚合状态），宿主面板与 `subagent_list` 共用同一数据源。**内存 only**（pi-flash-9eck：descriptor 逐条落盘已删——曾以 ~15 条/秒灌爆父会话到 209MB；宿主观测改从主会话消息推导，见 044-3）。

## 2 pi 核心机制映射

| 框架概念 | pi 原语实现 |
|---|---|
| activation 结算与结果 | 默认 `parent` 投递：派发只回 childId，终态走结算消息；`blocking: true` 时 await 即工具结果（nested-call，`parentToolCallId` 记账、`nestedCalls` 有界清单、usage 逐层上卷为 pi 原生） |
| 结构化输出 | `outputSchema` + `structuredContent` |
| 并行 fan-out | codemode：`Promise.all([tools.agent(…), …])`（direct 暴露，脚本可调）；分类汇总类任务只有结论进模型 |
| 驻留子会话 | 子会话是**一等公民 pi session**：独立 JSONL、`parentSession` header、自己的 codemode/tool_search/MCP（`extensionFactories` 显式装配） |
| 冷恢复与孤儿 | 子会话 `subagent/child` custom entry 记深度（`restoreChildDepth`）；孤儿恢复 UI 未做（原「重启标 interrupted descriptor」随 descriptor 落盘删除，pi-flash-9eck） |
| 完成通知（parent） | toast `ctx.ui.notify`（人看）+ `pi.sendMessage(customType: subagent-completion)` 进父会话（模型看）；空闲 `triggerTurn` 唤醒、流式 `steer` 注入当前轮；三段式结算见 §4 |
| persona / 工具面收敛 | profile 体系（§8）：`.md` frontmatter，语义对齐 dsh preset，形态走 pi package 资源 |
| 上下文继承措辞 | 工具描述按 profile 继承配置动态生成（fresh 类"看不到本对话"/继承类"看不到进行中的轮次"） |

## 3 工具面：一个工具，两个入口

```
agent(prompt, { profile?, model?, cwd?, label?, maxloop?, resumeFrom?, blocking? })
  → { childId, status: "started" }        # 默认：立即返回，终态走完成通知
  → { childId, text, stopReason, … }      # 仅 blocking: true

- label: 任务短标签(transcript/面板显示);maxloop: 委派级轮次预算
  (0=不限,省略=100;优先级 maxloop > profile maxTurns > 默认 100)。
- resumeFrom: 冷恢复入口（§4.6）——续跑既有子会话，而不是重开一个。
- 工具描述注册期动态注入容量上限 +「CAPACITY_REACHED 等槽位释放重派」引导。
```

- `exposure: "direct"` + `executionMode: "parallel"` + `namespace: "subagents"` + `outputSchema`；direct = 模型可直接声明调用，脚本也可经 `ctx.executeTool` 调用——不设 model-only。
- 错误结果用稳定分码（§1 第 6 条），不解析自由文本。

## 4 派发 / 回传 / 收尾契约

### 4.1 工具描述（`tool.ts` · `delegationDescription()`）

原第一句「The child's final answer is returned as this tool's result.」已删（它与派发即返回矛盾），改为直接声明派发语义：

```
This tool starts an independently managed subagent and immediately returns its id: it
never parks your turn. The runtime notifies you when it finishes, and the notice includes
its final answer. Right after dispatching, keep working or reply to the user — the notice
is steered into your turn if you are still working, or starts a new one when you are idle.
The child does not report mid-run; use 'subagent_send' to steer it while running or
continue its conversation after it finishes.
```

`blocking` 参数描述：`Only for a programmatic caller that must read the answer inline: parks your turn for the child's whole run, so the user cannot reach you until it returns. Leave unset for every interactive delegation.`
`label` 参数描述：`Short (3-5 word) display name for the child.`

### 4.2 派发结果（`tool.ts` · `runDelegation()` parent 分支）

```
started subagent <id> — it runs in the background. Do NOT poll for it (no `sleep`,
no repeated `subagent_list`): its result arrives as a notification on its own.
```

**2026-11-08 真机回归**：裸句 `started subagent <id>` 太软——模型把它读成「在等结果」，
接着 `bash sleep 30` 轮询。反轮询契约因此落到**结果文案**上（不只是描述）：每一条
派发结果都必须点名 `sleep` / `subagent_list` 轮询为非法。同步把描述里
「wait for the results」删掉（`wait` 会被模型读成「去等」）。

`subagent_list` 同样收紧：描述声明它是 **diagnostic snapshot, NOT a polling API**，
结果首行固定为 `Snapshot only — do not poll: finished children notify you on their own.`；
`manager.list()` 对已 `settled` 的记录强制归一到终态（`stopReason` → completed/
interrupted/failed），避免快照里残留 `active` 让模型「看到还在跑」而去轮询。

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

### 4.6 冷恢复（`resumeFrom`，对齐 dsh `coldResume`）

```ts
agent("continue the survey", { resumeFrom: "…/2026-11-08T05-20-11-330Z_01a1b….jsonl" })
```

- **语义**：新 childId + **同一会话文件**。子代理带着完整历史续跑，父会话看到一张新卡、一条新结算；不复用旧 childId（卡片=一次运行，复用会让已结算卡二次结算，破坏 044-3 的「一个状态」不变量）。dsh 是 same-id 续聊，差别在宿主卡片模型，不在机制。
- **准入**（先于容量，失败不花槽位）：`assertResumable()`（`backends/local.ts`）要求 `*.jsonl` + 文件存在 + 非目录；`tool.ts` 另拒「父会话自己的文件」（会把父 transcript 写成子会话）与 `manager.liveSessionFiles()` 里的会话（两个写者交错同一文件）。错误码统一 `NOT_RESUMABLE`。
- **构造**：`index.ts` 用 `SessionManager.open(file, parentSessionDir)`，cwd 取会话头的 `getCwd()`（历史工具结果都相对于它）；**不重打 `subagent/child` 标记**（文件里已有，重复 append 会污染侧栏过滤与深度恢复）。
- **发现入口**：完成通知（`completion.ts`）在 `max_turns`/`aborted` 且带 `session_file` 时附「Resume it with `agent(prompt, { resumeFrom: "<file>" })`」，卡片 details 里也一直有 `session_file`。

### 4.7 外部委派面：billion-context 的 `acp_delegate*`（2026-11-08）

PF 宿主里**曾有第二个委派面**，不是 PF 也不是 pi 内核提供的：billion-context 扩展（`~/.pi/agent/npm/node_modules/billion-context`，`pi.extensions → ./dist/agent/pi-native.js`）注册 `acp_delegate` / `acp_delegate_wait` / `acp_delegate_cancel`，语义与 `agent` 相反（`runId`、默认异步但可阻塞 wait）。模型混用二者 → 拿 `childId` 去 `acp_delegate_wait` 报 `No delegate run with runId`，再读 `subagent_list` 当状态轮询，最后 `bash sleep 30`。

- **它的自动避让不生效**：billion-context 自带 `delegate.forceEnable=false` 检测——只在发现 npm `pi-subagents` 安装（`npmInstallAt(agentDir)` / `extensions/*` 的 `package.json.name === "pi-subagents"`）时关掉。PF 用 `-e` 指仓库 `extensions/pf-subagents`，检测不到。
- **关闭方式**：`C:/Users/bi_da/.config/billion-context/billion-context.json` 加 `"pi": {"subagents": {"enabled": false}}`（`CONFIGURATION.md:868-905`；`enabled:false` 同时移除三个工具与其系统提示词段，**新会话生效**）。已改（备份 `billion-context.json.bak-20261108`）。
- **不依赖它**：即使没关，4.2 的反轮询契约 + `subagent_list` 的 snapshot 语义也让模型不去 poll。
- **为什么还要 spawn 侧钉一次**：那份 JSON 会被 bili 自己的 web UI 整份重写成它的 schema（实测 16:36 把 `pi` 段丢了，只留 `providers`/`compress`），所以 `spawn.rs` 在所有档位额外发 `--exclude-tools acp_delegate,acp_delegate_wait,acp_delegate_cancel`（denylist 对扩展工具同样生效，已用 probe 扩展实测 `getAllTools()` 里三项彻底消失）。

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

子会话首组自定义 entry = `subagent/child` 标记（深度；宿主侧栏过滤与嵌套深度还原的钥匙）。委派元数据（parentToolCallId 等）只走内存 descriptor 与派发 result details，不落子会话转录。

## 6 可观测（RPC 宿主视角）

- 进度经 spawn 工具 `onUpdate` 流出（details 带 sessionId、状态、最后活动、token 计数）→ `tool_execution_update` 过 RPC → 嵌套卡片；失败 onUpdate 分片（status=failed + error_code）即时上卡（限 agent 工具）
- 完整过程：宿主直读子 JSONL（路径在 details/entry），点击跳转
- 聚合面板：**主会话自足**（pi-flash-9eck：caller=工具 result details、background=父会话 `subagent-completion` custom 消息回灌，宿主零读盘推导，见 044-3）；被拒委派经 fail onUpdate 即时上卡
- watcher 守卫陷阱：每轮 stats/usage 上报的守卫须 **settled/failed 才拦**——caller 投递下 `wait()` 即置 settling，`state!=="active"` 会吞掉整个运行期的逐轮上报

## 7 持久化与孤儿恢复

- 子会话落 `subagent/child` 标记 entry（深度）；父会话不落 descriptor（pi-flash-9eck 已删）
- 孤儿恢复：**机制已具备**（§4.6 `resumeFrom` 复用同一子 session 文件续跑，路径来自卡片/结算的 `session_file`）；宿主重启后遗留卡仍按中断态呈现，卡片上的「继续」按钮未做（当前由模型/用户经 `agent(resumeFrom)` 触发）

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
