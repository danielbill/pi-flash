//! Chat 构造器（自 chat.rs 外移）：全部字段簇初始化 + git 提交框接线 +
//! 启动泵挂载（spawn_boot_tasks / spawn_boot_restore）。

use crate::models_config::EnabledState;
use crate::settings::models::MjCatalogState;

use super::Chat;
use super::WsUi;
use super::ui_fields::{AuxUi, CfgUi, ComposerUi, DlgUi, LayoutUi, PoolUi, PspUi};
use crate::*;

impl Chat {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let dialog_focus = cx.focus_handle();
        // 010 重设计（2026-10-17 用户定案）：**cwd 是会话的属性，不是应用状态**
        // ——启动永远从无项目态出发（ws.cwd = None），应用不捏造任何当前目录
        //（进程 cwd / exe 目录 / 主目录兜底整族删除；payload 死循环一类的根
        // 因不复存在）。cwd 的合法来源只剩：
        //   ① 启动恢复「最后工作的会话」（startup_restore=on，recents 全局
        //      最新一条，其所属项目随之成为 cwd）——spawn_boot_restore 负责；
        //   ② 显式意图：切项目/打开项目、PI_FLASH_CWD（开发覆盖，同走 ① 通道）。
        // 无项目态下 composer 发送会弹「打开项目」先要一个目录（send 门）。
        let ui = ui_state();
        crate::perf_mark("Chat::new: ui_state (workspace prefs)");
        // fs watch 通道：tx 永驻 Chat，rx 交给 startup 的泵任务
        let (fs_watch_tx, fs_watch_rx_ch) = std::sync::mpsc::channel::<()>();

        let mut chat = Self {
            // v54.4 定案：启动固定会话界面（面板/内容区绑定关系见
            // status_bar；不恢复上次离开时的面板）
            ws: WsUi {
                project_files: Vec::new(),
                expanded_dirs: HashSet::new(),
                tree_rows: std::sync::Arc::new(Vec::new()),
                tree_drag_expand: None,
                tree_selected: None,
                tree_sb_state: crate::ui::psp_scrollbar::PspScrollbarState::new(),
                fs_watch: None,
                file_watches: std::collections::HashMap::new(),
                changes_only: ui.tree_changes_only,
                pin_view: false,
                pinned: Vec::new(),
                tree_memory: std::collections::HashMap::new(),
                tree_owner: None,
                file_cache: std::collections::HashMap::new(),
                search: crate::search::SearchUi::default(),
                pending_jump: None,
                panel_tabs: Vec::new(),
                active_panel_tab: None,
                crumb_menu_dir: None,
                crumb_dd: cx.new(|_| crate::ui::DropdownState::new()),
                ext_probe: (0, 0),
                pending_focus_file: None,
                tab_restore_busy: false,
                terminals: Vec::new(),
                active_terminal: None,
                term_seq: 0,
                term_cursor_on: true,
                term_cursor_focused: false,
                term_events: None,
                branch: String::new(),
                git_files: Vec::new(),
                git_selected: None,
                git_tab: function_panel::git_panel::GitTab::Changes,
                git_log: Vec::new(),
                git_error: None,
                git_add_del: (0, 0),
                git_refresh_gen: 0,
                git_files_cwd: PathBuf::new(),
                md_preview_scrollbar: gpui_component::scroll::ScrollbarState::default(),
                md_preview_list: gpui::ListState::new(0, gpui::ListAlignment::Top, px(1000.)),
                md_preview_list_path: None,
                md_preview_focus: cx.focus_handle(),
                fs_watch_tx,
                fs_watch_rx: Some(fs_watch_rx_ch),
                cwd: None,
                git_commit_input: {
                    let input =
                        cx.new(|cx| TextInput::new(cx).placeholder(tr("git.commit_placeholder")));
                    input
                },
            },
            layout: LayoutUi {
                focus,
                dock_panel: DockPanel::Default,
                slp_w: ui.slp_w,
                panes_hidden: ui.panes_hidden,
                slp_drag: None,
                slp_hover: false,
                split_hover: false,
                dock_split: ui.dock_split.unwrap_or(0.5),
                // dock_max = 哪个子面板最大化（映射见 workspace::min_from_dock_max）
                sess_min: crate::services::workspace::min_from_dock_max(&ui.dock_max).0,
                files_min: crate::services::workspace::min_from_dock_max(&ui.dock_max).1,
                split_drag: false,
                dock_bounds: std::rc::Rc::new(std::cell::RefCell::new(None)),
                ft_title_bounds: std::rc::Rc::new(std::cell::RefCell::new(None)),
                content_view: ContentView::Chat,
                browse_last: ContentView::Term,
                nav_open: false,
                nav_hide_at: None,
                nav_flyout_hovered: false,
                nav_hover_turn: None,
                nav_flyout_list: gpui::ListState::new(0, gpui::ListAlignment::Top, px(1000.)),
                tabs_scroll: gpui::ScrollHandle::new(),
            },
            composer: ComposerUi {
                input: String::new(),
                pending_images: Vec::new(),
                history: Vec::new(),
                history_ix: None,
                composer: None,
                input_focused: false,
                menu_ix: 0,
                menu_scroll: gpui::ScrollHandle::new(),
                menu_dismissed: false,
                at_index: std::collections::HashMap::new(),
                at_filter_cache: std::cell::RefCell::new(None),
                expanded_skills: std::collections::HashSet::new(),
                bar_hover: None,
                queue_drag: None,
                queue_drop_ix: None,
                composer_h: std::rc::Rc::new(std::cell::Cell::new(0.)),
                bubble_scrolls: std::rc::Rc::new(std::cell::RefCell::new(
                    std::collections::HashMap::new(),
                )),
            },
            pool: PoolUi {
                runtimes: std::collections::HashMap::new(),
                active_key: String::new(),
                primary_key: String::new(),
                tab_labels: std::collections::HashMap::new(),
                sessions: Vec::new(),
                session_tail_cache: std::collections::HashMap::new(),
                draft_seq: 0,
                models_by_cwd: std::collections::HashMap::new(),
                globals: startup::Globals::default(),
                project_ctx: std::collections::HashMap::new(),
                ext_tokens: services::token_probe::read_cache(),
                booted: false,
                boot_resolved: false,
                pending_zoom: false,
            },
            dlg: DlgUi {
                dialog_focus,
                dialog: None,
                plan: crate::mode::PlanUi::new(cx),
                pill_menu: None,
                pi_extension_picker: None,
                mcp_picker: None,
                model_picker: None,
                pill_anchor: None,
                pill_btn: PillBtns::default(),
                ctx_tip_ring_hover: false,
                ctx_tip_panel_hover: false,
                ctx_tip_closing: None,
                renaming: None,
                rename_input: None,
                confirm_delete: None,
                proj_tip: None,
                hover_card: None,
                proj_card: None,
                tree_menu: None,
                tree_proj_menu: None,
                tree_edit: None,
                editor_menu: None,
                term_menu: None,
                input_menu: None,
                msg_menu: None,
                status_mgr_menu: None,
                tab_menu: None,
                tree_confirm_del: None,
                tree_del_error: None,
                tree_deleting: Vec::new(),
                status_toast: None,
                settings: None,
                pending_send: None,
                subagent_panel: None,
                market_menu: None,
                plugin_submenu: false,
                tree_menu_h: std::rc::Rc::new(std::cell::Cell::new(0.)),
                tree_sub_scroll: gpui::ScrollHandle::new(),
            },
            psp: PspUi {
                projects: Vec::new(),
                project_hits: Vec::new(),
                project_filter: String::new(),
                project_sel: 0,
                active_file: None,
                list_mode: if ui.list_mode == "flat" {
                    ListMode::Flat
                } else {
                    ListMode::Grouped
                },
                sort_mode: if ui.sort_mode == "manual" {
                    SortMode::Manual
                } else {
                    SortMode::Time
                },
                collapsed_keys: ui.collapsed.iter().cloned().collect(),
                psp_shown: std::collections::HashMap::new(),
                psp_scroll: gpui::ScrollHandle::new(),
                psp_sb_state: crate::ui::psp_scrollbar::PspScrollbarState::new(),
                search_hits: Vec::new(),
                search_truncated: false,
                search_running: false,
                search_needle: String::new(),
                search_gen: 0,
                session_query: None,
                session_collapsed: std::collections::HashSet::new(),
                session_list: gpui::ListState::new(0, gpui::ListAlignment::Top, px(1000.)),
                session_sb_state: gpui_component::scroll::ScrollbarState::default(),
                pending_locate: None,
                unread: HashSet::new(),
                turn_errors: HashSet::new(),
                turn_warnings: HashSet::new(),
                pending_tabs: Vec::new(),
                window_active: true,
                hovered_project: None,
                proj_order: (!ui.proj_order.is_empty()).then(|| ui.proj_order.clone()),
                proj_drag: None,
                proj_drop_ix: None,
                proj_shift: None,
                session_pins: ui.session_pins.iter().map(|p| PathBuf::from(p)).collect(),
                hovered_session: None,
            },
            cfg: CfgUi {
                mc_patterns: None,
                mc_state: EnabledState::default(),
                mc_creds: Vec::new(),
                mj_probe_cache: Default::default(),
                mc_project_scope: false,
                mc_default_model: None,
                mc_pi_default_model: None,
                mc_default_thinking: None,
                mc_model_thinking: Vec::new(),
                mc_skills: Vec::new(),
                mc_agents: Vec::new(),
                mc_pkgs_global: Vec::new(),
                mc_pkgs_project: Vec::new(),
                pkg_op: None,
                pkg_confirm_remove: None,
                mc_default_tools: None,
                mc_mj_error: None,
                mc_mj_delete_confirm: None,
                mc_probe: std::collections::HashMap::new(),
                mj_speed: None,
                mj_test: None,
                mj_models_dev_state: MjCatalogState::Idle,
                mj_return: None,
                mj_draft_store_id: None,
                mj_fill_entry: None,
                mj_form_for: None,
                provider_registry: None,
                registry_state: crate::session::model_picker::RegistryState::Idle,
                mc_remove_confirm: None,
                mcp_servers: Vec::new(),
                mcp_errors: Vec::new(),
                op_tx: None,
            },
            plugins: pf_plugin::PluginUi::default(),
            aux: AuxUi {
                ext_widgets: Vec::new(),
                ext_dialog: None,
                ext_dialog_info_scroll: gpui::ScrollHandle::new(),
                ext_dialog_op_scroll: gpui::ScrollHandle::new(),
                ext_dialog_waiting: Vec::new(),
                ext_notice: None,
                ask: None,
                // 问卷卡「自定义答案」内嵌输入：flat 档（无边框/无内件底色），
                // 选中态的 chrome 由宿主行提供（用户定稿：输入区不要 box-in-box）
                ask_input: cx.new(|cx| TextInput::new(cx).flat()),
                update: services::updater::initial_state(),
                changelog: None,
                changelog_scroll: gpui::ScrollHandle::new(),
                pi_extension_updates: services::pi_extension_updates::UpdatesState::default(),
                market: crate::market::MarketUi::default(),
                tool_sel: None,
                sysprompt_scroll: gpui::ScrollHandle::new(),
                decl_open: [false; 7],
                ext_input: cx.new(|cx| TextInput::new(cx)),
                ext_editor_input: cx.new(|cx| TextInput::new(cx).multiline()),
                ext_sel_ix: 0,
                ext_title: None,
                ext_title_applied: String::new(),
            },
        };
        crate::perf_mark("Chat::new: struct init");
        // 启动阶段：全局态一次性装载（010-启动.md §1/§6）——模型清单（磁盘 ∪ 自有
        // 缓存）、命令、默认项、插件、全局 mcp，全部先于第一帧进内存，**不 spawn 进程**。
        chat.pool.globals = startup::load_globals();
        crate::perf_mark("Chat::new: load_globals");
        // new-session defaults (default model / thinking / enabledModels
        // scope) before the first frame — the draft pills render from them
        //（无项目态：project scope 恒 false；恢复挂载后 spawn_boot_restore 重算）
        chat.reload_model_defaults();
        // wire input callbacks that need the root entity handle
        let weak_ext = cx.entity().downgrade();
        chat.aux.ext_input.update(cx, |ti, _| {
            ti.set_on_submit(Box::new(move |v, cx| {
                let _ = weak_ext.update(cx, |c, cx| {
                    c.ext_respond(Some(v.to_string()), None, false, cx);
                });
            }));
        });
        // 问卷卡自定义答案输入框：Enter = 跳下一未答题 / 全答完则提交
        let weak_ask = cx.entity().downgrade();
        chat.aux.ask_input.update(cx, |ti, _| {
            ti.set_on_submit(Box::new(move |_v, cx| {
                let _ = weak_ask.update(cx, |c, cx| c.ask_enter(cx));
            }));
        });
        // 文件树 / git / fs watch 的挂载全部随「进入项目」动作走
        //（spawn_boot_restore / switch_project），无项目态零挂载。

        // settings panel CLI op pump (pi install/remove runs in background)
        let (op_tx, mut op_rx) = futures::channel::mpsc::unbounded::<String>();
        chat.cfg.op_tx = Some(op_tx);
        cx.spawn(async move |this, cx| {
            while let Some(msg) = op_rx.next().await {
                if this
                    .update(cx, |chat, cx| {
                        chat.cfg.pkg_op = None;
                        // 040-1 / 040-2：市场页行内按钮的转圈随 op 收尾清零
                        //（漏清哪一项，哪个页的按钮就永远转圈——040-2 首踩）
                        chat.aux.market.native.pending = None;
                        chat.set_status(msg, cx);
                        // CLI op 改的是盘上的 settings.json；globals 是启动时
                        // 的内存副本，不重读则 reload_settings_panel 只会拷到
                        // 旧列表（040：安装后列表不刷新的根因）
                        chat.pool.globals = startup::load_globals();
                        chat.reload_settings_panel();
                        // 082：CLI op（装/卸/更）动了包 → 重跑扩展更新检测，
                        // 刷新新会话页通知与扩展页箭头
                        chat.spawn_pi_extension_updates_check(cx);
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
        chat.ws.term_events = Some(term_tx);
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
        // terminal caret blink (~1Hz): 500ms heartbeat flips the phase and
        // repaints only while a focused terminal is on screen (paint writes
        // term_cursor_focused); phase lives on Chat so repaints stay
        // tick-aligned instead of drifting against the wall clock
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;
                let focused = this.update(cx, |chat, cx| {
                    if chat.ws.term_cursor_focused {
                        chat.ws.term_cursor_on = !chat.ws.term_cursor_on;
                        cx.notify();
                    }
                    chat.ws.term_cursor_focused
                });
                if focused.is_err() {
                    break;
                }
            }
        })
        .detach();
        // models panel state (enabledModels whitelist + credentials) for the
        // picker filter — loaded once at startup, refreshed when opened
        chat.reload_settings_panel();
        crate::perf_mark("Chat::new: reload_settings_panel");
        // 初始 runtime = 无项目态占位（draft-0）：cwd 为空、永不 spawn——
        // 需要项目的动作全部在入口被门拦下（composer 发送 → 打开项目；
        // 文件树/git/终端空态）。占位只为让 rt() 全路径安全（rpc 对未附着
        // 进程是 no-op）；真正的恢复/挂载在 spawn_boot_restore。
        let rt = cx.new(|_| {
            let mut r = session::runtime::SessionRuntime::new(
                crate::Chat::PLACEHOLDER_KEY.to_string(),
                PathBuf::new(),
                None,
            );
            r.default_model = services::workspace::default_model_pref();
            r
        });
        chat.pool.draft_seq = 1; // PLACEHOLDER_KEY 已被上面的初始 runtime 占用
        chat.pool
            .runtimes
            .insert(crate::Chat::PLACEHOLDER_KEY.to_string(), rt.clone());
        chat.pool.active_key = crate::Chat::PLACEHOLDER_KEY.to_string();
        // 001-S4：运行期状态初值进 hub.state()（dock 缺省 + 当前会话键；
        // 会话切换发布点在 session/pool——该域文件另有进行中改动，S4+ 补）
        crate::services::pf_state::publish_dock(DockPanel::Default);
        crate::services::pf_state::publish_active_session(&chat.pool.active_key);
        chat.subscribe_runtime(&rt, cx);
        // 010-启动.md §9：启动期的一切（120ms 泵 / 3s 外部追加观察 / 60s 空闲
        // 回收 / 30s recents 对账 / 启动页闸门）都由 startup 发起；恢复最后
        // 会话 + 进入其项目 + 会话清单装载 = spawn_boot_restore（cwd 不再在
        // Chat::new 捏造）。进程附着随恢复路径走（open_session 自带 spawn +
        // GetMessages），无项目态零进程。
        startup::spawn_boot_tasks(cx);
        startup::spawn_boot_restore(cx);
        if PERF.load(std::sync::atomic::Ordering::Relaxed) {
            if let Some(t0) = T0.get() {
                eprintln!("[perf] last-session tail rendered: {:?}", t0.elapsed());
            }
        }
        // git panel: Enter in the commit box commits staged changes
        let entity_for_git = cx.entity();
        chat.ws.git_commit_input.update(cx, |ti, _| {
            let weak_git = entity_for_git.downgrade();
            ti.set_on_submit(Box::new(move |_, cx| {
                let _ = weak_git.update(cx, |c, cx| c.git_commit_staged(cx));
            }));
        });
        chat
    }
}
