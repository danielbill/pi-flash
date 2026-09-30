//! Dialog openers + session content search.

//! Split out of main.rs for the file-size budget. Child module of the
//! crate root: Chat's root-private fields stay accessible here.

use crate::*;

impl Chat {
    pub(crate) fn open_git_diff(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let untracked = self
            .git_files
            .iter()
            .any(|f| f.path == path && f.status == GitStatus::Untracked);
        let patch = git_file_diff(&self.cwd, &path, untracked);
        self.dialog = Some(Dialog::GitDiff { path, patch });
        cx.notify();
    }


    /// Open the session content search (013): query input + grouped results.
    pub(crate) fn open_session_search(&mut self, cx: &mut Context<Self>) {
        let weak = cx.weak_entity();
        let weak_esc = weak.clone();
        let input = cx.new(|cx| {
            TextInput::new(cx)
                .placeholder(tr("搜索会话内容…"))
                .on_change(Box::new(move |q: &str, cx: &mut App| {
                    let _ = weak.update(cx, |c, cx| c.search_changed(q, cx));
                }))
                .on_escape(Box::new(move |cx: &mut App| {
                    let _ = weak_esc.update(cx, |c, cx| {
                        c.dialog = None;
                        cx.notify();
                    });
                }))
        });
        self.search_hits.clear();
        self.search_truncated = false;
        self.search_needle.clear();
        self.dialog = Some(Dialog::SessionSearch { input });
        cx.notify();
    }

    /// Query text changed: bump the generation, debounce 300ms, then scan
    /// this project's session files on the background executor (pi-web
    /// SessionSearch debounce parity; stale responses drop by generation).
    pub(crate) fn search_changed(&mut self, q: &str, cx: &mut Context<Self>) {
        self.search_gen += 1;
        let epoch = self.search_gen;
        let needle = q.trim().to_string();
        self.search_needle = needle.clone();
        self.search_hits.clear();
        self.search_truncated = false;
        if needle.is_empty() {
            self.search_running = false;
            cx.notify();
            return;
        }
        self.search_running = true;
        let cwd = self.cwd.clone();
        cx.spawn(async move |weak, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(300))
                .await;
            let cancelled = weak.update(cx, |c, _| c.search_gen != epoch).unwrap_or(true);
            if cancelled {
                return;
            }
            let needle2 = needle.clone();
            let resp = cx
                .background_spawn(async move {
                    pi_link::sessions::search_sessions_for_cwd(
                        &cwd.to_string_lossy(),
                        &needle2,
                    )
                })
                .await;
            let _ = weak.update(cx, |c, cx| {
                if c.search_gen != epoch {
                    return;
                }
                c.search_running = false;
                c.search_truncated = resp.truncated;
                c.search_hits = resp.hits;
                cx.notify();
            });
        })
        .detach();
    }

    /// Jump to a search hit: open (or switch to) the session, then reveal the
    /// matched row — located by payload timestamp, falling back to the first
    /// message containing the needle.
    pub(crate) fn jump_to_hit(&mut self, path: PathBuf, ts: Option<i64>, cx: &mut Context<Self>) {
        let needle = self.search_needle.clone();
        self.dialog = None;
        let key = path.to_string_lossy().to_string();
        let already_open = self.active_key == key || self.runtimes.contains_key(&key);
        if self.active_key != key {
            self.open_session(path.clone(), false, cx);
        }
        let rt = self.rt();
        let located = rt.update(cx, |r, cx| r.locate_message(ts, &needle, cx));
        if !located && !already_open {
            // messages still loading (pool miss) — apply after the reconcile
            self.pending_locate = Some((path, ts, needle));
        }
        cx.notify();
    }
}
