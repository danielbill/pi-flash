//! Session pool: new/open/delete.

//! Split out of main.rs for the file-size budget. Child module of the
//! crate root: Chat's root-private fields stay accessible here.

use crate::*;

impl Chat {
    pub(crate) fn new_session(&mut self, cx: &mut Context<Self>) {
        // lazy draft (pi-web parity): the pi process spawns on the first
        // prompt, not now; running sessions are untouched
        let key = format!("draft-{}", self.draft_seq);
        self.draft_seq += 1;
        let rt = cx.new(|_| {
            let mut r = session::runtime::SessionRuntime::new(key.clone(), self.cwd.clone(), None);
            r.default_model = services::workspace::default_model_pref();
            r.status = tr("新会话").to_string();
            r
        });
        self.runtimes.insert(key, rt.clone());
        self.subscribe_runtime(&rt, cx);
        clear_last_open(&self.cwd.to_string_lossy());
        self.renaming = None;
        self.rename_input = None;
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
        if let Some(entity) = self.runtimes.remove(&old_key) {
            self.runtimes.insert(path_key.clone(), entity);
        }
        if self.active_key == old_key {
            self.active_key = path_key.clone();
        }
        self.active_file = Some(path.clone());
        // recents (003-session管理): a fresh session file just landed
        pi_link::recents::touch_recent(&path);
        set_last_open(&rt.read(cx).cwd.to_string_lossy(), &path_key);
        self.refresh_sessions();
        cx.notify();
    }

    pub(crate) fn open_session(&mut self, path: PathBuf, rename: bool, cx: &mut Context<Self>) {
        // recents (003-session管理): opening is activity — promote to the
        // top of the recent-activity list (persist throttled)
        pi_link::recents::touch_recent(&path);
        // cwd lookup: current project first, then all psp groups (v54 跨项目)
        let cwd = self
            .sessions
            .iter()
            .find(|s| s.path == path)
            .map(|s| PathBuf::from(s.cwd.clone()))
            .or_else(|| {
                self.projects
                    .iter()
                    .find(|g| g.sessions.iter().any(|s| s.path == path))
                    .map(|g| g.path.clone())
            })
            .unwrap_or_else(|| self.cwd.clone());
        let key = path.to_string_lossy().to_string();
        // pool hit: switch attention — instant, all messages in place
        if let Some(rt) = self.runtimes.get(&key).cloned() {
            self.cwd = cwd;
            self.branch = read_branch(&self.cwd);
            self.renaming = None;
            self.rename_input = None;
            self.confirm_delete = None;
            if rename {
                rt.update(cx, |r, _| r.pending_rename = true);
            }
            self.switch_to(rt, cx);
            return;
        }
        // new runtime: tail render first (cache or disk), process spawns in
        // the background — the conversation is on screen before node exists
        let tail = match self.session_tail_cache.get(&path).cloned() {
            Some(msgs) => msgs,
            None => {
                let msgs = msgs_from_tail(read_tail_messages(&path, 256 * 1024, 100));
                if self.session_tail_cache.len() >= crate::services::workspace::TAIL_PRELOAD * 2 {
                    self.session_tail_cache.clear();
                }
                self.session_tail_cache.insert(path.clone(), msgs.clone());
                msgs
            }
        };
        let rt = cx.new(|_| {
            let mut r = session::runtime::SessionRuntime::new(key, cwd.clone(), Some(path.clone()));
            r.default_model = services::workspace::default_model_pref();
            // leaf-chain integrity probe for the get_messages guard
            r.disk_msg_count = pi_link::sessions::count_message_entries(&path) as usize;
            r.messages = tail;
            r.pager.reload(r.messages.len());
            r.status = "resuming".into();
            r.pending_rename = rename;
            r
        });
        self.runtimes.insert(rt.read(cx).key.clone(), rt.clone());
        self.subscribe_runtime(&rt, cx);
        if !same_ws(&cwd.to_string_lossy(), &self.cwd.to_string_lossy()) {
            self.cwd = cwd;
            self.branch = read_branch(&self.cwd);
            self.expanded_dirs.clear();
            // 文件树：根项目行默认展开
            self.expanded_dirs.insert(self.cwd.clone());
            self.refresh_sessions();
            self.load_project_files();
            // 换工作区：git 状态/树缓存/fs watcher 全部跟着 cwd 重挂
            //（此前这里漏 refresh_git，git 面板会显示上一个项目的状态）
            self.refresh_git();
            self.attach_fs_watch();
        }
        set_last_open(&self.cwd.to_string_lossy(), &path.to_string_lossy());
        self.renaming = None;
        self.rename_input = None;
        self.confirm_delete = None;
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
                if let Some(s) = &r.agent.session {
                    let _ = s.send(&Command::GetMessages);
                }
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
        self.confirm_delete = None;
        let key = path.to_string_lossy().to_string();
        // 会话没了，它的自定义插件清单也清掉（034）
        if let Some(store) = pi_link::session_ext::store_path() {
            let _ = pi_link::session_ext::remove_for(&store, &key);
        }
        let was_active = self.runtimes.get(&self.active_key)
            .map(|rt| rt.read(cx).file.as_deref() == Some(path.as_path()))
            .unwrap_or(false);
        // tear down the runtime (abort + process kill; dropping the entity
        // clears the file handle on Windows before unlink)
        if let Some(rt) = self.runtimes.remove(&key) {
            rt.update(cx, |r, _| {
                if let Some(s) = &r.agent.session {
                    let _ = s.send(&Command::Abort);
                }
                r.shutdown_process();
            });
        }
        self.unread.remove(&path);
        self.turn_errors.remove(&path);
        self.turn_warnings.remove(&path);
        // recents (003-session管理): the file is gone, drop the entry
        pi_link::recents::remove_recent(&path);
        if was_active {
            self.active_file = None;
            self.new_session(cx);
        }
        match std::fs::remove_file(&path) {
            Ok(_) => {
                self.sessions.retain(|s| s.path != path);
                self.set_status("session deleted".into(), cx);
            }
            Err(e) => {
                self.set_status(crate::i18n::tf("删除失败: {e}", &[("e", e.to_string())]), cx);
            }
        }
        self.refresh_sessions();
        cx.notify();
    }
}
