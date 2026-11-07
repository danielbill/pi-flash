//! 子代理表单的 InputPanel 同款菜单（2026-11-07 对齐定稿）：模型/思考/工具
//! 档/ext/mcp 五个下拉。复用清单——
//! - 触发钮 = composer 操作栏 [`bar_trigger`]（InputPanel 五颗钮同一份样式）
//! - 思考档位表 / 工具预设档位表 / pill 行 = [`crate::session::menu_rows`]
//!   （composer 胶囊菜单同一份）
//! - 模型列表体 = [`model_menu_body`]（composer 模型菜单同一份：分组、
//!   置顶序、五角星、✓ 槽位、过滤阈值）
//! - ext / mcp 勾选行与头 = pi_extension_picker / mcp_picker 同款组件
//!   （[`ext_menu_items`] / [`plugin_row`] / [`mcp_menu_items`] / [`mcp_row`]）
//! 弹层壳 = 设置页统一 [`crate::ui::dropdown`]（anchored + 外点收起守卫）。
//! 行为差异只在写入目标：composer 写 runtime，这里写 SettingsPanel 表单字段。

use gpui::{AnyElement, SharedString, div, prelude::*, px, rgb};

use super::*;
use crate::session::input::bar_trigger;
use crate::session::mcp_picker::{mcp_menu_header, mcp_menu_items, mcp_row};
use crate::session::menu_rows::{PillRow, pill_row, thinking_rows, tools_preset_rows};
use crate::session::model_picker::{MODEL_FILTER_MIN, model_menu_body, model_menu_content_h};
use crate::session::pi_extension_picker::{ext_menu_header, ext_menu_items, plugin_row};
use crate::session::picker_shell::{PAD, capped_list, capped_list_h, empty_row};

/// 触发钮色：锁定 = text_faint、菜单开 = accent、常态 = text_muted。
fn trg_color(disabled: bool, open: bool) -> u32 {
    let t = T();
    if disabled {
        t.text_faint
    } else if open {
        t.accent
    } else {
        t.text_muted
    }
}

/// 开合写 ag_popup 槽位（dropdown 的 on_toggle 回调；锁定时早退不开）。
fn toggle_cb(
    panel: &gpui::Entity<SettingsPanel>,
    slot: u8,
    disabled: bool,
) -> impl Fn(&mut gpui::Window, &mut gpui::App) + 'static {
    let panel = panel.clone();
    move |_, cx| {
        if disabled {
            return;
        }
        let _ = panel.update(cx, |s, cx| {
            s.ag_popup = if s.ag_popup == Some(slot) {
                None
            } else {
                Some(slot)
            };
            cx.notify();
        });
    }
}

/// 外点收起（dropdown 的 on_dismiss 回调；五个下拉共享一个守卫）。
fn dismiss_cb(
    panel: &gpui::Entity<SettingsPanel>,
) -> impl Fn(&mut gpui::Window, &mut gpui::App) + 'static {
    let panel = panel.clone();
    move |_, cx| {
        let _ = panel.update(cx, |s, cx| {
            if s.ag_popup.take().is_some() {
                cx.notify();
            }
        });
    }
}

/// 表单字段写入（菜单行回调共用）：写 ag_dirty + 关本下拉 + notify。
fn form_pick(
    c: &mut Chat,
    cx: &mut gpui::Context<Chat>,
    f: impl FnOnce(&mut SettingsPanel, &mut gpui::Context<SettingsPanel>),
) {
    if let Some(st) = c.dlg.settings.clone() {
        let _ = st.update(cx, |s, cx| {
            s.ag_dirty = true;
            s.ag_popup = None;
            f(s, cx);
            cx.notify();
        });
    }
    cx.notify();
}

// ---------------------------------------------------------------------------
// 模型 + 思考（一行两钮）
// ---------------------------------------------------------------------------

/// 「模型」行控件：模型下拉 + 思考强度下拉（InputPanel 同款并排）。
pub(super) fn model_thinking_control(
    chat: &Chat,
    panel: &gpui::Entity<SettingsPanel>,
    weak: &gpui::WeakEntity<Chat>,
    d: &SettingsFormData,
    readonly: bool,
) -> gpui::AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(15.)) // 与 composer 操作栏组内间距同一档
        .child(model_dd(chat, panel, weak, d, readonly))
        .child(thinking_dd(panel, weak, d, readonly))
        .into_any_element()
}

/// 模型下拉：触发钮显示当前模型（None = 继承）+ 菜单 = 继承主代理模型置顶
/// （分隔线隔离）+ 原有分组模型菜单（composer 同款，含过滤框）。
fn model_dd(
    chat: &Chat,
    panel: &gpui::Entity<SettingsPanel>,
    weak: &gpui::WeakEntity<Chat>,
    d: &SettingsFormData,
    readonly: bool,
) -> AnyElement {
    let open = d.ag_popup == Some(0) && !readonly;
    let label = d
        .ag_model
        .clone()
        .unwrap_or_else(|| tr("agents.inherit").to_string());
    let trigger = bar_trigger(
        "ag-model-trg",
        None,
        13.,
        &label,
        true,
        trg_color(readonly, d.ag_popup == Some(0)),
        !readonly,
    )
    .into_any_element();
    let popup_weak = weak.clone();
    let popup_d = d.clone();
    let popup_chat = chat; // 'static 无关：popup 闭包无生命周期约束，借用即可
    let default_ref = chat
        .cfg
        .mc_default_model
        .as_ref()
        .map(|(p, id)| format!("{p}/{id}"));
    crate::ui::dropdown(
        "ag-model-dd",
        &d.ag_dd,
        open,
        toggle_cb(panel, 0, readonly),
        dismiss_cb(panel),
        trigger,
        move || {
            let t = T();
            // cx-free：cwd 传 None（子代理定义不绑项目），过滤走表单过滤框；
            // 只在弹层真正渲染的帧才算（收起态零开销）
            let (models, total) =
                popup_chat.picker_models_from(None, Some(popup_d.ag_filter_value.clone()));
            let show_filter = total >= MODEL_FILTER_MIN;
            // 「继承主代理模型」置顶行（分隔线隔离，下面是原有菜单）
            let inherit = pill_row(
                &popup_weak,
                |c, cx| {
                    form_pick(c, cx, |s, _| s.ag_model = None);
                },
                PillRow {
                    key: "ag-inherit-main",
                    label: tr("agents.inherit_main").to_string(),
                    desc: String::new(),
                    active: popup_d.ag_model.is_none(),
                },
            );
            let pick: crate::session::model_picker::ModelPickAction =
                std::rc::Rc::new(|c, cx, p, id| {
                    form_pick(c, cx, |s, _| s.ag_model = Some(format!("{p}/{id}")))
                });
            let mut body = model_menu_body(
                &popup_weak,
                &models,
                popup_d.ag_model.as_deref(),
                default_ref.as_deref(),
                |_| false, // 无键盘导航态
                Some(pick),
                t,
            );
            // 列表内容高：内容不到按内容高，超出截 10 行滚动；空态提示行
            let list_h = if models.is_empty() {
                body = body.child(empty_row(SharedString::from(tr("action.no_models_match"))));
                60.
            } else {
                model_menu_content_h(&models).min(300.)
            };
            div()
                .w(px(380.))
                .rounded(px(8.))
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.bg))
                .shadow_lg()
                .overflow_hidden()
                .flex()
                .flex_col()
                // 过滤框按 composer 菜单同阈值出现
                .children(show_filter.then(|| {
                    div()
                        .px(px(PAD))
                        .pt(px(PAD))
                        .pb(px(PAD))
                        .child(popup_d.ag_filter.clone())
                }))
                .child(inherit)
                .child(div().h(px(1.)).mx(px(PAD)).my(px(4.)).bg(rgb(t.border)))
                .child(
                    div()
                        .id("ag-model-list")
                        .h(px(list_h))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .pb(px(PAD))
                        .pr(px(10.))
                        .child(body),
                )
                .into_any_element()
        },
    )
    .into_any_element()
}

/// 思考强度下拉：auto（= 不覆盖，落 pi 默认档）+ pi 七档，档位表复用
/// composer 胶囊菜单。
fn thinking_dd(
    panel: &gpui::Entity<SettingsPanel>,
    weak: &gpui::WeakEntity<Chat>,
    d: &SettingsFormData,
    readonly: bool,
) -> AnyElement {
    let open = d.ag_popup == Some(2) && !readonly;
    let trigger = bar_trigger(
        "ag-thinking-trg",
        Some("lightbulb"),
        13.,
        &d.ag_thinking.clone(),
        true,
        trg_color(readonly, d.ag_popup == Some(2)),
        !readonly,
    )
    .into_any_element();
    let popup_weak = weak.clone();
    let cur = d.ag_thinking.clone();
    crate::ui::dropdown(
        "ag-thinking-dd",
        &d.ag_dd,
        open,
        toggle_cb(panel, 2, readonly),
        dismiss_cb(panel),
        trigger,
        move || {
            let t = T();
            let rows = thinking_rows(&cur)
                .into_iter()
                .map(|row| {
                    let popup_weak = popup_weak.clone();
                    let key = row.key.to_string();
                    pill_row(
                        &popup_weak,
                        move |c, cx| form_pick(c, cx, |s, _| s.ag_thinking = key.clone()),
                        row,
                    )
                })
                .collect::<Vec<_>>();
            div()
                .w(px(320.))
                .rounded(px(8.))
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.bg))
                .shadow_lg()
                .overflow_hidden()
                .flex()
                .flex_col()
                .pt(px(PAD))
                .pb(px(PAD))
                .children(rows)
                .into_any_element()
        },
    )
    .into_any_element()
}

// ---------------------------------------------------------------------------
// 工具（一行三钮：工具档 / ext / mcp）
// ---------------------------------------------------------------------------

/// 「工具」行控件：档位下拉 + ext 勾选 + mcp 勾选（composer 工具胶囊三件套
/// 同款；ext 仅 full+ 档激活，mcp 常驻）。
pub(super) fn tools_control(
    chat: &Chat,
    panel: &gpui::Entity<SettingsPanel>,
    weak: &gpui::WeakEntity<Chat>,
    d: &SettingsFormData,
    readonly: bool,
) -> gpui::AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(15.))
        .child(tier_dd(panel, weak, d, readonly))
        .child(ext_dd(chat, panel, weak, d, readonly))
        .child(mcp_dd(chat, panel, weak, d, readonly))
        .into_any_element()
}

/// 工具档位下拉：inherit（继承委派者）置顶 + composer 五档（键 custom ↔
/// profile 键 full+ 双向映射，其余直传）。
fn tier_dd(
    panel: &gpui::Entity<SettingsPanel>,
    weak: &gpui::WeakEntity<Chat>,
    d: &SettingsFormData,
    readonly: bool,
) -> AnyElement {
    let open = d.ag_popup == Some(1) && !readonly;
    let trigger = bar_trigger(
        "ag-tier-trg",
        Some("wrench"),
        13.,
        &d.ag_tools.clone(),
        true,
        trg_color(readonly, d.ag_popup == Some(1)),
        !readonly,
    )
    .into_any_element();
    let popup_weak = weak.clone();
    let cur_menu = if d.ag_tools == "full+" {
        "custom"
    } else {
        d.ag_tools.as_str()
    };
    crate::ui::dropdown(
        "ag-tier-dd",
        &d.ag_dd,
        open,
        toggle_cb(panel, 1, readonly),
        dismiss_cb(panel),
        trigger,
        move || {
            let t = T();
            // inherit 置顶（composer 无此档——子代理专属语义），其余档位表
            // 与 composer 工具菜单同源
            let mut rows = vec![PillRow {
                key: "inherit",
                label: tr("agents.inherit_tools").to_string(),
                desc: String::new(),
                active: cur_menu == "inherit",
            }];
            rows.extend(tools_preset_rows(cur_menu));
            let items = rows
                .into_iter()
                .map(|row| {
                    let popup_weak = popup_weak.clone();
                    let key = row.key.to_string();
                    pill_row(
                        &popup_weak,
                        move |c, cx| {
                            // 菜单键 custom = profile 键 full+；其余直传
                            let v = if key == "custom" {
                                "full+".to_string()
                            } else {
                                key.clone()
                            };
                            form_pick(c, cx, |s, _| s.ag_tools = v);
                        },
                        row,
                    )
                })
                .collect::<Vec<_>>();
            div()
                .w(px(320.))
                .rounded(px(8.))
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.bg))
                .shadow_lg()
                .overflow_hidden()
                .flex()
                .flex_col()
                .pt(px(PAD))
                .pb(px(PAD))
                .children(items)
                .into_any_element()
        },
    )
    .into_any_element()
}

/// ext 勾选下拉（full+ 档）：条目/勾选行/头 = composer 扩展菜单同款组件，
/// 勾选写 ag_extensions（勾选即存，无确认按钮）。
fn ext_dd(
    chat: &Chat,
    panel: &gpui::Entity<SettingsPanel>,
    weak: &gpui::WeakEntity<Chat>,
    d: &SettingsFormData,
    readonly: bool,
) -> AnyElement {
    let full_plus = d.ag_tools == "full+";
    let disabled = readonly || !full_plus;
    let open = d.ag_popup == Some(3) && !disabled;
    let label = format!("[{}]", d.ag_extensions.len());
    let trigger = bar_trigger(
        "ag-ext-trg",
        Some("plug"),
        16., // plug 特档：2026-10-15 用户定夺（glyph 窄长补视觉质量）
        &label,
        false,
        trg_color(disabled, d.ag_popup == Some(3)),
        !disabled,
    )
    .into_any_element();
    let popup_weak = weak.clone();
    let selected = d.ag_extensions.clone();
    crate::ui::dropdown(
        "ag-ext-dd",
        &d.ag_dd,
        open,
        toggle_cb(panel, 3, disabled),
        dismiss_cb(panel),
        trigger,
        move || {
            let t = T();
            let is_sel = |s: &str| selected.iter().any(|x| x == s);
            let items = ext_menu_items(chat, &is_sel);
            let rows: Vec<AnyElement> = items
                .iter()
                .map(|(src, project)| {
                    let tokens = chat.pool.ext_tokens.get(src).map(|e| e.ext.max(0) as u64);
                    let src = src.clone();
                    let pick = src.clone();
                    plugin_row(
                        &popup_weak,
                        &src,
                        is_sel(&src),
                        *project,
                        tokens,
                        move |c, cx| {
                            form_pick(c, cx, |s, _| {
                                match s.ag_extensions.iter().position(|x| *x == pick) {
                                    Some(ix) => {
                                        s.ag_extensions.remove(ix);
                                    }
                                    None => s.ag_extensions.push(pick.clone()),
                                }
                            });
                        },
                        t,
                    )
                })
                .collect();
            let n = rows.len();
            let mut body = rows;
            if body.is_empty() {
                body.push(empty_row(SharedString::from(tr(
                    "extensions.not_configured",
                ))));
            }
            div()
                .w(px(420.))
                .rounded(px(8.))
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.bg))
                .shadow_lg()
                .overflow_hidden()
                .flex()
                .flex_col()
                .child(ext_menu_header(chat, &items, &is_sel))
                .child(capped_list(
                    "ag-ext-list",
                    &d.ag_ext_scroll,
                    capped_list_h(n),
                    body,
                ))
                .into_any_element()
        },
    )
    .into_any_element()
}

/// mcp 勾选下拉（常驻）：条目/勾选行/头 = composer MCP 菜单同款组件，
/// 勾选写 ag_mcp。
fn mcp_dd(
    chat: &Chat,
    panel: &gpui::Entity<SettingsPanel>,
    weak: &gpui::WeakEntity<Chat>,
    d: &SettingsFormData,
    readonly: bool,
) -> AnyElement {
    let open = d.ag_popup == Some(4) && !readonly;
    let label = format!("[{}]", d.ag_mcp.len());
    let trigger = bar_trigger(
        "ag-mcp-trg",
        Some("server"),
        13.,
        &label,
        false,
        trg_color(readonly, d.ag_popup == Some(4)),
        !readonly,
    )
    .into_any_element();
    let popup_weak = weak.clone();
    let selected = d.ag_mcp.clone();
    crate::ui::dropdown(
        "ag-mcp-dd",
        &d.ag_dd,
        open,
        toggle_cb(panel, 4, readonly),
        dismiss_cb(panel),
        trigger,
        move || {
            let t = T();
            let is_sel = |s: &str| selected.iter().any(|x| x == s);
            let items = mcp_menu_items(chat, &is_sel);
            let rows: Vec<AnyElement> = items
                .iter()
                .map(|(name, transport)| {
                    let name = name.clone();
                    let pick = name.clone();
                    mcp_row(
                        &popup_weak,
                        &name,
                        is_sel(&name),
                        transport,
                        move |c, cx| {
                            form_pick(c, cx, |s, _| {
                                match s.ag_mcp.iter().position(|x| *x == pick) {
                                    Some(ix) => {
                                        s.ag_mcp.remove(ix);
                                    }
                                    None => s.ag_mcp.push(pick.clone()),
                                }
                            });
                        },
                        t,
                    )
                })
                .collect();
            let n = rows.len();
            let mut body = rows;
            if body.is_empty() {
                body.push(empty_row(SharedString::from(tr("mcp.none_configured"))));
            }
            let enabled_n = chat
                .cfg
                .mcp_servers
                .iter()
                .filter(|s| s.scope == pi_link::mcp::Scope::Global && s.enabled)
                .count();
            div()
                .w(px(320.))
                .rounded(px(8.))
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.bg))
                .shadow_lg()
                .overflow_hidden()
                .flex()
                .flex_col()
                .child(mcp_menu_header(enabled_n, items.len()))
                .child(capped_list(
                    "ag-mcp-list",
                    &d.ag_mcp_scroll,
                    capped_list_h(n),
                    body,
                ))
                .into_any_element()
        },
    )
    .into_any_element()
}
