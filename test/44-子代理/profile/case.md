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
- 实测（2026-11-10 真机）：`stealth/crouton-alpha` 已于 2026-11-09 被 OpenRouter 下架，
  实际返回 `404: Crouton Alpha is no longer available. This stealth model was retired on
  November 9, 2026`，错误原样上抛父会话、文本可读——上抛语义 **PASS**；
  "地区限制"场景未能复现（模型已退役，复测需另选受限模型）。

## D12 无技能 subagent 技能负向冒烟（人工）

- 对应 044-2 第 4 项"技能设置测试"第一条。
- 步骤：创建**没有任何 skills** 的 subagent，让它执行妙想查询技能（mx-search）。
- 期望：返回错误（技能不可用），错误文本可读；不得静默编造结果。
- 实测（2026-11-10 真机）：子代理明确答复"我没有 mx-search 技能，无法执行该查询"，
  `tool_search` 检索技能返回 No matching tools found，未用其他方式替代。**PASS**。
  （无技能 profile 用 `skills: [__none__]` 名单式过滤实现——清单语义"空/缺省=全部发现集"，
  想表达"零技能"需给一个不存在的技能名。）

## D13 东财财经员双技能冒烟（人工）

- 对应 044-2 第 4 项"技能设置测试"【东财财经员】两条。
- 前置：按 testcase 建 profile——skills 只设 mx 系列、环境注入 off、tools default、
  model 继承、描述与提示词照 044-2 原文。
- 步骤1：让它执行财经新闻查询 → 期待返回结果（mx 技能可用）。
- 步骤2：让它执行 jev skill → 期待返回错误（未授权技能）。
- 实测（2026-11-10 真机）：步骤1 真调 mx-search 返回当日 A 股要点（**PASS**）；
  步骤2 明确报"我没有 jev 技能"，且可见技能面恰好只有 mx 三件套（过滤语义精确）。**PASS**。
- 环境备注：mx 技能不在 pi 全局技能目录，经**项目级 `<cwd>/.pi/skills`** 注入测试目录；
  `MX_APIKEY` 走用户级环境变量被子代理继承。profile 已留存至 `~/.pi-flash/agents/dongcai.md`。

## D14 AGENTS.md 注入问答冒烟（人工）

- 对应 044-2 第 4 项"AGENTS.MD注入测试"。
- 步骤：向【东财财经员】（环境注入 off）询问环境问题（如项目约定、工作区内容）。
  **提问不得点名 AGENTS.md**（点名会诱导子代理用 read 直接读文件，测的就不是注入了）。
- 期望：回答失败/无环境信息——它读不到 AGENTS.md。D4 已自动验证转发开关，
  本条验证运行时行为。
- 实测（2026-11-10 真机）：首跑提示词点名 AGENTS.md，子代理用 bash+read 直读文件答出暗号
  （测试设计泄漏，判无效，非实现缺陷）；重跑改为"不读任何文件、凭启动时上下文回答"，
  子代理答"不知道"。**PASS**。正向对照 dino（默认注入 on）答出暗号，负向/正向闭环成立。

## D15 附加身份冒烟（人工）

- 对应 044-2 第 4 项"附加提示词测试"。
- 步骤：profile 设置额外身份"你是大恐龙"，派任务并问"你是谁"。
- 期望：按附加身份回复。D5 已自动验证 persona 解析/替换机制，本条验证模型行为。
- 实测（2026-11-10 真机）：dino 子代理自称"**大恐龙**"（正文人设生效）。**PASS**。
