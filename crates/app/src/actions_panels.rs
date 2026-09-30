//! File preview tabs + attachments + v54 psp 项目动作（选目录开项目、组内
//! 新会话、终端定向打开、删除项目、资源管理器打开）。

//! Split out of main.rs for the file-size budget. Child module of the
//! crate root: Chat's root-private fields stay accessible here.

use crate::*;

/// 用系统默认浏览器打开文件（html/htm）。
pub(crate) fn open_in_browser(path: &std::path::Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
}

/// 在系统文件浏览器中打开目录（Windows explorer / macOS open）。
pub(crate) fn open_in_explorer(path: &std::path::Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer")
            .arg(path)
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
}

impl Chat {
    /// psp「打开项目」：目录选择器 → 切换工作区。
    pub(crate) fn pick_project_folder(&mut self, cx: &mut Context<Self>) {
        let opts = gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        };
        let rx = cx.prompt_for_paths(opts);
        cx.spawn(async move |this, cx| {
            let picked = rx.await.ok().and_then(|r| r.ok()).flatten();
            let Some(mut paths) = picked else {
                return;
            };
            if let Some(dir) = paths.pop() {
                let _ = this.update(cx, |c, cx| {
                    c.switch_project(dir, cx);
                });
            }
        })
        .detach();
    }

    /// psp 项目行 ＋：切换到该项目并新建会话（组展开、选中）。
    pub(crate) fn new_session_in(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !same_path(&self.cwd, &path) {
            self.switch_project(path, cx);
        }
        self.new_session(cx);
    }

    /// psp 项目菜单「在终端中打开」：以该项目为 cwd 开终端 + 切内容区 tab。
    pub(crate) fn open_terminal_in(
        &mut self,
        cwd: PathBuf,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.open_terminal(Some(cwd), window, cx);
        self.set_content_view(ContentView::Term);
        cx.notify();
    }

    /// psp「删除项目及所有会话」：删除该项目全部会话文件（活跃会话先中止
    /// 并重置为新草稿），从 psp 移除该组。
    pub(crate) fn delete_project(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let sessions: Vec<PathBuf> = self
            .projects
            .iter()
            .find(|g| same_path(&g.path, &path))
            .map(|g| g.sessions.iter().map(|s| s.path.clone()).collect())
            .unwrap_or_default();
        for p in &sessions {
            if self
                .runtimes
                .get(&self.active_key)
                .map(|rt| rt.read(cx).file.as_deref() == Some(p.as_path()))
                .unwrap_or(false)
            {
                self.new_session(cx);
            }
            if let Some(rt) = self.runtimes.remove(&p.to_string_lossy().to_string()) {
                rt.update(cx, |r, _| {
                    if let Some(s) = &r.agent.session {
                        let _ = s.send(&Command::Abort);
                    }
                    r.shutdown_process();
                });
            }
            let _ = std::fs::remove_file(p);
        }
        self.projects.retain(|g| !same_path(&g.path, &path));
        if same_path(&self.cwd, &path) {
            // 当前项目被删：清当前组数据，保留草稿会话
            self.sessions.clear();
            self.sync_current_sessions();
        }
        self.unread.retain(|p| {
            sessions
                .iter()
                .all(|s| !same_path(s, p))
        });
        self.set_status(
            crate::i18n::tf("项目已删除（{n} 个会话）", &[("n", sessions.len().to_string())]),
            cx,
        );
        cx.notify();
    }

    /// v54.4：文件打开路由——html/htm 交系统浏览器（webview 内嵌在 gpui
    /// 上不可行，绕过）；其余统一打开为内容区 tab（md 渲染、源码带行号）。
    pub(crate) fn open_file_tab(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let is_html = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "html" | "htm"));
        if is_html {
            open_in_browser(&path);
            return;
        }
        const MAX: u64 = 400 * 1024;
        let too_big = std::fs::metadata(&path).map(|m| m.len() > MAX).unwrap_or(false);
        let content = if too_big {
            "(file too large to preview)".to_string()
        } else {
            match std::fs::read(&path) {
                Ok(bytes) => {
                    if bytes.contains(&0) {
                        "(binary file)".to_string()
                    } else {
                        String::from_utf8_lossy(&bytes).to_string()
                    }
                }
                Err(e) => format!("read failed: {e}"),
            }
        };
        self.file_cache.insert(path.clone(), FileTab { content });
        // 已打开则只切过去
        if let Some(ix) = self
            .panel_tabs
            .iter()
            .position(|t| matches!(t, PanelTab::File(p) if same_path(p, &path)))
        {
            self.active_panel_tab = Some(ix);
            self.set_content_view(ContentView::File);
            cx.notify();
            return;
        }
        self.panel_tabs.push(PanelTab::File(path));
        self.active_panel_tab = Some(self.panel_tabs.len() - 1);
        self.set_content_view(ContentView::File);
        cx.notify();
    }

    pub(crate) fn close_panel_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.panel_tabs.len() {
            return;
        }
        let removed = self.panel_tabs.remove(ix);
        if let PanelTab::Term(id) = &removed {
            if let Some(tix) = self.terminals.iter().position(|t| t.id == *id) {
                let _ = self.terminals[tix]
                    .pty
                    .send(alacritty_terminal::event_loop::Msg::Shutdown);
                self.terminals.remove(tix);
            }
        }
        self.active_panel_tab = match self.active_panel_tab {
            Some(a) if a >= self.panel_tabs.len() => {
                if self.panel_tabs.is_empty() {
                    None
                } else {
                    Some(a.saturating_sub(1))
                }
            }
            other => other,
        };
        cx.notify();
    }

    /// File viewer meta line: language · lines · size (pi-web FileViewer).
    pub(crate) fn file_meta(path: &Path, content: &str) -> String {
        let lang = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("txt")
            .to_string();
        let lines = content.lines().count();
        let bytes = content.len();
        let size = if bytes < 1024 {
            format!("{bytes} B")
        } else {
            format!("{:.1} KB", bytes as f64 / 1024.)
        };
        format!("{lang} · {lines} lines · {size}")
    }

    pub(crate) fn attach_images(&mut self, cx: &mut Context<Self>) {
        let opts = gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        };
        let rx = cx.prompt_for_paths(opts);
        cx.spawn(async move |this, cx| {
            let picked = rx.await.ok().and_then(|r| r.ok()).flatten();
            let Some(paths) = picked else {
                return;
            };
            let _ = this.update(cx, |chat, cx| {
                for path in paths {
                    let Ok(bytes) = std::fs::read(&path) else { continue };
                    use base64::Engine as _;
                    let data_b64 =
                        base64::engine::general_purpose::STANDARD.encode(&bytes);
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "image".into());
                    let mime = mime_from_ext(&path);
                    chat.pending_images
                        .push(AttachedImage { name, data_b64, mime });
                }
                if !chat.pending_images.is_empty() {
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

impl Chat {
    /// psp 详情卡改名提交（Enter）：对目标会话的 runtime 发
    /// SetSessionName；未加载的会话临时 spawn 进程发完即收。名字先乐观
    /// 写入内存，扫描器随后从会话文件回读确认。
    pub(crate) fn rename_card_commit(
        &mut self,
        path: &PathBuf,
        name: String,
        cx: &mut Context<Self>,
    ) {
        self.hover_card = None;
        if name.is_empty() {
            cx.notify();
            return;
        }
        let name_for_status = name.clone();
        let key = path.to_string_lossy().to_string();
        match self.runtimes.get(&key).cloned() {
            Some(rt) => {
                rt.update(cx, |r, _| {
                    if let Some(s) = &r.agent.session {
                        let _ = s.send(&Command::SetSessionName { name: name.clone() });
                    }
                });
            }
            None => {
                // 未加载的会话：tail 渲染 + 临时进程发改名，不切换活跃会话
                let tail = match self.session_tail_cache.get(path).cloned() {
                    Some(msgs) => msgs,
                    None => msgs_from_tail(read_tail_messages(path, 256 * 1024, 100)),
                };
                let cwd = self
                    .projects
                    .iter()
                    .find(|g| g.sessions.iter().any(|s| same_path(&s.path, path)))
                    .map(|g| g.path.clone())
                    .unwrap_or_else(|| self.cwd.clone());
                let rt = cx.new(|_| {
                    let mut r = session::runtime::SessionRuntime::new(
                        key.clone(),
                        cwd,
                        Some(path.clone()),
                    );
                    r.messages = tail;
                    r.list.reset(r.messages.len());
                    r
                });
                self.runtimes.insert(key, rt.clone());
                self.subscribe_runtime(&rt, cx);
                let rt_spawn = rt.clone();
                cx.spawn(async move |_this, cx| {
                    let _ = rt_spawn.update(cx, |r, cx| {
                        if r.agent.session.is_none() {
                            if let Some(rx) = r.spawn() {
                                let epoch = r.agent.epoch;
                                session::runtime::SessionRuntime::attach_pump(
                                    &rt_spawn, rx, epoch, cx,
                                );
                            }
                        }
                        if let Some(s) = &r.agent.session {
                            let _ = s.send(&Command::SetSessionName { name: name.clone() });
                        }
                    });
                    // 改名发完即收进程（消息保留，下次打开秒开）
                    let _ = rt_spawn.update(cx, |r, _| r.shutdown_process());
                })
                .detach();
            }
        }
        // 乐观更新内存中的标题
        for g in &mut self.projects {
            for s in &mut g.sessions {
                if same_path(&s.path, path) {
                    s.name = Some(name_for_status.clone());
                }
            }
        }
        self.set_status(tr("已改名").to_string(), cx);
        cx.notify();
    }

    /// psp 详情卡改名取消（Esc）。
    pub(crate) fn rename_card_cancel(&mut self, _path: &PathBuf, cx: &mut Context<Self>) {
        if let Some(card) = self.hover_card.as_mut() {
            card.renaming = false;
            card.rename_input = None;
        }
        cx.notify();
    }
}
