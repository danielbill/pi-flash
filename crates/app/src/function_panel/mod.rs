//! psp / dock (v54): 项目+会话一体列表（ChatGPT 式，设计核心=隐藏复杂度）。
//! dock = statusbar 三面板容器（psp / 文件树 / git），nav 底色，右缘 6px
//! 拖拽调宽（250–500，双击复位 282）。psp 的悬浮层（路径 tooltip / 会话
//! 详情卡 / 两级菜单 / 删除确认）由 `psp_overlays` 在根渲染挂载。

pub(crate) mod file_actions;
pub(crate) mod file_tree;
pub(crate) mod file_tree_drag;
pub(crate) mod file_tree_edit;
pub(crate) mod file_tree_pins;
pub(crate) mod file_tree_sticky;
pub(crate) mod git_panel;
pub(crate) mod psp_actions;
pub(crate) mod psp_drag;
pub(crate) mod psp_menus;
pub(crate) mod psp_overlays;
pub(crate) mod psp_row_views;
pub(crate) mod tab_menu;
pub(crate) mod tree_menu;

use gpui::{Context, Entity, MouseButton, SharedString, div, prelude::*, px, rgb};

use std::path::PathBuf;

use crate::Chat;
use crate::DockPanel;
use crate::ListMode;
use crate::SortMode;
use crate::appearance::theme::{Theme, theme as T};
use crate::i18n::tr;
use crate::services::workspace::{same_path, same_ws, same_ws_key};
use crate::ui::TextInput;
use crate::ui::tokens::align;
use crate::ui::{WithTip, icon};
use psp_row_views::{pending_row_view, pinned_header_row, session_row_view};

/// Flattened psp row model (one virtual list over all rows).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PspRow {
    Project(usize),
    Session {
        p: usize,
        s: usize,
    },
    /// 置顶组头行（伪项目行）：pin icon + 「置顶」；不收折/不拖拽/无卡，
    /// 仅作分组视觉。组内会话行用 [`PspRow::PinnedSession`]（元素 id 与
    /// 原组行区分，防 gpui id 冲突）。
    PinnedHeader,
    /// 置顶组内的会话行（p/s = 原项目组下标；**移入语义**——置顶会话只在
    /// 置顶组显示，原项目组已按 pin 清单过滤，不重复出现）：
    /// 不参与项目块避让平移（前置头行之前无 Project 行，shift 恒 0）。
    PinnedSession {
        p: usize,
        s: usize,
    },
    /// 伪标签（draft 已发送、pi 未落盘的占位行；key = 池 key draft-N）
    Pending {
        key: String,
    },
    /// 空项目展开时的「无会话」占位
    Empty,
    /// 「显示更多」行（key = 组 ws_key；Flat 模式用全局键）
    More {
        key: String,
    },
}

/// 每页显示的会话标签数（学 zcode：「显示更多」每次续一页）——默认值来自
/// 设置-其他「会话显示数」（3-10）。
fn psp_page() -> usize {
    crate::services::workspace::session_display_count()
}

pub(crate) fn psp_rows(chat: &Chat) -> Vec<PspRow> {
    let mut rows: Vec<PspRow> = Vec::new();
    // 伪标签归属：cwd 匹配项目组的插在组内会话行之前（它就是该项目最新的
    // 活动）；没有任何组可归（该项目还没有已落盘会话）的插在 Title 后。
    // 折叠的组里伪标签照常显示——它正是用户刚发出的那条消息的标签。
    let orphan_pending: Vec<String> = chat
        .psp
        .pending_tabs
        .iter()
        .filter(|e| {
            !chat
                .psp
                .projects
                .iter()
                .any(|g| same_ws(&g.path.to_string_lossy(), &e.cwd))
        })
        .map(|e| e.key.clone())
        .collect();
    match chat.psp.list_mode {
        ListMode::Grouped => {
            // 置顶组（会话 pin）：列表最顶；内容为空不建组（空置顶组永不
            // 显示）。**zcode 同款「移入」语义**：置顶会话只在置顶组出现，
            // 原项目组按 pin 清单过滤（见下方 visible），不重复显示；
            // 组内按 pin 序（后置顶的总在最上面）。
            let pinned = pinned_rows(chat);
            if !pinned.is_empty() {
                rows.push(PspRow::PinnedHeader);
                rows.extend(pinned);
            }
            for (pi, g) in chat.psp.projects.iter().enumerate() {
                rows.push(PspRow::Project(pi));
                let key = same_ws_key(&g.path.to_string_lossy());
                for e in chat
                    .psp
                    .pending_tabs
                    .iter()
                    .filter(|e| same_ws(&g.path.to_string_lossy(), &e.cwd))
                {
                    rows.push(PspRow::Pending { key: e.key.clone() });
                }
                // 拖拽中的项目收起为标签（020）：渲染期折叠——只显示组标题
                // 行，落位重排后状态清零自动还原，不碰 collapsed_keys
                let drag_collapsed = chat.psp.proj_drag.as_ref().is_some_and(|d| d.key == key);
                if drag_collapsed || chat.psp.collapsed_keys.contains(&key) {
                    continue;
                }
                // 置顶会话已被「移入」置顶组，原项目组不再重复显示（zcode
                // parity）：按 pin 清单过滤；分页计数用过滤后的可见条数，
                // 否则会留下空组还挂「显示更多」的死行
                let visible: Vec<usize> = (0..g.sessions.len())
                    .filter(|si| !chat.session_pinned(&g.sessions[*si].path))
                    .collect();
                if visible.is_empty() {
                    rows.push(PspRow::Empty);
                } else {
                    let shown = chat
                        .psp
                        .psp_shown
                        .get(&key)
                        .copied()
                        .unwrap_or_else(psp_page)
                        .min(visible.len());
                    for si in visible.iter().take(shown) {
                        rows.push(PspRow::Session { p: pi, s: *si });
                    }
                    if visible.len() > shown {
                        rows.push(PspRow::More { key });
                    }
                }
            }
        }
        ListMode::Flat => {
            // Flat：伪标签按插入序排列表最前（最新活动优先），其后置顶会话
            // 排到会话区第一位（pin 序），其余维持现排序
            for e in &chat.psp.pending_tabs {
                rows.push(PspRow::Pending { key: e.key.clone() });
            }
            let mut all: Vec<(usize, usize)> = Vec::new();
            for (pi, g) in chat.psp.projects.iter().enumerate() {
                for si in 0..g.sessions.len() {
                    all.push((pi, si));
                }
            }
            if chat.psp.sort_mode == SortMode::Time {
                all.sort_by(|a, b| {
                    let ma = chat.psp.projects[a.0].sessions[a.1].modified;
                    let mb = chat.psp.projects[b.0].sessions[b.1].modified;
                    mb.cmp(&ma)
                });
            }
            // 置顶分区：pin 序（session_pins 位置升序）排到最前
            let pin_rank = |p: usize, s: usize| -> usize {
                let path = &chat.psp.projects[p].sessions[s].path;
                chat.psp
                    .session_pins
                    .iter()
                    .position(|q| same_path(q, path))
                    .unwrap_or(usize::MAX)
            };
            all.sort_by_key(|&(p, s)| pin_rank(p, s));
            let key = "__flat__".to_string();
            let shown = chat
                .psp
                .psp_shown
                .get(&key)
                .copied()
                .unwrap_or_else(psp_page);
            let total = all.len();
            all.truncate(shown);
            all.into_iter()
                .for_each(|(p, s)| rows.push(PspRow::Session { p, s }));
            if total > shown {
                rows.push(PspRow::More { key });
            }
        }
    }
    // 无组可归的伪标签（该项目组还没被 rebuild_projects 建出来）：插在置顶
    // 组（若有）之后、项目区之前——用户刚发的消息必须是第一眼能看到的东西，
    // 但不能盖住置顶组的头行
    let orphan_at = rows
        .iter()
        .take_while(|r| matches!(r, PspRow::PinnedHeader | PspRow::PinnedSession { .. }))
        .count();
    for (i, key) in orphan_pending.into_iter().enumerate() {
        rows.insert(orphan_at + i, PspRow::Pending { key });
    }
    rows
}

/// 置顶组的会话行（Grouped 视图）：按 session_pins 序（后置顶的总在最上面）
/// 在 projects 里找到每个置顶会话的当前下标，产出副本行引用。清单外
/// （已删 / 未装载）的置顶项静默跳过；全部跳过则空向量 —— 上层不建组。
fn pinned_rows(chat: &Chat) -> Vec<PspRow> {
    let mut rows = Vec::new();
    for pin in &chat.psp.session_pins {
        for (pi, g) in chat.psp.projects.iter().enumerate() {
            if let Some(si) = g.sessions.iter().position(|s| same_path(&s.path, pin)) {
                rows.push(PspRow::PinnedSession { p: pi, s: si });
                break;
            }
        }
    }
    rows
}

impl Chat {
    /// Group the cross-project session scan into project groups (020 拖拽
    /// 版排序：用户排序 > 首建默认序（当前项目钉顶 + 活动倒序，仅一次）>
    /// 运行期保序；组内会话恒按活动倒序——详见下方实现注释）。
    pub(crate) fn rebuild_projects(&mut self, all: Vec<pi_link::sessions::SessionInfo>) {
        // PF 项目注册表（打开项目即注册）：打开时刻参与 newest() 排序兑底。
        // 注册项目**零会话不建组**（用户 2026-10-18 定案，推翻 10-16 空组
        // 并集口径）——psp 只显示装载窗口内有会话的项目 + 当前项目空组；
        // 全部注册项仍在「打开项目」弹窗可寻
        let records = pi_link::projects::project_records();
        let opened: std::collections::HashMap<String, i64> = records
            .iter()
            .map(|r| (same_ws_key(&r.path.to_string_lossy()), r.last_opened_ms))
            .collect();
        let opened_ms = |p: &std::path::Path| {
            opened
                .get(&same_ws_key(&p.to_string_lossy()))
                .copied()
                .unwrap_or(0)
        };
        let mut groups: Vec<crate::ProjectGroup> = Vec::new();
        // 影子会话（微信线 pf-wxbot-*）不建行：过滤清单由模块注入
        let hidden = pi_link::hidden_sessions::load();
        for s in all {
            if hidden.hides(s.name.as_deref(), &s.path) {
                continue;
            }
            let cwd = PathBuf::from(s.cwd.clone());
            // 应用自有目录（载荷目录/npm 包根）不建组：污染期落在 payload
            // 的历史脏会话不得把安装目录钉成常驻项目组（注册表已由 boot
            // 清洗，这里堵会话扫描并集这一路）
            if pi_link::paths::is_app_install_path(&cwd) {
                continue;
            }
            if let Some(g) = groups
                .iter_mut()
                .find(|g| same_ws(&g.path.to_string_lossy(), &cwd.to_string_lossy()))
            {
                g.sessions.push(s);
            } else {
                groups.push(crate::ProjectGroup {
                    name: crate::services::workspace::project_name(&cwd),
                    opened_ms: opened_ms(&cwd),
                    path: cwd,
                    sessions: vec![s],
                });
            }
        }
        // 当前项目空组仅在有项目时建（无项目态没有「当前项目」行）
        if let Some(cwd) = self.ws.cwd.clone()
            && !groups
                .iter()
                .any(|g| same_ws(&g.path.to_string_lossy(), &cwd.to_string_lossy()))
        {
            groups.push(crate::ProjectGroup {
                name: crate::services::workspace::project_name(&cwd),
                path: cwd.clone(),
                sessions: Vec::new(),
                opened_ms: opened_ms(&cwd),
            });
        }
        // 组内会话恒按活动倒序（020）：项目排序走哪条路都不影响组内
        for g in &mut groups {
            g.sessions.sort_by(|a, b| b.modified.cmp(&a.modified));
        }
        // —— 项目排序（020 拖拽版）——
        // 1) 用户排序（拖拽产物）一旦存在永远生效：按保存序；未收录的
        //    新项目按活动倒序垫底；
        // 2) 无用户排序且是首建（projects 还为空）：默认序只排这一次——
        //    当前项目钉顶 + 活动倒序；此后运行期重建不再自动置顶；
        // 3) 其余运行期重建：保序，新出现的组按活动倒序追加尾部。
        if let Some(order) = &self.psp.proj_order {
            groups.sort_by(|a, b| {
                let ka = order
                    .iter()
                    .position(|k| *k == same_ws_key(&a.path.to_string_lossy()));
                let kb = order
                    .iter()
                    .position(|k| *k == same_ws_key(&b.path.to_string_lossy()));
                match (ka, kb) {
                    (Some(x), Some(y)) => x.cmp(&y),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => newest(b).cmp(&newest(a)),
                }
            });
        } else if self.psp.projects.is_empty() {
            // current pinned first, the rest by newest session mtime desc
            let cwd = self.ws.cwd.clone();
            groups.sort_by(|a, b| {
                let ka = cwd
                    .as_deref()
                    .is_some_and(|cur| same_ws(&a.path.to_string_lossy(), &cur.to_string_lossy()));
                let kb = cwd
                    .as_deref()
                    .is_some_and(|cur| same_ws(&b.path.to_string_lossy(), &cur.to_string_lossy()));
                match (ka, kb) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => newest(b).cmp(&newest(a)),
                }
            });
        } else {
            let mut ordered: Vec<crate::ProjectGroup> = Vec::with_capacity(groups.len());
            let mut pool = groups;
            for old in &self.psp.projects {
                let k_old = same_ws_key(&old.path.to_string_lossy());
                if let Some(pos) = pool
                    .iter()
                    .position(|g| same_ws_key(&g.path.to_string_lossy()) == k_old)
                {
                    ordered.push(pool.remove(pos));
                }
            }
            pool.sort_by(|a, b| newest(b).cmp(&newest(a)));
            ordered.extend(pool);
            groups = ordered;
        }
        // v60: 加载单位是会话（设置.默认加载会话数）——组由这批会话的
        // cwd 自然形成，不再按项目数截断
        self.psp.projects = groups;
        self.sync_current_sessions();
    }

    /// 启动自动折叠（其他页「自动折叠时间」档位 1/3/7 天，内部按小时算）：
    /// 项目内**所有**会话的最后活动时间都早于阈值 → 该项目组默认折叠。
    /// 只在启动装载项目集后调一次（`startup::spawn_session_list_load`）——
    /// 运行期 `refresh_sessions` 等重建不得再折叠，否则用户手动展开的组
    /// 会被下一轮重建打回去。空组（无会话）不折叠；当前项目永不折叠
    /// （钉顶的活动项目折叠等于看不见自己刚开的会话）。
    pub(crate) fn auto_collapse_stale_projects(&mut self) {
        let hours = crate::services::workspace::auto_collapse_hours();
        let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(hours * 3600);
        let cwd = self.ws.cwd.clone();
        for g in &self.psp.projects {
            if g.sessions.is_empty() {
                continue;
            }
            if cwd
                .as_deref()
                .is_some_and(|cur| same_ws(&g.path.to_string_lossy(), &cur.to_string_lossy()))
            {
                continue;
            }
            if newest(g) < cutoff {
                self.psp
                    .collapsed_keys
                    .insert(same_ws_key(&g.path.to_string_lossy()));
            }
        }
    }

    /// Keep `sessions` (current project) in sync with the matching group.
    pub(crate) fn sync_current_sessions(&mut self) {
        // 无项目态：没有「当前项目」清单
        let Some(cwd) = self.ws.cwd.clone() else {
            self.pool.sessions = Vec::new();
            return;
        };
        let cwd = cwd.to_string_lossy().to_string();
        self.pool.sessions = self
            .psp
            .projects
            .iter()
            .find(|g| same_ws(&g.path.to_string_lossy(), &cwd))
            .map(|g| g.sessions.clone())
            .unwrap_or_default();
    }

    /// 会话是否已置顶（020 会话置顶；路径判等 same_path）。
    pub(crate) fn session_pinned(&self, path: &std::path::Path) -> bool {
        self.psp.session_pins.iter().any(|p| same_path(p, path))
    }

    /// 会话置顶开关（pin/unpin 唯一写路）：pin = 插到最前（后置顶的总在
    /// 最上面）；unpin = 移除（置顶组随之空掉 → 不再显示）。落盘时机即
    /// 点击时刻（不等关窗，防异常退出丢失）。
    pub(crate) fn session_pin_toggle(&mut self, path: PathBuf, cx: &mut gpui::Context<Self>) {
        match self
            .psp
            .session_pins
            .iter()
            .position(|p| same_path(p, &path))
        {
            Some(ix) => {
                self.psp.session_pins.remove(ix);
            }
            None => self.psp.session_pins.insert(0, path),
        }
        self.persist_ui();
        cx.notify();
    }
}

fn newest(g: &crate::ProjectGroup) -> std::time::SystemTime {
    let from_sessions = g
        .sessions
        .iter()
        .map(|s| s.modified)
        .max()
        .unwrap_or(std::time::UNIX_EPOCH);
    // 注册表打开时刻兑底：零会话的注册项目按打开时刻参与活动排序
    let from_open =
        std::time::UNIX_EPOCH + std::time::Duration::from_millis(g.opened_ms.max(0) as u64);
    from_sessions.max(from_open)
}

// ---------------------------------------------------------------------------
// psp view
// ---------------------------------------------------------------------------

pub(crate) fn psp_view(
    chat: &mut Chat,
    _entity: Entity<Chat>,
    weak: gpui::WeakEntity<Chat>,
    window: &mut gpui::Window,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    // 020-1 会话子面板：控制栏固定在子面板顶（操作行永远可见），只有列表
    // 滚动。之前操作行放滚动容器内时，其按钮 hitbox 被 scroll mask 裁掉，
    // 点击全部失效——这也是设计的本意：操作行不是列表内容。
    let t = T();
    // 统一对齐线（§7 / tokens::align）：控制栏钮图标与列表行图标共
    // 用视觉左缘 ICON_LINE。容器 px = LIST_PX 是唯一水平留白源，
    // 行内 pl = ROW_PL 是对齐补偿（LIST_PX + ROW_PL = ICON_LINE）；右线 =
    // LIST_PX。改线只改 tokens 里 ICON_LINE，这里全自动跟随。
    let sess_min = chat.layout.sess_min;
    let files_min = chat.layout.files_min;
    let mut panel = div()
        .id("sub-sess")
        .w_full()
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(psp_ctrl_bar(chat, &weak, t));
    if !sess_min {
        let mut body = div()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            .px(px(align::LIST_PX))
            .pt(px(6.));
        let mut rows: Vec<gpui::AnyElement> = Vec::new();
        // 避让位移（020 拖拽）的块归属：Project 行之后的一切行都属于该
        // 项目块（Session 自带下标；Empty/More 跟随最近的 Project 行），
        // 整组一起平移让位；首个 Project 之前的孤儿伪标签不参与平移。
        let mut last_pi: Option<usize> = None;
        for row in psp_rows(chat) {
            let block = match &row {
                PspRow::Project(pi) => {
                    last_pi = Some(*pi);
                    None
                }
                // 置顶组行不参与项目块避让平移（头行在所有 Project 之前，
                // 置顶组引用的是别组的项目块）
                PspRow::PinnedHeader | PspRow::PinnedSession { .. } => None,
                PspRow::Session { p, .. } => Some(*p),
                _ => last_pi,
            };
            let shift = block.map(|b| psp_drag::row_shift(chat, b)).unwrap_or(0.);
            match row {
                PspRow::Project(pi) => rows.push(project_row(chat, pi, &weak, t)),
                PspRow::PinnedHeader => rows.push(pinned_header_row(t)),
                PspRow::Session { p, s } => rows.push(session_row_view(
                    chat, p, s, shift, false, &weak, t, window, cx,
                )),
                PspRow::PinnedSession { p, s } => rows.push(session_row_view(
                    chat, p, s, shift, true, &weak, t, window, cx,
                )),
                PspRow::Pending { key } => rows.push(pending_row_view(chat, &key, shift, &weak, t)),
                PspRow::Empty => rows.push(
                    div()
                        .id("psp-empty")
                        .relative()
                        .top(px(shift))
                        .pl(px(45.))
                        .py(px(5.5))
                        // 与「显示更多」行同款：placeholder 色 + 面板字号−1（2026-10-10 定夺）
                        .text_size(crate::appearance::ui_size(11.))
                        .text_color(rgb(t.text_faint))
                        .child(tr("psp.no_sessions"))
                        .into_any_element(),
                ),
                PspRow::More { key } => {
                    let weak_more = weak.clone();
                    let key_more = key.clone();
                    rows.push(
                        div()
                            .id(SharedString::from(format!("psp-more-{key}")))
                            .relative()
                            .top(px(shift))
                            .pl(px(45.))
                            .py(px(5.5))
                            // placeholder 色 + 面板字号−1（2026-10-10 定夺）
                            .text_size(crate::appearance::ui_size(11.))
                            .text_color(rgb(t.text_faint))
                            .cursor_pointer()
                            .hover(|s| s.text_color(rgb(t.text)))
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let key = key_more.clone();
                                let _ = weak_more.update(cx, |chat, cx| {
                                    *chat.psp.psp_shown.entry(key).or_insert_with(psp_page) +=
                                        psp_page();
                                    cx.notify();
                                });
                            })
                            .child(tr("settings.show_more"))
                            .into_any_element(),
                    );
                }
            }
        }
        // psp 会话列表滚动 + 右缘滚动条。Scrollbar 挂在与滚动容器平级的
        // relative 包裹上（贴 dock 右缘，不吃容器内 10px 留白）；ZED Regular
        // 视觉：只有 thumb 无 track（track 色已在 sync_gpui_tokens 透明化）。
        body = body.child(
            div()
                .id("psp-scroll-wrap")
                .relative()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(
                    div()
                        .id("psp-scroll")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&chat.psp.psp_scroll)
                        // 鼠标进 panel → 滚动条立现；离开 → 3s 后淡出（自绘
                        // psp_scrollbar 读这两个状态自绘透明度）
                        .on_hover(cx.listener(|this, hovered: &bool, _w, cx| {
                            this.psp.psp_sb_state.set_parent_hovered(*hovered);
                            cx.notify();
                        }))
                        // 滚动时立刻关掉全部浮层（详情卡/项目tooltip/删除确认）
                        //——它们锚定的是行位置，滚动后锚点漂移，钉在原地
                        // 只会错位（用户定稿：滚动 = 菜单立即消失）
                        .on_scroll_wheel(cx.listener(|this, _: &gpui::ScrollWheelEvent, _w, cx| {
                            if this.dlg.hover_card.is_some()
                                || this.dlg.proj_card.is_some()
                                || this.dlg.proj_tip.is_some()
                            {
                                this.dlg.hover_card = None;
                                this.dlg.proj_card = None;
                                this.dlg.proj_tip = None;
                                cx.notify();
                            }
                        }))
                        // 项目拖拽（020）：容器级 drag_move = 状态同步漏斗——
                        // 列表区内续期、拖出列表区立即清态（指示线消失、收起
                        // 的组复原）；on_drop 兜底整片列表的落点提交（行内半行
                        // 判定已在 project_row 做，这里只消费 proj_drop_ix）
                        .on_drag_move::<psp_drag::ProjDragTab>({
                            let w_sync = weak.clone();
                            move |ev, window, cx| {
                                let inside = ev.bounds.contains(&ev.event.position);
                                let key = ev.drag(cx).key.clone();
                                // 拖拽全程全窗口握紧小手：AnyDrag.cursor_style
                                // 是窗口级光标（优先于一切行光标），拖拽激活时
                                // 从源行样式取值（OpenHand），这里覆写成
                                // ClosedHand；refresh 与拖拽期逐 move 刷新合并
                                let _ = cx.set_active_drag_cursor_style(
                                    gpui::CursorStyle::ClosedHand,
                                    window,
                                );
                                let _ = w_sync.update(cx, |c, cx| {
                                    psp_drag::sync_drag(c, key, inside, cx);
                                });
                            }
                        })
                        .on_drop::<psp_drag::ProjDragTab>({
                            let w_drop = weak.clone();
                            move |_, _, cx| {
                                let _ = w_drop.update(cx, |c, cx| {
                                    let target = c.psp.proj_drop_ix;
                                    psp_drag::commit_drop(c, target, cx);
                                });
                            }
                        })
                        // 水平留白由 col 外层统一（10px），此处不再叠加
                        .flex()
                        .flex_col()
                        .children(rows),
                )
                // 自绘滚动条（ZED Regular 移植）：贴边 0px；无 track；panel 内
                // 常显、离开 3s+1s 淡出；thumb 悬停加宽；事件不穿透
                .child(
                    div()
                        .absolute()
                        .top(px(0.))
                        .bottom(px(0.))
                        .right(px(-10.))
                        .w(px(10.))
                        .child(crate::ui::psp_scrollbar::psp_scrollbar(
                            &chat.psp.psp_sb_state,
                            &chat.psp.psp_scroll,
                        )),
                ),
        );
        panel = panel.child(body);
    }
    // 子面板在 dock 列里的伸缩（020-1）：对面最大化 → 本面板填满；本面板
    // 最小 → 只剩控制栏（auto 高）；常态 = 分隔线占比
    if files_min {
        panel = panel.flex_1().min_h_0();
    } else if sess_min {
        panel = panel.flex_shrink_0();
    } else {
        panel = panel
            .flex_basis(gpui::relative(chat.layout.dock_split))
            .flex_shrink_0()
            .min_h_0();
    }
    panel.into_any_element()
}

/// 020-1 会话子面板控制栏（38px、无下 border）：左二 icon+文字（大图标
/// LG18）= 项目（打开项目）/ 会话（新建会话）；右三纯 icon（小图标 MD16）
/// = 搜索 + 列表切换（项目分组 ↔ 最近会话，图标随模式换）+ 最大化（本面板
/// 填满、文件树收起到操作栏）。搜索已降级为小 icon 并入右侧组（去掉文字，
/// 与右侧钮同宽 26 同档 MD16）。左缘 = ICON_LINE 12（容器 pl4 + 钮 px8）、
/// 右缘 = LIST_PX 10，与文件树操作栏同线。
///
/// **自动隐藏**：右侧「搜索」「列表切换」「最大化」三颗常态不可见
/// （opacity 0），鼠标进入本控制栏（group `psp-bar-reveal`）才显影——少三
/// 颗粒常驻图标减视觉冗余。只用绘制期 opacity，钮盒位置/命中区不变（列
/// 对齐不抖）。受设置-界面「部分操作隐藏」总开关（`autohide_icons`，默认
/// on）控制。
fn psp_ctrl_bar(chat: &Chat, weak: &gpui::WeakEntity<Chat>, t: &'static Theme) -> gpui::AnyElement {
    let grouped = chat.psp.list_mode == ListMode::Grouped;
    let w_open = weak.clone();
    let w_new = weak.clone();
    let w_search = weak.clone();
    let w_toggle = weak.clone();
    let w_max = weak.clone();
    // 钮盒：h30（tokens::panel_ctrl::BTN_H）px8 gap6，左二带文字（ui_size
    // 12.5 Semibold），右三定宽 26 纯 icon（与文件树操作栏钮盒同宽，列对
    // 齐见下方注释）；hover = bg_hover + 前景变亮（icon 走同名 group 的
    // group_hover 同步变色）
    let btn = |id: &'static str| {
        div()
            .id(id)
            .h(px(crate::ui::tokens::panel_ctrl::BTN_H))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(crate::ui::tokens::space::SP3))
            .rounded(px(8.))
            .cursor_pointer()
            .group("pc-btn")
            .text_size(crate::appearance::ui_size(12.5))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(rgb(t.text_dim))
            .hover(move |s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
    };
    let icon = |name: &'static str, size: f32| {
        gpui::svg()
            .path(SharedString::from(format!("icons/{name}.svg")))
            .text_color(rgb(t.text_dim))
            .group_hover("pc-btn", |s| s.text_color(rgb(t.text)))
            .size(px(size))
            .into_any_element()
    };
    // 右缘三颗纯 icon 钮链外组好再进 child（child 实参位置不能写 let）。
    // 自动隐藏（设置-界面「部分操作隐藏」，默认 on）= 常态透明、控制栏
    // hover 显影；命中区/布局不动，关掉开关则恒显。搜索/列表切换/最大化
    // 三颗低频入口一并纳入显影组
    let autohide = |b: gpui::Stateful<gpui::Div>| {
        if crate::services::workspace::autohide_icons() {
            b.opacity(0.)
                .group_hover("psp-bar-reveal", |s| s.opacity(1.))
        } else {
            b
        }
    };
    let search_btn = autohide(btn("psp-search").w(px(26.)).px(px(0.)).justify_center());
    let list_toggle = autohide(
        btn("psp-list-toggle")
            .w(px(26.))
            .px(px(0.))
            .justify_center(),
    );
    div()
        .id("psp-ctrl")
        // 自动隐藏的显影组：鼠标在本控制栏内 → 右侧三颗低频钮显影
        .group("psp-bar-reveal")
        .w_full()
        .flex_shrink_0()
        .h(px(crate::ui::tokens::panel_ctrl::BAR_H))
        .flex()
        .items_center()
        .gap(px(2.))
        // 左线 = ICON_LINE（4 + 钮内边距 8）；右缘 = LIST_PX
        .pl(px(4.))
        .pr(px(align::LIST_PX))
        // 左二：icon+文字
        .child(
            btn("psp-open-project")
                .with_tip_action("psp.open_project", &crate::OpenProjectPicker)
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = w_open.update(cx, |c, cx| c.open_project_picker(false, cx));
                })
                .child(icon("icon-project", crate::ui::tokens::icon::LG))
                .child(tr("scope.project")),
        )
        .child(
            btn("psp-new-session")
                .with_tip_action("psp.new_session", &crate::NewSession)
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = w_new.update(cx, |c, cx| c.new_session(cx));
                })
                // 会话 icon 是 iconfont 实底形（viewBox 824×767，笔画内容
                // 占 ~79%），比 lucide 描边形（~83%）光学上小一圈——19 =
                // 旧操作行验证过的补偿值（其余两颗 18 时它 19），维持同视高
                .child(icon("icon-new-chat", 19.))
                .child(tr("psp.session_label")),
        )
        .child(div().flex_1())
        // 右三：纯 icon（钮盒 26 = 文件树操作栏 bar_btn 同款，bar gap 2 →
        // 与下方操作栏右缘逐列对齐：最大化列、次列图标中心严格同 x）。
        // 搜索居本组最左 = 右缘定宽对齐不动，只往左多占一列
        .child(
            search_btn
                .with_tip_action("psp.search_sessions", &crate::OpenSessionSearch)
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = w_search.update(cx, |c, cx| c.open_session_search_panel(cx));
                })
                .child(icon("search", crate::ui::tokens::icon::MD)),
        )
        .child(
            // 列表切换：图标随当前模式换（分组 = folder-list，扁平 =
            // icon-flat），点击直切，不再走 ⋯ 菜单（020-1 定稿）
            list_toggle
                .with_tip(if grouped {
                    "psp.switch_to_flat"
                } else {
                    "psp.switch_to_grouped"
                })
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = w_toggle.update(cx, |c, cx| {
                        c.psp.list_mode = match c.psp.list_mode {
                            ListMode::Grouped => ListMode::Flat,
                            ListMode::Flat => ListMode::Grouped,
                        };
                        c.persist_ui();
                        cx.notify();
                    });
                })
                .child(icon(
                    if grouped {
                        "icon-folder-list"
                    } else {
                        "icon-flat"
                    },
                    crate::ui::tokens::icon::MD,
                )),
        )
        .child(
            // 最大化（单钮语义）：本面板填满，文件树收起到操作栏；拖分隔线
            // 即退出最大态。Alt+1 = 同一键（082 全局绑定，chip 反查 keymap）。
            // 与搜索/列表切换同入显影组（自动隐藏，见 autohide 头注）
            autohide(btn("psp-panel-max").w(px(26.)).px(px(0.)).justify_center())
                .with_tip_action("psp.maximize_session_list", &crate::MaximizeSessionList)
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = w_max.update(cx, |c, cx| {
                        c.layout.files_min = true;
                        c.layout.sess_min = false;
                        // 最大化态立即落盘（不必等关窗，防异常退出丢失）
                        c.persist_ui();
                        cx.notify();
                    });
                })
                .child(icon("maximize", crate::ui::tokens::icon::MD)),
        )
        .into_any_element()
}

/// 项目行：folder 开合图标 + 名字；点击 = 组收起/展开；hover 显新会话。
/// 拖拽排序三件套挂本行（020）：`on_drag` 发起（幽灵 = 全行宽项目行小卡，
/// 「收起为标签」的可见形态；源行 opacity 0、组体折叠在 [`psp_rows`] 做）、
/// `on_drag_move` 按鼠标在行的上/下半判定插入位（提交与取消清理走容器）；
/// 避让让位 = 本行按所属块的动画位移整体平移（`.top()`，空槽即落点）。
fn project_row(
    chat: &Chat,
    pi: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
) -> gpui::AnyElement {
    let Some(g) = chat.psp.projects.get(pi) else {
        return div().into_any_element();
    };
    let active = chat
        .ws
        .cwd
        .as_deref()
        .is_some_and(|cur| same_ws(&g.path.to_string_lossy(), &cur.to_string_lossy()));
    let collapsed = chat
        .psp
        .collapsed_keys
        .contains(&same_ws_key(&g.path.to_string_lossy()));
    let hovered = chat.psp.hovered_project == Some(pi);
    let path = g.path.clone();
    let path_toggle = path.clone();
    let path_card = path.clone();
    let name: SharedString = g.name.clone().into();
    let drag_key = same_ws_key(&g.path.to_string_lossy());
    let w_toggle = weak.clone();
    let w_new = weak.clone();
    let w_tip_on = weak.clone();
    let w_card = weak.clone();
    let w_drag_ix = weak.clone();
    let ghost_name = g.name.clone();
    let ghost_w = chat.layout.slp_w - 2. * align::LIST_PX;
    // 拖拽中的源行：隐形但保留 32px 槽位——「被拎起」的空感，落点空槽
    // 由其余块的避让平移让出（ZCode 源行 opacity 0 同款）
    let is_dragged = chat
        .psp
        .proj_drag
        .as_ref()
        .is_some_and(|d| d.key == drag_key);
    let shift = psp_drag::row_shift(chat, pi);

    div()
        .id(SharedString::from(format!("prj-{pi}")))
        .relative()
        .top(px(shift))
        // §5.8 主列表档真行高 32（原 33=32+mb2 补偿，mb 写法废除）
        .h(px(crate::ui::tokens::row::MAIN))
        // 滚动容器 overflow 压缩行时 flex-shrink 会把行压到内容高
        // （min-height:auto），hover 按钮一现（28 高）行高就跳变、整列
        // 下移（「行浮动」）——列表行一律禁止收缩
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(8.))
        .pl(px(align::ROW_PL))
        .pr(px(align::LIST_PX))
        .rounded(px(8.))
        // 抓取手型（020）：Windows 系统没有掌形光标，OpenHand/ClosedHand
        // 由 vendored gpui 自绘 SVG 光标实现（platform/windows/cursor.rs，
        // 用户供图 lucide hand/hand-grab）；拖拽开始后全窗口切握紧手
        .cursor(gpui::CursorStyle::OpenHand)
        .when(is_dragged, |d| d.opacity(0.))
        // 折叠切换走 on_click（按下+抬起无位移才触发）：on_mouse_down 会
        // 在拖拽起手时就翻转折叠态，与拖拽语义打架（gpui 拖拽激活会吞掉
        // click，两者天然互斥）
        .on_click(move |_, _, cx| {
            let _ = w_toggle.update(cx, |c, cx| {
                let key = crate::services::workspace::same_ws_key(&path_toggle.to_string_lossy());
                if !c.psp.collapsed_keys.remove(&key) {
                    c.psp.collapsed_keys.insert(key);
                }
                c.persist_ui();
                cx.notify();
            });
        })
        .on_mouse_down(
            MouseButton::Right,
            move |ev: &gpui::MouseDownEvent, _, cx| {
                // 项目行右键详情卡（020）：样式对齐会话详情卡，锚点 = 鼠标
                // 视口坐标。与会话卡互斥：右键别行 = 收旧开新
                let (x, y) = (f32::from(ev.position.x), f32::from(ev.position.y));
                let _ = w_card.update(cx, |c, cx| {
                    c.dlg.hover_card = None;
                    c.dlg.proj_card = Some(crate::function_panel::ProjCard {
                        path: path_card.clone(),
                        x,
                        y,
                        confirming: false,
                    });
                    cx.notify();
                });
            },
        )
        .on_drag(
            psp_drag::ProjDragTab {
                key: drag_key.clone(),
            },
            {
                // 幽灵定位由 gpui 按 mouse − cursor_offset 完成（天然保留
                // 抓取点偏移），这里只捕获锁 x 补偿要用的起手鼠标位
                move |_, _, window, cx| {
                    let mouse = window.mouse_position();
                    cx.new(|_| psp_drag::ProjDragGhost {
                        name: ghost_name.clone(),
                        width: ghost_w,
                        start_mouse_x: f32::from(mouse.x),
                    })
                }
            },
        )
        .on_drag_move::<psp_drag::ProjDragTab>(move |ev, _, cx| {
            // 每次 mouse move 全体在册行都会收到（capture 相、无悬停过
            // 滤），contains 自筛：只有鼠标所在的行往下走。命中区随避让
            // 平移走 =「所见即所拖」（dnd-kit 手感）
            if !ev.bounds.contains(&ev.event.position) {
                return;
            }
            let key = ev.drag(cx).key.clone();
            // 半行判定：上/下半 = 插到本行前/后；拖回自己或相邻 = no-op
            // → 空槽闭合、drop 也不动（no-op 对比的是**被拖项目**的当前位
            // 置，不是本行——本行位置和 target 同源，比了等于全判原地）
            let target = if ev.event.position.y < ev.bounds.center().y {
                pi
            } else {
                pi + 1
            };
            let _ = w_drag_ix.update(cx, |c, cx| {
                psp_drag::sync_drag(c, key.clone(), true, cx);
                let no_op = c
                    .psp
                    .projects
                    .iter()
                    .position(|g| psp_drag::group_key(g) == key)
                    .is_some_and(|cur| target == cur || target == cur + 1);
                psp_drag::set_drop_target(c, (!no_op).then_some(target), cx);
            });
        })
        .on_hover(move |h, _, cx| {
            let _ = w_tip_on.update(cx, |c, cx| {
                let on = *h;
                if on {
                    if c.psp.hovered_project != Some(pi) {
                        c.psp.hovered_project = Some(pi);
                        cx.notify();
                    }
                } else {
                    if c.psp.hovered_project == Some(pi) {
                        c.psp.hovered_project = None;
                    }
                    if c.dlg.proj_tip.is_some() {
                        c.dlg.proj_tip = None;
                        cx.notify();
                    }
                }
            });
        })
        // 项目路径悬浮条（proj_tip）已关：用户肯定知道自己的目录路径
        //（2026-10-14 定夺）——触发器摘除，机制保留
        .child(icon(
            if collapsed { "folder" } else { "folder-open" },
            15.,
            t.text_dim,
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                // 项目标签 = 面板设置值（字体大小设置.md §1）；
                // 020-1：选中项目 = 主题色 + 粗体（与文件树操作栏项目名一致）
                .text_size(crate::appearance::ui_size(12.))
                .font_weight(if active {
                    gpui::FontWeight::BOLD
                } else {
                    gpui::FontWeight::MEDIUM
                })
                .text_color(if active {
                    rgb(t.accent)
                } else {
                    rgb(t.text_dim)
                })
                .child(name),
        )
        .when(hovered, |d| {
            d.child(
                div()
                    .flex()
                    // 列对齐：行内容右缘 = 容器 px10 + 行 pr10 = 距面板 20，
                    // 而上下两条控制栏钮右缘距面板 10——负右距 10 把钮组拉回
                    // 同线，新会话↔最大化同 x（仍落在行 bounds 右缘之内，
                    // 行 hover 不会丢）。垃圾桶钮已撤（2026-10-16）：删除
                    // 走项目行右键详情卡
                    .mr(px(-10.))
                    .gap(px(2.))
                    // ＋ → 小号新建会话 icon（与控制栏「会话」同图形，小档）
                    .child(
                        div()
                            .id(SharedString::from(format!("prj-new-{pi}")))
                            .w(px(26.))
                            .h(px(26.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .cursor_pointer()
                            .group("prj-act")
                            .text_color(rgb(t.text_dim))
                            .hover(|s| s.bg(rgb(t.bg_hover)))
                            .with_tip("psp.new_session_in_project")
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                cx.stop_propagation();
                                let _ = w_new.update(cx, |c, cx| {
                                    c.new_session_in(path.clone(), cx);
                                });
                            })
                            .child(
                                gpui::svg()
                                    .path(SharedString::from("icons/icon-new-chat.svg"))
                                    .text_color(rgb(t.text_dim))
                                    .group_hover("prj-act", |s| s.text_color(rgb(t.text)))
                                    .size(px(crate::ui::tokens::icon::MD))
                                    .into_any_element(),
                            ),
                    ),
            )
        })
        .into_any_element()
}

// ---------------------------------------------------------------------------
// dock container (020-1 v54): nav 底色 + 上下子面板（会话/文件树）+ 分隔线
// ---------------------------------------------------------------------------

pub(crate) fn dock(
    chat: &mut Chat,
    entity: Entity<Chat>,
    weak: &gpui::WeakEntity<Chat>,
    window: &mut gpui::Window,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    // git 面板保留独立视图（022；状态栏 tab 已隐藏，automation panel.dock 可达）
    if chat.layout.dock_panel == DockPanel::Git {
        return div()
            .id("dock")
            .flex_1()
            .min_h_0()
            .relative()
            .flex()
            .flex_col()
            .bg(rgb(t.nav))
            .child(git_panel::view(chat, weak, cx))
            .into_any_element();
    }
    // 插件面板（beads 360 反馈②）：左列格子视图，互斥于整合面板/git。
    // **停用/条目不在（已被摘掉）时直接漏到下面的默认 psp 列表**——
    // 左列是「停用后仍旧挂着旧面板」的最后一道口，不能只倚靠状态侧去撤 dock。
    if let DockPanel::Plugin(ix) = chat.layout.dock_panel
        && chat.plugins.plugins.get(ix).is_some_and(|e| !e.disabled)
    {
        return div()
            .id("dock")
            .flex_1()
            .min_h_0()
            .relative()
            .flex()
            .flex_col()
            .bg(rgb(t.nav))
            .child(crate::pf_plugin::plugin_panel_view(chat, ix, weak))
            .into_any_element();
    }
    // 微信面板已迁出 dock（2026-10-23）：入口收三横菜单【微信通信】，
    // 内容区全屏页（ContentView::Wx，remote_control::panel::page）。
    // 020-1 整合面板：会话列表(上) + 文件树(下)，默认 50/50；分隔线拖拽
    // 改占比（拖拽即退出两侧最小态、双击复位 50%），各自可收起到控制栏
    //（单最大化钮，语义见 LayoutUi::sess_min / files_min）
    let bounds_slot = chat.layout.dock_bounds.clone();
    // §5.12 统一分隔线：常驻 1px；**hover 与拖拽同为 4px accent**
    let split_drag = chat.layout.split_drag;
    let split_active = split_drag || chat.layout.split_hover;
    let d_hit = crate::ui::tokens::divider::HIT;
    div()
        .id("dock")
        .flex_1()
        .min_h_0()
        .relative()
        .flex()
        .flex_col()
        .bg(rgb(t.nav))
        // dock 容器 bounds 每帧捕获（PillBtns::tracker 同款 canvas）：
        // app-body 层的拖拽跟踪要把窗口 y 换算成占比
        .child(
            gpui::canvas(
                move |bounds, _, _| *bounds_slot.borrow_mut() = Some(bounds),
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        )
        .child(psp_view(chat, entity, weak.clone(), window, cx))
        // 子面板分隔线：7px 命中带 + 常驻 1px 视觉线（左右缩进 10 = 列表
        // 右线），hover/拖拽按 §5.12 统一规范换 4px accent 覆盖线（absolute
        // 改 top/h 不占布局零推挤，带内居中 = 与常驻线同一中线）
        .child(
            div()
                .id("sp-divider")
                .with_tip("psp.split_tip")
                .h(px(d_hit))
                .flex_shrink_0()
                .relative()
                .cursor_row_resize()
                // hover 状态存进字段（与 content 侧 slp_hover 同一机制），
                // 不用 group_hover——后者受 has_active_drag 门控，粗细拿不到
                .on_hover({
                    let weak_hv = weak.clone();
                    move |h: &bool, _, cx| {
                        let _ = weak_hv.update(cx, |c, cx| {
                            if c.layout.split_hover != *h {
                                c.layout.split_hover = *h;
                                cx.notify();
                            }
                        });
                    }
                })
                .on_mouse_down(MouseButton::Left, {
                    let weak_dz = weak.clone();
                    move |ev: &gpui::MouseDownEvent, _, cx| {
                        let _ = weak_dz.update(cx, |c, cx| {
                            if ev.click_count == 2 {
                                c.layout.dock_split = 0.5;
                                c.layout.sess_min = false;
                                c.layout.files_min = false;
                                c.persist_ui();
                            } else {
                                c.layout.split_drag = true;
                            }
                            cx.notify();
                        });
                    }
                })
                // 线形交给统一实现（ui::divider）：1px 常驻 / 4px accent
                // 高亮，带内垂直居中 = 两种粗细同一中线
                .child(crate::ui::divider::line(
                    crate::ui::divider::LineSpec::horizontal(10., d_hit),
                    split_active,
                    t,
                )),
        )
        .child(files_view(chat, weak, cx))
        .into_any_element()
}

/// files view：树渲染在 `file_tree.rs`（vlist 虚拟化 + 展平缓存，render
/// 不碰磁盘；Zed 图标主题 + gitignore 过滤在 services::file_tree/。
/// file_icons）。
pub(crate) fn files_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    _cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    file_tree::files_view(chat, weak, _cx)
}

// ---- psp/文件树 overlay 状态类型（自 shell/types.rs 下沉，本域自持）----

/// 会话右键详情卡（2026-10-15 定夺：悬停自动弹窗整体撤销，改右键召唤的
/// 上下文卡——定位右键点、上/下方弹出、点外即收；名称沿用 hover 时代）。
#[derive(Debug, Clone)]
pub(crate) struct HoverCard {
    pub path: PathBuf,
    /// 右键点视口坐标：卡左缘对齐 x、上/下方 12px 弹出（PopupEdges 四象限）
    pub x: f32,
    pub y: f32,
    /// 点「删除」后卡内换 取消/确认 两键
    pub confirming: bool,
    /// 标题点击后原地改名（卡内变输入框，Enter 提交 / Esc 取消）
    pub renaming: bool,
    pub rename_input: Option<gpui::Entity<crate::ui::TextInput>>,
}

/// 项目行右键详情卡（020：样式对齐会话详情卡）：标题加粗 + 路径/会话数
/// 信息行 + 分隔线下 标签栏项目行同款双图标动作栏（删除带卡内确认态）。
#[derive(Debug, Clone)]
pub(crate) struct ProjCard {
    pub path: PathBuf,
    /// 右键点视口坐标：卡左缘对齐 x、上/下方 12px 弹出（PopupEdges 四象限）
    pub x: f32,
    pub y: f32,
    /// 点「删除」后动作栏换 取消/确认 两键
    pub confirming: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum TreeMenu {
    Dir { path: PathBuf, x: f32, y: f32 },
    File { path: PathBuf, x: f32, y: f32 },
}

/// 文件树标题项目下拉（树根跨项目快切）：一览 psp.projects（会话列表里
/// 的全部项目），↑/↓ 高亮 + Enter 切换、点击直切。`sel` = 键盘高亮行
/// （psp.projects 下标，打开时默认落当前项目）。切换语义与激活文件标签
/// 的跨项目同步（file_tree::sync_tree_to_file）一致：只重挂工作区（树根/
/// git/fs watcher/侧栏当前项目），活跃会话不动、不写 last_workspace。
#[derive(Debug, Clone)]
pub(crate) struct TreeProjMenu {
    pub(crate) sel: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct TreeEdit {
    pub(crate) kind: TreeEditKind,
    pub(crate) is_dir: bool,
    pub(crate) input: gpui::Entity<TextInput>,
    /// 实时校验态（键入即查，Zed populate_validation_error 移植）
    pub(crate) validation: TreeValidation,
}

#[derive(Debug, Clone)]
pub(crate) enum TreeEditKind {
    /// 在该目录下新建（Zed leaf_entry_id: None——is_new_entry 判据）
    New { dir: PathBuf },
    /// 同目录改名该条目
    Rename { path: PathBuf },
}

/// Zed ValidationState 移植：错误（重名/非法）拦 Enter 提交，警告放行。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) enum TreeValidation {
    #[default]
    None,
    Error(String),
    Warning(String),
}
