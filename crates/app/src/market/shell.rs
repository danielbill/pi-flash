//! 市场页共用分段外壳（090-4 共享约定的代码落点）。
//!
//! 三个市场页（pi_ext_market / pi_native_market / pf_market）的页面骨架
//! 同构：定宽段盒子 + 显式左右留白居中 + 「卡片头 / 卡行 / 页码条」分段。
//! 这里收拢**结构与几何**（几何真值仍只落
//! [`crate::market::pi_native_market::design`]，本文件不自造尺寸），
//! **色板由调用方注入**——pi 页传 `design::` 字面量皮肤，PF 页传主题
//! 语义 token（090-4：两页只共享 layout/paging/card/按钮等结构，色板
//! 各归各家）。
//!
//! 各页在自己文件里保留一层的签名包装（`slice_shell(m, top, bottom, t)`
//! 这类），调用点与外部导入路径（如 `pf_market::slice_shell`）零改动。
//!
//! [`page_window`] 是三处逐字符复制的页码窗口纯函数（连单测一起）的唯一
//! 落点。

use gpui::{AnyElement, IntoElement, ParentElement, Styled, px};

use crate::market::pi_native_market::design::{Metrics, sp};

// ---------------------------------------------------------------------------
// 分段外壳
// ---------------------------------------------------------------------------

/// 居中外壳：定宽段盒子显式左右留白（列表项里 `mx_auto` 不可靠，三页同坑）。
/// 收 `impl IntoElement`：`Stateful<Div>`（带 id 的滚动列）也能直接包。
pub(crate) fn centered(inner: impl IntoElement, m: &Metrics) -> gpui::Div {
    gpui::div()
        .pl(px(m.side_pad()))
        .pr(px(m.side_pad()))
        .child(inner)
}

/// 段外壳：两侧描边 + 面板底；`top/bottom` = 首/末段补边。
/// `bg`/`line` 由调用方注入（pi 页 = design 字面量，PF 页 = 主题 token）。
pub(crate) fn slice_shell(m: &Metrics, top: bool, bottom: bool, bg: gpui::Rgba, line: gpui::Rgba) -> gpui::Div {
    let mut boxed = gpui::div()
        // 定宽（`w_full()` 在变高列表项里拿不到父宽，会整段塌掉——pi 页同坑）
        .w(px(m.shell_inner_w()))
        .flex()
        .flex_col()
        .bg(bg)
        .border_l_1()
        .border_r_1()
        .border_color(line);
    if top {
        boxed = boxed.border_t_1();
    }
    if bottom {
        boxed = boxed.border_b_1();
    }
    boxed
}

/// 卡行：左右两道内距 + 行间距；`top_pad` = 行顶补一份体距（pi 页由工具栏
/// 容器给 `pt`，PF 页没有工具栏，这一份由卡行自己带）。
pub(crate) fn row_shell(m: &Metrics, top_pad: bool) -> gpui::Div {
    let mut row = gpui::div()
        .w_full()
        .flex()
        .gap(px(sp::BLOCK_GAP))
        .px(px(m.card_pad_body));
    if top_pad {
        row = row.pt(px(m.card_pad_body));
    }
    row.pb(px(sp::BLOCK_GAP))
}

/// 卡片头容器（标题件 + 可选右端件）。pi 页 `.content-card-header` 同构；
/// 标题元素由调用方构造（pi 页 = mono 大写标签，PF 页 = 页签行）。
pub(crate) fn card_header(
    title: AnyElement,
    right: Option<AnyElement>,
    m: &Metrics,
    line: gpui::Rgba,
) -> AnyElement {
    gpui::div()
        .flex()
        .items_baseline()
        .justify_between()
        .gap(px(sp::BLOCK_GAP))
        .px(px(m.card_pad_head))
        .py(px(sp::HEADER_PAD_Y))
        .border_b_1()
        .border_color(line)
        .child(title)
        .children(right)
        .into_any_element()
}

/// 页码段：末段容器（`top` = 是否补顶边；`outer_pb` = 外壳底留白，pi 页带
/// 4rem、PF 页贴面板底为 0）。内层内距固定：px(体距) / pt(BODY_GAP) / pb(体距)。
#[allow(clippy::too_many_arguments)]
pub(crate) fn pager_block(
    m: &Metrics,
    pager: AnyElement,
    top: bool,
    outer_pb: f32,
    bg: gpui::Rgba,
    line: gpui::Rgba,
) -> AnyElement {
    centered(
        slice_shell(m, top, true, bg, line).child(
            gpui::div()
                .px(px(m.card_pad_body))
                .pt(px(sp::BODY_GAP))
                .pb(px(m.card_pad_body))
                .child(pager),
        ),
        m,
    )
    .pb(px(outer_pb))
    .into_any_element()
}

// ---------------------------------------------------------------------------
// 页码窗口（三处复制的纯函数的唯一落点）
// ---------------------------------------------------------------------------

/// 页码格子：数字页 / 省略号。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PageBtn {
    Num(usize),
    Gap,
}

/// 页码窗口（纯函数，单测友好）：≤7 页全显示；否则 首/末 + 当前前后各 1，
/// 缺口用省略号。
pub(crate) fn page_window(page: usize, total: usize) -> Vec<PageBtn> {
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
        let w = page_window(1, 20);
        assert_eq!(w[0], PageBtn::Num(1));
        assert_eq!(w[1], PageBtn::Num(2));
        assert_eq!(w[2], PageBtn::Gap);
        assert_eq!(w.last(), Some(&PageBtn::Num(20)));
    }

    #[test]
    fn window_middle_page() {
        assert_eq!(
            page_window(10, 20),
            vec![
                PageBtn::Num(1),
                PageBtn::Gap,
                PageBtn::Num(9),
                PageBtn::Num(10),
                PageBtn::Num(11),
                PageBtn::Gap,
                PageBtn::Num(20)
            ]
        );
    }

    #[test]
    fn window_end_page() {
        assert_eq!(
            page_window(20, 20),
            vec![
                PageBtn::Num(1),
                PageBtn::Gap,
                PageBtn::Num(19),
                PageBtn::Num(20)
            ]
        );
    }
}
