# 与 dsh-subagent 的对齐记录（PARITY）

目标：准入语义**由结构保证**，而不是靠"手工算根"的代码。本文只写现状与偏差，
不写历史。参考实现：`D:\github\deepseek-harness\packages\subagent\subagent\src\{activation,manager,index}.ts`。

## 实测（P0：宿主基数）

| 问题 | 证据 | 结论 |
|---|---|---|
| 一个 PF 会话几个 pi 进程 | `crates/app/src/agent_session.rs:39` `spawn_pi(cwd, &args, envs)` | 1 会话 = 1 进程 |
| 子会话装配哪些扩展 | `extensions/pf-subagents/src/index.ts` 建会话时只传 `options.extensions`（profile 的 `extensions` 字段）作 `additionalExtensionPaths` | 只有 profile 列了才装 |
| 现有 profile 列了吗 | `C:/Users/bi_da/.pi/agent/agents/` 只有 `explorer.md` + `settings.json`，无 `extensions:` frontmatter；`~/.pi/agent/extensions/` 只有 `pi-notify.ts` | **子会话不装 pf-subagents** |

推论：子会话没有 `agent` 工具 → **depth≥2 物理不可达** → dsh 的 per-root 池继承
（`manager.ts:743` `resident.get(inputs.parent.id)?.pool ?? rootPool(inputs.parent)`）
在我们的宿主里**没有可观测差异**。per-parent 桶与 per-root 桶是同一个桶。

## 当前对齐状态

| 语义 | dsh | 本实现 | 状态 |
|---|---|---|---|
| 先占槽后启动 | `activation.ts` `pool.reserve(capacity)` 后才 prepare | `manager.ts` 先 `acquire` 再 `prepare` | 同 |
| 满即拒、不排队 | `throw ... 'ACTIVATION_LIMIT_REACHED'` | `Capacity.acquire` 抛 `CAPACITY_REACHED`，文案沿用 dsh | 同（码名保留，宿主契约） |
| 降限不杀 | `setLimit` 只影响新启动 | `Capacity.setLimit` | 同 |
| 结算后才归还 | `releaseSlot` 在 finishDisposal | `manager.ts` settle → cleanup 完成后 release | 同 |
| 深度先于资源 | `assertDelegationDepth` 前置 | `manager.ts` 深度闸在 prepare 前 | 同 |
| 池的粒度 | per root（树共享） | per 会话 | **偏差，但不可达**（见上表） |
| 深度来源 | agent header，缺失 = 0 | `subagent/child` marker 里的 `depth`，`restoreChildDepth` | 同形 |
| 谱系内存态 | `#resident` / `#rootPools` | 无（无可达的树） | 不适用 |
| 授权边 | `holdOwnership` / `acquireOwnership` 维护 `ownedChildren` | 无 | **不移植**：它服务 continuable-parent 生命周期（父不结算直到子归还），我们没有该形态；跨会话访问也不存在（每会话一个 manager） |
| 错误码 | `ACTIVATION_LIMIT_REACHED` | `CAPACITY_REACHED` | 码名保留，文案对齐 |
| 配置边界 | `maxActiveSubagents` 1..MAX 默认 8；`maxDepth` 0..MAX 默认 1 | `FALLBACK_MODULE_CONFIG` 相同 | 同 |

## 已删除的替代品

上一轮为"树共享"加的机制，P0 之后全部移除：

- `sharedPoolRegistry()` / `Symbol.for("pf-subagents.capacity.pools")` / `resetPoolRegistry()`
- `SubagentRequest.rootId` / `DelegationContext.rootId` / `childRoots` / `currentRootOf`
- marker 里的 `rootId`、`currentChildLineage`

保留 `restoreChildDepth`（dsh 也从 header 读深度）。

## 什么情况下要重开本计划

任一条件成立，"池的粒度"就从不可达变可达，必须重新对齐 dsh 的 resident/rootPools：

1. 某个 profile 的 frontmatter 写了 `extensions: [<pf-subagents 路径>]`；
2. 子会话开始走全局扩展发现（`~/.pi/agent/extensions/` 出现本扩展，或 loader 默认带 `additionalExtensionPaths`）；
3. `maxDepth` 配置 > 1 且实测确有子代理第二次委派成功。

## 验收（test/parity.test.ts）

1. 运行中降限：活着的子代理不动，新启动被拒；
2. 深度闸先于配额；
3. 两个 manager 的桶互相独立（对应"每会话一个 manager"）；
4. settling 中的子代理仍占槽，cleanup 完成后才归还。

其余语义由 `test/manager.test.ts`、`test/capacity.test.ts` 覆盖。
