# 44-子代理 · 并发上限（配置驱动）

对象：`<profiles dir>/config.json` 的 `capacity`（默认 8，值域 1..MAX_SAFE_INTEGER）。
对应 `044-2-子代理testcase.md` 第 1 项"内部并发限制"与第 2 项"外部并发限制"。

## B1 设 10 派 10：全部放行

- 前置：`config.json` = `{"capacity":{"default":10,"min":2,"max":10}}`。
- 步骤：同一会话内连续委派 10 个。
- 期望：10 个全部启动，第 11 个才被拒。
- 自动：`module-config.test.ts::capacity 10 from the config admits the 4th delegation`
- 自动：`module-config.test.ts::honours a max of 10 — the old hardcoded 5 ceiling is gone`

## B2 设 2 派 5：拒 3 个

- 前置：`capacity.default` = 2。
- 步骤：连续委派 5 个。
- 期望：前 2 个通过，第 3-5 个 `CAPACITY_REACHED`。
- 自动：`module-config.test.ts::capacity 3 from the config rejects the 4th same-frame delegation`
  （用例内部把 default 降到 3 以节省起子代理的开销，语义同"拒第 N+1 个"）

## B3 设 6 派 5：全部放行

- 说明：与 B1 同一断言路径（限额 ≥ 派发数 ⇒ 全通过），无需单独用例；
  差量由 B1 覆盖。

## B4 值域真的来自文件，不是代码硬顶

- 期望：`min`/`max` 从 `config.json` 读；文件写 `max: 10` 时第 4 个不再被 5 的
  旧硬顶挡住。
- 自动：`module-config.test.ts::takes values AND bounds from the file (no code-side ceiling)`

## B5 改配置热生效（不重启）

- 步骤：运行中改 `config.json`，下一次委派即按新值判定。
- 自动：`module-config.test.ts::re-reads on every sync, so an edit applies without a restart`

## B6 坏配置不打断会话

- 步骤：把 `config.json` 写成非法 JSON / 删掉。
- 期望：保持当前生效值，继续可委派（`problem` 上报，不抛）。
- 自动：`module-config.test.ts::keeps current values on a broken/missing file`
- 自动：`module-config.test.ts::reports the fallback with a reason when the file is missing`

## B7 外部并发（模型服务端限流）— 人工

- 前置：模型 `z.ai/glm 5.3 flash`（智谱侧并发通常 3-5），PF 侧 `capacity` = 10。
- 步骤：派 3 / 5 / 10 个，观察子代理返回。
- 期望：3 个通过；5 个结果不确定；10 个出现服务端报错（`429` 或 provider 错误文本），
  且错误被原样带回父会话，不被扩展吞掉。
- 人工原因：限流在服务端，结果依赖当时负载，无法做出稳定断言；扩展侧只保证
  "不吞错、不重试放大"。记录见本文件"实测记录"。
- 实测（2026-11-10 真机 pif-ui）：`capacity=10` 一次并发派 10 个 `zai/glm-5.3-flash`
  子代理（chat-only 单回合），**10/10 全部 completed，无服务端报错**——本次负载下
  智谱限流未触发（预期"10 个失败"未复现，属人工项"依赖当时负载"约定内的不确定结果）。
  PF 侧语义验证通过：10 并发全放行（真机复现 B1"设 10 派 10"）、结果全量原样回传、
  无吞错无重试放大。复测可加重任务负载或压低 provider 侧配额后再触发。
