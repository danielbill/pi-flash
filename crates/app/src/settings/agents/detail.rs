//! 子代理右表单：agent_detail 全字段装配（名称/模型/提示词/轮数等）
//! 与 detail_shell / turns_seg 构件。

use super::dropdowns::{model_dropdown, pick_list, tools_dropdown};
use super::*;

/// 右栏表单：新建 / 损坏 / 内置只读 / 自定义可编辑 四态。
pub(super) fn agent_detail(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
    d: &SettingsFormData,
) -> gpui::AnyElement {
    let t = T();
    let SettingsFormData {
        ag_name,
        ag_desc,
        ag_prompt,
        ag_turns_n,
        ag_model,
        ag_tools,
        ag_turns,
        ag_inject,
        ag_extensions,
        ag_mcp,
        ag_skills,
        ag_popup,
        ag_dd,
        ag_dirty,
        ag_name_value,
        ag_desc_value,
        ag_prompt_value,
        ..
    } = d;

    // 新建态：空表单（可编辑）
    let (agent, is_new) = if section == AG_NEW {
        (None, true)
    } else {
        (
            chat.cfg.mc_agents.iter().find(|a| a.id == section).cloned(),
            false,
        )
    };
    let builtin = agent
        .as_ref()
        .map(|a| !a.origin.is_custom())
        .unwrap_or(false);
    let readonly = builtin; // 内置只读灰化
    let broken = agent.as_ref().and_then(|a| a.error.clone());

    let mut head = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .flex_shrink_0()
        .child(
            div()
                .w(px(96.))
                .flex_shrink_0()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text))
                .child(tr("agents.name")),
        )
        .child({
            // 只读态渲染静态文本（TextInput 无禁用 API，原型 .frm.ro 整表单禁交互）
            if readonly {
                div()
                    .w(px(250.))
                    .h(px(30.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .opacity(0.62)
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_size(crate::appearance::ui_size(12.))
                    .text_color(rgb(t.text))
                    .child(SharedString::from(ag_name_value.clone()))
            } else {
                div().w(px(250.)).flex_shrink_0().child(ag_name.clone())
            }
        })
        .child(div().flex_1())
        .when(!is_new, |d| {
            // 删除（仅自定义）/ 复制为自定义（仅内置）
            if builtin {
                d.child(config_button(
                    "ag-copy",
                    weak,
                    &tr("agents.copy_custom"),
                    Btn::Secondary,
                    true,
                    false,
                    {
                        let id = agent.as_ref().map(|a| a.id.clone()).unwrap_or_default();
                        move |c, cx| c.mc_agent_copy_builtin(id.clone(), cx)
                    },
                ))
            } else {
                d.child(config_button(
                    "ag-del",
                    weak,
                    &tr("agents.delete"),
                    Btn::Danger,
                    true,
                    false,
                    {
                        let id = agent.as_ref().map(|a| a.id.clone()).unwrap_or_default();
                        move |c, cx| c.mc_agent_delete(id.clone(), cx)
                    },
                ))
            }
        })
        .when(is_new || !builtin, |d| {
            d.child(config_button(
                "ag-save",
                weak,
                &tr("agents.save"),
                Btn::Primary,
                true,
                false,
                |c, cx| c.mc_agent_save(cx),
            ))
        });

    let dirty_dot = div()
        .size(px(6.))
        .rounded_full()
        .flex_shrink_0()
        .bg(rgb(t.accent));
    if *ag_dirty && !readonly {
        head = head.child(dirty_dot);
    }

    let mut form = div().flex().flex_col().gap(px(12.)).mt(px(12.)).flex_1();

    // 损坏态：红条 + 打开文件修复 + 删除
    if let Some(err) = broken {
        form = form
            .child(error_note(&format!("{}：{err}", tr("agents.broken"))))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(config_button(
                        "ag-fix",
                        weak,
                        &tr("agents.open_fix"),
                        Btn::Secondary,
                        true,
                        false,
                        {
                            let path = agent
                                .as_ref()
                                .and_then(|a| a.path.clone())
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_default();
                            move |c, cx| c.mc_agent_open_fix(path.clone(), cx)
                        },
                    ))
                    .child(config_button(
                        "ag-del-broken",
                        weak,
                        &tr("agents.delete"),
                        Btn::Danger,
                        true,
                        false,
                        {
                            let id = agent.as_ref().map(|a| a.id.clone()).unwrap_or_default();
                            move |c, cx| c.mc_agent_delete(id.clone(), cx)
                        },
                    )),
            );
        return detail_shell_with(head, form);
    }

    // 描述
    form = form.child(agent_row(
        &tr("agents.desc"),
        if readonly {
            div()
                .w_full()
                .opacity(0.62)
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(ag_desc_value.clone()))
        } else {
            div().w_full().child(ag_desc.clone())
        },
        true,
    ));

    // 模型下拉（首项「继承」+ 分隔线 + 模型列表）
    let model_opts = chat
        .mc_sorted_enabled()
        .iter()
        .map(|m| {
            let key = format!("{}/{}", m.provider, m.id);
            let label = if m.name.is_empty() {
                key.clone()
            } else {
                m.name.clone()
            };
            (key, label)
        })
        .collect::<Vec<_>>();
    form = form.child(agent_row(
        &tr("agents.model"),
        model_dropdown(
            weak,
            &model_opts,
            ag_model,
            *ag_popup,
            ag_dd.clone(),
            readonly,
        ),
        false,
    ));

    // 工具权限下拉（044-0 六档）
    form = form.child(agent_row(
        &tr("agents.tools"),
        tools_dropdown(weak, ag_tools, *ag_popup, ag_dd.clone(), readonly),
        false,
    ));

    // full+ 档：自选 pi-ext / MCP 服务器（044-0 工具定制）；skills 定制恒显。
    // 只读态（内置）不渲染清单（内置不配扩展）。
    if !readonly {
        if ag_tools.as_str() == "full+" {
            form = form
                .child(agent_row(
                    &tr("agents.ext_pick"),
                    pick_list(
                        weak,
                        "ext",
                        pi_link::extensions::personal_extensions(),
                        ag_extensions,
                    ),
                    false,
                ))
                .child(agent_row(
                    &tr("agents.mcp_pick"),
                    {
                        let cwd = chat.ws.cwd.clone().unwrap_or_default();
                        let (servers, _) = pi_link::mcp::load(Some(std::path::Path::new(&cwd)));
                        let opts = servers
                            .iter()
                            .filter(|s| s.enabled)
                            .map(|s| s.name.clone())
                            .collect::<Vec<_>>();
                        pick_list(weak, "mcp", opts, ag_mcp)
                    },
                    false,
                ));
        }
        form = form.child(agent_row(
            &tr("agents.skills_pick"),
            {
                let cwd = chat.ws.cwd.clone().unwrap_or_default();
                let agent_dir = pi_link::config::agent_dir();
                let home_agents = agent_dir
                    .parent()
                    .map(|p| p.join("..").join(".agents").join("skills"))
                    .map(|p| dunce::canonicalize(&p).unwrap_or(p))
                    .unwrap_or_else(|| agent_dir.clone());
                let settings_value = pi_link::config::read_json(&pi_link::config::settings_path())
                    .unwrap_or(serde_json::json!({}));
                let entries = pi_link::skills::discover_skills(
                    std::path::Path::new(&cwd),
                    &agent_dir,
                    &home_agents,
                    &settings_value,
                );
                let opts = entries.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
                pick_list(weak, "skill", opts, ag_skills)
            },
            false,
        ));
    }

    // 系统提示 textarea（170px）；只读态渲染静态文本块
    form = form.child(agent_row(
        &tr("agents.prompt"),
        if readonly {
            div()
                .w_full()
                .h(px(170.))
                .rounded(px(6.))
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.bg_panel))
                .p(px(8.))
                .px(px(10.))
                .opacity(0.62)
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text))
                .child(SharedString::from(ag_prompt_value.clone()))
        } else {
            div()
                .w_full()
                .h(px(170.))
                .rounded(px(6.))
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.bg_panel))
                .child(ag_prompt.clone())
        },
        true,
    ));

    // 最大轮次 seg（50/100/不限/指定）+ 注入AGENTS.md 开关
    let seg = turns_seg(weak, ag_turns, ag_turns_n, readonly);
    let inject_label = div()
        .text_size(crate::appearance::ui_size(12.))
        .text_color(rgb(t.text))
        .child(tr("agents.inject_agents_md"));
    let inject_sw = config_switch("ag-inject", weak, *ag_inject, readonly, |c, cx| {
        if let Some(st) = c.dlg.settings.clone() {
            let _ = st.update(cx, |s, cx| {
                s.ag_inject = !s.ag_inject;
                s.ag_dirty = true;
                cx.notify();
            });
        }
    });
    form = form.child(agent_row(
        &tr("agents.max_turns"),
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .w_full()
            .child(seg)
            .child(div().flex_1())
            .child(inject_label)
            .child(inject_sw),
        false,
    ));

    if readonly {
        form = form.child(note(&tr("agents.readonly_builtin")));
    }

    detail_shell_with(head, form)
}

/// detail 容器（head + form）。
fn detail_shell_with(head: gpui::Div, form: gpui::Div) -> gpui::AnyElement {
    div()
        .id("mc-agents-detail")
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_y_scroll()
        .p(px(15.))
        .text_size(crate::appearance::ui_size(12.))
        .flex()
        .flex_col()
        .child(head)
        .child(form)
        .into_any_element()
}

/// 最大轮次四档 seg（原型 `.seg`）+ 指定时的数字输入。
fn turns_seg(
    weak: &gpui::WeakEntity<Chat>,
    turns: &MaxTurnsTier,
    turns_n: &gpui::Entity<TextInput>,
    readonly: bool,
) -> gpui::AnyElement {
    let t = T();
    let tiers: [(MaxTurnsTier, &'static str); 4] = [
        (MaxTurnsTier::Fifty, "50"),
        (MaxTurnsTier::Hundred, "100"),
        (MaxTurnsTier::Unlimited, tr("agents.unlimited")),
        (MaxTurnsTier::Custom(0), tr("agents.specify")),
    ];
    let is_custom = matches!(turns, MaxTurnsTier::Custom(_));
    let mut seg = div()
        .flex()
        .flex_shrink_0()
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.bg_panel))
        .overflow_hidden();
    for (tier, label) in tiers {
        let on = match (tier, turns) {
            (MaxTurnsTier::Custom(_), MaxTurnsTier::Custom(_)) => true,
            (a, b) => {
                std::mem::discriminant(&a) == std::mem::discriminant(b)
                    && !matches!(a, MaxTurnsTier::Custom(_))
            }
        };
        let weak_item = weak.clone();
        seg = seg.child(
            div()
                .h(px(28.))
                .px(px(14.))
                .flex()
                .items_center()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(if on { t.text } else { t.text_muted }))
                .bg(rgb(if on { t.bg_selected } else { t.bg_panel }))
                .when(on, |d| d.font_weight(gpui::FontWeight::SEMIBOLD))
                .when(!readonly, |d| {
                    d.cursor_pointer()
                        .hover(|s| s.bg(rgb(t.bg_hover)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak_item.update(cx, |c, cx| {
                                if let Some(st) = c.dlg.settings.clone() {
                                    st.update(cx, |s, cx| {
                                        s.ag_turns = tier;
                                        if matches!(tier, MaxTurnsTier::Custom(_)) {
                                            if s.ag_turns_n.read(cx).value().trim().is_empty() {
                                                s.ag_turns_n.update(cx, |ti, cx| {
                                                    ti.set_value("25".into(), cx)
                                                });
                                            }
                                        }
                                        s.ag_dirty = true;
                                        cx.notify();
                                    });
                                }
                            });
                        })
                })
                .child(SharedString::from(label.to_string())),
        );
    }
    let mut out = div().flex().items_center().gap(px(8.)).child(seg);
    if is_custom && !readonly {
        out = out.child(div().w(px(70.)).flex_shrink_0().child(turns_n.clone()));
    }
    out.into_any_element()
}
