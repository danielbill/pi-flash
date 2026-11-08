# 001-0 PFContext 总体设计

> 状态：设计定稿（2026-11-08 用户定名 `pf-context`，`pf-settings` 为其子集）。实现切片见 [001-3-切片与验收](001-3-切片与验收.md)。
> 本文是本目录的**唯一事实源**：三子集契约、分层与优先级、失效语义、与 pi 文件的边界。

## 1 为什么需要它（不是抽象洁癖，是已发生的 bug）

2026-11-08 实测：设置页把子代理并发改成 10，实际仍在第 6 个委派起
`CAPACITY_REACHED ... (active child limit: 5)`。同一个值同时存在于四处：

| 位置 | 值 | 谁写 | 失效方式 |
|---|---|---|---|
| `~/.pi-flash/app-settings.json` `subagent_capacity` | 10 | 设置页 | 用户以为它生效 |
| `~/.pi-flash/agents/config.json` `capacity.default` | 5 | **无人写**（只能手编） | 扩展每次重读，直接压过前两处 |
| spawn 时 env `PI_SUBAGENTS_CAPACITY` | 10 | 进程启动快照 | 改了要重启，且仍被文件压过 |
| clamp 规则 | app `2..10` / 扩展 `1..MAX` / 文档 `min..max` | 三份各写一遍 | 越界行为不一致 |

同类结构性问题：

- `app-settings.json` 的写路径从结构体**整份重建** JSON（`crates/app/src/services/workspace/app_settings.rs:192` `save_app_settings`），手工加的未知键会在下一次保存时被静默抹掉；
- 25 个平铺键挤在一个 `Option` 字段堆里（`app_settings.rs:120` `app_settings()`），无 `version`、无 section 边界；
- `dev-flags.json` 用 `OnceLock` 快照（`app_settings.rs:385-386`），进程内永不刷新，改完必须重启；
- PF 自有 json 约 20 份（app-settings / workspace / projects / plugins / session-ext / session-index / session-recents / hidden-sessions / custom-provider / catalog-cache / model-price-cache / openrouter-cache / registry-cache / ext-tokens / pi-ext-market-cache / pi-native-kinds-cache / update-state / context-dump / agents/config.json …），每份自带解析、缺省、写函数，没有统一的读盘与失效模型。

结论：**成本不在读盘，而在「同一个值有几个 owner」与「改了之后谁在什么时候看见」。**
读盘本身很便宜——投影文件（约 200B）读一次 `readFileSync + JSON.parse` 实测平均 90.5µs（5000 次，本机），相对一次委派的秒级耗时是噪声。

## 2 三个子集（契约）

| 子集 | 内容 | 持久化 | 跨进程 | 失败策略 |
|---|---|---|---|---|
| `settings` | 用户意图：开关 / 上限 / 默认模型 / 项目参数 / 外观 | `app-settings.json`（唯一 owner，原子写） | 投影 `ext-context.json` | 坏档保持上一份好快照，不静默改行为 |
| `state` | 运行期状态：当前会话 / 面板 / 进程 / 在跑的子代理 | 内存为主，至多写缓存 | 不投影 | 丢了大不了重建 |
| `domain` | 域数据：任务定义与运行、管家待办与记忆 | SQLite WAL（后补） | 不投影 | 事务失败即回滚 |

一句话：**settings 是人许的愿，state 是此刻的样子，domain 是发生过的事。**
三者混在一起，就是下一个 owner 冲突。

## 3 crate 结构与命名

```
crates/pf-context/src/
  lib.rs              Hub：一个 Arc<Inner>，三子集共享同一生命周期
  settings/
    mod.rs  section.rs  source.rs  doc.rs  export.rs  watch.rs
    sections/         subagents.rs appearance.rs chat.rs sessions.rs …
  state/              v1 只建目录，S4 再填
  domain/             v1 只留 trait（Source 的 DB 实现预留给任务/管家）
```

- **不进 `pi-link`**：pi-link 的定位是钉版 pi RPC 协议层；pf-context 是产品基础设施，未来任务/管家都要依赖它，不该拽上 RPC 层。
- 门面类型 `Hub`，方法 `hub.settings()` / `hub.state()` / `hub.domain()`；子集类型 `SettingsStore` / `StateStore` / `DomainStore`。
- **禁止任何公开类型叫 `Context`**：`gpui::Context` 在本仓无处不在（`cx: &mut Context<Chat>`），`use pf_context::Context` 会与之天天打架。需要局部别名时用 `PfHub`。

## 4 分层与优先级（固定，禁止临时加层）

| 层 | 来源 | 可写 | 用途 |
|---|---|---|---|
| L0 默认 | 代码 `Default` | — | 恒有值 |
| L1 全局 | `<PF数据目录>/app-settings.json` 的 section | 是 | 用户设置 |
| L2 项目 | 同文档 `projects.<id>.{…}` 段 | 是 | 任务/管家的 per-project 参数 |
| L3 会话 | 内存（可选落会话元数据） | 是 | 单次对话临时覆盖 |
| L4 env | `PI_FLASH_*` | **只读** | **仅 bootstrap 路径**，永不传策略值 |

三条规则：

1. 读取 = L3 > L2 > L1 > L0 合并后的 typed snapshot，`get()` 零 I/O。
2. 每个 key 只有一个 primary source；其它层只做读覆盖，**永不回写**。
3. 同一个 key 不允许既在文件又在数据库——DB 若上，只装**永不进文件**的域数据，或整段声明归 DB。

## 5 失效语义（「统一调度」的实质）

所有来源汇入同一条通知通道（`watch`，订阅者拿到 `Arc<Snapshot>`）：

1. **本进程写**：内存先换（UI 零延迟）→ 单写者线程 50ms coalesce → 原子写（tmp+rename）→ 广播；退出路径有 `flush()`。
2. **外部手改**：`notify` 非递归监听 PF 目录 + 内容哈希，变化即重载并广播；自己写的回声用「预期哈希窗口」吃掉，避免回环。
3. **多实例**：文件是仲裁者，last-writer-wins；仅当检测到对方写入、内容不同、且本进程有未落盘改动时，状态栏提示一次（不弹窗）。
4. **坏档**：保持上一份好快照并记录 `problem`（学扩展 `syncModuleConfig`：坏文件不许静默改行为）。
5. **写失败**：内存保留（用户意图不丢）+ 退避重试 + 可见告警，禁止静默吞。

## 6 与 pi 文件的边界（严禁并入）

| 文件 | 归属 | PF 权限 |
|---|---|---|
| `~/.pi/agent/settings.json` | pi | 只读（`crates/pi-link/src/config.rs:25`） |
| `~/.pi/agent/models.json` / `auth.json` | pi，042-1 起 PF 可写 | 整份草稿保存、不可读拒写、proper-lockfile 同款锁、apikey 只进钥匙库 |
| `~/.pi/agent/mcp.json` | pi（043 由 PF 编辑） | 走既定 mcp 模块，不经 context |
| `~/.pi-flash/app-settings.json` | **PF** | context 全权 |
| `~/.pi-flash/ext-context.json` | **PF 写 / 扩展读** | 见 001-2 |
| `~/.pi-flash/agents/config.json` | legacy fallback（S3 投影上线后，双写一版过渡） | 见 001-2 §8 |

**PF 的配置文件永远不叫 `settings.json`**——与 pi 的文件同名会让文档、issue、排查互相污染（2026-11-08 用户否决定名）。

## 7 非目标

- 不做通用 DI / 服务定位器；只有三个具名子集。
- 不把缓存搬进 context：`registry-cache` / `model-price-cache` / `session-index` / `catalog-cache` 等可重建、自校验，各归各的模块。
- 不做跨进程数据库共享；扩展只看到投影。
- 不在 context 里放密钥（051 / 042-1 边界）。

## 8 文档地图

- [001-0-PFContext总体设计](001-0-PFContext总体设计.md) —— 本文
- [001-1-settings正式化](001-1-settings正式化.md) —— `app-settings.json` v2：section 化、读写模型、迁移、Section 契约
- [001-2-跨进程投影](001-2-跨进程投影.md) —— `ext-context.json`：形态、扩展读取契约、env 白名单、安全与观测
- [001-3-切片与验收](001-3-切片与验收.md) —— S1–S5、验收指标、测试清单、风险与回滚
