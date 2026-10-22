//! 统一弹出定位算法（2026-10-11 用户定夺）——toast 与会话栏（psp）全套
//! 浮层共用的唯一放置逻辑。
//!
//! 口径：**弹层朝屏幕中心一侧展开**——锚点在视口上半 → 向下弹，反之向上
//! 弹；锚点在左半 → 向右弹，反之向左弹。两轴独立判定，可组合成四象限。
//!
//! **职责边界（2026-10-11 二次定夺）**：本算法只裁决**弹出方向**，不负责
//! 精准定位——贴行顶/行底这类对齐语义由调用方自行实现（如会话 hover
//! 详情卡：向下弹卡顶=行顶、向上弹卡底=行底）。
//!
//! 实现取巧：返回「贴哪条边」而非坐标——向下弹给 `top`、向上弹给
//! `bottom`（相对视口），调用方原样喂给 `.left()/.right()/.top()/.bottom()`。
//! 弹层自身多高多宽无关紧要：bottom/right 锚定天然向上/向左生长，不需要
//! 事先量内容尺寸（pi_extension_picker 同款手法，收拢到此）。

use gpui::{Pixels, Styled, px};

/// 一组边锚定值：left/right 必有其一为 Some，top/bottom 同理。
#[derive(Debug, Clone, Copy)]
pub(crate) struct PopupEdges {
    pub(crate) left: Option<Pixels>,
    pub(crate) right: Option<Pixels>,
    pub(crate) top: Option<Pixels>,
    pub(crate) bottom: Option<Pixels>,
}

impl PopupEdges {
    /// anchor = 视口坐标锚点（弹出方向的起点，如点击点/按钮角/固定槽位）；
    /// gap_x/gap_y = 沿弹出方向离锚点的间距（向上/向左弹时同样为正：再让
    /// 出 gap）。锚点恰好在视口正中（ax==w/2 或 ay==h/2）按「反之」处理。
    pub(crate) fn around(anchor: (f32, f32), vp: (f32, f32), gap_x: f32, gap_y: f32) -> Self {
        let (ax, ay) = (px(anchor.0), px(anchor.1));
        let (w, h) = (px(vp.0), px(vp.1));
        let pop_down = ay < h / 2.;
        let pop_right = ax < w / 2.;
        Self {
            left: pop_right.then(|| ax + px(gap_x)),
            right: (!pop_right).then(|| w - ax + px(gap_x)),
            top: pop_down.then(|| ay + px(gap_y)),
            bottom: (!pop_down).then(|| h - ay + px(gap_y)),
        }
    }

    /// 只取纵向判定：上半 → `top = y + gap`，下半 → `bottom = h - y + gap`。
    /// status toast 的水平居中是 2026-10-10 定夺（left_0+right_0+justify_center），
    /// 不走横向象限规则，只从本算法取纵向边。
    pub(crate) fn vertical(anchor_y: f32, vp_h: f32, gap_y: f32) -> Self {
        Self::around((0., anchor_y), (f32::MAX, vp_h), 0., gap_y)
    }

    /// 喂给 gpui 元素：按 Some 的轴调 .left()/.right()/.top()/.bottom()。
    /// 泛型 `Styled` 同时覆盖 Div / Stateful<Div>（.id() 之后是后者）。
    pub(crate) fn apply<S: Styled>(self, el: S) -> S {
        let el = match (self.left, self.right) {
            (Some(l), _) => el.left(l),
            (_, Some(r)) => el.right(r),
            _ => el,
        };
        match (self.top, self.bottom) {
            (Some(t), _) => el.top(t),
            (_, Some(b)) => el.bottom(b),
            _ => el,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VP: (f32, f32) = (1440., 900.);

    #[test]
    fn upper_left_quarter_pops_down_right() {
        let e = PopupEdges::around((300., 200.), VP, 0., 12.);
        assert_eq!(e.top, Some(px(212.)));
        assert_eq!(e.bottom, None);
        assert_eq!(e.left, Some(px(300.)));
        assert_eq!(e.right, None);
    }

    #[test]
    fn lower_right_quarter_pops_up_left() {
        let e = PopupEdges::around((1200., 700.), VP, 0., 12.);
        assert_eq!(e.top, None);
        assert_eq!(e.bottom, Some(px(VP.1 - 700. + 12.)));
        assert_eq!(e.left, None);
        assert_eq!(e.right, Some(px(VP.0 - 1200.)));
    }

    #[test]
    fn cross_quadrants_independent_axes() {
        // 左下 → 向上 + 向右；右上 → 向下 + 向左
        let e = PopupEdges::around((300., 800.), VP, 0., 0.);
        assert!(e.bottom.is_some() && e.left.is_some() && e.top.is_none() && e.right.is_none());
        let e = PopupEdges::around((1100., 100.), VP, 0., 0.);
        assert!(e.top.is_some() && e.right.is_some() && e.bottom.is_none() && e.left.is_none());
    }

    #[test]
    fn exact_half_counts_as_the_else_branch() {
        // 恰好在正中按「反之」处理：y=h/2 向上弹、x=w/2 向左弹
        let e = PopupEdges::around((VP.0 / 2., VP.1 / 2.), VP, 0., 0.);
        assert!(e.bottom.is_some() && e.right.is_some());
    }

    #[test]
    fn status_toast_vertical_slot() {
        // 顶槽锚点 y=0 → 向下弹 top=44（现状等价输出）
        let e = PopupEdges::vertical(0., VP.1, 44.);
        assert_eq!(e.top, Some(px(44.)));
        assert_eq!(e.bottom, None);
    }

    #[test]
    fn ext_notice_top_right_slot() {
        // 右上槽锚点 (w, 0) + gap 12 → top 12 / right 12（与原写死值等价）
        let e = PopupEdges::around((VP.0, 0.), VP, 12., 12.);
        assert_eq!(e.top, Some(px(12.)));
        assert_eq!(e.right, Some(px(12.)));
        assert_eq!(e.bottom, None);
        assert_eq!(e.left, None);
    }

    #[test]
    fn centered_toast_gets_right_edge_at_center() {
        // 锚点 x=w/2 判「右半」→ right = w/2：右缘贴中线 = 内容宽度自适应居中
        let e = PopupEdges::around((VP.0 / 2., 0.), VP, 0., 0.);
        assert_eq!(e.right, Some(px(VP.0 / 2.)));
        assert_eq!(e.left, None);
    }
}
