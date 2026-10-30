//! 会话池 Chat 方法域：new/open/delete/switch/subscribe_runtime + 会话改名。
//! （P2 归并：actions_sessions + actions_runtime + actions_rename 池侧；
//! SessionRuntime 本体在 session/runtime.rs。）

use crate::*;

impl Chat {
    pub(crate) fn subscribe_runtime(
        &mut self,
        rt: &gpui::Entity<session::runtime::SessionRuntime>,
        cx: &mut Context<Self>,
    ) {
        // 执行域实例配置随会话挂册同步（90-6；新会话铸造 token + 写实例配置）
        crate::pf_plugin::svc::svc_sync_configs(self);
        cx.subscribe(rt, |chat, rt, ev: &session::runtime::SessionEvent, cx| {
            use session::runtime::SessionEvent;
            let is_active = rt.read(cx).key == chat.pool.active_key;
            match ev {
                SessionEvent::Changed => {
                    // recents (003-session管理): message-level activity on
                    // this runtime is "recently updated" — promote its file
                    // in the recent-activity list (persist throttled)
                    if let Some(f) = rt.read(cx).file.clone() {
                        pi_link::recents::touch_recent(&f);
                    }
                    if !is_active {
                        // v54 未读绿点：非活跃会话在跑/在流式 → 记未读，
                        // 切换到它时清除
                        let (file, running) = {
                            let r = rt.read(cx);
                            (
                                r.file.clone(),
                                r.agent_running || r.state.as_ref().is_some_and(|s| s.is_streaming),
                            )
                        };
                        if let Some(f) = file {
                            if running && !chat.psp.unread.contains(&f) {
                                chat.psp.unread.insert(f);
                            }
                        }
                    }
                    cx.notify();
                }
                SessionEvent::TurnSettled => {
                    // 轮次终点（2026-10-10 规则修订）：agent 结束时**窗口没
                    // 焦点、或本会话已转后台**（用户切到同一窗口的别的对话）
                    // 就给点——三色分类：外部错误/崩溃 → 红；内部中断
                    // （length/aborted）→ 黄；正常完成 → 青蓝。在场看见不给。
                    // is_active = 该 runtime key vs 当前活跃 key，结算时刻
                    // 现算即前台/后台判定
                    if !chat.psp.window_active || !is_active {
                        let (file, turn_error, turn_interrupted) = {
                            let r = rt.read(cx);
                            (r.file.clone(), r.turn_error, r.turn_interrupted)
                        };
                        if let Some(f) = file {
                            // 通知点只表达**最近一轮**的结局：先摘掉上一轮的
                            // 档位再挂新点——否则旧红点（优先级最高）会盖住
                            // 新一轮的完成通知，反之亦然
                            chat.psp.unread.remove(&f);
                            chat.psp.turn_errors.remove(&f);
                            chat.psp.turn_warnings.remove(&f);
                            if turn_error {
                                chat.psp.turn_errors.insert(f);
                            } else if turn_interrupted {
                                chat.psp.turn_warnings.insert(f);
                            } else {
                                chat.psp.unread.insert(f);
                            }
                        }
                    }
                    cx.notify();
                }
                SessionEvent::ListDirty => {
                    chat.refresh_sessions();
                    cx.notify();
                }
                SessionEvent::PendingTab(preview) => {
                    // 伪标签生命周期（draft 首条消息乐观上侧栏）：挂/刷新按
                    // key 幂等，撤按 key 摘除。真标签由 FileBound 落地。
                    let key = rt.read(cx).key.clone();
                    match preview {
                        Some(p) => {
                            let p = p.clone();
                            let cwd = rt.read(cx).cwd.to_string_lossy().to_string();
                            if let Some(e) = chat.psp.pending_tabs.iter_mut().find(|e| e.key == key)
                            {
                                e.preview = p;
                            } else {
                                chat.psp.pending_tabs.push(PendingTab {
                                    key,
                                    preview: p,
                                    cwd,
                                });
                            }
                        }
                        None => {
                            chat.psp.pending_tabs.retain(|e| e.key != key);
                        }
                    }
                    cx.notify();
                }
                SessionEvent::ExtUi(req) => {
                    // **只接远程插件线自己的请求**（与下面 Assistant 同一条闸）。
                    // 扩展的 ui 广播是全局的：任何会话里任何扩展调
                    // `ui.notify` / `setStatus` / `setTitle` 都走这条事件，
                    if is_active {
                        let req = req.clone();
                        chat.on_ext_ui(req, cx);
                    } else {
                        // parked session asks for permission: queue it, the
                        // badge surfaces it; popped when user switches there
                        rt.update(cx, |r, _| r.ext_queue.push(req.clone()));
                        cx.notify();
                    }
                }
                SessionEvent::Models(models) => {
                    // 进程答案写项目槽（pi-web /api/models parity：每个 runtime 只写
                    // 自己 cwd 的条目），同时**并进全局态并回写自有缓存**
                    // （010-启动.md §2/§4.1：包内联 provider 只有进程答得到，
                    // 存下来下次冷启动就有真名字）。
                    let cwd_key = rt.read(cx).cwd.to_string_lossy().to_string();
                    chat.pool.models_by_cwd.insert(cwd_key, models.clone());
                    // 只并进本进程内存（无落盘缓存层）
                    crate::startup::merge_models(&mut chat.pool.globals, &models);
                    chat.cfg.mc_state = models_config::compute_state(
                        chat.cfg.mc_patterns.as_ref(),
                        &chat.mc_refs(),
                    );
                    cx.notify();
                }
                SessionEvent::Commands(commands) => {
                    // 扩展命令（包内注册，磁盘没有数据文件）：并进全局清单 + 回写缓存。
                    // `skill:` 前缀不入全局（skill 走磁盘派生，见 startup::merge_commands）。
                    crate::startup::merge_commands(&mut chat.pool.globals, &commands);
                    cx.notify();
                }
                SessionEvent::FileBound(path) => {
                    chat.on_file_bound(&rt, path.clone(), cx);
                }
                SessionEvent::RenameReady(prefill) => {
                    if is_active {
                        if let Some(f) = rt.read(cx).file.clone() {
                            chat.start_rename(f, prefill.clone(), cx);
                        }
                    }
                }
                SessionEvent::RecallInput { text, images } => {
                    // abort 撤回的排队消息回填输入框（pi-web restoring-to-editor
                    // parity）：文本置于已有草稿之前（撤回的比正在写的旧）。
                    // 活跃会话以 composer 为真值源回填可见输入框；后台会话
                    // 没有 composer 镜像，吞进 runtime 草稿，切回时随
                    // switch_to 带回。images = 090 本地队列撤回的附件（pi 侧
                    // 撤回恒空）——runtime 已吞，活跃会话把 composer 附件条
                    // 也拼上（前置，不清用户已有）。
                    rt.update(cx, |r, _| {
                        if !text.is_empty() {
                            r.input = if r.input.is_empty() {
                                text.clone()
                            } else {
                                format!("{text}\n\n{}", r.input)
                            };
                        }
                    });
                    if is_active {
                        if !text.is_empty() {
                            let draft = chat.composer.input.clone();
                            let merged = if draft.is_empty() {
                                text.clone()
                            } else {
                                format!("{text}\n\n{draft}")
                            };
                            chat.set_input(merged, cx);
                        }
                        if !images.is_empty() {
                            let mut imgs = images.clone();
                            imgs.extend(chat.composer.pending_images.drain(..));
                            chat.composer.pending_images = imgs;
                        }
                    }
                }
            }
        })
        .detach();
    }

    /// Attention switch: flush the editor mirrors into the outgoing
    /// session, load the incoming one's inputPanel state. Zero process
    /// operations, zero IO — this is the pi-web "switch" and it is instant.
    pub(crate) fn switch_to(
        &mut self,
        rt: gpui::Entity<session::runtime::SessionRuntime>,
        cx: &mut Context<Self>,
    ) {
        if let Some(old) = self.pool.runtimes.get(&self.pool.active_key).cloned() {
            if old != rt {
                let (input, images, history) = (
                    std::mem::take(&mut self.composer.input),
                    std::mem::take(&mut self.composer.pending_images),
                    std::mem::take(&mut self.composer.history),
                );
                old.update(cx, |r, _| {
                    r.input = input;
                    r.pending_images = images;
                    r.history = history;
                });
            }
        }
        let (input, images, history, key, file) = rt.update(cx, |r, _| {
            (
                r.input.clone(),
                r.pending_images.clone(),
                r.history.clone(),
                r.key.clone(),
                r.file.clone(),
            )
        });
        self.composer.input = input;
        self.composer.pending_images = images;
        self.composer.history = history;
        self.composer.history_ix = None;
        self.pool.active_key = key;
        self.psp.active_file = file.clone();
        // 注意力切到会话 ⇒ 内容区必须回到 chat 前台：否则在终端/文件编辑
        // 视图里点会话列表行（或「新会话」）后，内容区还停在 Term/File，
        // 看着像「切了会话但窗口没切过来」。browse_last 不受影响，
        // statusbar 的 files tab 仍能回到终端/文件（set_content_view 只在
        // Term/File 时更新它）。
        if self.layout.content_view != crate::ContentView::Chat {
            self.set_content_view(crate::ContentView::Chat);
        }
        if let Some(f) = &file {
            // 无项目态不存在可切会话（switch_pool_rt 已先重挂），防御式取 Some。
            if let Some(cwd) = &self.ws.cwd {
                if !self.plugin_claimed(&f.to_string_lossy()) {
                    set_last_open(&cwd.to_string_lossy(), &f.to_string_lossy());
                }
            }
            // v54: 切入会话清除未读/出错红点
            self.psp.unread.remove(f);
            self.psp.turn_errors.remove(f);
            self.psp.turn_warnings.remove(f);
        }
        self.dlg.pill_menu = None;
        self.composer.menu_ix = 0;
        // 「点外/Esc 收起」不能跨会话泄漏：否则上个会话收起过菜单，切过来
        // 后 / @ 永远弹不出（active_menu 第一行就短路）
        self.composer.menu_dismissed = false;
        // surface queued permission requests of the incoming session
        let queued = rt.update(cx, |r, _| {
            let q = std::mem::take(&mut r.ext_queue);
            r.touch();
            q
        });
        if let Some(first) = queued.first() {
            let first = first.clone();
            cx.notify();
            self.on_ext_ui(first, cx);
            for r in queued.into_iter().skip(1) {
                rt.update(cx, |r2, _| r2.ext_queue.push(r));
            }
        }
        // 子代理面板缓存随会话激活常备（open=false）：徽标从切换起就准，
        // 不依赖先开过一次面板（2026-10-19 拍板：关面板只是隐藏，轮询常驻）。
        // 功能总开关关着不建缓存不起轮询（零 IO）。
        if crate::services::workspace::subagents_enabled() {
            self.ensure_subagent_panel(cx);
        }
        cx.notify();
    }
}

impl Chat {
    /// 无项目态占位会话：恒在、永不 spawn、非插件功能线。`shell::chat` 建它，
    /// 「关掉功能页回桌面」也回它——不再到处写字面量 `"draft-0"`。
    pub(crate) const PLACEHOLDER_KEY: &'static str = "draft-0";

    /// 新会话草稿（用户显式新建：psp ＋、新建按钮、删会话后归位）。
    /// `clear_last_open` 跟在这里：用户明确要空白会话 ⇒ 该项目不再恢复旧会话。
    pub(crate) fn new_session(&mut self, cx: &mut Context<Self>) {
        self.new_session_with(cx, true);
    }

    /// 启动落草稿（restore=off / 该项目无会话）：**不清 per-project 会话
    /// 记忆**——旧会话还在盘上，下次开 restore 仍应能直接加载（010）。
    pub(crate) fn boot_new_session(&mut self, cx: &mut Context<Self>) {
        self.new_session_with(cx, false);
    }

    fn new_session_with(&mut self, cx: &mut Context<Self>, clear_memory: bool) {
        // 无项目态防御：new_session 只在有项目的上下文被调（composer 发送
        // 已在 send 门处先要目录；切项目/恢复路径 cwd 已就位）
        let Some(cwd) = self.ws.cwd.clone() else {
            return;
        };
        // lazy draft (pi-web parity): the pi process spawns on the first
        // prompt, not now; running sessions are untouched
        let key = format!("draft-{}", self.pool.draft_seq);
        self.pool.draft_seq += 1;
        let rt = cx.new(|_| {
            let mut r = session::runtime::SessionRuntime::new(key.clone(), cwd.clone(), None);
            r.default_model = services::workspace::default_model_pref();
            r.status = tr("session.new").to_string();
            r
        });
        self.pool.runtimes.insert(key, rt.clone());
        self.subscribe_runtime(&rt, cx);
        if clear_memory {
            clear_last_open(&cwd.to_string_lossy());
        }
        self.dlg.renaming = None;
        self.dlg.rename_input = None;
        self.switch_to(rt, cx);
    }

    /// Draft persisted its first prompt: pi bound this process to a fresh
    /// session file (pi-web promoteNewSession parity). Migrate the pool key
    /// draft-N → path so the sidebar entry and the pool share one identity,
    /// otherwise reopening the session would spawn a duplicate runtime.
    pub(crate) fn on_file_bound(
        &mut self,
        rt: &gpui::Entity<session::runtime::SessionRuntime>,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let path_key = path.to_string_lossy().to_string();
        let old_key = rt.read(cx).key.clone();
        if old_key == path_key {
            return;
        }
        // 伪标签到此退役：磁盘扫描现在找得到真文件，真标签接管
        self.psp.pending_tabs.retain(|e| e.key != old_key);
        rt.update(cx, |r, _| r.key = path_key.clone());
        // 会话级插件清单跟着身份走（草稿 key → 会话文件路径）：确认时按草稿
        // key 落盘，这里把它搬到真实会话键上，重启后按会话恢复（034）
        if let Some(store) = pi_link::session_ext::store_path() {
            let sources = pi_link::session_ext::read_for(&store, &old_key);
            if !sources.is_empty() {
                let _ = pi_link::session_ext::write_for(&store, &path_key, &sources);
            }
            let _ = pi_link::session_ext::remove_for(&store, &old_key);
        }
        // 执行域锚点注册表随身份搬家（token 不变 → 插件侧绑定指针跨迁移稳定，90-6 §2）
        if let Some(base) = pi_link::paths::dir() {
            let _ = pi_link::service_ext::move_key(&base, &old_key, &path_key);
        }
        if let Some(entity) = self.pool.runtimes.remove(&old_key) {
            self.pool.runtimes.insert(path_key.clone(), entity);
        }
        if self.pool.active_key == old_key {
            self.pool.active_key = path_key.clone();
        }
        // 插件认领线跟随迁移（同语义，泛化形态）：草稿键 → 会话文件键，
        // 过滤清单按真身补一条（path 精确，重启后仍藏得住）；交接文件同步
        if let Some(c) = self
            .plugins
            .claims
            .iter_mut()
            .find(|c| c.key == old_key)
            .map(|c| (c.key.clone(), c.prefix.clone(), c.plugin_id.clone()))
        {
            let (_old, prefix, plugin_id) = c;
            self.plugins
                .claims
                .iter_mut()
                .find(|c| c.plugin_id == plugin_id)
                .map(|c| c.key = path_key.clone());
            let name = std::path::Path::new(&path_key)
                .file_stem()
                .map(|n| n.to_string_lossy().into_owned());
            pi_link::hidden_sessions::set_owned(
                &plugin_id,
                name.as_deref(),
                Some(&path),
                Some(&prefix),
            );
            // 交接文件同步（ext 读的是 key，草稿键已退役）
            if let Some(e) = self
                .plugins
                .plugins
                .iter()
                .find(|e| e.manifest.id == plugin_id)
            {
                let f = e.manifest.dir.join("data").join("claim.json");
                if f.exists() {
                    let _ = std::fs::write(
                        &f,
                        serde_json::json!({"key": path_key, "name": name.as_deref().unwrap_or(""), "cwd": "", "prefix": prefix, "plugin_id": plugin_id})
                            .to_string(),
                    );
                }
            }
        }
        self.psp.active_file = Some(path.clone());
        // recents (003-session管理): a fresh session file just landed
        pi_link::recents::touch_recent(&path);
        // 插件认领线不记 last_open（002：影子会话永不进桌面恢复线）
        if !self.plugin_claimed(&path_key) {
            set_last_open(&rt.read(cx).cwd.to_string_lossy(), &path_key);
        }
        // 新会话落地的项目组若处于折叠态则自动展开：伪标签（draft）在折叠
        // 组里照常显示，FileBound 一换真身就被折叠藏掉——用户眼里刚发的
        // 会话凭空消失。fork / 远程端新会话同经此处（2026-10-15 用户报障）。
        self.psp
            .collapsed_keys
            .remove(&same_ws_key(&rt.read(cx).cwd.to_string_lossy()));
        self.refresh_sessions();
        cx.notify();
    }

    /// 池内直切（会话行点击/伪标签行/自动化 session.switch）：目标会话属
    /// 别的项目时先重挂工作区再切注意力——switch_to 本身是纯注意力切换，
    /// 跨项目场景绕过 open_session 的入口都必须走这里（用户实测
    /// 2026-10-13：池命中跨项目只改 cwd 不重建，树/变更面板停在上一项目）。
    pub(crate) fn switch_pool_rt(
        &mut self,
        rt: gpui::Entity<session::runtime::SessionRuntime>,
        cx: &mut Context<Self>,
    ) {
        let cwd = rt.read(cx).cwd.clone();
        if !self
            .ws
            .cwd
            .as_deref()
            .is_some_and(|cur| same_path(cur, &cwd))
        {
            self.ws.cwd = Some(cwd);
            self.remount_workspace(cx);
        }
        self.switch_to(rt, cx);
    }

    /// 跨项目切换的工作区重挂（cwd 已由调用方写入 self.ws.cwd）：树根/
    /// git/文件树/fs watcher/cwd 外单文件监听全套跟着新 cwd 重建。
    /// open_session 的池命中与新运行时两条路径共用。
    pub(crate) fn remount_workspace(&mut self, cx: &mut Context<Self>) {
        let Some(cwd) = self.ws.cwd.clone() else {
            return;
        };
        // ① 归档旧项目的文件树状态（展开集合 + Pin 视图）：对齐 Zed 的
        // per-worktree 内存表，切回来不丢；同时落盘（`__tree`）
        self.archive_tree_state();
        // ①.5 「进入项目」即 touch 项目注册表：切项目三条路（switch_project /
        //     点会话行 open_session / 树选文件 sync_tree_to_file）都要算活动，
        //     否则最近活动项目会停在别处（用户实测：点观宏知微的会话后重启
        //     回到 pi-flash——那条路漏了 touch）
        pi_link::projects::register_project(&cwd);
        self.ws.branch = read_branch(&cwd);
        // ② Pin 收藏清单先装载（下面恢复树状态时要判断 Pin 视图是否可用）
        self.ws.pinned = crate::services::workspace::pins_for(&cwd.to_string_lossy())
            .into_iter()
            .map(PathBuf::from)
            .collect();
        // ③ 文件树展开集合：内存 → 磁盘 → 默认「只展开根」（对齐 Zed：打开
        //    项目默认展开 cwd）；Pin 视图同样按项目恢复（关闭前开着 = 重启
        //    还开），但收藏为空时无意义（照旧退回文件树）
        self.restore_tree_state(&cwd);
        self.refresh_sessions();
        self.load_project_files();
        // 9rk0：refresh_git 的 rebuild_tree 随数据后台换入（~150ms），这里先
        // 用空 git 数据同步展平新 cwd 的树，否则换项目瞬间文件树残留旧项目
        self.rebuild_tree();
        // 换工作区：git 状态/树缓存/fs watcher 全部跟着 cwd 重挂
        //（此前这里漏 refresh_git，git 面板会显示上一个项目的状态）
        self.refresh_git(cx);
        self.attach_fs_watch();
        // cwd 变了：cwd 外单文件监听面跟着重算（对齐 switch_project）
        self.retain_file_watches();
    }

    pub(crate) fn open_session(&mut self, path: PathBuf, rename: bool, cx: &mut Context<Self>) {
        // 入参可能带历史字符串形态（recents/last-open 存过 pi 自拼的 group
        // 大小写）：先归一到扫描器形态再入池——pool key、侧栏行匹配、通知
        // 点集合共用同一条字符串身份，形态分裂就会点行重复 spawn runtime
        let path = pi_link::sessions::canonical_session_path(&path);
        // recents (003-session管理): opening is activity — promote to the
        // top of the recent-activity list (persist throttled)
        pi_link::recents::touch_recent(&path);
        // cwd lookup: current project first, then all psp groups (v54 跨项目)，
        // 兜底当前项目；三处皆无（无项目态孤儿会话）→ 不开
        let cwd = self
            .pool
            .sessions
            .iter()
            .find(|s| s.path == path)
            .map(|s| PathBuf::from(s.cwd.clone()))
            .or_else(|| {
                self.psp
                    .projects
                    .iter()
                    .find(|g| g.sessions.iter().any(|s| s.path == path))
                    .map(|g| g.path.clone())
            })
            .or_else(|| self.ws.cwd.clone());
        let Some(cwd) = cwd else { return };
        let key = path.to_string_lossy().to_string();
        // pool hit: switch attention — instant, all messages in place
        if let Some(rt) = self.pool.runtimes.get(&key).cloned() {
            self.dlg.renaming = None;
            self.dlg.rename_input = None;
            self.dlg.confirm_delete = None;
            if rename {
                rt.update(cx, |r, _| r.pending_rename = true);
            }
            self.switch_pool_rt(rt, cx);
            return;
        }
        // new runtime: tail render first (cache or disk), process spawns in
        // the background — the conversation is on screen before node exists
        let tail = match self.pool.session_tail_cache.get(&path).cloned() {
            Some(msgs) => msgs,
            None => {
                let msgs = msgs_from_tail(read_tail_messages(&path, 256 * 1024, 100));
                if self.pool.session_tail_cache.len()
                    >= crate::services::workspace::TAIL_PRELOAD * 2
                {
                    self.pool.session_tail_cache.clear();
                }
                self.pool
                    .session_tail_cache
                    .insert(path.clone(), msgs.clone());
                msgs
            }
        };
        let rt = cx.new(|_| {
            let mut r = session::runtime::SessionRuntime::new(key, cwd.clone(), Some(path.clone()));
            r.default_model = services::workspace::default_model_pref();
            r.messages = tail;
            r.pager.reload(r.messages.len());
            r.status = "resuming".into();
            r.pending_rename = rename;
            r
        });
        self.pool
            .runtimes
            .insert(rt.read(cx).key.clone(), rt.clone());
        self.subscribe_runtime(&rt, cx);
        // 秒开深度优化（pi-flash-2o92）：磁盘计数改后台——主线程全文件读
        // 是 O(会话大小) 的打开冻结源
        rt.update(cx, |r, cx| r.load_disk_msg_count(cx));
        if !self
            .ws
            .cwd
            .as_deref()
            .is_some_and(|cur| same_path(cur, &cwd))
        {
            self.ws.cwd = Some(cwd);
            self.remount_workspace(cx);
        }
        if let Some(cur) = &self.ws.cwd {
            // 插件认领线不进桌面恢复线（与远程功能线同口径）
            if !self.plugin_claimed(&path.to_string_lossy()) {
                set_last_open(&cur.to_string_lossy(), &path.to_string_lossy());
            }
        }
        self.dlg.renaming = None;
        self.dlg.rename_input = None;
        self.dlg.confirm_delete = None;
        self.switch_to(rt.clone(), cx);
        let rt_spawn = rt;
        cx.spawn(async move |_this, cx| {
            let _ = rt_spawn.update(cx, |r, cx| {
                if r.agent.session.is_none() {
                    if let Some(rx) = r.spawn() {
                        let epoch = r.agent.epoch;
                        session::runtime::SessionRuntime::attach_pump(&rt_spawn, rx, epoch, cx);
                    }
                }
                r.rpc(&Command::GetMessages);
                r.refresh_anchors();
                r.refresh_state();
            });
        })
        .detach();
    }

    /// pi-web DELETE /api/sessions/{id}: unlink the session file; a live
    /// (active) session is aborted + shut down first and the shell resets
    /// to a fresh draft with the same cwd.
    pub(crate) fn delete_session(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.dlg.confirm_delete = None;
        let key = path.to_string_lossy().to_string();
        // 会话没了，它的自定义插件清单也清掉（034）
        if let Some(store) = pi_link::session_ext::store_path() {
            let _ = pi_link::session_ext::remove_for(&store, &key);
        }
        let was_active = self
            .pool
            .runtimes
            .get(&self.pool.active_key)
            .map(|rt| {
                rt.read(cx)
                    .file
                    .as_deref()
                    .is_some_and(|f| same_path(f, &path))
            })
            .unwrap_or(false);
        // tear down the runtime (abort + process kill; dropping the entity
        // clears the file handle on Windows before unlink)
        if let Some(rt) = self.pool.runtimes.remove(&key) {
            rt.update(cx, |r, _| {
                r.rpc(&Command::Abort);
                r.shutdown_process();
            });
        }
        self.psp.unread.remove(&path);
        self.psp.turn_errors.remove(&path);
        self.psp.turn_warnings.remove(&path);
        // 会话没了，置顶记录同步清册（020 会话置顶）
        self.psp.session_pins.retain(|p| !same_path(p, &path));
        // recents (003-session管理): the file is gone, drop the entry
        pi_link::recents::remove_recent(&path);
        if was_active {
            self.psp.active_file = None;
            self.new_session(cx);
        }
        match std::fs::remove_file(&path) {
            Ok(_) => {
                self.pool.sessions.retain(|s| s.path != path);
                self.pool.session_tail_cache.remove(&path);
                // refresh_sessions 只重扫当前 cwd，其余项目组沿用各组缓存——
                // 非当前项目的会话也要在这里摘掉，否则列表一直显示已删会话
                for g in &mut self.psp.projects {
                    g.sessions.retain(|s| s.path != path);
                }
                self.set_status("session deleted".into(), cx);
            }
            Err(e) => {
                self.set_status(
                    crate::i18n::tf("extensions.remove_failed", &[("e", e.to_string())]),
                    cx,
                );
            }
        }
        self.refresh_sessions();
        cx.notify();
    }
}

impl Chat {
    pub(crate) fn start_rename(&mut self, path: PathBuf, prefill: String, cx: &mut Context<Self>) {
        let weak_ok = cx.entity().downgrade();
        let weak_esc = cx.entity().downgrade();
        let input = cx.new(|cx| {
            TextInput::new(cx)
                .select_all_on_focus()
                .placeholder(tr("action.session_name"))
        });
        input.update(cx, |ti, cx| ti.set_value(prefill, cx));
        input.update(cx, |ti, _| {
            ti.set_on_submit(Box::new(move |v, cx| {
                let _ = weak_ok.update(cx, |c, cx| c.apply_rename(v.trim().to_string(), cx));
            }));
            ti.set_on_escape(Box::new(move |cx| {
                let _ = weak_esc.update(cx, |c, cx| {
                    c.dlg.renaming = None;
                    c.dlg.rename_input = None;
                    cx.notify();
                });
            }));
        });
        self.dlg.renaming = Some(path);
        self.dlg.rename_input = Some(input);
        cx.notify();
    }

    /// Rename commit path that never reads the input entity (called from
    /// the input's own submit callback where the entity is borrowed).
    pub(crate) fn apply_rename(&mut self, name: String, cx: &mut Context<Self>) {
        self.rt().read(cx).rpc(&Command::SetSessionName { name });
        // sidebar label comes from the session file's `session_info` entry;
        // reload it when pi confirms the write (set_session_name response —
        // an immediate re-read here would race the file flush)
        self.refresh_state(cx);
        self.dlg.renaming = None;
        self.dlg.rename_input = None;
        cx.notify();
    }
}

// ---------------------------------------------------------------------------
// 会话池方法：刷新/切换/活跃态/上下文/标题（P3 自 main.rs impl Chat 拆入）
// ---------------------------------------------------------------------------

impl Chat {
    pub(crate) fn refresh_state(&self, cx: &mut gpui::App) {
        self.rt().update(cx, |r, _| r.refresh_state());
    }

    pub(crate) fn refresh_sessions(&mut self) {
        // 无项目态：没有「当前项目会话清单」可言（psp 窗口分组照常）
        let Some(cwd) = self.ws.cwd.clone() else {
            return;
        };
        let cwd = cwd.to_string_lossy().to_string();
        // 影子会话（插件认领线）不进任何列表：过滤清单由插件自己注入
        let hidden = pi_link::hidden_sessions::load();
        let full = list_sessions_for_cwd(&cwd, 500)
            .into_iter()
            .filter(|s| same_ws(&s.cwd, &cwd))
            .filter(|s| !hidden.hides(s.name.as_deref(), &s.path))
            .collect::<Vec<_>>();
        // v60: 加载语义 = 时间窗口（设置.加载时间窗口）——当前项目只保留
        // 窗口内活跃的会话（mtime 近似清单 last_active）+ 当前激活会话
        // 兜底（保证改名/打开的旧会话始终可见）；显示量由 psp 分页控制
        let cutoff = std::time::SystemTime::now()
            - std::time::Duration::from_secs(load_window_days() * 86_400);
        let mut sessions: Vec<SessionInfo> = full
            .iter()
            .filter(|s| s.modified >= cutoff)
            .cloned()
            .collect();
        if let Some(active) = &self.psp.active_file {
            if !sessions.iter().any(|s| &s.path == active) {
                if let Some(info) = full.iter().find(|s| &s.path == active) {
                    sessions.push(info.clone());
                }
            }
        }
        // 置顶兑底（020 会话置顶）：当前项目的置顶会话哪怕跌出加载时间
        // 窗口也要在列表里——pin 的意义就是老会话常驻可见，对齐上面的
        // active 兑底；其他项目的置顶会话沿用各组启动时装入的缓存
        for pin in &self.psp.session_pins {
            if !sessions.iter().any(|s| same_path(&s.path, pin))
                && let Some(info) = full.iter().find(|s| same_path(&s.path, pin))
            {
                sessions.push(info.clone());
            }
        }
        self.pool.sessions = sessions;
        // sync the current project's group, then re-pin project order
        let all: Vec<SessionInfo> = self
            .psp
            .projects
            .iter()
            .flat_map(|g| {
                if same_ws(&g.path.to_string_lossy(), &cwd) {
                    self.pool.sessions.clone()
                } else {
                    g.sessions.clone()
                }
            })
            .collect();
        self.rebuild_projects(all);
    }

    pub(crate) fn switch_project(&mut self, cwd: PathBuf, cx: &mut Context<Self>) {
        // 「进入项目」唯一显式入口：cwd 只在这里 / 恢复路径 / open_session
        // 跨项目三处被写入，全部源自用户动作（010 重设计：应用永不搢造）
        self.ws.cwd = Some(cwd.clone());
        // 打开项目即注册（PF 项目注册表）：供「打开项目」弹窗枚举与 newest()
        // 排序兑底；psp 分组侧零会话项目不建组（用户 2026-10-18 定案）
        pi_link::projects::register_project(&cwd);
        self.dlg.dialog = None;
        // 全套跟着新 cwd 重挂（Pin 收藏夹/树/git/fs watcher/项目文件索引/
        // cwd 外单文件监听）——与 open_session 的池命中路径共用同一条
        // remount_workspace。此前本函数把这一套自己抄了一遍，漏掉了
        // **Pin 收藏夹**（按项目存的白名单）：切项目后 ws.pinned 还是上一个
        // 项目的绝对路径，Pin 视图下 rebuild_tree → flatten_pins →
        // is_abs_path_ignored → strip_prefix().unwrap() 当场 panic，
        // 且 panic 发生在鼠标事件回调（非 unwind 边界）里 → 进程直接消失
        // （用户 2026-10-17 报崩：从文件树切项目后点 psp 项目行 ＋ 新建会话）。
        self.remount_workspace(cx);
        let resumed = get_last_open(&cwd.to_string_lossy())
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .map(|path| self.open_session(path, false, cx))
            .is_some();
        if resumed {
            cx.notify();
            return;
        }
        // 2026-12-06 用户定夺：项目切了 ≠ 要新开会话。新项目没有可恢复的
        // 上次会话时，当前会话若**进行中**（已绑定会话文件、非空白草稿）则
        // 原地保留：树/侧栏已随新 cwd 重挂，注意力与 composer 仍连原会话，
        // 用户看完新项目文档点会话标签即回原对话（切回原项目/点会话行也
        // 能靠 last-open 记忆归位）。只有当前本身就是空白草稿才落新草稿
        // ——空白换空白无感知，且发送目标跟着落到可见的新项目，不会重演
        // 2026-10-18「会话落错项目且列表隐形」（那次正是空白草稿跨项目
        // 发消息，新会话落进启动时旧项目的缓存组）。
        let in_progress = self
            .pool
            .runtimes
            .get(&self.pool.active_key)
            .is_some_and(|rt| rt.read(cx).file.is_some());
        if in_progress {
            cx.notify();
            return;
        }
        self.new_session(cx);
        cx.notify();
    }

    pub(crate) fn rt(&self) -> gpui::Entity<session::runtime::SessionRuntime> {
        self.pool
            .runtimes
            .get(&self.pool.active_key)
            .expect("active runtime exists")
            .clone()
    }

    /// 「其他」页提示音开关 → 全池广播。轮末提示音归 runtime 职责，每个常驻
    /// runtime 各持一份副本，所以新开关必须扇出到**所有** runtime 而不只是活跃
    /// 那个——后台会话同样会跑 agent、同样需要提示。
    pub(crate) fn broadcast_sound_on(&self, on: bool, cx: &mut Context<Self>) {
        for (_, rt) in &self.pool.runtimes {
            rt.update(cx, |r, _| r.sound_on = on);
        }
    }

    /// 当前项目的上下文（010-启动.md §5）：命中启动装载的集合就直接返回，
    /// 未命中（新打开的项目）用同一个装载函数现算一次并纳入集合。
    pub(crate) fn project_ctx_now(&mut self) -> startup::ProjectCtx {
        // 无项目态：无项目上下文（全局设置照常，项目层为空默认）
        let Some(cwd) = self.ws.cwd.clone() else {
            return startup::ProjectCtx::default();
        };
        let key = cwd.to_string_lossy().to_string();
        if let Some(ctx) = self.pool.project_ctx.get(&key) {
            return ctx.clone();
        }
        let ctx = startup::load_project(&cwd);
        self.pool.project_ctx.insert(key, ctx.clone());
        ctx
    }

    /// 技能表现读（盘上真值）并回写启动缓存。
    ///
    /// 设置页的启停开关**写盘后必须走这里**：`project_ctx` 是进程启动时的
    /// 快照，直接拿它回读只能看到开关前的旧 `disable-model-invocation`，
    /// 于是 UI 永远不翻转、开关回调又按旧状态再算一次同样的目标值——看起来
    /// 就是「两个开关都是摆设」（本页写盘后回读的坑与 043 MCP 同源）。
    pub(crate) fn skills_fresh(&mut self) -> Vec<pi_link::skills::SkillEntry> {
        let Some(cwd) = self.ws.cwd.clone() else {
            return Vec::new();
        };
        let skills = startup::load_skills(&cwd);
        let commands = pi_link::catalog::skill_commands(&skills);
        if let Some(ctx) = self
            .pool
            .project_ctx
            .get_mut(&cwd.to_string_lossy().to_string())
        {
            ctx.skills = skills.clone();
            ctx.skill_commands = commands;
        }
        skills
    }

    /// 当前会话标题（topbar 会话视图展示；≤30 字截断）。优先 pi 会话名，
    /// 回落首条用户消息。
    pub(crate) fn session_title(&self, cx: &gpui::App) -> String {
        self.session_title_of(&self.pool.active_key.clone(), cx)
    }

    /// 指定池 key 的会话标题（topbar 第一格在查看插件功能页时显示桌面会话用）。
    pub(crate) fn session_title_of(&self, key: &str, cx: &gpui::App) -> String {
        let file = self
            .pool
            .runtimes
            .get(key)
            .and_then(|rt| rt.read(cx).file.clone());
        let raw = file
            .and_then(|f| {
                self.psp
                    .projects
                    .iter()
                    .flat_map(|g| &g.sessions)
                    .find(|s| same_path(&s.path, &f))
                    .and_then(|s| {
                        s.name
                            .clone()
                            .filter(|n| !n.trim().is_empty())
                            .or_else(|| Some(s.preview.clone()).filter(|p| !p.trim().is_empty()))
                    })
            })
            .unwrap_or_else(|| tr("session.new").to_string());
        // 首行 + ≤30 字
        let first_line = raw.lines().next().unwrap_or("").trim().to_string();
        let mut out: String = first_line.chars().take(30).collect();
        if first_line.chars().count() > 30 {
            out.push('…');
        }
        out
    }
}
