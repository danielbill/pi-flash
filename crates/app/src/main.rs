//! pi-flash — desktop shell for the pi coding agent.
//!
//! v54 shell: topbar 两段（左=收放钮 chrome / 右=内容 tabs+设置+窗口控制）·
//! psp 项目+会话一体列表 · 内容区 chat/term/md 状态机 · statusbar 仅面板段。
//! Layout values come from docs/UI设计/主界面UI设计-2.html (mist tokens).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use futures::StreamExt;
use gpui::{
    App, Application, Context, FocusHandle, Focusable, KeyDownEvent,
    MouseButton, ParentElement,
    Render, SharedString, Styled, WindowOptions, div, prelude::*, px, rgb,
};
use pi_link::protocol::Command;
use pi_link::sessions::{SessionInfo, list_sessions, list_sessions_for_cwd, read_tail_messages};

mod actions_dialogs;
mod agent_session;
mod actions_menu;
mod actions_panels;
mod actions_rename;
mod actions_runtime;
mod actions_sessions;
mod actions_terminal;
mod appearance;
mod assets;
mod content;
mod dialogs;
mod ext_ui;
mod ext_ui_actions;
mod function_panel;
mod i18n;
mod markdown;
mod models_config;
mod theme;
mod services;
mod session;
mod settings;
mod status_bar;
mod titlebar;
mod terminal;
mod ui;
mod webview;
pub(crate) use ui::editor_input::EditorInputElement;
use i18n::tr;
use models_config::EnabledState;
use theme::theme as T;
use services::format::*;
use services::git::*;
use services::workspace::*;
use session::messages::{Msg, msgs_from_tail};
use session::runtime::SessionRuntime;
use terminal::{TermStatus, TerminalTab};
use ui::TextInput;
use ui::icon;
use ext_ui::render_ext_dialog;

static T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
static PERF: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

// ---------------------------------------------------------------------------
// state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Dialog {
    ModelSelect { input: gpui::Entity<TextInput> },
    GitDiff { path: PathBuf, patch: String },
    SessionSearch { input: gpui::Entity<TextInput> },
}

/// functionPanel active view (statusbar 三 tab): mutually exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DockPanel {
    Sessions,
    Files,
    Git,
}

impl DockPanel {
    fn as_str(&self) -> &'static str {
        match self {
            DockPanel::Sessions => "sessions",
            DockPanel::Files => "files",
            DockPanel::Git => "git",
        }
    }
}

/// 内容区视图状态机 (v54): chat 默认；terminal / markdown 预览以 topbar tab 打开。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContentView {
    Chat,
    Term,
    /// 文件查看 tab（PanelTab::File；md/html 渲染、其余源码）
    File,
}

/// psp 列表方式（⋯ 菜单）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListMode {
    Grouped,
    Flat,
}

/// psp 排序方式（⋯ 菜单；手动=初始序，真拖拽待实现）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SortMode {
    Time,
    Manual,
}

/// One project group of the psp (sessions sorted mtime desc at build).
#[derive(Debug, Clone)]
pub(crate) struct ProjectGroup {
    pub name: String,
    pub path: PathBuf,
    pub sessions: Vec<SessionInfo>,
}

/// 会话 hover 详情卡状态（300ms 离行宽限 + 进卡取消隐藏）。
#[derive(Debug, Clone)]
pub(crate) struct HoverCard {
    pub path: PathBuf,
    pub y: f32,
    pub hide_at: Option<std::time::Instant>,
    pub confirming: bool,
    /// 鼠标当前是否在卡上（gpui 的行退出/卡进入事件顺序不保证，
    /// 行退出只在 !card_hovered 时才启动消失宽限）
    pub card_hovered: bool,
    /// 标题点击后原地改名（设计稿：卡内变输入框，Enter 提交 / Esc 取消）
    pub renaming: bool,
    pub rename_input: Option<gpui::Entity<crate::ui::TextInput>>,
}

/// psp 菜单（⋯ 排序两级 / 项目菜单）；(x, y) 为事件坐标锚点。
#[derive(Debug, Clone)]
pub(crate) enum PspMenu {
    /// sub: 0=列表方式, 1=排序方式
    Sort { sub: Option<u8>, x: f32, y: f32 },
    Project { path: PathBuf, x: f32, y: f32 },
}

#[derive(Debug, Clone)]
struct AttachedImage {
    name: String,
    data_b64: String,
    mime: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum MenuKind {
    Slash,
    At,
}

#[derive(Debug, Clone)]
struct MenuItem {
    insert: String,
}

struct Chat {
    focus: FocusHandle,
    dialog_focus: FocusHandle,
    dialog: Option<Dialog>,
    dock_panel: DockPanel,
    input: String,
    pending_images: Vec<AttachedImage>,
    history: Vec<String>,
    history_ix: Option<usize>,
    session_tail_cache: std::collections::HashMap<PathBuf, Vec<Msg>>,
    sessions: Vec<SessionInfo>,
    cwd: PathBuf,
    branch: String,
    available_models: Vec<pi_link::protocol::ModelInfo>,
    runtimes: std::collections::HashMap<String, gpui::Entity<session::runtime::SessionRuntime>>,
    active_key: String,
    draft_seq: usize,
    menu_ix: usize,
    term_events: Option<futures::channel::mpsc::UnboundedSender<(usize, alacritty_terminal::event::Event)>>,
    op_tx: Option<futures::channel::mpsc::UnboundedSender<String>>,
    // editor view state
    ime_marked: Option<std::ops::Range<usize>>,
    caret_on: bool,
    input_focused: bool,
    // inline rename (active session)
    renaming: Option<PathBuf>,
    rename_input: Option<gpui::Entity<TextInput>>,
    confirm_delete: Option<PathBuf>,
    // session content search (013): dialog query input + background results
    search_hits: Vec<pi_link::sessions::SearchHit>,
    search_truncated: bool,
    search_running: bool,
    search_needle: String,
    search_gen: u64,
    pending_locate: Option<(PathBuf, Option<i64>, String)>,
    // shell surfaces
    pill_menu: Option<PillMenu>,
    // git panel
    git_files: Vec<GitFile>,
    git_add_del: (u64, u64),
    git_selected: Option<PathBuf>,
    git_tab: function_panel::git_panel::GitTab,
    git_log: Vec<GitCommit>,
    git_error: Option<String>,
    git_commit_input: gpui::Entity<TextInput>,
    // workspace / project files
    project_files: Vec<String>,
    expanded_dirs: HashSet<PathBuf>,
    file_cache: std::collections::HashMap<PathBuf, FileTab>,
    // terminals (content-area tabs)
    terminals: Vec<TerminalTab>,
    active_terminal: Option<usize>,
    term_seq: usize,
    panel_tabs: Vec<PanelTab>,
    active_panel_tab: Option<usize>,
    // settings panel data
    mc_patterns: Option<Vec<String>>,
    mc_state: EnabledState,
    mc_creds: Vec<(String, pi_link::config::CredentialKind)>,
    mc_project_scope: bool,
    mc_skills: Vec<pi_link::skills::SkillEntry>,
    mc_pkgs_global: Vec<serde_json::Value>,
    mc_pkgs_project: Vec<serde_json::Value>,
    mc_default_tools: Option<Vec<String>>,
    // extension UI surface (active session)
    ext_status: Vec<(String, String)>,
    ext_widgets: Vec<(String, Vec<String>, bool)>,
    ext_dialog: Option<pi_link::protocol::ExtensionUiRequest>,
    ext_input: gpui::Entity<TextInput>,
    ext_notice: Option<(String, u8)>,
    // subagent test runs (settings panel)
    sa_profiles: Vec<pi_link::subagents::SubagentProfile>,
    sa_settings: pi_link::subagents::SubagentSettings,
    sa_runs: Vec<SubagentRun>,
    sa_run_seq: usize,
    sound_on: bool,
    settings: Option<gpui::Entity<settings::SettingsPanel>>,
    // ---- v54 shell state ----
    /// psp 项目组（当前项目钉顶，其余按最近会话倒序；上限=设置.默认加载项目数）
    projects: Vec<ProjectGroup>,
    /// 当前活跃会话文件（psp 选中态；switch_to 时更新）
    active_file: Option<PathBuf>,
    /// 正在运行/流式中的会话文件集合（psp 旋转圈；Changed 事件维护）
    running_files: std::collections::HashSet<PathBuf>,
    list_mode: ListMode,
    sort_mode: SortMode,
    collapsed_keys: HashSet<String>,
    slp_w: f32,
    panes_hidden: bool,
    slp_drag: Option<(f32, f32)>,
    content_view: ContentView,
    /// 浏览操作区的最后视图（Term/File）：文件树标签点击时恢复
    browse_last: ContentView,
    /// 应用内 HTML 渲染面板（wry/WebView2 子窗口；html 文件 tab 激活时显示）
    html_panel: Option<webview::HtmlPanel>,
    /// html_panel 当前加载的文件
    html_panel_path: Option<PathBuf>,
    /// render 期写的期望状态（pump 消费；wry 调用禁止在 render 借用内）
    html_want: Option<(PathBuf, String)>,
    html_geo: webview::HtmlPanelGeo,
    /// 主窗口句柄（pump 创建 webview 需在其 window 上下文中执行）
    main_window: Option<gpui::AnyWindowHandle>,
    /// 文件查看视图的滚动（滚动条渲染数据源）
    file_scroll: gpui::ScrollHandle,
    file_scrollbar: gpui_component::scroll::ScrollbarState,
    nav_open: bool,
    nav_hide_at: Option<std::time::Instant>,
    nav_flyout_hovered: bool,
    /// flyout 内鼠标所在轮（选择框/比例尺亮点跟随鼠标）
    nav_hover_turn: Option<usize>,
    unread: HashSet<PathBuf>,
    hovered_project: Option<usize>,
    proj_tip: Option<(PathBuf, f32, f32)>,
    hover_card: Option<HoverCard>,
    psp_menu: Option<PspMenu>,
    confirm_prj_del: Option<(PathBuf, f32, f32)>,
    status_toast: Option<(String, std::time::Instant)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum PillMenu {
    Thinking,
    Tools,
}

/// One content-area tab: a terminal session or a file viewer.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PanelTab {
    Term(usize),
    File(PathBuf),
}

/// Cached file content for a viewer tab (read once on open).
struct FileTab {
    content: String,
}

/// One live subagent run (child RPC session spawned with profile flags).
struct SubagentRun {
    id: usize,
    profile: String,
    status: u8,
    last_text: String,
    session: Option<pi_link::client::PiSession>,
}

impl Chat {
    fn new(cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let dialog_focus = cx.focus_handle();
        let cwd = std::env::var("PI_FLASH_CWD")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        // startup restore decides workspace + last session BEFORE the first
        // spawn — exactly one pi process (ARCHITECTURE.md §4, was two)
        let last_ws = if startup_restore() { get_last_workspace() } else { None };
        let target_ws = last_ws.unwrap_or_else(|| cwd.to_string_lossy().to_string());
        let cwd = if !same_ws(&target_ws, &cwd.to_string_lossy()) {
            let ws_path = PathBuf::from(&target_ws);
            if ws_path.is_dir() { ws_path } else { cwd }
        } else {
            cwd
        };
        let branch = read_branch(&cwd);
        let last_open = if startup_restore() {
            get_last_open(&cwd.to_string_lossy())
                .map(PathBuf::from)
                .filter(|p| p.exists())
        } else {
            None
        };
        let ui = ui_state();

        let mut chat = Self {
            focus,
            dialog_focus,
            dialog: None,
            input: String::new(),
            pending_images: Vec::new(),
            session_tail_cache: std::collections::HashMap::new(),
            sessions: Vec::new(),
            search_hits: Vec::new(),
            search_truncated: false,
            search_running: false,
            search_needle: String::new(),
            search_gen: 0,
            pending_locate: None,
            // v54.4 定案：启动固定会话界面（面板/内容区绑定关系见
            // status_bar；不恢复上次离开时的面板）
            dock_panel: DockPanel::Sessions,
            cwd: cwd.clone(),
            branch,
            runtimes: std::collections::HashMap::new(),
            active_key: String::new(),
            draft_seq: 0,
            available_models: Vec::new(),
            expanded_dirs: HashSet::new(),
            git_files: Vec::new(),
            git_selected: None,
            git_tab: function_panel::git_panel::GitTab::Changes,
            git_log: Vec::new(),
            git_error: None,
            git_commit_input: {
                let input = cx.new(|cx| TextInput::new(cx).placeholder(tr("提交信息（提交已暂存更改）")));
                input
            },
            git_add_del: (0, 0),
            project_files: Vec::new(),
            history: Vec::new(),
            history_ix: None,
            menu_ix: 0,
            terminals: Vec::new(),
            active_terminal: None,
            term_seq: 0,
            term_events: None,
            mc_patterns: None,
            mc_state: EnabledState::default(),
            mc_creds: Vec::new(),
            mc_project_scope: false,
            mc_skills: Vec::new(),
            mc_pkgs_global: Vec::new(),
            mc_pkgs_project: Vec::new(),
            mc_default_tools: None,
            op_tx: None,
            ext_status: Vec::new(),
            ext_widgets: Vec::new(),
            ext_dialog: None,
            ext_input: cx.new(|cx| TextInput::new(cx)),
            ext_notice: None,
            sa_profiles: Vec::new(),
            sa_settings: pi_link::subagents::SubagentSettings::default(),
            sa_runs: Vec::new(),
            sa_run_seq: 0,
            panel_tabs: Vec::new(),
            active_panel_tab: None,
            file_cache: std::collections::HashMap::new(),
            caret_on: true,
            input_focused: false,
            pill_menu: None,
            sound_on: load_sound_pref(),
            ime_marked: None,
            settings: None,
            renaming: None,
            rename_input: None,
            confirm_delete: None,
            projects: Vec::new(),
            active_file: last_open.clone(),
            running_files: std::collections::HashSet::new(),
            list_mode: if ui.list_mode == "flat" { ListMode::Flat } else { ListMode::Grouped },
            sort_mode: if ui.sort_mode == "manual" { SortMode::Manual } else { SortMode::Time },
            collapsed_keys: ui.collapsed.iter().cloned().collect(),
            slp_w: ui.slp_w,
            panes_hidden: ui.panes_hidden,
            slp_drag: None,
            content_view: ContentView::Chat,
            browse_last: ContentView::Term,
            html_panel: None,
            html_panel_path: None,
            html_want: None,
            html_geo: webview::HtmlPanelGeo {
                x: 0., y: 0., w: 800., h: 600.,
            },
            main_window: None,
            file_scroll: gpui::ScrollHandle::new(),
            file_scrollbar: gpui_component::scroll::ScrollbarState::default(),
            nav_open: false,
            nav_hide_at: None,
            nav_flyout_hovered: false,
            nav_hover_turn: None,
            unread: HashSet::new(),
            hovered_project: None,
            proj_tip: None,
            hover_card: None,
            psp_menu: None,
            confirm_prj_del: None,
            status_toast: None,
        };
        // wire input callbacks that need the root entity handle
        let weak_ext = cx.entity().downgrade();
        chat.ext_input.update(cx, |ti, _| {
            ti.set_on_submit(Box::new(move |v, cx| {
                let _ = weak_ext.update(cx, |c, cx| {
                    c.ext_respond(Some(v.to_string()), None, false, cx);
                });
            }));
        });
        chat.load_project_files();
        chat.refresh_git();

        // caret blink pump (2 Hz toggle; repaint only while the editor is
        // focused — input_focused is refreshed every render). Also expires
        // the psp hover card (300ms grace) and status toasts (2.5s).
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(120))
                    .await;
                // html webview 创建：三段式（chat 取任务 → window 上下文
                // 创建 wry → chat 写回），任一段都不与另一段的借用嵌套
                // （wry 创建会同步分发 Win32 消息，render/chat 借用期内
                // 执行会重入 panic）
                let job = this
                    .update(cx, |c, _cx| c.take_html_job())
                    .ok()
                    .flatten();
                if let Some((path, html, geo)) = job {
                    let handle = this
                        .update(cx, |c, _cx| c.main_window)
                        .ok()
                        .flatten();
                    if let Some(h) = handle {
                        let created = h.update(cx, |_, window, _| {
                            wry::WebView::new_as_child(
                                window,
                                wry::WebViewAttributes::default(),
                            )
                            .map(|w| {
                                let _ = w.set_bounds(geo.rect());
                                let _ = w.set_visible(true);
                                w
                            })
                        });
                        match created {
                            Ok(Ok(w)) => {
                                let _ = this.update(cx, |c, _cx| {
                                    c.html_panel =
                                        Some(webview::HtmlPanel::from_webview(w, path.clone()));
                                    c.html_panel_path = Some(path);
                                });
                            }
                            _ => eprintln!("[webview] create failed"),
                        }
                    }
                }
                let ok = this
                    .update(cx, |c, cx| {
                        c.caret_on = !c.caret_on;
                        let mut dirty = c.input_focused;
                        // 改名中：不打字时鼠标虽不在卡上，也不能清卡
                        let renaming = c
                            .hover_card
                            .as_ref()
                            .is_some_and(|h| h.renaming);
                        if !renaming {
                            if let Some(at) =
                                c.hover_card.as_ref().and_then(|h| h.hide_at)
                            {
                                if at.elapsed()
                                    > std::time::Duration::from_millis(300)
                                {
                                    c.hover_card = None;
                                    dirty = true;
                                }
                            }
                        }
                        c.sync_html_panel();
                        // 导航 flyout 250ms 离开宽限（pi-web
                        // PREVIEW_HIDE_DELAY parity）
                        if c.nav_open {
                            if let Some(at) = c.nav_hide_at {
                                if at.elapsed()
                                    > std::time::Duration::from_millis(250)
                                {
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
        // settings panel CLI op pump (pi install/remove runs in background)
        let (op_tx, mut op_rx) = futures::channel::mpsc::unbounded::<String>();
        chat.op_tx = Some(op_tx);
        cx.spawn(async move |this, cx| {
            while let Some(msg) = op_rx.next().await {
                if this
                    .update(cx, |chat, cx| {
                        chat.set_status(msg, cx);
                        chat.reload_settings_panel();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        // terminal event pump: alacritty EventListener -> Chat (single channel)
        let (term_tx, term_rx) =
            futures::channel::mpsc::unbounded::<(usize, alacritty_terminal::event::Event)>();
        chat.term_events = Some(term_tx);
        cx.spawn(async move |this, cx| {
            let mut rx = term_rx;
            while let Some((tab_id, event)) = rx.next().await {
                if this
                    .update(cx, |chat, cx| chat.on_term_event(tab_id, event, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        // models panel state (enabledModels whitelist + credentials) for the
        // picker filter — loaded once at startup, refreshed when opened
        chat.reload_settings_panel();
        // initial runtime: restore the last session (disk-direct tail) or a
        // lazy draft. The process spawns AFTER first paint.
        let rt_key = last_open
            .as_deref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "draft-0".to_string());
        let rt = cx.new(|_| {
            let mut r = session::runtime::SessionRuntime::new(
                rt_key.clone(),
                cwd.clone(),
                last_open.clone(),
            );
            if let Some(path) = &last_open {
                r.messages = msgs_from_tail(read_tail_messages(path, 256 * 1024, 100));
                r.list.reset(r.messages.len());
                r.status = "resuming".into();
            }
            r
        });
        chat.draft_seq = if last_open.is_some() { 1 } else { 0 };
        chat.runtimes.insert(rt_key.clone(), rt.clone());
        chat.active_key = rt_key.clone();
        chat.subscribe_runtime(&rt, cx);
        // background process attach (no process kill involved; spawn is a
        // one-time ~100ms process creation off the first frame)
        let rt_for_spawn = rt.clone();
        cx.spawn(async move |_this, cx| {
            let _ = rt_for_spawn.update(cx, |r, cx| {
                if r.agent.session.is_none() {
                    if let Some(rx) = r.spawn() {
                        let epoch = r.agent.epoch;
                        session::runtime::SessionRuntime::attach_pump(&rt_for_spawn, rx, epoch, cx);
                    }
                }
                if let Some(s) = &r.agent.session {
                    let _ = s.send(&Command::GetMessages);
                    let _ = s.send(&Command::GetTree);
                }
                r.refresh_state();
            });
        })
        .detach();
        if PERF.load(std::sync::atomic::Ordering::Relaxed) {
            if let Some(t0) = T0.get() {
                eprintln!("[perf] last-session tail rendered: {:?}", t0.elapsed());
            }
        }
        // git panel: Enter in the commit box commits staged changes
        let entity_for_git = cx.entity();
        chat.git_commit_input.update(cx, |ti, _| {
            let weak_git = entity_for_git.downgrade();
            ti.set_on_submit(Box::new(move |_, cx| {
                let _ = weak_git.update(cx, |c, cx| c.git_commit_staged(cx));
            }));
        });
        // idle recycle (pi-web idle-timeout parity): every 60s, kill the
        // process of any non-active session idle >10min.
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
                            !r.agent_running
                                && r.last_activity.elapsed() > idle_cap
                                && r.agent.session.is_some()
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
        // background fill: cross-project scan -> psp 项目组（startup budget
        // §4 — the first frame renders the empty shell while this lands）
        let cwd_text = chat.cwd.to_string_lossy().to_string();
        cx.spawn(async move |this, cx| {
            let sessions = list_sessions_for_cwd(&cwd_text, 100)
                .into_iter()
                .filter(|s| same_ws(&s.cwd, &cwd_text))
                .collect::<Vec<_>>();
            let _ = this.update(cx, |chat, cx| {
                chat.sessions = sessions;
                cx.notify();
                if PERF.load(std::sync::atomic::Ordering::Relaxed) {
                    if let Some(t0) = T0.get() {
                        eprintln!("[perf] session list: {:?}", t0.elapsed());
                    }
                }
            });
            // psp 一体列表: all projects (grouped), capped by settings
            let all = cx
                .background_spawn(async move {
                    let mut all = list_sessions(400);
                    all.sort_by(|a, b| b.modified.cmp(&a.modified));
                    all
                })
                .await;
            let _ = this.update(cx, |chat, cx| {
                chat.rebuild_projects(all);
                cx.notify();
            });
            // cross-project tail preload (startup §4)
            let n = preload_sessions();
            if n > 0 {
                let active = last_open.clone();
                let preloaded = cx
                    .background_spawn(async move {
                        let mut map = std::collections::HashMap::new();
                        for s in list_sessions(n) {
                            if map.len() >= n {
                                break;
                            }
                            if Some(&s.path) == active.as_ref() {
                                continue;
                            }
                            let msgs =
                                msgs_from_tail(read_tail_messages(&s.path, 256 * 1024, 100));
                            map.insert(s.path, msgs);
                        }
                        map
                    })
                    .await;
                let _ = this.update(cx, |chat, _cx| {
                    chat.session_tail_cache = preloaded;
                });
            }
        })
        .detach();
        chat
    }

    fn refresh_state(&self, cx: &mut gpui::App) {
        self.rt().update(cx, |r, _| r.refresh_state());
    }

    fn refresh_sessions(&mut self) {
        let cwd = self.cwd.to_string_lossy().to_string();
        self.sessions = list_sessions_for_cwd(&cwd, 100)
            .into_iter()
            .filter(|s| same_ws(&s.cwd, &cwd))
            .collect();
        // sync the current project's group, then re-pin project order
        let all: Vec<SessionInfo> = self
            .projects
            .iter()
            .flat_map(|g| {
                if same_ws(&g.path.to_string_lossy(), &cwd) {
                    self.sessions.clone()
                } else {
                    g.sessions.clone()
                }
            })
            .collect();
        self.rebuild_projects(all);
    }

    fn refresh_git(&mut self) {
        self.git_files = git_status_files(&self.cwd);
        self.git_add_del = git_numstat(&self.cwd);
    }

    // -----------------------------------------------------------------------
    // built-in terminal (pi-web TerminalPanel parity)
    // -----------------------------------------------------------------------

    fn on_term_event(
        &mut self,
        tab_id: usize,
        event: alacritty_terminal::event::Event,
        cx: &mut Context<Self>,
    ) {
        use alacritty_terminal::event_loop::Msg;
        let Some(tab) = self.terminals.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        match event {
            alacritty_terminal::event::Event::Wakeup => {
                cx.notify();
            }
            alacritty_terminal::event::Event::PtyWrite(s) => {
                let _ = tab.pty.send(Msg::Input(s.into_bytes().into()));
            }
            alacritty_terminal::event::Event::ClipboardStore(_, text) => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            }
            alacritty_terminal::event::Event::ClipboardLoad(_, fmt) => {
                let text = cx
                    .read_from_clipboard()
                    .and_then(|i| i.text())
                    .unwrap_or_default();
                let _ = tab.pty.send(Msg::Input(fmt(&text).into_bytes().into()));
            }
            alacritty_terminal::event::Event::ColorRequest(i, fmt) => {
                let _ = tab
                    .pty
                    .send(Msg::Input(fmt(terminal::term_rgb(i)).into_bytes().into()));
            }
            alacritty_terminal::event::Event::TextAreaSizeRequest(fmt) => {
                let ws = alacritty_terminal::event::WindowSize {
                    num_cols: tab.cols as u16,
                    num_lines: tab.rows as u16,
                    cell_width: tab.cell_w as u16,
                    cell_height: tab.line_h as u16,
                };
                let _ = tab.pty.send(Msg::Input(fmt(ws).into_bytes().into()));
            }
            alacritty_terminal::event::Event::ChildExit(status) => {
                tab.status = TermStatus::Exited(status.code());
                cx.notify();
            }
            alacritty_terminal::event::Event::Exit => {
                if tab.status == TermStatus::Ready {
                    tab.status = TermStatus::Exited(None);
                }
                cx.notify();
            }
            _ => {}
        }
    }

    fn switch_project(&mut self, cwd: PathBuf, cx: &mut Context<Self>) {
        self.cwd = cwd;
        // mark as the globally-last active workspace for startup restore
        set_last_workspace(&self.cwd.to_string_lossy());
        self.branch = read_branch(&self.cwd);
        self.refresh_sessions();
        self.dialog = None;
        self.expanded_dirs.clear();
        self.refresh_git();
        self.load_project_files();
        if let Some(p) = get_last_open(&self.cwd.to_string_lossy()) {
            let path = PathBuf::from(&p);
            if path.exists() {
                self.open_session(path, false, cx);
                cx.notify();
                return;
            }
        }
        self.new_session(cx);
        cx.notify();
    }

    fn with_active_editor<Act: FnOnce(&mut SessionRuntime, &mut Context<SessionRuntime>)>(
        &mut self,
        cx: &mut Context<Self>,
        act: Act,
    ) {
        let rt = self.rt();
        let (input, images, history) = (
            self.input.clone(),
            self.pending_images.clone(),
            self.history.clone(),
        );
        rt.update(cx, |r, cx| {
            r.input = input;
            r.pending_images = images;
            r.history = history;
            act(r, cx);
        });
        // pull post-action state back (history push, input clear)
        let (input, images, history) = {
            let r = rt.read(cx);
            (r.input.clone(), r.pending_images.clone(), r.history.clone())
        };
        self.input = input;
        self.pending_images = images;
        self.history = history;
        self.history_ix = None;
        cx.notify();
    }

    fn send_input(&mut self, cx: &mut Context<Self>) {
        self.with_active_editor(cx, |r, cx| r.send_input(cx));
    }

    fn steer_input(&mut self, cx: &mut Context<Self>) {
        self.with_active_editor(cx, |r, cx| r.steer_input(cx));
    }

    fn follow_up_input(&mut self, cx: &mut Context<Self>) {
        self.with_active_editor(cx, |r, cx| r.follow_up_input(cx));
    }

    fn abort_stream(&mut self, cx: &mut Context<Self>) {
        self.rt().update(cx, |r, cx| r.abort_stream(cx));
        cx.notify();
    }

    fn set_thinking_level(&mut self, key: &str, cx: &mut Context<Self>) {
        self.rt().update(cx, |r, cx| r.set_thinking_level(key, cx));
    }

    fn mc_set_tools_preset(&mut self, key: &str, cx: &mut Context<Self>) {
        let rt = self.rt();
        rt.update(cx, |r, cx| {
            r.tools_preset = key.to_string();
            if let Some(rx) = r.spawn() {
                let epoch = r.agent.epoch;
                session::runtime::SessionRuntime::attach_pump(&rt, rx, epoch, cx);
            }
            if let Some(s) = &r.agent.session {
                let _ = s.send(&Command::GetMessages);
                let _ = s.send(&Command::GetTree);
            }
            r.refresh_state();
            r.status = crate::i18n::tf("工具预设: {k} (会话进程已重绑)", &[("k", key.to_string())]);
            cx.emit(session::runtime::SessionEvent::Changed);
        });
        cx.notify();
    }

    fn rt(&self) -> gpui::Entity<session::runtime::SessionRuntime> {
        self.runtimes
            .get(&self.active_key)
            .expect("active runtime exists")
            .clone()
    }

    fn set_status(&mut self, msg: String, cx: &mut Context<Self>) {
        self.status_toast = Some((msg, std::time::Instant::now()));
        self.rt().update(cx, |r, _| r.status = String::new());
        let _ = cx;
    }

    fn load_project_files(&mut self) {
        self.project_files = walk_files(&self.cwd, 3, 400);
    }

    /// 当前激活的文件 tab（PanelTab::File），无则 None
    fn active_file_tab(&self) -> Option<PathBuf> {
        self.active_panel_tab
            .and_then(|ix| self.panel_tabs.get(ix))
            .and_then(|t| match t {
                PanelTab::File(p) => Some(p.clone()),
                _ => None,
            })
    }

    /// render 期只写期望状态（geo/want/hwnd），wry 实际调用在 pump
    /// （borrow 外）执行——wry 会同步分发 Win32 消息，render 内调用会
    /// 重入借用 panic。
    pub(crate) fn stage_html_panel(&mut self, window: &mut gpui::Window) {
        self.html_geo = webview::HtmlPanelGeo::new(self, window);
        self.main_window = Some(window.window_handle());
        self.html_want = self
            .active_file_tab()
            .filter(|_| self.content_view == ContentView::File)
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "html" | "htm"))
            })
            .and_then(|p| {
                self.file_cache.get(&p).map(|f| (p.clone(), f.content.clone()))
            });
    }

    /// pump 期取走待创建任务（无 render 借用）。创建需要 Window
    /// （HasWindowHandle），由调用方在 AnyWindowHandle::update 里执行。
    pub(crate) fn take_html_job(
        &mut self,
    ) -> Option<(PathBuf, String, webview::HtmlPanelGeo)> {
        let want = self.html_want.take();
        if let Some((path, html)) = want {
            if self.html_panel.is_none() {
                return Some((path, html, self.html_geo));
            }
            self.html_want = Some((path, html));
        }
        None
    }

    /// pump 期执行（无 render 借用）：差量驱动 load/bounds/visible。
    pub(crate) fn sync_html_panel(&mut self) {
        if let Some(panel) = self.html_panel.as_mut() {
            let visible = self.html_want.is_some()
                && matches!(self.content_view, ContentView::File);
            if visible {
                panel.set_bounds(&self.html_geo);
            }
            panel.set_visible(visible);
        }
    }

    /// 切内容区视图；落在浏览操作区（Term/Md）时记住，供文件树标签恢复
    pub(crate) fn set_content_view(&mut self, v: ContentView) {
        self.content_view = v;
        if matches!(v, ContentView::Term | ContentView::File) {
            self.browse_last = v;
        }
    }
}

impl Focusable for Chat {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

// ---------------------------------------------------------------------------
// rendering
// ---------------------------------------------------------------------------

impl Render for Chat {
    fn render(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        static FIRST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if PERF.load(std::sync::atomic::Ordering::Relaxed)
            && !FIRST.swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            if let Some(t0) = T0.get() {
                eprintln!("[perf] first frame: {:?}", t0.elapsed());
            }
        }
        // keep terminal focus alive across frames (render focuses chat input
        // otherwise, which would steal it back every redraw)
        let dialog_input = match &self.dialog {
            Some(Dialog::ModelSelect { input }) | Some(Dialog::SessionSearch { input }) => {
                Some(input.clone())
            }
            _ => None,
        };
        // 详情卡的原地改名输入框也要持有焦点——否则每帧的焦点回收
        // 会把它抢回主输入框，键盘输入进不去
        let card_rename_focus = self
            .hover_card
            .as_ref()
            .and_then(|c| c.rename_input.clone());
        let rename_focus = self.rename_input.clone().or(card_rename_focus);
        if let Some(input) = rename_focus.or(dialog_input) {
            let handle = input.read(cx).focus_handle_in(cx);
            if !handle.is_focused(window) {
                window.focus(&handle);
            }
        } else if self.ext_dialog.is_some() {
            let ext_text = matches!(
                self.ext_dialog.as_ref().map(|r| &r.method),
                Some(
                    pi_link::protocol::ExtUiMethod::Input { .. }
                        | pi_link::protocol::ExtUiMethod::Editor { .. }
                )
            );
            let handle = if ext_text {
                self.ext_input.read(cx).focus_handle()
            } else {
                self.dialog_focus.clone()
            };
            if !handle.is_focused(window) {
                window.focus(&handle);
            }
        } else if self.dialog.is_some() {
            if !self.dialog_focus.is_focused(window) {
                window.focus(&self.dialog_focus);
            }
        } else if let Some(panel) = self.settings.as_ref() {
            let p = panel.read(cx);
            let inner_focused = p.focus.is_focused(window)
                || p.key_input.read(cx).focus_handle_in(cx).is_focused(window)
                || p.install_input.read(cx).focus_handle_in(cx).is_focused(window)
                || p.sa_input.read(cx).focus_handle_in(cx).is_focused(window);
            if !inner_focused && !self.dialog_focus.is_focused(window) {
                window.focus(&self.dialog_focus);
            }
        } else if !self.terminals.iter().any(|t| t.focus.is_focused(window)) {
            window.focus(&self.focus);
        }
        let t = T();

        let entity = cx.entity();
        let weak = entity.downgrade();
        let weak_for_dialog = weak.clone();

        // popup menu overlay anchored above the composer controls row
        let (thinking_override, preset_key) = {
            let rt = self.rt();
            let r = rt.read(cx);
            (r.thinking_override.clone(), r.tools_preset.clone())
        };
        let pill_menu_el = self.pill_menu.map(|menu| {
            let weak_menu = weak.clone();
            let rows: Vec<(String, String, bool)> = match menu {
                PillMenu::Thinking => [
                    ("auto", tr("使用 pi 默认设置"), thinking_override.is_none()),
                    ("low", tr("低强度推理"), thinking_override.as_deref() == Some("low")),
                    ("high", tr("高强度推理"), thinking_override.as_deref() == Some("high")),
                    ("max", tr("最强推理"), thinking_override.as_deref() == Some("max")),
                ]
                .iter()
                .map(|(k, d, on)| (k.to_string(), d.to_string(), *on))
                .collect(),
                PillMenu::Tools => [
                    ("configured", tr("取自 settings.json 的 defaultTools"), preset_key == "configured"),
                    ("chat-only", tr("仅聊天"), preset_key == "chat-only"),
                    ("read-only", tr("4 个只读内置工具"), preset_key == "read-only"),
                    ("default", tr("4 个内置工具"), preset_key == "default"),
                    ("full", tr("全部内置工具"), preset_key == "full"),
                ]
                .iter()
                .map(|(k, d, on)| (k.to_string(), d.to_string(), *on))
                .collect(),
            };
            let is_thinking = menu == PillMenu::Thinking;
            let items = rows
                .into_iter()
                .map(|(key, desc, active)| {
                    let weak_row = weak_menu.clone();
                    let key_for_rpc = key.clone();
                    div()
                        .id(SharedString::from(format!(
                            "pm-{}-{}",
                            if is_thinking { "t" } else { "w" },
                            key
                        )))
                        .min_h(px(40.))
                        .px_3()
                        .py_2()
                        .flex()
                        .items_center()
                        .gap_2()
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(t.bg_hover)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            cx.stop_propagation();
                            let _ = weak_row.update(cx, |c, cx| {
                                c.pill_menu = None;
                                if is_thinking {
                                    c.set_thinking_level(&key_for_rpc, cx);
                                } else {
                                    c.mc_set_tools_preset(&key_for_rpc, cx);
                                }
                            });
                        })
                        .child(
                            div()
                                .w(px(16.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .children(active.then(|| icon("check", 13., t.accent))),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(if active {
                                    gpui::FontWeight::SEMIBOLD
                                } else {
                                    gpui::FontWeight::NORMAL
                                })
                                .text_color(rgb(t.text))
                                .child(SharedString::from(key)),
                        )
                        .child(
                            div()
                                .ml_auto()
                                .text_size(px(11.))
                                .text_color(rgb(t.text_dim))
                                .child(desc),
                        )
                        .into_any_element()
                })
                .collect::<Vec<_>>();
            div()
                .absolute()
                .inset_0()
                .occlude()
                .child(
                    // transparent backdrop: click anywhere closes the menu
                    div()
                        .size_full()
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, {
                            let weak = weak_menu.clone();
                            move |_, _, cx| {
                                let _ = weak.update(cx, |c, cx| {
                                    if c.pill_menu.is_some() {
                                        c.pill_menu = None;
                                        cx.notify();
                                    }
                                });
                            }
                        }),
                )
                .child(
                    div()
                        .absolute()
                        .bottom(px(64.))
                        .right(px(24.))
                        .min_w(px(320.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(rgb(t.border))
                        .bg(rgb(t.bg))
                        .shadow_lg()
                        .overflow_hidden()
                        .flex()
                        .flex_col()
                        .children(items),
                )
                .into_any_element()
        });

        // ---- v54 body: panel-col (topbar-l + dock + statusbar) | content-col
        let entity_for_body = entity.clone();
        let weak_for_body = weak.clone();
        let panes_hidden = self.panes_hidden;
        let slp_dragging = self.slp_drag.is_some();
        let mut body = div()
            .id("app-body")
            .flex_1()
            .min_h_0()
            .relative()
            .flex()
            // slp 宽度拖拽（resizer 按下时在此跟踪）
            .on_mouse_move(move |ev: &gpui::MouseMoveEvent, _, cx| {
                let _ = weak_for_body.update(cx, |c, cx| {
                    if let Some((start_x, start_w)) = c.slp_drag {
                        c.slp_w = (start_w + f32::from(ev.position.x) - start_x).clamp(250., 500.);
                        cx.notify();
                    }
                });
            })
            .on_mouse_up(
                MouseButton::Left,
                {
                    let weak = weak.clone();
                    move |_, _, cx| {
                        let _ = weak.update(cx, |c, cx| {
                            if c.slp_drag.take().is_some() {
                                c.persist_ui();
                                cx.notify();
                            }
                        });
                    }
                },
            );
        if !panes_hidden {
            body = body.child(
                div()
                    .id("panel-col")
                    .w(px(self.slp_w))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .bg(rgb(t.chrome))
                    .child(titlebar::topbar_l(self, cx))
                    .child(function_panel::dock(
                        self,
                        entity_for_body.clone(),
                        &weak,
                        cx,
                    ))
                    .child(status_bar::control_bar(self, cx)),
            );
        }
        body = body.child(
            div()
                .id("content-col")
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .relative()
                .bg(rgb(t.bg))
                .when(!panes_hidden && !slp_dragging, |d| {
                    d.border_l_1().border_color(gpui::rgba(0xafc4ba99))
                })
                .child(titlebar::topbar_r(self, window, cx))
                .child(content::content_main(
                    self,
                    entity_for_body,
                    &weak,
                    window,
                    cx,
                )),
        );

        let mut root = div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(rgb(t.bg))
            .text_color(rgb(t.text))
            .font_family(crate::appearance::panel_font().family.clone())
            .child(body);

        root = dialogs::render_dialogs(root, self, &weak_for_dialog, t, cx);

        if let Some(panel) = self.settings.clone() {
            let data = settings::SettingsFormData::snapshot(panel.read(cx), cx);
            root = root.child(settings::render_settings(self, &weak_for_dialog, &data));
        }
        // psp 悬浮层（tooltip / 详情卡 / 菜单 / 确认）
        root = root.child(function_panel::psp_overlays::psp_overlays(self, cx));
        // toolbar pill popup menus
        if let Some(el) = pill_menu_el {
            root = root.child(el);
        }
        // status toast（v54: statusbar 无状态文本，改瞬时提示）
        if let Some((msg, _)) = &self.status_toast {
            let text: SharedString = msg.clone().into();
            root = root.child(
                div()
                    .absolute()
                    .top(px(44.))
                    .left_1_2()
                    .ml(px(-160.))
                    .w(px(320.))
                    .px(px(12.))
                    .py(px(7.))
                    .rounded(px(8.))
                    .bg(rgb(0x22312d))
                    .shadow_lg()
                    .text_size(px(12.))
                    .text_color(rgb(0xeef4f1))
                    .flex()
                    .justify_center()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(text),
            );
        }
        // extension notify toast (top-right)
        if let Some((message, ty)) = &self.ext_notice {
            let color = match ty {
                1 => 0xfacc15,
                2 => 0xf87171,
                _ => 0x4ade80,
            };
            let text: SharedString = message.clone().into();
            root = root.child(
                div()
                    .absolute()
                    .top(px(12.))
                    .right(px(12.))
                    .max_w(px(420.))
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(rgb(color))
                    .bg(rgb(t.bg_panel))
                    .shadow_lg()
                    .flex()
                    .items_start()
                    .gap_2()
                    .child(div().size(px(7.)).rounded_full().mt(px(4.)).bg(rgb(color)))
                    .child(div().text_size(px(12.)).text_color(rgb(t.text)).child(text)),
            );
        }
        // blocking extension dialog (select/confirm/input/editor)
        if let Some(req) = &self.ext_dialog {
            root = root.child(render_ext_dialog(self, req.clone(), &weak_for_dialog));
        }
        // html 文件 tab 的应用内渲染面板（只 stage；wry 调用在 pump）
        self.stage_html_panel(window);
        root
    }
}

fn main() {
    let _ = T0.set(std::time::Instant::now());
    PERF.store(true, std::sync::atomic::Ordering::Relaxed);
    // theme: PI_FLASH_THEME (dev override) > persisted settings.json > mist
    if std::env::var("PI_FLASH_THEME").ok().and_then(|n| theme::set_by_name(&n).then_some(())).is_none() {
        let app = app_settings().theme;
        if let Some(name) = app.or_else(|| pi_link::config::read_theme(&pi_link::config::settings_path())) {
            theme::set_by_name(&name);
        }
    }
    // language: persisted workspace-memory preference
    if let Some(ix) = load_lang_pref() {
        i18n::set_lang(ix);
    }
    Application::new()
        .with_assets(assets::Assets)
        .run(|cx: &mut App| {
            // gpui-component (widget library powering TextInput): global
            // init + token mapping from the active app theme
            gpui_component::init(cx);
            appearance::sync_gpui_tokens(cx);
            // startup restore (§4)：每次启动默认最大化（位置不持久化——
            // gpui Windows 的外框/客户区坐标在存取间不对称，每个周期漂移
            // 一个边框宽）。取消最大化后的尺寸仍保存，供会话内还原参考。
            let _restored = get_window_state();
            let bounds = gpui::Bounds::centered(None, gpui::size(px(1180.), px(760.)), cx);
            let window_bounds = gpui::WindowBounds::Maximized(bounds);
            // gpui 0.2.2 Windows 创建路径对 Maximized 的延迟处理依赖
            // initial_placement/可见时序，实测不生效——回调里再显式 zoom
            let mut force_maximize = true;
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(window_bounds),
                    titlebar: Some(gpui::TitlebarOptions {
                        title: Some("pi-flash".into()),
                        // client-side title bar (v54 topbar 两段, window
                        // control hitboxes registered by titlebar.rs)
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |window, cx| {
                    if force_maximize {
                        window.zoom_window();
                        force_maximize = false;
                    }
                    // gpui-component widgets require their Root as the window
                    // root view (renders their context-menu/popover layers)
                    let chat = cx.new(Chat::new);
                    let weak = chat.downgrade();
                    // persist window bounds + shell layout on close so the
                    // startup restore layer has data (§4)
                    window.on_window_should_close(cx, move |window, cx| {
                        let b = window.bounds();
                        save_window_state(&WindowState {
                            x: f64::from(b.origin.x),
                            y: f64::from(b.origin.y),
                            w: f64::from(b.size.width),
                            h: f64::from(b.size.height),
                            maximized: window.is_maximized(),
                        });
                        if let Some(chat) = weak.upgrade() {
                            chat.update(cx, |chat, _cx| chat.persist_ui());
                        }
                        true
                    });
                    cx.new(|cx| gpui_component::Root::new(chat.into(), window, cx))
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
