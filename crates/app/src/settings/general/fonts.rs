//! 「界面」页字体区：字体行（族 + 字号）、档位触发器与弹卡。

use super::*;

/// 一个字体槽位行：标签 + 250px 族下拉 + 字号三档下拉。所见即所得分工：
/// 面板字号 = 全局 UI 缩放（ui_size）；会话字体 = 用户/agent 输出正文的
/// 族+字号（markdown.rs MD_SPEC，不含 meta 等 chrome）；Markdown 字体 =
/// 文件预览 markdown。
pub(super) fn font_row(
    ix: usize,
    label: &str,
    weak: &gpui::WeakEntity<Chat>,
    font_dd: gpui::Entity<DropdownState>,
    font_filter: gpui::Entity<TextInput>,
    font_filter_value: &str,
    open: bool,
    size_open: bool,
    size_dd: &gpui::Entity<DropdownState>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    let spec = slot_font(ix);
    let fam_display = spec.family.clone();

    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .mt(px(10.))
        .child(
            div()
                .w(px(130.))
                .flex_shrink_0()
                .whitespace_nowrap()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(label.to_string())),
        )
        .child(font_trigger(
            ix,
            &fam_display,
            open,
            font_dd,
            weak,
            &font_filter,
            font_filter_value,
            t,
        ))
        .child(size_trigger(ix, spec.size, size_open, weak, size_dd, t))
        .into_any_element()
}

/// 字号下拉：会话/面板共用 15/16/17/18；文档字体 16/18/20/22（2026-10-15
/// 用户定夺：文件正文要大一号，档位整体上移）。默认 15 不在文档档位内，
/// 由就近档位逻辑显示/高亮。触发按钮 100px 显示当前档位；历史存档不在
/// 档位内时按就近档位显示/高亮，点任意档即落位。选中只改字号，字体族
/// 沿用当前值。
fn size_trigger(
    ix: usize,
    size: f32,
    open: bool,
    weak: &gpui::WeakEntity<Chat>,
    size_dd: &gpui::Entity<DropdownState>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    let sizes: &[(f32, &'static str)] = if ix == 2 {
        &[
            (16., tr("size.small")),
            (18., tr("size.medium")),
            (20., tr("size.large")),
            (22., tr("size.extra_large")),
        ]
    } else {
        &[
            (15., tr("size.small")),
            (16., tr("size.medium")),
            (17., tr("size.large")),
            (18., tr("size.extra_large")),
        ]
    };
    let current_label = |v: f32| -> &'static str {
        let mut best = sizes[0];
        for s in sizes {
            if (s.0 - v).abs() < (best.0 - v).abs() {
                best = *s;
            }
        }
        best.1
    };
    let fam = slot_font(ix).family;
    let weak_toggle = weak.clone();
    let weak_dismiss = weak.clone();

    // 文字真居中：标签绝对定位铺满整行居中，箭头绝对定位贴右，互不挤占
    let trigger = div()
        .relative()
        .w(px(100.))
        .h(px(30.))
        .flex_shrink_0()
        .rounded(px(5.))
        .border_1()
        .border_color(rgb(if open { t.accent } else { t.border }))
        .bg(rgb(t.bg))
        .child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text))
                .child(SharedString::from(current_label(size))),
        )
        .child(
            div()
                .absolute()
                .right(px(9.))
                .top(px(9.))
                .child(crate::ui::icon(
                    if open { "chevron-up" } else { "chevron-down" },
                    12.,
                    t.text_muted,
                )),
        )
        .into_any_element();

    crate::ui::dropdown(
        SharedString::from(format!("size-dd-{ix}")),
        size_dd,
        open,
        // 触发按钮被点：开本槽位（再点已开的槽位 = 收起）
        move |_, cx| {
            let _ = weak_toggle.update(cx, |c, cx| {
                if let Some(st) = c.dlg.settings.clone() {
                    st.update(cx, |s, cx| {
                        s.size_popup = if s.size_popup == Some(ix) {
                            None
                        } else {
                            Some(ix)
                        };
                        cx.notify();
                    });
                }
            });
        },
        // 弹层外被点：收起
        move |_, cx| {
            let _ = weak_dismiss.update(cx, |c, cx| {
                if let Some(st) = c.dlg.settings.clone() {
                    st.update(cx, |s, cx| {
                        if s.size_popup.take().is_some() {
                            cx.notify();
                        }
                    });
                }
            });
        },
        trigger,
        move || {
            div()
                .w(px(100.))
                .py(px(3.))
                .bg(rgb(t.bg_panel))
                .border_1()
                .border_color(rgb(t.border))
                .rounded(px(6.))
                .shadow_lg()
                .overflow_hidden()
                .children(sizes.iter().map(|(v, label)| {
                    let active = *label == current_label(size);
                    let fam = fam.clone();
                    let weak_item = weak.clone();
                    let v = *v;
                    div()
                        .id(SharedString::from(format!("size-item-{ix}-{v}")))
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .px(px(10.))
                        .text_size(crate::appearance::ui_size(12.))
                        .text_color(rgb(if active { t.accent } else { t.text }))
                        .bg(rgb(if active { t.bg_selected } else { t.bg_panel }))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(t.bg_hover)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak_item.update(cx, |c, cx| {
                                crate::appearance::save_font(
                                    slot_of(ix),
                                    crate::services::workspace::FontSpec {
                                        family: fam.clone(),
                                        size: v,
                                    },
                                );
                                // 弹层内点击不会触发 on_mouse_down_out，选中后手动收起
                                if let Some(st) = c.dlg.settings.clone() {
                                    st.update(cx, |s, cx| {
                                        if s.size_popup.take().is_some() {
                                            cx.notify();
                                        }
                                    });
                                }
                                cx.notify();
                            });
                        })
                        .child(SharedString::from(*label))
                        .child(
                            div()
                                .text_size(crate::appearance::ui_size(10.5))
                                .text_color(rgb(t.text_muted))
                                .child(SharedString::from(format!("{v}px"))),
                        )
                        .into_any_element()
                }))
                .into_any_element()
        },
    )
}

/// 族下拉：通用 dropdown 组件（触发按钮 250px + 贴附弹层）。
fn font_trigger(
    ix: usize,
    family: &str,
    open: bool,
    font_dd: gpui::Entity<DropdownState>,
    weak: &gpui::WeakEntity<Chat>,
    font_filter: &gpui::Entity<TextInput>,
    font_filter_value: &str,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    let fam = family.to_string();
    let weak_toggle = weak.clone();
    let weak_dismiss = weak.clone();
    let weak_card = weak.clone();
    let filter = font_filter.clone();
    let filter_value = font_filter_value.to_string();

    let trigger = div()
        .w(px(250.))
        .h(px(30.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_between()
        .px(px(9.))
        .rounded(px(5.))
        .border_1()
        .border_color(rgb(if open { t.accent } else { t.border }))
        .bg(rgb(t.bg))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text))
                .font_family(fam.clone())
                .child(SharedString::from(fam)),
        )
        .child(crate::ui::icon(
            if open { "chevron-up" } else { "chevron-down" },
            12.,
            t.text_muted,
        ))
        .into_any_element();

    crate::ui::dropdown(
        SharedString::from(format!("font-dd-{ix}")),
        &font_dd,
        open,
        // 触发按钮被点：开本槽位（再点已开的槽位 = 收起）
        move |_, cx| {
            let _ = weak_toggle.update(cx, |c, cx| {
                if let Some(st) = c.dlg.settings.clone() {
                    st.update(cx, |s, cx| {
                        if s.font_popup == Some(ix) {
                            s.close_font_popup(cx);
                        } else {
                            s.open_font_popup(ix, cx);
                        }
                    });
                }
            });
        },
        // 弹层外被点：收起
        move |_, cx| {
            let _ = weak_dismiss.update(cx, |c, cx| {
                if let Some(st) = c.dlg.settings.clone() {
                    st.update(cx, |s, cx| s.close_font_popup(cx));
                }
            });
        },
        trigger,
        move || font_popup_card(ix, &filter, &filter_value, &weak_card, t),
    )
}

/// 弹层卡片：顶部筛选输入 + uniform_list 系统字体列表（每项以自身字体渲染）。
fn font_popup_card(
    slot_ix: usize,
    filter_input: &gpui::Entity<TextInput>,
    filter_value: &str,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    let weak = weak.clone();
    // 过滤后的字体索引（字母序目录，大小写不敏感子串匹配）
    let catalog = crate::appearance::font_catalog();
    let needle = filter_value.trim().to_lowercase();
    let idxs: std::rc::Rc<Vec<usize>> = std::rc::Rc::new(
        catalog
            .iter()
            .enumerate()
            .filter(|(_, name)| needle.is_empty() || name.to_lowercase().contains(&needle))
            .map(|(ix, _)| ix)
            .collect(),
    );

    let spec = slot_font(slot_ix);
    let current = spec.family.clone();

    let card = div()
        .w(px(250.))
        .h(px(400.))
        .bg(rgb(t.bg_panel))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(8.))
        .shadow_lg()
        .overflow_hidden()
        .flex()
        .flex_col()
        // 顶部筛选行
        .child(
            div()
                .flex_shrink_0()
                .p(px(6.))
                .border_b_1()
                .border_color(rgb(t.border))
                .child(filter_input.clone()),
        );
    card.child(vlist(
        "font-list",
        idxs.len(),
        30.,
        VListHeight::Fill,
        false,
        false,
        tr("settings.no_matching_fonts"),
        None,
        move |ix, _window, _cx| {
            let cat = crate::appearance::font_catalog();
            let Some(cat_ix) = idxs.get(ix).copied() else {
                return div().into_any_element();
            };
            let family = &cat[cat_ix];
            let active = *family == current;
            let weak_item = weak.clone();
            let fam = family.clone();
            div()
                .id(SharedString::from(format!("font-item-{ix}")))
                .h(px(30.))
                .w_full()
                .flex()
                .items_center()
                .px(px(10.))
                .font_family(family.clone())
                .text_size(crate::appearance::ui_size(12.5))
                .text_color(rgb(if active { t.accent } else { t.text }))
                .bg(rgb(if active { t.bg_selected } else { t.bg_panel }))
                .overflow_hidden()
                .whitespace_nowrap()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_item.update(cx, |c, cx| {
                        crate::appearance::save_font(
                            slot_of(slot_ix),
                            crate::services::workspace::FontSpec {
                                family: fam.clone(),
                                size: spec.size,
                            },
                        );
                        if let Some(st) = c.dlg.settings.clone() {
                            st.update(cx, |s, cx| s.close_font_popup(cx));
                        }
                        cx.notify();
                    });
                })
                .child(SharedString::from(family.clone()))
                .into_any_element()
        },
    ))
    .into_any_element()
}
