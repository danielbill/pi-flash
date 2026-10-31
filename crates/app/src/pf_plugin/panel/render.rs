//! 组件树 gpui 渲染层：[`ResolvedNode`] → 元素（结构渲染 + 交互接线 +
//! 样式助手）。渲染规则见 mod.rs 头注释（树解释器，不猜布局）。

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
        } => render_button(
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
                        click_command(weak.clone(), ix, command.clone(), String::new()),
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

#[allow(clippy::too_many_arguments)]
fn render_button(
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

fn click_action(
    weak: gpui::WeakEntity<Chat>,
    ix: usize,
    action: Option<CellAction>,
) -> impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static {
    move |_, window, cx| {
        let action = action.clone();
        let _ = weak.update(cx, |c, cx| {
            if let Some(a) = action {
                c.plugin_dispatch(ix, vec![to_js_action(a)], window, cx);
            }
        });
    }
}

/// 左键：按钮 `command` → 插件命令回调（与右键菜单项无 action 时的同一条链）。
fn click_command(
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
            let a = if a.trim().is_empty() {
                let key =
                    c.pool
                        .runtimes
                        .get(&c.pool.active_key)
                        .and_then(|rt| {
                            rt.read(cx).state.as_ref().and_then(|s| {
                                s.session_file.clone().or_else(|| s.session_id.clone())
                            })
                        })
                        .unwrap_or_default();
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

/// 插件格子右键菜单层：挂载点是 `psp-overlays`（与文件树右键菜单同一处），
/// 卡座与行样式复用 `menu_card` / `menu_item_asset`——**样式只有一份**。
pub(crate) fn cell_menu_layer(
    chat: &mut Chat,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let Some(ix) = chat
        .plugins
        .plugins
        .iter()
        .position(|p| p.cell_menu.is_some())
    else {
        return div().into_any_element();
    };
    let Some(menu) = chat.plugins.plugins[ix].cell_menu.clone() else {
        return div().into_any_element();
    };
    let plugin_id = chat.plugins.plugins[ix].manifest.id.clone();
    let weak = cx.entity().downgrade();
    // 与 psp 菜单同参：向下弹让 12px，左右象限决定横向方向
    let edges = crate::ui::popup::PopupEdges::around((menu.x, menu.y), vp, 0., 12.);
    let layer = crate::ui::menu::menu_layer(Some(&chat.dlg.dialog_focus), &weak, move |c, cx| {
        if let Some(e) = c.plugins.plugins.get_mut(ix) {
            e.cell_menu = None;
            cx.notify();
        }
    });

    let mut card = crate::function_panel::psp_overlays::menu_card(edges, t);
    for (i, item) in menu.items.iter().enumerate() {
        let cmd_id = item.id.clone();
        let action = item.action.clone();
        let path = menu.path.clone();
        let label = item.title.clone();
        let danger = item.danger;
        let asset = (!item.icon.is_empty()).then(|| icon_asset(&item.icon, &plugin_id));
        let weak_row = weak.clone();
        let row = crate::function_panel::psp_overlays::menu_item_asset(
            SharedString::from(format!("pf-menu-{ix}-{i}")),
            asset,
            14.,
            &label,
            danger,
            false,
        )
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            let (cmd_id, action, path) = (cmd_id.clone(), action.clone(), path.clone());
            let _ = weak_row.update(cx, |c, cx| {
                if let Some(entry) = c.plugins.plugins.get_mut(ix) {
                    entry.cell_menu = None;
                }
                match action {
                    Some(a) => c.plugin_dispatch(ix, vec![to_js_action(a)], window, cx),
                    None => c.plugin_command_fire(ix, &cmd_id, &path, window, cx),
                }
                cx.notify();
            });
        });
        card = card.child(row);
    }
    layer.child(card).into_any_element()
}

fn to_js_action(a: CellAction) -> JsAction {
    match a {
        CellAction::Run(p) => JsAction::Run(p),
        CellAction::Edit(p) => JsAction::Edit(p),
        CellAction::Reveal(p) => JsAction::RevealInExplorer(p),
        CellAction::Toast(m) => JsAction::Toast(m),
        CellAction::OpenUrl(u) => JsAction::OpenUrl(u),
        CellAction::OpenFolder(p) => JsAction::OpenFolder(p),
        CellAction::NewSession => JsAction::NewSession,
        CellAction::AskPi(t) => JsAction::AskPi(t),
        CellAction::Unsupported(name) => JsAction::Toast(format!("宿主不支持的动作：{name}")),
    }
}

/// 分区标题：图标 + 文字，**左对齐到内容左边**（外壳 `PANEL_PAD` 由
/// `panel-frame` 统一给，这里不再叠一层），上方留 `SECTION_HEAD_PT`。
fn section_header(
    icon_name: Option<&str>,
    label: &str,
    align: &str,
    pad: f32,
    t: &'static Theme,
    plugin_id: &str,
) -> gpui::AnyElement {
    let mut row = div()
        .flex_none()
        .flex()
        .items_center()
        // 对齐与留白由插件给意图（titleAlign / titlePad），像素在这里算：
        // 外壳已经有 PANEL_PAD，插件要的距离比它大就补差额，小则不再外扩
        .gap(px(6.))
        .pt(px(SECTION_HEAD_PT))
        .pb(px(2.));
    let inset = (pad - PANEL_PAD).max(0.);
    row = match align {
        "end" => row.justify_end().pr(px(inset)),
        "center" => row.justify_center(),
        _ => row.justify_start(),
    };
    if let Some(name) = icon_name {
        row = row.child(
            gpui::svg()
                .path(SharedString::from(icon_asset(name, plugin_id)))
                .text_color(rgb(t.text_faint))
                .size(px(crate::ui::tokens::icon::MD)),
        );
    }
    row.child(
        div()
            // 标题字号 = 面板正文（UI 规范 §2 body = ui(12)），不再用 9.5 小字
            .text_size(ui_size(12.))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(rgb(t.text_faint))
            .child(SharedString::from(label.to_string())),
    )
    .into_any_element()
}

/// `#rrggbb` / `#rgb` → 0xRRGGBB；非法 fill 返回 None（跳过上色，不留黑块）。
fn hex_color(s: &str) -> Option<u32> {
    let h = s.strip_prefix('#')?;
    let (r, g, b) = match h.len() {
        6 => (
            u8::from_str_radix(&h[0..2], 16).ok()?,
            u8::from_str_radix(&h[2..4], 16).ok()?,
            u8::from_str_radix(&h[4..6], 16).ok()?,
        ),
        3 => {
            let c =
                |i: usize| -> Option<u8> { Some(u8::from_str_radix(&h[i..i + 1], 16).ok()? * 17) };
            (c(0)?, c(1)?, c(2)?)
        }
        _ => return None,
    };
    Some(((r as u32) << 16) | ((g as u32) << 8) | b as u32)
}

fn group_header(label: &str, t: &'static Theme) -> gpui::AnyElement {
    div()
        .h(px(24.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .text_size(ui_size(12.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(label.to_string())),
        )
        .into_any_element()
}

/// 发丝线：主题 border 在暗色下与底色太近（看不清），统一用 text 的低透明度。
fn hairline(t: &'static Theme) -> gpui::Rgba {
    gpui::rgba(crate::appearance::theme::rgba_a(t.text, 0.18))
}

/// 主题色名 → 色值（column.bg / column.border 用；与 tone 词表同一套 + 补充）。
fn theme_color(name: &str, t: &'static Theme) -> u32 {
    match name {
        "bg" => t.bg,
        "bg_panel" => t.bg_panel,
        "chrome" => t.chrome, // topbar/statusbar 同层
        "border" => crate::appearance::theme::border_alpha(t, 0xC0),
        "white" => 0xffffff,
        _ => tone_color(name, t),
    }
}

fn tone_color(tone: &str, t: &'static Theme) -> u32 {
    match tone {
        "text" => t.text,
        "text.muted" => t.text_muted,
        "text.faint" => t.text_faint,
        "accent" => t.accent,
        "danger" => t.danger,
        _ => t.text_dim, // text.dim 默认
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_color_parses_and_rejects() {
        assert_eq!(hex_color("#111827"), Some(0x111827));
        assert_eq!(hex_color("#abc"), Some(0xaabbcc));
        assert_eq!(hex_color("red"), None);
        assert_eq!(hex_color("#12345"), None);
        assert_eq!(hex_color(""), None);
    }
}
