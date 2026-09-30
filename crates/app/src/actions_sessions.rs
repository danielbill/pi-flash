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

    pub(crate) fn open_session(&mut self, path: PathBuf, rename: bool, cx: &mut Context<Self>) {
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
                if self.session_tail_cache.len() >= preload_sessions() * 2 {
                    self.session_tail_cache.clear();
                }
                self.session_tail_cache.insert(path.clone(), msgs.clone());
                msgs
            }
        };
        let rt = cx.new(|_| {
            let mut r = session::runtime::SessionRuntime::new(key, cwd.clone(), Some(path.clone()));
            r.messages = tail;
            r.list.reset(r.messages.len());
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
            self.refresh_sessions();
            self.load_project_files();
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
                    let _ = s.send(&Command::GetTree);
                }
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
        self.running_files.remove(&path);
        self.unread.remove(&path);
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
