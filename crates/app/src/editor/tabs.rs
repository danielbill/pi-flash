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
        // 文件/键盘切 tab 都经这里，语义 = 激活的文件标签就是树的当前项目；
        // 会话标签去留见 switch_project 的 2026-12-06 保留规则）
        if let Some(crate::PanelTab::File(p)) = self.ws.panel_tabs.get(ix).cloned() {
            self.sync_tree_to_file(&p, cx);
        }
        // 跨项目树同步会 switch_project → open_session/new_session →
        // switch_to「自由区全灭」清掉上面刚写的激活位（044-0 起普通会话
        // 切入的回 chat 姿态）。但本次激活才是最外层注意力落点——调用方
        // 随后 set_content_view(File)，激活位必须跟着文件走，否则 file_view
        // 渲染 active_file_path() = None 落空态「在左侧文件树中选择一个
        // 文件」（2026-11-07 用户报障：消息里点他项目路径，树定位了、
        // 编辑区空）。同项目路径不会触发 switch_to，重写同值无副作用。
        self.ws.active_panel_tab = Some(ix);
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
        // 子代理观察窗（044-0 验收②）：复用 Chat 视图——内容区渲染
        // active_key runtime，激活它 = 切池指针到该子会话。池命中缺失时
        // open_subagent_session_window 重建（key 即子会话文件路径；身份
        // 由 tab 本身携带，不走 open_session 防呆闸）。
        if let Some(PanelTab::Observer { key }) = self.ws.panel_tabs.get(ix).cloned() {
            self.activate_panel_tab(ix, cx);
            match self.pool.runtimes.get(&key).cloned() {
                Some(rt) => self.switch_pool_rt(rt, cx),
                None => self.open_subagent_session_window(PathBuf::from(&key), cx),
            }
            return;
        }
        let view = match self.ws.panel_tabs.get(ix) {
            Some(PanelTab::Term(_)) => ContentView::Term,
            // 观察窗已在上方提前 return（穷尽性兜底臂）
            Some(PanelTab::Observer { .. }) => ContentView::Chat,
            Some(PanelTab::Changelog) => ContentView::Changelog,
            Some(PanelTab::PiNativeMarket) => ContentView::PiNativeMarket,
            Some(PanelTab::PfMarket) => ContentView::PfMarket,
            Some(PanelTab::Search) => ContentView::Search,
            Some(PanelTab::SessionSearch) => ContentView::SessionSearch,
            Some(PanelTab::File(_)) => ContentView::File,
            // 插件预览页签（契约 v2.4）：内容渲染按活跃标签 pkg 现查（content.rs）
            Some(PanelTab::Plugin { .. }) => ContentView::Plugin,
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
        // 当前位置：会话 tab = None；其余 = active_panel_tab 下标。
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
        // 带序变了：落盘（内容相同自动跳过；启动恢复期被抑制）
        self.persist_tabs();
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
        // 带子变了：落盘（内容相同自动跳过写盘；启动恢复期被抑制）
        self.persist_tabs();
        cx.notify();
    }

    // -----------------------------------------------------------------------
    // tab 带持久化（pi-flash-weia）：启动恢复的第二条腿。快照/回放两段：
    // 变更点即快照落盘（**单槽全局**——panel_tabs 本就是全局态，按项目
    // 分槽会存出多份漂移拷贝，关一个槽另一槽复活，实测踩坑），启动按存
    // 档回放。文件/终端进存档；Changelog/市场/搜索页属瞬态页不持久化。
    // -----------------------------------------------------------------------

    /// 当前 tab 带的存档快照（含激活位映射：瞬态页不进存档，原下标在
    /// 存档里可能前移）。
    pub(crate) fn snapshot_tabs_state(&self) -> services::workspace::TabsState {
        tabs_state_from_parts(
            &self.ws.panel_tabs,
            |id| {
                self.ws
                    .terminals
                    .iter()
                    .find(|t| t.id == id)
                    .map(|t| t.cwd.clone())
            },
            self.ws.active_panel_tab,
            self.layout.content_view == ContentView::Chat,
        )
    }

    /// tab 带落盘（单槽全局）：变更点即写（内容相同自动跳过写盘）。启动
    /// 恢复期/回放中被 tab_restore_busy 抑制，回放完成后统一回写。
    pub(crate) fn persist_tabs(&mut self) {
        // 「启动恢复」关 = 不写存档（关着就什么都不恢复：存档清空、全程不写）。
        // 写侧唯一门口就在这里，别在调用点各加判断。
        if self.ws.tab_restore_busy || !services::workspace::startup_restore() {
            return;
        }
        let state = self.snapshot_tabs_state();
        services::workspace::save_tabs_state(&state);
    }

    /// 启动回放：按存档重建 tab 带。文件重读磁盘（消失的条目丢弃）；
    /// 终端按 cwd 重生（PTY 进程不保活）；最后按存档复位激活位与前台
    /// 视图。`window` = 主窗口（终端 spawn 要测字宽；automation 同款经
    /// 句柄进 window 上下文），None 只恢复文件 tab。调用方保证启动期
    /// 落盘抑制已开（否则回放中间态会写穿存档）。
    pub(crate) fn apply_saved_tabs(
        &mut self,
        saved: &services::workspace::TabsState,
        mut window: Option<&mut gpui::Window>,
        cx: &mut Context<Self>,
    ) {
        if saved.tabs.is_empty() {
            return;
        }
        self.ws.tab_restore_busy = true;
        for tab in &saved.tabs {
            match tab.kind.as_str() {
                "file" => {
                    let p = PathBuf::from(&tab.path);
                    // 已装订（重复）/磁盘已消失的条目跳过；装载核心不激活
                    // 不切视图，激活位最后统一复位
                    let dup = self
                        .ws
                        .panel_tabs
                        .iter()
                        .any(|t| matches!(t, PanelTab::File(q) if same_path(q, &p)));
                    if !dup && p.is_file() {
                        self.load_file_tab_core(p, cx);
                    }
                }
                "term" => {
                    let p = PathBuf::from(&tab.path);
                    if p.is_dir()
                        && let Some(w) = window.as_deref_mut()
                    {
                        self.spawn_terminal_tab(p, w, cx);
                    }
                }
                _ => {}
            }
        }
        // 激活位与前台视图按存档复位（回放装载路不激活，这里是唯一复位
        // 点）；复位后焦点尾巴只留给「存档激活位是文本文件」的情况
        let active = saved.active.filter(|a| *a < self.ws.panel_tabs.len());
        if let Some(ix) = active {
            self.ws.active_panel_tab = Some(ix);
            if let Some(tix) = self.shown_terminal_index() {
                self.ws.active_terminal = Some(tix);
            }
        }
        if saved.chat {
            self.set_content_view(ContentView::Chat);
        } else if let Some(view) = active
            .and_then(|ix| self.ws.panel_tabs.get(ix))
            .map(|t| t.view())
        {
            self.set_content_view(view);
        }
        self.ws.pending_focus_file = match self.active_file_path() {
            Some(p)
                if self
                    .ws
                    .file_cache
                    .get(&p)
                    .map(|f| !f.binary)
                    .unwrap_or(false) =>
            {
                Some(p)
            }
            _ => None,
        };
        self.ws.tab_restore_busy = false;
        // 存档对账回写（剪后的清单落盘，下次启动不再重剪；全剪空 = 移除
        // 槽位）。降级路（无 window，终端没法重生）**不回写**——免得把
        // 存档里的终端抹掉，下次正常启动还能回放
        if window.is_some() {
            self.persist_tabs();
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests;

mod close;
mod conflict;
mod open;
mod persist;
mod save;

use self::persist::tabs_state_from_parts;
