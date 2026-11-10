//! 子代理右表单：agent_detail 全字段装配（名称/简介/定制指令/轮次等）
//! 与 detail_shell / seg_control 构件。

use super::menus::{model_thinking_control, skills_trigger, tools_control};
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
        ag_turns,
        ag_inject,
        ag_cwd_pf,
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
        .when(!is_new && !builtin, |d| {
            // 删除（仅自定义；复制入口已移列表行 hover [复制]，2026-11-10）
            d.child(config_button(
                "ag-del",
                weak,
                &tr("agents.delete"),
                Btn::Danger,
                true,
                false,
                {
                    let id = agent.as_ref().map(|a| a.id.clone()).unwrap_or_default();
                    move |c, cx| c.mc_agent_ask_delete(id.clone(), cx)
                },
            ))
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
                            move |c, cx| c.mc_agent_ask_delete(id.clone(), cx)
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

    // 模型 + 思考强度（InputPanel 同款触发钮 + composer 菜单组件全复用，
    // 2026-11-07 对齐定稿）；工具 = 档位/ext/mcp 三菜单
    if let Some(panel) = chat.dlg.settings.clone() {
        form = form
            .child(agent_row(
                &tr("agents.model"),
                model_thinking_control(chat, &panel, weak, d, readonly),
                false,
            ))
            .child(agent_row(
                &tr("agents.tools"),
                tools_control(chat, &panel, weak, d, readonly),
                false,
            ));
    }

    // 技能设置（继承 | 自定义下拉，2026-11-08 设计对齐）：菜单 = 技能页
    // 同一份列表（mc_skills，全局在前项目在后）+ 搜索 + 全选。
    // 只读态（内置）不渲染定制入口。
    if let Some(panel) = chat.dlg.settings.clone() {
        form = form.child(agent_row(
            &tr("agents.skills_pick"),
            skills_trigger(chat, &panel, weak, d, readonly),
            false,
        ));
    }

    // 定制指令 textarea（170px，追加在 Pi 系统指令之后）；只读态渲染静态文本块
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

    // 环境注入（默认注入 = 与主代理一致的环境上下文；2026-11-08 设计对齐，
    // 自「最大轮次」行拆出独立行）
    let inject_hint = div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .text_size(crate::appearance::ui_size(11.))
        .text_color(rgb(t.text_dim))
        .child(tr("agents.env_inject_hint"));
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
        &tr("agents.inject_agents_md"),
        // 开关在前、说明随后左对齐（2026-11-08 用户定稿）
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .w_full()
            .child(inject_sw)
            .child(inject_hint),
        false,
    ));

    // 最大轮次 seg（5/30/100/不限/指定；指定 = 主代理委派时给 maxloop）
    form = form.child(agent_row(
        &tr("agents.max_turns"),
        turns_seg(weak, ag_turns, readonly),
        false,
    ));

    // 工作目录（继承 | PF运行目录；默认继承，2026-11-08 设计对齐）
    form = form.child(agent_row(
        &tr("agents.workdir"),
        workdir_seg(weak, *ag_cwd_pf, readonly),
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

/// 最大轮次五档 seg（原型 `.seg`；2026-11-08 设计对齐 + 2026-11-10 加 5 档）：
/// 5/30/100/不限/指定。「指定」= 主代理委派时给 maxloop，profile 不写
/// maxTurns（原数字输入退役）；5 档给内置摘要机用，不另造语义。
fn turns_seg(
    weak: &gpui::WeakEntity<Chat>,
    turns: &MaxTurnsTier,
    readonly: bool,
) -> gpui::AnyElement {
    seg_control(
        weak,
        readonly,
        [
            (MaxTurnsTier::Five, "5", *turns == MaxTurnsTier::Five),
            (MaxTurnsTier::Thirty, "30", *turns == MaxTurnsTier::Thirty),
            (
                MaxTurnsTier::Hundred,
                "100",
                *turns == MaxTurnsTier::Hundred,
            ),
            (
                MaxTurnsTier::Unlimited,
                tr("agents.unlimited"),
                *turns == MaxTurnsTier::Unlimited,
            ),
            (
                MaxTurnsTier::Custom,
                tr("agents.specify"),
                *turns == MaxTurnsTier::Custom,
            ),
        ],
        |s, tier| s.ag_turns = tier,
    )
}

/// 工作目录两档 seg（2026-11-08 设计对齐）：继承委派者 | PF 数据目录
/// （profile `cwd: pf-data`，子进程落 PF 数据目录）。
fn workdir_seg(weak: &gpui::WeakEntity<Chat>, cwd_pf: bool, readonly: bool) -> gpui::AnyElement {
    seg_control(
        weak,
        readonly,
        [
            (false, tr("agents.inherit"), !cwd_pf),
            (true, tr("agents.workdir_pf"), cwd_pf),
        ],
        |s, v| s.ag_cwd_pf = v,
    )
}

/// seg 通用装配（最大轮次/工作目录共用一份壳：28px 圆角分段、选中
/// bg_selected + semibold）：`options` = (值, 文案, 选中)，`set` 写对应
/// 表单字段 + 脏标。
fn seg_control<T: Copy + PartialEq + 'static>(
    weak: &gpui::WeakEntity<Chat>,
    readonly: bool,
    options: impl IntoIterator<Item = (T, &'static str, bool)>,
    set: impl Fn(&mut SettingsPanel, T) + Copy + 'static,
) -> gpui::AnyElement {
    let t = T();
    let mut seg = div()
        .flex()
        .flex_shrink_0()
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.bg_panel))
        .overflow_hidden();
    for (value, label, on) in options {
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
                                        set(s, value);
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
    seg.into_any_element()
}
