//! Pi 扩展市场——翻页条（官方式：← 上一页 / 页码窗口 / 下一页 →）。

use gpui::prelude::*;

use gpui::{AnyElement, IntoElement, MouseButton, SharedString, Styled};

use super::{Chat, PAGE_LEFT, PAGE_RIGHT, PAGER_H};
use crate::appearance::theme::Theme;
use crate::i18n::tr;
use crate::ui::tokens::{radius, space};

// ---------------------------------------------------------------------------
// 翻页条（官方式：← 上一页 / 页码窗口 / 下一页 →）
// ---------------------------------------------------------------------------

use crate::market::shell::{PageBtn, page_window};

pub(super) fn pager(
    weak: &gpui::WeakEntity<Chat>,
    page: usize,
    total_pages: usize,
    t: &'static Theme,
) -> AnyElement {
    let mut bar = gpui::div()
        .flex_shrink_0()
        .h(gpui::px(PAGER_H))
        .flex()
        .items_center()
        .gap(gpui::px(space::SP6))
        .pl(gpui::px(PAGE_LEFT))
        .pr(gpui::px(PAGE_RIGHT))
        .border_t_1()
        .border_color(gpui::rgb(t.border));

    // ← 上一页
    //
    // 不因 loading 禁用：一页要拉 50 个包的清单（约 7 秒），若禁用就是
    // “翻到第 2 页后按钮全灰、等不到”。上一轮的回包由 `gen_ix` 作废，
    // 不需要用禁用按钮来防重入。
    let prev_ok = page > 1;
    let weak_prev = weak.clone();
    bar = bar.child(pager_nav(
        "pi-ext-prev",
        tr("market.prev"),
        prev_ok,
        t,
        move |cx| {
            let _ = weak_prev.update(cx, |c, cx| {
                let (p, _) = c.aux.market.pi_ext.pager();
                if p > 1 {
                    c.pi_ext_goto(p - 1, cx);
                }
            });
        },
    ));

    // 页码（居中）
    let mut pages = gpui::div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .gap(gpui::px(space::SP4));
    for btn in page_window(page, total_pages.max(1)) {
        match btn {
            PageBtn::Gap => {
                pages = pages.child(
                    gpui::div()
                        .px(gpui::px(space::SP1))
                        .text_size(crate::appearance::ui_size(12.))
                        .text_color(gpui::rgb(t.text_faint))
                        .child("…"),
                );
            }
            PageBtn::Num(n) => {
                let active = n == page;
                let weak_n = weak.clone();
                pages = pages.child(
                    gpui::div()
                        .id(SharedString::from(format!("pi-ext-page-{n}")))
                        .w(gpui::px(crate::ui::tokens::btn::MD_H))
                        .h(gpui::px(crate::ui::tokens::btn::MD_H))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(gpui::px(radius::CTRL))
                        .border_1()
                        .when(active, |d| {
                            d.border_color(gpui::rgb(t.accent))
                                .bg(gpui::rgba(crate::appearance::theme::rgba_a(t.accent, 0.10)))
                                .text_color(gpui::rgb(t.accent))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                        })
                        .when(!active, |d| {
                            d.border_color(gpui::rgb(t.border))
                                .text_color(gpui::rgb(t.text_muted))
                        })
                        .text_size(crate::appearance::ui_size(12.))
                        .cursor_pointer()
                        .hover(|s| s.bg(gpui::rgb(t.bg_hover)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            cx.stop_propagation();
                            let _ = weak_n.update(cx, |c, cx| c.pi_ext_goto(n, cx));
                        })
                        .child(SharedString::from(n.to_string())),
                );
            }
        }
    }
    bar = bar.child(pages);

    // 下一页 →
    let next_ok = page < total_pages;
    let weak_next = weak.clone();
    bar = bar.child(pager_nav(
        "pi-ext-next",
        tr("market.next"),
        next_ok,
        t,
        move |cx| {
            let _ = weak_next.update(cx, |c, cx| {
                let (p, tp) = c.aux.market.pi_ext.pager();
                if p < tp {
                    c.pi_ext_goto(p + 1, cx);
                }
            });
        },
    ));
    bar.into_any_element()
}

fn pager_nav(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    t: &'static Theme,
    on_click: impl Fn(&mut gpui::App) + 'static,
) -> AnyElement {
    // 标签**永远绘制**（不能按 = 置灰，不消失）：v19 曾把 child(label) 只挂
    // 在 enabled 分支，禁用时渲染成空盒（用户截图的「幽灵按钮」）
    let mut el = gpui::div()
        .id(id)
        .h(gpui::px(crate::ui::tokens::btn::MD_H))
        .px(gpui::px(crate::ui::tokens::btn::MD_PX))
        .flex()
        .items_center()
        .rounded(gpui::px(radius::CTRL))
        .border_1()
        .border_color(gpui::rgb(t.border))
        .text_size(crate::appearance::ui_size(12.))
        .text_color(gpui::rgb(t.text_muted))
        .child(label);
    if !enabled {
        return el.opacity(0.38).into_any_element();
    }
    el = el
        .cursor_pointer()
        .hover(|s| s.bg(gpui::rgb(t.bg_hover)).text_color(gpui::rgb(t.text)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            on_click(cx);
        });
    el.into_any_element()
}
