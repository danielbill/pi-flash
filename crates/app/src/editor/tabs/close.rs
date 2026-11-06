//! tab 关闭族（自 tabs.rs 外移）：单关/批量关/位置关 + 关后回落路由 +
//! 无提示丢弃。

use super::*;

impl Chat {
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
        // 子代理观察窗：摘池（observer 无进程）+ tab 摘除 + 前台回退主会话
        // 全在 close_observer_tab——不走下面的通用回退（那套按 view 找剩余
        // tab，Chat 视图的回退语义归池指针管）
        if let PanelTab::Observer { key } = &tab {
            self.close_observer_tab(key, cx);
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
                    | ContentView::PfMarket
                    | ContentView::Search
                    | ContentView::SessionSearch
                    | ContentView::Plugin
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
                // 观察窗批量摘除：池 runtime + 快照就地清（不走
                // close_observer_tab——它的前台回退由批量收尾统一裁决）
                PanelTab::Observer { key } => {
                    self.pool.runtimes.remove(&key);
                    self.pool.tab_labels.remove(&key);
                    self.close_panel_tab(*ix, cx);
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
        // 批量可能关掉前台观察窗：active_key 悬空时回主会话（primary 在池
        // → 切回；否则落新草稿）
        if !self.pool.runtimes.contains_key(&self.pool.active_key) {
            self.psp.active_file = None;
            let pri = self
                .pool
                .runtimes
                .get(&self.pool.primary_key)
                .cloned()
                .filter(|rt| !rt.read(cx).observer);
            match pri {
                Some(rt) => self.switch_pool_rt(rt, cx),
                None => {
                    self.pool.primary_key.clear();
                    self.new_session(cx);
                }
            }
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
}
