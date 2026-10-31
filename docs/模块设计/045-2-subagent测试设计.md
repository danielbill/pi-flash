# subagent 测试设计(045-1 配套)

> 状态:**待开工**。配套 [045-1-subagent开发计划](045-1-subagent开发计划.md)。
> 策略:**逐阶段对齐 dsh 的 43 个 spec 文件**(22071 行测试 : 10112 行代码
> ≈ 2.2:1),把它的 spec 映射到我们的 M1/M2/M3,并标注**必须复刻 / 按需 / 不复刻**。

## 0. 测试栈与分层

| 层 | 工具 | 范围 |
|---|---|---|
| 单元/契约 | **vitest**(pi 与 dsh 同栈,`vitest --run`) | manager/activation/registry/descriptor 行为约定,纯内存 |
| 集成(真实 pi session) | vitest + 真实 `createAgentSessionFromServices` | 子会话 JSONL、parentSession header、extensionFactories 装配 |
| 端到端(CLI) | 脚本 + vendored pi CLI | `agent` 工具在真实会话里完成委派(验证外部世界,不验证自述) |
| 宿主联调 | PF 侧(044) | onUpdate details / setWidget 载荷(不在本计划验收范围) |

**规则(承两边实践)**:
- 只 mock 开销高或不确定的边界(LLM adapter);manager/activation/registry 全真实。
- 每个行为约定一条测试,**不测“看起来对”**,测 dsh README 写明的可观察行为。
- 失败路径优先:容量耗尽、能力缺失、取消、崩溃恢复。
- 断言外部世界:子会话文件真实存在且 header 正确,不读子代理自述。

## 1. M1 测试(L1 机制复刻)

### 1.1 复刻 spec 映射(dsh 服务层 34 个 → 我们的取舍)

| dsh spec | 覆盖行为 | 取舍 | 我们的落点 |
|---|---|---|---|
| `service.spec.ts` | 服务入口、provider 注册表、生命周期事件 | ✅ 复刻 | `m1-service.test.ts` |
| `activation-policy.spec.ts` | 准入/深度/能力拒绝 | ✅ 复刻 | `m1-policy.test.ts` |
| `run-settlement.spec.ts` | **结算语义**:等后代/等清理/迟到工作 | ✅ **必测(最高风险)** | `m1-settlement.test.ts` |
| `parked-wait.spec.ts` | 空闲停放子树驻留但不延迟宿主完成 | ✅ 必测 | `m1-parked.test.ts` |
| `child-agent.spec.ts` | 子级组装、委派策略、深度辅助 | ✅ 复刻 | `m1-child.test.ts` |
| `continuation*.spec.ts` ×4 | 可续子级、消息继承、preset 续接 | ⚠️ 部分:v1 只测本地可续 + 队列保留,preset 换成 profile | `m1-continuation.test.ts` |
| `catalog.spec.ts` + `list-children.spec.ts` | 目录投影、O(D) 顺序、外部叶节点 | ✅ 复刻 | `m1-catalog.test.ts` |
| `structured.spec.ts` | 结构化结果暂存闸门、拒绝后重开 | ✅ 复刻 | `m1-structured.test.ts` |
| `parent-result.spec.ts` | 结果捕获与失败分码 | ✅ 复刻 | `m1-result.test.ts` |
| `control.spec.ts` | 控制请求校验、稳定分码 | ✅ 复刻 | `m1-control.test.ts` |
| `timing-projection.spec.ts` | durationMs 等计时投影 | ✅ 复刻 | `m1-timing.test.ts` |
| `working-directory.spec.ts` | cwd 解析、继承、独立于父变化 | ✅ 复刻 | `m1-cwd.test.ts` |
| `assistant-output.spec.ts` | 子级最终输出捕获 | ✅ 复刻 | `m1-output.test.ts` |
| `archive-admission.spec.ts` | 归档期取消运行中子孙 | ⚠️ 按需(宿主生命周期相关) | 延后 |
| `activation-presets.spec.ts` / `continuation-presets.spec.ts` | preset 体系 | ❌ 不复刻(走 profile) | — |
| `external-*.spec.ts` ×2 / `out-of-process.spec.ts` | 外部后端 | ❌ 不复刻(远期) | — |
| `schedule-tools.spec.ts` | 浏览器控制面 | ❌ 不复刻(宿主面归 044) | — |
| `continuation-messages-adapter.spec.ts` | Host 专用 Queue/Steer 适配器 | ⚠️ 部分(pi 侧语义不同) | M3 |

dsh 后端 spec(各 1 个文件):`subagent-spawn-in-process.spec.ts`、`subagent-fork-in-process.spec.ts`、
`multi-subagent.spec.ts`(fork 多子代理) → **全部复刻**,这三个最薄也最直接。

### 1.2 M1 任务级验收清单

| 任务 | 测试要点(必测项加粗) |
|---|---|
| 1.1 类型平移 | tsc 严格模式通过;契约类型与 dsh 语义等价性(逐字段对照注释) |
| 1.2 descriptor | 写读回环;**版本号不兼容时拒绝启动**;字段缺失 fail-closed |
| 1.3 容量槽 | **占满即拒不排队**;调低不杀已有;释放后名额归还;并发抢占无超发 |
| 1.4 SubagentManager | **启动失败保持队列**;**结算等后代+等清理**;**释放防迟到工作(关准入→迟到输入被拒)**;父子所有权唯一性;重复 childId 拒绝 |
| 1.5 provider 注册表 | 缺能力注册即拒 `BACKEND_UNSUPPORTED`;请求不支持能力启动失败同码;同名重复注册拒绝 |
| 1.6 spawn 后端 | 子会话独立 JSONL 落盘;**header 带 parentSession**;上下文不含父转录 |
| 1.7 fork 后端 | 子会话首条含父已完成轮次内容;**不含进行中轮次**;多子代理并行互不干扰 |
| 1.8 委派工具 | caller 投递 await 即结果;outputSchema 结构化;**失败→分码**;并发同轮多调用 |
| 1.9 深度护栏 | **构造期:子会话 loader 无本扩展**;**运行期:child-marker 存在→不注册工具**(双保险各一测) |
| 1.10 动态措辞 | 快照断言两种 profile 的描述文本不同 |

### 1.3 M1 端到端(CLI,外部世界断言)

1. **单代理**:CLI 内让模型委派"统计仓库 .rs 文件数"→ 断言子会话 JSONL 存在、
   header 父子关系正确、父会话只有一条工具结果
2. **并行批**:同轮两个 agent 调用 → 断言两个子会话文件独立、父结果两条
3. **失败分码**：构造容量耗尽(父开 8 个)→ 第 9 个返回 `CAPACITY_REACHED`
4. **深度阻断**:子会话里尝试再委派 → 工具不可见(读会话工具清单断言)

## 2. M2 测试(L2 pi 增强)

dsh 无对应(它不靠 codemode 编排),这部分**新增**:

| 任务 | 测试要点 |
|---|---|
| 2.1 direct + outputSchema | 模型直调通;**脚本 `ctx.executeTool` 调通**;结构化结果字段完整 |
| 2.2 codemode fan-out | 脚本 3 路 `Promise.allSettled`;**只有聚合结论进模型上下文**(断言父下一轮请求体);单路失败不拖垮整批 |
| 2.3 nested-call 上卷 | 父工具结果含 `usage`;**usage 逐层上卷数值正确**;compaction 清单含 nestedCalls |
| 2.4 prepareLoadout | `codemode.mode: "only"` 下声明隐藏;normal 模式下可见 |
| 2.5 子会话装配 | 子会话内 codemode/tool_search/MCP 三项可用(各一测) |

**M2 关键回归**:子会话零装配时(若装配失败)工具仍能跑裸调用——**不允许静默降级**,
应显式报错(对应 `BACKEND_UNSUPPORTED` 精神)。

## 3. M3 测试(parent 投递 + 宿主契约)

| 任务 | 测试要点 |
|---|---|
| 3.1 parent 投递 | 立即返回 childId 不阻塞;**完成通知到达**(notify + entry 双落) |
| 3.2 中断三层 | 本地=停轮次**队列保留**(再 send 恢复);外部=永久停;已结束=空操作不报错 |
| 3.3 catalog + 控制工具 | list 与实际一致;schema 复用(父/子定义同一份**快照比对**) |
| 3.4 孤儿恢复 | 杀进程重启 → 非终态标 **interrupted**;`aborted` 区分取消/崩溃两态;resume 续跑用同一 session 文件 |
| 3.5 setWidget 载荷 | 载荷格式与 044 契约表逐字段比对(联调项) |

## 4. 测试量与门禁

| 阶段 | 预计测试文件 | 预计行数 | 门禁 |
|---|---|---|---|
| M1 | 14 | ~3500 | 全绿 + 覆盖率 ≥85%(机制层);**结算/容量/深度三条必须覆盖失败路径** |
| M2 | 5 | ~800 | 全绿 + fan-out 上下文断言 |
| M3 | 5 | ~1200 | 全绿 + 重启恢复测试必过 |
| 合计 | ~24 | ~5500 | 测试:代码 ≈ 1.6:1(dsh 为 2.2:1) |

**覆盖率策略**:不追 dsh 的按文件 100%,守 85% 且
**关键路径 100%**(容量槽、结算、深度、分码四类)。

## 5. 最高风险与对应测试(先写测试再写实现)

| 风险 | 专项测试 | 做法 |
|---|---|---|
| manager.ts 隐性不变式 | `m1-settlement` + `m1-parked` | **TDD 反序**:先按 dsh README 行为写测试,再实现到绿 |
| 剥 cordis 漏生命周期 | `m1-service` 的 dispose 等价物测试 | 对照清单逐一断言:每个注册点都有对应释放断言 |
| 迟到工作进入已释放 agent | `m1-settlement` 专条 | 构造:释放时并发投递 → 断言被拒 |
| 深度穿透(全局配置装进子会话) | `m1-child` 双保险两条 | 构造期 + 运行期各一测 |
| Windows worktree | 后端能力测试 | 断言 `BACKEND_UNSUPPORTED` 拒绝而非静默忽略 |
| RPC 宿主差异 | 留给 044 联调 | M1–M3 只在 CLI 验收,不假装覆盖 RPC |
