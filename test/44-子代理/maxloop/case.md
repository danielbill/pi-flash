# 44-子代理 · maxloop / 轮次预算

对应 `044-2-子代理testcase.md` 第 3 项"max_loop 限制测试"。

## C1 预算触发收尾（软限）

- 步骤：`maxloop: 10`，子代理被灌 11+ 轮任务。
- 期望：到预算时子代理收到 wrap-up 引导，正常收尾返回（不硬杀、结果可读）。
- 自动：`local-provider.test.ts::steers a wrap-up at the budget instead of cutting the child off`

## C2 无视收尾则硬停（错误码 `MAX_TURNS`）

- 步骤：子代理忽略 wrap-up 继续干活。
- 期望：被中止，结果带稳定错误码，文本可读。
- 自动：`local-provider.test.ts::aborts with max_turns when the child ignores the wrap-up and keeps working`

## C3 默认预算来自配置（100），无预算不干扰

- 期望：profile 未写 `maxTurns` 时落配置默认值；子代理轮次不超预算时不被引导。
- 自动：`local-provider.test.ts::falls back to the default budget when the profile leaves it unset`
- 自动：`local-provider.test.ts::keeps a child under the default budget unsteered`

## C4 每次调用可覆盖

- 步骤：调用参数 `maxloop` 优先于 profile/配置。
- 自动：`local-provider.test.ts::prefers the per-call maxloop over the profile's maxTurns`

## C5 不限档（`maxTurns: 0`）

- 期望：0 = 永不引导、永不停。
- 自动：`local-provider.test.ts::honours the unlimited tier (maxTurns: 0) by never steering`

## C6 配置非法值 fail-closed

- 期望：`maxTurns` 非负整数校验，负数/小数拒绝。
- 自动：`module-config.test.ts`（`maxTurns must be a non-negative safe integer`）
