//! File preview tabs + attachments + v54 psp 项目动作（选目录开项目、组内
//! 新会话、终端定向打开、删除项目、资源管理器打开）。

//! Split out of main.rs for the file-size budget. Child module of the
//! crate root: Chat's root-private fields stay accessible here.

use crate::*;

/// 自动保存静默期（023；Zed autosave after_timeout 同构）。
const AUTOSAVE_DEBOUNCE_MS: u64 = 1000;

/// 附件上限（pi-web image-attachments.ts parity）：10 张、单张解码后 10MB
pub(crate) const MAX_ATTACHED_IMAGES: usize = 10;
const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;

/// 由原始字节构造附件：base64 供 RPC、thumb 预构建供缩略图渲染。
pub(crate) fn attached_image_from_bytes(mime: String, bytes: Vec<u8>) -> AttachedImage {
    use base64::Engine as _;
    let data_b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let thumb = crate::session::messages::mime_to_image_format(&mime)
        .map(|f| std::sync::Arc::new(gpui::Image::from_bytes(f, bytes)));
    AttachedImage { data_b64, mime, thumb }
}

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
    /// 打开项目菜单的【打开文件夹】：目录选择器 → 切换工作区。`fresh`
    /// （新会话页来源）= 选完落**全新草稿**（new_session_in，不恢复该目录
    /// 的 last_open）；psp 来源保持恢复上次会话的既定行为。
    pub(crate) fn pick_project_folder(&mut self, fresh: bool, cx: &mut Context<Self>) {
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
                    if fresh {
                        c.new_session_in(dir, cx);
                    } else {
                        c.switch_project(dir, cx);
                    }
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
    /// 上不可行，绕过）；其余统一打开为内容区 tab。
    ///
    /// 023 改版：编辑器实体懒创建（InputState::new 要 window，本调用链
    /// 没有——渲染帧在 content.rs 里补），二进制/超限文件提示不打开。
    pub(crate) fn open_file_tab(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let is_html = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "html" | "htm"));
        if is_html {
            open_in_browser(&path);
            return;
        }
        const MAX: u64 = 10 * 1024 * 1024;
        if std::fs::metadata(&path).map(|m| m.len() > MAX).unwrap_or(false) {
            self.set_status(crate::i18n::tr("文件超过 10MB，不打开").to_string(), cx);
            return;
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                self.set_status(format!("{}: {e}", crate::i18n::tr("读取失败")), cx);
                return;
            }
        };
        if bytes.contains(&0) {
            self.set_status(crate::i18n::tr("二进制文件，不打开").to_string(), cx);
            return;
        }
        let content = String::from_utf8_lossy(&bytes).to_string();
        let sig = file_sig(&path);
        // 已打开则切过去；未脏顺手重读磁盘（可能被外部改过），脏则保留缓冲
        if let Some(ix) = self
            .panel_tabs
            .iter()
            .position(|t| matches!(t, PanelTab::File(p) if same_path(p, &path)))
        {
            self.activate_panel_tab(ix, cx);
            self.set_content_view(ContentView::File);
            self.pending_focus_file = Some(path.clone());
            if let Some(ft) = self.file_cache.get_mut(&path) {
                if !ft.dirty {
                    ft.content = content;
                    ft.reload_pending = true;
                    ft.conflict = None;
                    ft.disk_sig = sig;
                }
            }
            cx.notify();
            return;
        }
        let mut ft = FileTab::from_disk(content);
        ft.disk_sig = sig;
        self.file_cache.insert(path.clone(), ft);
        self.pending_focus_file = Some(path.clone());
        self.panel_tabs.push(PanelTab::File(path));
        let ix = self.panel_tabs.len() - 1;
        self.activate_panel_tab(ix, cx);
        self.set_content_view(ContentView::File);
        cx.notify();
    }

    /// 关文件 tab：有未保存修改（或挂着冲突）先弹确认，否则直接关。
    pub(crate) fn close_file_tab(&mut self, path: &Path, cx: &mut Context<Self>) {
        let dirty = self
            .file_cache
            .get(path)
            .map(|f| f.dirty || f.conflict.is_some())
            .unwrap_or(false);
        if dirty {
            self.dialog = Some(crate::Dialog::FileDirty { path: path.to_path_buf() });
            cx.notify();
            return;
        }
        self.discard_file_tab(path, cx);
    }

    /// 无条件丢弃文件 tab（确认弹窗三个按钮的公共尾）。
    pub(crate) fn discard_file_tab(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(ix) = self
            .panel_tabs
            .iter()
            .position(|t| matches!(t, PanelTab::File(p) if same_path(p, path)))
        else {
            return;
        };
        let was_active = self.active_panel_tab == Some(ix);
        self.close_panel_tab(ix, cx);
        self.file_cache.remove(path);
        if was_active && self.content_view == ContentView::File {
            // 激活位可能落到终端 tab 上——优先指去最近的文件 tab；
            // 一个文件 tab 都不剩则内容区回退（有终端回浏览区，否则回会话）
            let any_file = self
                .panel_tabs
                .iter()
                .position(|t| matches!(t, PanelTab::File(_)));
            match any_file {
                Some(fix)
                    if !matches!(
                        self.panel_tabs.get(self.active_panel_tab.unwrap_or(usize::MAX)),
                        Some(PanelTab::File(_))
                    ) =>
                {
                    self.activate_panel_tab(fix, cx);
                }
                None => {
                    let v = if self.panel_tabs.is_empty() {
                        ContentView::Chat
                    } else {
                        self.browse_last
                    };
                    self.set_content_view(v);
                }
                _ => {}
            }
        }
        cx.notify();
    }

    /// 保存文件 tab（Ctrl+S / 关闭确认「保存并关闭」共用）：编辑器在则取
    /// 编辑器值，写盘成功后刷新磁盘真值缓存与外部改动基准。
    pub(crate) fn save_file(&mut self, path: &Path, cx: &mut Context<Self>) {
        let err = self.write_file_back(path, cx);
        if let Some(e) = err {
            self.set_status(format!("{}: {e}", crate::i18n::tr("保存失败")), cx);
            return;
        }
        self.set_status(crate::i18n::tr("已保存").to_string(), cx);
        cx.notify();
    }

    /// 写盘 + 刷新磁盘真值/基准/脏位。Err = 写盘失败的错误串（调用方决定
    /// 提示方式：手动保存 toast，自动保存静默）。
    fn write_file_back(&mut self, path: &Path, cx: &mut Context<Self>) -> Option<String> {
        let text = match self.file_cache.get(path).and_then(|f| f.editor.as_ref()) {
            Some(ed) => ed.read(cx).value().to_string(),
            None => match self.file_cache.get(path) {
                Some(f) => f.content.clone(),
                None => return None,
            },
        };
        if let Err(e) = std::fs::write(path, &text) {
            return Some(e.to_string());
        }
        if let Some(ft) = self.file_cache.get_mut(path) {
            ft.content = text;
            ft.dirty = false;
            ft.conflict = None;
            ft.disk_sig = file_sig(path);
        }
        cx.notify();
        None
    }

    /// 023 自动保存（Zed after-timeout 同构）：编辑停顿约 1 秒写盘。每次
    /// 脏变起一个定时器；静默期内的后续编辑各自再起定时器，先到的发现
    /// 「最后编辑 < 1s」就让位退出，最晚的那个落盘。冲突挂着不自动写
    /// （等用户在横幅裁决），写盘静默（不抢状态栏）。
    pub(crate) fn autosave_later(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !crate::services::workspace::autosave() {
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(AUTOSAVE_DEBOUNCE_MS))
                    .await;
                let proceed = this.update(cx, |c, _| {
                    c.file_cache.get(&path).is_some_and(|f| {
                        f.dirty
                            && f.conflict.is_none()
                            && f.last_edit
                                .is_some_and(|t| t.elapsed().as_millis() as u64 >= AUTOSAVE_DEBOUNCE_MS)
                    })
                });
                if !proceed.unwrap_or(false) {
                    return; // 已保存/被丢弃/冲突/仍在输入（后续定时器接管）
                }
                let _ = this.update(cx, |c, cx| {
                    let _ = c.write_file_back(&path, cx);
                });
                return;
            }
        })
        .detach();
    }

    /// Ctrl+S（FileSave action，"Input" 上下文绑定）：保存当前文件 tab。
    pub(crate) fn save_active_file(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.active_file_path() {
            self.save_file(&path, cx);
        }
    }

    /// 冲突 banner「重新加载」：磁盘为准，丢弃本地未保存修改。
    pub(crate) fn file_reload_from_disk(&mut self, path: &Path, cx: &mut Context<Self>) {
        let bytes = match std::fs::read(path) {
            Ok(b) if !b.contains(&0) => b,
            _ => {
                self.set_status(crate::i18n::tr("文件已从磁盘消失").to_string(), cx);
                return;
            }
        };
        if let Some(ft) = self.file_cache.get_mut(path) {
            ft.content = String::from_utf8_lossy(&bytes).to_string();
            ft.dirty = false;
            ft.reload_pending = true;
            ft.conflict = None;
            ft.disk_sig = file_sig(path);
        }
        cx.notify();
    }

    /// 冲突 banner「忽略」：保留本地缓冲；以当前磁盘态为新基准，磁盘再变
    /// 才会再次提示。
    pub(crate) fn file_ignore_conflict(&mut self, path: &Path, cx: &mut Context<Self>) {
        if let Some(ft) = self.file_cache.get_mut(path) {
            ft.conflict = None;
            ft.disk_sig = file_sig(path);
        }
        cx.notify();
    }

    /// 023 外部改动检测（对齐 Zed）：fs 监听泵的每个合批信号跑一遍。
    /// 无未保存修改 → 自动重载（reload_pending 由渲染帧灌进编辑器）；
    /// 有修改 → 标冲突，等用户在 banner 上裁决。自己的保存已把基准刷到
    /// 写后元数据，本轮信号比较为空转。
    pub(crate) fn check_external_file_changes(&mut self, cx: &mut Context<Self>) {
        self.ext_probe.0 += 1;
        let mut changed = false;
        let paths: Vec<PathBuf> = self
            .panel_tabs
            .iter()
            .filter_map(|t| match t {
                PanelTab::File(p) => Some(p.clone()),
                _ => None,
            })
            .collect();
        for path in paths {
            let Some(ft) = self.file_cache.get_mut(&path) else {
                continue;
            };
            match std::fs::metadata(&path) {
                Err(_) => {
                    if ft.conflict.is_none() {
                        ft.conflict = Some(crate::FileConflict::Deleted);
                        changed = true;
                    }
                }
                Ok(md) => {
                    let sig = md.modified().ok().map(|m| (m, md.len()));
                    if sig.is_none() || sig == ft.disk_sig {
                        continue;
                    }
                    match std::fs::read(&path) {
                        Ok(b) if !b.contains(&0) => {
                            self.ext_probe.1 += 1;
                            if !ft.dirty {
                                ft.content = String::from_utf8_lossy(&b).to_string();
                                ft.reload_pending = true;
                                ft.conflict = None;
                            } else if ft.conflict.is_none() {
                                ft.conflict = Some(crate::FileConflict::Changed);
                            }
                            ft.disk_sig = sig;
                            changed = true;
                        }
                        _ => {
                            if ft.conflict.is_none() {
                                ft.conflict = Some(crate::FileConflict::Deleted);
                            }
                            changed = true;
                        }
                    }
                }
            }
        }
        if changed {
            cx.notify();
        }
    }

    /// 新建文件（标签栏 + 菜单）：项目根下按输入名创建（含中间目录），
    /// 已存在则直接打开。
    pub(crate) fn create_new_file(&mut self, name: &str, cx: &mut Context<Self>) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let target = self.cwd.join(name);
        if !target.starts_with(&self.cwd) {
            self.set_status(crate::i18n::tr("路径越出项目根").to_string(), cx);
            return;
        }
        if target.exists() {
            self.open_file_tab(target, cx);
            return;
        }
        if let Some(parent) = target.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                self.set_status(format!("{}: {e}", crate::i18n::tr("创建失败")), cx);
                return;
            }
        }
        if let Err(e) = std::fs::write(&target, "") {
            self.set_status(format!("{}: {e}", crate::i18n::tr("创建失败")), cx);
            return;
        }
        self.open_file_tab(target, cx);
    }

    /// 标签栏 + 菜单「打开文件…」：系统文件选择器（可多选）逐个开 tab。
    pub(crate) fn pick_open_files(&mut self, cx: &mut Context<Self>) {
        let opts = gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        };
        let rx = cx.prompt_for_paths(opts);
        cx.spawn(async move |this, cx| {
            let picked = rx.await.ok().and_then(|r| r.ok()).flatten();
            if let Some(paths) = picked {
                let _ = this.update(cx, |c, cx| {
                    for p in paths {
                        c.open_file_tab(p, cx);
                    }
                });
            }
        })
        .detach();
    }

    /// 标签栏 + 菜单「新建文件」：名字输入弹窗（Enter 提交 / Esc 取消）。
    /// 提交里同步置 dialog=None，与 apply_rename 同款（established 模式）。
    pub(crate) fn start_new_file(&mut self, cx: &mut Context<Self>) {
        let weak_ok = cx.entity().downgrade();
        let weak_esc = cx.entity().downgrade();
        let input = cx.new(|cx| {
            TextInput::new(cx)
                .select_all_on_focus()
                .placeholder(tr("文件名（可含子目录）"))
        });
        input.update(cx, |ti, _| {
            ti.set_on_submit(Box::new(move |v, cx| {
                let _ = weak_ok.update(cx, |c, cx| {
                    c.create_new_file(v, cx);
                    c.dialog = None;
                    cx.notify();
                });
            }));
            ti.set_on_escape(Box::new(move |cx| {
                let _ = weak_esc.update(cx, |c, cx| {
                    c.dialog = None;
                    cx.notify();
                });
            }));
        });
        self.dialog = Some(crate::Dialog::NewFile { input });
        cx.notify();
    }

    /// 当前激活的文件 tab 路径。
    pub(crate) fn active_file_path(&self) -> Option<PathBuf> {
        self.active_panel_tab
            .and_then(|ix| self.panel_tabs.get(ix))
            .and_then(|t| match t {
                PanelTab::File(p) => Some(p.clone()),
                _ => None,
            })
    }

    /// 激活内容区 tab 并挪到标签流首位（023 小功能：选中标签永远在最左，
    /// zed 没有）。Term/File 都是轻量句柄（终端实体在 terminals 按 id 索引），
    /// 移动无副作用。
    pub(crate) fn activate_panel_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.panel_tabs.len() {
            return;
        }
        if ix != 0 {
            let tab = self.panel_tabs.remove(ix);
            self.panel_tabs.insert(0, tab);
        }
        self.active_panel_tab = Some(0);
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


    pub(crate) fn attach_images(&mut self, cx: &mut Context<Self>) {
        // 压缩锁：压缩期间不弹文件选择器
        if self.rt().read(cx).compacting {
            return;
        }
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
                    let mime = mime_from_ext(&path);
                    chat.pending_images
                        .push(attached_image_from_bytes(mime, bytes));
                }
                if !chat.pending_images.is_empty() {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Ctrl+V 剪贴板图片 → 附件（pi-web handlePaste parity：剪贴板含图
    /// 即接管本次粘贴，文本不再落输入框）。返回 true 表示已消费（宿主
    /// 不再重派发组件 Paste）；false 交还原文本粘贴路径。
    /// 超限（张数/体积）与不支持的格式（svg 等）静默跳过——与 pi-web
    /// isBase64ImageWithinLimits 的过滤行为一致。
    pub(crate) fn attach_clipboard_image(&mut self, cx: &mut Context<Self>) -> bool {
        use gpui::ClipboardEntry;
        let Some(item) = cx.read_from_clipboard() else {
            return false;
        };
        let Some(image) = item.entries().iter().find_map(|e| match e {
            ClipboardEntry::Image(img) => Some(img.clone()),
            _ => None,
        }) else {
            return false;
        };
        let mime = match image.format {
            gpui::ImageFormat::Png => "image/png",
            gpui::ImageFormat::Jpeg => "image/jpeg",
            gpui::ImageFormat::Gif => "image/gif",
            gpui::ImageFormat::Webp => "image/webp",
            // 剪贴板可能给出 svg 文本（复制矢量图）——缩略图与 pi 均不收
            _ => return true,
        };
        let bytes = image.bytes;
        if bytes.len() > MAX_IMAGE_BYTES || self.pending_images.len() >= MAX_ATTACHED_IMAGES {
            return true;
        }
        self.pending_images
            .push(attached_image_from_bytes(mime.into(), bytes));
        cx.notify();
        true
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
                    r.disk_msg_count = pi_link::sessions::count_message_entries(&path) as usize;
                    r.messages = tail;
                    r.pager.reload(r.messages.len());
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
