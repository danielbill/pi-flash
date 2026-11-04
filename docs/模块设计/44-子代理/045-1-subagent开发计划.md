# subagent 开发计划(扩展侧,045 配套)

> 依据 [045-subagent管理](045-subagent管理.md)(设计);
> 三家 subagent 全量对照研究见本地调研笔记（不入库）。
> **优先级:L1 复刻 dsh-subagent 调用机制先行;L2 pi 增强(codemode/nested-call)
> 后置叠加。** 机制与 codemode 正交——manager/activation 是纯编排类,不依赖 codemode。
> 交付物形态:扩展主体为 **pi package 扩展**(`pf-subagents`);
> 宿主接入走 `pi-link/src/subagents.rs` 定位 + spawn 追加 `-e`。
> 状态:**M1–M3 扩展侧已完成;M4–M6 待做**(M3.5 宿主联调待 044)。

## 0. 复刻基准事实(dsh 0.2.1-rc.2)

| dsh 包 | 行数 | 复刻取舍 |
|---|---|---|
| `subagent/src/manager.ts` | 1493 | **核心复刻对象**:启动/准入/容量/父子所有权/结算/释放 |
| `subagent/src/index.ts` | 655 | provider 注册表 + 服务入口(剥 cordis 注册壳) |
| `subagent/src/types.ts` | 356 | 契约类型,直接平移 |
| `subagent/src/descriptor.ts` | 269 | 版本化 `subagent/descriptor` 会话事件 |
| `subagent/src/catalog.ts` + `projection.ts` | 360 | 子级目录投影 |
| `subagent/src/activation.ts` | 186 | 执行期记录/容量槽 |
| `subagent/src/child-agent.ts` | 291 | 子级组装 + 深度策略 |
| `subagent/src/continuation-messages.ts` | 168 | 相邻 agent 消息/结算通知 |
| `subagent/src/structured.ts` | 155 | 结构化结果暂存与闸门 |
| `subagent-spawn-in-process` / `fork-in-process` | 59 / 83 | 两个后端,极薄 |
| `tool-subagent/src/index.ts` | 505 | 委派工具(工具面) |

**合计 ~5143(服务)+1056(工具)+142(后端)**。裁掉 cordis 注册、model-selection/
preset-selection 分支(我们走 profile,见 045 §8),预计落到 3000–3500 行 TS。

## 1. M1:L1 机制复刻(spawn + caller 投递跑通)

**目标**:`agent(prompt)` 在 pi CLI 里可用,子会话是完整 pi session,结果即工具结果。

> 1.9 深度护栏依赖构造期事实
> (子会话由 `createAgentSession` 创建,默认资源发现不含 `-e` 路径 → 孙代无
> `agent` 工具),运行期 `childDepths` 作兜底。
>
> **宿主接入**:`pi-link/src/subagents.rs` 定位扩展,
> `client.rs` 在 spawn 时追加 `-e`;`scripts/release.sh` 随包分发。PF 内验收
> 步骤见 [045-3](045-3-subagent验收清单.md)。

| # | 任务 | 产出 | 验收 |
|---|---|---|---|
| 1.1 | 类型层平移 `types.ts`(子代理请求/结果/provider 契约,去 dsh 特有字段) | `src/types.ts` | tsc 通过 |
| 1.2 | descriptor(版本化自定义 entry:profile/parentToolCallId/depth/resourceSnapshot) | `src/descriptor.ts` | 写读回环测试 |
| 1.3 | Activation 生命周期 + 容量槽(在线名额、耗尽即拒 `CAPACITY_REACHED`) | `src/activation.ts` | 容量测试:8 个占满→第 9 个拒且不排队 |
| 1.4 | SubagentManager(启动/准入/父子所有权/结算/释放、兑现即发布) | `src/manager.ts` | **对着 dsh README 行为约定写测试**:启动后失败要保持队列、结算等后代、释放防迟到工作 |
| 1.5 | provider 注册表 + 能力声明协商(`continuable`/`depthLimit`/`modelOptions`/`worktree`,不满足即拒 `BACKEND_UNSUPPORTED`) | `src/registry.ts` | 缺能力后端注册即拒 |
| 1.6 | spawn 后端(全新上下文子会话,`createAgentSessionFromServices` + `parentSession`) | `src/backends/spawn.ts` | 子会话独立 JSONL、header 带父 |
| 1.7 | fork 后端(fork 已完成轮次内容) | `src/backends/fork.ts` | 子会话首条消息含父转录 |
| 1.8 | 委派工具(单工具、`namespace: subagents`、caller 投递=await 即结果、稳定分码) | `src/tool.ts` | CLI 内 spawn 一个子代理完成任务并回结果 |
| 1.9 | 深度护栏(构造期 loader 不装配 + 运行期 child-marker 不注册) | — | 子会话内 `agent` 工具不可见 |
| 1.10 | 工具描述动态措辞(按 profile 继承配置,fresh/fork 两种句) | — | 快照断言描述文本 |

**M1 出口**:三个真实场景手跑——单代理委派、并行批(模型同轮多调用)、
子代理失败(分码正确)。

## 2. M2:L2 pi 增强(codemode + 原生记账)

**目标**:不动 M1 机制,叠 pi 能力面。

> - 2.3 usage:子会话是独立 session,nested-call 上卷**不适用**(它只管
>   同会话 executeTool 链);usage 取自子会话末条 assistant message
>   (`ChildSession.usage()`),经工具结果 usage 字段回流(token 明细+成本)
> - 2.4 prepareLoadout:**不需要自写**——codemode 内建扩展统一处理
>   `codemode.mode:"only"`(所有 declared 工具含 agent 都被其隐藏)
> - 2.5 子会话 extensionFactories 装配 codemode/tool_search/MCP;
>   子会话 sandbox 含 codemode + 用户 mcp.json 服务器;防孙代在
>   codemode 层天然成立——子会话 sandbox 无 agent 工具

| # | 任务 | 验收 |
|---|---|---|
| 2.1 | `exposure: "direct"` + `outputSchema`/`structuredContent` 正式化 | 模型直调 + 脚本调双通 |
| 2.2 | codemode fan-out(`ctx.executeTool` 并行 `Promise.allSettled`) | 脚本 3 路 fan-out,只有聚合结论进模型 |
| 2.3 | nested-call usage 上卷验证(不自建账本) | 父工具结果含子代理 usage;compaction 清单含嵌套 |
| 2.4 | `prepareLoadout` 适配(`codemode.mode: "only"` 时隐藏声明) | 两模式下工具声明正确 |
| 2.5 | 子会话 `extensionFactories` 显式装配 codemode/tool_search/MCP(0.99 补 pi-web 的课) | 子会话内脚本可跑 |

**M2 出口**:同一任务用「模型并行 2 路」与「脚本 fan-out 3 路」两种写法各跑通。

## 3. M3:parent 投递(v2 后台)+ 宿主契约

> - 3.1 parent 投递:`background:true` 立即回 childId;完成经
>   `ctx.ui.notify`(onCompleted → 会话级 notify 绑定)+ 终态 descriptor entry
> - 3.2 中断三层:`subagent_interrupt`(settled=noop / live=停轮次留队列/
>   unknown=稳定分码)
> - 3.3 控制工具三件:`subagent_send`/`subagent_interrupt`/`subagent_list`
>   (schema 全局一份;list=父会话 catalog 全历史含 settled)
> - 3.4 孤儿对账:session_start 对账 `aborted:true`(宿主退出≈中止),
>   标 interrupted;resume=直接打开子会话文件(子会话是普通 pi 会话,PF
>   既有能力),无需扩展支持
> - 3.5 setWidget:待 044 联调确定

| # | 任务 | 验收 |
|---|---|---|
| 3.1 | `delivery: 'parent'`:立即返回 childId、完成走 `ctx.ui.notify` + entry | CLI 后台委派不阻塞父 turn |
| 3.2 | 中断三层语义(本地停轮次留队列 / 外部永久停 / 已结束空操作) | 三态各一测 |
| 3.3 | catalog 投影 + `list_agents` 类控制工具(复用同一 schema) | 列表与实际一致 |
| 3.4 | 孤儿恢复(`agent_settled.aborted` 区分取消/崩溃 + resourceSnapshot resume) | 杀进程重启后标 interrupted 可续 |
| 3.5 | setWidget 载荷格式确定(与 044 契约表对齐) | 宿主侧联调 |

**M3 出口**:PF 宿主接上后台通知与聚合面板(044 v2)。

## 7. 实现计划

> 依赖顺序:M4(扩展字段)先行——M5 表单/M6 面板的字段契约由它落实;
> M5 与 M6 互不依赖,可并行。验收 = 跑起 PF 在界面里看到功能。

### M4:扩展侧字段落地(pf-subagents,TS)

| # | 任务 | 验收 |
|---|---|---|
| 4.1 | `maxTurns` 软限:local backend 数 turn(子会话事件流),到限 steer「请汇总收尾」,再一轮未收尾 abort;`stopReason: max_turns` | 测试:maxTurns=2 的委派,第 2 轮 steer、第 3 轮 abort,descriptor 记 max_turns |
| 4.2 | 委派级 `maxloop`:agent 工具 inputSchema 加 `maxloop?`,优先级 maxloop > profile maxTurns > 默认 100 | CLI 委派传 maxloop=3 实测 |
| 4.3 | `systemPrompt` 替换式(before_agent_start override,替换优先于追加) | 子会话系统提示不含默认人格、含自定义 |
| 4.4 | `injectAgentsMd`(默认 true;false 时子会话不加载项目 AGENTS.md) | 关闭后子会话上下文无 AGENTS.md 内容 |
| 4.5 | `tools` 档位化:废弃白名单裁剪逻辑,改四档枚举(inherit 沿用委派者装配 / read-only / default / full → 334 行既有三套白名单) | 四档各一测,继承档与委派者一致 |
| 4.6 | profile 解析补全上述字段 + 未知字段原样透传 | 解析回环测试 |

### M5:Settings-子代理页(crates/app,设计 = 044-1 §2)

| # | 任务 | 落点 |
|---|---|---|
| 5.1 | pi-link 设施:`agents.rs` 定义目录读写(扫 `~/.pi/agent/agents/*.md` + 内置常量清单、frontmatter 解析/写回、未知字段透传、损坏定位) | `crates/pi-link/src/agents.rs` |
| 5.2 | 设置面板新页(tab + nav 项 + i18n 三语) | `settings/agents.rs` + `mod.rs` |
| 5.3 | 左列表:内置/自定义两组 + 组头计数 + 自定义行「新建」按钮 + sidebar_item 选中态 | 同上(件照抄 settings/widgets.rs) |
| 5.4 | 右表单:名称/描述/模型(继承首项+分隔+模型列表)/工具权限四档/系统提示 textarea/最大轮次四档(50·100·不限·指定 N)/注入AGENTS.md 开关 | 同上 |
| 5.5 | 动作:保存(写回 md)/删除(进回收站)/新建/内置只读 + 复制为自定义 | 同上 |
| 5.6 | 损坏态:红条 + 打开文件修复 + 删除 | 同上 |

### M6:子代理面板(crates/app,设计 = 044-1 §1)

| # | 任务 | 落点 |
|---|---|---|
| 6.1 | `subagent_catalog::read_catalog(file, from_offset)` 流式扫 descriptor,同 childId 末条覆盖 | `crates/pi-link/src/subagent_catalog.rs` |
| 6.2 | 操作栏按钮 + 运行中计数徽标(数 phase ∈ provisioning/active) | `session/mod.rs session_fn_bar` |
| 6.3 | 浮层面板(absolute 挂 chat-wrap 顶左,宽 400、最大高内容区上半;✕/再点按钮关,不挂全屏外点拦截) | 新 `session/subagent_panel.rs` |
| 6.4 | 打开期 1s 轮询增量读;整行点击 = open_session 开正式 tab 并关浮层 | 同上 |
| 6.5 | 空态文案 + i18n 三语 | 同上 |

### 收尾

- i18n_audit 门禁 + `cargo test -p app` 全绿 + pif-ui 自动化链路跑 M5/M6 界面断言
- 与用户一起做完整测试(045-3 验收清单)

## 10. 不做

- 不做 team/roster/DAG(远期,045 §8 之外的协作面)
- 不做多后端外部 provider(ACP/CC/Codex,远期)
- 不自建 token 账本、不自建通知去重、不自建 toolCallId 映射(pi 原生)
