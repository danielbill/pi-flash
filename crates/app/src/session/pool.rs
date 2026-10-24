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
                    // 微信端与桌面端**并行**拿到同一请求（060 §4.1）：两边都要
                    // 渲染一份文本，但只有一方能应答 —— 桌面 ext_respond 会
                    // clear_pending，pi 侧也只 resolve 一次（§8 档 2 不重复消费）
                    chat.aux.remote.on_ext_ui(req);
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
                SessionEvent::Assistant(ev) => {
                    // 回推口径（2026-10-24 修正）：**只认微信线**。
                    // 2026-10-23 那版放的是「桌面活跃会话**或**微信线」——
                    // 当时微信线就是桌面活跃会话，放行 is_active 才收得到
                    // 回复；微信线升格独立会话后，这条规则变成把桌面会话
                    // 的回复（乃至别的会话的启动横幅）原样灌进微信，用户
                    // 实测「别的对话进入了微信会话」。
                    //
                    // 桌面与微信已经**各是一条会话**：微信只该收到微信线
                    // 自己的回复。且只在**发送轮内**转发：pi 空闲期吐的
                    // 启动横幅之类不得混进回复通道。
                    let key = rt.read(cx).key.clone();
                    let running = rt.read(cx).agent_running;
                    let is_wx_line = chat.aux.remote.wx_session.as_deref() == Some(key.as_str());
                    if is_wx_line && running {
                        for text in chat.aux.remote.on_assistant(&ev, &key) {
                            chat.aux.remote.send(&text);
                        }
                    }
                }
                SessionEvent::Models(models) => {
                    // 进程答案写项目槽（pi-web /api/models parity：每个 runtime 只写
                    // 自己 cwd 的条目），同时**并进全局态并回写自有缓存**
                    // （010-启动.md §2/§4.1：包内联 provider 只有进程答得到，
                    // 存下来下次冷启动就有真名字）。
                    let cwd_key = rt.read(cx).cwd.to_string_lossy().to_string();
                    chat.pool.models_by_cwd.insert(cwd_key, models.clone());
                    if crate::startup::merge_models(&mut chat.pool.globals, &models) {
                        let _ = crate::startup::write_cache(
                            &chat.pool.globals.models,
                            &chat.pool.globals.commands,
                        );
                    }
                    chat.cfg.mc_state = models_config::compute_state(
                        chat.cfg.mc_patterns.as_ref(),
                        &chat.mc_refs(),
                    );
                    cx.notify();
                }
                SessionEvent::Commands(commands) => {
                    // 扩展命令（包内注册，磁盘没有数据文件）：并进全局清单 + 回写缓存。
                    // `skill:` 前缀不入全局（skill 走磁盘派生，见 startup::merge_commands）。
                    if crate::startup::merge_commands(&mut chat.pool.globals, &commands) {
                        let _ = crate::startup::write_cache(
                            &chat.pool.globals.models,
                            &chat.pool.globals.commands,
                        );
                    }
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
        // 非微信线的活跃会话记为「桌面会话」：微信线单开 tab（2026-10-24
        // 定夺），查看微信时 topbar 第一格继续显示并回到它
        if self.aux.remote.wx_session.as_deref() != Some(key.as_str()) {
            self.aux.remote.desktop_key = Some(key.clone());
        }
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
            // 无项目态不存在可切会话（switch_pool_rt 已先重挂），防御式取 Some
            if let Some(cwd) = &self.ws.cwd {
                set_last_open(&cwd.to_string_lossy(), &f.to_string_lossy());
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
    /// 新会话草稿（用户显式新建：psp ＋、新建按钮、删会话后归位）。
    /// `clear_last_open` 跟在这里：用户明确要空白会话 ⇒ 该项目不再恢复旧会话。
    pub(crate) fn new_session(&mut self, cx: &mut Context<Self>) {
        self.new_session_with(cx, true);
    }

    /// 微信 `/新建`：建草稿**不切桌面**（bot 语境自己的会话线，对齐 ZCode
    /// 「bot 的 activeTask 独立于桌面选中态」）。返回新草稿 key；焦点、
    /// 活跃态、per-project 会话记忆全不动。首条 prompt 后草稿键经
    /// `on_file_bound` 迁移到会话文件路径，微信线跟随。
    pub(crate) fn new_draft_detached(&mut self, cx: &mut Context<Self>) -> Option<String> {
        let cwd = self.ws.cwd.clone()?;
        let key = format!("draft-{}", self.pool.draft_seq);
        self.pool.draft_seq += 1;
        let rt = cx.new(|_| {
            let mut r = session::runtime::SessionRuntime::new(key.clone(), cwd, None);
            r.default_model = services::workspace::default_model_pref();
            r.status = tr("session.new").to_string();
            r
        });
        self.pool.runtimes.insert(key.clone(), rt.clone());
        self.subscribe_runtime(&rt, cx);
        Some(key)
    }

    /// 后台唤醒既有会话（微信线投递用）：建 runtime + 订阅 + 尾窗快照，
    /// **不动桌面注意力**——`active_key`/`ws.cwd`/侧栏选中/last-open 全部
    /// 原样。进程不在这里 spawn（懒 spawn 惯例，与草稿同）：调用方随后的
    /// 首条投递经 `ensure_spawned` 同步拉起。已在池中直接返回。
    pub(crate) fn open_session_detached(
        &mut self,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Entity<session::runtime::SessionRuntime>> {
        let path = pi_link::sessions::canonical_session_path(&path);
        let key = path.to_string_lossy().to_string();
        if let Some(rt) = self.pool.runtimes.get(&key).cloned() {
            return Some(rt);
        }
        // cwd 解析口径同 open_session：现项目 → 项目组 → 当前工作区兜底
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
            .or_else(|| self.ws.cwd.clone())?;
        let tail = match self.pool.session_tail_cache.get(&path).cloned() {
            Some(msgs) => msgs,
            None => {
                let msgs = msgs_from_tail(read_tail_messages(&path, 256 * 1024, 100));
                self.pool
                    .session_tail_cache
                    .insert(path.clone(), msgs.clone());
                msgs
            }
        };
        let rt = cx.new(|_| {
            let mut r = session::runtime::SessionRuntime::new(key, cwd, Some(path));
            r.default_model = services::workspace::default_model_pref();
            r.messages = tail;
            r.pager.reload(r.messages.len());
            r.status = "resuming".into();
            r
        });
        self.pool
            .runtimes
            .insert(rt.read(cx).key.clone(), rt.clone());
        self.subscribe_runtime(&rt, cx);
        Some(rt)
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
        if let Some(entity) = self.pool.runtimes.remove(&old_key) {
            self.pool.runtimes.insert(path_key.clone(), entity);
        }
        if self.pool.active_key == old_key {
            self.pool.active_key = path_key.clone();
        }
        // 微信线跟随迁移：草稿键 → 会话文件路径键（否则微信 /新建 后第一条
        // 消息一落地，归属就指向已退役的 draft-N，后续全部回退活跃会话）
        if self.aux.remote.wx_session.as_deref() == Some(old_key.as_str()) {
            self.aux.remote.set_wx_session(path_key.clone());
            // 影子会话落盘：名字补进过滤清单（按 path 精确，重启后仍藏得住）
            let name = self.aux.remote.wx_session_name.clone();
            pi_link::hidden_sessions::set_owned(
                pi_link::hidden_sessions::OWNER_WX,
                name.as_deref(),
                Some(&path),
            );
            // 落盘后名字再钉一次（草稿期 pi 可能只记在内存里）
            if let Some(n) = name {
                if let Some(rt) = self.pool.runtimes.get(&path_key) {
                    rt.read(cx).rpc(&Command::SetSessionName { name: n });
                }
            }
        }
        self.psp.active_file = Some(path.clone());
        // recents (003-session管理): a fresh session file just landed
        pi_link::recents::touch_recent(&path);
        set_last_open(&rt.read(cx).cwd.to_string_lossy(), &path_key);
        // 新会话落地的项目组若处于折叠态则自动展开：伪标签（draft）在折叠
        // 组里照常显示，FileBound 一换真身就被折叠藏掉——用户眼里刚发的
        // 会话凭空消失。fork / 微信端新会话同经此处（2026-10-15 用户报障）。
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
            set_last_open(&cur.to_string_lossy(), &path.to_string_lossy());
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
        // 影子会话（微信线 pf-wxbot-*）不进任何列表：过滤清单由模块自己注入
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

    /// 指定池 key 的会话标题（topbar 第一格在查看微信 tab 时显示桌面会话用）。
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
