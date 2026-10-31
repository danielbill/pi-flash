//! 文件树操作栏（021/022）：git changes 模式的 action_bar（stage/unstage/
//! discard/commit）+ bar_btn 钮。三视图与行渲染在 [`super::file_tree`]。
//! 自 file_tree.rs 拆出（090 重构，延续 file_tree_* 命名风格）。

use super::*;

pub(crate) fn action_bar(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    let project: SharedString = chat
        .project()
        .map(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| p.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| tr("statusbar.no_project").to_string())
        .into();
    let changes_only = chat.ws.changes_only;
    let proj_open = chat.dlg.tree_proj_menu.is_some();
    let pin_open = chat.ws.pin_view;
    let title_bounds = chat.layout.ft_title_bounds.clone();
    let w_proj = weak.clone();
    let w_toggle = weak.clone();
    let w_pin = weak.clone();
    let w_search = weak.clone();
    let w_term = weak.clone();
    let w_menu = weak.clone();
    let w_max = weak.clone();
    let cwd_menu = chat.ws.cwd.clone();
    let cwd_explorer = chat.ws.cwd.clone();
    let cwd_term = chat.ws.cwd.clone();
    // 021 二改定稿：操作栏即根行——项目名居左（020-1：主题色粗体）、
    // 操作钮右对齐（钮盒 26/间距 2，与会话控制栏右二列对齐，各带 tooltip）；
    // 名字区右键 = 根目录菜单（树体不再有根行，根下的新建/改名等入口保留下）
    div()
        .id("ft-action-bar")
        // 自动隐藏的显影组：鼠标在本操作栏内 → 五颗功能 icon 显影
        // （group_hover 的父命中区覆盖子钮，见 bar_btn）
        .group("ft-bar-reveal")
        .w_full()
        .h(px(crate::ui::tokens::panel_ctrl::BAR_H))
        .flex_shrink_0()
        .flex()
        .items_center()
        .pl(px(crate::ui::tokens::align::ICON_LINE))
        .pr(px(crate::ui::tokens::align::LIST_PX))
        .gap(px(2.))
        .child(
            div()
                .id("ft-project")
                .relative()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                // gpui div 默认 block（Style::default 显式 Display::Block），
                // 名字与 chevron 会竖着堆——标题行显式横排
                .flex()
                .items_center()
                .text_size(crate::appearance::ui_size(12.))
                // 020-1：当前项目名 = 主题色 + 粗体（与项目列表选中行一致）
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(t.accent))
                .cursor_pointer()
                .with_tip(tr("file_tree.switch_project"))
                // 项目下拉（树根跨项目快切）：左键开/收，浮层贴标题下缘
                // （锚点 = 下方 canvas 捕获的 paint 期 bounds）
                .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                    let _ = w_proj.update(cx, |c, cx| c.toggle_tree_proj_menu(cx));
                })
                // 名字自适应内容宽（flex-grow 0：chevron 紧贴名字，不吃剩
                // 余宽）；长名走 flex-shrink + min_w_0 收缩 ellipsis
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(project),
                )
                // 开合指示：下拉开着时翻向上
                .child(div().flex_shrink_0().ml(px(4.)).child(crate::ui::icon(
                    if proj_open {
                        "chevron-up"
                    } else {
                        "chevron-down"
                    },
                    12.,
                    t.accent,
                )))
                // 标题 paint 期 bounds 捕获（dock_bounds 同款 canvas，无交互
                // 不挡命中）：项目下拉的锚点
                .child(
                    gpui::canvas(
                        move |bounds, _, _| *title_bounds.borrow_mut() = Some(bounds),
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
                .on_mouse_down(
                    gpui::MouseButton::Right,
                    move |ev: &gpui::MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let Some(cwd_menu) = cwd_menu.clone() else {
                            return;
                        };
                        let _ = w_menu.update(cx, |c, cx| {
                            c.dlg.tree_menu = Some(crate::function_panel::TreeMenu::Dir {
                                path: cwd_menu,
                                x: f32::from(ev.position.x),
                                y: f32::from(ev.position.y),
                            });
                            cx.notify();
                        });
                    },
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                // 020-1 列对齐：钮盒 26、间距 2，与会话控制栏右二同指标——
                // 两排右缘图标逐列对齐（最大化列/次列同 x）
                .gap(px(2.))
                .child(bar_btn(
                    "ft-pins",
                    // 视图切换钮（021 三改）：树视图 = pin icon（进入收藏
                    // 视图）；Pin 视图 = 目录树 icon（点击回树）
                    if pin_open { "folder-tree" } else { "pin" },
                    if pin_open {
                        tr("file_tree.back_to_tree")
                    } else {
                        tr("file_tree.pin_favorites")
                    },
                    t,
                    pin_open,
                    true,
                    None,
                    move |_, _, cx| {
                        let _ = w_pin.update(cx, |c, cx| c.toggle_pin_view(cx));
                    },
                ))
                .child(bar_btn(
                    "ft-toggle-changes",
                    // 同上：树视图 = 变更 icon；变更视图 = 目录树 icon
                    if changes_only {
                        "folder-tree"
                    } else {
                        "git-commit-horizontal"
                    },
                    if changes_only {
                        tr("file_tree.back_to_tree")
                    } else {
                        tr("file_tree.show_changes_only")
                    },
                    t,
                    changes_only,
                    true,
                    None,
                    move |_, _, cx| {
                        let _ = w_toggle.update(cx, |c, cx| c.toggle_changes_only(cx));
                    },
                ))
                .child(bar_btn(
                    "ft-search",
                    "search",
                    tr("file_tree.search"),
                    t,
                    false,
                    true,
                    // tooltip 附键位 chip（OpenFileSearch = ctrl-l，082 同款）
                    Some(&crate::OpenFileSearch),
                    move |_, _, cx| {
                        let _ = w_search.update(cx, |c, cx| c.open_file_search(cx));
                    },
                ))
                .child(bar_btn(
                    "ft-explorer",
                    "folder-input",
                    tr("file_tree.reveal_in_explorer"),
                    t,
                    false,
                    true,
                    None,
                    move |_, _, _cx| {
                        // 无项目态：没有可展示的目录，按钮 no-op
                        if let Some(cwd) = &cwd_explorer {
                            crate::services::shell_open::open_in_explorer(cwd);
                        }
                    },
                ))
                .child(bar_btn(
                    "ft-term",
                    "icon-terminal-solid",
                    tr("file_tree.open_in_terminal"),
                    t,
                    false,
                    true,
                    None,
                    move |_, window, cx| {
                        // 无项目态：没有可打开终端的目录，按钮 no-op
                        let Some(cwd) = cwd_term.clone() else { return };
                        let _ = w_term.update(cx, |c, cx| c.open_terminal_in(cwd, window, cx));
                    },
                ))
                .child(bar_btn(
                    "ft-panel-max",
                    "maximize",
                    tr("file_tree.maximize"),
                    t,
                    false,
                    // 最大化与五颗低频钮同入显影组（自动隐藏，见 action_bar 头注）
                    true,
                    Some(&crate::MaximizeFileTree),
                    move |_, _, cx| {
                        let _ = w_max.update(cx, |c, cx| {
                            // 020-1 单最大化钮：本面板填满，会话列表收起到控制栏
                            c.layout.sess_min = true;
                            c.layout.files_min = false;
                            // 最大化态立即落盘（用户报障：最大化后关闭重启丢失）
                            c.persist_ui();
                            cx.notify();
                        });
                    },
                )),
        )
        .into_any_element()
}

/// 操作栏图标钮：26px 命中槽 + 16px 图标（icon_hover 悬停变亮；激活态
/// 用 accent 实色不参与 hover 变亮——开关点亮语义）。`tip` = hover
/// tooltip（ui::tooltip 统一入口 §5.10，显示期过 tr 随语言切换）；
/// `action` 非空时 tooltip 附键位 chip（键位反查 keymap，082 最大化钮）。
/// `reveal` = 自动隐藏（常态 opacity 0，操作栏 hover 显影；见 action_bar 头注）。
fn bar_btn(
    id: &'static str,
    icon_name: &'static str,
    tip: &'static str,
    t: &'static crate::appearance::theme::Theme,
    active: bool,
    reveal: bool,
    action: Option<&'static dyn gpui::Action>,
    on_click: impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> gpui::AnyElement {
    let btn = div()
        .id(id)
        .w(px(26.))
        .h(px(26.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)));
    // 自动隐藏：opacity 只管绘制，命中区/布局原样——显隐之间按钮不挪位，
    // 悬停灭掉一个 icon 不会让同排钮重排。tooltip 照旧（图标透明也能悬到）。
    // 设置-界面「部分操作隐藏」关掉后恒显（开关默认 on）
    let btn = if reveal && crate::services::workspace::autohide_icons() {
        btn.opacity(0.)
            .group_hover("ft-bar-reveal", |s| s.opacity(1.))
    } else {
        btn
    };
    let btn = match action {
        Some(a) => btn.with_tip_action(tip, a),
        None => btn.with_tip(tip),
    };
    btn.on_mouse_down(gpui::MouseButton::Left, on_click)
        .child(if active {
            crate::ui::icon(icon_name, 16., t.accent).into_any_element()
        } else {
            crate::ui::icon_hover(icon_name, 16., t.text_dim).into_any_element()
        })
        .into_any_element()
}
