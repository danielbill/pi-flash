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


    /// 004 projectManager 打开项目菜单（触发：psp 打开项目 icon / 012 新会话页
    /// 操作栏）：搜索框 + 【打开文件夹】 + 最近 30 天活动项目列表。列表后台
    /// 扫描回填（进程级持久 Scanner，索引命中不重读盘）；「从某项目新建会话
    /// 打开」的默认选中在渲染期对 same_ws 打勾，见 render_project_picker。
    pub(crate) fn open_project_picker(&mut self, cx: &mut Context<Self>) {
        let weak = cx.weak_entity();
        let weak_esc = weak.clone();
        let input = cx.new(|cx| {
            TextInput::new(cx)
                .placeholder(tr("搜索项目…"))
                .on_change(Box::new(move |q: &str, cx: &mut App| {
                    let _ = weak.update(cx, |c, cx| {
                        c.project_filter = q.to_string();
                        cx.notify();
                    });
                }))
                .on_escape(Box::new(move |cx: &mut App| {
                    let _ = weak_esc.update(cx, |c, cx| {
                        c.dialog = None;
                        cx.notify();
                    });
                }))
        });
        self.project_hits.clear();
        self.project_filter.clear();
        self.dialog = Some(Dialog::ProjectPicker { input });
        cx.notify();
        // 后台聚合：全量会话按 cwd 聚合 → 30 天窗口（004 v2）→ 字母序。
        // 候选上限给足余量（索引命中，无盘读）；超深处项目极少见。
        cx.spawn(async move |weak, cx| {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let cutoff = now_ms - 30 * 86_400_000;
            let mut entries = cx
                .background_spawn(async move { collect_recent_projects(cutoff) })
                .await;
            let _ = weak.update(cx, |c, cx| {
                // 当前项目兜底入选（无 30 天活动也显示，004：默认选中该项目）
                let cwd_key = c.cwd.to_string_lossy().to_string();
                if !entries
                    .iter()
                    .any(|p| crate::services::workspace::same_ws(&p.path.to_string_lossy(), &cwd_key))
                {
                    let name = c
                        .cwd
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| cwd_key.clone());
                    entries.push(crate::ProjectEntry {
                        name,
                        path: c.cwd.clone(),
                    });
                    entries.sort_by(|a, b| project_sort_key(a).cmp(&project_sort_key(b)));
                }
                c.project_hits = entries;
                cx.notify();
            });
        })
        .detach();
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

    /// topbar ⋯ 菜单 → 系统提示词 / 工具定义弹窗（窗体 = 设置弹窗那套大卡片）。
    /// 同一面再点一次收起，换面直接切。
    ///
    /// 数据不在这里拉：pi 0.86+ 把系统提示词与工具声明写进 transcript 的
    /// system 消息，runtime 每次 get_messages 都 replay 一份（见
    /// `pi_link::transcript::transcript_system`），所以打开即是最新。
    pub(crate) fn open_session_info(&mut self, kind: crate::TopPanel, cx: &mut Context<Self>) {
        self.top_menu_open = false;
        self.dialog = if self.session_info_open(kind) {
            None
        } else {
            Some(Dialog::SessionInfo { kind })
        };
        cx.notify();
    }

    /// 该面的弹窗是否正开着（⋯ 菜单打勾用）。
    pub(crate) fn session_info_open(&self, kind: crate::TopPanel) -> bool {
        matches!(self.dialog, Some(Dialog::SessionInfo { kind: k }) if k == kind)
    }
}

/// 004 打开项目菜单的列表聚合：30 天窗口内有会话活动的项目（一个 cwd 一行，
/// 活动时刻不再保留），按项目名字母序输出（后台线程执行）。聚合键走
/// ws 归一化（Windows 盘符/大小写、正反斜杠差异同项目合并，首见路径上屏）。
fn collect_recent_projects(cutoff_ms: i64) -> Vec<crate::ProjectEntry> {
    let mut by_key: std::collections::HashMap<String, crate::ProjectEntry> =
        std::collections::HashMap::new();
    for s in pi_link::sessions::list_sessions(2000) {
        let ms = s
            .modified
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        if ms < cutoff_ms {
            continue;
        }
        let key = crate::services::workspace::same_ws_key(&s.cwd);
        if by_key.contains_key(&key) {
            continue;
        }
        let path = PathBuf::from(&s.cwd);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| s.cwd.clone());
        by_key.insert(key, crate::ProjectEntry { name, path });
    }
    let mut rows: Vec<crate::ProjectEntry> = by_key.into_values().collect();
    rows.sort_by(|a, b| project_sort_key(a).cmp(&project_sort_key(b)));
    rows
}

/// 字母序排序键：名字不分大小写，路径做稳定次序。
fn project_sort_key(p: &crate::ProjectEntry) -> (String, String) {
    (
        p.name.to_lowercase(),
        p.path.to_string_lossy().to_lowercase(),
    )
}
