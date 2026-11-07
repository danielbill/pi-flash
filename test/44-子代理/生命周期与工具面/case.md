# 44-子代理 · 子代理生命周期与工具面

覆盖 `044-0` 的调用契约与描述符/面板数据源。对应 `044-2-子代理testcase.md`
未单列但属"自定义subagent测试"验收面。

## E1 caller 投递返回子代理答案

- 自动：`tool.test.ts::returns the child's answer for caller delivery`
- 自动：`local-provider.test.ts::returns the child's final answer as the result`

## E2 background 投递立刻返回 childId

- 自动：`tool.test.ts::returns a child id immediately for background delivery`

## E3 失败映射为稳定错误码（不抛裸异常）

- 自动：`tool.test.ts::maps failures onto stable codes`
- 自动：`local-provider.test.ts::reports a start failure with a stable code`
- 自动：`local-provider.test.ts::reports a child failure as an error result instead of throwing from wait`

## E4 描述符阶段机与父子落盘

- 期望：`provisioning → active → 终态` 逐相发布；子会话文件随 handle 建立即回填；
  失败启动也带 session file；父会话 jsonl 有 `subagent/descriptor` 条目。
- 自动：`manager.test.ts::emits a descriptor for every phase change`
- 自动：`manager.test.ts::writes the child session file back as soon as the handle exists`
- 自动：`manager.test.ts::carries the session file on a failed-start descriptor too`
- 自动：`descriptor.test.ts::round-trips through parse`

## E5 中断语义（settled=no-op / live=停当轮留队列 / unknown=稳定错误）

- 自动：`manager.test.ts::interrupt on a settled child is a no-op`
- 自动：`manager.test.ts::interrupt on a live child reports interrupted`
- 自动：`manager.test.ts::interrupt on an unknown child rejects with a stable code`
- 自动：`manager.test.ts::interrupts through the activation`

## E6 会话崩溃/中止后的孤儿子代理对账

- 期望：重启后未结算的子代理按中止/崩溃标记为 `interrupted`/`failed`。
- 自动：`descriptor.test.ts::marks orphaned children interrupted when the session was aborted`
- 自动：`descriptor.test.ts::marks orphaned children failed when the session crashed`

## E7 工具面说明文案（fresh/fork 上下文可见性、并发提示）

- 自动：`tool.test.ts::says the child cannot see the conversation for fresh children`
- 自动：`tool.test.ts::says the child cannot see the in-flight turn for fork children`
- 自动：`tool.test.ts::injects the concurrency cap and the retry guidance`

## E8 shutdown 全清

- 自动：`manager.test.ts::shuts down every live activation`
- 自动：`manager.test.ts::parent delivery completes without blocking start`
