//! 子代理表单下拉与勾选构件：全局设置顶条（模块开关 + 最大并发）、
//! 模型下拉、turns/并发 pick_list、工具档位下拉与 dd_item 族、sanitize_id。

use super::*;

/// 模型下拉：首项「继承」+ 分隔线 + 启用模型列表（`provider/id`）。
#[allow(clippy::too_many_arguments)]
pub(super) fn model_dropdown(
    weak: &gpui::WeakEntity<Chat>,
    models: &[(String, String)],
    model: &Option<String>,
    popup: Option<u8>,
    dd: gpui::Entity<DropdownState>,
    readonly: bool,
) -> gpui::AnyElement {
    let t = T();
    let open = popup == Some(0);
    let label = match model {
        Some(m) => m.clone(),
        None => tr("agents.inherit").to_string(),
    };
    let models = models.to_vec();
    let weak_toggle = weak.clone();
    let weak_dismiss = weak.clone();
    let weak_out = weak.clone();
    let trigger = sel_trigger(&label, open);
    let ro_toggle = readonly;
    crate::ui::dropdown(
        "ag-model-dd",
        &dd,
        open && !readonly,
        move |_, cx| {
            if ro_toggle {
                return;
            }
            let _ = weak_toggle.update(cx, |c, cx| {
                if let Some(st) = c.dlg.settings.clone() {
                    st.update(cx, |s, cx| {
                        s.ag_popup = if s.ag_popup == Some(0) { None } else { Some(0) };
                        cx.notify();
                    });
                }
            });
        },
        move |_, cx| {
            let _ = weak_dismiss.update(cx, |c, cx| {
                if let Some(st) = c.dlg.settings.clone() {
                    st.update(cx, |s, cx| {
                        if s.ag_popup.take().is_some() {
                            cx.notify();
                        }
                    });
                }
            });
        },
        trigger,
        move || {
            let mut menu = div()
                .w(px(250.))
                .py(px(3.))
                .bg(rgb(t.bg))
                .border_1()
                .border_color(rgb(t.border))
                .rounded(px(8.))
                .shadow_lg()
                .overflow_hidden();
            // 「继承」项
            menu = menu.child(dd_item(
                weak_out.clone(),
                tr("agents.inherit").to_string(),
                model.is_none(),
                Some(String::new()),
                0,
            ));
            // 分隔线
            menu = menu.child(div().h(px(1.)).my(px(4.)).bg(rgb(t.border)));
            for (value, name) in &models {
                let selected = model.as_deref() == Some(value.as_str());
                menu = menu.child(dd_item(
                    weak_out.clone(),
                    name.clone(),
                    selected,
                    Some(value.clone()),
                    0,
                ));
            }
            menu.into_any_element()
        },
    )
}

/// 工具权限下拉（四档，带描述）。
/// 多选清单（044-0 工具定制/skills 定制）：checkbox + 名称行，选中态读
/// `selected`，点击经 `kind` 分派到 Settings 态的对应清单翻转。空清单显
/// 暂无提示。`kind`: "ext"（pi-ext 路径）/ "mcp"（服务器名）/ "skill"。
pub(super) fn pick_list(
    weak: &gpui::WeakEntity<Chat>,
    kind: &'static str,
    options: Vec<String>,
    selected: &[String],
) -> gpui::AnyElement {
    let t = T();
    let mut col = div()
        .id(SharedString::from(format!("ag-picks-{kind}")))
        .w_full()
        .max_h(px(150.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(px(2.));
    if options.is_empty() {
        col = col.child(
            div()
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_dim))
                .child(tr("agents.pick_empty")),
        );
    }
    for opt in options {
        let checked = selected.iter().any(|s| s == &opt);
        let weak_row = weak.clone();
        let opt_label = opt.clone();
        let opt_for_id = opt.clone();
        col = col.child(
            div()
                .id(SharedString::from(format!(
                    "ag-pick-{kind}-{}",
                    sanitize_id(&opt_for_id)
                )))
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(4.))
                .py(px(3.))
                .rounded(px(crate::ui::tokens::radius::INLINE))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                    let _ = weak_row.update(cx, |c, cx| {
                        if let Some(st) = c.dlg.settings.clone() {
                            let _ = st.update(cx, |s, cx| {
                                let slot = match kind {
                                    "ext" => &mut s.ag_extensions,
                                    "mcp" => &mut s.ag_mcp,
                                    _ => &mut s.ag_skills,
                                };
                                match slot.iter().position(|x| x == &opt) {
                                    Some(ix) => {
                                        slot.remove(ix);
                                    }
                                    None => slot.push(opt.clone()),
                                }
                                s.ag_dirty = true;
                                cx.notify();
                            });
                        }
                        cx.notify();
                    });
                })
                .child(crate::ui::kit::checkbox(checked))
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_size(crate::appearance::ui_size(11.5))
                        .text_color(rgb(if checked { t.text } else { t.text_dim }))
                        .child(SharedString::from(opt_label)),
                ),
        );
    }
    col.into_any_element()
}

/// 元素 id 消毒：扩展路径/服务器名可能带 `/\:.#` 等字符，gpui 元素 id
/// 只留字母数字与 `_`。
fn sanitize_id(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// 顶条「全局设置」（T 型布局首行，2026-11-07 用户定稿，对齐扩展页顶条）：
/// 两项一行——模块开关（写 app_settings `subagent_enabled`，关闭 = 下方
/// 列表/表单整体不可见）+ 最大并发（2-10，默认 3，kit 标准步进器）。
/// 嵌套深度恒定 1，不设 UI。开发旗标 `subagents` 仍是总闸
/// （PI_FLASH_SUBAGENTS 强关 = 整页从导航消失）；并发 spawn 时 env
/// 传导给 pf-subagents。
pub(super) fn global_settings_bar(weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    let t = T();
    let on = crate::services::workspace::subagent_module_switch();
    let cap = crate::services::workspace::subagent_capacity();
    let cap_item = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text))
                .child(tr("agents.capacity")),
        )
        .child(stepper(
            "ag-cap",
            cap,
            2,
            10,
            1,
            |v| v.to_string(),
            crate::services::workspace::subagent_capacity,
            crate::services::workspace::set_subagent_capacity,
            weak,
        ));
    // 模块开关行
    let sw_row = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text))
                .child(tr("agents.module_enable")),
        )
        .child(config_switch(
            "ag-module-sw",
            weak,
            on,
            false,
            move |_, cx| {
                crate::services::workspace::set_subagent_module_switch(!on);
                cx.notify();
            },
        ));
    div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(28.))
        // pl 15 = 列表内容左缘（list px7 + 行内 8），与扩展页顶条同一对齐
        .pl(px(15.))
        .pr(px(20.))
        .pt(px(10.))
        .pb(px(10.))
        .border_b_1()
        .border_color(rgb(t.border))
        .child(section_title(&tr("agents.global_settings")))
        .child(sw_row)
        .child(cap_item)
        .into_any_element()
}

pub(super) fn tools_dropdown(
    weak: &gpui::WeakEntity<Chat>,
    tools: &str,
    popup: Option<u8>,
    dd: gpui::Entity<DropdownState>,
    readonly: bool,
) -> gpui::AnyElement {
    let t = T();
    let open = popup == Some(1);
    let label = tool_tiers()
        .iter()
        .find(|(v, _)| *v == tools)
        .map(|(v, _)| (*v).to_string())
        .unwrap_or_else(|| tools.to_string());
    let weak_toggle = weak.clone();
    let weak_dismiss = weak.clone();
    let weak_out = weak.clone();
    let trigger = sel_trigger(&label, open);
    let ro_toggle = readonly;
    crate::ui::dropdown(
        "ag-tools-dd",
        &dd,
        open && !readonly,
        move |_, cx| {
            if ro_toggle {
                return;
            }
            let _ = weak_toggle.update(cx, |c, cx| {
                if let Some(st) = c.dlg.settings.clone() {
                    st.update(cx, |s, cx| {
                        s.ag_popup = if s.ag_popup == Some(1) { None } else { Some(1) };
                        cx.notify();
                    });
                }
            });
        },
        move |_, cx| {
            let _ = weak_dismiss.update(cx, |c, cx| {
                if let Some(st) = c.dlg.settings.clone() {
                    st.update(cx, |s, cx| {
                        if s.ag_popup.take().is_some() {
                            cx.notify();
                        }
                    });
                }
            });
        },
        trigger,
        move || {
            let mut menu = div()
                .w(px(250.))
                .py(px(3.))
                .bg(rgb(t.bg))
                .border_1()
                .border_color(rgb(t.border))
                .rounded(px(8.))
                .shadow_lg()
                .overflow_hidden();
            for (value, desc) in tool_tiers() {
                let selected = value == tools;
                let mut item = dd_item_with_desc(
                    weak_out.clone(),
                    value.to_string(),
                    desc.to_string(),
                    selected,
                    Some(value.to_string()),
                    1,
                );
                let _ = &mut item;
                menu = menu.child(item);
            }
            menu.into_any_element()
        },
    )
}

/// 下拉项（原型 `.dd-it`）：左 ✓ 占位 + 名称。
fn dd_item(
    weak: gpui::WeakEntity<Chat>,
    label: String,
    selected: bool,
    value: Option<String>,
    which: u8,
) -> gpui::AnyElement {
    dd_item_inner(weak, label, String::new(), selected, value, which)
}

/// 下拉项带右侧描述（工具档位用）。
fn dd_item_with_desc(
    weak: gpui::WeakEntity<Chat>,
    label: String,
    desc: String,
    selected: bool,
    value: Option<String>,
    which: u8,
) -> gpui::AnyElement {
    dd_item_inner(weak, label, desc, selected, value, which)
}

fn dd_item_inner(
    weak: gpui::WeakEntity<Chat>,
    label: String,
    desc: String,
    selected: bool,
    value: Option<String>,
    which: u8,
) -> gpui::AnyElement {
    let t = T();
    div()
        .id(SharedString::from(format!("ag-ddit-{which}-{label}")))
        .h(px(30.))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(crate::appearance::ui_size(12.))
        .text_color(rgb(if selected { t.accent } else { t.text }))
        .bg(rgb(if selected { t.bg_selected } else { t.bg }))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| {
                if let Some(st) = c.dlg.settings.clone() {
                    let val = value.clone().unwrap_or_default();
                    st.update(cx, |s, cx| {
                        match which {
                            0 => s.ag_model = if val.is_empty() { None } else { Some(val) },
                            1 => s.ag_tools = val,
                            _ => {}
                        }
                        s.ag_popup = None;
                        s.ag_dirty = true;
                        cx.notify();
                    });
                }
            });
        })
        .child(div().w(px(12.)).flex_shrink_0().when(selected, |d| {
            d.child(
                div()
                    .text_size(crate::appearance::ui_size(12.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(rgb(t.accent))
                    .child("✓"),
            )
        }))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(SharedString::from(label)),
        )
        .when(!desc.is_empty(), |d| {
            d.child(
                div()
                    .flex_shrink_0()
                    .text_size(crate::appearance::ui_size(11.))
                    .text_color(rgb(t.text_faint))
                    .child(SharedString::from(desc)),
            )
        })
        .into_any_element()
}
