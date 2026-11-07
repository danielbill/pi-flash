//! 会话打开/删除与页签管理：open_session（transcript 回放挂接）、
//! delete_session、主 tab 激活与观察窗关闭。

use super::*;

impl Chat {
    /// 子会话 tab 标题（044-0：任务名）。pi-flash-9eck：descriptor/catalog
    /// 已删——标题从主会话消息反查（agent 调用 args 的 label → profile →
    /// prompt 首行），查不到给占位名「子代理」（零读盘、零后台任务）。
    pub(crate) fn subagent_tab_label(
        &mut self,
        child: &Path,
        _key: &str,
        cx: &mut Context<Self>,
    ) -> String {
        let placeholder = || tr("subagent.tab").to_string();
        self.label_for_child(cx, child).unwrap_or_else(placeholder)
    }

    pub(crate) fn open_session(&mut self, path: PathBuf, rename: bool, cx: &mut Context<Self>) {
        // 入参可能带历史字符串形态（recents/last-open 存过 pi 自拼的 group
        // 大小写）：先归一到扫描器形态再入池——pool key、侧栏行匹配、通知
        // 点集合共用同一条字符串身份，形态分裂就会点行重复 spawn runtime
        let path = pi_link::sessions::canonical_session_path(&path);
        // 子会话判定（044-0 只读观察窗）：头部有 subagent/child marker 的
        // 会话 = 观察窗——永不 spawn 进程（驱动权归父 pi），无输入通道。
        // 判定在 touch_recent 之前：子会话不进最近列表（侧栏扫描器同口径）。
        let observer = pi_link::sessions::is_subagent_session(&path);
        // recents (003-session管理): opening is activity — promote to the
        // top of the recent-activity list (persist throttled)
        if !observer {
            pi_link::recents::touch_recent(&path);
        }
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
                // 空 tail 不入缓存（pi-flash-9eck）：老洪泛会话尾段垃圾行
                // 读空时缓存空 Vec 会让每次重开都复用空投影、永远走慢放行
                // 链；扩窗（read_tail_messages 内建）读空说明确实真空。
                if !msgs.is_empty() {
                    if self.pool.session_tail_cache.len()
                        >= crate::services::workspace::TAIL_PRELOAD * 2
                    {
                        self.pool.session_tail_cache.clear();
                    }
                    self.pool
                        .session_tail_cache
                        .insert(path.clone(), msgs.clone());
                }
                msgs
            }
        };
        let rt = cx.new(|_| {
            let mut r = session::runtime::SessionRuntime::new(key, cwd.clone(), Some(path.clone()));
            r.default_model = services::workspace::default_model_pref();
            r.observer = observer;
            r.messages = tail;
            // tail 有内容 = 打开即有东西看，加载页不拦；空尾窗（文件刚建 /
            // 老洪泛会话尾段全是 custom 条目）等 get_messages 投影放行
            r.messages_loaded = !r.messages.is_empty();
            r.pager.reload(r.messages.len());
            r.status = if observer {
                "observer".into()
            } else {
                "resuming".into()
            };
            r.pending_rename = rename;
            r
        });
        // observer 子会话的 tab 标题 = 任务名（044-0 验收）：主会话消息反查
        // （agent 调用 args 的 label → profile → prompt 首行），查不到占位
        // 「子代理」。父会话 = 当前激活会话（面板行/compact 卡点击两条路都
        // 满足）。
        if observer {
            let new_key = rt.read(cx).key.clone();
            let label = self.subagent_tab_label(&path, &new_key, cx);
            self.pool.tab_labels.insert(new_key.clone(), label);
            // 观察窗 = 自由区普通 tab（044-0 验收②：追加**末尾**，不抢固定
            // 段第一位）；已开过（面板行重复点）只激活不重复 push
            let existing =
                self.ws.panel_tabs.iter().position(
                    |t| matches!(t, crate::PanelTab::Observer { key: k } if *k == new_key),
                );
            match existing {
                Some(ix) => self.activate_panel_tab(ix, cx),
                None => {
                    self.ws
                        .panel_tabs
                        .push(crate::PanelTab::Observer { key: new_key });
                    self.activate_panel_tab(self.ws.panel_tabs.len() - 1, cx);
                }
            }
        }
        self.pool
            .runtimes
            .insert(rt.read(cx).key.clone(), rt.clone());
        self.subscribe_runtime(&rt, cx);
        // 005 预热：挂载台账里该会话名下有 pi 扩展 → 立即拉起进程。pi 只在
        // 发送时懒加载（input 层 ensure_spawned），重启后绑定会话进程不活 →
        // ext 无处安身，微信消息无人收（真机 2026-11-03）。通用判据 = 台账
        // （session-ext.json），零插件知识。
        // 判据要过 090-10 版本层自愈：插件升级后台账里只剩旧版本的死路径，
        // 按原样判非空会为一个再也挂不上任何 ext 的会话白起进程（且 spawn
        // 时那条死 `-e` 还会让其立刻退出）。
        // 只读观察窗不预热（永不 spawn，044-0）。
        if !observer {
            let key = rt.read(cx).key.clone();
            let has_ext = pi_link::session_ext::store_path()
                .map(|p| {
                    let raw = pi_link::session_ext::read_for(&p, &key);
                    !pi_link::session_ext::resolve_sources(&raw).0.is_empty()
                })
                .unwrap_or(false);
            if has_ext {
                rt.update(cx, |r, cx| {
                    r.ensure_spawned(cx);
                });
            }
        }
        // 秒开深度优化（pi-flash-2o92）：磁盘计数改后台——主线程全文件读
        // 是 O(会话大小) 的打开冻结源（观察窗跳过：全量快照自己计数）
        if !observer {
            rt.update(cx, |r, cx| r.load_disk_msg_count(cx));
        }
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
                if r.observer {
                    // 观察窗：无进程可起，磁盘链即真相；全量快照后台装载
                    // （tail 只是首帧），此后外部追加靠 3s tick 跟随（044-0）
                    r.spawn_observer_snapshot(cx);
                    return;
                }
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
        // 会话没了：tab 摘牌——标题快照、主会话指针（was_active 分支的
        // new_session 会 switch 进新草稿并接管 primary）、自由区观察窗 tab
        //（close_panel_tab 负责激活位下标平移）
        self.pool.tab_labels.remove(&key);
        if self.pool.primary_key == key {
            self.pool.primary_key.clear();
        }
        while let Some(ix) = self
            .ws
            .panel_tabs
            .iter()
            .position(|t| matches!(t, crate::PanelTab::Observer { key: k } if *k == key))
        {
            self.close_panel_tab(ix, cx);
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

    /// 点固定段主会话 tab：回到主会话/新会话（primary）。runtime 驻留 →
    /// switch_pool_rt（切注意力，switch_to 顺带清自由区激活位）；空闲退役
    /// 被摘的 → open_session 秒开重开（primary 即会话文件路径；draft 不退役）。
    pub(crate) fn activate_primary_tab(&mut self, cx: &mut Context<Self>) {
        let key = self.pool.primary_key.clone();
        match self.pool.runtimes.get(&key).cloned() {
            Some(rt) => self.switch_pool_rt(rt, cx),
            None if !key.is_empty() => self.open_session(PathBuf::from(&key), false, cx),
            None => {
                self.set_content_view(crate::ContentView::Chat);
                cx.notify();
            }
        }
    }

    /// 关一枚观察窗 tab（自由区 × / 右键菜单 Close 同源）：observer 无进程，
    /// runtime 纯 drop 即可（不 abort 不 shutdown）。tab 摘除走
    /// close_panel_tab（激活位下标平移）。被关的是前台时切回主会话
    /// （primary）；primary 不在池 → 落新草稿（同 delete_session was_active）。
    /// tail cache 不动——再点面板行秒开重开。
    pub(crate) fn close_observer_tab(&mut self, key: &str, cx: &mut Context<Self>) {
        let is_observer = self
            .pool
            .runtimes
            .get(key)
            .map(|rt| rt.read(cx).observer)
            .unwrap_or(false);
        if !is_observer {
            return;
        }
        self.pool.runtimes.remove(key);
        self.pool.tab_labels.remove(key);
        while let Some(ix) = self
            .ws
            .panel_tabs
            .iter()
            .position(|t| matches!(t, crate::PanelTab::Observer { key: k } if k == key))
        {
            self.close_panel_tab(ix, cx);
        }
        if self.pool.active_key == key {
            self.psp.active_file = None;
            let next = self
                .pool
                .runtimes
                .get(&self.pool.primary_key)
                .cloned()
                .filter(|rt| !rt.read(cx).observer);
            match next {
                Some(rt) => {
                    self.switch_pool_rt(rt, cx);
                }
                None => {
                    self.pool.primary_key.clear();
                    self.new_session(cx);
                }
            }
        }
        cx.notify();
    }
}
