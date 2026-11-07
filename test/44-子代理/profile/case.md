# 44-子代理 · 自定义 subagent（profile）

对应 `044-2-子代理testcase.md` 第 4 项"自定义subagent测试"。
profile 解析器：`src/profile.ts`；两级目录：`<PF 数据目录>/agents/` 与项目 `.pi/agents/`
（项目遮蔽全局）；内置 explorer / general-purpose / summarizer。

## D1 工具档位（tools: 六档 tier）

- 步骤：profile 写合法档位 → 通过；写旧式 allowlist 列表 / 档位外字符串 → 拒绝。
- 期望：fail-closed，不静默降级。
- 自动：`profile.test.ts::builds a profile`
- 自动：`profile.test.ts::rejects a tools value outside the tier enum`
- 自动：`profile.test.ts::rejects a tools list — tiers replaced the allowlist`

## D2 工具档位真的生效（full+ 的 ext / mcp 选择）

- 说明：full+ 档允许挑 pi 扩展与 MCP server；扩展的实际可用性属 pi 宿主，
  这里钉住"选中的组合被原样传给会话工厂"。
- 自动：`local-provider.test.ts::forwards the profile composition to the factory`

## D3 技能注入（skills）

- 期望：profile 的 `skills` 列表原样进会话工厂；空/缺省走 loader 自身发现。
- 自动：`local-provider.test.ts::forwards the profile composition to the factory`（组合断言含 skills）

## D4 AGENTS.md 注入开关

- 期望：`injectAgentsMd: false` 时子会话不加载项目 AGENTS.md；缺省 = 加载。
- 自动：`local-provider.test.ts::forwards the profile composition to the factory`（组合断言含 injectAgentsMd）

## D5 附加身份（systemPrompt 覆盖）

- 期望：frontmatter 无 `systemPrompt` 时，正文即人设；写 `systemPrompt` 块时替换。
  实际回复内容（如"你是大恐龙"）属模型行为，人工冒烟即可。
- 自动：`profile.test.ts::reads a systemPrompt block scalar as the replacement persona`
- 自动：`profile.test.ts::treats a document without frontmatter as a pure prompt`

## D6 模型三优先级

- 期望：调用参数 > profile 的 `model` > 配置默认（`config.json` 的 `model`）；
  非法 `provider/model` 字符串在配置层被拒。
- 自动：`local-provider.test.ts::forwards the profile composition to the factory`
- 自动：`module-config.test.ts::rejects malformed JSON and a non-provider/model model string`

## D7 两级目录与遮蔽

- 期望：项目 `.pi/agents/<id>.md` 遮蔽全局同名；无文件时回退内置三件套。
- 自动：`profile.test.ts::prefers the project directory over the global one`
- 自动：`profile.test.ts::falls back to the global directory`
- 自动：`profile.test.ts::falls back to the built-in explorer when no file exists`

## D8 未知名单 fail-closed

- 期望：`profile: '不存在'` → 稳定错误码，不降级到默认。
- 自动：`profile.test.ts::rejects unknown profiles with a stable code`
- 自动：`tool.test.ts::rejects a profile outside the allowlist before delegating`

## D9 路径穿越拒绝

- 期望：profile id 含 `..` → 拒绝。
- 自动：`profile.test.ts::rejects path traversal in profile ids`

## D10 thinking level

- 期望：七档合法值可解析，档外值拒绝；缺省 = auto。
- 自动：`profile.test.ts::parses the thinking level; absent = auto (2026-11-07 子代理表单)`
- 自动：`profile.test.ts::rejects a thinking value outside pi's seven levels`

## D11 人工冒烟（模型行为，不自动化）

- 指定 `OpenRouter/crouton alpha` 的 profile：期望按 OpenRouter 地区限制返回其报错
  文本（结果依赖服务端，无法稳定断言；扩展侧只保证错误原样上抛，见 maxloop/case.md C2 同款约定）。
