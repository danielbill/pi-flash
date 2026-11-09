# 44-子代理 · 接入控制（并发准入 / 深度 / 结算）

对象：`extensions/pf-subagents`。设计口径见 `docs/模块设计/44-子代理/044-0-子代理设计.md`
与 `extensions/pf-subagents/PARITY.md`（与 dsh-subagent 的语义对照）。
全部用例由 `node --test` 驱动，`node test/44-子代理/run.mjs` 一次验收。

## A1 满即拒，不排队

- 前置：`capacity` 生效值为 2。
- 步骤：同一父会话连续委派第 3 个。
- 期望：第 3 个立即返回 `CAPACITY_REACHED`，不阻塞、不排队。
- 自动：`capacity.test.ts::rejects instead of queueing when the bucket is full`

## A2 拒绝文案给出下一步

- 步骤：配额耗尽后再委派一次，读错误文本。
- 期望：`subagent limit reached (active child limit: N); wait for an existing child to finish or complete this work with the current agents`（与 dsh 一致）。
- 自动：`capacity.test.ts::tells the caller to wait for a child or finish with the current agents`

## A3 配额按会话分桶

- 步骤：两个不同 `parentId` 各占 1 槽，限额 1。
- 期望：互不挤占；同一 `parentId` 才共用。
- 自动：`capacity.test.ts::counts each parent session separately`

## A4 两个 manager 不共享桶

- 说明：本宿主一个 pi 进程只服务一个 PF 会话（`crates/app/src/agent_session.rs:39`），
  不存在跨 manager 的同一棵树；此用例钉住"不做进程级池表"这个决定。
- 自动：`parity.test.ts::keeps buckets per manager, not per process`

## A5 降限不杀在跑的子代理

- 步骤：限额 2、已有 2 个在跑，把限额改为 1，再委派。
- 期望：两个在跑的不受影响；新委派被拒。
- 自动：`parity.test.ts::lowering the limit mid-run keeps live children and refuses the next start`
- 自动：`capacity.test.ts::lowering the limit does not evict running children`

## A6 结算清理完成才归还槽位

- 步骤：子代理给出答案但 cleanup 被挂住，此时再委派。
- 期望：仍占槽 → `CAPACITY_REACHED`；cleanup 完成后同一请求放行。
- 自动：`parity.test.ts::counts a settling child until its cleanup finished`
- 自动：`manager.test.ts::keeps the slot until cleanup finished`

## A7 深度闸先于配额

- 步骤：限额已满 + 请求 depth 2。
- 期望：返回 `DEPTH_LIMIT`（不是 `CAPACITY_REACHED`），且未消耗槽位、未调后端。
- 自动：`parity.test.ts::checks depth before quota`
- 自动：`manager.test.ts::enforces the depth limit before touching resources`

## A8 失败路径必须归还槽位

- 步骤：后端 `prepare` 抛错 / `accept` 抛错 / provider 擅自改 id。
- 期望：返回稳定错误码，槽位已归还（下一次委派可成功）。
- 自动：`manager.test.ts::returns the capacity when the backend refuses to prepare`
- 自动：`manager.test.ts::returns the capacity and disposes the handle when accept fails`
- 自动：`manager.test.ts::rejects a handle whose id the provider did not honour`

## A9 结算后才交付结果、并关闭准入

- 自动：`manager.test.ts::resolves the result only after the handle was disposed`
- 自动：`manager.test.ts::closes admission on settle and rejects late input`

## A10 被拒的委派也要留痕

- 期望：父会话出现一条 `failed` 描述符（面板可见），id 照常消耗。
- 自动：`manager.test.ts::emits a failed descriptor for a capacity-rejected delegation`
- 自动：`tool.test.ts::reports CAPACITY_REACHED with the failed update too`

## A11 子会话深度可恢复

- 步骤：子会话文件里有 `subagent/child` marker（depth=1），重开该会话。
- 期望：`currentDepthOf` = 1（不会冒充顶层会话）。
- 自动：`depth-restore.test.ts::reads depth from the persisted marker`
- 自动：`depth-restore.test.ts::ignores entries that are not the child marker`

## A12 宿主模型守卫（环境用例，非单元）

- 目的：`PARITY.md` P0 的结论是"子会话不装配本扩展 → depth≥2 不可达"。
  若有人给 profile 加了 `extensions:`，该结论失效，必须重开对齐。
- 步骤：扫描 `~/.pi/agent/agents/`、`<PF 数据目录>/agents/`、`.pi/agents/` 的
  frontmatter `extensions:`。
- 期望：无任何 profile 引用 `pf-subagents`；否则 FAIL 并提示重开 `PARITY.md`。
- 自动：`run.mjs` 内置（`host-model`）

## A13 套娃端到端（人工冒烟）

- 对应 `044-2-子代理testcase.md` 第 0 项。自动面由 A7（深度闸先于配额，返回
  `DEPTH_LIMIT`）+ A12（宿主模型守卫保证"子会话不装配本扩展"前提）拼合覆盖，
  但两条都不是真实派发路径，按文档字面验收需人工跑一把。
- 步骤：向子代理派发"委派一个子代理"的任务。
- 期望：子代理侧派发失败（无工具面/深度闸拒绝），父会话收到失败结果；
  **子代理派发成功 = 系统失败**。
- 人工原因：需真实模型驱动子会话执行任务，无法稳定断言错误文本。
- 实测（2026-11-10 真机 pif-ui 链路）：general-purpose 子代理收到"委派一个子代理"任务后，
  用 codemode 实测工具清单 `tools.agent does not exist`（16 个工具无委派面），
  如实报告委派失败，父会话原样转告。**PASS**。

## 人工项

仅 A13（2026-11-10 已真机走查 PASS）。其余接入控制全部可由自动用例验收。
