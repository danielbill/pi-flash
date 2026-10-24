//! §5.12 分隔线统一实现——壳内可拖拽分隔线的**唯一**视觉落点。
//!
//! 两处共用本文件的 [`line`]：内容区 ⇆ 左面板（竖，content-col 左缘）与
//! dock 上下分区（横，会话列表 ⇆ 文件树之间）。契约：常驻 1px 细线；
//! **hover 与拖拽同款 4px accent 粗线**（不是只有拖拽才粗）。
//!
//! 为什么不用 gpui 的 `group_hover`：它在 `div.rs` 的 paint 里被
//! `if !cx.has_active_drag()` 包着、且要求元素自身带 hitbox 才 refine，
//! 与「显式状态位」两条路踩不到一处，同一份契约会渲染出两种结果（实测
//! 颜色变了粗细没变）。故 hover/拖拽状态一律由调用方存成 bool 字段传进来，
//! 两处走同一条计算，行为必然一致。
//!
//! 高亮线一律 absolute overlay，不占布局：gpui 的 border 参与布局，改粗细
//! 会推挤内容（hover 抖动）；overlay 换尺寸零位移。

use gpui::{Div, Rgba, Styled, div, px};

use crate::appearance::theme::{self, Theme};
use crate::ui::tokens::divider as tk;

/// 分隔线朝向：`V` 竖直（左右分区），`H` 水平（上下分区）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Axis {
    V,
    H,
}

/// 一条分隔线线的形状参数（交互、命中带、提示归调用方）。
#[derive(Clone, Copy)]
pub(crate) struct LineSpec {
    axis: Axis,
    /// 两端缩进：`V` 为上下端缩进，`H` 为左右端缩进。
    inset: f32,
    /// `H` 专用：所在命中带高——线在带内垂直居中，保证 1px↔4px 中线不动。
    band: f32,
}

impl LineSpec {
    /// 竖直分隔线（全高贴缘，不缩进）。
    pub(crate) fn vertical() -> Self {
        Self {
            axis: Axis::V,
            inset: 0.,
            band: 0.,
        }
    }

    /// 水平分隔线：两端缩进 `inset`，命中带高 `band`。
    pub(crate) fn horizontal(inset: f32, band: f32) -> Self {
        Self {
            axis: Axis::H,
            inset,
            band,
        }
    }
}

/// 画分隔线：`active` = hover 或拖拽（调用方 `hovered || dragging`）。
///
/// 颜色由本函数按主题语义 token 统一取——常驻线 = 细线灰、高亮 = accent，
/// 调用方不各自传灰（否则三处又会各调一种灰，名义统一、实际三种）。
pub(crate) fn line(spec: LineSpec, active: bool, t: &Theme) -> Div {
    let thick = if active { tk::HOVER } else { tk::LINE };
    let color: Rgba = if active {
        gpui::rgb(t.accent)
    } else {
        gpui::rgba(theme::border_alpha(t, 0x99))
    };
    match spec.axis {
        Axis::V => div()
            .absolute()
            .left_0()
            .top(px(spec.inset))
            .bottom(px(spec.inset))
            .w(px(thick))
            .bg(color),
        Axis::H => div()
            .absolute()
            .left(px(spec.inset))
            .right(px(spec.inset))
            .top(px((spec.band - thick) / 2.))
            .h(px(thick))
            .bg(color),
    }
}
