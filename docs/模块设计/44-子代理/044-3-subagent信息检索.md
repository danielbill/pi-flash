# 044-3 会话 subagent 信息检索（现行设计）

> 状态：**现行**（已落地）。上游：`044-0`（descriptor 字段总表）、`044-5`（扩展侧机制）、`044-1`（Settings 页）。
> 本文只管一件事：**父会话 JSONL 里的 descriptor 日志怎么读出来**。UI 形态、字段语义、写盘时机、append-only 契约一律不动。
> 落点：检索层 `crates/pi-link/src/subagent_catalog.rs`；宿主侧 `crates/app/src/session/subagent_panel/mod.rs`、`crates/app/src/session/pool/open.rs`。

## 1 设计原则

1. **载体不动**：append-only descriptor 仍是唯一事实源，末条覆盖语义不变——老会话免迁移、跨版本兼容、不依赖扩展先升级
2. **解析最少**：只有每个 childId 的**末条**值得 JSON 解析；判「谁是末条」用字节扫即可
3. **线程隔离**：UI 线程永不读会话文件、永不解析 JSON；只接收快照
4. **缓存自校验**：偏移量随行的缓存（`len/offset/rows`），文件变小或身份不符一律归零重扫，兜底永远回到全扫——不允许"缓存坏了就答错"
5. **热路径零读**：打开子会话（tab 标题）只查内存，不碰磁盘；缺了先占位、后台补

## 2 检索层（`crates/pi-link/src/subagent_catalog.rs`）

### 2.1 接口

```rust
pub struct CatalogScan {
    pub rows: Vec<CatalogRow>,   // 本块末条投影（首现序，同 id 已末条覆盖）
    pub next_offset: u64,        // 下一轮续读起点（半行不推进）
    pub restarted: bool,         // 本块是「文件变小 → 归零重扫」的结果
    pub parsed: usize,           // 本块真正跑了 serde 的行数（验收量尺）
}
pub fn read_catalog(file: &Path, from_offset: u64) -> CatalogScan
```

- `from_offset` 增量续读；半行（文件尾部未写完）**不推进 offset**，下轮重读
- 打不开 / 读到一半失败 → 空块（offset 不动，下轮重试）；文件变小 → 归零重扫并把 `restarted` 置真（宿主据此**先清 rows 再合并**，否则旧文件的行永久残留）；非法 UTF-8 **逐行跳过**（一行坏不让整个 tick 白跑）
- `rows` 输出顺序 = **首次出现序**（与逐行全解析等价）

### 2.2 算法：字节预筛 + 末条下沉

```
1. 读入 [from_offset, len) 整块字节（read_to_end + 逐行 from_utf8）
2. 逐行切分（\n），累计 consumed；末段不以 \n 结尾 → 半行，不消费
3. 逐行字节预筛 b"subagent/descriptor"（19 字节，非命中行永不进 serde_json）
   - 命中 → 在该行内找 b"\"childId\":\"" 提取 id 字节
   - 每个 id 只保留最后 K=4 条候选行区间（首现 offset 另记）
4. 收尾：每个 id 从新到旧试解析候选行，第一个「parse_line 通过且 child_id 与字节提取一致」
   的行胜出；全不通过则该 id 丢弃（fail-closed）
5. 按首现 offset 排序输出；next_offset = from_offset + consumed；parsed = 试解析过的行数
```

要点：

- JSON 解析次数从 `行数` 降到 `唯一 childId 数`；保序靠首现偏移，不做第二遍解析
- 候选深度 K=4 兜住「消息里整段贴了 descriptor JSON」这类伪行（伪行 `type != "custom"` 被 `parse_line` 拒，自动回退到真末条）
- 只依赖「同 id 末条最新」这一条语义，写侧如何瘦身都不影响

### 2.3 实测（27 MB / 37,882 行 / 90 个子代理）

| 口径 | 结果 |
|---|---|
| `read_catalog(file, 0)` | 32 ms（release），解析 90 行 |
| 增量空读（无新字节） | 0.2 ms（仅 open + metadata） |
| 行数 vs 解析数 | 37,882 行 → 90 次 serde |

## 3 宿主侧（`crates/app`）

### 3.1 后台读 + 在飞守卫

`crates/app/src/session/subagent_panel/mod.rs` 的 `spawn_poll` 分两段：

1. UI 线程：取 `(file, offset)` 快照，若 `read_in_flight` 已在跑则跳过本 tick
2. `background_executor().spawn(...)` 执行 `read_catalog`（IO + 解析全在后台）
3. 回灌守卫（`apply_catalog_scan`）：`panel.file` 未变才继续（变了 = 别的会话的陈旧快照，直接丢）；同会话则先清 `read_in_flight`，再要求 `panel.offset == from` 才 merge + 重算按轮投影 + `cx.notify()`——offset 已被另一份回灌推进就丢弃（下一 tick 自愈）
4. `scan.restarted == true` → 先清 `rows` 再合并

`read_in_flight: bool` 落在 `SubagentPanel` 上，与 `polling` 同族。

### 3.2 进程内 stash（父↔子来回切不重扫）

`SubagentPanel` 的 `stash: Vec<CatalogStashEntry>`（`{file, offset, rows}`，LRU ≤ 4，按文件路径）：

- 入口统一在 `adopt(file)`：先 `stash_put()`（`std::mem::take` 取走 rows，不留重复副本；同名先剔、放末尾、超 4 丢最旧），再把菜单锚/宽限/`in_flight` 清零——锚属于旧会话的轮卡，留着会错位
- 会话切走时把当前 `(file, offset, rows)` 存入 stash（不动 offset——文件增长由增量读跟随）
- 切回来先查 stash：命中 → 恢复 `rows/offset` 后**只读增量**（通常 0 字节 → 一次 metadata）；未命中 → 全扫（后台）

### 3.3 按轮投影

`turn_agents: Vec<Vec<CatalogRow>>` 由 `regroup_turn_agents(&rt, cx)` 一处重建（rows ≤ 90，每 tick 数十 µs，远小于一次读盘）。归组依据：`ToolCall(agent).id` ↔ `row.parent_tool_call_id`，`CallSite { turn, msg, block }` 由 `scan_agent_calls` 增量扫描（最后一条消息重扫，接住流式/后到 toolCall）。

若将来真成瓶颈，改为 `Vec<Vec<usize>>` 下标只需要动这一处。

### 3.4 打开路径零读

`crates/app/src/session/pool/open.rs` 不做同步 `read_catalog`，改用 `Chat::subagent_tab_label(&child, &key, cx)`：

- 主路径：从 `panel.rows`（或 stash）按 `session_file` 匹配取 `label → profile → 「子代理」`；从导航菜单点进来的场景必然命中
- 兜底：没命中时先用占位标题 `tr("subagent.tab")` 建 tab，同时后台扫父文件，扫完若 tab 还在就改名（`pool.tab_labels.insert`）
- 匹配逻辑：`SubagentPanel::label_for_child(&Path)`（rows + stash 都查）与 `subagent_panel::child_tab_label(&CatalogRow)`（label → profile），口径唯一

`read_tail_messages` 保留（≤256 KB，~1-2 ms，首帧有内容的来源）。

## 4 验收

### 4.1 `[perf]` 日志（进代码，长期量尺）

`PI_FLASH_PERF=1` 时宿主侧 poll 每完成一次后台读打一行（pi-link 保持纯函数、不碰全局 io）：

```
[perf] subagent_catalog bytes=27241180 us=32160 parsed=90 rows=90 mode=full|incr
```

`us` 在**后台线程内**计时（只算 `read_catalog` 本身，不含排队/回灌）；`mode=full` 的条件是 `from == 0 || scan.restarted`。稳态（无新字节）每 tick 打 `bytes=0 us=<200 mode=incr`——这是「有没有误走全扫」的判据。
线程纪律：`read_catalog` 只允许在 `background_executor` 里调（review 检查点 + §3.1 守卫）。

### 4.2 验收项（最坏样本 = 27 MB / 37,882 行）

| 项 | 标准 |
|---|---|
| 首次打开子代理会话（导航菜单点行） | UI 不冻结（无 ≥16 ms 主线程 IO）；tab 立即出现，标题同步命中面板缓存 |
| 父 ↔ 子来回切 3 次 | 每次只走增量（`mode=incr` / idle），不再出现 `mode=full` |
| 切回父会话 | 导航卡第三行状态点 1 tick（≤1 s）内恢复，且恢复过程不卡 |
| 全量首扫（进程内首次见到该会话） | 后台 ≤ 100 ms（release），期间 UI 可交互 |
| 功能零回归 | 导航第三行点数/配色、hover 菜单行数与字段、运行中徽标、tab 标题、只读观察窗同步 |

### 4.3 测试

`crates/pi-link/src/subagent_catalog/tests.rs` 15 条（`cargo test -p pi-link` 常跑），含：

- `quoted_descriptor_line_does_not_shadow_real_last_entry`：真末条后追加两行「消息里贴的 descriptor JSON」→ 仍取真末条
- `candidate_depth_covers_three_pseudo_lines`：连贴 3 条伪行 → K=4 仍兜住
- `matches_naive_scan_on_large_log`：合成 2,700 条 descriptor（90 id × 30 轮）+ 非 descriptor 行 → 与全解析参考实现逐字段相等
- `big_file_without_descriptors_is_skipped`：20,000 行纯文本、无 marker → 0 行、offset 推进
- `truncated_file_reports_restarted`：offset 越界 → `restarted == true` 且只出新文件的行（append-only 契约下只有「文件变短」可检测；同长重写的旧 offset 会读到行中书，属字节游标固有限制，会话文件不重写故可接受）

宿主侧（后台读 / stash 命中 / 打开零读盘）是时序行为，验收走 §4.1 的 `[perf]` 日志断言；UI 只做「导航第三行点色、hover 菜单、tab 标题、观察窗同步」的功能回归。

## 5 不做 / 延后

| 项 | 原因 |
|---|---|
| 写侧瘦身（composition 只写首条 / stats 节流） | 已不需要：写侧只在相位变迁/终态落盘（044-0），本文算法只依赖末条语义 |
| 落盘索引（`<PF数据目录>/subagent-index/…`） | stash 已覆盖会话内切换；重启首开仍只是一次 ≤100 ms 后台扫，不值得多一层失效面 |
| 会话级聚合统计（子代理总数 / 合计 token / 墙钟） | 数据在内存里，作为检索层派生物另案；本文不引入新存储 |
| descriptor 轮询间隔调整 | 1 s / 5 s 现行值够用，问题在单次代价不在频率 |
| `memchr::memmem` 整块 marker 扫描 | 预计再降到 10-15 ms，收益小于一个 tick，且新增直接依赖，暂不引入 |
