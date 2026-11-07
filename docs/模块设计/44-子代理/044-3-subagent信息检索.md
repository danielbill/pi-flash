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
| background 交付卡片终态 | 父会话 `subagent-completion` custom 消息（扩展 onCompleted 落盘，5554fa4a）→ `sync_cards_from_completions` 按 childId 回灌 ToolCall details + result_arrived |
| 子会话 tab 标题 | 主会话反查 agent 调用 args：`label → profile → prompt 首行`，查不到占位「子代理」 |
| 卡片点开子会话（details 缺 session_file 窗口期） | `Chat::session_file_for_child`：主会话消息里按 details.child_id 反查 |

## 2 结构（`SubagentPanel`）

- 纯内存推导，`polling` 守卫的 1s 常驻任务保留（同会话流式自愈），但
  **每 tick 只做内存扫描**：`scan_agent_calls` 增量维护
  `toolCallId / childId → CallSite{turn,msg,block}` 落点映射（游标 =
  消息数/轮数/首条时间戳，换会话/fork/回退作废重扫）+
  `regroup_turn_agents` 全量重投影第三行点阵（每秒一次、微秒级）
- 没有 rows / offset / stash / 文件字段；`bg 终态回灌` 幂等（值相同不 notify）

## 3 老洪泛会话的兜底（不受本机制管辖，但同根因）

descriptor 时代灌爆的老会话文件尾段全是垃圾行，两个独立兜底
（pi-flash-9eck 同批落地）：

1. `pi_link::sessions::read_tail_messages`：尾窗 renderable < 10 时倍增窗口
   重读（256KB → 2MB 封顶）——tail 秒开不再被垃圾尾段击穿卡「载入中」
2. 空 tail 不写 `session_tail_cache` / 启动预载（毒缓存曾致每次重开都复用
   空投影、永远走慢放行链）；leaf-repair 只增强不闸门（RPC 快照先投影放行）

## 4 验收

- `cargo test -p app`：`turn_scan_tests`（落点归组/childId 反查）+
  `rows::tests`（点色口径：运行中灰点 / 终态分色）
- `[perf] subagent_catalog` 日志已随 read_catalog 删除，不再存在
- 功能回归：导航第三行点数/配色、卡片三态、background 卡终态回灌、
  子会话 tab 标题、只读观察窗同步
