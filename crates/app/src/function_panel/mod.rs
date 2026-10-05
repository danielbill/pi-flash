//! psp / dock (v54): 项目+会话一体列表（ChatGPT 式，设计核心=隐藏复杂度）。
//! dock = statusbar 三面板容器（psp / 文件树 / git），nav 底色，右缘 6px
//! 拖拽调宽（250–500，双击复位 282）。psp 的悬浮层（路径 tooltip / 会话
//! 详情卡 / 两级菜单 / 删除确认）由 `psp_overlays` 在根渲染挂载。

pub(crate) mod file_tree;
pub(crate) mod git_panel;
pub(crate) mod psp_overlays;

use gpui::{Context, Entity, MouseButton, SharedString, div, prelude::*, px, rgb};

use self::file_tree::collect_tree_rows;
use std::path::PathBuf;

use crate::Chat;
use crate::DockPanel;
use crate::i18n::tr;
use crate::services::workspace::{same_ws, same_ws_key};
use crate::ListMode;
use crate::SortMode;
use crate::theme::{Theme, theme as T};
use crate::ui::{icon, icon_hover, spinner};

/// Flattened psp row model (one virtual list over all rows).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PspRow {
    Title,
    Project(usize),
    Session { p: usize, s: usize },
    /// 空项目展开时的「无会话」占位
    Empty,
    /// 「显示更多」行（key = 组 ws_key；Flat 模式用全局键）
    More { key: String },
}

/// 每页显示的会话标签数（学 zcode：「显示更多」每次续一页）
const PSP_PAGE: usize = 10;

pub(crate) fn psp_rows(chat: &Chat) -> Vec<PspRow> {
    let mut rows = vec![PspRow::Title];
    match chat.list_mode {
        ListMode::Grouped => {
            for (pi, g) in chat.projects.iter().enumerate() {
                rows.push(PspRow::Project(pi));
                let key = same_ws_key(&g.path.to_string_lossy());
                if chat.collapsed_keys.contains(&key) {
                    continue;
                }
                if g.sessions.is_empty() {
                    rows.push(PspRow::Empty);
                } else {
                    let shown = chat
                        .psp_shown
                        .get(&key)
                        .copied()
                        .unwrap_or(PSP_PAGE)
                        .min(g.sessions.len());
                    for si in 0..shown {
                        rows.push(PspRow::Session { p: pi, s: si });
                    }
                    if g.sessions.len() > shown {
                        rows.push(PspRow::More { key });
                    }
                }
            }
        }
        ListMode::Flat => {
            let mut all: Vec<(usize, usize)> = Vec::new();
            for (pi, g) in chat.projects.iter().enumerate() {
                for si in 0..g.sessions.len() {
                    all.push((pi, si));
                }
            }
            if chat.sort_mode == SortMode::Time {
                all.sort_by(|a, b| {
                    let ma = chat.projects[a.0].sessions[a.1].modified;
                    let mb = chat.projects[b.0].sessions[b.1].modified;
                    mb.cmp(&ma)
                });
            }
            let key = "__flat__".to_string();
            let shown = chat.psp_shown.get(&key).copied().unwrap_or(PSP_PAGE);
            let total = all.len();
            all.truncate(shown);
            all.into_iter().for_each(|(p, s)| rows.push(PspRow::Session { p, s }));
            if total > shown {
                rows.push(PspRow::More { key });
            }
        }
    }
    rows
}

impl Chat {

    /// Group the cross-project session scan into ≤N project groups (current
    /// project pinned first, then by newest session mtime desc).
    pub(crate) fn rebuild_projects(&mut self, all: Vec<pi_link::sessions::SessionInfo>) {
        let mut groups: Vec<crate::ProjectGroup> = Vec::new();
        for s in all {
            let cwd = PathBuf::from(s.cwd.clone());
            if let Some(g) = groups
                .iter_mut()
                .find(|g| same_ws(&g.path.to_string_lossy(), &cwd.to_string_lossy()))
            {
                g.sessions.push(s);
            } else {
                groups.push(crate::ProjectGroup {
                    name: cwd
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| cwd.to_string_lossy().to_string()),
                    path: cwd,
                    sessions: vec![s],
                });
            }
        }
        let cwd = self.cwd.clone();
        if !groups.iter().any(|g| same_ws(&g.path.to_string_lossy(), &cwd.to_string_lossy())) {
            groups.push(crate::ProjectGroup {
                name: cwd
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| cwd.to_string_lossy().to_string()),
                path: cwd.clone(),
                sessions: Vec::new(),
            });
        }
        // current pinned first, the rest by newest session mtime desc
        groups.sort_by(|a, b| {
            let ka = same_ws(&a.path.to_string_lossy(), &cwd.to_string_lossy());
            let kb = same_ws(&b.path.to_string_lossy(), &cwd.to_string_lossy());
            match (ka, kb) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => newest(b).cmp(&newest(a)),
            }
        });
        // v60: 加载单位是会话（设置.默认加载会话数）——组由这批会话的
        // cwd 自然形成，不再按项目数截断
        self.projects = groups;
        self.sync_current_sessions();
    }

    /// Keep `sessions` (current project) in sync with the matching group.
    pub(crate) fn sync_current_sessions(&mut self) {
        let cwd = self.cwd.to_string_lossy().to_string();
        self.sessions = self
            .projects
            .iter()
            .find(|g| same_ws(&g.path.to_string_lossy(), &cwd))
            .map(|g| g.sessions.clone())
            .unwrap_or_default();
    }
}

fn newest(g: &crate::ProjectGroup) -> std::time::SystemTime {
    g.sessions
        .iter()
        .map(|s| s.modified)
        .max()
        .unwrap_or(std::time::UNIX_EPOCH)
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
    // title 行固定在 dock 顶部（操作行永远可见），只有列表滚动。
    // 之前 title 行放滚动容器内时，其按钮 hitbox 被 scroll mask 裁掉，
    // 点击全部失效——这也是设计的本意：title 行不是列表内容。
    let t = T();
    // 统一对齐线（v55）：col 外层 px 10 是唯一的水平留白源——title 行与
    // 列表同处一个坐标系；行内 pl/pr 10 叠加其上（图标列 = 10+10），
    // title 行自身 px 0（文字 = 10，与下方 folder 图标…不对，folder 图标
    // 在行内 pl 10 → 20px）。title 文字要与 folder 图标同线 → title 行
    // 也挂 pl 10（= 10 容器 + 10 行 = 20px 同线）。右线同理。
    let mut col = div()
        .flex_1()
        .min_h_0()
        .w_full()
        .flex()
        .flex_col()
        .px(px(10.))
        .pt(px(6.))
        .child(title_row(chat, &weak, t));
    let rows: Vec<gpui::AnyElement> = psp_rows(chat)
        .into_iter()
        .filter(|row| !matches!(row, PspRow::Title))
        .map(|row| match row {
            PspRow::Project(pi) => project_row(chat, pi, &weak, t),
            PspRow::Session { p, s } => session_row_view(chat, p, s, &weak, t, window, cx),
            PspRow::Empty => div()
                .id("psp-empty")
                .pl(px(45.))
                .py(px(5.5))
                .text_size(crate::appearance::ui_size(12.5))
                .text_color(rgb(t.text_dim))
                .child(tr("无会话"))
                .into_any_element(),
            PspRow::More { key } => {
                let weak_more = weak.clone();
                let key_more = key.clone();
                div()
                    .id(SharedString::from(format!("psp-more-{key}")))
                    .pl(px(45.))
                    .py(px(5.5))
                    .text_size(crate::appearance::ui_size(12.5))
                    .text_color(rgb(t.text_dim))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(t.text)))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let key = key_more.clone();
                        let _ = weak_more.update(cx, |chat, cx| {
                            *chat.psp_shown.entry(key).or_insert(PSP_PAGE) += PSP_PAGE;
                            cx.notify();
                        });
                    })
                    .child(tr("显示更多"))
                    .into_any_element()
            }
            PspRow::Title => div().into_any_element(),
        })
        .collect();
    // psp 会话列表滚动 + 右缘滚动条。Scrollbar 挂在与滚动容器平级的
    // relative 包裹上（贴 dock 右缘，不吃容器内 10px 留白）；ZED Regular
    // 视觉：只有 thumb 无 track（track 色已在 sync_gpui_tokens 透明化）。
    col = col.child(
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
                    .track_scroll(&chat.psp_scroll)
                    // 鼠标进 panel → 滚动条立现；离开 → 3s 后淡出（自绘
                    // psp_scrollbar 读这两个状态自绘透明度）
                    .on_hover(cx.listener(|this, hovered: &bool, _w, cx| {
                        this.psp_sb_state.set_parent_hovered(*hovered);
                        cx.notify();
                    }))
                    // 滚动时立刻关掉全部浮层（详情卡/⋯菜单/项目tooltip/删除
                    // 确认）——它们锚定的是行位置，滚动后锚点漂移，钉在原地
                    // 只会错位（用户定稿：滚动 = 菜单立即消失）
                    .on_scroll_wheel(cx.listener(|this, _: &gpui::ScrollWheelEvent, _w, cx| {
                        if this.hover_card.is_some()
                            || this.psp_menu.is_some()
                            || this.proj_tip.is_some()
                            || this.confirm_prj_del.is_some()
                        {
                            this.hover_card = None;
                            this.psp_menu = None;
                            this.proj_tip = None;
                            this.confirm_prj_del = None;
                            cx.notify();
                        }
                    }))
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
                    .w(px(12.))
                    .child(crate::ui::psp_scrollbar::psp_scrollbar(
                        &chat.psp_sb_state,
                        &chat.psp_scroll,
                    )),
            ),
    );
    col.into_any_element()
}

/// title 行（操作行）：左文案（项目/会话），右 4 常显钮。
fn title_row(chat: &Chat, weak: &gpui::WeakEntity<Chat>, t: &'static Theme) -> gpui::AnyElement {
    let label = match chat.list_mode {
        ListMode::Flat => tr("会话"),
        _ => tr("项目"),
    };
    let w_open = weak.clone();
    let w_new = weak.clone();
    let w_search = weak.clone();
    let w_sort = weak.clone();
    div()
        .id("psp-title")
        .w_full()
        .h(px(30.))
        .flex()
        .items_center()
        .pl(px(10.))
        .pr(px(10.))

        .text_size(crate::appearance::ui_size(12.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(t.text_soft))
        .child(div().flex_1().child(SharedString::from(label)))
        .child(
            div()
                .ml_auto()
                .flex()
                .items_center()
                .flex_shrink_0()
                .gap(px(10.))

                // 打开项目（iconfont 实底）
                .child(
                    div()
                        .id("psp-open-project")
                        .w(px(28.))
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(|s| s.text_color(rgb(t.text)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = w_open.update(cx, |c, cx| c.pick_project_folder(cx));
                        })
                        .child(icon_hover("icon-project", 18., t.text_dim)),
                )
                // 新建会话（19px 实底，ml 2）
                .child(
                    div()
                        .id("psp-new-session")
                        .ml(px(2.))
                        .w(px(28.))
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(|s| s.text_color(rgb(t.text)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = w_new.update(cx, |c, cx| c.new_session(cx));
                        })
                        .child(icon_hover("icon-new-chat", 19., t.text_dim)),
                )
                // 会话查询（lucide search）
                .child(
                    div()
                        .id("psp-search")
                        .w(px(28.))
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(|s| s.text_color(rgb(t.text)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = w_search.update(cx, |c, cx| c.open_session_search(cx));
                        })
                        .child(icon_hover("search", 18., t.text_dim)),
                )
                // 排序（ellipsis，无 tip）
                .child(
                    div()
                        .id("psp-sort-menu")
                        .w(px(28.))
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(|s| s.text_color(rgb(t.text)))
                        .on_mouse_down(MouseButton::Left, move |ev: &gpui::MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            let _ = w_sort.update(cx, |c, cx| {
                                c.psp_menu = Some(crate::PspMenu::Sort {
                                    sub: None,
                                    x: (f32::from(ev.position.x) - 20.).max(8.),
                                    y: f32::from(ev.position.y) + 12.,
                                });
                                cx.notify();
                            });
                        })
                        .child(icon_hover("ellipsis", 18., t.text_dim)),
                ),
        )
        .into_any_element()
}

/// 项目行：folder 开合图标 + 名字；点击 = 组收起/展开；hover 显 ⋯/＋。
fn project_row(
    chat: &Chat,
    pi: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
) -> gpui::AnyElement {
    let Some(g) = chat.projects.get(pi) else {
        return div().into_any_element();
    };
    let active = same_ws(&g.path.to_string_lossy(), &chat.cwd.to_string_lossy());
    let collapsed = chat.collapsed_keys.contains(&same_ws_key(&g.path.to_string_lossy()));
    let hovered = chat.hovered_project == Some(pi)
        // ⋯ 菜单打开中：按钮保持显示（交互进行中不消失）
        || matches!(
            &chat.psp_menu,
            Some(crate::PspMenu::Project { path: mp, .. })
            if crate::services::workspace::same_path(mp, &g.path)
        );
    let path = g.path.clone();
    let path_toggle = path.clone();
    let path_tip = path.clone();
    let path_menu = path.clone();
    let name: SharedString = g.name.clone().into();
    let w_toggle = weak.clone();
    let w_menu = weak.clone();
    let w_new = weak.clone();
    let w_tip_on = weak.clone();
    let w_tip_off = weak.clone();

    div()
        .id(SharedString::from(format!("prj-{pi}")))
        .h(px(33.))
        .mb(px(2.))
        .flex()
        .items_center()
        .gap(px(8.))
        .pl(px(10.))
        .pr(px(10.))
        .rounded(px(8.))
        .cursor_pointer()
        // v55：项目行悬停不给背景色（避免与会话行选中态混淆）
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = w_toggle.update(cx, |c, cx| {
                let key = crate::services::workspace::same_ws_key(&path_toggle.to_string_lossy());
                if !c.collapsed_keys.remove(&key) {
                    c.collapsed_keys.insert(key);
                }
                c.persist_ui();
                cx.notify();
            });
        })
        .on_hover(move |h, _, cx| {
            let _ = w_tip_on.update(cx, |c, cx| {
                let on = *h;
                if on {
                    if c.hovered_project != Some(pi) {
                        c.hovered_project = Some(pi);
                        cx.notify();
                    }
                } else {
                    if c.hovered_project == Some(pi) {
                        c.hovered_project = None;
                    }
                    if c.proj_tip.is_some() {
                        c.proj_tip = None;
                        cx.notify();
                    }
                }
            });
        })
        .on_mouse_move(move |ev: &gpui::MouseMoveEvent, _, cx| {
            let (x, y) = (f32::from(ev.position.x), f32::from(ev.position.y));
            let _ = w_tip_off.update(cx, |c, cx| {
                let path = path_tip.clone();
                let path_str = path.to_string_lossy().to_string();
                if c.proj_tip.as_ref().is_none_or(|(p, _, _)| {
                    !same_ws(&p.to_string_lossy(), &path_str)
                }) {
                    c.proj_tip = Some((path, x, y));
                    cx.notify();
                } else if let Some(p) = c.proj_tip.as_mut() {
                    p.1 = x;
                    p.2 = y;
                }
            });
        })
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
                // 项目标签 = 面板设置值（字体大小设置.md §1）
                .text_size(crate::appearance::ui_size(12.))
                .font_weight(if active {
                    gpui::FontWeight::SEMIBOLD
                } else {
                    gpui::FontWeight::MEDIUM
                })
                .text_color(if active { rgb(t.text) } else { rgb(t.text_dim) })
                .child(name),
        )
        .when(hovered, |d| {
            d.child(
                div()
                    .flex()
                    .gap(px(10.))
                    // ⋯ 项目菜单
                    .child(
                        div()
                            .id(SharedString::from(format!("prj-menu-{pi}")))
                            .size(px(28.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .text_color(rgb(t.text_dim))
                            .cursor_pointer()
                            .hover(|s| s.text_color(rgb(t.text)))
                            .on_mouse_down(
                                MouseButton::Left,
                                move |ev: &gpui::MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    let (x, y) = (f32::from(ev.position.x), f32::from(ev.position.y));
                                    let _ = w_menu.update(cx, |c, cx| {
                                        c.psp_menu = Some(crate::PspMenu::Project {
                                            path: path_menu.clone(),
                                            x: (x - 16.).max(8.),
                                            y: y + 12.,
                                        });
                                        cx.notify();
                                    });
                                },
                            )
                            .child(icon_hover("ellipsis", 18., t.text_dim)),
                    )
                    // ＋ 新会话入组
                    .child(
                        div()
                            .id(SharedString::from(format!("prj-new-{pi}")))
                            .size(px(28.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .text_color(rgb(t.text_dim))
                            .cursor_pointer()
                            .hover(|s| s.text_color(rgb(t.text)))
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                cx.stop_propagation();
                                let _ = w_new.update(cx, |c, cx| {
                                    c.new_session_in(path.clone(), cx);
                                });
                            })
                            .child(icon_hover("plus", 18., t.text_dim)),
                    ),
            )
        })
        .into_any_element()
}

/// 会话行：15px 状态槽（运行=旋转圈 / 未读=绿点 / 空）+ 标题；hover 详情卡。
fn session_row_view(
    chat: &Chat,
    p: usize,
    s: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    window: &gpui::Window,
    _cx: &gpui::Context<Chat>,
) -> gpui::AnyElement {
    let Some(g) = chat.projects.get(p) else {
        return div().into_any_element();
    };
    let Some(info) = g.sessions.get(s) else {
        return div().into_any_element();
    };
    let is_active = chat
        .active_file
        .as_deref()
        .is_some_and(|f| crate::services::workspace::same_path(f, &info.path));
    let running = chat.running_files.contains(&info.path);
    let title: SharedString = info
        .name
        .clone()
        .filter(|n| !n.trim().is_empty())
        .map(SharedString::from)
        .unwrap_or_else(|| {
            if info.preview.is_empty() {
                "(empty)".into()
            } else {
                info.preview.clone().into()
            }
        });
    let path = info.path.clone();
    let path_click = path.clone();
    let path_card = path.clone();
    let path_bounds = path.clone();
    let w_click = weak.clone();
    let w_hover = weak.clone();
    let w_move = weak.clone();
    let w_bounds = weak.clone();

    // ---- ZCode TaskListItem 样式 parity（源码实测值）----
    // 列表 p-3 留白 + 行 pl-2.5/pr-1/py-1 + space-y-0.5 行距 + rounded-lg；
    // 选中/悬停 = 中性前景薄纱（10% / 5%，非 accent——accent 底会与毛玻璃
    // overlay 互相染色）；标题溢出 = 尾部 24px 渐隐，**量宽确认溢出才挂**
    // （短标题不糊尾）。overlay 尾色 = 行底合成视觉色（不透明）：透明端必
    // 须同色 a=0，透明黑会在 HSL 插值中段产生暗带。
    let row_sel = gpui::rgba((t.text << 8) | 0x1a); // 10%（悬停与选中同色）
    let row_base = if is_active {
        gpui::rgb(crate::theme::mix_rgb(t.text, t.nav, 0.1))
    } else {
        gpui::rgb(t.nav)
    };
    let fade_to = gpui::Hsla::from(row_base);
    let fade = gpui::linear_gradient(
        90.,
        gpui::linear_color_stop(gpui::Hsla { a: 0., ..fade_to }, 0.),
        gpui::linear_color_stop(fade_to, 1.),
    );
    // 悬停态行底变成 10% 白纱（over nav），渐隐尾色必须跟着换白纱的合成
    // 视觉色——否则纯 nav 的 overlay 盖在白纱底上就是一块深色（黑块回归）
    let fade_hover_to = gpui::Hsla::from(gpui::rgb(crate::theme::mix_rgb(t.text, t.nav, 0.1)));
    let fade_hover = gpui::linear_gradient(
        90.,
        gpui::linear_color_stop(gpui::Hsla { a: 0., ..fade_hover_to }, 0.),
        gpui::linear_color_stop(fade_hover_to, 1.),
    );

    // 溢出测量：标题真实排版宽 vs 行内可用宽（列表 px 24 + 行 pl/pr 14 +
    // slot 15 + 两个 gap 16 + 时间列 38）
    let panel_family = crate::appearance::panel_font().family;
    let measure_font = gpui::Font {
        family: panel_family.clone().into(),
        features: gpui::FontFeatures::default(),
        fallbacks: None,
        weight: gpui::FontWeight::NORMAL,
        style: gpui::FontStyle::Normal,
    };
    let measure_run = gpui::TextRun {
        len: title.len(),
        font: measure_font,
        color: rgb(t.text).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let title_w = window
        .text_system()
        .layout_line(&title, px(13.), &[measure_run], None)
        .width;
    let available = px(chat.slp_w - 24. - 14. - 15. - 16. - 38.);
    let title_overflows = title_w > available;

    // 行顶对齐钩子：详情卡的 top = 本行 bounds.origin.y（ZCode 用
    // getBoundingClientRect；gpui 等价物 = wrapper 的 children_prepainted）。
    // 自下而上进入行时鼠标首事件落在行底，只靠 mouse move 会偏下——这里
    // 用真实 bounds 校正；bounds 稳定后不再 notify。滚动时自动跟随。
    let weak_for_row_el = weak.clone();
    let _ = weak_for_row_el;
    div()
        .on_children_prepainted(move |children: Vec<gpui::Bounds<gpui::Pixels>>, _, cx| {
            if let Some(b) = children.first() {
                let top = b.origin.y;
                let _ = w_bounds.update(cx, |c, cx| {
                    if let Some(card) = c.hover_card.as_mut() {
                        if crate::services::workspace::same_path(&card.path, &path_bounds)
                            && (card.y - f32::from(top)).abs() > 0.5
                        {
                            card.y = f32::from(top);
                            cx.notify();
                        }
                    }
                });
            }
        })
        .child(
    div()
        .id(SharedString::from(format!("ps-{}", info.id)))
        .relative()
        .group("psrow")
        .w_full()
        .h(px(32.))
        .mb(px(2.))
        .flex()
        .items_center()
        .gap(px(8.))
        .pl(px(10.))
        .pr(px(10.))
        .rounded(px(8.))
        .cursor_pointer()
        .when(is_active, |d| d.bg(row_sel).hover(|s| s.bg(row_sel)))
        .when(!is_active, |d| d.hover(|s| s.bg(row_sel)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = w_click.update(cx, |c, cx| {
                c.open_session(path_click.clone(), false, cx);
            });
        })
        .on_hover(move |h, _w, cx| {
            let leaving = !*h;
            let _ = w_hover.update(cx, |c, _cx| {
                if leaving {
                    if let Some(card) = c.hover_card.as_mut() {
                        // 鼠标若已进卡（card_hovered），行退出事件晚到
                        // 也不得启动消失宽限
                        if !card.card_hovered {
                            card.hide_at = Some(std::time::Instant::now());
                        }
                    }
                }
            });
        })
        .on_mouse_move(move |ev: &gpui::MouseMoveEvent, _, cx| {
            let y = f32::from(ev.position.y);
            let _ = w_move.update(cx, |c, cx| {
                let same = c
                    .hover_card
                    .as_ref()
                    .is_some_and(|card| crate::services::workspace::same_path(&card.path, &path_card));
                if !same {
                    c.hover_card = Some(crate::HoverCard {
                        path: path_card.clone(),
                        // 临时估计（鼠标 y - 半行高）；prepaint 钩子会把
                        // y 校正为本行的真实顶部（bounds.origin.y）
                        y: y - 16.,
                        hide_at: None,
                        show_at: std::time::Instant::now(),
                        shown: false,
                        confirming: false,
                        card_hovered: false,
                        renaming: false,
                        rename_input: None,
                    });
                    cx.notify();
                } else if let Some(card) = c.hover_card.as_mut() {
                    // 鼠标在本行内移动：只要不在卡上就持续取消消失宽限
                    if !card.card_hovered {
                        card.hide_at = None;
                    }
                }
            });
        })
        .child(
            div()
                .size(px(15.))
                .flex()
                .items_center()
                .justify_center()
                .child(if running {
                    spinner(9., t.accent)
                } else if chat.unread.contains(&info.path) {
                    div()
                        .size(px(7.))
                        .rounded_full()
                        .bg(rgb(crate::theme::UNREAD))
                        .into_any_element()
                } else {
                    div().into_any_element()
                }),
        )
        .child(
            // 标题：flex_1 到时间区左缘为止（不进入时间区）；仅确认溢出时
            // 挂尾部 24px 渐隐（ZCode TaskTitleOverflowText parity）
            div()
                .relative()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .font_family(panel_family)
                // 会话标签 = 面板设置值（字体大小设置.md §1）
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text))
                .child(title)
                .when(title_overflows, |d| {
                    d.child(
                        div()
                            .absolute()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .w(px(24.))
                            .bg(fade)
                            .group_hover("psrow", |s| s.bg(fade_hover)),
                    )
                }),
        )
        // 时间区（v55 fmtAgo）：固定宽右对齐成一列（宽度与左侧图标区+
        // padding 视觉平衡，标题可用空间最大化）
        .child(
            div()
                .flex_shrink_0()
                .w(px(38.))
                .flex()
                .items_center()
                .justify_end()
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_faint))
                .child(SharedString::from(crate::services::format::fmt_ago(
                    info.modified,
                ))),
        ),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// dock container (015 v54): nav 底色 + 三视图 + 右缘 resizer
// ---------------------------------------------------------------------------

pub(crate) fn dock(
    chat: &mut Chat,
    entity: Entity<Chat>,
    weak: &gpui::WeakEntity<Chat>,
    window: &mut gpui::Window,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let view: gpui::AnyElement = match chat.dock_panel {
        DockPanel::Sessions => psp_view(chat, entity, weak.clone(), window, cx),
        DockPanel::Files => files_view(chat, weak, cx).into_any_element(),
        DockPanel::Git => git_panel::view(chat, weak, cx).into_any_element(),
    };
    div()
        .id("dock")
        .flex_1()
        .min_h_0()
        .relative()
        .flex()
        .flex_col()
        .bg(rgb(t.nav))
        .child(view)
        .into_any_element()
}

/// files view (v54: 文件树面板不变——Zed 式树，根项目行 + 缩进 guide +
/// 类型图标 + 选中行全宽色带；点 .md 行打开内容区预览 tab)。
pub(crate) fn files_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    _cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(rgb(t.nav))
        .px(px(6.))
        .py(px(10.))
        .child(
            div()
                .id("file-tree-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .relative()
                .children({
                    let mut rows: Vec<gpui::AnyElement> = Vec::new();
                    let git_map: std::collections::HashMap<PathBuf, crate::services::git::GitStatus> =
                        chat.git_files
                            .iter()
                            .map(|f| (f.path.clone(), f.status))
                            .collect();
                    let changed_dirs: std::collections::HashSet<PathBuf> = git_map
                        .keys()
                        .filter_map(|p| p.parent().map(|d| d.to_path_buf()))
                        .collect();
                    collect_tree_rows(
                        &chat.cwd,
                        0,
                        &chat.expanded_dirs,
                        &git_map,
                        &changed_dirs,
                        &weak,
                        t,
                        &mut rows,
                    );
                    rows
                }),
        )
        .into_any_element()
}
