# subagent 管理(扩展设计)

> 状态:**扩展侧已实现(`extensions/pi-subagents`);宿主侧配套见 044-1**。
> 结论先行:**采用 dsh-subagent 框架结构,在该框架下纳入 pi 的核心机制;形态为
> pi package 扩展**(CLI、pi-flash、任何宿主都能加载),不做宿主内置、不做引擎内置。
> **总开关**:PF 侧默认**不加载**本扩展(dev-flags.json /
> `PI_FLASH_SUBAGENTS=1` 开,重启生效;发布包恒关),见 044-1 头注。
> 宿主侧(pi-flash)配套设计见 [044](044-subagent宿主设计.md)。
> pi-web / ZCode / dsh 的研究过程见本地调研笔记（不入库）。

## 背景与决策

- pi-web 的 subagent 实现已拆除;ZCode 版复杂度不可维护。
- dsh-subagent(`packages/subagent`,正式包组)是当前最完整的框架蓝本:服务/provider/
  工具三层分离、统一受管理生命周期、能力声明协商、稳定失败分码。
- **框架骨架采 dsh,机制层站 pi**:dsh 的 activation/通知模型经 `delivery` 二分
  与 pi nested-call 合流(见 §3 映射表),不存在二选一。
- pi-flash 宿主边界不变:profile 管理、扩展开关、运行观测与子会话跳转,
  **编排逻辑一行都不进宿主**。

## pi 0.99 → 1.1 版本事实(机制层的地基)

| 版本 | 对 subagent 有意义的变更 |
|---|---|
| **0.99.0** | **codemode 诞生**:模型写 JS 跑在 QuickJS 沙箱调工具,只有脚本输出进模型。同批落地编排 API:`exposure` 五级暴露、`namespace`、`outputSchema`+`structuredContent`、`prepareLoadout()`、**`ctx.executeTool()` 嵌套调用**(`parentToolCallId` 记账、`nestedCalls` 有界清单、**usage 逐层上卷**)。另有虚拟模型、分类器模型 |
| **0.99.2** | MCP 默认 `codemode` 暴露;codemode 与 MCP 合流 |
| **1.0.0** | codemode 减 40% prompt token。**SDK 创建的 session 默认不加载 codemode/tool_search/MCP 内建扩展**,宿主须显式加 `extensionFactories` |
| **1.0.4** | `--tools` 支持 `*`;沙箱内建对象冻结;`read()` 可返回图片块 |
| **1.1.0** | `agent_settled` 事件增加 **`aborted` 字段**(取消 vs 正常结束,孤儿判定基础);工具事件带 `durationMs` |

## 1. 框架骨架(采 dsh-subagent,七个组件)

1. **服务层(扩展内 SubagentManager)**:统一启动、消息准入、容量、父子所有权、
   结算与释放;"兑现即发布"——启动仅在子级已接受且可取消后返回。
2. **provider 注册表**:具名后端,按名称路由。v1 两个进程内后端:
   `spawn`(全新上下文)/`fork`(带父转录);远期外部后端(ACP:另一 harness、
   Claude Code、Codex)。**能力声明协商**:每个 provider 声明
   `continuable`(可续聊)/`depthLimit`/`modelOptions`/`worktree`,
   请求不满足的能力 → 稳定分码显式拒绝,**绝不静默降级**。
3. **delivery 二分**:`caller`(结果即工具结果,await 返回;v1 全部)⊥
   `parent`(立即返回 childId,完成走通知;v2 后台)。同一框架,两种投递。
4. **容量⊥深度分离**:每父会话在线名额(默认 4~8),**耗尽即拒不排队**
   (`CAPACITY_REACHED`;排队会造成"等待后代的父代理又等待自己占用名额"
   级联死锁);名额与 `maxTurns`、委派深度预算相互独立。深度写死 1
   (构造期:子会话 loader 不装配本扩展;运行期:读到 child-marker entry
   不注册工具)。
5. **中断三层**:本地子级=停当前轮次、保留队列与后代;外部后端=永久停止;
   对已结束子级=空操作容忍。父 turn 中止 = 前台子代理级联中止(signal 级联)。
6. **稳定失败分码**:`CAPACITY_REACHED` / `PROFILE_INVALID` / `DEPTH_LIMIT` /
   `BACKEND_UNSUPPORTED` / `ABORTED`——宿主与 codemode 脚本按码编程,
   不解析自由文本。
7. **子代理目录(catalog)**:父会话自有投影(list_children/聚合状态),
   宿主聚合面板与 `list_agents` 类工具共用同一数据源。

## 2. pi 核心机制映射(dsh 概念 → pi 原语)

| dsh 框架概念 | pi 原语实现 |
|---|---|
| activation 结算与结果 | **nested-call**:`caller` 投递下 await 即工具结果;`parentToolCallId` 记账、`nestedCalls` 有界清单、**usage 逐层上卷**全为 0.99 原生,不自建账本 |
| 结构化输出 | `outputSchema` + `structuredContent` |
| 并行 fan-out | codemode:`Promise.all([tools.agent(…), …])`(direct 暴露,脚本可调);分类汇总类任务只有结论进模型 |
| 驻留子会话 | 子会话是**一等公民 pi session**:独立 JSONL、`parentSession` header、自己的 codemode/tool_search/MCP(`extensionFactories` 显式装配,0.99 补 pi-web 的课) |
| 冷恢复与孤儿 | 自定义 entry(`pi.appendEntry`:profile 名、parentToolCallId、深度、resourceSnapshot)+ `agent_settled.aborted` 区分取消/崩溃 |
| 完成通知(v2 parent) | `ctx.ui.notify`(RPC 下 fire-and-forget)→ 宿主消费(044 §6 契约:默认用户确认才 `followUp()`) |
| persona/工具面收敛 | profile 体系(§6):`.md` frontmatter,语义对齐 dsh preset(persona+工具白名单),形态走 pi package 资源 |
| 上下文继承措辞 | 工具描述按 profile 继承配置动态生成(fresh 类"看不到本对话"/继承类"看不到进行中的轮次") |

## 3. 工具面:一个工具,两个入口

```
agent(prompt, { profile?, model?, cwd?, background? }) → { sessionId, text, usage, durationMs }
```

- `exposure: "direct"` + `executionMode: "parallel"` + `namespace: "subagents"` +
  `outputSchema`。direct = 模型可直接声明调用,**脚本也可经 `ctx.executeTool`
  调用**——不设 model-only。
- 错误结果用稳定分码(§1 第 6 条),不解析自由文本。
- v1 全部 `delivery: caller`;`background: true`(v2)切换为 `parent`。

## 4. 子会话构造

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

- 首组自定义 entry 记录:profile 名、parentToolCallId、深度、resourceSnapshot
  (模型/系统提示/工具面快照,resume 时还原)。

## 5. 后台模式分期

- **v1 只做前台(caller)**。并行 batch + 脚本 fan-out 已覆盖绝大多数场景。
- **v2 加 `background: true`(parent)**:完成通知走 `ctx.ui.notify` + 自定义
  entry,**由宿主决定何时 `followUp()`**(044:默认用户确认)。

## 6. 可观测(RPC 宿主视角,契约见 044)

- 进度经 spawn 工具 `onUpdate` 流出(details 带 sessionId、状态、最后活动、
  token 计数)→ `tool_execution_update` 天然过 RPC → 嵌套卡片。
- 完整过程:宿主直读子 JSONL(路径在 details/entry),点击跳转。
- 聚合面板:扩展 `setWidget`,宿主渲染(数据源 = §1 第 7 条 catalog)。

## 7. 持久化与孤儿恢复

- `session_start` 扫自定义 entry,非终态子会话标 `interrupted`(用
  `agent_settled.aborted` 区分取消与崩溃)。
- 按 resourceSnapshot 可 resume(复用同一子 session 文件续跑)。

## 8. profile = Agent 定义(用户资产)

**概念分层**:profile 不是扩展私有数据,是**用户的
Agent 定义资产**(「类」);委派产生的运行记录是「实例」。pi-subagents 是
manager(机制层),只消费目录约定,不拥有定义——预制与自定义两层由此
解耦(对齐 dsh「bundle 内置 preset + 补丁自定义」的概念,载体用 pi 的
目录发现,不搬 cordis 体系)。

### 8.1 存放与来源(解析顺序:同名 用户文件 > 包 > 内置)

| 来源 | 路径/载体 | 说明 |
|---|---|---|
| 自定义 | `~/.pi/agent/agents/*.md` | 用户完全所有;同名覆盖内置 |
| 内置 | 扩展代码内常量 | 随 pi-subagents 出厂,只读 |

内置清单:**explorer + general-purpose** 两枚(抄 ZCode 两
内置;dsh 无出厂内置)。落点 = profile.ts BUILTIN_PROFILE_DOCS +
pi-link agents.rs builtin_agents,两侧逐字一致(有对账测试);委派工具描述
注册期扫盘列清单(带 description + tools 档位),模型按意图点名。

文件即事实源(PF/CLI/扩展三方共享,零自有存储)。边界:
①**不分项目级**(只分内置/自定义);②**不做包分发**(内部组件);
③**PF 编辑走表单**不让用户直写明文,配置后续单独管理
存储;PF 未识别的新字段**原样透传**。

### 8.2 字段集 v1(frontmatter)

```yaml
description: 代码评审          # 列表展示
model: provider/id            # 路由
tools: inherit                # 工具档位(034 配方复用):inherit(委派者)
                              # / read-only / default / full;无 full+ 档
                              # (full+ 委派者专属,经 inherit 间接获得)
maxTurns: 15                  # 轮次预算:软限(到限 steer 收尾,超限
                              #    abort),覆盖全局默认;缺省 25
systemPrompt: |               # 替换式人格(before_agent_start
  你是代码评审员…               #    systemPrompt override;优先于追加)
appendSystemPrompt: …         # 追加式(与替换并存;替换优先)
injectAgentsMd: true          # 项目 AGENTS.md 是否注入子代理上下文
                              # (默认 true;ZCode 同款开关)
```

(追加的语义对齐 pi 官方 SYSTEM.md/APPEND_SYSTEM.md 两档。)

后续字段(不进 v1):skills、inheritContextLimit、isolation(worktree)、
输出契约。

### 8.3 生命周期

- 修改只影响**之后创建的**子代理(dsh 同语义:"修改只影响之后创建的
  Agent");运行中的实例保持其 resourceSnapshot。
- tools 为**档位枚举**(034 配方复用):spawn 时解析
  inherit→读委派者当前档位配方,转发对应 `--tools`/`-ne` 参数。
- 损坏 profile fail-closed(`PROFILE_INVALID`),不静默降级。
- profile 说明挂进 `namespace: "subagents"` instructions,codemode 脚本
  `describeNamespace()` 可读。

## 9. 护栏

- 在线名额(§1 第 4 条)即并发护栏;`maxTurns` 软限:到限先 steer 要求收尾,
  再超一 turn 才 abort。
- 成本预算可选项:父会话 codemode 里 `models.classify()` 判断"子代理该不该继续"。
- 取消传播:工具 execute 的 signal 级联给子会话 abort;父 turn 中止 = 子代理
  中止(前台语义天然成立);v2 后台按中断三层(§1 第 5 条)。

## 10. 待研究

1. onUpdate details 字段集、setWidget 载荷格式冻结(与 044 契约表对齐,
   对一遍 040 的 extension_ui 通道)。
2. 子会话 MCP 共享父连接(borrowed 快照)还是独立 `createMcpExtension()`?
   (凭据共享 vs 生命周期隔离)
3. `agent` 工具在 `codemode.mode: "only"` 下要不要自动隐藏声明(prepareLoadout)?
4. worktree 隔离在 Windows 上的代价(pi-flash 双平台铁律);若 Windows 不可用,
   按 `BACKEND_UNSUPPORTED` 分码拒绝(能力声明,不静默降级)。
5. 远期外部 provider(ACP)的能力映射:continuable=false 的语义对齐、
   深度预算外包(`provider-managed` 等价物)。
