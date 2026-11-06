//! PF 插件市场——页码条（pi 页 controls.rs 同构；PF 本地索引恒单页，恒禁用）。

use gpui::prelude::*;
use gpui::{AnyElement, IntoElement, SharedString, px, rgb};

use super::{MONO, accent_alpha, line, muted};
use crate::appearance::theme::Theme;
use crate::i18n::tr;
use crate::market::pi_native_market::design::{px18, sp};
use crate::market::shell::{PageBtn, page_window};

pub(super) fn pager(page: usize, total_pages: usize, t: &'static Theme) -> AnyElement {
    let mut pages = gpui::div().flex().items_center().gap(px(6.3)).flex_wrap();
    for b in page_window(page, total_pages) {
        match b {
            PageBtn::Gap => {
                pages = pages.child(
                    gpui::div()
                        .min_w(px(sp::PAGER_CELL))
                        .h(px(sp::PAGER_CELL))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(muted(t))
                        .font_family(MONO)
                        .text_size(px(px18::PAGER_FS))
                        .child(SharedString::from("…")),
                )
            }
            PageBtn::Num(p) => {
                let active = p == page;
                let mut cell = gpui::div()
                    .id(SharedString::from(format!("pf-market-page-{p}")))
                    .min_w(px(sp::PAGER_CELL))
                    .h(px(sp::PAGER_CELL))
                    .flex()
                    .items_center()
                    .justify_center()
                    .px(px(sp::PAGER_PAD_X))
                    .border_1()
                    .font_family(MONO)
                    .text_size(px(px18::PAGER_FS))
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(p.to_string()));
                cell = if active {
                    // 当前页格子：底色与卡片一致（t.bg），不铺 hover 洗底
                    cell.border_color(accent_alpha(t, 0.42))
                        .bg(rgb(t.bg))
                        .text_color(rgb(t.text))
                } else {
                    cell.border_color(line(t))
                };
                pages = pages.child(cell);
            }
        }
    }

    let prev = nav_button("pf-market-prev", tr("market.prev"), false, t);
    let next = nav_button("pf-market-next", tr("market.next"), false, t);

    gpui::div()
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(sp::PAGER_GAP))
        .pt(px(sp::PAGER_PT))
        .border_t_1()
        .border_color(line(t))
        .child(prev)
        .child(pages)
        .child(next)
        .into_any_element()
}

/// ← / → 大格（单页恒禁用）。
fn nav_button(
    id: &'static str,
    text: &'static str,
    enabled: bool,
    t: &'static Theme,
) -> AnyElement {
    let base = gpui::div()
        .id(id)
        .min_w(px(sp::PAGER_CELL_MIN_W))
        .h(px(sp::PAGER_CELL))
        .flex()
        .items_center()
        .justify_center()
        .px(px(sp::PAGER_PAD_X))
        .border_1()
        .border_color(line(t))
        .font_family(MONO)
        .text_size(px(px18::PAGER_FS))
        .child(text);
    if enabled {
        base.text_color(rgb(t.text_dim)).into_any_element()
    } else {
        base.text_color(gpui::rgba(crate::appearance::theme::rgba_a(
            t.text_faint,
            0.55,
        )))
        .into_any_element()
    }
}
