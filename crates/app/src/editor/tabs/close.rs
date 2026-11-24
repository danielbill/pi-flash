//! tab 关闭族（自 tabs.rs 外移）：单关/批量关/位置关 + 关后回落路由 +
//! 无提示丢弃。

use super::*;

impl Chat {
    /// 脏判据（close_file_tab / close_tabs_batch 批量关 / 047-P2-2 soft cap
    /// 逐出三处同源）：挂冲突即脏，唯 `Deleted` 且内容干净不算——已从磁盘
    /// 消失且无未保存修改的缓冲直接关，不弹「未保存的修改」（zed parity）。
    pub(crate) fn file_tab_dirty(&self, path: &Path) -> bool {
        self.ws
            .file_cache
            .get(path)
            .map(|f| match f.conflict {
                Some(crate::FileConflict::Deleted) => f.dirty,
                _ => f.dirty || f.conflict.is_some(),
            })
            .unwrap_or(false)
    }

    /// 关文件 tab：有未保存修改（或挂着外部改动冲突）先弹确认，否则直接
    /// 关。zed parity：已从磁盘消失（Deleted）且无未保存修改的缓冲不算
    /// 脏——直接关，不弹「未保存的修改」；脏的照旧弹（保存并关闭=重建文件）。
    pub(crate) fn close_file_tab(&mut self, path: &Path, cx: &mut Context<Self>) {
        if self.file_tab_dirty(path) {
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
        // 046-T 任务页签：仅摘牌不杀进程（任务后台照跑）+ 前台回退主会话
        // 全在 close_task_tab——同上不走通用回退
        if let PanelTab::Task { key } = &tab {
            self.close_task_tab(key, cx);
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
                    | ContentView::PiNativeMarket
                    | ContentView::PfMarket
                    | ContentView::Tasks
                    | ContentView::Search
                    | ContentView::SessionSearch
                    | ContentView::Plugin
            ) {
                // 新激活位可能是任务页签/观察窗：池指针必须跟上，不能只换
                // 视图（见 route_content_to_active_tab 注）
                self.route_content_to_active_tab(cx);
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
                    if self.file_tab_dirty(&p) {
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
                // 任务页签批量摘除：仅摘牌（runtime 留池由退役泵按窗清账，
                // 046-T 关闭不杀进程；前台回退由批量收尾统一裁决）
                PanelTab::Task { key } => {
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
                //（回退路由统一走 route_content_to_active_tab——右键的若是
                // 任务页签/观察窗，池指针跟上，内容不滞留主会话）
                self.activate_panel_tab(f, cx);
                self.route_content_to_active_tab(cx);
            }
            None => self.fallback_after_closes(cx),
        }
        // 批量可能关掉前台池会话 tab：观察窗 runtime 随批量摘除（active_key
        // 悬空），任务页签仅摘牌（runtime 留池、tab 已不在）——两种都算
        // 前台失去 tab，落点按邻位补位结果统一裁决（reseat，与单关同源；
        // 旧版此处 switch 主会话还会把 focus 分支刚立起的激活位/前台视图
        // 冲掉，同为跳过左邻的不一致）
        let front_pool_tab_gone = {
            let has_tab = self.ws.panel_tabs.iter().any(|t| {
                matches!(t,
                crate::PanelTab::Task { key: k } | crate::PanelTab::Observer { key: k }
                if *k == self.pool.active_key)
            });
            let kind = self.pool.runtimes.get(&self.pool.active_key).map(|rt| {
                let r = rt.read(cx);
                (r.observer, r.task_owned)
            });
            !has_tab && matches!(kind, None | Some((true, _)) | Some((_, true)))
        };
        if front_pool_tab_gone {
            self.reseat_front_after_pool_tab_close(cx);
        }
        cx.notify();
    }

    /// 关闭回退共用尾：让内容区跟上 close_panel_tab 平移出的新激活位。
    /// 池会话 tab（Task/Observer）走激活核（activate_pool_session_tab，池
    /// 指针跟上）——Chat 视图渲染的是 active_key 那枚 runtime，只
    /// set_content_view(Chat) 会把主会话顶在前台、高亮却停在任务页签/
    /// 观察窗上（2026-11-25 用户报障：关最后一个文件 tab，激活位落到
    /// 任务页签、内容却还是主会话）；其余 tab = 各自视图；无自由 tab =
    /// 回主会话。
    pub(crate) fn route_content_to_active_tab(&mut self, cx: &mut Context<Self>) {
        match self.ws.active_panel_tab {
            Some(ix)
                if matches!(
                    self.ws.panel_tabs.get(ix),
                    Some(crate::PanelTab::Task { .. }) | Some(crate::PanelTab::Observer { .. })
                ) =>
            {
                self.activate_pool_session_tab(ix, cx)
            }
            Some(ix) => {
                let v = self
                    .ws
                    .panel_tabs
                    .get(ix)
                    .map(|t| t.view())
                    .unwrap_or(ContentView::Chat);
                self.set_content_view(v);
            }
            None => self.set_content_view(ContentView::Chat),
        }
    }

    /// 前台池会话（任务页签/观察窗）失去其 tab 后的落点（close_task_tab /
    /// close_observer_tab / close_tabs_batch 收尾三处同源）。左邻补位一致
    /// 性（2026-11-25 用户指出：关任务页签不该跳过左邻直落固定段会话，
    /// 文件 tab 关闭走的是 close_panel_tab 邻位补位）：新激活位也是池会话
    /// tab → 池指针直接切过去（route 的激活核）；是普通自由 tab（文件/
    /// 终端/任务管理页…）→ 池指针回主会话（Chat 轴不得悬在已摘牌的
    /// runtime 上），但激活位/前台视图仍跟邻位走——switch_to 的普通会话
    /// 臂会把激活位清回固定段，须复原补位结果；自由区不剩 → 回固定段
    /// 会话（primary 不在池 → 落新草稿，旧语义）。
    pub(crate) fn reseat_front_after_pool_tab_close(&mut self, cx: &mut Context<Self>) {
        match self.ws.active_panel_tab {
            Some(ix)
                if matches!(
                    self.ws.panel_tabs.get(ix),
                    Some(crate::PanelTab::Task { .. }) | Some(crate::PanelTab::Observer { .. })
                ) =>
            {
                self.route_content_to_active_tab(cx)
            }
            Some(ix) => {
                self.switch_front_to_primary(cx);
                // switch_to 的普通会话臂把激活位清回固定段：前台是邻位
                // 自由 tab，复原补位结果再路由内容区
                self.ws.active_panel_tab = Some(ix);
                self.route_content_to_active_tab(cx);
            }
            None => {
                self.psp.active_file = None;
                self.switch_front_to_primary(cx);
            }
        }
    }

    /// Chat 轴回主会话（primary 不在池 → 清键落新草稿）。只切池指针，
    /// 不裁激活位/前台视图——那是调用方按邻位补位结果裁决的事。
    fn switch_front_to_primary(&mut self, cx: &mut Context<Self>) {
        let pri = self
            .pool
            .runtimes
            .get(&self.pool.primary_key)
            .cloned()
            .filter(|rt| !rt.read(cx).observer && !rt.read(cx).task_owned);
        match pri {
            Some(rt) => self.switch_pool_rt(rt, cx),
            None => {
                self.pool.primary_key.clear();
                self.new_session(cx);
            }
        }
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
                    // 文档全关：内容区跟新激活位走（激活位可能平移到任务
                    // 页签/观察窗/终端——池指针/视图跟着换位，不再无条件回
                    // 主会话；只有自由 tab 一枚不剩才回会话。会话 tab 常驻；
                    // browse_last 可能记着 File，回 File 只会渲染「选择一个
                    // 文件」空提示页）
                    self.route_content_to_active_tab(cx);
                }
                _ => {}
            }
        }
        cx.notify();
    }
}
