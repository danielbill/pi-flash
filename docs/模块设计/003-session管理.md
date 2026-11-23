# Session 管理：最近活动会话清单 + 后台索引

管理最近活动会话清单，让启动只加载最近 N 个 session（非 project），达到闪电打开。
摘要索引负责「每个会话文件是谁」，活动清单负责「哪些最近活跃」，两层配合。

模块代码：`crates/pi-link/src/sessions/`（摘要索引 + 扫描器）、`crates/pi-link/src/recents.rs`（活动清单）、`crates/app/src/startup/restore.rs`（启动装载）

实现位置：
- 摘要索引 + 扫描器：`crates/pi-link/src/sessions/`（指纹 `(mtime, size)`，冷启动全量有界扫描：256K 前缀 + 64K 尾 + 流式计数；热启动零文件读取）
- 活动清单：`crates/pi-link/src/recents.rs`（容量 `RECENTS_CAPACITY` = 500，30s 对账轮询）
- 启动加载路径：`crates/app/src/startup/restore.rs`（当前项目 list_for_cwd(100) → 窗口内跨项目 `recent_load_paths(days)` → 尾部预载）

## 设计目标

- pf 定位给人用，机制按 2 年重度使用量级设计（数千至上万会话文件）。
- 启动不做全盘枚举：读一个几 KB 的清单 JSON + stat 前 N 个文件 + 查摘要索引。
- 首次安装到已有会话的机器：自动全量建索引，无需用户干预。

## 两层存储

| 层 | 内容 | 落点 |
|---|---|---|
| 摘要索引 | 每个会话文件是谁：id / cwd / preview / name / 消息数，指纹 `(mtime, size)` 失效 | `~/.pi-flash/session-index.json` |
| 活动清单 | top 500 会话的 `(path, last_active)`，按活动时间排序 | `~/.pi-flash/session-recents.json` |

清单条目只存 path + last_active；preview / name / 条数渲染时查摘要索引，
不冗余进清单（两处存摘要必然漂移，索引已有指纹保活）。路径身份键在
Windows 下大小写不敏感（pi/Node 回传的盘符/group 目录大小写可能与
Rust 枚举不同，同一物理文件；严格相等会把会话重复渲染）。

## 加载窗口与分页

- **加载单位是时间窗口，不是数量**：设置-其他「加载时间窗口」
  7 天 / 14 天 / 30 天 档位（默认 7 天，key `load_window_days`）。
  加载集 = 清单中 `last_active` 在窗口内的会话；清单容量 500 是硬上限。
- **窗口即归档**：pi 无归档机制（append-only 永久保留），会话滑出窗口 =
  显示层自动"归档"，数据不删；窗口外的会话用搜索找回，打开即回清单顶。
- **每项目分页**：psp 列表每组先显示一页会话标签（页大小 = 设置-其他
  「会话显示数」，3–10 档、默认 10），还有更多则底部显示「显示更多」，
  点击续一页（Flat 模式用全局键同样分页）。分页状态跨刷新保持
  （Chat.psp_shown，key = 组 ws_key）。
- **尾部预载**固定 10 条（`TAIL_PRELOAD` 内部常量，非设置项）。

## 活动语义

活动 = max(最近被打开， 最近有更新)。

- 最近被打开：pf 内打开/新建会话的动作。
- 最近有更新：会话文件 mtime 变化（pi 子进程追加、pi-web / 终端 cli 外部写入）。

清单本身就是打开记录，不另做"最近打开"表：打开动作同步把条目提到清单顶。

## 清单的三条写入路径

1. 用户打开 / 新建会话 → 同步提顶，last_active = now（最近被打开）。
2. 活跃会话收到新消息 → 消息回调里就地刷新（不等 30 秒，体验优先）。
3. 轮询发现外部变更（pi-web、终端 cli 写的文件，或 pf 空闲会话被外部追加）
   → 刷新 last_active = mtime + 补扫摘要。

## 后台轮询（30 秒）

- 形态：app 内常驻 async task（`spawn_recents_poll`），**不是独立后台进程**；
  共享 Scanner 内存态。
- 每轮动作：全枚举 group 目录 + stat（几万文件几十 ms 级、后台线程执行）
  → 指纹比对（命中零读取；
  扫描窗口 550 = 容量 500 + 余量，给外部新文件留挤进清单的空间）
  → 新增 / 变更文件补扫摘要 → 维护清单（仅变更时写盘）。
- **必须排除本 app 活跃 runtime 正在写的会话文件**（`poll_recent_sessions`
  的 `exclude` 参数）：其提顶由上面写入路径 1 / 2 事件驱动，不需要轮询发现。

## 启动路径

两种情形统一为一个入口（`startup::spawn_boot_restore`），不做分支：

- **首次启动（索引 / 清单缺失）**：`boot()` 显式种子（`ensure_seeded`，
  `list_sessions(500)` 按 mtime 落盘）→ 清单随后正常服务。干净机器是退化
  情形：全量扫到 0 个文件，秒完。
- **日常启动**：读清单 → 窗口过滤（`recent_load_paths(days)`）→ 只 stat 这些
  → 查摘要索引 → 渲染 psp 列表 + 尾部预载（预载取活动序前 10）。全盘枚举不挡首屏。

settings 落点：设置-其他「加载时间窗口」档位 7/14/30 天
（key `load_window_days`，默认 7）。psp 数据源 = `recent_load_paths(days)` +
`sessions_for_paths`（纯 stat + 指纹索引查）。

## 运行中边界（明确的非目标）

轮询是**数据层维护**，不驱动运行中 UI：

- 轮询发现外部变更后只更新清单/索引并重绘（时间戳类文案刷新），
  **不重建 psp 列表数据**——外部新会话（终端 cli / pi-web 新建）不会
  实时蹦进列表，进入列表依赖用户操作触发的刷新（打开会话、切换项目等
  走 `refresh_sessions` 补全当前项目；跨项目组数据留在下次启动）。
- **刷新路径同样遵守窗口**：`refresh_sessions` 对当前项目
  只取 mtime 在窗口内的会话 + 当前激活会话（保证改名/打开的旧会话始终
  可见）；显示量交给 psp 分页。

## 收敛不变式

- 枚举 + 截断 + 指纹服务收敛为一个带参数的入口：启动路径与 30 秒轮询共用，
  不各写一份（`Scanner::list_excluding`）。
- 真相源原则：磁盘为真相源，清单是加速缓存；漏会话是静默错误，多扫只是慢一点。

## 秒开深度优化

打开任何会话 ≤1s（用户口径：对话文件基本不超 20MB）：
两处 O(会话大小) 的**主线程**磁盘操作全部后台化（`session/runtime/nav.rs`）。

- **磁盘计数**（`count_message_entries`，主线程 `fs::read` 整个会话文件 +
  扫描）：open 路径（`pool.rs` / `psp_actions.rs` 改名临时进程 /
  `fork.rs` follow 换绑 / `chat.rs` 启动恢复）四处统一改
  `SessionRuntime::load_disk_msg_count`——后台算完回灌
  `disk_msg_count`。计数晚到的补偿：get_messages 到达时计数还是 0，
  修复守卫空转过一轮；计数落地后若快照比磁盘短，补一次后台 leaf 修复。
- **leaf 全文重读**（`read_leaf_messages`，逐行 JSON 解析，20MB 上限下
  ~300ms+）：get_messages 的 leaf 修复（`spawn_leaf_repair`，落败分支把
  RPC 权威投影延后到读落地再执行，同守卫下无陈旧回退）与 3s 外部追加
  观察（`check_external_append`，在飞标记防重复调度）都改后台线程；
  回灌守卫 =（文件路径 + epoch + 空闲态）——重绑/重启/读的这段时间会话
  动了都作废。
- 对话首屏不受影响（tail 预渲染）；打开→完整（resumed）总时长受 node
  冷启支配（非主线程）。

已知限制：get_messages 权威投影（`project_rpc_messages`，大快照 ingest）与
跨项目 remount（git status/numstat 子进程 + 项目树装载）仍在主线程，
大快照/大项目切换有可感知卡顿。
