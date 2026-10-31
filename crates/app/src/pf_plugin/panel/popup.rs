//! 插件弹窗浮层（契约 v3 popups）：非模态居中卡，同一解释器渲染。
//! 外点/× 关闭；尺寸档位（SM/LG/MD）由宿主映射。自 panel.rs 拆出（090）。

use super::*;

/// 插件弹窗浮层（契约 v3 popups）：非模态居中卡（尺寸档位映射，MD≈500x500），
/// 外点/× 关闭；内容 = 插件 render_popup 树，同一解释器渲染。挂在 psp-overlays。
pub(crate) fn plugin_popup_layer(
    chat: &mut Chat,
    _vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let Some(p) = chat.plugins.popup.as_ref() else {
        return div().into_any_element();
    };
    let ix = p.ix;
    let (id, title, body, tree_loaded) =
        (p.id.clone(), p.title.clone(), p.body.clone(), p.tree_loaded);
    let weak = cx.entity().downgrade();
    // 非模态：不压暗背景、不抢焦点；点外/× 即收（overlay 基座规则 2/5）
    let layer = crate::ui::menu::menu_layer(None, &weak, |c, cx| c.plugin_popup_close(cx));
    // 尺寸档位 → 实现值（宿主映射；插件不写像素）
    let (w, h) = match chat
        .plugins
        .plugins
        .get(ix)
        .and_then(|e| e.manifest.popups.iter().find(|p| p.id == id))
        .map(|p| p.size.as_str())
    {
        Some("SM") => (400., 420.),
        Some("LG") => (600., 640.),
        _ => (420., 630.), // MD（缺省）：用户定（2026-10-28）
    };
    let mut card = div()
        .occlude()
        .flex()
        .flex_col()
        .w(px(w))
        .h(px(h))
        .max_h_full()
        .bg(rgb(t.chrome)) // 卡片底色 = topbar 同层（用户定）
        .border_1()
        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x8c)))
        .rounded(px(crate::ui::tokens::radius::CARD))
        .overflow_hidden();
    // 标题栏：标题 + ×（overlay 规则 5：有可见内容的卡片挂关闭钮）
    // title 为空 = 不画标题栏（旧版弹窗无标题栏形态，用户 2026-10-28 定夺）
    let weak_x = weak.clone();
    if !title.is_empty() {
        card = card.child(
            div()
                .id("pf-popup-titlebar")
                .flex()
                .items_center()
                .justify_between()
                .px(px(12.))
                .py(px(8.))
                .border_b_1()
                .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x66)))
                .child(
                    div()
                        .text_size(crate::appearance::ui_size(12.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(t.text))
                        .child(SharedString::from(title)),
                )
                .child(
                    div()
                        .id("pf-popup-close")
                        .cursor_pointer()
                        .px(px(6.))
                        .rounded(px(4.))
                        .hover(|s| s.bg(rgb(t.bg_hover)))
                        .text_color(rgb(t.text_muted))
                        .on_mouse_down(MouseButton::Left, move |_, _w, cx| {
                            let _ = weak_x.update(cx, |c, cx| c.plugin_popup_close(cx));
                        })
                        .child(crate::ui::icon("close", 14., t.text_muted)),
                ),
        );
    }

    // 内容区：同一解释器；未就绪占位
    let content = if tree_loaded && let Some(tree) = body.as_ref().and_then(resolve) {
        render_root(&tree, ix, &weak, t, &popup_render_ctx(chat, ix))
    } else {
        placeholder_center("…", t)
    };
    card = card.child(div().flex_1().min_h_0().flex().flex_col().child(content));
    // 居中：外层满幅 + 主轴居中（点外即层的外点关闭）
    layer
        .flex()
        .items_center()
        .justify_center()
        .child(card)
        .into_any_element()
}

/// 弹窗内容的渲染 ctx（面板同款；宽度用面板宽——插件树自适应）。
fn popup_render_ctx<'a>(chat: &'a Chat, ix: usize) -> RenderCtx<'a> {
    let entry = &chat.plugins.plugins[ix];
    RenderCtx {
        plugin_id: &entry.manifest.id,
        panel_w: chat.layout.slp_w,
        ratios: vec![1.0],
        scrolls: vec![],
        split_hover: *entry.split_hover.borrow(),
        split_drag: entry.split_drag.borrow().as_ref().map(|d| d.index),
    }
}

pub(crate) fn placeholder_center(text: &str, t: &'static Theme) -> gpui::AnyElement {
    div()
        .text_size(ui_size(11.))
        .text_color(rgb(t.text_dim))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

