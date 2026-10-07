//! 010-启动：启动阶段唯一入口。
//!
//! 1. **启动页**：参考 codex（docs/模块设计/image.bmp）——近黑背景、logo 居中、
//!    普通窗口不最大化；pi 附着（或超时兜底，见 main.rs 的启动闸门）后由 Chat
//!    揭幕，落点即启动恢复的既有分流：有上次会话 → 030 会话界面，无 → 012
//!    新会话页。
//! 2. **全局态一次性装载**（010-启动.md §1-§8）：启动时把后续所有界面要用的
//!    全局量从硬盘装进内存（模型清单 / 命令 / 插件 / mcp / 默认项 / 项目上下文），
//!    渲染路径只读内存，**不扫盘、不发 RPC**；会话进程答到的 RPC 只做覆盖
//!    （并回写 `catalog-cache.json`，见 §4.1）。
//!
//! 自有文件全部落 `~/.pi-flash/`（`pi_link::paths`），`~/.pi/agent/` 只留给 pi。

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{div, prelude::*, px, rgb, WindowControlArea};
use pi_link::protocol::{ModelInfo, SlashCommand};

use crate::Chat;

/// 最短展示：从启动页首次绘制起算（见 `mark_splash_painted`）。低于这个数
/// 观感是「闪一下黑」而不是启动页。
pub(crate) const MIN_SPLASH: Duration = Duration::from_millis(500);
/// 就绪兜底：pi 起不来（缺 node / 被占用等）也不能把用户困在启动页。
pub(crate) const SPLASH_TIMEOUT: Duration = Duration::from_millis(3000);

static SPLASH_PAINT: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// 启动页首帧绘制时刻（splash 渲染分支调用；OnceLock 只记第一次）。
pub(crate) fn mark_splash_painted() {
    let _ = SPLASH_PAINT.set(std::time::Instant::now());
}

/// 启动页已绘制时长——None = 还没画出来（忙机首帧可晚于 pi 附着，
/// 揭幕闸门必须等这个有值，否则用户一帧启动页都见不到）。
pub(crate) fn splash_paint_age() -> Option<Duration> {
    SPLASH_PAINT.get().map(|t| t.elapsed())
}

/// 全幅启动页。取色自参考图：底 0x202020、logo 0xa1a1a1。顶部 40px 注册
/// 拖拽区——client-side titlebar 下没有原生拖拽带，否则启动页拖不动窗口
///（关闭走 Alt+F4 / 任务栏；启动页通常亚秒级，控制钮不画）。
pub(crate) fn splash_view() -> gpui::Div {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(0x202020))
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .w_full()
                .h(px(40.))
                .window_control_area(WindowControlArea::Drag),
        )
        .child(crate::ui::icon("logo-marks", 88., 0xa1a1a1))
}

// ---------------------------------------------------------------------------
// 启动第一步：目录迁移 + 首启种子（010-启动.md §4 / §10）
// ---------------------------------------------------------------------------

/// 启动第一步（`main()` 最前面调用，必须早于任何读盘者：workspace 记忆缓存 /
/// recents 单例 / sessions 扫描器都是进程级惰性单例）：
/// 1. 建 `~/.pi-flash/`；旧 `~/.pi/agent/pi-flash-*.json` 原样搬家
/// 2. recents 清单仍为空 → 显式种子（按会话 mtime 序落盘）
pub(crate) fn boot() {
    let moved = pi_link::paths::migrate_legacy_files();
    if moved > 0 {
        eprintln!("[startup] migrated {moved} legacy pi-flash file(s) → ~/.pi-flash/");
    }
    pi_link::recents::ensure_seeded();
}

// ---------------------------------------------------------------------------
// 全局态（一次装载，渲染只读）：模型清单 / 命令 / 默认项 / 插件 / mcp
// ---------------------------------------------------------------------------

/// 跨项目共享的全局态（010-启动.md §1）。
#[derive(Default)]
pub(crate) struct Globals {
    /// 模型清单 = `catalog-cache.json` ∪ 磁盘（`models.json` + `models-store.json`）。
    /// 会话进程的 `get_available_models` 答到后**合并覆盖**（见 `merge_models`）。
    pub models: Vec<ModelInfo>,
    /// 扩展命令缓存 + pi 内置表（pi 1.0.0 = 空）。skill 命令按项目走 `ProjectCtx`。
    pub commands: Vec<SlashCommand>,
    /// `settings.json defaultTools`（会话预设 `configured` 由 pi 按它解析）。
    pub default_tools: Option<Vec<String>>,
    /// 全局插件声明（`settings.json packages`）。
    pub packages: Vec<serde_json::Value>,
}

/// 一个项目的上下文（010-启动.md §5）：启动集合内逐项目装载一次，切项目直接命中。
#[derive(Default, Clone)]
pub(crate) struct ProjectCtx {
    /// 项目 `.pi/settings.json` 覆盖了 enabledModels（设置页只读）。
    pub project_scope: bool,
    pub packages: Vec<serde_json::Value>,
    pub skills: Vec<pi_link::skills::SkillEntry>,
    /// `skill:<name>` 命令（由 `skills` 派生，草稿态 `/` 菜单用）。
    pub skill_commands: Vec<SlashCommand>,
    pub mcp_servers: Vec<pi_link::mcp::ServerEntry>,
    pub mcp_errors: Vec<String>,
    pub profiles: Vec<pi_link::subagents::SubagentProfile>,
    pub agents_settings: pi_link::subagents::SubagentSettings,
}

/// 装载全局态：模型清单（缓存 ∪ 磁盘）、命令（内置 + 扩展缓存）、默认项、插件、全局 mcp。
pub(crate) fn load_globals() -> Globals {
    let cache = read_cache();
    // 缓存优先：它含包内联 provider（pi-freeflow 之类磁盘没有数据文件的清单）
    let mut models = cache.models;
    models.extend(pi_link::catalog::disk_models());
    let models = pi_link::catalog::dedupe(models);

    let mut commands = pi_link::catalog::builtin_commands();
    commands.extend(cache.commands);

    let settings_path = pi_link::config::settings_path();
    Globals {
        models,
        commands,
        default_tools: pi_link::config::read_default_tools(&settings_path).unwrap_or(None),
        packages: pi_link::config::read_packages(&settings_path).unwrap_or_default(),
    }
}

/// 装载一个项目的上下文（纯磁盘读；启动集合内逐项目跑，切项目未命中时现算）。
pub(crate) fn load_project(cwd: &Path) -> ProjectCtx {
    let agent_dir = pi_link::config::agent_dir();
    let settings_path = pi_link::config::settings_path();
    let project_settings = pi_link::config::project_settings_path(cwd);
    let settings_value = pi_link::config::read_json(&settings_path).unwrap_or(serde_json::json!({}));
    let home_agents = agent_dir
        .parent()
        .map(|p| p.join("..").join(".agents").join("skills"))
        .map(|p| p.canonicalize().unwrap_or(p))
        .unwrap_or_else(|| agent_dir.clone());
    let skills = pi_link::skills::discover_skills(cwd, &agent_dir, &home_agents, &settings_value);
    let skill_commands = pi_link::catalog::skill_commands(&skills);
    let agents_settings = pi_link::subagents::read_settings(&agent_dir);
    let profiles = pi_link::subagents::list_profiles(cwd, &agent_dir, &agents_settings);
    let (mcp_servers, mcp_errors) = pi_link::mcp::load(Some(cwd));
    ProjectCtx {
        project_scope: pi_link::config::read_enabled_models(&project_settings)
            .unwrap_or_else(|_| None)
            .is_some(),
        packages: pi_link::config::read_packages(&project_settings).unwrap_or_default(),
        skills,
        skill_commands,
        mcp_servers,
        mcp_errors,
        profiles,
        agents_settings,
    }
}

/// 把 RPC 答到的清单并进全局态（进程答案只做覆盖）：返回是否有变化。
pub(crate) fn merge_models(globals: &mut Globals, rpc: &[ModelInfo]) -> bool {
    let mut merged = rpc.to_vec();
    merged.extend(globals.models.iter().cloned());
    let deduped = pi_link::catalog::dedupe(merged);
    let changed = deduped.len() != globals.models.len()
        || deduped.iter().zip(globals.models.iter()).any(|(a, b)| {
            a.provider != b.provider
                || a.id != b.id
                || a.name != b.name
                || a.context_window != b.context_window
        });
    globals.models = deduped;
    changed
}

/// 把 RPC 答到的扩展命令并进全局命令清单（`skill:` 前缀不入全局，skill 走磁盘
/// 派生）：返回是否有变化。
pub(crate) fn merge_commands(globals: &mut Globals, rpc: &[SlashCommand]) -> bool {
    let mut changed = false;
    for c in rpc.iter().filter(|c| !c.name.starts_with("skill:")) {
        if !globals.commands.iter().any(|g| g.name == c.name) {
            globals.commands.push(c.clone());
            changed = true;
        }
    }
    changed
}

// ---------------------------------------------------------------------------
// 自有缓存 `~/.pi-flash/catalog-cache.json`（010-启动.md §4.1）
// ---------------------------------------------------------------------------

/// 缓存内容：模型目录 + 扩展命令（磁盘上拿不到的那部分）。
#[derive(Default, Clone)]
pub(crate) struct CatalogCache {
    pub models: Vec<ModelInfo>,
    pub commands: Vec<SlashCommand>,
}

/// 从显式路径读缓存（测试用同一实现，不碰进程环境）。
fn cache_load_from(path: &Path) -> CatalogCache {
    let Ok(value) = pi_link::config::read_json(path) else {
        return CatalogCache::default();
    };
    let models = value
        .get("models")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let id = m.get("id")?.as_str()?.to_string();
                    let provider = m.get("provider")?.as_str()?.to_string();
                    Some(ModelInfo {
                        id,
                        name: m.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        provider,
                        context_window: m.get("contextWindow").and_then(|v| v.as_u64()),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let commands = value
        .get("commands")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    Some(SlashCommand {
                        name: c.get("name")?.as_str()?.to_string(),
                        description: c.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    CatalogCache { models, commands }
}

/// 写缓存（原子写）；返回是否写入。
fn cache_save_to(path: &Path, cache: &CatalogCache) -> bool {
    let models: Vec<serde_json::Value> = cache
        .models
        .iter()
        .map(|m| {
            serde_json::json!({
                "provider": m.provider,
                "id": m.id,
                "name": m.name,
                "contextWindow": m.context_window,
            })
        })
        .collect();
    let commands: Vec<serde_json::Value> = cache
        .commands
        .iter()
        .map(|c| serde_json::json!({ "name": c.name, "description": c.description }))
        .collect();
    let value = serde_json::json!({
        "version": 1,
        "fetchedAt": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
        "models": models,
        "commands": commands,
    });
    let _ = pi_link::paths::ensure_dir();
    pi_link::config::write_json(path, &value).is_ok()
}

/// 读自有缓存（无文件/坏文件 → 空）。
pub(crate) fn read_cache() -> CatalogCache {
    match pi_link::paths::catalog_cache_file() {
        Some(p) => cache_load_from(&p),
        None => CatalogCache::default(),
    }
}

/// 回写自有缓存（会话进程的 RPC 答复到位时调用）。
pub(crate) fn write_cache(models: &[ModelInfo], commands: &[SlashCommand]) -> bool {
    let Some(path) = pi_link::paths::catalog_cache_file() else {
        return false;
    };
    cache_save_to(
        &path,
        &CatalogCache {
            models: models.to_vec(),
            commands: commands.to_vec(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_cache_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!(
                "pi-flash-cache-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .subsec_nanos()
            ))
            .join("catalog-cache.json")
    }

    #[test]
    fn cache_roundtrip_keeps_models_and_commands() {
        let path = tmp_cache_path("roundtrip");
        let cache = CatalogCache {
            models: vec![ModelInfo {
                id: "m1".into(),
                name: "Model One".into(),
                provider: "p".into(),
                context_window: Some(1234),
            }],
            commands: vec![SlashCommand {
                name: "notify".into(),
                description: "d".into(),
            }],
        };
        assert!(cache_save_to(&path, &cache));
        let back = cache_load_from(&path);
        assert_eq!(back.models.len(), 1);
        assert_eq!(back.models[0].name, "Model One");
        assert_eq!(back.models[0].context_window, Some(1234));
        assert_eq!(back.commands.len(), 1);
        assert_eq!(back.commands[0].name, "notify");
        // 缺文件 → 空缓存（不 panic）
        let missing = cache_load_from(&path.with_file_name("nope.json"));
        assert!(missing.models.is_empty() && missing.commands.is_empty());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn merge_models_keeps_union_with_priority_to_rpc() {
        let mut g = Globals {
            models: vec![ModelInfo {
                id: "x".into(),
                name: "disk".into(),
                provider: "p".into(),
                context_window: None,
            }],
            ..Default::default()
        };
        let rpc = vec![
            ModelInfo { id: "x".into(), name: "rpc".into(), provider: "p".into(), context_window: Some(9) },
            ModelInfo { id: "y".into(), name: "rpc2".into(), provider: "p".into(), context_window: None },
        ];
        assert!(merge_models(&mut g, &rpc));
        assert_eq!(g.models.len(), 2);
        assert_eq!(g.models[0].name, "rpc", "RPC 答案优先");
        assert_eq!(g.models[1].id, "y");
    }

    #[test]
    fn merge_commands_skips_skill_prefix() {
        let mut g = Globals::default();
        let rpc = vec![
            SlashCommand { name: "skill:image-gen".into(), description: "s".into() },
            SlashCommand { name: "notify".into(), description: "n".into() },
        ];
        assert!(merge_commands(&mut g, &rpc));
        assert_eq!(g.commands.len(), 1);
        assert_eq!(g.commands[0].name, "notify");
        // 幂等
        assert!(!merge_commands(&mut g, &rpc));
    }
}

// ---------------------------------------------------------------------------
// §9 启动阶段的后台任务与揭幕闸门（Chat::new 只调下面两个入口）
// ---------------------------------------------------------------------------

/// 启动期后台任务 + 揭幕闸门（010-启动.md §9）。`Chat::new` 只调这一个入口，
/// 启动阶段做了什么全在这一节里可见：
/// 1. 首帧后附着初始 runtime 的 pi 进程并拉一次状态/命令/模型清单
/// 2. 120ms 泵：悬停卡 / 导航 flyout / 状态条过期
/// 3. 3s 外部追加观察：非空闲 runtime 的文件被外部（pi-web / CLI）追加时重读
/// 4. 60s 空闲回收：非活动会话进程超 10 分钟未动就杀掉
/// 5. 30s recents 对账：外部写入者的活跃时间回灌清单
/// 6. 启动页闸门：pi 附着（或超时兜底）+ 最短展示 500ms → 揭幕
pub(crate) fn spawn_boot_tasks(
    rt: gpui::Entity<crate::session::runtime::SessionRuntime>,
    cx: &mut gpui::Context<Chat>,
) {
    spawn_initial_attach(rt, cx);
    spawn_pump_120ms(cx);
    spawn_external_append_watch(cx);
    spawn_idle_recycle(cx);
    spawn_recents_poll(cx);
    spawn_splash_gate(cx);
    spawn_fs_watch_pump(cx);
}

/// fs 监听泵（Zed worktree 扫描器的轻量对应物）：watcher 事件经后台
/// 去抖线程合批（FS_WATCH_LATENCY ≈ 100ms 静默期 + 批内一次抽干），
/// gpui 侧 150ms 轮询信号 → 重扫展开目录 + git 状态。线程与 executor
/// 各干各的活，互不阻塞。
fn spawn_fs_watch_pump(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| {
        let rx = match this.update(cx, |c, _| c.fs_watch_rx.take()) {
            Ok(Some(rx)) => rx,
            _ => return,
        };
        let _ = this.update(cx, |c, _| c.attach_fs_watch());

        // 去抖线程：首批事件后进入静默期循环，静默 100ms 才放行一次信号
        let (ui_tx, ui_rx) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            while rx.recv().is_ok() {
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    let mut more = false;
                    while rx.try_recv().is_ok() {
                        more = true;
                    }
                    if !more {
                        break;
                    }
                }
                if ui_tx.send(()).is_err() {
                    break; // UI 侧已亡
                }
            }
        });

        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(150))
                .await;
            if ui_rx.try_recv().is_err() {
                continue;
            }
            let ok = this
                .update(cx, |chat, cx| {
                    chat.refresh_git();
                    // 023：同一信号顺带做打开文件的外部改动检测
                    //（无未保存修改自动重载；有修改标冲突等用户裁决）
                    chat.check_external_file_changes(cx);
                    cx.notify();
                })
                .is_ok();
            if !ok {
                break;
            }
        }
    })
    .detach();
}

/// 首帧后的进程附着（spawn 只做一次 ~100ms 的进程创建，不阻塞首帧）：
/// 附着事件泵 + 拉一次 GetMessages/anchors/refresh_state（模型清单与命令的
/// RPC 答复经事件泵进入 `models_by_cwd` 并回写自有缓存）。
fn spawn_initial_attach(
    rt: gpui::Entity<crate::session::runtime::SessionRuntime>,
    cx: &mut gpui::Context<Chat>,
) {
    cx.spawn(async move |_this, cx| {
        let _ = rt.update(cx, |r, cx| {
            if r.agent.session.is_none() {
                if let Some(rx) = r.spawn() {
                    let epoch = r.agent.epoch;
                    crate::session::runtime::SessionRuntime::attach_pump(&rt, rx, epoch, cx);
                }
            }
            if let Some(s) = &r.agent.session {
                let _ = s.send(&pi_link::protocol::Command::GetMessages);
            }
            r.refresh_anchors();
            r.refresh_state();
        });
    })
    .detach();
}

/// 120ms 泵：悬停卡 / 导航 flyout / 状态条过期（光标闪烁由输入组件自管，
/// 不再需要 2Hz 切换）。
fn spawn_pump_120ms(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(120))
                .await;
            let ok = this
                .update(cx, |c, cx| {
                    let mut dirty = false;
                    // 改名中：不打字时鼠标虽不在卡上，也不能清卡
                    let renaming = c.hover_card.as_ref().is_some_and(|h| h.renaming);
                    if !renaming {
                        if let Some(at) = c.hover_card.as_ref().and_then(|h| h.hide_at) {
                            if at.elapsed() > std::time::Duration::from_millis(300) {
                                c.hover_card = None;
                                dirty = true;
                            }
                        }
                    }
                    // 详情卡延迟显示（0.3s）：悬停期满且鼠标未在离行宽限时才置
                    // shown，下一次渲染把卡画出来
                    if let Some(card) = c.hover_card.as_mut() {
                        if !card.shown
                            && card.hide_at.is_none()
                            && card.show_at.elapsed() >= std::time::Duration::from_millis(300)
                        {
                            card.shown = true;
                            dirty = true;
                        }
                    }
                    // 导航 flyout 250ms 离开宽限（pi-web PREVIEW_HIDE_DELAY parity）
                    if c.nav_open {
                        if let Some(at) = c.nav_hide_at {
                            if at.elapsed() > std::time::Duration::from_millis(250) {
                                c.nav_open = false;
                                c.nav_flyout_hovered = false;
                                c.nav_hover_turn = None;
                                c.nav_hide_at = None;
                                dirty = true;
                            }
                        }
                    }
                    if let Some((_, at)) = &c.status_toast {
                        if at.elapsed() > std::time::Duration::from_millis(2500) {
                            c.status_toast = None;
                            dirty = true;
                        }
                    }
                    if dirty {
                        cx.notify();
                    }
                })
                .is_ok();
            if !ok {
                break;
            }
        }
    })
    .detach();
}

/// 3s 外部追加观察（pi-web session-revision parity）：非空闲 runtime 的文件被
/// 外部写入（pi-web / CLI 写同一会话）时重读。运行中的 agent 自己持有文件，
/// 由 `check_external_append` 内部判断。
fn spawn_external_append_watch(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| loop {
        cx.background_executor()
            .timer(std::time::Duration::from_secs(3))
            .await;
        let ok = this
            .update(cx, |chat, cx| {
                for rt in chat.runtimes.values() {
                    rt.update(cx, |r, cx| r.check_external_append(cx));
                }
            })
            .is_ok();
        if !ok {
            break;
        }
    })
    .detach();
}

/// 60s 空闲回收（pi-web idle-timeout parity）：非活动会话进程空闲超 10 分钟
/// 就杀掉（清单/内存状态不动）。
fn spawn_idle_recycle(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| loop {
        cx.background_executor()
            .timer(std::time::Duration::from_secs(60))
            .await;
        let ok = this
            .update(cx, |chat, cx| {
                let idle_cap = std::time::Duration::from_secs(600);
                for (key, rt) in &chat.runtimes {
                    if *key == chat.active_key {
                        continue;
                    }
                    let idle = {
                        let r = rt.read(cx);
                        !r.agent_running && r.last_activity.elapsed() > idle_cap && r.agent.session.is_some()
                    };
                    if idle {
                        rt.update(cx, |r2, _| r2.shutdown_process());
                    }
                }
            })
            .is_ok();
        if !ok {
            break;
        }
    })
    .detach();
}

/// 30s recents 对账（003-session管理）：外部写入者（pi-web / CLI）改了会话文件
/// 就把活跃时间回灌清单并落盘。活着的 runtime 的文件排除在外——它们的活跃时间
/// 由事件路径维护，轮询会在 pi 追加时反复重扫。
fn spawn_recents_poll(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| loop {
        cx.background_executor()
            .timer(std::time::Duration::from_secs(30))
            .await;
        let exclude = match this.update(cx, |chat, cx| {
            chat.runtimes
                .values()
                .filter_map(|rt| rt.read(cx).file.clone())
                .collect::<Vec<_>>()
        }) {
            Ok(v) => v,
            Err(_) => break,
        };
        let changed = cx
            .background_spawn(async move { pi_link::recents::poll_recent_sessions(&exclude) })
            .await;
        if changed {
            let _ = this.update(cx, |_chat, cx| cx.notify());
        }
    })
    .detach();
}

/// 启动页闸门（010-启动.md §6 第 7 步）：pi 附着（或 `SPLASH_TIMEOUT` 兜底）
/// 且最短展示 `MIN_SPLASH` 之后揭幕；揭幕帧由 render 的 `pending_zoom` 补最大化。
fn spawn_splash_gate(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| {
        let t0 = std::time::Instant::now();
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(60))
                .await;
            let attached = this
                .update(cx, |c, cx| {
                    c.runtimes
                        .get(&c.active_key)
                        .is_some_and(|r| r.read(cx).agent.session.is_some())
                })
                .unwrap_or(true); // 实体已亡（窗口关闭）：停止闸门
            let elapsed = t0.elapsed();
            // MIN_SPLASH 从启动页首帧起算（忙机首帧可能晚于 pi 附着）
            let splash_shown = splash_paint_age().is_some_and(|age| age >= MIN_SPLASH);
            if (attached && splash_shown) || elapsed >= SPLASH_TIMEOUT {
                break;
            }
        }
        let _ = this.update(cx, |c, cx| {
            c.booted = true;
            c.pending_zoom = true;
            if crate::PERF.load(std::sync::atomic::Ordering::Relaxed) {
                eprintln!("[perf] splash reveal: {:?}", t0.elapsed());
            }
            cx.notify();
        });
    })
    .detach();
}

/// 会话清单装载（010-启动.md §5/§6 第 6 步），全部在后台线程，首帧只画空壳：
/// 当前项目会话（前 100）→ 「加载时间窗口」内活跃会话（清单驱动）→ 项目分组 →
/// 集合内每个项目的项目上下文预装 → 活动序前 `TAIL_PRELOAD` 条的尾部预载。
pub(crate) fn spawn_session_list_load(
    cwd_text: String,
    last_open: Option<PathBuf>,
    cx: &mut gpui::Context<Chat>,
) {
    cx.spawn(async move |this, cx| {
        let sessions = pi_link::sessions::list_sessions_for_cwd(&cwd_text, 100)
            .into_iter()
            .filter(|s| crate::services::workspace::same_ws(&s.cwd, &cwd_text))
            .collect::<Vec<_>>();
        let _ = this.update(cx, |chat, cx| {
            chat.sessions = sessions;
            cx.notify();
            if crate::PERF.load(std::sync::atomic::Ordering::Relaxed) {
                if let Some(t0) = crate::T0.get() {
                    eprintln!("[perf] session list: {:?}", t0.elapsed());
                }
            }
        });
        // 清单窗口过滤，摘要查指纹索引，无全盘枚举；首次运行清单为空时
        // `recent_load_paths` 内部按 mtime 序种子（`startup::boot` 已先种一次）
        let days = crate::services::workspace::load_window_days();
        let paths = cx
            .background_spawn(async move { pi_link::recents::recent_load_paths(days) })
            .await;
        // 清单刚种子/保活，此处必命中索引：纯 stat + 内存查，不碰文件
        let all = pi_link::sessions::sessions_for_paths(&paths);
        let _ = this.update(cx, |chat, cx| {
            chat.rebuild_projects(all);
            // 项目集确定后，后台把集合内每个项目的项目上下文装好
            // （settings/mcp/skills/子代理）——首帧不等它；切项目/开设置页直接命中
            let pending: Vec<PathBuf> = chat
                .projects
                .iter()
                .map(|g| g.path.clone())
                .filter(|p| !chat.project_ctx.contains_key(&p.to_string_lossy().to_string()))
                .collect();
            if !pending.is_empty() {
                cx.spawn(async move |this, cx| {
                    for cwd in pending {
                        let key = cwd.to_string_lossy().to_string();
                        let ctx = cx.background_spawn(async move { load_project(&cwd) }).await;
                        let _ = this.update(cx, |chat, cx| {
                            chat.project_ctx.insert(key, ctx);
                            cx.notify();
                        });
                    }
                })
                .detach();
            }
            cx.notify();
        });
        // tail preload（活动序前 TAIL_PRELOAD 条；active 已由首屏渲染，跳过）
        let active = last_open.clone();
        let preloaded = cx
            .background_spawn(async move {
                let mut map = std::collections::HashMap::new();
                for path in paths {
                    if map.len() >= crate::services::workspace::TAIL_PRELOAD {
                        break;
                    }
                    if Some(&path) == active.as_ref() || !path.is_file() {
                        continue;
                    }
                    let msgs = crate::msgs_from_tail(pi_link::sessions::read_tail_messages(
                        &path,
                        256 * 1024,
                        100,
                    ));
                    map.insert(path, msgs);
                }
                map
            })
            .await;
        let _ = this.update(cx, |chat, _cx| {
            chat.session_tail_cache = preloaded;
        });
    })
    .detach();
}
  
