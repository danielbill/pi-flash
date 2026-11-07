//! 模型菜单渲染：组头行、模型行、整卡 view（锚点定位 + 键盘态）。

use super::*;

/// 组头行：provider id 大写（截图同款 DEEPSEEK / ZAI-CODING-CN / FREEFLOW）。
pub(crate) fn group_header_row(
    provider: &str,
    pad_x: f32,
    h: f32,
    t: &'static crate::appearance::theme::Theme,
) -> AnyElement {
    div()
        .h(px(h))
        .px(px(pad_x))
        .flex()
        .items_center()
        .child(
            div()
                .text_size(crate::appearance::ui_size(9.5))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(provider.to_string())),
        )
        .into_any_element()
}

/// 整行点击动作（菜单 = 切换会话模型并收起；设置·下列表无行点击）。
pub(crate) type ModelRowAction = Box<dyn Fn(&mut Chat, &mut gpui::Context<Chat>) + 'static>;

/// 模型行（两处共用）：✓槽位（当前会话模型）+ 文本 + pin 徽章 + 五角星
/// （app-settings 默认）。槽位恒占位——组头 20px 的层级缩进由它保证。
#[allow(clippy::too_many_arguments)]
pub(crate) fn group_model_row(
    weak: &gpui::WeakEntity<Chat>,
    m: &pi_link::protocol::ModelInfo,
    is_default: bool,
    is_current: bool,
    selected: bool,
    clickable: bool,
    row_h: f32,
    pad_x: f32,
    pin: Option<String>,
    on_row: Option<ModelRowAction>,
    t: &'static crate::appearance::theme::Theme,
) -> AnyElement {
    let ui = crate::appearance::ui_size;
    let row_weak = weak.clone();
    let star_weak = weak.clone();
    let (dp, di) = (m.provider.clone(), m.id.clone());
    let (sp, si) = (dp.clone(), di.clone());
    let label = if m.name.is_empty() {
        m.id.clone()
    } else {
        m.name.clone()
    };
    let mut row = div()
        .id(SharedString::from(format!("mp-{}-{}", m.provider, m.id)))
        .h(px(row_h))
        // uniform_list/vlist 的行是 fit-content：不写 w_full 右侧星标会贴
        // 着文字（flex_1 无处可撑）；菜单 flex-col 里天然满宽，写了也无害
        .w_full()
        .px(px(pad_x))
        .flex()
        .items_center()
        .gap(px(8.))
        .when(selected, |d| d.bg(rgb(t.bg_selected)))
        .child(if is_current {
            crate::ui::icon("check", 12., t.accent)
        } else {
            div().w(px(12.)).flex_shrink_0().into_any_element()
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(ui(11.))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        // 默认行只用品色区分（2026-10-10 定稿：不加粗）
                        .text_color(rgb(if is_default { t.text } else { t.text_muted }))
                        .child(SharedString::from(label)),
                )
                .children(pin.map(|p| {
                    crate::ui::kit::tag(
                        &p,
                        crate::ui::kit::indigo_bg(),
                        crate::ui::kit::indigo_fg(),
                    )
                })),
        )
        .child(
            // 五角星：实心 = 当前默认（全局唯一）；再点取消
            div()
                .id(SharedString::from(format!(
                    "mp-star-{}-{}",
                    m.provider, m.id
                )))
                .flex_shrink_0()
                .cursor_pointer()
                .hover(|s| s.opacity(0.8))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    cx.stop_propagation();
                    let _ =
                        star_weak.update(cx, |c, cx| c.mc_set_default(sp.clone(), si.clone(), cx));
                })
                .child(crate::ui::icon(
                    if is_default { "star-filled" } else { "star" },
                    12.,
                    if is_default { 0xf5a623 } else { t.text_faint },
                )),
        );
    if clickable {
        row = row
            .cursor_pointer()
            .hover(|s| s.bg(rgb(t.bg_hover)))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                if let Some(on_row) = &on_row {
                    let _ = row_weak.update(cx, |c, cx| on_row(c, cx));
                }
            });
    }
    row.into_any_element()
}

/// 行点击动作（双参版：composer 选会话模型、设置·子代理页写表单字段）。
pub(crate) type ModelPickAction =
    std::rc::Rc<dyn Fn(&mut Chat, &mut gpui::Context<Chat>, String, String) + 'static>;

/// 模型菜单列表体（composer 菜单与设置·子代理页共用，2026-11-07 收口）：
/// `group_entries` 装配组头 + 模型行。`current` = ✓ 模型键（None = 无 ✓），
/// `default_ref` = 五角星默认，`sel_highlight(abs)` = 键盘高亮（composer 传
/// 闭包、设置传恒 false），`on_pick` = 行点击 (provider, id)。
pub(crate) fn model_menu_body(
    weak: &gpui::WeakEntity<Chat>,
    models: &[pi_link::protocol::ModelInfo],
    current: Option<&str>,
    default_ref: Option<&str>,
    sel_highlight: impl Fn(usize) -> bool,
    on_pick: Option<ModelPickAction>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::Div {
    let entries = Chat::group_entries(models);
    let mut body = div().flex().flex_col();
    for (abs, e) in entries.iter().enumerate() {
        match e {
            ModelGroupEntry::Header(name) => {
                body = body.child(group_header_row(name, PAD, ROW_H * 0.8, t));
            }
            ModelGroupEntry::Model(ix) => {
                let m = &models[*ix];
                let selected = sel_highlight(abs);
                let r = format!("{}/{}", m.provider, m.id);
                let on_row = on_pick.clone().map(|on| {
                    let (dp, di) = (m.provider.clone(), m.id.clone());
                    let action: ModelRowAction = Box::new(move |c: &mut Chat, cx| {
                        on(c, cx, dp.clone(), di.clone());
                    });
                    action
                });
                body = body.child(group_model_row(
                    weak,
                    m,
                    default_ref == Some(r.as_str()),
                    current == Some(r.as_str()),
                    selected,
                    true,
                    ROW_H,
                    PAD,
                    None,
                    on_row,
                    t,
                ));
            }
        }
    }
    body
}

/// 列表内容高（组头 0.8×行高 + 模型行 1×行高）：菜单卡限高与设置页复用
/// 同一把尺。
pub(crate) fn model_menu_content_h(models: &[pi_link::protocol::ModelInfo]) -> f32 {
    Chat::group_entries(models)
        .iter()
        .map(|e| match e {
            ModelGroupEntry::Header(_) => ROW_H * 0.8,
            ModelGroupEntry::Model(_) => ROW_H,
        })
        .sum()
}

/// 面板元素（`None` = 没开）。挂在 app root 上，锚点同 MCP 菜单。
pub(crate) fn view(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    window: &gpui::Window,
    cx: &gpui::App,
) -> Option<AnyElement> {
    let picker = chat.dlg.model_picker.as_ref()?;
    let t = T();
    let ui = crate::appearance::ui_size;

    let (models, total) = chat.picker_models(cx);
    let show_filter = total >= MODEL_FILTER_MIN;
    // ✓ = 当前会话模型。草稿 / 进程已死时 select_model 只记 pending_model
    //（spawn 时才变成 --model），它是用户最新意图，优先于 state 里的旧模型；
    // 活进程下 pending 恒空（走 SetModel），仍以 state 为准。都没有（全新
    // 草稿）→ 兜底到 spawn 真会用的默认（星标 / pi 默认），与 pill 同源：
    // 新会话一打开 ✓ 就在默认行上（用户 2026-11-02 定稿）
    let current = {
        let r = chat.rt().read(cx);
        r.pending_model
            .as_ref()
            .map(|(p, id)| format!("{p}/{id}"))
            .or_else(|| {
                r.state
                    .as_ref()
                    .and_then(|s| s.model.as_ref())
                    .map(|m| format!("{}/{}", m.provider, m.id))
            })
            .or_else(|| {
                chat.new_session_default()
                    .0
                    .map(|(p, id)| format!("{p}/{id}"))
            })
    };
    let default_ref = chat
        .cfg
        .mc_default_model
        .as_ref()
        .map(|(p, id)| format!("{p}/{id}"));

    // sel（模型行序）→ 展开行的绝对下标（entries 里第 sel 个 Model 行）
    let mut sel_abs = None;
    {
        let mut seen = 0usize;
        for (abs, e) in Chat::group_entries(&models).iter().enumerate() {
            if matches!(e, ModelGroupEntry::Model(_)) {
                if seen == picker.sel {
                    sel_abs = Some(abs);
                    break;
                }
                seen += 1;
            }
        }
    }

    let pick_action: ModelPickAction = std::rc::Rc::new(|c: &mut Chat, cx, dp, di| {
        c.rt().update(cx, |r, cx| r.select_model(dp, di, cx));
        // picking is also the dismissal gesture
        c.dlg.model_picker = None;
        cx.notify();
    });
    let mut body = model_menu_body(
        weak,
        &models,
        current.as_deref(),
        default_ref.as_deref(),
        |abs| sel_abs == Some(abs),
        Some(pick_action),
        t,
    );
    if models.is_empty() {
        body = body.child(
            div()
                .px(px(PAD))
                .py(px(PAD))
                .text_size(ui(11.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(crate::i18n::tr(
                    "action.no_models_match",
                ))),
        );
    }
    // 列表内容高：精确值，不用行数估；空列表 = 空态提示行（上下 12px 留白
    // + 一行文字）
    let content_h: f32 = if models.is_empty() {
        46.
    } else {
        model_menu_content_h(&models)
    };

    // 锚点算法与 MCP 菜单一致（统一定位：按钮居中 + 半屏定向，唯一落点
    // pill_anchor_edges）
    let vp = window.viewport_size();
    let panel_w = px(380.);
    let pos = crate::session::picker_shell::pill_anchor_edges(chat.dlg.pill_anchor, vp, panel_w);

    // 列表高 = min(内容高, 10 行上限, 视口内可用高)：内容超高时列表滚动
    // （滚动条常显），卡片也不顶出窗口沿。
    // 可用高 = 卡片纵边到对面视口缘 - 过滤块(10+30+6) - 列表底部留白(6)
    let filter_h = if show_filter { 46. } else { 0. };
    let space = match pos.top {
        Some(top) => vp.height - top,
        None => pos.bottom.unwrap_or_default(),
    };
    let avail_h = (space - px(filter_h + 6.)).max(px(80.));
    let list_h = content_h.min(ROW_H * 10.).min(avail_h.to_f64() as f32);
    // 内容超出可视区 = 可滚动：右缘多留 4px 给滚动条命中带（star 不被抢点击）
    let scrollable = content_h > list_h - 6. + 0.5;
    let row_pr = if scrollable { 14. } else { 10. };

    let layer = crate::ui::menu::menu_layer(None, weak, |c, _cx| c.dlg.model_picker = None);

    let weak_apply = weak.clone();
    let weak_up = weak.clone();
    let weak_down = weak.clone();
    let card = crate::ui::overlay::stop_click(
        crate::session::picker_shell::picker_card(pos, panel_w, t)
            // Enter：与原 ModelSelect 同路——过滤输入单行模式的 Enter 会向上
            // 冒泡，在卡片上拦下；apply 会丢弃派发中的实体，defer 出派发周期
            .on_key_down(move |ev: &gpui::KeyDownEvent, _w, cx| {
                if ev.keystroke.key != "enter" {
                    return;
                }
                cx.stop_propagation();
                let weak = weak_apply.clone();
                cx.defer(move |cx| {
                    let _ = weak.update(cx, |c, cx| c.apply_model_sel(cx));
                });
            })
            .on_action(move |_: &crate::ComposerUp, _w, cx| {
                let _ = weak_up.update(cx, |c, cx| c.move_model_sel(-1, cx));
            })
            .on_action(move |_: &crate::ComposerDown, _w, cx| {
                let _ = weak_down.update(cx, |c, cx| c.move_model_sel(1, cx));
            })
            // 无标题；过滤框按 MODEL_FILTER_MIN 阈值出现（占位壳保持节奏）
            .child(
                div()
                    .px(px(PAD))
                    .pt(px(PAD))
                    .pb(px(PAD))
                    .children(show_filter.then(|| picker.input.clone())),
            )
            .child(
                // 列表 + 滚动条：relative 壳让滚动条贴列表右缘（不随内容滚走），
                // 可滚动即常显（menu_scrollbar 变体：可滚动才画）
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("model-picker-list")
                            .pb(px(PAD))
                            .h(px(list_h))
                            .overflow_y_scroll()
                            .track_scroll(&picker.scroll)
                            .flex()
                            .flex_col()
                            // 右侧让出滚动条位（可滚动时多留 4px：滚动条命中带
                            // 不压住模型名/星标）
                            .pr(px(row_pr))
                            .child(body),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(2.))
                            .bottom(px(2.))
                            .right(px(0.))
                            .w(px(10.))
                            .child(crate::ui::psp_scrollbar::menu_scrollbar(&picker.scroll)),
                    ),
            ),
    );
    Some(layer.child(card).into_any_element())
}
