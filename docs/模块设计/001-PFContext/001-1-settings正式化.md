# 001-1 settings 子集：app-settings.json 正式化（v2）

> 前置：[001-0-PFContext总体设计](001-0-PFContext总体设计.md)。投影见 [001-2](001-2-跨进程投影.md)，agent 设置面见 [001-4](001-4-PFContext内部工具化.md)。

## 0 文件名

PF 的设置文档为 `<PF数据目录>/app-settings.json`。**永不叫 `settings.json`**——
那是 pi 的文件（`~/.pi/agent/settings.json`，PF 只读）。

## 1 文档形态（v2）

```json
{
  "version": 2,
  "appearance": {
    "theme": "dark",
    "icon_theme": "default",
    "session_font": { "family": "…", "size": 14 },
    "panel_font": { "family": "…", "size": 13 },
    "markdown_font": { "family": "…", "size": 15 },
    "lang": "zh-CN"
  },
  "chat": {
    "content_width": 880,
    "md_live_preview": true,
    "send_mode": "queue",
    "show_thinking": true,
    "git_markers": true
  },
  "sessions": {
    "load_window_days": 30,
    "startup_restore": true,
    "display_count": 50,
    "auto_collapse_hours": 3,
    "auto_clean_days": 30
  },
  "subagents": { "capacity": 10, "max_depth": 1, "enabled": true }
}
```

> **dev_flags 不在此文档**：开发旗标是 boot-only 只读段，源 =
> `<PF数据目录>/dev-flags.json` + `PI_FLASH_<键大写>` env（env 最高优先），
> open 时一次性灌入 hub，**改了需重启**；永不进 app-settings.json（写通道按
> 注册表 `in_file` 过滤，整份升级也不落位）。

- section 的**键名即 `Section::KEY`**，字段名即结构体字段（`serde(rename_all = "snake_case")`）。
- 未在本文件登记的键/section 一律原样保留，PF 不认领就不碰。
- `version` 缺失 = v1（平铺），首次写入升为 v2 并迁移（§3）。

## 2 读写模型

**读**

- 进程内 `SettingsStore` 只做一次 `load()`：读盘 → 逐 section 解析 → 合成
  `Snapshot`（`Arc`）；`get::<T>()` O(1)、零 I/O，跨 UI 帧安全。
- 坏 section 不拖垮整份：该段回退 `Default` 并记 `problem`（`Snapshot::problems`）。

**写**（`hub.settings().set(|s| s.subagents.capacity = 10)`）

1. 内存立即换（UI 下一帧生效）；
2. 标记 dirty → 单写者线程 50ms coalesce；
3. 落盘 = **读-改-写**：读当前文件 Map → 只替换本 section（section 内未知字段
   透传）→ 原子写（tmp+rename，`pi_link::config::write_json`）；
4. 内容与盘上一致则跳过写。
5. 未知键/未知 section **逐字保留**（单测锁定：写入前后未知键不变）。
6. `flush()`：退出、切项目、切会话前同步落盘；`save_now()` 供迁移与测试；
   `set_preview()` 只换内存不标脏（拖拽预览），`save_now()` 时转 dirty。

## 3 v1 平铺键 → v2 section 映射（25 键）

| v2 section | v1 键 | 备注 |
|---|---|---|
| `appearance` | theme, icon_theme, session_font, panel_font, markdown_font, lang | 字体是对象，保留原结构 |
| `chat` | chat_content_width, md_live_preview, send_mode, show_thinking, git_markers | |
| `sessions` | load_window_days, startup_restore, session_display_count, auto_collapse_hours, auto_clean_days | 键名去冗余前缀：`display_count` |
| `index` | at_index_ttl_secs, at_index_max_projects | @ 文件索引参数 |
| `models` | default_provider, default_model | |
| `updates` | auto_update | |
| `shell` | autohide_icons, sound | |
| `subagents` | subagent_capacity → `capacity`、subagent_enabled → `enabled`（新增 `max_depth`，恒 1） | 见 §4 |
| `agent` | （无 v1 键，新增段） | `self_debug`，见 001-4 |

读迁移：v2 section 存在则用 v2；否则读同名 v1 平铺键；两者都无 → `Default`。
写迁移：盘上 `version != 2` 时任一写触发整份升级（`version: 2` + 全部 in-file
section + 全部 legacy 平铺键同值保留一版）；此后按段读-改-写。

## 4 Section 契约

```rust
pub trait Section: Default + Clone + Serialize + DeserializeOwned + Send + Sync + 'static {
    const KEY: &'static str;
    /// 唯一 clamp / 归一化落点。UI 不再重复钳，扩展只做防御性钳。
    fn validate(&mut self);
    /// v1 平铺键读迁移（缺省无）/ 写迁移同值回写一版。
    fn from_legacy(...) -> Option<Self> { None }
    fn to_legacy(&self, ...) {}
}
```

- 是否落 app-settings.json（`in_file`）与是否进投影（`exportable`）是
  **注册表级**静态属性（`sections::SectionDef`），单一事实源在
  `sections::registered()`。
- `validate` 在「解析后」「set 后」「从投影读回后」三处调用，保证内存里永远是
  合法值；clamp 范围只写在 section 的注释与单测。
- 新增一个 setting 的 diff：1 个字段 + 1 个控件（`ui/kit`）+ 0 新文件 + 0 新 env。

## 5 `subagents` section

| 字段 | 类型 | 默认 | 范围 | 说明 |
|---|---|---|---|---|
| `capacity` | u64 | 3 | 2..=10 | 同帧并发子代理上限；文件/投影更窄的 min/max 可再收窄 |
| `max_depth` | u64 | 1 | 恒 1 | 禁止递归委派；任何来源写不进别的值 |
| `enabled` | bool | true | — | 关闭后 `agent` 工具直接拒绝 |

- UI 落点：`crates/app/src/settings/agents/dropdowns.rs`（kit stepper）。
- 写入口 `set_subagent_capacity` **双写** `agents/config.json`（legacy fallback，
  未升级扩展可用）；启动期文件缺失按 PF 缺省补一份（`ensure_module_config`）。

## 6 与 044 扩展的读取关系

扩展读 `ext-context.json` 的 `subagents` 段（投影为准）；缺投影回退 legacy
`agents/config.json` → 扩展 FALLBACK。详见 [001-2 §4/§8](001-2-跨进程投影.md)。

## 7 验收（细则见 001-3）

- 未知键往返单测：手工塞 `"x_unknown": {"a": 1}`，一次 `set` 后仍在且逐字不变。
- 拖 stepper 20 次只产生 1 次写盘（coalesce，日志 `[perf] settings.flush=…`）。
- 冷启动 settings 读盘 1 次，`[perf] settings.load=<µs>` 进 pfctx.log。
