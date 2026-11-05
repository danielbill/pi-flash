//! psp 文件树标题项目下拉（树根跨项目快切）：一览 psp.projects（会话
//! 列表里的全部项目，当前项目勾选），↑/↓ 高亮 + Enter 切换、鼠标点击
//! 直切。自 psp_overlays.rs 切出（itlq.12 超规拆分，纯结构搬移）。

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::Theme;
use crate::services::workspace::same_ws;
use crate::ui::icon;

/// 文件树标题项目下拉（树根跨项目快切）：一览 psp.projects（会话列表里
/// 的全部项目，当前项目勾选），↑/↓ 高亮 + Enter 切换、鼠标点击直切。
///
/// 键盘路径 = overlay::confirm 同款：render 焦点仲裁把 dialog_focus 交给
/// 下拉层（track_focus），layer 的 on_key_down 接 ↑/↓/Enter（ESC 由
/// overlay::layer 自带）。锚点 = 标题 paint 期 bounds（layout.ft_title_
/// bounds）：贴标题下缘左对齐（列表盖在树体上，dropdown 语义）；下方放
/// 不下才贴标题上缘向上弹。切换语义见 tree_proj_menu_pick（活跃会话不动）。
pub(crate) fn tree_proj_menu_layer(
    chat: &mut Chat,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = cx.entity().downgrade();
    let menu = chat.dlg.tree_proj_menu.clone().expect("checked by caller");
    // 标题 bounds 未量到（首帧 / 面板刚好收起）→ 收槽位不挂（防挂到空气
    // 里，tab_menu 同款守卫）
    let Some(tb) = chat.layout.ft_title_bounds.borrow().as_ref().map(|b| {
        (
            f32::from(b.origin.x),
            f32::from(b.origin.y),
            f32::from(b.size.width),
            f32::from(b.size.height),
        )
    }) else {
        chat.dlg.tree_proj_menu = None;
        return div().into_any_element();
    };
    // (名字, 路径, 是否当前)：当前态渲染期判定一次，行闭包只吃快照
    let items: Vec<(String, std::path::PathBuf, bool)> =
        chat.psp
            .projects
            .iter()
            .map(|g| {
                (
                    g.name.clone(),
                    g.path.clone(),
                    chat.ws.cwd.as_deref().is_some_and(|cur| {
                        same_ws(&g.path.to_string_lossy(), &cur.to_string_lossy())
                    }),
                )
            })
            .collect();
    let weak_nav = weak.clone();
    let layer = crate::ui::menu::menu_layer(Some(&chat.dlg.dialog_focus), &weak, |c, _cx| {
        c.dlg.tree_proj_menu = None;
    })
    // ↑/↓ 移高亮、Enter 切换（裸修饰键才接管；ESC 落到 layer 自己的
    // escape 分支）。无输入实体参与派发，apply 不需要 defer 出派发周期
    .on_key_down(move |ev: &gpui::KeyDownEvent, _w, cx| {
        let k = &ev.keystroke;
        let delta = match k.key.as_str() {
            "up" if !k.modifiers.modified() => Some(-1i32),
            "down" if !k.modifiers.modified() => Some(1),
            _ => None,
        };
        if let Some(delta) = delta {
            cx.stop_propagation();
            let _ = weak_nav.update(cx, |c, cx| c.tree_proj_menu_move(delta, cx));
            return;
        }
        if k.key == "enter" && !k.modifiers.modified() {
            cx.stop_propagation();
            let _ = weak_nav.update(cx, |c, cx| c.tree_proj_menu_apply(cx));
        }
    });
    let row_h = 28.;
    let max_h = 320.;
    // 估算高度（行数 × 行高 + 上下 p4）判下方放不放得下；超高滚动兜底
    let est_h = ((items.len() as f32) * row_h + 8.).min(max_h);
    let below = tb.1 + tb.3 + 4.;
    let fits_below = below + est_h <= vp.1 - 8.;
    let sel_bg = gpui::rgba((t.text << 8) | 0x1a);
    let mut card = div()
        .id("ftp-card")
        .absolute()
        .left(px(tb.0))
        .w(px(240.))
        .max_h(px(max_h))
        .overflow_y_scroll()
        .p(px(4.))
        .bg(rgb(t.bg))
        .border_1()
        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x8c)))
        .rounded(px(8.))
        .shadow_lg()
        .flex()
        .flex_col()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        });
    card = if fits_below {
        card.top(px(below))
    } else {
        card.bottom(px(vp.1 - tb.1 + 4.))
    };
    for (ix, (name, _path, current)) in items.into_iter().enumerate() {
        let row_weak = weak.clone();
        let selected = menu.sel == ix;
        card = card.child(
            div()
                .id(SharedString::from(format!("ftp-{ix}")))
                .h(px(row_h))
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(8.))
                .rounded(px(6.))
                .overflow_hidden()
                .text_size(crate::appearance::ui_size(11.5))
                // 键盘高亮 = 10% 前景薄纱常驻（psp 会话行/tree 选中同色系）
                .when(selected, |d| d.bg(sel_bg).hover(move |s| s.bg(sel_bg)))
                .when(!selected, |d| d.hover(|s| s.bg(rgb(t.bg_hover))))
                .child(icon("folder", 14., t.text_dim))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_color(rgb(if current { t.accent } else { t.text }))
                        .child(SharedString::from(name)),
                )
                .children(current.then(|| icon("check", 12., t.accent)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    cx.stop_propagation();
                    let _ = row_weak.update(cx, |c, cx| {
                        c.tree_proj_menu_pick(ix, cx);
                    });
                }),
        );
    }
    layer.child(card).into_any_element()
}
