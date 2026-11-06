//! 组件树 gpui 渲染层·按钮（01 §5 button）：格子（图标上/短名下）、纯图标
//! 方钮、文字行钮三形态 + 左键 action/command 接线 + 右键菜单接线。

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn render_button(
    label: &str,
    icon: &str,
    tip: &str,
    arg: &str,
    action: &Option<CellAction>,
    menu: &[MenuItem],
    variant: &str,
    size: &str,
    disabled: bool,
    command: &str,
    ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    ctx: &RenderCtx,
) -> gpui::AnyElement {
    let has_cmd = !command.is_empty();
    let dead = disabled || (!has_cmd && matches!(action, None | Some(CellAction::Unsupported(_))));
    let (cell_h, icon_px) = button_dims(size);
    let icon_only = variant == "icon";

    // 点/右键 都只在"活"按钮上挂
    let with_clicks = |mut el: gpui::Stateful<gpui::Div>| -> gpui::Stateful<gpui::Div> {
        if !dead {
            el = el.cursor_pointer().hover(|s| s.bg(rgb(t.bg_hover)));
            if has_cmd {
                // 契约 v3+：按钮左键 = 回环插件命令（"用户自己点"的回环入口）
                el = el.on_mouse_down(
                    MouseButton::Left,
                    click_command(weak.clone(), ix, command.to_string(), arg.to_string()),
                );
            } else if !matches!(action, None | Some(CellAction::Unsupported(_))) {
                el = el.on_mouse_down(
                    MouseButton::Left,
                    click_action(weak.clone(), ix, action.clone()),
                );
            }
        }
        if !menu.is_empty() {
            el = el.on_mouse_down(
                MouseButton::Right,
                open_cell_menu(weak.clone(), ix, menu.to_vec(), arg.to_string()),
            );
        }
        el
    };

    // 纯图标按钮：方形，label 退成 tooltip
    if icon_only {
        let side = match size {
            "SM" => 28.,
            "LG" => 40.,
            _ => 34.,
        };
        let mut el = div()
            .id(SharedString::from(format!("pf-icobtn-{ix}-{label}")))
            .w(px(side))
            .h(px(side))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(crate::ui::tokens::radius::CTRL));
        el = with_clicks(el);
        let tip_text = if tip.is_empty() { label } else { tip };
        if !tip_text.is_empty() {
            el = el.with_tip_text(tip_text.to_string());
        }
        return el
            .child(
                gpui::svg()
                    .path(SharedString::from(icon_asset(icon, ctx.plugin_id)))
                    .text_color(rgb(if dead { t.text_dim } else { t.text }))
                    .size(px(icon_px.min(side - 8.))),
            )
            .into_any_element();
    }

    // 无图标 = 文字按钮（列内整行）
    if icon.is_empty() {
        let mut el = div()
            .id(SharedString::from(format!("pf-btn-{ix}-{label}")))
            .flex()
            .items_center()
            .h(px(cell_h * 0.34))
            .px(px(8.))
            .rounded(px(crate::ui::tokens::radius::CTRL))
            .text_size(ui_size(11.))
            .text_ellipsis()
            .text_color(rgb(if dead { t.text_dim } else { t.text }));
        el = with_clicks(el);
        if !tip.is_empty() {
            el = el.with_tip_text(tip.to_string());
        }
        return el
            .child(SharedString::from(label.to_string()))
            .into_any_element();
    }

    // 有图标 = 格子（图标上、短名下；尺寸随 button.size）
    let mut cell = div()
        .id(SharedString::from(format!("pf-cell-{ix}-{label}")))
        .w_full()
        .h(px(cell_h))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(crate::ui::tokens::space::SP1))
        .rounded(px(crate::ui::tokens::radius::CTRL));
    cell = with_clicks(cell);
    if !tip.is_empty() {
        cell = cell.with_tip_text(tip.to_string());
    }
    cell.child(
        div()
            .w(px(icon_px))
            .h(px(icon_px))
            .flex()
            .items_center()
            .justify_center()
            .child(
                gpui::svg()
                    .path(SharedString::from(icon_asset(icon, ctx.plugin_id)))
                    .text_color(rgb(if dead { t.text_dim } else { t.text }))
                    .size(px(icon_px)),
            ),
    )
    .child(
        div()
            .w_full()
            .px(px(2.))
            .text_size(ui_size(11.))
            .text_center()
            .text_color(rgb(t.text_muted))
            .text_ellipsis()
            .child(SharedString::from(label.to_string())),
    )
    .into_any_element()
}

pub(super) fn click_action(
    weak: gpui::WeakEntity<Chat>,
    ix: usize,
    action: Option<CellAction>,
) -> impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static {
    move |_, window, cx| {
        let action = action.clone();
        let _ = weak.update(cx, |c, cx| {
            if let Some(a) = action {
                c.plugin_dispatch(ix, vec![super::menu_layer::to_js_action(a)], window, cx);
            }
        });
    }
}

/// 左键：按钮 `command` → 插件命令回调（与右键菜单项无 action 时的同一条链）。
pub(super) fn click_command(
    weak: gpui::WeakEntity<Chat>,
    ix: usize,
    command: String,
    arg: String,
) -> impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static {
    move |_, window, cx| {
        let cmd = command.clone();
        let a = arg.clone();
        let _ = weak.update(cx, |c, cx| {
            // 契约：会话域命令回环必须带 env.session.key（fn_bar 同规）。
            // 画布/开关点击不带 arg 时宿主补当前会话键——壳侧 onCommand 用
            // env.session.key 定位投递目标，空 env = 意图静默丢失（2026-10-30
            // 真机：超时白遮态点画布回环无响应，即空 env 丢失）。
            // 键源 = **pool.active_key 规范键**（005 §②，与挂载台账/PF_SESSION_KEY
            // 同源；此前取 pi 侧 session_file/session_id，草稿会话两者皆空）。
            let a = if a.trim().is_empty() {
                let key = c.pool.active_key.clone();
                serde_json::json!({ "session": { "key": key } }).to_string()
            } else {
                a
            };
            c.plugin_command_fire(ix, &cmd, &a, window, cx);
        });
    }
}

/// 右键：把菜单项连同鼠标位置记进 `PluginEntry::cell_menu`，由面板渲染弹出层。
fn open_cell_menu(
    weak: gpui::WeakEntity<Chat>,
    ix: usize,
    menu: Vec<MenuItem>,
    cell_path: String,
) -> impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static {
    move |ev: &gpui::MouseDownEvent, _window, cx| {
        cx.stop_propagation();
        let menu = menu.clone();
        let path = cell_path.clone();
        let (x, y) = (f32::from(ev.position.x), f32::from(ev.position.y));
        let _ = weak.update(cx, |c, cx| {
            if let Some(entry) = c.plugins.plugins.get_mut(ix) {
                entry.cell_menu = Some(CellMenuState {
                    x,
                    y,
                    path,
                    items: menu,
                });
                cx.notify();
            }
        });
    }
}
