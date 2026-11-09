# 001-0 PFContext 总体设计

> 本文是本目录的**唯一事实源**：三子集契约、分层与优先级、失效语义、与 pi 文件的边界。
> 实现切片与验收：[001-3-切片与验收](001-3-切片与验收.md)。

## 1 设计目标

PF 全部配置与运行期上下文统一收口：**每个值只有一个 owner**，读取 = 合并快照零 I/O，
变更经统一失效通道广播；跨进程只经文件投影，密钥与 pi 的文件不进本模块。

## 2 三个子集（契约）

| 子集 | 内容 | 持久化 | 跨进程 | 失败策略 |
|---|---|---|---|---|
| `settings` | 用户意图：开关 / 上限 / 默认模型 / 项目参数 / 外观 | `app-settings.json`（唯一 owner，原子写） | 投影 `ext-context.json` | 坏档保持上一份好快照，不静默改行为 |
| `state` | 运行期状态：当前会话 / 面板 / 进程 / 在跑的子代理 | 内存为主，至多写缓存 | 不投影 | 丢了大不了重建 |
| `domain` | 域数据：任务定义与运行、管家待办与记忆 | SQLite WAL（`context.db`） | 不投影 | 事务失败即回滚 |

settings 是人许的愿，state 是此刻的样子，domain 是发生过的事；三者不混。

## 3 crate 结构与命名

```
crates/pf-context/src/
  lib.rs              Hub 门面：三子集共享同一生命周期
  log.rs              pfctx.log 观测日志（>2MB 滚动 .old；纯内存态零写）
  settings/
    mod.rs  section.rs  source.rs  doc.rs  export.rs  watch.rs
    sections/         agent.rs appearance.rs chat.rs dev_flags.rs index.rs
                      models.rs sessions.rs shell.rs subagents.rs
  state/              StateStore：Arc 快照读 + update 写
  domain/             DbSource（rusqlite WAL）+ TaskStore（046 种子）+ Source trait
```

- pf-context 不依赖 pi-link 的 RPC 模块（仅 paths / config::write_json / dev_flags）。
- 门面类型 `Hub`，字段 `hub.settings` / `hub.state` / `hub.domain`；子集类型
  `SettingsStore` / `StateStore` / `DomainStore`。
- **禁止任何公开类型叫 `Context`**：与 `gpui::Context` 冲突；局部别名用 `PfHub`。

## 4 分层与优先级（固定，禁止临时加层）

| 层 | 来源 | 可写 | 用途 |
|---|---|---|---|
| L0 默认 | 代码 `Default` | — | 恒有值 |
| L1 全局 | `<PF数据目录>/app-settings.json` 的 section | 是 | 用户设置 |
| L2 项目 | 同文档 `projects.<id>.{…}` 段 | 是 | 任务/管家的 per-project 参数 |
| L3 会话 | 内存（可选落会话元数据） | 是 | 单次对话临时覆盖 |
| L4 env | `PI_FLASH_*` | **只读** | **仅 bootstrap 路径**，永不传策略值 |

规则：

1. 读取 = L3 > L2 > L1 > L0 合并后的 typed snapshot，`get()` 零 I/O。
2. 每个 key 只有一个 primary source；其它层只做读覆盖，**永不回写**。
3. 同一个 key 不允许既在文件又在数据库——DB 只装**永不进文件**的域数据。

## 5 失效语义

所有来源汇入同一条通知通道（`watch`，订阅者拿到 `Arc<Snapshot>`）：

1. **本进程写**：内存先换（UI 零延迟）→ 单写者线程 50ms coalesce → 原子写
   （tmp+rename）→ 广播；退出路径有 `flush()`。
2. **外部手改**：`notify` 非递归监听 PF 目录 + 内容哈希，变化即重载并广播；
   自写回声用「预期哈希窗口」吃掉，避免回环。
3. **多实例**：文件是仲裁者，last-writer-wins；检测到对方写入且本进程有未落盘
   改动时提示一次（不弹窗）。
4. **坏档**：整份非法保持上一份好快照；单 section 非法该段回退 `Default`；
   均记 `problem`，不许静默改行为。
5. **写失败**：内存保留（用户意图不丢）+ 退避重试 + 可见告警，禁止静默吞。
6. **观测留痕**：启动 / 实际写盘（段名+规范值）/ 外部重载差值 / 投影落盘 /
   problem / perf(load·flush µs) / domain 开库 / agent 设置 op 全部追加进
   `<PF数据目录>/pfctx.log`（>2MB 滚动 `pfctx.log.old`；纯内存态零写）。

## 6 与 pi 文件的边界（严禁并入）

| 文件 | 归属 | PF 权限 |
|---|---|---|
| `~/.pi/agent/settings.json` | pi | 只读 |
| `~/.pi/agent/models.json` / `auth.json` | pi（042-1 起 PF 可写） | 整份草稿保存、不可读拒写、proper-lockfile 同款锁、apikey 只进钥匙库 |
| `~/.pi/agent/mcp.json` | pi（043 由 PF 编辑） | 走既定 mcp 模块，不经 context |
| `~/.pi-flash/app-settings.json` | **PF** | context 全权 |
| `~/.pi-flash/ext-context.json` | **PF 写 / 扩展读** | 见 001-2 |
| `~/.pi-flash/agents/config.json` | legacy fallback（投影上线后双写一版过渡） | 见 001-2 §8 |
| `~/.pi-flash/context.db` | **PF**（domain 专用） | 见 001-0 §2 |

**PF 的配置文件永远不叫 `settings.json`**——`settings.json` 是 pi 的文件，
同名会让文档、issue、排查互相污染。

## 7 非目标

- 不做通用 DI / 服务定位器；只有三个具名子集。
- 不把缓存搬进 context：`registry-cache` / `model-price-cache` / `session-index` /
  `catalog-cache` 等可重建、自校验，各归各的模块。
- 不做跨进程数据库共享；扩展只看到投影。
- 不在 context 里放密钥（051 / 042-1 边界）。

## 8 文档地图

- [001-0-PFContext总体设计](001-0-PFContext总体设计.md) —— 本文
- [001-1-settings正式化](001-1-settings正式化.md) —— `app-settings.json` v2：section 化、读写模型、25 键迁移、Section 契约
- [001-2-跨进程投影](001-2-跨进程投影.md) —— `ext-context.json`：形态、扩展读取契约、env 白名单、安全与观测
- [001-3-切片与验收](001-3-切片与验收.md) —— 验收指标、测试清单、风险与回滚
- [001-4-PFContext内部工具化](001-4-PFContext内部工具化.md) —— pfctx_manager：agent 设置面、执行链、参数目录
