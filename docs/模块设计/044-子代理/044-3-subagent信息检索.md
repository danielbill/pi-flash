# 044-3 会话 subagent 信息检索（现行设计）

> 状态：**现行**（pi-flash-9eck 重写，2026-11-08 用户定夺）。
> 上游：`044-0`（观测数据面）、`044-5`（扩展侧机制）、`044-1`（Settings 页）。
> 本文只管一件事：**宿主从哪里拿到子代理观测信息**。UI 形态、字段语义见 044-0。
> 落点：`crates/app/src/session/subagent_panel/mod.rs`、`crates/app/src/session/pool/open.rs`。

## 0 旧机制已删（descriptor 落盘 + catalog 读取整链）

2026-11-08 用户定夺：扩展往父会话 JSONL 逐条追加 `subagent/descriptor` 的机制
**整体删除**（宿主 `pi-link::subagent_catalog` / `read_catalog` / stash /
1s 读盘轮询一并删）。原因：

- 逐条持久化曾以 ~15 条/秒把父会话灌到 209MB / 29.9 万条（洪泛根因）
- 收窄到相位变迁/终态落盘后仍有残余成本，而它的全部消费面都能从
  **主会话自身**推导——子会话不回补主会话任何东西，主会话对子代理只有
  无状态引用（agent 工具卡）
- descriptor 落盘是 agent 自创机制（pi-web 无此物），非产品需求

扩展侧 `SubagentDescriptor` 只留**内存**（`children` 工具实时统计可读），
永不写转录。

## 1 现行设计：主会话自足推导

宿主对子代理的全部观测都来自父会话消息，零读盘、零 JSON 解析轮询：

| 消费方 | 事实源 |
|---|---|
| 导航轮卡第三行点阵 | `agent` ToolCall 块自身：无 result → 运行中灰点；result details `stop_reason` 定终态色（completed=NOTICE、aborted/max_turns=WARN、其余=danger） |
| caller 交付卡片（状态/错误/开子会话） | 工具 result details（扩展 `tool.ts success()`：child_id / stop_reason / session_file / error_code / duration_ms 全齐） |
| background 交付卡片终态 | 父会话 `subagent-completion` custom 消息（扩展 onCompleted 落盘，5554fa4a）→ `messages::apply_completion_messages` 按 childId 折进 ToolCall details + result_arrived（装载 / 磁盘链路 / live 事件三处收口） |
| 子会话 tab 标题 | 主会话反查 agent 调用 args：`label → profile → prompt 首行`，查不到占位「子代理」 |
| 卡片点开子会话（details 缺 session_file 窗口期） | `Chat::session_file_for_child`：主会话消息里按 details.child_id 反查 |

## 2 结构：无轮询、单一状态（2026-11-08 重构）

**设计判据（用户定夺）**：一个子会话只有一个状态。重载后不允许出现
「先运行中灰点、后终态青点」的两段式渲染。

- 终态一次性归一：`messages::apply_completion_messages(msgs)` 把
  `subagent-completion` 消息折进派发卡 details + `result_arrived`，
  三处收口——(1) `project_rpc_messages`（装载）、(2) `msgs_from_tail`
  （秒开 tail / 观察窗 / 外部追加对账 / leaf 修复，全部磁盘链路）、
  (3) live `message_start` custom 臂。幂等：值相同零变更零 notify。
- 第三行点阵 = 导航缓存字段 `NavData::turn_agents`（与 `turns` 同一次
  构建、同一个 `notify_list` 版本），由纯函数
  `subagent_panel::project_turn_agents(&msgs)` 推导。渲染层不再另存一份。
- `SubagentPanel` 退化为标记类型（`dlg.subagent_panel` 的存在语义 +
  自动化快照字段）；1s 常驻轮询、落点映射、`regroup_turn_agents`、
  `sync_cards_from_completions` 全部删除——没有定时器、没有第二份状态。

## 3 老洪泛会话的兜底（不受本机制管辖，但同根因）

descriptor 时代灌爆的老会话文件尾段全是垃圾行，两个独立兜底
（pi-flash-9eck 同批落地）：

1. `pi_link::sessions::read_tail_messages`：尾窗 renderable < 10 时倍增窗口
   重读（256KB → 2MB 封顶）——tail 秒开不再被垃圾尾段击穿卡「载入中」
2. 空 tail 不写 `session_tail_cache` / 启动预载（毒缓存曾致每次重开都复用
   空投影、永远走慢放行链）；leaf-repair 只增强不闸门（RPC 快照先投影放行）

## 4 验收

- `cargo test -p app --bin pi-flash`：`subagent_panel::dots_tests`
  （轮序归属 / 首帧即终态 / 归一幂等 / 非 agent 忽略）+
  `rows::tests`（点色口径：运行中灰点 / 终态分色）
- `[perf] subagent_catalog` 日志已随 read_catalog 删除，不再存在
- 功能回归：导航第三行点数/配色、卡片三态、background 卡终态回灌、
  子会话 tab 标题、只读观察窗同步
