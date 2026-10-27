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

use gpui::{WindowControlArea, div, prelude::*, px, rgb};
use pi_link::protocol::{ModelInfo, SlashCommand};

use crate::Chat;

/// 最短展示：从启动页首次绘制起算（见 `mark_splash_painted`）。低于这个数
/// 观感是「闪一下黑」而不是启动页。
pub(crate) const MIN_SPLASH: Duration = Duration::from_millis(500);
/// 就绪兜底：pi 起不来（缺 node / 被占用等）也不能把用户困在启动页。
pub(crate) const SPLASH_TIMEOUT: Duration = Duration::from_millis(3000);

static SPLASH_PAINT: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// 主窗口句柄（main.rs 建窗即登记，pi-flash-weia）：启动恢复的终端 tab
/// 要 window（测字宽/焦点），异步恢复任务经它进 window 上下文——与
/// automation 同款模式。
static MAIN_WINDOW: std::sync::OnceLock<gpui::AnyWindowHandle> = std::sync::OnceLock::new();

pub(crate) fn note_main_window(handle: gpui::AnyWindowHandle) {
    let _ = MAIN_WINDOW.set(handle);
}

pub(crate) fn main_window() -> Option<gpui::AnyWindowHandle> {
    MAIN_WINDOW.get().copied()
}

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
        // 盒 106 = 原始 88 的 1.2 倍（用户定的 +20%）；墨迹占盒 51.7%（v5 裸标）⇒ 实际墨迹高 ~55px
        .child(crate::ui::icon("logo-marks", 106., 0xa1a1a1))
}

// ---------------------------------------------------------------------------
// 启动第一步：目录迁移 + 首启种子（010-启动.md §4 / §10）
// ---------------------------------------------------------------------------

/// 启动第一步（`main()` 最前面调用，必须早于任何读盘者：workspace 记忆缓存 /
/// recents 单例 / sessions 扫描器都是进程级惰性单例）：
/// 1. 建 `~/.pi-flash/`；旧 `~/.pi/agent/pi-flash-*.json` 原样搬家
/// 2. recents 清单仍为空 → 显式种子（按会话 mtime 序落盘）
/// 3. 密钥/自定义 provider 收编（051）：全部**复制式**——auth.json 明文进
///    凭据库 + pf-auth.json，pi models.json 的自定义条目复制进 providers.json；
///    pi 的两个文件字节不动。必须先于任何会话 spawn（-e 扩展 + env 注入
///    依赖账本与凭据库就绪）
pub(crate) fn boot() {
    let moved = pi_link::paths::migrate_legacy_files();
    if moved > 0 {
        eprintln!("[startup] migrated {moved} legacy pi-flash file(s) → ~/.pi-flash/");
    }
    // dev-flags（重型模块发版排除）状态一眼可见：关掉的功能为什么没了。
    // 只在 debug 构建打印：release 包必须 stdout/stderr 零输出（包内烟测门禁，
    // 0.2.1 实录——默认关态的这行状态本身就把烟测打红），且开发机上可能
    // 放着全关的 dev-flags.json，「文件存在才打印」也躲不开。
    if cfg!(debug_assertions) {
        eprintln!("[startup] dev-flags: {}", pi_link::dev_flags::summary());
    }
    // 存量清洗（幂等）：注册表里落在当前安装根内的历史脏条目——0.1.x 更新
    // 交接 spawn 不设 cwd，继承的老 payload 目录曾被当项目写进来（「payload
    // 项目删了每次启动又回来」的根因，pi_link::paths::is_app_install_path）。
    // 放在 boot 第一步：注册表 with_registry 惰性单例首读就是清洗后的盘面。
    let purged = pi_link::projects::purge_app_paths();
    if purged > 0 {
        eprintln!("[startup] purged {purged} app-owned path(s) from project registry");
    }
    // 工作区记忆同批清洗：脏 __last（启动恢复目标被钉死在载荷目录 = 每次双
    // 击都落回 payload 的另一半根因，恢复路径已在 Chat::new 消毒）+ 载荷目
    // 录名下的每项目记忆；__ 前缀的非路径键（布局/UI 状态）不动
    let purged_ws = crate::services::workspace::purge_app_install_memory();
    if purged_ws > 0 {
        eprintln!("[startup] purged {purged_ws} app-owned key(s) from workspace memory");
    }
    pi_link::recents::ensure_seeded();
    // 042-1 一次性迁移：PF 私有账本（providers.json / pf-auth.json）→ pi 的
    // models.json + auth.json 标记条目；旧文件改名 .bak，幂等。失败只记日志，
    // 绝不阻塞启动（下次启动重试）。
    for line in pi_link::migrate_0421::run() {
        eprintln!("[startup] migrate: {line}");
    }
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
    // 043：MCP 不进 ProjectCtx——mcp.json 会被设置页/勾选菜单随时改盘，
    // 缓存副本只会拿到旧列表；消费方（reload_settings_panel）一律盘上现读
}

/// 装载全局态：模型清单（缓存 ∪ 磁盘）、命令（内置 + 扩展缓存）、默认项、插件、全局 mcp。
pub(crate) fn load_globals() -> Globals {
    // 无缓存层：模型清单每次启动现读磁盘（providers.json / models.json /
    // models-store.json）；包内联 provider 与扩展命令等会话进程答到后经
    // `merge_models` / `merge_commands` 并进本进程内存（不落盘）。
    let models = pi_link::catalog::dedupe(pi_link::catalog::disk_models());
    let commands = pi_link::catalog::builtin_commands();

    let settings_path = pi_link::config::settings_path();
    Globals {
        models,
        commands,
        default_tools: pi_link::config::read_default_tools(&settings_path).unwrap_or(None),
        packages: pi_link::config::read_packages(&settings_path).unwrap_or_default(),
    }
}

/// 装载一个项目的技能表（纯磁盘读）。设置页开关写盘后也要现读一遍——
/// `ProjectCtx` 是启动缓存，拿它当写后回读源就是「开关点了没反应」。
pub(crate) fn load_skills(cwd: &Path) -> Vec<pi_link::skills::SkillEntry> {
    let agent_dir = pi_link::config::agent_dir();
    let settings_value = pi_link::config::read_json(&pi_link::config::settings_path())
        .unwrap_or(serde_json::json!({}));
    let home_agents = agent_dir
        .parent()
        .map(|p| p.join("..").join(".agents").join("skills"))
        // dunce：剥掉 std canonicalize 在 Windows 必带的 \\?\ verbatim 前缀，
        // 否则会顺着 SkillEntry.path 漏进技能面板显示（Zed 同款处理）。
        .map(|p| dunce::canonicalize(&p).unwrap_or(p))
        .unwrap_or_else(|| agent_dir.clone());
    pi_link::skills::discover_skills(cwd, &agent_dir, &home_agents, &settings_value)
}

/// 装载一个项目的上下文（纯磁盘读；启动集合内逐项目跑，切项目未命中时现算）。
pub(crate) fn load_project(cwd: &Path) -> ProjectCtx {
    let project_settings = pi_link::config::project_settings_path(cwd);
    let skills = load_skills(cwd);
    let skill_commands = pi_link::catalog::skill_commands(&skills);
    ProjectCtx {
        project_scope: pi_link::config::read_enabled_models(&project_settings)
            .unwrap_or_else(|_| None)
            .is_some(),
        packages: pi_link::config::read_packages(&project_settings).unwrap_or_default(),
        skills,
        skill_commands,
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

#[cfg(test)]
mod tests {
    use super::*;

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
            ModelInfo {
                id: "x".into(),
                name: "rpc".into(),
                provider: "p".into(),
                context_window: Some(9),
            },
            ModelInfo {
                id: "y".into(),
                name: "rpc2".into(),
                provider: "p".into(),
                context_window: None,
            },
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
            SlashCommand {
                name: "skill:image-gen".into(),
                description: "s".into(),
            },
            SlashCommand {
                name: "notify".into(),
                description: "n".into(),
            },
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
/// 1. 120ms 泵：悬停卡 / 导航 flyout / 状态条过期
/// 2. 3s 外部追加观察：非空闲 runtime 的文件被外部（pi-web / CLI）追加时重读
/// 3. 60s 空闲回收：非活动会话进程超 10 分钟未动就杀掉
/// 4. 30s recents 对账：外部写入者的活跃时间回灌清单
/// 5. 启动页闸门：pi 附着（或恢复已定案/超时兜底）+ 最短展示 500ms → 揭幕
/// 6. 恢复最后会话 + 进入其项目 + 会话清单装载 = spawn_boot_restore
///    （010 重设计：进程附着随恢复路径走，无项目态零进程，不再有首帧附着）
pub(crate) fn spawn_boot_tasks(cx: &mut gpui::Context<Chat>) {
    spawn_pump_120ms(cx);
    spawn_external_append_watch(cx);
    spawn_idle_recycle(cx);
    spawn_recents_poll(cx);
    spawn_splash_gate(cx);
    spawn_fs_watch_pump(cx);
    // 微信 Bot 入站泵（060）：dev-flag `wx` 关（默认，发版排除）= 泵都
    // 不挂——入口全隐（菜单/tab/open_wx），模块整死。开态渠道由泵里的
    // 服务状态机决定：**开关 on 且【微信通话】窗口激活**才在跑（002
    // 「关于微信通话服务激活」的设计取舍：低频功能不激活就不后台收消息）
    if crate::services::workspace::wx_enabled() {
        crate::remote_control::spawn_wx_pump(cx);
    }
    // syntect 语法引擎冷加载 ~百 ms 挪出首个 md 渲染帧（第一次渲染卡顿）：
    // 后台线程预热 OnceLock，命中后首渲零成本
    std::thread::spawn(|| crate::editor::markdown::warm_up());
    // PF 插件清单扫描（beads 360 ①；02 §4：启动只承担清单，store/ctx 懒加载
    // ——微秒级/个，后台落位后状态栏 tab / 右键【插件】即出现）
    crate::pf_plugin::spawn_plugin_scan(cx);
}

/// fs 监听泵（Zed worktree 扫描器的轻量对应物）：watcher 事件经后台
/// 去抖线程合批（FS_WATCH_LATENCY ≈ 100ms 静默期 + 批内一次抽干），
/// gpui 侧 150ms 轮询信号 → 重扫展开目录 + git 状态。线程与 executor
/// 各干各的活，互不阻塞。
fn spawn_fs_watch_pump(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| {
        let rx = match this.update(cx, |c, _| c.ws.fs_watch_rx.take()) {
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
                    chat.refresh_git(cx);
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

/// 首帧后的进程附着已退役（010 重设计）：附着随恢复路径走——open_session
/// 自带 spawn + GetMessages/anchors/refresh_state，无项目态零进程。

/// 120ms 泵：导航 flyout / 状态条过期（光标闪烁由输入组件自管，
/// 不再需要 2Hz 切换；会话详情卡 2026-10-15 起改右键召唤，无定时逻辑）。
fn spawn_pump_120ms(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(120))
                .await;
            let ok = this
                .update(cx, |c, cx| {
                    let mut dirty = false;
                    // 导航 flyout 250ms 离开宽限（pi-web PREVIEW_HIDE_DELAY parity）
                    if c.layout.nav_open {
                        if let Some(at) = c.layout.nav_hide_at {
                            if at.elapsed() > std::time::Duration::from_millis(250) {
                                c.layout.nav_open = false;
                                c.layout.nav_flyout_hovered = false;
                                c.layout.nav_hover_turn = None;
                                c.layout.nav_hide_at = None;
                                dirty = true;
                            }
                        }
                    }
                    if let Some((_, at)) = &c.dlg.status_toast {
                        if at.elapsed() > std::time::Duration::from_millis(2500) {
                            c.dlg.status_toast = None;
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
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(3))
                .await;
            let ok = this
                .update(cx, |chat, cx| {
                    for rt in chat.pool.runtimes.values() {
                        rt.update(cx, |r, cx| r.check_external_append(cx));
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

/// 60s 空闲回收（pi-web idle-timeout parity）：非活动会话空闲超 10 分钟就
/// 杀掉 pi 进程，并连 runtime 实体一起退役——transcript（messages 含工具
/// 结果全文、system_msgs 含整份系统提示词）随实体释放，是长跑内存的
/// 大头；清单/psp 标记不动，回开走 open_session 的 tail 秒开路径（磁盘
/// 尾巴 + 后台重挂），与冷启动打开旧会话同一条路。活跃会话永不退役。
fn spawn_idle_recycle(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(60))
                .await;
            let ok = this
                .update(cx, |chat, cx| {
                    let idle_cap = std::time::Duration::from_secs(600);
                    // 先收集后摘除：退役即 drop 实体，不在遍历中动 map
                    let mut retire: Vec<String> = Vec::new();
                    for (key, rt) in &chat.pool.runtimes {
                        if *key == chat.pool.active_key {
                            continue;
                        }
                        let idle = {
                            let r = rt.read(cx);
                            !r.agent_running
                                && r.last_activity.elapsed() > idle_cap
                                && r.agent.session.is_some()
                                && r.ext_queue.is_empty()
                        };
                        if idle {
                            retire.push(key.clone());
                        }
                    }
                    for key in retire {
                        if let Some(rt) = chat.pool.runtimes.remove(&key) {
                            rt.update(cx, |r2, _| r2.shutdown_process());
                            // rt 在此 drop：transcript 随实体释放；
                            // session_tail_cache 不动，供秒开回切
                        }
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

/// 30s recents 对账（003-session管理）：外部写入者（pi-web / CLI）改了会话文件
/// 就把活跃时间回灌清单并落盘。活着的 runtime 的文件排除在外——它们的活跃时间
/// 由事件路径维护，轮询会在 pi 追加时反复重扫。
fn spawn_recents_poll(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(30))
                .await;
            let exclude = match this.update(cx, |chat, cx| {
                chat.pool
                    .runtimes
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
            let (attached, boot_resolved) = this
                .update(cx, |c, cx| {
                    let attached = c
                        .pool
                        .runtimes
                        .get(&c.pool.active_key)
                        .is_some_and(|r| r.read(cx).agent.session.is_some());
                    (attached, c.pool.boot_resolved)
                })
                .unwrap_or((true, true)); // 实体已亡（窗口关闭）：停止闸门
            let elapsed = t0.elapsed();
            // MIN_SPLASH 从启动页首帧起算（忙机首帧可能晚于 pi 附着）
            let splash_shown = splash_paint_age().is_some_and(|age| age >= MIN_SPLASH);
            // 无项目态没有可附着的进程：boot_resolved 直接放行，不等超时
            if ((attached || boot_resolved) && splash_shown) || elapsed >= SPLASH_TIMEOUT {
                break;
            }
        }
        let _ = this.update(cx, |c, cx| {
            c.pool.booted = true;
            c.pool.pending_zoom = true;
            // 040 延迟加载：揭幕（切换启动页）后才开始测扩展/skill 说明
            // token——非关键数据，不对启动造成任何负担
            c.spawn_token_probe(cx);
            // 081 延迟加载：新版本首启开「更新日志」tab + 后台检查更新
            // （timeout 30s、失败重试 2 次间隔 10min，全程后台线程不阻塞）
            c.spawn_startup_update_hooks(cx);
            // 082 延迟加载：Pi 扩展更新检测（只读 + 网络，零写入；结果进
            // aux.plugin_updates，新会话页通知与设置·扩展页消费）
            c.spawn_pi_extension_updates_check(cx);
            // 040-1 延迟加载：Pi 扩展市场预热——只预取首页，且**只在已有
            // 首页快照时**才发请求（从没用过市场页 = 零请求），见
            // market/prefetch.rs
            c.spawn_pi_ext_market_prefetch(cx);
            // 052 延迟加载：会话自动清理（后排任务——扫描+回收站搬运全在
            // 后台线程，主线程只做状态摘除；0 天 = 关闭）
            c.spawn_session_auto_clean(cx);
            if crate::PERF.load(std::sync::atomic::Ordering::Relaxed) {
                eprintln!("[perf] splash reveal: {:?}", t0.elapsed());
            }
            cx.notify();
        });
    })
    .detach();
}

/// 启动恢复目标（010 重设计）。
pub(crate) enum BootTarget {
    /// 进入项目（cwd）；session = 要恢复的最后会话（None = 项目新开草稿）。
    Project {
        cwd: PathBuf,
        session: Option<PathBuf>,
    },
    /// 无项目态：维持 ws.cwd = None，零进程。
    None,
}

/// 最近活动项目 = **最后一次 cwd 切换的项目**（cwd 切换总有时刻，简单可靠）。
///
/// 时刻来源只有一个：`projects.json.last_opened_ms`——由 `register_project`
/// 在**每次进入项目**时写入。三条切 cwd 的路径都汇到 `remount_workspace`
/// （switch_project / 打开老会话 open_session / 文件树切 sync_tree_to_file），
/// 所以「最近打开」就是「最后切换到的 cwd」，不需要再聚合会话活动。
/// 目录已删 / 落在应用自有目录（历史污染）的记录跳过。
fn recent_project() -> Option<PathBuf> {
    pi_link::projects::project_records()
        .into_iter()
        .filter(|r| r.path.is_dir() && !pi_link::paths::is_app_install_path(&r.path))
        .max_by_key(|r| r.last_opened_ms)
        .map(|r| r.path)
}

/// 启动落点判定（纯盘读，后台线程跑）：
/// - `PI_FLASH_CWD` 显式覆盖优先（开发意图）：目录存在即进，目录不存在直接
///   无项目态（绝不兜底乱进）；
/// - 否则 cwd = 最近活动项目（[`recent_project`]）；
/// - `startup_restore=on` 时连带加载**该项目的**最后会话（per-project 记忆
///   `get_last_open`，由 open_session/new_session 维护）；off 停在**新会话页**；
/// - 没有任何项目记录（真首次运行）→ None（无项目态）。
fn boot_target() -> BootTarget {
    let restore = crate::services::workspace::startup_restore();

    // ① 开发覆盖
    let cwd = match std::env::var("PI_FLASH_CWD") {
        Ok(raw) => {
            let cwd = PathBuf::from(raw);
            if !cwd.is_dir() {
                return BootTarget::None;
            }
            cwd
        }
        // ② 最近活动项目（最后一次切换到的 cwd）
        Err(_) => match recent_project() {
            Some(cwd) => cwd,
            None => return BootTarget::None,
        },
    };

    // ③ restore 只决定要不要连带加载该项目的最后会话
    let session = if restore {
        let hidden = pi_link::hidden_sessions::load();
        crate::services::workspace::get_last_open(&cwd.to_string_lossy())
            .map(PathBuf::from)
            // 影子会话（微信线）不做桌面恢复目标（2026-10-25 孤儿页事故）：
            // 微信线归【微信通话】标签管，历史遗留指针一律按「无会话可
            // 恢复」处理，落新会话页
            .filter(|p| p.exists() && !hidden.hides(None, p))
            // 影子会话（pf-wxbot-*）**永不**做桌面恢复目标：按文件名前缀与
            // state.session_key 双保险（过滤清单可能滞后一轮）
            .filter(|p| {
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if name.starts_with("pf-wxbot-") {
                    return false;
                }
                let key = wxprobe::state::get_str(&wxprobe::state::load(), "session_key")
                    .unwrap_or_default();
                key != p.to_string_lossy()
            })
    } else {
        None
    };
    BootTarget::Project { cwd, session }
}

/// 启动恢复 + 会话清单装载（010 重设计，原 spawn_session_list_load）：
/// 1. 判定恢复目标（后台线程）→ 进入其项目（remount + 恢复会话/新草稿）；
///    无目标 → 维持无项目态并置 `boot_resolved`（揭幕闸门据此放行）。
/// 2. 「加载时间窗口」内活跃会话 → 项目分组 → 集合内项目上下文预装 →
///    活动序前 `TAIL_PRELOAD` 条的尾部预载。
pub(crate) fn spawn_boot_restore(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| {
        let target = cx.background_spawn(async move { boot_target() }).await;
        // tab 带存档（pi-flash-weia，单槽全局）。语义只有两条，别再加
        // 特例：**「启动恢复」开 = 存 + 回放；关 = 存档清空、整个会话不写**。
        // 开关本身在 persist_tabs 里把门（写侧），这里只管读。
        let restore_on = crate::services::workspace::startup_restore();
        if !restore_on {
            // 关态：清掉存量存档（含上一个开关周期留下的），确保「关着就
            // 什么都不恢复」；本次会话 persist_tabs 全部 no-op
            crate::services::workspace::clear_tabs_state();
        }
        let saved_tabs = match &target {
            BootTarget::Project { .. } if restore_on => {
                crate::services::workspace::tabs_state_for()
                    .filter(|s| !s.tabs.is_empty() || s.wx_open)
            }
            _ => None,
        };
        let (active_cwd, active_session) = match &target {
            BootTarget::Project { cwd, session } => {
                (Some(cwd.to_string_lossy().to_string()), session.clone())
            }
            BootTarget::None => (None, None),
        };
        let _ = this.update(cx, |chat, cx| {
            // 启动期 tab 带落盘抑制：开偏空带/回放中间态不许写穿存档；
            // 回放路（apply_saved_tabs）自持，结束即解；无存档的空带启动
            // 也抑制到本块结束——避免把别名的存量存档误抹掉
            chat.ws.tab_restore_busy = true;
            match target {
                BootTarget::Project { cwd, session } => {
                    chat.ws.cwd = Some(cwd.clone());
                    // 打开项目即注册：恢复落进的项目算显式打开
                    pi_link::projects::register_project(&cwd);
                    chat.remount_workspace(cx);
                    let key = cwd.to_string_lossy().to_string();
                    chat.pool
                        .project_ctx
                        .entry(key)
                        .or_insert_with(|| load_project(&cwd));
                    chat.reload_model_defaults();
                    match session {
                        Some(path) => chat.open_session(path, false, cx),
                        // 项目在、会话无从恢复（restore=off 或不存）：落新会话页。
                        // 用 boot_new_session：启动不抹 per-project 会话记忆
                        None => {
                            chat.pool.boot_resolved = true;
                            chat.boot_new_session(cx);
                        }
                    }
                }
                BootTarget::None => chat.pool.boot_resolved = true,
            }
            cx.notify();
        });
        // —— tab 带回放（pi-flash-weia）：文件/终端/激活位/前台视图按存档
        // 复原，在会话恢复之后（switch_to 会把前台拽回 Chat，回放覆盖它）。
        // 终端重开要 window：经主窗口句柄进 window 上下文（automation 同款）。
        // 句柄缺失（建窗早于本任务首帧，理论不可达）降级为只恢复文件 tab。
        if let Some(saved) = saved_tabs {
            let _ = cx.update(|cx| {
                let _ = main_window().map(|win| {
                    let _ = win.update(cx, |_view, window, cx| {
                        let _ = this.update(cx, |chat, cx| {
                            chat.apply_saved_tabs(&saved, Some(window), cx)
                        });
                    });
                });
                // 无窗口降级：文件 tab 不需要 window，照旧回放
                let _ = this.update(cx, |chat, cx| {
                    if !chat.ws.panel_tabs.is_empty() {
                        return;
                    }
                    chat.apply_saved_tabs(&saved, None, cx);
                });
            });
        }
        // 回放结束**无条件**解锁（finally 语义）：apply_saved_tabs 在空带形态
        // 会提前 return，锁一旦泄漏整个进程的 persist_tabs 全部静默跳过
        let _ = this.update(cx, |chat, _| chat.ws.tab_restore_busy = false);
        // —— 会话清单装载（全部在后台线程，首帧只画空壳）：当前项目会话
        //（前 100；无项目态跳过）→ 「加载时间窗口」内活跃会话 → 项目分组 →
        // 集合内每个项目的项目上下文预装 → tail 预载 ——
        let sessions = match &active_cwd {
            Some(cwd_text) => pi_link::sessions::list_sessions_for_cwd(cwd_text, 100)
                .into_iter()
                .filter(|s| crate::services::workspace::same_ws(&s.cwd, cwd_text))
                .collect::<Vec<_>>(),
            None => Vec::new(),
        };
        let _ = this.update(cx, |chat, cx| {
            // 影子会话（微信线 pf-wxbot-*）不进列表：过滤清单读一次
            let hidden = pi_link::hidden_sessions::load();
            chat.pool.sessions = sessions
                .into_iter()
                .filter(|s| !hidden.hides(s.name.as_deref(), &s.path))
                .collect();
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
        let mut paths = cx
            .background_spawn(async move { pi_link::recents::recent_load_paths(days) })
            .await;
        // 置顶会话并入装载清单（020 会话置顶）：pin 的意义就是老会话常驻
        // 可见，哪怕早已跌出加载时间窗口。追加在尾部——尾部预载按序取前
        // N 条，置顶路径不会挤占近期会话的预载名额
        let pins = this
            .update(cx, |chat, _| chat.psp.session_pins.clone())
            .unwrap_or_default();
        for pin in pins {
            if pin.is_file()
                && !paths
                    .iter()
                    .any(|p| crate::services::workspace::same_path(p, &pin))
            {
                paths.push(pin);
            }
        }
        // 清单刚种子/保活，此处必命中索引：纯 stat + 内存查，不碰文件
        let all = pi_link::sessions::sessions_for_paths(&paths);
        let _ = this.update(cx, |chat, cx| {
            chat.rebuild_projects(all);
            // 启动自动折叠：项目内会话全部早于「自动折叠时间」阈值 → 折叠
            // （其他页档位 1/3/7 天）。仅启动这一次，运行期重建不参与
            chat.auto_collapse_stale_projects();
            // 项目集确定后，后台把集合内每个项目的项目上下文装好
            // （settings/mcp/skills）——首帧不等它；切项目/开设置页直接命中
            let pending: Vec<PathBuf> = chat
                .psp
                .projects
                .iter()
                .map(|g| g.path.clone())
                .filter(|p| {
                    !chat
                        .pool
                        .project_ctx
                        .contains_key(&p.to_string_lossy().to_string())
                })
                .collect();
            if !pending.is_empty() {
                cx.spawn(async move |this, cx| {
                    for cwd in pending {
                        let key = cwd.to_string_lossy().to_string();
                        let ctx = cx.background_spawn(async move { load_project(&cwd) }).await;
                        let _ = this.update(cx, |chat, cx| {
                            chat.pool.project_ctx.insert(key, ctx);
                            cx.notify();
                        });
                    }
                })
                .detach();
            }
            cx.notify();
        });
        // tail preload（活动序前 TAIL_PRELOAD 条；active 已由首屏渲染，跳过）
        let active = active_session;
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
            chat.pool.session_tail_cache = preloaded;
        });
    })
    .detach();
}
