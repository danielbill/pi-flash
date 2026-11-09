# 001-4 PFContext 内部工具化：pfctx_manager

> 前置：[001-0 总体设计](001-0-PFContext总体设计.md) / [001-1 settings](001-1-settings正式化.md)。测试：`test/001-PFContext/pfctx-manager/case.md`。

## 1 目标

agent 在会话内像用户在设置界面一样读写 PF 设置——**同一条执行链**（同函数、
同校验、同副作用、同持久化），实现管家热管理 Pi-Flash 地基。

## 2 架构

```
agent 调工具            传输                          执行（宿主）
──────────────────  ──────────────────────────  ─────────────────────────────────
pfctx_manager      pi.appendEntry(              120ms 泵 drain（Chat 上下文）
(extensions/         "pfctx-manager-op",        → 逐 op 调设置页同款函数：
 pfctx-manager)      自定义条目，entry_appended     persist_theme+sync_gpui_tokens /
                     实时推宿主）                   save_font+sync / set_lang /
                                                 set_subagent_capacity（双写
                                                 config.json）/ apply_subagent_switch
                                                 （重绑）/ save_send_mode …
                                                 → hub.set → 落盘+投影（001-1/001-2）
```

- **工具面**：薄 pi 扩展（agent 拿工具的唯一方式；工具清单是 spawn 参数，
  RPC 无运行时切换——041 §6）。
- **传输**：自定义条目通道（subagent-settled 同款），entry_appended 实时推宿主。
- **执行**：op 队列（`services/pfctx_ops.rs`，静态 Mutex<VecDeque>，pf-svc
  同款先例）→ `startup/pumps.rs` 120ms 泵在 Chat 上下文逐 op 分发。
- **读**：query 直接读 `app-settings.json` 落盘态（与宿主内存合并窗口 ≤50ms）。
- **规则**：外部直改文件只对纯快照项等效；带副作用项必须走本执行链。

## 3 工具面（extensions/pfctx-manager）

- `query`：读 `<PI_FLASH_DIR>/app-settings.json`（落盘态）+ 全部参数目录
  （名称/含义/可设值）+ 日志路径。
- `set`：`[{setting: "subagents.capacity", value: 8}]` 点路径；扩展侧目录校验
  （键/类型/枚举/范围）——**任一非法整批拒绝**（未做任何修改）；合法 →
  `appendEntry` 投递 → 返回「已受理」。异步执行，query 确认（与「UI 无回执」对齐）。
- pi 工具结果契约：`content` 必须是块数组 `[{type:"text",text}]`。
- 恒拒项：`subagents.max_depth`（恒 1）、`appearance.icon_theme`（无运行时消费者）。

## 4 参数目录（v1，与宿主 setter 一一对应；双侧手工同步）

| 点路径 | 含义 | 可设值 | 宿主执行 |
|---|---|---|---|
| `subagents.capacity` | 子代理并发上限 | 2..10 | `set_subagent_capacity`（双写 agents/config.json） |
| `subagents.enabled` | 子代理模块开关 | bool | `set_subagent_module_switch` + `apply_subagent_switch`（重绑） |
| `chat.send_mode` | 运行中发送方式 | "queue"\|"steer" | `save_send_mode` |
| `chat.show_thinking` / `git_markers` / `md_live_preview` | 界面项 | bool | save_app_settings diff 链 |
| `chat.content_width` | 会话内容列宽 | 560..1200 | 同上（validate 钳） |
| `sessions.startup_restore` / `load_window_days` / `display_count` / `auto_collapse_hours` / `auto_clean_days` | 会话项 | 见工具 query | 同上（auto_clean_days=启动自动清理会话） |
| `index.at_index_ttl_secs` / `at_index_max_projects` | @ 索引 | 1..3600 / 1..100 | 同上 |
| `models.default_model` | 默认模型 | "provider/model" \| null | `set_default_model_pref` |
| `appearance.theme` | 主题 id | 内置集 | `persist_theme` + `sync_gpui_tokens` |
| `appearance.lang` | 语言 | 0/1/2 | `set_lang` + `save_lang_pref` |
| `appearance.session_font` / `panel_font` / `markdown_font` | 字体 | {family, size 10..30} | `save_font`（含 sync_ui_scale） |
| `shell.autohide_icons` / `sound` | 界面/提示音 | bool | diff 链 / `save_sound_pref`+broadcast+play |
| `updates.auto_update` | 自动更新 | bool | diff 链 |
| `agent.self_debug` | 本工具总闸 | bool | hub set + 重绑 |

**不在目录**：appearance.icon_theme（无消费者）、dev_flags（boot-only）、pi 的
文件（001-0 §6）。

## 5 开关（设置-更多【自我调试】）

- 存储 = hub `agent.self_debug`（section `agent`，默认 **on**）。
- on/off = spawn 门控（`client.rs` `mount_pfctx_manager`，`--tools` allowlist
  同步点名）+ 切换时全池 pending + 空闲会话立即重绑（仿 `apply_subagent_switch`）
  ——真注入/真移除；运行中会话推迟到下轮空闲。
- read-only / chat-only 档不注入（allowlist 口径与 `agent` 工具一致）。

## 6 观测

`<PF数据目录>/pfctx.log`：`[agent]`（宿主执行 agent op，含 session key 与生效
值）/ `[write]`（落盘）/ `[reload]`（外部变更差值）/ `[watch]` / `[export]` /
`[problem]`。`PI_FLASH_DIR` env 交子进程（001-2 §5 路径类白名单）。

## 7 Limitations

- query 为落盘态（合并窗口 ≤50ms + dirty 窗口）；set 异步受理（agent 用 query
  确认）。
- 目录与宿主 setter 手工同步（扩展 catalog + 宿主分发器双侧）；漂移表现为目录外
  拒绝（fail-safe）。
- 工具清单变更需 rebind（041 §6 架构约束）。
