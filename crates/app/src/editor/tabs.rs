//! 文件 tab 生命周期：打开路由（二进制/超限拒绝、已开复用）、
//! 保存/自动保存（023，Zed after-timeout 同构）、外部改动检测与冲突 banner、
//! tab 激活（顺序稳定不重排，082 二期定夺）/关闭/无条件丢弃。
//! （P2 归并：actions_panels 的 tab 侧。）

use crate::*;

/// 自动保存静默期（023；Zed autosave after_timeout 同构）。
const AUTOSAVE_DEBOUNCE_MS: u64 = 1000;

/// tab 换位后激活下标的平移（纯函数，供单测）：remove(from) 后 >from 的
/// -1，insert(to) 后 >=to 的 +1；被拖 tab 本身（a == from）跟随到 to。
fn shifted_active(a: usize, from: usize, to: usize) -> usize {
    if a == from {
        to
    } else {
        let na = if a > from { a - 1 } else { a };
        if to <= na { na + 1 } else { na }
    }
}

impl Chat {
    /// v54.4：文件打开路由——统一打开为内容区 tab，主体渲染与编辑器配置
    /// 由 kind handler 决定（未注册类型走默认文本 kind）。html/htm 直接
    /// 开源码（2026-10-12 二改拍板，不再旁路系统浏览器；浏览器入口保留在
    /// 右键菜单首位文件动作）。
    ///
    /// 023 改版：编辑器实体懒创建（InputState::new 要 window，本调用链
    /// 没有——渲染帧在 content.rs 里补），二进制/超限文件提示不打开。
    pub(crate) fn open_file_tab(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        // 消息文本/微信 Bot 可能传来相对路径：统一按 cwd 绝对化（已开复用
        // 的 same_path 比较与磁盘读取都基于绝对形态）
        let path = if path.is_absolute() {
            path
        } else {
            // 相对路径按当前项目绝对化；无项目态无从解析，拒绝打开
            let Some(cwd) = self.ws.cwd.clone() else {
                return;
            };
            cwd.join(path)
        };
        const MAX: u64 = 10 * 1024 * 1024;
        if std::fs::metadata(&path)
            .map(|m| m.len() > MAX)
            .unwrap_or(false)
        {
            self.set_status(
                format!(
                    "{}：{}",
                    crate::i18n::tr("file_view.too_large"),
                    path.display()
                ),
                cx,
            );
            return;
        }
        // 已打开则直接切过去（Zed 行为）：不重读磁盘、不整文重灌——那会
        // 复位滚动/光标，并在大文件上冻结数百毫秒。外部改动由 fs watcher
        // 负责：cwd 内文件走 cwd 的递归 watch，cwd 外文件由
        // retain_file_watches 按所在目录单挂监听，两边信号都经
        // check_external_file_changes 按 disk_sig 复查后自动重载/标冲突。
        if let Some(ix) = self
            .ws
            .panel_tabs
            .iter()
            .position(|t| matches!(t, PanelTab::File(p) if same_path(p, &path)))
        {
            self.activate_panel_tab(ix, cx);
            self.set_content_view(ContentView::File);
            self.ws.pending_focus_file = Some(path.clone());
            cx.notify();
            return;
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                // 带上完整路径：消息里的路径点击失败（相对解析/拼写/盘符），
                // 没有路径无法定位是哪个文件没打开
                self.set_status(
                    format!(
                        "{} {}: {e}",
                        crate::i18n::tr("file_view.read_failed"),
                        path.display()
                    ),
                    cx,
                );
                return;
            }
        };
        // 打开路由对齐 zed ProjectItemRegistry 逆序尝试：图片类 kind 先认领
        // （ImageItem::try_open 先于 editor），认领失败/文本类 kind 落文本
        // 管线（decode_text），Binary → 无法打开灰屏（InvalidItemView 同构）
        // ——不再「含 NUL 一律灰屏」，png/bmp 等真图片由此可开。
        let kind = super::kind::kind_of(&path);
        let mut ft = FileTab::from_disk(String::new());
        if let Some(format) = kind.try_binary(&bytes) {
            // 图片查看 tab（zed ImageView）：无缓冲，保存/冲突检测按 binary 位跳过
            ft.binary = true;
            ft.image = Some(format);
        } else {
            // 文本管线：zed decode_text 同款——BOM/UTF-16/chardetng 嗅探；
            // 二进制判败 → 灰屏 tab 照开让用户看见是哪个文件，主体渲染
            // 「无法打开 {ext} 类型文件」（view.rs unsupported_body）
            match super::file_content::decode_text(bytes) {
                Ok(decoded) => {
                    ft = FileTab::from_disk(decoded.text);
                    ft.encoding = decoded.encoding;
                    ft.has_bom = decoded.has_bom;
                }
                Err(_) => {
                    ft.binary = true;
                }
            }
        }
        if ft.binary {
            // 二进制（图片/无法打开）没有行可跳：搜索结果里点到它时清掉
            // 待跳请求，免得滞留在同路径的下一次文本打开上
            self.ws.pending_jump = None;
            self.ws.file_cache.insert(path.clone(), ft);
        } else {
            ft.disk_sig = file_sig(&path);
            self.ws.file_cache.insert(path.clone(), ft);
            self.ws.pending_focus_file = Some(path.clone());
        }
        self.ws.panel_tabs.push(PanelTab::File(path));
        // cwd 外文件：按所在目录补一条单文件监听（否则磁盘改了不同步）
        self.retain_file_watches();
        let ix = self.ws.panel_tabs.len() - 1;
        self.activate_panel_tab(ix, cx);
        self.set_content_view(ContentView::File);
        cx.notify();
    }

    /// 关文件 tab：有未保存修改（或挂着外部改动冲突）先弹确认，否则直接
    /// 关。zed parity：已从磁盘消失（Deleted）且无未保存修改的缓冲不算
    /// 脏——直接关，不弹「未保存的修改」；脏的照旧弹（保存并关闭=重建文件）。
    pub(crate) fn close_file_tab(&mut self, path: &Path, cx: &mut Context<Self>) {
        let dirty = self
            .ws
            .file_cache
            .get(path)
            .map(|f| match f.conflict {
                Some(crate::FileConflict::Deleted) => f.dirty,
                _ => f.dirty || f.conflict.is_some(),
            })
            .unwrap_or(false);
        if dirty {
            self.dlg.dialog = Some(crate::Dialog::FileDirty {
                paths: vec![path.to_path_buf()],
            });
            cx.notify();
            return;
        }
        self.discard_file_tab(path, cx);
    }

    /// 关一个自由 tab（tab 行尾 × 与标签右键菜单 Close 共用）：文件走脏
    /// 确认；终端/更新日志直接关 + 内容区回退（原 × 闭包逻辑收拢成方法，
    /// 两条入口同源）。
    pub(crate) fn close_tab_at(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.ws.panel_tabs.get(ix).cloned() else {
            return;
        };
        if let PanelTab::File(p) = &tab {
            self.close_file_tab(p, cx);
            cx.notify();
            return;
        }
        let view = tab.view();
        let was_active = self.ws.active_panel_tab == Some(ix) && self.layout.content_view == view;
        self.close_panel_tab(ix, cx);
        // 关掉当前 tab 后内容区回退：终端→浏览区遗留→chat；
        // 更新日志（081）→ 剩余 tab 激活最近的，没有则回会话
        if was_active && self.layout.content_view == view {
            if matches!(
                view,
                ContentView::Changelog
                    | ContentView::PiExtMarket
                    | ContentView::PiNativeMarket
                    | ContentView::Search
                    | ContentView::SessionSearch
                    | ContentView::Wx
            ) {
                match self.ws.active_panel_tab {
                    Some(nix) => {
                        let v = self
                            .ws
                            .panel_tabs
                            .get(nix)
                            .map(|t| t.view())
                            .unwrap_or(ContentView::Chat);
                        self.set_content_view(v);
                    }
                    None => self.set_content_view(ContentView::Chat),
                }
            } else if self.ws.active_panel_tab.is_none() {
                self.fallback_after_closes(cx);
            }
        }
        cx.notify();
    }

    /// 批量关自由 tab（topbar 标签右键菜单移植 zed：Close Others/Left/
    /// Right/All 共用核心）。`targets` = 待关下标集合；`focus` = 关完要
    /// 激活的 tab（右键那个，Close All 传 None）。干净 tab 直接关；脏
    /// 文件不逐个弹——收拢进一个 FileDirty 确认（zed「N unsaved」三键
    /// 同构：保存并关闭/不保存关闭对整批生效，取消则脏 tab 留着）。会话
    /// tab 不在 panel_tabs 里，天然不被波及（首标签恒不可关）。
    pub(crate) fn close_tabs_batch(
        &mut self,
        mut targets: Vec<usize>,
        focus: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        if targets.is_empty() {
            return;
        }
        targets.sort_unstable();
        targets.dedup();
        // focus 按身份追踪：批量移除会让下标平移，事后按下标找必指错
        let focus_tab = focus.and_then(|f| self.ws.panel_tabs.get(f).cloned());
        let mut dirty: Vec<PathBuf> = Vec::new();
        // 倒序移除：前面的下标不因删除漂移
        for ix in targets.iter().rev() {
            let Some(tab) = self.ws.panel_tabs.get(*ix).cloned() else {
                continue;
            };
            match tab {
                PanelTab::File(p) => {
                    // 脏判据与 close_file_tab 同源（Deleted 且干净不算脏）
                    let is_dirty = self
                        .ws
                        .file_cache
                        .get(&p)
                        .map(|f| match f.conflict {
                            Some(crate::FileConflict::Deleted) => f.dirty,
                            _ => f.dirty || f.conflict.is_some(),
                        })
                        .unwrap_or(false);
                    if is_dirty {
                        dirty.push(p);
                    } else {
                        self.discard_file_tab(&p, cx);
                    }
                }
                _ => self.close_panel_tab(*ix, cx),
            }
        }
        if !dirty.is_empty() {
            dirty.reverse(); // 回正序：与标签视觉一致（左→右）
            self.dlg.dialog = Some(crate::Dialog::FileDirty { paths: dirty });
        }
        match focus_tab.and_then(|t| self.ws.panel_tabs.iter().position(|p| p == &t)) {
            Some(f) => {
                // 右键 tab 存活（Others/Left/Right 必然）：激活它，视野跟上
                let v = self.ws.panel_tabs[f].view();
                self.activate_panel_tab(f, cx);
                self.set_content_view(v);
            }
            None => self.fallback_after_closes(cx),
        }
        cx.notify();
    }

    /// 关了一批 tab 后没人可激活（Close All / focus 也被关）时的内容区
    /// 回退，与单关回退同源：优先最近的文件 tab；文档全关回会话——会话
    /// tab 常驻，直接跳回（2026-11-24 用户定夺，不再回 browse_last：
    /// 文件 tab 已不剩时 browse_last 可能记着 File，落空渲染
    /// 「在左侧文件树中选择一个文件」空提示页）；终端/搜索等 tab 仍在
    /// 标签条上，点标签即回。
    pub(crate) fn fallback_after_closes(&mut self, cx: &mut Context<Self>) {
        if let Some(fix) = self
            .ws
            .panel_tabs
            .iter()
            .position(|t| matches!(t, PanelTab::File(_)))
        {
            self.activate_panel_tab(fix, cx);
            self.set_content_view(ContentView::File);
        } else {
            self.set_content_view(ContentView::Chat);
        }
    }

    /// 无条件丢弃文件 tab（确认弹窗三个按钮的公共尾）。
    pub(crate) fn discard_file_tab(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(ix) = self
            .ws
            .panel_tabs
            .iter()
            .position(|t| matches!(t, PanelTab::File(p) if same_path(p, path)))
        else {
            return;
        };
        let was_active = self.ws.active_panel_tab == Some(ix);
        self.close_panel_tab(ix, cx);
        self.ws.file_cache.remove(path);
        // 关掉后可能没人再需要这个目录的监听：卸掉
        self.retain_file_watches();
        if was_active && self.layout.content_view == ContentView::File {
            // 激活位可能落到终端 tab 上——优先指去最近的文件 tab；
            // 一个文件 tab 都不剩则内容区回退（有终端回浏览区，否则回会话）
            let any_file = self
                .ws
                .panel_tabs
                .iter()
                .position(|t| matches!(t, PanelTab::File(_)));
            match any_file {
                Some(fix)
                    if !matches!(
                        self.ws
                            .panel_tabs
                            .get(self.ws.active_panel_tab.unwrap_or(usize::MAX)),
                        Some(PanelTab::File(_))
                    ) =>
                {
                    self.activate_panel_tab(fix, cx);
                }
                None => {
                    // 文档全关：直接回会话（会话 tab 常驻；browse_last 可能
                    // 记着 File，回 File 只会渲染「选择一个文件」空提示页）
                    self.set_content_view(ContentView::Chat);
                }
                _ => {}
            }
        }
        cx.notify();
    }

    /// 保存文件 tab（Ctrl+S / 关闭确认「保存并关闭」共用）：编辑器在则取
    /// 编辑器值，写盘成功后刷新磁盘真值缓存与外部改动基准。
    pub(crate) fn save_file(&mut self, path: &Path, cx: &mut Context<Self>) {
        // 二进制占位 tab 无缓冲可存，直接短路（不弹「已保存」假提示）
        if self.ws.file_cache.get(path).is_some_and(|f| f.binary) {
            return;
        }
        let err = self.write_file_back(path, cx);
        if let Some(e) = err {
            self.set_status(
                format!("{}: {e}", crate::i18n::tr("file_view.save_failed")),
                cx,
            );
            return;
        }
        self.set_status(crate::i18n::tr("status.saved").to_string(), cx);
        cx.notify();
    }

    /// 写盘 + 刷新磁盘真值/基准/脏位。Err = 写盘失败的错误串（调用方决定
    /// 提示方式：手动保存 toast，自动保存静默）。
    fn write_file_back(&mut self, path: &Path, cx: &mut Context<Self>) -> Option<String> {
        let Some(ft) = self.ws.file_cache.get(path) else {
            return None;
        };
        let text = match ft.editor.as_ref() {
            Some(ed) => ed.read(cx).value().to_string(),
            None => ft.content.clone(),
        };
        // 按 tab 记录的磁盘编码编码回盘（zed encode_text 同款：UTF-8 无 BOM
        // 原样直写；UTF-16/遗留编码/BOM 文件往返保持，不静默转 UTF-8）
        let bytes = super::file_content::encode_text(text.clone(), ft.encoding, ft.has_bom);
        // zed fs.save 同款：先建父目录再写——已删文件保存即重建（目录被
        // 一并删除时也能恢复）；目录健在时 create_dir_all 是空操作
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return Some(e.to_string());
            }
        }
        if let Err(e) = std::fs::write(path, &bytes) {
            return Some(e.to_string());
        }
        if let Some(ft) = self.ws.file_cache.get_mut(path) {
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
    /// （2026-10-10 用户定夺：恒开，无开关——默认行为即唯一行为）
    pub(crate) fn autosave_later(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(AUTOSAVE_DEBOUNCE_MS))
                    .await;
                let proceed = this.update(cx, |c, _| {
                    c.ws.file_cache.get(&path).is_some_and(|f| {
                        f.dirty
                            && f.conflict.is_none()
                            && f.last_edit.is_some_and(|t| {
                                t.elapsed().as_millis() as u64 >= AUTOSAVE_DEBOUNCE_MS
                            })
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
        let decoded = match std::fs::read(path).map(super::file_content::decode_text) {
            Ok(Ok(d)) => d,
            _ => {
                // 读不出来/二进制化：视同消失（沿用既有口径）
                self.set_status(crate::i18n::tr("conflict.deleted_on_disk").to_string(), cx);
                return;
            }
        };
        if let Some(ft) = self.ws.file_cache.get_mut(path) {
            ft.content = decoded.text;
            ft.encoding = decoded.encoding;
            ft.has_bom = decoded.has_bom;
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
        if let Some(ft) = self.ws.file_cache.get_mut(path) {
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
        self.ws.ext_probe.0 += 1;
        let mut changed = false;
        let paths: Vec<PathBuf> = self
            .ws
            .panel_tabs
            .iter()
            .filter_map(|t| match t {
                PanelTab::File(p) => Some(p.clone()),
                _ => None,
            })
            .collect();
        for path in paths {
            let Some(ft) = self.ws.file_cache.get_mut(&path) else {
                continue;
            };
            if ft.binary {
                continue; // 二进制占位 tab 无缓冲，磁盘变化不参与冲突/重载裁决
            }
            match std::fs::metadata(&path) {
                Err(_) => {
                    // 文件已从磁盘消失：标 Deleted（zed DiskState::Deleted 同构
                    // ——缓冲保留可继续编辑、tab 删除线、保存即重建；不弹横幅）。
                    // 已在 Deleted 态不重复置位（watcher 泵 150ms 一轮，避免
                    // 每轮空 notify）
                    if ft.conflict != Some(crate::FileConflict::Deleted) {
                        ft.conflict = Some(crate::FileConflict::Deleted);
                        changed = true;
                    }
                }
                Ok(md) => {
                    let sig = md.modified().ok().map(|m| (m, md.len()));
                    if sig.is_none() || sig == ft.disk_sig {
                        continue;
                    }
                    match std::fs::read(&path).map(super::file_content::decode_text) {
                        Ok(Ok(decoded)) => {
                            self.ws.ext_probe.1 += 1;
                            if !ft.dirty {
                                ft.content = decoded.text;
                                ft.encoding = decoded.encoding;
                                ft.has_bom = decoded.has_bom;
                                ft.reload_pending = true;
                                ft.conflict = None;
                            } else if ft.conflict != Some(crate::FileConflict::Changed) {
                                // Deleted→Changed 覆盖：文件被外部重建而本地
                                // 仍有未保存修改（zed：has_conflict 弹裁决）
                                ft.conflict = Some(crate::FileConflict::Changed);
                            }
                            ft.disk_sig = sig;
                            changed = true;
                        }
                        _ => {
                            // 二进制化/读不出来：视同消失（同样只置一次）
                            if ft.conflict != Some(crate::FileConflict::Deleted) {
                                ft.conflict = Some(crate::FileConflict::Deleted);
                                changed = true;
                            }
                        }
                    }
                }
            }
        }
        if changed {
            cx.notify();
        }
    }

    /// 当前激活的文件 tab 路径。
    pub(crate) fn active_file_path(&self) -> Option<PathBuf> {
        self.ws
            .active_panel_tab
            .and_then(|ix| self.ws.panel_tabs.get(ix))
            .and_then(|t| match t {
                PanelTab::File(p) => Some(p.clone()),
                _ => None,
            })
    }

    /// 激活内容区 tab。023 的「选中标签挪到标签流首位」MRU 行为已按 082
    /// 二期用户定夺移除：tab 条 = 稳定打开顺序（浏览器/zed 同款），新开
    /// tab 追加到最右、激活只换高亮位——Ctrl+←/→ 逐格切换依赖下标稳定，
    /// 激活即重排会让邻位关系每次漂移。Term/File 都是轻量句柄（终端实体
    /// 在 terminals 按 id 索引）。
    pub(crate) fn activate_panel_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.ws.panel_tabs.len() {
            return;
        }
        self.ws.active_panel_tab = Some(ix);
        // 切到终端 tab：同步 active_terminal（按键/自动化都读它；否则双终端
        // 切 tab 后输入会送进后台那个终端）
        if let Some(tix) = self.shown_terminal_index() {
            self.ws.active_terminal = Some(tix);
        }
        // 文件标签驱动树同步：同项目定位选中，跨项目切工作区（点标签/开
        // 文件/键盘切 tab/关标签回退都经这里，语义 = 激活的文件标签就是
        // 树的当前项目）
        if let Some(crate::PanelTab::File(p)) = self.ws.panel_tabs.get(ix).cloned() {
            self.sync_tree_to_file(&p, cx);
        }
        // zed `update_active_tab` 同款：让标签条把激活 tab 滚进视口（最小
        // 滚动，已可见即不动）。唯一写点，点标签/Ctrl+←/→/关标签补位全跟
        // 随；不挂渲染路径，否则用户手滚走一帧就被拽回。
        self.layout.tabs_scroll.scroll_to_item(ix);
        cx.notify();
    }

    /// 激活内容区 tab（鼠标点标签与 Ctrl+←/→ 共用的唯一入口）。082 四期
    /// 用户拍板：**切 tab 不再自动聚焦内容区**（终端不抢键盘）——只改状
    /// 态，并在视图真变化时把焦点收回 content-main 锚点（`layout.focus`，
    /// track_focus 永在树上，宿主在 content.rs）：旧视图元素卸载后句柄
    /// 悬空，必须收回一个在树上的所有者，否则派发路径为空、Ctrl+←/→
    /// 无法连续切（blur() 方案验收已证伪）。锚点不能挂 content-col：
    /// topbar_r（窗口控制钮/拖拽区）在其子树，track_focus 监听会吞 NC
    /// 按下致窗口钮全哑。键入目标由内容区点击决定；Term/File 源码态
    /// 焦点逐帧仲裁不回收。
    pub(crate) fn activate_content_tab(
        &mut self,
        ix: usize,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let view = match self.ws.panel_tabs.get(ix) {
            Some(PanelTab::Term(_)) => ContentView::Term,
            Some(PanelTab::Changelog) => ContentView::Changelog,
            Some(PanelTab::PiExtMarket) => ContentView::PiExtMarket,
            Some(PanelTab::PiNativeMarket) => ContentView::PiNativeMarket,
            Some(PanelTab::PfMarket) => ContentView::PfMarket,
            Some(PanelTab::Search) => ContentView::Search,
            Some(PanelTab::SessionSearch) => ContentView::SessionSearch,
            Some(PanelTab::Wx) => ContentView::Wx,
            Some(PanelTab::File(_)) => ContentView::File,
            None => return,
        };
        let changed = self.layout.content_view != view || self.ws.active_panel_tab != Some(ix);
        self.activate_panel_tab(ix, cx);
        self.set_content_view(view);
        if changed {
            // 收回中性锚点（路径含 "ContentTabs"，下一键照常命中）；
            // focus() 同值早退，重复调用无副作用
            window.focus(&self.layout.focus);
            cx.notify();
        }
    }

    /// 082 视图切换：Ctrl+←/→ 切到当前标签左/右邻视图。tab 条 = 置顶会话
    /// tab + 自由标签区，顺序稳定（激活不重排，见 activate_panel_tab），
    /// 序列 [会话, p0..pn] 上的「左/右邻」= 下标 -1/+1。最左再往左、最右
    /// 再往右 = no-op（不环绕，浏览器同语义）。焦点策略见
    /// activate_content_tab（不自动聚焦，视图变化即截断旧焦点）。
    /// 返回是否发生切换。
    pub(crate) fn cycle_content_tab(
        &mut self,
        offset: isize,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // 当前位置：会话 tab = None；自由区 = active_panel_tab 下标。
        // 非会话视图但下标悬空（理论不可达）按自由区 0 处理
        let cur = if self.layout.content_view == ContentView::Chat {
            None
        } else {
            Some(self.ws.active_panel_tab.unwrap_or(0))
        };
        let target = match cur {
            // 会话 tab：右 = 自由区第一颗；左 = 空
            None => (offset > 0).then_some(Some(0)),
            Some(cur_ix) => {
                let next = cur_ix as isize + offset;
                // 自由区最左 → 会话 tab；最右 → 空
                if next < 0 {
                    Some(None)
                } else if (next as usize) < self.ws.panel_tabs.len() {
                    Some(Some(next as usize))
                } else {
                    None
                }
            }
        };
        let Some(target) = target else { return false };
        match target {
            Some(ix) => self.activate_content_tab(ix, window, cx),
            None => {
                let changed = self.layout.content_view != ContentView::Chat;
                self.set_content_view(ContentView::Chat);
                if changed {
                    // 焦点收回锚点；仲裁 Chat 分支随后回 composer
                    window.focus(&self.layout.focus);
                    cx.notify();
                }
            }
        }
        true
    }

    /// tab 拖拽换位落位（zed handle_tab_drop 单 pane 版，2026-10-14）：把
    /// `key` 对应的 tab 移到 to 位置。from 由 key 现查（不信任发起时的
    /// 下标——拖拽中途列表变了不会错位）；找不到（已关）= no-op；`to` 越界
    /// 按 zed `insertion_index.min(items.len())` 夹到末尾（标签区空白 drop
    /// 传的就是 items.len()）。active_panel_tab 随 remove/insert 平移（被拖
    /// tab 若是激活 tab 跟随到新位）；树同步不需要——同一文件换位，选中不变。
    pub(crate) fn move_panel_tab(&mut self, key: &TabKey, to: usize, cx: &mut Context<Self>) {
        let Some(from) = self.ws.panel_tabs.iter().position(|t| &t.key() == key) else {
            return;
        };
        let to = to.min(self.ws.panel_tabs.len().saturating_sub(1));
        if from == to {
            return;
        }
        let tab = self.ws.panel_tabs.remove(from);
        self.ws.panel_tabs.insert(to, tab);
        self.ws.active_panel_tab = self
            .ws
            .active_panel_tab
            .map(|a| shifted_active(a, from, to));
        // 换位同 zed：drop 落位后把被拖 tab 滚进视口（拖到条尾/条头时它
        // 可能本身就在屏外）
        self.layout.tabs_scroll.scroll_to_item(to);
        cx.notify();
    }

    pub(crate) fn close_panel_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.ws.panel_tabs.len() {
            return;
        }
        let removed = self.ws.panel_tabs.remove(ix);
        if let PanelTab::Term(id) = &removed {
            if let Some(tix) = self.ws.terminals.iter().position(|t| t.id == *id) {
                let _ = self.ws.terminals[tix]
                    .pty
                    .send(alacritty_terminal::event_loop::Msg::Shutdown);
                self.ws.terminals.remove(tix);
                // 该终端的右键菜单若开着，随 tab 一起收（挂载处也校验，双保险）
                if self.dlg.term_menu.is_some_and(|(tid, _, _)| tid == *id) {
                    self.dlg.term_menu = None;
                }
                // 终端可开多个（同项目不限量）：下标随移除平移/夹紧，
                // 否则 active_terminal 会指错终端或越界
                self.ws.active_terminal = match self.ws.active_terminal {
                    Some(a) if a == tix => {
                        if self.ws.terminals.is_empty() {
                            None
                        } else {
                            Some(tix.min(self.ws.terminals.len() - 1))
                        }
                    }
                    Some(a) if a > tix => Some(a - 1),
                    other => other,
                };
            }
        }
        self.ws.active_panel_tab = match self.ws.active_panel_tab {
            // 关的是激活 tab：右邻补位（空表置 None）；关的在激活位左侧：
            // 激活下标随删左移一格（tab 条顺序稳定后激活位可以是任意下标）
            Some(a) if a >= self.ws.panel_tabs.len() => {
                if self.ws.panel_tabs.is_empty() {
                    None
                } else {
                    Some(a.saturating_sub(1))
                }
            }
            Some(a) if a > ix => Some(a - 1),
            other => other,
        };
        // 关 tab 后激活的可能就是另一个终端 tab：对齐一次（与激活路径同源）
        if let Some(tix) = self.shown_terminal_index() {
            self.ws.active_terminal = Some(tix);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::shifted_active;

    /// 换位平移穷举：[t0..t4] 里枚举全部 (from, to, a)，用「模拟序列
    /// remove/insert 后查同元素下标」做 oracle 对照。
    #[test]
    fn active_index_follows_tab_move() {
        for n in 1..=5usize {
            let tabs: Vec<usize> = (0..n).collect();
            for from in 0..n {
                for to in 0..n {
                    let mut moved = tabs.clone();
                    let t = moved.remove(from);
                    moved.insert(to, t);
                    for a in 0..n {
                        let elem = tabs[a];
                        let want = moved.iter().position(|&e| e == elem).unwrap();
                        assert_eq!(
                            shifted_active(a, from, to),
                            want,
                            "n={n} from={from} to={to} a={a}"
                        );
                    }
                }
            }
        }
    }
}
