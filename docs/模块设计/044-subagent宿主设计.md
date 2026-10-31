# subagent 宿主设计(pi-flash 侧)

> 状态:**设计思考阶段,未实现**。与 [045-subagent管理](045-subagent管理.md) 配对:
> 045 定编排侧(pi-subagents 扩展,站 pi 0.99→1.1 原语),本文定**宿主侧**——
> pi-flash 作为 RPC 宿主对 subagent 的观测、管理、交互面。
> 总原则(承 045):**编排逻辑一行都不进宿主;宿主只消费 pi 标准事件面,
> 零自建协议**。

## 0. 结论速览

宿主侧是**薄管理面**:四件事——观测渲染(嵌套进度卡/子会话跳转/聚合面板)、
Agent 定义管理(Settings-子代理页,见 [044-1](044-1-子代理面板.md) §6)、
孤儿恢复 UI(interrupted/resume)、v2 后台通知消费。
无自建状态机、无自建 token 账本、无自建后端路由。

| 宿主拿到的 | 来源 | 宿主做的 |
|---|---|---|
| 子代理进度流 | `tool_execution_update`(spawn 工具 onUpdate,details 带 sessionId/状态/token) | 渲染嵌套卡 |
| 完整过程 | 子会话 JSONL 文件(路径在 details/entry) | 直读 + 跳转 |
| 多代理聚合视图 | 父会话文件的 descriptor entries(实时落盘) | 面板渲染——见 [044-1-子代理面板](044-1-子代理面板.md) |
| 成本 | nested-call usage **逐层上卷**进父工具结果(0.99 原生) | ctx_tip 现有面板直接显示,零新代码 |
| 孤儿 | 自定义 entry + `agent_settled.aborted`(1.1.0) | interrupted 标记 + resume 入口 |
| 后台完成通知(v2) | `ctx.ui.notify`(fire-and-forget) | 消费并决定 followUp 策略 |

## 1. 与 045 的分工边界

| | 044(本文,宿主) | 045(扩展) |
|---|---|---|
| 编排/子会话构造/护栏 | ❌ 不碰 | ✅ |
| 工具面(`agent` 工具、exposure) | ❌ 不碰 | ✅ |
| profile 内容 schema | ❌ 不碰 | ✅ 定义 |
| profile 文件 CRUD/开关 UI | ✅ | ❌ |
| 观测渲染/跳转/面板 | ✅ | 提供数据(onUpdate/setWidget/entry) |
| 后台通知消费、followUp 触发 | ✅ 决策方 | 只发 notify |
| 后端差异(dsh/CC/Codex) | ❌ 不感知 | ✅ provider 层 |

依赖方向:宿主消费扩展的**输出契约**(onUpdate details 字段、entry 格式、
setWidget 载荷),契约变更 = 扩展版本 bump + 宿主适配,与 040/041 扩展管理
同一套纪律。

## 2. 观测数据面(零协议成本)

### 2.1 嵌套进度卡(session/messages/cards)

- spawn 调用是普通工具卡,`toolCallId` 形如 `<parent>/<n>`(0.99 nested-call)。
- 卡片渲染 onUpdate details:状态(运行/收尾/完成/interrupted)、最后活动、
  token 计数、durationMs(1.1.0)、profile 名。
- 完成即工具结果:`{ sessionId, text, usage, durationMs }`(outputSchema 结构化),
  正文折叠、usage 常显;失败卡显示稳定分码(045 工具面节定义,如
  `CAPACITY_REACHED`),不解析自由文本。
- 不做的事:**不渲染子代理内部 step 流**——五定律第 1 条(子 transcript 不进
  父上下文)的 UI 对应:宿主也不把内部过程塞进主窗口。

### 2.2 子会话跳转(session/nav,033)

- 卡片与完成结果卡提供「打开子会话」:PF 按详情里的 JSONL 路径定位
  (sessions 索引扫描已具备,pi-link/sessions.rs),入会话池后 nav 跳转。
- 子会话带 `parentSession` header——详情页展示父链接,支持父子往返导航。
- 跳转是只读检视 + 可 resume(045 持久化语义),**不在子会话里提供 steer
  输入框**(前台语义下父 turn 未结束,子会话无独立输入权;避免造出第二个
  操纵面)。

### 2.3 聚合面板(v2)

- 数据源:扩展 `setWidget`(namespace 组织的并行子代理状态投影)。
- 落点:function_panel dock 或会话页 overlay,复用 psp 列表骨架;不做独立
  顶级页(并行子代理是会话内现象,不升格为全应用概念)。

### 2.4 成本显示(零新代码)

nested-call usage 逐层上卷(0.99 原生记账)→ 父工具结果自带子代理成本 →
ctx_tip 用量面板现有逻辑直接覆盖。**不自建分账**——账在 nestedCalls 里,宿主只显示。

## 3. 管理面

### 3.1 profile 管理

- 数据:`~/.pi/agent/agents/`(全局)与 `.pi/agents/`(项目)两级,`.md`
  frontmatter(schema 归 045)。
- UI:设置面板新增"子代理"页(与 042 模型管理同构):列表 + 编辑器 +
  内置 profile 只读可开关 + 损坏 fail-closed 红标。
- frontmatter 编辑复用 skills.rs 的 SKILL.md 编辑模式(已有先例)。
- 宿主**不校验 schema 语义**(那是扩展的 Config 校验职责),只做文件级
  CRUD 与开关。

### 3.2 扩展开关

pi-subagents 作为普通 pi package,走 040/041 的扩展开关与装载机制,无特殊
通道。开关状态变化 → 下次会话生效(与 pi 扩展语义一致,不做运行中热插)。

## 4. 审批与护栏的宿主角色

- **审批路由**:子代理的工具审批经 `extension_ui` / 权限请求自然回流宿主
  审批流(040 已消化);宿主要做的唯一增量:**卡片标注来源子代理**
  (parentToolCallId → 父卡),满足"子代理的权限问题回到父/UI 会话上问"。
- **取消传播**:父 turn 中止 = 子代理中止(前台语义天然成立);宿主在父卡
  提供取消按钮 → 正常工具取消链,无新协议。
- **护栏可视化**:并发上限 FIFO 排队中、maxTurns steer 提示、abort——全部
  经 onUpdate details 展示,宿主无护栏逻辑。

## 5. 孤儿恢复 UI

- `session_start` 后扩展写入的自定义 entry 已标 interrupted/aborted——宿主
  在父会话转录中把对应卡片渲染为「已中断(取消/崩溃)」两态,提供 resume
  按钮(触发 045 的 resume 路径:复用同一子 session 文件续跑)。
- resume 的用户确认默认开启(花的是新 token,不自动续跑)。

## 6. v2 后台模式的宿主契约

- 完成通知经 `ctx.ui.notify` 到达宿主(fire-and-forget)。
- **followUp 触发策略:默认"用户确认"**,通知卡给「继续此会话」按钮 →
  `followUp()`;不做自动触发(后台子代理完成即打断父会话,违背用户对
  主窗口的掌控预期)。设置项留"自动"档,默认关。
- 通知聚合:同父会话多后台完成合并为一张通知卡。

## 7. 后端不感知原则(dsh/CC/Codex 的挂接点)

- 045 的 `agent` 工具/profile 若未来扩展 provider(另一 harness 经 ACP、
  Claude Code、Codex、vendored dsh),宿主观测面**零改动**:onUpdate/
  setWidget/usage 上卷/entry 契约不变,卡片至多多个 provider 标签。
- vendored dsh 作为扩展侧 provider
候选:ACP 协议层若做,住 pi-link/acp.rs(协议测试在 pi-link),宿主仍
不感知。**宿主永远只见 pi 标准事件面。**

## 8. 分期

| 期 | 内容 | 宿主工作量 |
|---|---|---|
| v1 | 嵌套进度卡、usage 显示、跳转、profile 管理页、扩展开关、孤儿 UI | 小(全在现有骨架内) |
| v2 | 后台通知消费 + followUp 确认卡、setWidget 聚合面板 | 中 |
| 远期 | 多后端 provider 标签、团队级视图(若扩展层提供 roster/DAG 投影);控制工具复用父会话已有 schema 定义,不新增子级专属工具面(schema 不膨胀) | 按需 |

## 9. 待研究(接 045 待研究清单)

1. onUpdate details 字段集冻结(与 045 扩展实现同步确定,写进两文档契约表)。
2. setWidget 载荷格式(RPC 下经 extension_ui 子协议的哪条通道,需与 040
   实现对一遍)。
3. 子会话跳转后「返回父会话」的导航栈行为(033 nav 是否需要父子栈)。
4. v2 自动 followUp 若未来开放,防打扰策略(批量合并、勿扰时段)。
