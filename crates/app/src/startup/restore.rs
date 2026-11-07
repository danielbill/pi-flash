//! 启动恢复（010 重设计）：恢复目标判定 + 进入项目 + 会话清单装载。

use gpui::AppContext as _;

use super::{load_project, main_window};
use crate::Chat;
use std::path::PathBuf;

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
        // 影子会话（插件认领线）**永不**做桌面恢复目标（2026-10-25 孤儿页
        // 事故）：认领线归插件管，历史遗留指针一律按「无会话可恢复」处理，
        // 落新会话页。铁闸数据源 = 已装插件**清单声明**的认领前缀
        // （[[contributes.sessions]]）——静态、不随运行期过滤清单滞后或
        // 停用清条而漂移；PF 不含任何具体插件的身份信息。
        let claim_prefixes = crate::pf_plugin::declared_claim_prefixes();
        crate::services::workspace::get_last_open(&cwd.to_string_lossy())
            .map(PathBuf::from)
            .filter(|p| p.exists() && !hidden.hides(None, p))
            // 子会话（044-0 观察窗）永不做恢复目标：它是只读检视，驱动权归
            // 父 pi；旧版 open_session 曾把子会话写进 last-open（已修），这里
            // 把历史污染一并挡住——落新会话页
            .filter(|p| !pi_link::sessions::is_subagent_session(p))
            .filter(|p| {
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                !claim_prefixes
                    .iter()
                    .any(|pre| name.starts_with(pre.as_str()))
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
                crate::services::workspace::tabs_state_for().filter(|s| !s.tabs.is_empty())
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
            // 影子会话（插件认领线）不进列表：过滤清单读一次
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
            // 分组建好立刻对齐固定段标题快照（2026-11-07「新会话」复发
            // 根因②）：启动恢复路 open_session→switch_to 时分组/清单尚空，
            // switch_to 取不到真值、只留了活值兑底；不发消息就永不触发
            // ListDirty→refresh_sessions，必须这里补一次 sync 治愈
            chat.sync_primary_tab_label();
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
                    // 空 tail 不入缓存（pi-flash-9eck）：空投影会毒化打开路径
                    // ——每次重开都复用空尾窗、永远卡「载入中」等 RPC 放行。
                    if !msgs.is_empty() {
                        map.insert(path, msgs);
                    }
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
