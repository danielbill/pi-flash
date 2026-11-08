# 001-1 settings 子集：app-settings.json 正式化（v2）

> 状态：设计定稿，S1/S2 实现。
> 前置：[001-0-PFContext总体设计](001-0-PFContext总体设计.md)。投影细节见 [001-2-跨进程投影](001-2-跨进程投影.md)。

## 0 文件名不叫 settings.json

2026-11-08 用户否决「app-settings.json 并入 settings.json」：

> "app-settings.json 并入 settings.json ？ 为什么？？ settings.json是PI的和我们有啥关系？"

`~/.pi/agent/settings.json` 是 pi 的文件（`crates/pi-link/src/config.rs:25`），PF 只读。PF 的设置文档**保持** `<PF数据目录>/app-settings.json`，只做正式化——改名会让排查、文档、issue 全部指向错的东西。

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
  "subagents": { "capacity": 10, "max_depth": 1, "enabled": true },
  "dev_flags": { "subagents": true }
}
```

- section 的**键名即 `Section::KEY`**，字段名即结构体字段（`serde(rename_all = "snake_case")`）。
- 未在本文件登记的键/section 一律原样保留，PF 不认领就不碰。
- `version` 缺失 = v1（平铺），首次写入升为 v2 并迁移（§6）。

## 2 读写模型

**读**

- 进程内 `SettingsStore` 只做一次 `load()`：读盘 → 逐 section 解析 → 合成 `Snapshot`（`Arc`）。
- `hub.settings().get::<Subagents>()` O(1)，零 I/O、无锁，跨 UI 帧安全。
- 坏 section 不拖垮整份：该段回退 `Default` 并记 `problem`（`Snapshot::problems`）。
- UI 不做「读文件再判断」——一次 snapshot 拿全，比较只在内存。

**写**

- `hub.settings().set(|s| s.subagents.capacity = 10)`：
  1. 内存立即换（UI 下一帧就读到新值，无 spinner）；
  2. 标记 dirty，交给**单写者线程**，50ms coalesce（连续拖 stepper 只写一次）；
  3. 落盘 = **读-改-写**：读当前文件 Map → 只替换自己 section → 原子写（tmp+rename，复用 `pi_link::config::write_json`）；
  4. 内容与盘上一致则跳过写（`store.rs:68` `save_map_to` 已有该行为，迁移过来）。
- `flush()`：退出、切项目、切会话前同步落盘；`save_now()` 供迁移与测试。
- 拖拽预览（`preview_chat_content_width`，`app_settings.rs:469`）仍是「只改内存、松手才写」，v2 用同一条 `set` 通道的 `preview` 变体表达，不再单开旁路。

**为什么必须读-改-写**

`app_settings.rs:192` 的 `save_app_settings` 现在从结构体整份重建 `serde_json::Map`——用户手工加的键、未来版本写入的 section、其它分支实验字段，都会在下一次保存时被静默抹掉。这是 v2 必须消灭的缺陷，单测覆盖：写入前后未知键逐字相同。

## 3 v1 平铺键 → v2 section 映射（25 键）

| v2 section | v1 键 | 备注 |
|---|---|---|
| `appearance` | theme, icon_theme, session_font, panel_font, markdown_font, lang | 字体是对象，保留原结构 |
| `chat` | chat_content_width, md_live_preview, send_mode, show_thinking, git_markers | |
| `sessions` | load_window_days, startup_restore, session_display_count, auto_collapse_hours, auto_clean_days | 键名去掉冗余前缀：`display_count` |
| `index` | at_index_ttl_secs, at_index_max_projects | @ 文件索引参数 |
| `models` | default_provider, default_model | |
| `updates` | auto_update | |
| `shell` | autohide_icons, sound | |
| `subagents` | subagent_capacity → `capacity`、subagent_enabled → `enabled`（新增 `max_depth`，恒 1） | 见 §5 |

读迁移：v2 section 存在则用 v2；否则读同名 v1 平铺键；两者都无 → `Default`。
写迁移：任一 `set` 触发整份升级（写 `version: 2` + 全部 section），旧平铺键**保留一版**（便于回滚），下一版删除。

## 4 Section 契约

```rust
pub trait Section: Default + Serialize + DeserializeOwned {
    const KEY: &'static str;
    /// 唯一 clamp / 归一化落点。UI 不再重复钳，扩展只做防御性钳。
    fn validate(&mut self);
    /// 是否可写入跨进程投影（默认 false，见 001-2）。
    fn exportable(&self) -> bool { false }
}
```

- `validate` 在「解析后」「set 后」「从投影读回后」三处调用，保证内存里永远是合法值。
- clamp 范围写进 section 的注释与单测；不在 UI、不在扩展、不在文档里各写一遍。
- 新增一个 setting 的 diff 应该是：1 个字段 + 1 个控件（`ui/kit`）+ 0 新文件 + 0 新 env。

## 5 首个 section：`subagents`

| 字段 | 类型 | 默认 | 范围 | 说明 |
|---|---|---|---|---|
| `capacity` | u64 | 3 | 2..=10 | 同帧并发子代理上限；文件/投影更窄的 min/max 可再收窄 |
| `max_depth` | u64 | 1 | 恒 1 | 2026-11-07 用户拍板：禁止递归委派，UI 已移除该控件 |
| `enabled` | bool | true | — | 关闭后 `agent` 工具直接拒绝 |

- UI 落点不变：`crates/app/src/settings/agents/dropdowns.rs:113-132`（kit stepper，id `ag-cap`）。
- 迁移仅一次：v1 的 `subagent_capacity`/`subagent_enabled` 在 `agents/config.json` 缺失时用于回填（`ensure_module_config`，已实现于 `crates/pi-link/src/agents/module_config.rs`），此后**删除写入**，只读兼容一版。

## 6 与 044 扩展的关系（过渡）

| 阶段 | 事实源 | 扩展读到什么 |
|---|---|---|
| 现在（S1/S2） | PF 写 `agents/config.json` | 扩展每次委派重读该文件 |
| S3 起 | `app-settings.json` `subagents` 段 | `ext-context.json` 的 `subagents` 段；缺投影时 fallback 旧文件 → FALLBACK |
| S4 后 | 同上 | 旧文件退役，仅保留读取兼容一个版本 |

`044-5-subagent管理.md` §1.4 在 S3 落地时改写为「投影段」，其余章节的 UI/交互不变。

## 7 验收（细则见 001-3）

- 未知键往返单测：手工塞 `"x_unknown": {"a": 1}`，一次 `set` 后仍在且字节级不变。
- 拖 stepper 20 次只产生 1 次写盘（coalesce 生效，日志 `[perf] settings.flush=…`）。
- `grep -rn "subagent_capacity" crates/` 只剩迁移读点与测试。
- 冷启动 settings 读盘 1 次，`[perf] settings.load=<µs>` 进日志。
