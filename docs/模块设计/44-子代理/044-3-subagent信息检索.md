# 044-3 会话 subagent 信息检索（设计）

> 状态：**设计定稿 2026-11-07，待实现**。
> 上游：`044-0-子代理设计.md`（产品/宿主唯一定稿，descriptor 字段总表在那边）、
> `044-5-subagent管理.md`（扩展侧机制）、`044-1-子代理面板.md`（Settings 页）。
> 本文只管一件事：**父会话 JSONL 里的 descriptor 日志怎么读出来**。
> UI 形态、字段语义、写盘时机、append-only 契约一律不动。

触发：真机报「导航栏点开子代理会话卡顿」。查下来不是 UI 问题——是**检索设计问题**：
父会话文件 27 MB，真正需要的只有 90 条，却每次都把 37,387 行全量 JSON 解析**两遍**，
而且解析发生在 **UI 线程**。

---

**实现状态（2026-11-07）**：已落地。检索层 `crates/pi-link/src/subagent_catalog.rs`
（实测 27 MB 样本 892 ms → **32 ms**，serde 调用 37,387 → 90）；宿主侧
`crates/app/src/session/subagent_panel/mod.rs`（后台读 + 在飞守卫 + stash LRU4）、
`crates/app/src/session/pool/open.rs`（打开路径零读盘）。测试 15/15 绿。

## 1 现状实测（2026-11-07，本机真实会话）

### 1.1 数据面

样本：`C:/Users/bi_da/.pi/agent/sessions/--D--ai_workspace-pf_test--/2026-11-07T01-30-29-030Z_01a1b3a0-e725-7576-b0fe-0f5043fbea23.jsonl`

| 指标 | 值 |
|---|---|
| 文件大小 | 27,241,180 B（27 MB） |
| 总行数 | 37,882 |
| `subagent/descriptor` 行 | 37,387（26,021,936 B = **文件的 96%**） |
| 唯一 childId | **90** |
| 平均每子代理重写次数 | **≈ 415** |

同机另一份（`--D--my_obsidian-gitee_vault-mynotes-20观宏知微--/2026-11-05T23-50-26-245Z_01a1ae1e-….jsonl`）：
31,728,554 B / 36,352 条 descriptor，同形态。

**为什么写得这么密**：`extensions/pf-subagents/src/index.ts:568-574` 的 `persist` 每次
descriptor 变化都 `pi.appendEntry(DESCRIPTOR_TYPE, descriptor)`，写的是**整条**（含
`composition.systemPrompt` / `composition.tools` 全量 + `usage` 每轮累计 + `stats`），
`manager.ts:32` 的 `onDescriptor` 在 phase/usage/stats 任一处变化时都触发。
append-only 末条覆盖是该契约的正确读法，但代价是「事实重复 415 倍」。

**【已解决 2026-11-07】** 用户定夺**删除逐消息持久化**（上面的写密根因已铲除）：
`manager.ts` watch 回调只更新内存 descriptor，落盘收敛为相位变迁/终态（每子代理
~3 条）。本文 §1 的密度/读取代价实测是对**存量洪泛文件**的记录；新会话不再产生。
运行期 stats 不持久化——「不能统计就不统计」，面板运行中行停在激活快照、终态补齐。

### 1.2 读取代价

`crates/pi-link/src/subagent_catalog.rs:130` `read_catalog(file, 0)` 在样本上的实测
（3 次取最优，Windows，NVMe）：

| 口径 | release | debug |
|---|---|---|
| **现状 `read_catalog`** | **892 ms**（另一次 908 ms） | **2,037 ms** |
| 仅 `fs::read`（裸读 27 MB） | 10.5 ms | 12.1 ms |
| 原型：字节预筛 + 末条解析（§3.4） | **44 ms** | 156 ms |
| 实际 JSON 解析行数 | 37,387 → **90** | 同 |

结论：**卡顿 ≈ 892 ms（release）/ 2 s（debug）的主线程冻结**，其中 99% 是 JSON 解析，
不是磁盘。现状实现还额外扫了**两遍**（`subagent_catalog.rs:158-181`：先 `by_id` 建表、
再为保序把所有行重解析一遍），所以是 2× 解析。

### 1.3 卡顿链路（三个读点，全部在 UI 线程）

| # | 位置 | 场景 | 代价 |
|---|---|---|---|
| ① | `crates/app/src/session/subagent_panel/mod.rs:131`（`subagent_panel_poll`，跑在 `this.update` 里） | 面板缓存建立/会话切换后的首个 tick | 892 ms |
| ② | `crates/app/src/session/pool/open.rs:92` | **打开子会话**时为了查 tab 标题同步 `read_catalog(&parent, 0)` | 892 ms |
| ③ | `crates/app/src/session/subagent_panel/mod.rs:114-136` + `ensure_subagent_panel` 归零 | 从子会话切回父会话：panel.file 变了 → rows/offset 清零 → 下一 tick 重扫 | 892 ms/次切换 |

**用户报的那一次卡顿 = ②**：导航面板 → hover 菜单 → 点行 → `open_subagent_child`
（`crates/app/src/session/subagent_panel/rows.rs:51`）→ `open_session` → ② 的同步全扫。
紧随其后 ③ 还会在**每次**父↔子切换时再来一遍，只是错后一个 tick，容易被当成「又卡一下」。

已排除：`read_tail_messages(&path, 256 KB, 100)`（`open.rs:55`）和
`is_subagent_session`（`sessions/info.rs:24`，64 KB 有界扫）都是小常数，不是元凶。

---

## 2 设计原则（五条）

1. **载体不动**：append-only descriptor 仍是唯一事实源，末条覆盖语义不变——老会话文件
   免迁移、跨版本兼容、不依赖扩展先升级。（写侧 2026-11-07 定夺：只相位变迁/终态落盘，
   运行期不写——见 §1.1 已解决注记）
2. **解析最少**：只有每个 childId 的**末条**值得 JSON 解析；判「谁是末条」用字节扫即可。
3. **线程隔离**：UI 线程永不读会话文件、永不解析 JSON；只接收快照。
4. **缓存自校验**：偏移量随行的缓存（`len/offset/rows`），文件变小或身份不符一律归零重扫，
   兜底永远回到全扫——不允许「缓存坏了就答错」。
5. **热路径零读**：打开子会话（tab 标题）只查内存，不碰磁盘；缺了先占位、后台补。

---

## 3 检索层（`crates/pi-link/src/subagent_catalog.rs`）

### 3.1 接口不变

```rust
pub struct CatalogScan {
    pub rows: Vec<CatalogRow>,   // 本块末条投影（首现序，同 id 已末条覆盖）
    pub next_offset: u64,        // 下一轮续读起点（半行不推进）
    pub restarted: bool,         // 本块是「文件变小 → 归零重扫」的结果
    pub parsed: usize,           // 本块真正跑了 serde 的行数（验收量尺）
}
pub fn read_catalog(file: &Path, from_offset: u64) -> CatalogScan
```

- `from_offset` 增量续读语义不变；半行（文件尾部未写完）**不推进 offset**，下轮重读；
- 打不开 / 读到一半失败 → 空块（offset 不动，下轮重试）；文件变小 → 归零重扫并把
  `restarted` 置真（宿主据此**先清 rows 再合并**，否则旧文件的行永久残留）；
  非法 UTF-8 从「整块放弃」改成**逐行跳过**（一行坏不再让整个 tick 白跑）；
- `rows` 输出顺序 = **首次出现序**（与现状逐行一致，已用真实文件比对等价，见 §3.4）。

### 3.2 算法：字节预筛 + 末条下沉

```
1. 读入 [from_offset, len) 整块字节（read_to_string 改 read_to_end + 逐行 from_utf8）
2. 逐行切分（\n），累计 consumed：
     - 末段不以 \n 结尾 → 半行，不消费（现行语义）
3. 逐行字节预筛 b"subagent/descriptor"（19 字节，非命中行永不进 serde_json）
     - 命中 → 在**该行内**找 b"\"childId\":\"" 提取 id 字节
     - 每个 id 只保留最后 K=4 条候选行区间（首现 offset 另记）
4. 收尾：每个 id 从新到旧试解析候选行，第一个「parse_line 通过且 child_id 与字节提取一致」
   的行胜出；全不通过则该 id 丢弃（fail-closed，与现行一致）
5. 按首现 offset 排序输出；next_offset = from_offset + consumed；parsed = 试解析过的行数
```

要点：
- 现状「末条覆盖 + 保序」靠 HashMap + 第二遍解析实现；本设计把这两件事**都降到字节层**，
  JSON 解析次数从 `行数` 降到 `唯一 childId 数`。
- 第二遍解析**删除**（保序改记首现偏移）。
- 只有候选末条进 serde；候选深 K=4 是为了兜住「消息里整段贴了 descriptor JSON」这类伪行
  （伪行 `type != "custom"` 会被 parse_line 拒，自动回退到真末条）。

### 3.3 与写侧的关系

不需要写侧配合即可落地。若后续做写侧瘦身（composition 只写首条 / stats 节流），
本算法不动、继续成立——它只依赖「同 id 末条最新」这一条语义。

### 3.4 原型实测与等价性（同样本，release）

| 实现 | 耗时 | 解析行数 | 输出 |
|---|---|---|---|
| 现状 | 892 ms | 37,387 × 2 遍 | 90 行 |
| 原型 v1（逐行预筛） | 44 ms | **90** | 90 行，**逐行等价**（child_id/phase/stop_reason 全同），`next_offset` 相同 |
| 原型 v2（整块 marker 扫描） | 50 ms | 90 | 等价，但不比 v1 快（回看行界的反向扫抵消收益）→ **取 v1** |
| **落地实现**（`read_catalog`，2026-11-07） | **32 ms** | **90** | 90 行、`next_offset == len`、与原型等价；增量空读 0.2 ms（仅 open+metadata） |

**可选的进一步优化**（不进第一版）：`memchr::memmem` 整块扫 marker（memchr 2.8.3 已在
`Cargo.lock`，零新增下载）预计再降到 10-15 ms 级；收益小于一个 1 s tick，且要新增直接依赖，
先记账不动手。

### 3.5 测试（`crates/pi-link/src/subagent_catalog/tests.rs`）

现有 10 条全保留（增量/末条覆盖/半行/坏 JSON/终态配色等），新增 5 条
（`crates/pi-link/src/subagent_catalog/tests.rs`，已全绿 15/15）：

1. `quoted_descriptor_line_does_not_shadow_real_last_entry`：真末条后面追加两行
   「消息里贴的 descriptor JSON」→ 仍取真末条（伪行不制造第二个同 id）。
2. `candidate_depth_covers_three_pseudo_lines`：连贴 3 条伪行 → K=4 候选仍兜住。
3. `matches_naive_scan_on_large_log`：合成 2,700 条 descriptor（90 id × 30 轮）+ 夹杂
   非 descriptor 行 → 与「朴素全解析参考实现」**逐字段相等**（oracle 留在 test 里）。
4. `big_file_without_descriptors_is_skipped`：20,000 行纯文本、无 marker → 0 行、offset 推进。
5. `truncated_file_reports_restarted`：offset 越界 → `restarted == true` 且只出新文件的行。
   （注：append-only 契约下只有「文件变短」可检测；同长重写的旧 offset 会读到行中书，
   这是字节游标的固有限制，会话文件不重写故可接受。）

---

## 4 宿主侧（`crates/app`）

### 4.1 后台读 + 在飞守卫

`crates/app/src/session/subagent_panel/mod.rs` 的 `spawn_poll` 改成两段式：

1. UI 线程：取 `(file, offset)` 快照，若 `read_in_flight` 已在跑则跳过本 tick；
2. `background_executor().spawn(...)` 执行 `read_catalog`（IO + 解析全在后台）；
3. 回灌守卫（`apply_catalog_scan`）：`panel.file` 未变才继续（变了 = 别的会话的陈旧
   快照，直接丢，`in_flight` 归属新会话的读，已在 `adopt` 里清）；同会话则先清
   `read_in_flight`，再要求 `panel.offset == from` 才 merge + 重算按轮投影 + `cx.notify()`
   ——offset 已被另一份回灌推进就丢弃（下一 tick 自愈）。
4. `scan.restarted == true` → 先清 `rows` 再合并。

`read_in_flight: bool` 落在 `SubagentPanel` 上，与 `polling` 同族。

### 4.2 进程内 stash（父↔子来回切不重扫）

`SubagentPanel` 增 `stash: Vec<CatalogStashEntry>`（`{file, offset, rows}`，LRU ≤ 4，按文件路径）：

- 入口统一在 `adopt(file)`（`ensure_subagent_panel` 与 poll 自愈两条路都走它）：
  先 `stash_put()`（`std::mem::take` 取走 rows，不留重复副本；同名先剔、放末尾、超 4 丢最旧），
  再把菜单锚/宽限/`in_flight` 清零——锚属于旧会话的轮卡，留着会错位；

- 会话切走时把当前 `(file, offset, rows)` 存入 stash（不动 offset——文件增长由增量读跟随）；
- 切回来先查 stash：命中 → 恢复 `rows/offset` 后**只读增量**（通常 0 字节 → 一次 metadata）；
- 未命中 → 全扫（44 ms，后台）。

这是「父 ↔ 子观察窗来回点」不再重复 27 MB 扫描的关键；也是 1.3 ③ 的修法。

### 4.3 按轮投影去克隆

**原计划**：`turn_agents: Vec<Vec<CatalogRow>>` 持 `rows` 深拷贝 + 每 tick 深比较，改成
`Vec<Vec<usize>>`（下标）、只在变更时重建。

**实际结论：不做。** 落地时量过：rows ≤ 90 行、str 字段 ~15 个 → 每 tick 重建 ≈ 数十 µs，
与 32 ms 的读盘相比是噪声；而改下标要把 `gutter.rs` 的 `nav_card(rows)`、
`nav/mod.rs` 的 `turn_agents_row(&[CatalogRow])`、菜单渲染、快照 JSON 四处签名一起改，
收益/风险不划算。代码里把归组抽成 `regroup_turn_agents(&rt, cx)`（一处实现），
将来真成瓶颈再改下标只需动这一处。

### 4.4 打开路径零读

`crates/app/src/session/pool/open.rs` 的同步 `read_catalog(&parent, 0)` **删除**，
换成 `Chat::subagent_tab_label(&child, &key, cx)`（`session/pool/open.rs`）：

- 主路径：从 `panel.rows`（或 stash）里按 `session_file` 匹配取 `label → profile → 「子代理」`。
  从导航菜单点进来的场景必然命中（那一行就是面板里的行）。
- 兜底：没命中时先用占位标题 `tr("subagent.tab")` 建 tab，同时扔一个后台任务扫父文件，
  扫完若 tab 还在就改名（`pool.tab_labels.insert`）。
- 匹配逻辑抽成 `SubagentPanel::label_for_child(&Path)`（当前 rows + stash 都查）与
  `subagent_panel::child_tab_label(&CatalogRow)`（label → profile），两处口径不再分叉。

`read_tail_messages` 保留（≤256 KB，~1-2 ms，且是首帧有内容的来源）。

---

## 5 验收

### 5.1 `[perf]` 日志口径（进代码，长期量尺）

`PI_FLASH_PERF=1` 时宿主侧 poll 每完成一次后台读打一行（pi-link 保持纯函数、不碰全局 io）：

```
[perf] subagent_catalog bytes=27241180 us=32160 parsed=90 rows=90 mode=full|incr
```

`us` 在**后台线程内**计时（只算 `read_catalog` 本身，不含排队/回灌）；
`mode=full` 的条件是 `from == 0 || scan.restarted`。稳态（无新字节）每 tick 仍会打一行
`bytes=0 us=<200 mode=incr`——这是「有没有误走全扫」的判据。
线程纪律：`read_catalog` 只允许在 `background_executor` 里调（review 检查点 + 4.1 的守卫）。

### 5.2 验收项（最坏样本 = 27 MB / 37,882 行）

| 项 | 标准 |
|---|---|
| 首次打开子代理会话（导航菜单点行） | UI 不冻结（无 ≥16 ms 主线程 IO）；tab 立即出现，标题同步命中面板缓存 |
| 父 ↔ 子来回切 3 次 | 每次只走增量（`mode=incr` / `idle`），不再出现 `mode=full` |
| 切回父会话 | 导航卡第三行状态点 1 tick（≤1 s）内恢复，且恢复过程不卡 |
| 全量首扫（进程内首次见到该会话） | 后台 ≤ 100 ms（release），期间 UI 可交互 |
| 功能零回归 | 导航第三行点数/配色、hover 菜单行数与字段、运行中徽标、tab 标题、只读观察窗同步 |

### 5.3 测试用例

检索层（pi-link）的 5 条用例见 §3.5，随 `cargo test -p pi-link` 常跑，不需要手工 UI 案例。
宿主侧（后台读 / stash 命中 / 打开零读盘）是时序行为，验收走 §5.2 的 `[perf]` 日志断言：
UI 侧只做「导航第三行点色、hover 菜单、tab 标题、观察窗同步」的功能回归，
用例口头执行即可（用户端验收），`test/044-子代理/` 的既有案例不动。

---

## 6 不做 / 延后

| 项 | 原因 |
|---|---|
| 项 | 原因 |
|---|---|
| ~~**写侧瘦身**（composition 只写首条、stats 节流合并）~~ | **已执行（2026-11-07 用户定夺）**：比瘦身更彻底——逐消息持久化整体删除，落盘收敛为相位变迁/终态。见 §1.1 已解决注记 |
| **落盘索引**（`<PF数据目录>/subagent-index/…`） | stash 已覆盖会话内切换；重启首开仍只是一次 ≤100 ms 后台扫，不值得多一层失效面 |
| **会话级聚合统计**（子代理总数 / 合计 token / 墙钟） | 数据本来就在内存里，作为检索层的派生物另案；本文不引入新存储 |
| **descriptor 轮询间隔调整** | 1 s / 5 s 现行值够用，问题在单次代价不在频率 |

---

## 附录 A 复现方法（本次取证用的临时 bench，不入库）

在 `crates/pi-link/src/subagent_catalog/tests.rs` 末尾临时加一个 `#[ignore]` 测试，
读 `CAT_FILE` 环境变量指定的真实会话文件，跑三件事：现状 `read_catalog`、裸 `fs::read`、
原型实现（字节预筛 + 末条解析），并比对输出等价性：

```bash
CAT_FILE=<真实父会话.jsonl> cargo test -p pi-link --release catalog_perf_real_file -- --ignored --nocapture
CAT_FILE=<同一文件>        cargo test -p pi-link           catalog_perf_real_file -- --ignored --nocapture  # debug 口径
```

本文 §1.2 / §3.4 的数字即由此得出（各跑 3 次取最优）。
