# 44-子代理 自动验收

驱动：`node test/44-子代理/run.mjs`（一次跑全部自动化用例 + 宿主模型守卫）。
用例-断言映射分册：

| 分册 | 覆盖 | 对应 testcase 项（044-2） |
|---|---|---|
| [接入控制](接入控制/case.md) | 并发准入/深度/结算（A1-A12）+ 套娃口径（A13 人工） | 0 套娃 / 1 内部并发 |
| [并发上限](并发上限/case.md) | config.json 容量配置（B1-B7） | 1 内部并发 / 2 外部并发 |
| [maxloop](maxloop/case.md) | 轮次预算软限/硬停（C1-C6） | 3 max_loop |
| [profile](profile/case.md) | 自定义 subagent（D1-D15） | 4 自定义subagent |
| [生命周期与工具面](生命周期与工具面/case.md) | 投递/描述符/中断/对账（E1-E8） | 4 附带验收面 |

自动化主体 = `extensions/pf-subagents` 的 node:test（138 条，2026-11-07 实况），
`run.mjs` 只做两件额外的事：

1. 聚合跑扩展测试并报告 pass/fail；
2. **宿主模型守卫**（接入控制 A12）：扫描三级 agents 目录的 profile frontmatter，
   确认无人给子会话装配 `pf-subagents` —— 这是 PARITY.md"depth≥2 不可达"结论的
   前提，被打破时验收 FAIL 并提示重开对齐。

人工项（结果依赖外部服务端或真实模型行为，不做稳定断言）：

- A13 套娃端到端（向子代理派发委派任务 → 失败）
- B7 外部并发（z.ai/glm 服务端限流，3/5/10 个）
- D11 OpenRouter 地区限制冒烟
- D12 无技能 subagent 执行妙想查询 → 报错
- D13 东财财经员双技能冒烟（财经查询成功 / jev skill 报错）
- D14 AGENTS.md 注入问答冒烟（环境注入 off → 回答失败）
- D15 附加身份（"你是大恐龙"）冒烟

当前状态：2026-11-07 验收 PASS（extension pass=138 fail=0；host-model pass）。
