//! 组件树 gpui 渲染层·节点分派与外壳（[`ResolvedNode`] → 元素）：
//! 结构渲染（Column/Row/Grid/Group/Text/Icon/Canvas/Switch）+ 分区滚动 +
//! 可拖拽分隔线。按钮在 [`super::button`]、右键菜单层在 [`super::menu_layer`]、
//! 样式助手在 [`super::style`]。渲染规则见 mod.rs 头注释（树解释器，不猜布局）。

use super::style::{
    group_header, hairline, hex_color, section_header, theme_color, tone_color,
};
use super::*;

fn render_children(
    children: &[ResolvedNode],
    ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    ctx: &RenderCtx,
) -> Vec<gpui::AnyElement> {
    children
        .iter()
        .map(|c| render_node(c, ix, weak, t, ctx))
        .collect()
}

fn render_node(
    node: &ResolvedNode,
    ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    ctx: &RenderCtx,
) -> gpui::AnyElement {
    render_node_inner(node, ix, weak, t, ctx, false)
}

/// 根节点渲染入口：根 Column 撑满面板（justify 垂直居中等才有意义）。
/// 嵌套 Column 一律自适应高度——曾经无条件 flex_1 让页面级子列平分整页
/// 高度（分区布局被硬拉成对半分）。
pub(crate) fn render_root(
    node: &ResolvedNode,
    ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    ctx: &RenderCtx,
) -> gpui::AnyElement {
    render_node_inner(node, ix, weak, t, ctx, true)
}

fn render_node_inner(
    node: &ResolvedNode,
    ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    ctx: &RenderCtx,
    is_root: bool,
) -> gpui::AnyElement {
    match node {
        // ---- 外壳：分区 + 各自滚动 + 可拖拽分隔线（宿主画，插件不参与）----
        ResolvedNode::Frame { sections } => {
            let n = sections.len().max(1);
            let mut col = div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .h_full()
                .px(px(PANEL_PAD))
                .pt(px(2.))
                .pb(px(PANEL_PAD));
            for (i, sec) in sections.iter().enumerate() {
                if i > 0 {
                    col = col.child(render_splitter(ix, i - 1, weak, t, ctx));
                }
                let ratio = ctx.ratios.get(i).copied().unwrap_or(1. / n as f32);
                col = col.child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_basis(gpui::relative(ratio))
                        .min_h_0()
                        .child(render_section(sec, i, ix, weak, t, ctx)),
                );
            }
            col.into_any_element()
        }
        ResolvedNode::Section { .. } => render_section(node, 0, ix, weak, t, ctx),
        ResolvedNode::Column {
            pad,
            pad_x,
            pad_y,
            gap,
            align_center,
            justify_center,
            w,
            bg,
            radius,
            border,
            children,
        } => {
            let mut col = div().flex().flex_col().gap(px(*gap));
            // padX/padY：四边内距（数字=px，缺省回落 pad）
            col = col
                .px(px(pad_x.unwrap_or(*pad)))
                .py(px(pad_y.unwrap_or(*pad)));
            if let Some(w) = w {
                col = col.w(px(*w)).flex_none();
            }
            if let Some(c) = bg {
                col = col.bg(rgb(theme_color(c, t)));
            }
            if let Some(r) = radius {
                col = col.rounded(px(*r)).overflow_hidden();
            }
            if let Some(c) = border {
                col = col.border_1().border_color(rgb(theme_color(c, t)));
            }
            if is_root {
                // 仅根 Column 撑满面板高度（wrapper 已是 flex_col，子项
                // flex_1 = 占满；justify_center 居中整卡）
                col = col.flex_1().min_h_0();
            }
            if *align_center {
                col = col.items_center();
            }
            if *justify_center {
                col = col.justify_center();
            }
            for c in render_children(children, ix, weak, t, ctx) {
                col = col.child(c);
            }
            col.into_any_element()
        }
        ResolvedNode::Row {
            gap,
            pad,
            align,
            justify_between,
            justify_end,
            children,
        } => {
            let mut row = div().flex().flex_wrap().gap(px(*gap)).p(px(*pad));
            row = match align.as_str() {
                "start" => row.items_start(),
                "center" => row.items_center(),
                "end" => row.items_end(),
                _ => row, // stretch = 默认
            };
            if *justify_between {
                row = row.justify_between();
            } else if *justify_end {
                row = row.justify_end();
            } else {
                row = row.justify_center();
            }
            for c in render_children(children, ix, weak, t, ctx) {
                row = row.child(c);
            }
            row.into_any_element()
        }
        ResolvedNode::Group { title, children } => {
            let mut col = div().flex().flex_col().min_h_0();
            if !title.is_empty() {
                col = col.child(group_header(title, t));
            }
            for c in render_children(children, ix, weak, t, ctx) {
                col = col.child(c);
            }
            col.into_any_element()
        }
        ResolvedNode::Grid { min_item, children } => {
            let cols = grid_cols(ctx.panel_w, min_item);
            let mut grid = div()
                .grid()
                .grid_cols(cols)
                .gap(px(GRID_GAP))
                .pt(px(4.))
                .pb(px(8.));
            for c in render_children(children, ix, weak, t, ctx) {
                grid = grid.child(c);
            }
            grid.into_any_element()
        }
        ResolvedNode::Divider => div()
            .mx(px(10.))
            .my(px(6.))
            .h(px(1.))
            .bg(hairline(t))
            .into_any_element(),
        ResolvedNode::Text {
            text,
            tone,
            size,
            weight,
            ellipsis,
        } => {
            let mut el = div()
                .pt(px(4.))
                .text_size(*size)
                .font_weight(match weight.as_str() {
                    "medium" => gpui::FontWeight::MEDIUM,
                    "semibold" => gpui::FontWeight::SEMIBOLD,
                    _ => gpui::FontWeight::NORMAL,
                })
                .text_color(rgb(tone_color(tone, t)));
            if *ellipsis {
                el = el.text_ellipsis();
            }
            el.child(SharedString::from(text.clone()))
                .into_any_element()
        }
        ResolvedNode::Icon { name, size, tone } => gpui::svg()
            .path(SharedString::from(icon_asset(name, ctx.plugin_id)))
            .text_color(rgb(tone_color(tone, t)))
            .size(px(*size))
            .into_any_element(),
        ResolvedNode::Button {
            label,
            icon,
            tip,
            arg,
            action,
            menu,
            variant,
            size,
            disabled,
            command,
        } => super::button::render_button(
            label, icon, tip, arg, action, menu, variant, size, *disabled, command, ix, weak, t,
            ctx,
        ),
        ResolvedNode::Canvas {
            width,
            height,
            paints,
            command,
        } => {
            // 画布容器：定宽高 + 裁剪（契约：paint 坐标不许逃逸出节点框）
            let mut canvas = div()
                .relative()
                .w(px(*width))
                .h(px(*height))
                .overflow_hidden();
            for p in paints {
                canvas = canvas.child(match p {
                    CanvasPaint::Rect { x, y, w, h, fill } => {
                        let mut d = div()
                            .absolute()
                            .left(px(*x))
                            .top(px(*y))
                            .w(px(*w))
                            .h(px(*h));
                        if let Some(c) = hex_color(fill) {
                            d = d.bg(rgb(c));
                        }
                        d
                    }
                    CanvasPaint::Text {
                        x,
                        y,
                        size,
                        tone,
                        value,
                    } => div()
                        .absolute()
                        .left(px(*x))
                        .top(px(*y))
                        .text_size(px(*size))
                        .text_color(rgb(tone_color(tone, t)))
                        .child(SharedString::from(value.clone())),
                });
            }
            // 框内提示即按钮（contract canvas.command）：左键点画布回环插件命令
            if !command.is_empty() {
                return canvas
                    .id(SharedString::from(format!("pf-canvas-{ix}-{command}")))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        super::button::click_command(
                            weak.clone(),
                            ix,
                            command.clone(),
                            String::new(),
                        ),
                    )
                    .into_any_element();
            }
            canvas.into_any_element()
        }
        ResolvedNode::Switch {
            value,
            label,
            tip,
            command,
            disabled,
        } => {
            // 契约 v3 开关：轨道/knob 走 §10.4 尺寸（SW_MD 28x16·knob10），
            // 点击翻转 → onCommand(command, {value: 新布尔})；声明式：
            // 插件收到回调改自己的数据，重渲后 value 自然到位
            let t_ = *t;
            let (track, knob_x, knob_color) = if *value {
                (rgb(t_.accent), 28. - 16. + 3., t_.bg)
            } else {
                (rgb(t_.text_faint), 3., t_.bg)
            };
            let weak = weak.clone();
            let cmd = command.clone();
            let next = !*value;
            let env = serde_json::json!({ "value": next }).to_string();
            let mut row = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(10.))
                .py(px(6.));
            if !label.is_empty() {
                row = row.child(
                    div()
                        .text_size(crate::appearance::ui_size(11.5))
                        .text_color(rgb(t_.text_muted))
                        .child(SharedString::from(label.clone())),
                );
            }
            let mut sw = div()
                .id(SharedString::from(format!("pf-switch-{ix}-{cmd}")))
                .relative()
                .w(px(28.))
                .h(px(16.))
                .rounded_full()
                .bg(track)
                .cursor_pointer()
                .child(
                    div()
                        .absolute()
                        .left(px(knob_x))
                        .top(px(3.))
                        .w(px(10.))
                        .h(px(10.))
                        .rounded_full()
                        .bg(rgb(knob_color)),
                );
            if !*disabled {
                sw = sw.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    let _ = weak.update(cx, |c, cx| {
                        c.plugin_command_fire_env(ix, &cmd, &env, window, cx);
                        cx.notify();
                    });
                });
            }
            if !tip.is_empty() {
                sw = sw.with_tip_text(tip.clone());
            }
            row.child(sw).into_any_element()
        }
        ResolvedNode::Unknown(ty) => div()
            .px(px(10.))
            .child(placeholder_center(&format!("[{ty}]"), t))
            .into_any_element(),
    }
}

/// 一个分区：标题（宿主样式）+ 自己的滚动区（每区一个 ScrollHandle）。
fn render_section(
    node: &ResolvedNode,
    seq: usize,
    ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    ctx: &RenderCtx,
) -> gpui::AnyElement {
    let (title, icon, title_align, title_pad, children): (
        Option<String>,
        Option<String>,
        &str,
        f32,
        &[ResolvedNode],
    ) = match node {
        ResolvedNode::Section {
            title,
            icon,
            title_align,
            title_pad,
            children,
        } => (
            title.clone(),
            icon.clone(),
            title_align.as_str(),
            *title_pad,
            children.as_slice(),
        ),
        other => (None, None, "start", 0., std::slice::from_ref(other)),
    };
    let mut col = div().flex().flex_col().flex_1().min_h_0();
    if title.is_some() || icon.is_some() {
        col = col.child(section_header(
            icon.as_deref(),
            title.as_deref().unwrap_or(""),
            title_align,
            title_pad,
            t,
            ctx.plugin_id,
        ));
    }
    let handle = ctx
        .scrolls
        .get(seq)
        .cloned()
        .unwrap_or_else(gpui::ScrollHandle::new);
    let mut body = div()
        .id(SharedString::from(format!("pf-sec-{ix}-{seq}")))
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&handle);
    for c in render_children(children, ix, weak, t, ctx) {
        body = body.child(c);
    }
    col.child(body).into_any_element()
}

/// 分区之间的分隔线：7px 命中区，按下后进入拖拽（比例归宿主）。
/// 线形走 §5.12 统一实现 `ui::divider`（与 dock 上下分区、内容区左缘同一条
/// 代码）：常驻 1px / hover·拖拽 4px accent，不再自开一套 hover 底色块。
fn render_splitter(
    ix: usize,
    index: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    ctx: &RenderCtx,
) -> gpui::AnyElement {
    let weak = weak.clone();
    // 高亮态 = 悬停本条 ‖ 正在拖本条（§5.12：hover 与拖拽同款 4px accent）
    let active = ctx.split_hover == Some(index) || ctx.split_drag == Some(index);
    let band = crate::ui::tokens::divider::HIT;
    div()
        .id(SharedString::from(format!("pf-split-{ix}-{index}")))
        .h(px(band))
        .flex_none()
        .relative()
        .cursor(gpui::CursorStyle::ResizeUpDown)
        .on_hover({
            let weak_hv = weak.clone();
            move |h: &bool, _, cx| {
                let _ = weak_hv.update(cx, |c, cx| {
                    let Some(e) = c.plugins.plugins.get_mut(ix) else {
                        return;
                    };
                    // 进：本序号；出：只清自己（另一条分隔线接住 hover 时
                    // 不被误清）
                    let cur = *e.split_hover.borrow();
                    let next = if *h {
                        Some(index)
                    } else if cur == Some(index) {
                        None
                    } else {
                        cur
                    };
                    if next != cur {
                        *e.split_hover.borrow_mut() = next;
                        cx.notify();
                    }
                });
            }
        })
        .on_mouse_down(MouseButton::Left, move |ev, _, cx| {
            cx.stop_propagation();
            let y = f32::from(ev.position.y);
            let _ = weak.update(cx, |c, cx| {
                if let Some(e) = c.plugins.plugins.get_mut(ix) {
                    let base = e.split_ratios.borrow().clone();
                    *e.split_drag.borrow_mut() = Some(SplitDrag {
                        index,
                        start_y: y,
                        base,
                    });
                    cx.notify();
                }
            });
        })
        .child(crate::ui::divider::line(
            crate::ui::divider::LineSpec::horizontal(0., band),
            active,
            t,
        ))
        .into_any_element()
}
