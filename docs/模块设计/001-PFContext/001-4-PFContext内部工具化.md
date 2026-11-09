# 001-4 PFContext 内部工具化：pfctx_manager

> 状态：S8 实现（beads pi-flash-26y4）。前置：[001-0 总体设计](001-0-PFContext总体设计.md) / [001-1 settings](001-1-settings正式化.md) / [001-2 投影](001-2-跨进程投影.md)。

## 1 目标

让 agent 动态调试 Pi-Flash，实现管家热管理 Pi-Flash 地基：agent 在会话里像用户在设置界面一样读写 PF 设置——**同一条执行链**（同函数、同校验、同副作用、同持久化）。

需求（2026-11-10）：
1. 内部工具 `pfctx-manager`，随 app 启动注册入工具列表；
2. 设置-更多【自我调试】switch on/off 默认 on（注释：允许Agent动态修改Pi-Flash设置）；on 注入工具列表，off 移除；
3. 提供参数设置方法 + PF 全部设置查询方法；
4. 使用说明告知 agent 所有参数名、含义、设置值，并告知可观察 log。

## 2 架构（三段，各归其位）

```
agent 调工具            传输                          执行（宿主）
──────────────────  ──────────────────────────  ─────────────────────────────────
pfctx_manager      pi.appendEntry(              120ms 泵 drain（Chat 上下文）
(extensions/         "pfctx-manager-op",        → 逐 op 调设置页同款函数：
 pfctx-manager)      自定义条目，subagent-         persist_theme+sync_gpui_tokens /
                     settled 同款现成通道，        save_font+sync / set_lang /
                     entry_appended 实时推)        set_subagent_capacity（双写
                                                 config.json）/ apply_subagent_switch
                                                 （重绑）/ save_send_mode …
                                                 → hub.set → 落盘+投影（001-1/001-2）
```

- **工具面**：薄 pi 扩展（agent 拿工具的唯一方式；工具清单是 spawn 参数，RPC 无运行时切换——041 §6）。
- **执行**：宿主 `services/pfctx_ops.rs` 分发器，op 队列（静态 Mutex<VecDeque>，pf-svc `SvcShared.queues` 同款先例），120ms 泵消费（`startup/pumps.rs`）。
- **为什么经条目不走文件**：文件直写对副作用设置（theme/lang/字体/提示音）半生效（hub 变了、重算没跑）；同链执行让「效果同设置界面」对全部目录项成立。外部直改文件的通路依然有效（watcher，见 001-1 §7 / S7.1 `[reload]` 差值留痕）。

## 3 工具面（extensions/pfctx-manager）

- `query`：读 `<PI_FLASH_DIR>/app-settings.json`（落盘态，与宿主内存合并窗口 ≤50ms）+ 全部参数目录（名称/含义/可设值）+ 日志路径。
- `set`：`[{setting: "subagents.capacity", value: 8}]` 点路径；扩展侧目录校验（键/类型/枚举/范围）——**任一非法整批拒绝**（未做任何修改）；合法 → `appendEntry` 投递 → 返回「已受理」。异步执行，query 确认。
- 恒拒项：`subagents.max_depth`（恒 1 禁套娃）、`appearance.icon_theme`（无运行时消费者）。

## 4 目录（v1，与宿主 setter 一一对应；手工同步，漂移风险见 §7）

| 点路径 | 含义 | 可设值 | 宿主执行 |
|---|---|---|---|
| `subagents.capacity` | 子代理并发上限 | 2..10 | `set_subagent_capacity`（双写 agents/config.json） |
| `subagents.enabled` | 子代理模块开关 | bool | `set_subagent_module_switch` + `apply_subagent_switch`（重绑） |
| `chat.send_mode` | 运行中发送方式 | "queue"\|"steer" | `save_send_mode` |
| `chat.show_thinking` / `git_markers` / `md_live_preview` | 界面项 | bool | save_app_settings diff 链 |
| `chat.content_width` | 会话内容列宽 | 560..1200 | 同上（validate 钳） |
| `sessions.startup_restore` / `load_window_days` / `display_count` / `auto_collapse_hours` / `auto_clean_days` | 会话项 | 见目录 | 同上（auto_clean_days=启动自动清理会话，慎改） |
| `index.at_index_ttl_secs` / `at_index_max_projects` | @ 索引 | 1..3600 / 1..100 | 同上 |
| `models.default_model` | 默认模型 | "provider/model" \| null | `set_default_model_pref` |
| `appearance.theme` | 主题 id | 内置集 | `persist_theme` + `sync_gpui_tokens` |
| `appearance.lang` | 语言 | 0/1/2 | `set_lang` + `save_lang_pref` |
| `appearance.session_font` / `panel_font` / `markdown_font` | 字体 | {family, size 10..30} | `save_font`（含 sync_ui_scale） |
| `shell.autohide_icons` / `sound` | 界面/提示音 | bool | diff 链 / `save_sound_pref`+broadcast+play |
| `updates.auto_update` | 自动更新 | bool | diff 链 |
| `agent.self_debug` | 本工具总闸 | bool | hub set + 重绑 |

**不在目录**：appearance.icon_theme（无消费者）、dev_flags（boot-only）、pi 的文件（001-0 §6）。

## 5 开关（设置-更多【自我调试】）

- 存储 = hub `agent.self_debug`（新 section，默认 **on**）。
- on/off = spawn 门控（`client.rs` `mount_pfctx_manager`）+ **切换时全池 pending + 空闲会话立即重绑**（仿 `apply_subagent_switch`）——真注入/真移除，对活会话即生效；运行中会话推迟到下轮空闲。

## 6 观测

`<PF数据目录>/pfctx.log`：`[agent]`（宿主执行 agent op，含 session key 与生效值）/ `[write]`（落盘）/ `[reload]`（外部变更差值，S7.1）/ `[watch]` / `[export]` / `[problem]`。`PI_FLASH_DIR` env 已交子进程（001-2 §5 路径类白名单）。

## 7 Limitations

- query 为落盘态（合并窗口 ≤50ms + dirty 窗口）；set 异步受理（agent 用 query 确认，与「UI 无回执」对齐）。
- 目录与宿主 setter 手工同步（双侧：扩展 catalog + 宿主分发器）——新增设置项 = 两处各一行；漂移表现为目录外拒绝（fail-safe）。
- 工具清单变更需 rebind（041 §6 架构约束）。
- read-only / chat-only 档不注入（allowlist 点名只在 default/full；custom 档无 --tools 全注册）。
