//! Pi 原生扩展市场 —— 工具栏（搜索 + 类型 + 排序 + 两个按钮）与页码条（040-2）。
//!
//! 结构照设计稿 `.action-bar.packages-action-bar`（`grid-template-columns:
//! minmax(0,1fr) 12rem 13rem auto`，`<1024` 折成一列）与 `.pagination`。

use gpui::prelude::*;
use gpui::{AnyElement, IntoElement, MouseButton, SharedString, Styled};

use super::design::{self as d, Metrics, px18, sp};
use super::{BarModel, NativeKind, NativeSort, dropdown_row, label};
use crate::Chat;

/// 工具栏：三字段 + 两按钮。字段宽 12rem / 13rem（= 216 / 234）。
pub(super) fn action_bar(st: BarModel, weak: &gpui::WeakEntity<Chat>, m: &Metrics) -> AnyElement {
    // 搜索框：设计稿的 44px 框（外框由我这里出，内件 flat+form）
    let input: AnyElement = match st.input.as_ref() {
        Some(ix) => gpui::div()
            .flex()
            .items_center()
            .h(gpui::px(sp::CONTROL_H))
            .border_1()
            .border_color(d::line())
            .bg(d::panel_soft())
            .pl(gpui::px(9.))
            .pr(gpui::px(sp::CONTROL_PAD_X))
            .child(gpui::div().flex_1().min_w_0().child(ix.clone()))
            .into_any_element(),
        None => gpui::div()
            .h(gpui::px(sp::CONTROL_H))
            .border_1()
            .border_color(d::line())
            .into_any_element(),
    };

    let field1 = gpui::div()
        .min_w_0()
        .w_full()
        .flex()
        .flex_col()
        .child(input);

    let kind_col = gpui::div()
        .w(gpui::px(216.))
        .flex_col()
        .child(if st.kind_guard.is_none() {
            // 守卫只在 `native_ensure_input` 里建；找不到就画静态框（标签页
            // 恢复等边角路径宁可不给下拉，也不能 panic）
            select_box(NativeKind::label(st.kind))
        } else {
            crate::ui::dropdown(
                "pi-native-kind-dd",
                st.kind_guard.as_ref().expect("guard ensured on open"),
                st.kind_open,
                {
                    let weak = weak.clone();
                    move |_, cx| {
                        let _ = weak.update(cx, |c, cx| {
                            c.aux.market.native.kind_open = !c.aux.market.native.kind_open;
                            c.aux.market.native.sort_open = false;
                            cx.notify();
                        });
                    }
                },
                {
                    let weak = weak.clone();
                    move |_, cx| {
                        let _ = weak.update(cx, |c, cx| {
                            c.aux.market.native.kind_open = false;
                            cx.notify();
                        });
                    }
                },
                select_box(NativeKind::label(st.kind)),
                {
                    let weak = weak.clone();
                    let cur = st.kind;
                    move || {
                        let mut menu = menu_shell();
                        for k in NativeKind::ALL {
                            let w = weak.clone();
                            menu = menu.child(dropdown_row(
                                format!("pi-native-kind-{k:?}"),
                                k.label(),
                                k == cur,
                                move |cx| {
                                    let _ = w.update(cx, |c, cx| c.native_set_kind(k, cx));
                                },
                            ));
                        }
                        menu.into_any_element()
                    }
                },
            )
        });

    let sort_col = gpui::div()
        .w(gpui::px(234.))
        .flex_col()
        .child(if st.sort_guard.is_none() {
            select_box(NativeSort::label(st.sort))
        } else {
            crate::ui::dropdown(
                "pi-native-sort-dd",
                st.sort_guard.as_ref().expect("guard ensured on open"),
                st.sort_open,
                {
                    let weak = weak.clone();
                    move |_, cx| {
                        let _ = weak.update(cx, |c, cx| {
                            c.aux.market.native.sort_open = !c.aux.market.native.sort_open;
                            c.aux.market.native.kind_open = false;
                            cx.notify();
                        });
                    }
                },
                {
                    let weak = weak.clone();
                    move |_, cx| {
                        let _ = weak.update(cx, |c, cx| {
                            c.aux.market.native.sort_open = false;
                            cx.notify();
                        });
                    }
                },
                select_box(NativeSort::label(st.sort)),
                {
                    let weak = weak.clone();
                    let cur = st.sort;
                    move || {
                        let mut menu = menu_shell();
                        for s in NativeSort::ALL {
                            let w = weak.clone();
                            menu = menu.child(dropdown_row(
                                format!("pi-native-sort-{s:?}"),
                                s.label(),
                                s == cur,
                                move |cx| {
                                    let _ = w.update(cx, |c, cx| c.native_set_sort(s, cx));
                                },
                            ));
                        }
                        menu.into_any_element()
                    }
                },
            )
        });

    // 按钮组：Search（提交）/ Reset（清空）
    let buttons = gpui::div()
        .flex()
        .items_end()
        .gap(gpui::px(9.9))
        .child(bracket_button(
            "pi-native-search",
            label(&d::spaced("SEARCH", false)),
            {
                let weak = weak.clone();
                move |cx| {
                    let _ = weak.update(cx, |c, cx| {
                        let v = c.aux.market.native.filter.clone();
                        c.native_search_with(v, cx);
                    });
                }
            },
        ))
        .child(bracket_button(
            "pi-native-reset",
            label(&d::spaced("RESET", false)),
            {
                let weak = weak.clone();
                move |cx| {
                    let _ = weak.update(cx, |c, cx| c.native_reset(cx));
                }
            },
        ));

    let bar = if m.stacked_bar {
        gpui::div()
            .w_full()
            .flex()
            .flex_col()
            .gap(gpui::px(sp::BAR_GAP))
            .child(field1)
            .child(kind_col)
            .child(sort_col)
            .child(buttons)
    } else {
        gpui::div()
            .w_full()
            .flex()
            .items_end()
            .gap(gpui::px(sp::BAR_GAP))
            .child(gpui::div().flex_1().min_w_0().child(field1))
            .child(kind_col)
            .child(sort_col)
            .child(buttons)
    };
    bar.into_any_element()
}

/// 下拉触发块（设计 `.select-control`：44 高 / mono 15 / 右侧箭头）。
fn select_box(text: &str) -> AnyElement {
    gpui::div()
        .relative()
        .w_full()
        .h(gpui::px(sp::CONTROL_H))
        .flex()
        .items_center()
        .pl(gpui::px(sp::CONTROL_PAD_X))
        .pr(gpui::px(sp::CONTROL_PAD_X + sp::CARET + 7.56))
        .border_1()
        .border_color(d::line())
        .bg(d::panel_soft())
        .cursor_pointer()
        .text_size(gpui::px(px18::FORM_FS))
        .font_family(d::MONO)
        .text_color(d::text())
        .child(label(text))
        .child(
            gpui::div()
                .absolute()
                .right(gpui::px(sp::CARET_INSET))
                .top_0()
                .bottom_0()
                .flex()
                .items_center()
                .child(crate::ui::icon("caret-down", sp::CARET, 0xd5d8db)),
        )
        .into_any_element()
}

/// 下拉弹层外壳（深色面板 + 1px 描边，与设计稿下拉同观感）。
fn menu_shell() -> gpui::Div {
    gpui::div()
        .min_w(gpui::px(180.))
        .flex()
        .flex_col()
        .py(gpui::px(4.))
        .border_1()
        .border_color(d::line())
        .bg(d::panel())
}

/// `[ LABEL ]` 按钮（设计 `.action-button` / `.link-button`）。
fn bracket_button(
    id: &'static str,
    text: SharedString,
    on_click: impl Fn(&mut gpui::App) + 'static,
) -> AnyElement {
    gpui::div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .h(gpui::px(sp::CONTROL_H))
        .px(gpui::px(sp::BTN_PAD_X))
        .border_1()
        .border_color(d::line())
        .bg(d::control_bg())
        .cursor_pointer()
        .text_size(gpui::px(px18::BTN_FS))
        .font_family(d::MONO)
        .text_color(gpui::rgba(crate::appearance::theme::rgba_a(0xd5d8db, 0.8)))
        .hover(|s| {
            s.border_color(d::accent_alpha(0.42))
                .bg(d::control_bg_hover())
                .text_color(d::text())
        })
        .on_mouse_down(MouseButton::Left, move |_, _, cx| on_click(cx))
        .child(
            gpui::div()
                .pr(gpui::px(4.6))
                .text_color(gpui::rgb(d::ACCENT))
                .child(SharedString::from("[")),
        )
        .child(text)
        .child(
            gpui::div()
                .pl(gpui::px(4.6))
                .text_color(gpui::rgb(d::ACCENT))
                .child(SharedString::from("]")),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// 页码条
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
enum PageBtn {
    Num(usize),
    Gap,
}

/// 页码窗口（纯函数，单测友好）：≤7 页全显示；否则 首/末 + 当前前后各 1。
fn page_window(page: usize, total: usize) -> Vec<PageBtn> {
    if total <= 7 {
        return (1..=total).map(PageBtn::Num).collect();
    }
    let mut out = vec![PageBtn::Num(1)];
    let start = page.saturating_sub(1).max(2);
    let end = (page + 1).min(total - 1);
    if start > 2 {
        out.push(PageBtn::Gap);
    }
    for p in start..=end.max(start) {
        out.push(PageBtn::Num(p));
    }
    if end < total - 1 {
        out.push(PageBtn::Gap);
    }
    out.push(PageBtn::Num(total));
    out
}

/// 页码条（设计 `.pagination`：上边框 + 左右两组）。
pub(super) fn pager(
    weak: &gpui::WeakEntity<Chat>,
    page: usize,
    total_pages: usize,
    m: &Metrics,
    loading: bool,
) -> AnyElement {
    let _ = m;
    let _ = loading;
    let mut pages = gpui::div()
        .flex()
        .items_center()
        .gap(gpui::px(6.3))
        .flex_wrap();
    for b in page_window(page, total_pages) {
        match b {
            PageBtn::Gap => {
                pages = pages.child(
                    gpui::div()
                        .min_w(gpui::px(sp::PAGER_CELL))
                        .h(gpui::px(sp::PAGER_CELL))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(d::muted())
                        .font_family(d::MONO)
                        .text_size(gpui::px(px18::PAGER_FS))
                        .child(SharedString::from("…")),
                )
            }
            PageBtn::Num(p) => {
                let w = weak.clone();
                let active = p == page;
                let mut cell = gpui::div()
                    .id(SharedString::from(format!("pi-native-page-{p}")))
                    .min_w(gpui::px(sp::PAGER_CELL))
                    .h(gpui::px(sp::PAGER_CELL))
                    .flex()
                    .items_center()
                    .justify_center()
                    .px(gpui::px(sp::PAGER_PAD_X))
                    .border_1()
                    .font_family(d::MONO)
                    .text_size(gpui::px(px18::PAGER_FS))
                    .text_color(d::muted_strong())
                    .child(SharedString::from(p.to_string()));
                cell = if active {
                    cell.border_color(d::accent_alpha(0.42))
                        .bg(d::control_bg_hover())
                        .text_color(d::text())
                } else {
                    cell.border_color(d::line())
                        .cursor_pointer()
                        .hover(|s| {
                            s.border_color(d::accent_alpha(0.42))
                                .bg(d::control_bg_hover())
                                .text_color(d::text())
                        })
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = w.update(cx, |c, cx| c.native_goto(p, cx));
                        })
                };
                pages = pages.child(cell);
            }
        }
    }

    let prev = nav_button("pi-native-prev", "←  Previous", page > 1, {
        let weak = weak.clone();
        move |cx| {
            let _ = weak.update(cx, |c, cx| {
                let p = c.aux.market.native.page;
                c.native_goto(p.saturating_sub(1).max(1), cx);
            });
        }
    });
    let next = nav_button("pi-native-next", "Next  →", page < total_pages, {
        let weak = weak.clone();
        move |cx| {
            let _ = weak.update(cx, |c, cx| {
                let p = c.aux.market.native.page + 1;
                c.native_goto(p, cx);
            });
        }
    });

    gpui::div()
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .gap(gpui::px(sp::PAGER_GAP))
        .pt(gpui::px(sp::PAGER_PT))
        .border_t_1()
        .border_color(d::line())
        .child(prev)
        .child(pages)
        .child(next)
        .into_any_element()
}

/// ← / → 大格（设计 `.pagination-link`，min-width 6.8rem）。
fn nav_button(
    id: &'static str,
    text: &'static str,
    enabled: bool,
    on_click: impl Fn(&mut gpui::App) + 'static,
) -> AnyElement {
    let base = gpui::div()
        .id(id)
        .min_w(gpui::px(sp::PAGER_CELL_MIN_W))
        .h(gpui::px(sp::PAGER_CELL))
        .flex()
        .items_center()
        .justify_center()
        .px(gpui::px(sp::PAGER_PAD_X))
        .border_1()
        .border_color(d::line())
        .font_family(d::MONO)
        .text_size(gpui::px(px18::PAGER_FS))
        .child(label(text));
    if enabled {
        base.text_color(d::muted_strong())
            .cursor_pointer()
            .hover(|s| {
                s.border_color(d::accent_alpha(0.42))
                    .bg(d::control_bg_hover())
                    .text_color(d::text())
            })
            .on_mouse_down(MouseButton::Left, move |_, _, cx| on_click(cx))
            .into_any_element()
    } else {
        base.text_color(gpui::rgba(crate::appearance::theme::rgba_a(0xc5c2c9, 0.36)))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_short_lists_fully() {
        assert_eq!(
            page_window(1, 3),
            vec![PageBtn::Num(1), PageBtn::Num(2), PageBtn::Num(3)]
        );
        assert_eq!(
            page_window(1, 7),
            (1..=7).map(PageBtn::Num).collect::<Vec<_>>()
        );
    }

    #[test]
    fn window_clamps_edges() {
        // 第 1 页：1 2 … 20（首尾各一 + 当前后一）
        let w = page_window(1, 20);
        assert_eq!(w[0], PageBtn::Num(1));
        assert_eq!(w[1], PageBtn::Num(2));
        assert_eq!(w[2], PageBtn::Gap);
        assert_eq!(w.last(), Some(&PageBtn::Num(20)));
        let w = page_window(20, 20);
        assert_eq!(w[0], PageBtn::Num(1));
        assert_eq!(w[1], PageBtn::Gap);
        assert!(w.contains(&PageBtn::Num(19)));
        assert_eq!(w.last(), Some(&PageBtn::Num(20)));
    }
}
