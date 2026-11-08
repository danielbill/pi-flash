//! 子代理表单的 InputPanel 同款菜单（2026-11-07 对齐定稿）：模型/思考/工具
//! 档/ext/mcp 五个下拉。复用清单——
//! - 触发钮 = composer 操作栏 [`bar_trigger`]（InputPanel 五颗钮同一份样式）
//! - 思考档位表 / 工具预设档位表 / pill 行 = [`crate::session::menu_rows`]
//!   （composer 胶囊菜单同一份）
//! - 模型列表体 = [`model_menu_body`]（composer 模型菜单同一份：分组、
//!   置顶序、五角星、✓ 槽位、过滤阈值）
//! - ext / mcp 勾选行与头 = pi_extension_picker / mcp_picker 同款组件
//! - **弹层壳与定位** = composer picker 同一套：`picker_card` +
//!   `pill_anchor_edges`（唯一摆放算法：按钮水平居中 + 半屏定向）+ 槽位
//!   tracker，卡片挂 overlay 层（`shell/render/overlay.rs::overlay_stack`，
//!   本文件只出 [`ag_menus_overlay`] 一个挂载入口）
//! 行为差异只在写入目标：composer 写 runtime，这里写 SettingsPanel 表单字段。

use gpui::{AnyElement, Div, MouseButton, SharedString, div, prelude::*, px, rgb};

use super::*;
use crate::session::input::bar_trigger;
use crate::session::mcp_picker::{mcp_menu_header, mcp_menu_items, mcp_row};
use crate::session::menu_rows::{PillRow, pill_row, thinking_rows, tools_preset_rows};
use crate::session::model_picker::{
    MODEL_FILTER_MIN, group_header_row, model_menu_body, model_menu_content_h,
};
use crate::session::pi_extension_picker::{ext_menu_header, ext_menu_items, plugin_row};
use crate::session::picker_shell::{
    PAD, PillCardPos, ROW_H, capped_list, capped_list_h, empty_row, picker_card, pill_anchor_edges,
};

type BoundsSlot = std::rc::Rc<std::cell::RefCell<Option<gpui::Bounds<gpui::Pixels>>>>;

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

/// 表单字段写入（**勾选清单**用：composer 语义 = 勾选即生效且菜单不关，
/// 2026-11-08 与 ext/mcp/skills 三清单对齐；改值 + 脏标，不收弹层）。
fn form_edit(
    c: &mut Chat,
    cx: &mut gpui::Context<Chat>,
    f: impl FnOnce(&mut SettingsPanel, &mut gpui::Context<SettingsPanel>),
) {
    if let Some(st) = c.dlg.settings.clone() {
        let _ = st.update(cx, |s, cx| {
            s.ag_dirty = true;
            f(s, cx);
            cx.notify();
        });
    }
    cx.notify();
}

// ---------------------------------------------------------------------------
// 触发钮（渲染在表单行内；弹层本体在 overlay 层，见 ag_menus_overlay）
// ---------------------------------------------------------------------------

/// 触发钮通用装配：InputPanel bar_trigger + bounds tracker（paint 期捕获，
/// 弹层统一定位的测量端）+ 点击开合 ag_popup 槽位（锚点 = 本钮）。
/// `disabled`（内置只读 / ext 非 full+）不挂点击也不挂 tracker。
fn ag_trigger(
    weak: &gpui::WeakEntity<Chat>,
    panel: &gpui::Entity<SettingsPanel>,
    slot: u8,
    bounds: &BoundsSlot,
    disabled: bool,
    trigger: gpui::Stateful<gpui::Div>,
) -> AnyElement {
    if disabled {
        return trigger.into_any_element();
    }
    let weak = weak.clone();
    let panel = panel.clone();
    let bounds_click = bounds.clone();
    trigger
        .on_mouse_down(
            MouseButton::Left,
            move |ev: &gpui::MouseDownEvent, _, cx| {
                let at = ev.position;
                let _ = weak.update(cx, |c, cx| {
                    // inputpanel 同款：锚点 = 按钮槽位 bounds（未就绪退点击坐标）
                    c.dlg.pill_anchor = Some(crate::PillBtns::anchor(&bounds_click, at));
                    let _ = panel.update(cx, |s, cx| {
                        s.ag_popup = if s.ag_popup == Some(slot) {
                            None
                        } else {
                            Some(slot)
                        };
                        cx.notify();
                    });
                    cx.notify();
                });
            },
        )
        // 统一定位测量端：抓按钮 bounds（paint 期，每帧覆盖）
        .child(crate::PillBtns::tracker(bounds))
        .into_any_element()
}

/// 「模型」行控件：模型触发钮 + 思考强度触发钮（InputPanel 同款并排）。
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
        .child(model_trigger(chat, panel, weak, d, readonly))
        .child(thinking_trigger(chat, panel, weak, d, readonly))
        .into_any_element()
}

fn model_trigger(
    chat: &Chat,
    panel: &gpui::Entity<SettingsPanel>,
    weak: &gpui::WeakEntity<Chat>,
    d: &SettingsFormData,
    readonly: bool,
) -> AnyElement {
    let label = d
        .ag_model
        .clone()
        .unwrap_or_else(|| tr("agents.inherit").to_string());
    ag_trigger(
        weak,
        panel,
        0,
        &chat.dlg.pill_btn.ag_model,
        readonly,
        bar_trigger(
            "ag-model-trg",
            Some("cpu-ai"),
            &label,
            true,
            trg_color(readonly, d.ag_popup == Some(0)),
            !readonly,
        ),
    )
}

fn thinking_trigger(
    chat: &Chat,
    panel: &gpui::Entity<SettingsPanel>,
    weak: &gpui::WeakEntity<Chat>,
    d: &SettingsFormData,
    readonly: bool,
) -> AnyElement {
    ag_trigger(
        weak,
        panel,
        2,
        &chat.dlg.pill_btn.ag_thinking,
        readonly,
        bar_trigger(
            "ag-thinking-trg",
            Some("lightbulb"),
            &d.ag_thinking.clone(),
            true,
            trg_color(readonly, d.ag_popup == Some(2)),
            !readonly,
        ),
    )
}

/// 「工具」行控件：档位 + ext + mcp 三触发钮（composer 工具胶囊三件套
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
        .child(tier_trigger(chat, panel, weak, d, readonly))
        .child(ext_trigger(chat, panel, weak, d, readonly))
        .child(mcp_trigger(chat, panel, weak, d, readonly))
        .into_any_element()
}

fn tier_trigger(
    chat: &Chat,
    panel: &gpui::Entity<SettingsPanel>,
    weak: &gpui::WeakEntity<Chat>,
    d: &SettingsFormData,
    readonly: bool,
) -> AnyElement {
    ag_trigger(
        weak,
        panel,
        1,
        &chat.dlg.pill_btn.ag_tier,
        readonly,
        bar_trigger(
            "ag-tier-trg",
            Some("wrench"),
            &d.ag_tools.clone(),
            true,
            trg_color(readonly, d.ag_popup == Some(1)),
            !readonly,
        ),
    )
}

fn ext_trigger(
    chat: &Chat,
    panel: &gpui::Entity<SettingsPanel>,
    weak: &gpui::WeakEntity<Chat>,
    d: &SettingsFormData,
    readonly: bool,
) -> AnyElement {
    let full_plus = d.ag_tools == "full+";
    let disabled = readonly || !full_plus;
    ag_trigger(
        weak,
        panel,
        3,
        &chat.dlg.pill_btn.ag_ext,
        disabled,
        bar_trigger(
            "ag-ext-trg",
            Some("plug"),
            &format!("[{}]", d.ag_extensions.len()),
            false,
            trg_color(disabled, d.ag_popup == Some(3)),
            !disabled,
        ),
    )
}

/// 技能触发钮（继承 | 自定义，2026-11-08 设计对齐）：档外默认继承；
/// 自定义 = ag_skills 非空。菜单见 [`skills_card`]。
pub(super) fn skills_trigger(
    chat: &Chat,
    panel: &gpui::Entity<SettingsPanel>,
    weak: &gpui::WeakEntity<Chat>,
    d: &SettingsFormData,
    readonly: bool,
) -> AnyElement {
    let custom = !d.ag_skills.is_empty();
    ag_trigger(
        weak,
        panel,
        5,
        &chat.dlg.pill_btn.ag_skills,
        readonly,
        bar_trigger(
            "ag-skills-trg",
            Some("wand"),
            &if custom {
                tr("agents.skills_custom").to_string()
            } else {
                tr("agents.inherit").to_string()
            },
            true,
            trg_color(readonly, d.ag_popup == Some(5)),
            !readonly,
        ),
    )
}

fn mcp_trigger(
    chat: &Chat,
    panel: &gpui::Entity<SettingsPanel>,
    weak: &gpui::WeakEntity<Chat>,
    d: &SettingsFormData,
    readonly: bool,
) -> AnyElement {
    ag_trigger(
        weak,
        panel,
        4,
        &chat.dlg.pill_btn.ag_mcp,
        readonly,
        bar_trigger(
            "ag-mcp-trg",
            Some("server"),
            &format!("[{}]", d.ag_mcp.len()),
            false,
            trg_color(readonly, d.ag_popup == Some(4)),
            !readonly,
        ),
    )
}

// ---------------------------------------------------------------------------
// 弹层挂载入口（overlay_stack 调用；卡片定位走 pill_anchor_edges 唯一落点）
// ---------------------------------------------------------------------------

/// 子代理页五菜单弹层：`Some = ag_popup 指向的菜单卡`。挂 settings 模态
/// 之后（后渲染者在上）；点外收走 menu_layer（遮挡不穿透），点卡片本身
/// 不关（stop_click），与 composer picker 行为一致。
pub(crate) fn ag_menus_overlay(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    d: &SettingsFormData,
    window: &gpui::Window,
) -> Option<AnyElement> {
    // 模态必须在场（menu_layer 的点外收起要写它的 ag_popup）
    chat.dlg.settings.as_ref()?;
    let slot = d.ag_popup?;
    let t = T();
    // 菜单宽（锚点槽位在触发钮侧，卡片只按 ag_popup 配宽）
    let menu_w = match slot {
        0 => 380., // 模型
        1 => 320., // 工具档
        2 => 320., // 思考
        3 => 420., // ext
        4 => 320., // mcp
        5 => 380., // 技能
        _ => return None,
    };
    // 唯一定位落点（composer picker 同款：按钮居中 + 半屏定向）
    let vp = window.viewport_size();
    let pos: PillCardPos = pill_anchor_edges(chat.dlg.pill_anchor, vp, px(menu_w));
    let content: Div = match slot {
        0 => model_card(chat, weak, d, t),
        1 => tier_card(weak, d),
        2 => thinking_card(weak, d),
        3 => ext_card(chat, weak, d, t),
        4 => mcp_card(chat, weak, d, t),
        5 => skills_card(chat, weak, d, t),
        _ => return None,
    };
    let layer = crate::ui::menu::menu_layer(None, weak, |c, cx| {
        if let Some(st) = c.dlg.settings.clone() {
            let _ = st.update(cx, |s, cx| {
                if s.ag_popup.take().is_some() {
                    cx.notify();
                }
            });
        }
    });
    Some(
        layer
            .child(crate::ui::overlay::stop_click(
                picker_card(pos, px(menu_w), t).child(content),
            ))
            .into_any_element(),
    )
}

// ---------------------------------------------------------------------------
// 卡片内容（inputpanel 组件复用；宽随卡，样式同 composer picker 卡）
// ---------------------------------------------------------------------------

/// 模型卡：过滤框（≥10 模型）+「继承主代理模型」置顶行（分隔线隔离）+
/// composer 同款分组模型列表。
fn model_card(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    d: &SettingsFormData,
    t: &'static crate::appearance::theme::Theme,
) -> Div {
    // cx-free：cwd 传 None（子代理定义不绑项目），过滤走表单过滤框
    let (models, total) = chat.picker_models_from(None, Some(d.ag_filter_value.clone()));
    let show_filter = total >= MODEL_FILTER_MIN;
    let default_ref = chat
        .cfg
        .mc_default_model
        .as_ref()
        .map(|(p, id)| format!("{p}/{id}"));
    // 「继承主代理模型」置顶行
    let inherit = pill_row(
        weak,
        |c, cx| {
            form_pick(c, cx, |s, _| s.ag_model = None);
        },
        PillRow {
            key: "ag-inherit-main",
            label: tr("agents.inherit_main").to_string(),
            desc: String::new(),
            active: d.ag_model.is_none(),
        },
    );
    let pick: crate::session::model_picker::ModelPickAction = std::rc::Rc::new(|c, cx, p, id| {
        form_pick(c, cx, |s, _| s.ag_model = Some(format!("{p}/{id}")))
    });
    let mut body = model_menu_body(
        weak,
        &models,
        d.ag_model.as_deref(),
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
        .flex()
        .flex_col()
        // 过滤框按 composer 菜单同阈值出现
        .children(show_filter.then(|| {
            div()
                .px(px(PAD))
                .pt(px(PAD))
                .pb(px(PAD))
                .child(d.ag_filter.clone())
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
}

/// 思考卡：auto（= 不覆盖，落 pi 默认档）+ pi 七档，档位表复用 composer
/// 胶囊菜单。
fn thinking_card(weak: &gpui::WeakEntity<Chat>, d: &SettingsFormData) -> Div {
    let rows = thinking_rows(&d.ag_thinking.clone())
        .into_iter()
        .map(|row| {
            let weak = weak.clone();
            let key = row.key.to_string();
            pill_row(
                &weak,
                move |c, cx| form_pick(c, cx, |s, _| s.ag_thinking = key.clone()),
                row,
            )
        })
        .collect::<Vec<_>>();
    div().flex().flex_col().children(rows)
}

/// 工具档位卡：inherit（继承委派者）置顶 + composer 五档（键 custom ↔
/// profile 键 full+ 双向映射，其余直传）。
fn tier_card(weak: &gpui::WeakEntity<Chat>, d: &SettingsFormData) -> Div {
    let cur_menu = if d.ag_tools == "full+" {
        "custom"
    } else {
        d.ag_tools.as_str()
    };
    // inherit 置顶（composer 无此档——子代理专属语义），其余档位表与
    // composer 工具菜单同源
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
            let weak = weak.clone();
            let key = row.key.to_string();
            pill_row(
                &weak,
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
    div().flex().flex_col().children(items)
}

/// ext 勾选卡（full+ 档）：条目/勾选行/头 = composer 扩展菜单同款组件，
/// 勾选写 ag_extensions（勾选即存，无确认按钮）。
fn ext_card(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    d: &SettingsFormData,
    t: &'static crate::appearance::theme::Theme,
) -> Div {
    let is_sel = |s: &str| d.ag_extensions.iter().any(|x| x == s);
    let items = ext_menu_items(chat, &is_sel);
    let rows: Vec<AnyElement> = items
        .iter()
        .map(|(src, project)| {
            let tokens = chat.pool.ext_tokens.get(src).map(|e| e.ext.max(0) as u64);
            let src = src.clone();
            let pick = src.clone();
            plugin_row(
                weak,
                &src,
                is_sel(&src),
                *project,
                tokens,
                move |c, cx| {
                    form_edit(c, cx, |s, _| {
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
        .flex()
        .flex_col()
        .child(ext_menu_header(chat, &items, &is_sel))
        .child(capped_list(
            "ag-ext-list",
            &d.ag_ext_scroll,
            capped_list_h(n),
            body,
        ))
}

/// mcp 勾选卡（常驻）：条目/勾选行/头 = composer MCP 菜单同款组件，勾选
/// 写 ag_mcp。
fn mcp_card(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    d: &SettingsFormData,
    t: &'static crate::appearance::theme::Theme,
) -> Div {
    let is_sel = |s: &str| d.ag_mcp.iter().any(|x| x == s);
    let items = mcp_menu_items(chat, &is_sel);
    let rows: Vec<AnyElement> = items
        .iter()
        .map(|(name, transport)| {
            let name = name.clone();
            let pick = name.clone();
            mcp_row(
                weak,
                &name,
                is_sel(&name),
                transport,
                move |c, cx| {
                    form_edit(c, cx, |s, _| {
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
        .flex()
        .flex_col()
        .child(mcp_menu_header(enabled_n, items.len()))
        .child(capped_list(
            "ag-mcp-list",
            &d.ag_mcp_scroll,
            capped_list_h(n),
            body,
        ))
}

/// 技能卡（2026-11-08 设计对齐）：「继承」置顶 + 搜索框 / 分隔线 / 全选行 /
/// 技能列表（设置-技能页同一份列表 `mc_skills`，全局在前项目在后）。勾选
/// 即存且菜单不关；全选作用于过滤后可见集（全中 = 清可见，否则全加）。
fn skills_card(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    d: &SettingsFormData,
    t: &'static crate::appearance::theme::Theme,
) -> Div {
    let is_sel = |s: &str| d.ag_skills.iter().any(|x| x == s);
    // 过滤（名称包含，大小写不敏）；分组 = 全局 → 项目（2026-11-08 设计
    // 定稿：组头 + 分隔线，不再逐项挂「项目」标）
    let flt = d.ag_skill_filter_value.trim().to_lowercase();
    let filtered: Vec<&pi_link::skills::SkillEntry> = chat
        .cfg
        .mc_skills
        .iter()
        .filter(|e| flt.is_empty() || e.name.to_lowercase().contains(&flt))
        .collect();
    let global: Vec<&pi_link::skills::SkillEntry> = filtered
        .iter()
        .copied()
        .filter(|e| e.scope == pi_link::skills::SkillScope::Global)
        .collect();
    let project: Vec<&pi_link::skills::SkillEntry> = filtered
        .iter()
        .copied()
        .filter(|e| e.scope == pi_link::skills::SkillScope::Project)
        .collect();
    let items: Vec<&pi_link::skills::SkillEntry> = global
        .iter()
        .copied()
        .chain(project.iter().copied())
        .collect();
    let visible_sel = items.iter().filter(|e| is_sel(&e.name)).count();
    let all_selected = !items.is_empty() && visible_sel == items.len();

    // 「继承」置顶（✓ = 清单空）：点它清空清单并收卡（与模型卡同语义）
    let inherit = pill_row(
        weak,
        |c, cx| {
            form_pick(c, cx, |s, _| s.ag_skills.clear());
        },
        PillRow {
            key: "ag-skills-inherit",
            label: tr("agents.inherit").to_string(),
            desc: String::new(),
            active: d.ag_skills.is_empty(),
        },
    );

    // 全选行：checkbox + 全选（作用于可见集；点击不收卡）
    let weak_all = weak.clone();
    let all_items: Vec<String> = items.iter().map(|e| e.name.clone()).collect();
    let select_all = div()
        .id("ag-skills-all")
        .h(px(30.))
        .px(px(PAD))
        .flex()
        .items_center()
        .gap(px(8.))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            let all = all_items.clone();
            let _ = weak_all.update(cx, |c, cx| {
                form_edit(c, cx, |s, _| {
                    if all.iter().all(|n| s.ag_skills.iter().any(|x| x == n)) {
                        s.ag_skills.retain(|x| !all.iter().any(|n| n == x));
                    } else {
                        for n in all {
                            if !s.ag_skills.iter().any(|x| *x == n) {
                                s.ag_skills.push(n);
                            }
                        }
                    }
                });
            });
        })
        .child(crate::ui::kit::checkbox(all_selected))
        .child(
            div()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text))
                .child(SharedString::from(tr("agents.skills_select_all"))),
        )
        .when(visible_sel > 0, |row| {
            row.child(
                div()
                    .ml_auto()
                    .text_size(crate::appearance::ui_size(10.))
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(format!(
                        "{}/{}",
                        visible_sel,
                        items.len()
                    ))),
            )
        });

    // 分组列表：组头（composer 模型菜单同款 group_header_row）+ 分隔线，
    // 行不再挂项目标（组头即分组语义）。内容高按组头 0.8 行 + 技能行 1 行
    // 精确累加，超 10 行滚动（capped_list + 句柄）。
    let header = |label: &str| group_header_row(label, PAD, ROW_H * 0.8, t);
    let skill_row = |e: &pi_link::skills::SkillEntry| {
        let name = e.name.clone();
        let pick = name.clone();
        plugin_row(
            weak,
            &name,
            is_sel(&name),
            false, // 分组语义交给组头，行内不再挂「项目」标（2026-11-08）
            None,
            move |c, cx| {
                form_edit(c, cx, |s, _| {
                    match s.ag_skills.iter().position(|x| *x == pick) {
                        Some(ix) => {
                            s.ag_skills.remove(ix);
                        }
                        None => s.ag_skills.push(pick.clone()),
                    }
                });
            },
            t,
        )
    };
    let mut body: Vec<AnyElement> = Vec::new();
    let mut content_h = 0.;
    if !global.is_empty() {
        body.push(header(&tr("scope.global")));
        content_h += ROW_H * 0.8;
        for e in &global {
            body.push(skill_row(e));
            content_h += ROW_H;
        }
    }
    if !global.is_empty() && !project.is_empty() {
        body.push(
            div()
                .h(px(1.))
                .mx(px(PAD))
                .my(px(4.))
                .bg(rgb(t.border))
                .into_any_element(),
        );
        content_h += 9.;
    }
    if !project.is_empty() {
        body.push(header(&tr("scope.project")));
        content_h += ROW_H * 0.8;
        for e in &project {
            body.push(skill_row(e));
            content_h += ROW_H;
        }
    }
    if body.is_empty() {
        body.push(empty_row(SharedString::from(tr("agents.pick_empty"))));
        content_h += 60.;
    }

    div()
        .flex()
        .flex_col()
        .child(inherit)
        .child(div().h(px(1.)).mx(px(PAD)).my(px(4.)).bg(rgb(t.border)))
        // 搜索框顶置 + 分隔线 + 全选行，然后才是技能列表（设计定稿顺序）
        .child(
            div()
                .px(px(PAD))
                .pt(px(PAD))
                .pb(px(PAD))
                .child(d.ag_skill_filter.clone()),
        )
        .child(select_all)
        .child(div().h(px(1.)).mx(px(PAD)).bg(rgb(t.border)))
        .child(capped_list(
            "ag-skills-list",
            &d.ag_skills_scroll,
            content_h.min(300.),
            body,
        ))
}
