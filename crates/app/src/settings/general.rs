//! 设置「界面」页 (v60)：主题（淡色5 + 深色2 各一排）/ 字体（三槽位：族
//! 下拉 + 字号三档下拉）/ 语言（三个一排）。主题持久化只写
//! pi-flash 自己的 app_settings.json——pi 的 settings.json 有同名的 theme
//! 键（值域 dark/light），写入 pi-flash 主题 id 会让 pi 每次启动报错。

use super::*;
use crate::appearance::{FontSlot, ThemeEntry};

/// 槽位序号 → FontSlot（族/字号下拉共用）。
fn slot_of(ix: usize) -> FontSlot {
    match ix {
        0 => FontSlot::Session,
        1 => FontSlot::Panel,
        _ => FontSlot::File,
    }
}

fn slot_font(ix: usize) -> crate::services::workspace::FontSpec {
    match ix {
        0 => crate::appearance::session_font(),
        1 => crate::appearance::panel_font(),
        _ => crate::appearance::file_font(),
    }
}

pub(crate) fn mc_general_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    font_popup: Option<usize>,
    font_dd: &gpui::Entity<DropdownState>,
    font_filter: &gpui::Entity<TextInput>,
    font_filter_value: &str,
    size_popup: Option<usize>,
    size_dd: &gpui::Entity<DropdownState>,
) -> gpui::AnyElement {
    let t = T();
    let _ = chat;
    let mut col = div().flex().flex_col().pr(px(6.));

    // ---- 主题：淡色一排 / 深色一排 --------------------------------------
    col = col.child(section_label(tr("主题")));
    col = col.child(theme_row(false, weak));
    col = col.child(theme_row(true, weak));

    // ---- 字体：三槽位（族下拉 250px + 字号步进 + 预览行） ----------------
    col = col.child(section_label(tr("字体")));
    for (ix, label) in [
        (0usize, tr("会话字体")),
        (1, tr("面板字体")),
        (2, tr("文档字体")),
    ] {
        col = col.child(font_row(
            ix,
            label,
            weak,
            font_dd.clone(),
            font_filter.clone(),
            font_filter_value,
            font_popup == Some(ix),
            size_popup == Some(ix),
            size_dd,
            t,
        ));
    }

    // ---- 语言：三个一排，左对齐 ------------------------------------------
    col = col.child(section_label(tr("语言")));
    col = col.child(lang_row(weak, t));

    col.into_any_element()
}

fn section_label(text: &str) -> gpui::AnyElement {
    let t = T();
    div()
        .mt(px(20.))
        .mb(px(8.))
        .text_size(crate::appearance::ui_size(13.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(t.text_muted))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

/// 一排主题卡片。每排固定 5 个等宽槽位（淡色 5 满、深色 2 + 3 个空槽），
/// 上下两排槽位同宽。卡片即配色横条本身（无外框壳）：主题底色 + accent /
/// muted 圆点，主题名居中放进横条（文字用主题自身前景色保证可读）。
fn theme_row(dark: bool, weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    let current = theme::theme_name();
    let entries: Vec<&ThemeEntry> = crate::appearance::registry()
        .iter()
        .filter(|e| {
            theme::ALL
                .iter()
                .find(|(id, _)| *id == e.id)
                .map(|(_, th)| th.dark == dark)
                .unwrap_or(false)
        })
        .collect();
    let cells: Vec<Option<&ThemeEntry>> = (0..5).map(|ix| entries.get(ix).copied()).collect();
    div()
        .flex()
        .gap(px(8.))
        .mt(px(8.))
        .children(cells.into_iter().map(|cell| {
            let Some(entry) = cell else {
                // 空槽：只占位，保持与上一排等宽
                return div().flex_1().min_w_0().into_any_element();
            };
            let th = theme::ALL
                .iter()
                .find(|(id, _)| *id == entry.id)
                .map(|(_, t)| *t)
                .expect("registry id missing from theme table");
            let active = entry.id == current;
            let weak_card = weak.clone();
            let theme_id = entry.id.to_string();
            // 卡片外包一层无 padding/边框的等宽槽位：卡片自带的 padding+边框
            // 会计入 taffy 的假设主尺寸（flex-basis 0 压不掉），直接 flex_1 会
            // 比空槽宽出一个 padding+border，导致上下两排不等宽。
            div()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .id(SharedString::from(format!("theme-{}", entry.id)))
                        .w_full()
                        .h(px(36.))
                        .rounded(px(5.))
                        .border_1()
                        // 选中描边加粗到 2px（后写覆盖 border_1 的四边宽度）
                        .when(active, |d| d.border_2())
                        .border_color(rgb(if active { th.accent } else { th.border }))
                        .bg(rgb(th.bg))
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .px(px(8.))
                        .cursor_pointer()
                        .hover(|s| s.border_color(rgb(th.text_muted)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak_card.update(cx, |_c, cx| {
                                if crate::appearance::persist_theme(&theme_id) {
                                    // 006: re-map tokens so widget-library surfaces
                                    // (inputs/modals) follow the switch
                                    crate::appearance::sync_gpui_tokens(cx);
                                    cx.notify();
                                }
                            });
                        })
                        // 左圆点区 / 右空位等宽（各 flex_1），名字真正落在横条中央
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .child(div().size(px(7.)).rounded_full().bg(rgb(th.accent)))
                                .child(div().size(px(7.)).rounded_full().bg(rgb(th.text_muted))),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_size(crate::appearance::ui_size(11.5))
                                .font_weight(if active {
                                    gpui::FontWeight::SEMIBOLD
                                } else {
                                    gpui::FontWeight::NORMAL
                                })
                                .text_color(rgb(if active { th.accent } else { th.text }))
                                .whitespace_nowrap()
                                .child(SharedString::from(entry.name)),
                        )
                        .child(div().flex_1()),
                )
                .into_any_element()
        }))
        .into_any_element()
}

/// 一个字体槽位行：标签 + 250px 族下拉 + 字号三档下拉。所见即所得分工：
/// 面板字号 = 全局 UI 缩放（ui_size）；会话字体 = 用户/agent 输出正文的
/// 族+字号（markdown.rs MD_SPEC，不含 meta 等 chrome）；Markdown 字体 =
/// 文件预览 markdown。
fn font_row(
    ix: usize,
    label: &str,
    weak: &gpui::WeakEntity<Chat>,
    font_dd: gpui::Entity<DropdownState>,
    font_filter: gpui::Entity<TextInput>,
    font_filter_value: &str,
    open: bool,
    size_open: bool,
    size_dd: &gpui::Entity<DropdownState>,
    t: &'static crate::theme::Theme,
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

/// 字号下拉：三槽共用同一组档位 小 15 / 中 16 / 大 17 / 特大 18（默认
/// 15 = 小，默认档可选）。触发按钮 100px 显示当前档位；历史存档不在档位
/// 内时按就近档位显示/高亮，点任意档即落位。选中只改字号，字体族沿用
/// 当前值。
fn size_trigger(
    ix: usize,
    size: f32,
    open: bool,
    weak: &gpui::WeakEntity<Chat>,
    size_dd: &gpui::Entity<DropdownState>,
    t: &'static crate::theme::Theme,
) -> gpui::AnyElement {
    let sizes: &[(f32, &'static str)] = &[
        (15., tr("小")),
        (16., tr("中")),
        (17., tr("大")),
        (18., tr("特大")),
    ];
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
                if let Some(st) = c.settings.clone() {
                    st.update(cx, |s, cx| {
                        s.size_popup = if s.size_popup == Some(ix) { None } else { Some(ix) };
                        cx.notify();
                    });
                }
            });
        },
        // 弹层外被点：收起
        move |_, cx| {
            let _ = weak_dismiss.update(cx, |c, cx| {
                if let Some(st) = c.settings.clone() {
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
                                if let Some(st) = c.settings.clone() {
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
    t: &'static crate::theme::Theme,
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
                if let Some(st) = c.settings.clone() {
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
                if let Some(st) = c.settings.clone() {
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
    t: &'static crate::theme::Theme,
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
        "无匹配字体",
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
                        if let Some(st) = c.settings.clone() {
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

/// 语言：三个 radio（14px 圆圈，无边框无底色）。文字统一 text_muted，
/// 选中只加粗（SEMIBOLD）不变色；圆圈 accent 描边 + 内点。
fn lang_row(weak: &gpui::WeakEntity<Chat>, t: &'static crate::theme::Theme) -> gpui::AnyElement {
    let lang_current = i18n::lang_ix();
    div()
        .flex()
        .gap(px(24.))
        .children(i18n::LANG_LABELS.iter().enumerate().map(|(ix, label)| {
            let active = lang_current == ix;
            let weak_lang = weak.clone();
            div()
                .id(SharedString::from(format!("lang-{ix}")))
                .flex()
                .items_center()
                .gap(px(7.))
                .cursor_pointer()
                .font_weight(if active {
                    gpui::FontWeight::SEMIBOLD
                } else {
                    gpui::FontWeight::NORMAL
                })
                .text_color(rgb(t.text_muted))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_lang.update(cx, |_c, cx| {
                        i18n::set_lang(ix);
                        save_lang_pref(ix);
                        cx.notify();
                    });
                })
                .child(
                    div()
                        .size(px(14.))
                        .rounded_full()
                        .border_1()
                        .border_color(rgb(if active { t.accent } else { t.text_muted }))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(active, |d| {
                            d.child(div().size(px(6.)).rounded_full().bg(rgb(t.accent)))
                        }),
                )
                // 文字颜色继承父容器（radio 文字色统一在容器上设置）
                .child(
                    div()
                        .text_size(crate::appearance::ui_size(12.))
                        .child(SharedString::from(label.to_string())),
                )
                .into_any_element()
        }))
        .into_any_element()
}
