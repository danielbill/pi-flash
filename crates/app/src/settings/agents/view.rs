//! 子代理页骨架（T 型布局，2026-11-07 用户定稿，对齐扩展页顶条式样）：
//! 首行「全局设置」通栏（模块开关 + 最大并发），下方左列表（内置/自定义
//! 两组）+ 右表单两栏；模块关闭时下方两栏整体不可见。

use super::detail::agent_detail;
use super::dropdowns::global_settings_bar;
use super::*;

/// 子代理页：顶条全局设置 + 左列表/右表单两栏（T 型）。
pub(crate) fn mc_agents_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
    d: &SettingsFormData,
) -> gpui::AnyElement {
    let t = T();
    let builtins: Vec<&AgentProfile> = chat
        .cfg
        .mc_agents
        .iter()
        .filter(|a| !a.origin.is_custom())
        .collect();
    let customs: Vec<&AgentProfile> = chat
        .cfg
        .mc_agents
        .iter()
        .filter(|a| a.origin.is_custom())
        .collect();

    // ---- 左列表 ----
    let mut list = sidebar_list().px(px(7.));
    if !builtins.is_empty() {
        list = list.child(group_header(
            &format!("{} {}", tr("agents.builtin_group"), builtins.len()),
            None,
        ));
        for a in &builtins {
            list = list.child(agent_item(weak, a, section == a.id));
        }
    }
    // 自定义组头：计数 + 右靠「新建」按钮
    list = list.child(group_header(
        &format!("{} {}", tr("agents.custom_group"), customs.len()),
        Some(config_button(
            "ag-new",
            weak,
            &tr("agents.new"),
            // 与右表单「保存」同款主按钮样式（2026-11-08 用户定稿）
            Btn::Primary,
            true,
            false,
            |c, cx| c.mc_agent_new(cx),
        )),
    ));
    for a in &customs {
        list = list.child(agent_item(weak, a, section == a.id));
    }
    if customs.is_empty() {
        list = list.child(
            div()
                .p(px(12.))
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_dim))
                .child(tr("agents.empty")),
        );
    }

    let sidebar = sidebar_shell("mc-sidebar-agents")
        .w(px(LIST_W))
        .bg(rgb(t.bg))
        .child(list)
        .into_any_element();

    // ---- 右表单 ----
    let detail = agent_detail(chat, weak, section, d);

    // T 型装配：模块开关关着 → 两栏整体不可见（顶条留在原地，才有地方
    // 重新打开；配置项不渲染 = 不留残影）
    div()
        .flex()
        .flex_col()
        .w_full()
        .h_full()
        .min_h_0()
        .child(global_settings_bar(weak))
        .when(
            crate::services::workspace::subagent_module_switch(),
            |pane| pane.child(div().flex().flex_1().min_h_0().child(sidebar).child(detail)),
        )
        .into_any_element()
}

/// 列表行：7px 圆点 + 名称（name = 点名句柄 + 显示名）+ 行尾 hover 显影
/// 动作钮 = 复制/删除两枚 icon 钮（2026-11-22 用户令：按钮改图标形，删除自
/// 右表单头挪来、置于最右；仅自定义可删——损坏态必为自定义文件，行尾删除
/// 已全覆盖）。钮 = kit icon_btn（§5.2 IB_SM，046 任务行尾同款，2026-11-22
/// 收编）。显影 = 行 group + 绘制期 opacity（命中区不变，psp 控制栏同款；
/// GroupHitboxes 按名分栈、paint 子树内 push/pop，同名组不跨行显影）。
/// 行高/行间 = 046 任务行规格（row::TASK 36 + 行间 SP2 4；2026-11-22 用户
/// 令：列表高度间距向定时任务列表对齐，基座 30/mb1 按后写覆盖）。
fn agent_item(weak: &gpui::WeakEntity<Chat>, a: &AgentProfile, active: bool) -> gpui::AnyElement {
    let t = T();
    let id = a.id.clone();
    let broken = a.error.is_some();
    let custom = a.origin.is_custom();
    let reveal = SharedString::from(format!("ag-row-{}", a.id));
    widgets::sidebar_item(format!("ag-{}", a.id), active)
        .h(px(crate::ui::tokens::row::TASK))
        .mb(px(crate::ui::tokens::space::SP2))
        .bg(rgb(t.bg))
        .group(reveal.clone())
        .on_mouse_down(MouseButton::Left, {
            let weak = weak.clone();
            let sel = id.clone();
            move |_, _, cx| {
                let _ = weak.update(cx, |c, cx| c.mc_agent_select(sel.clone(), cx));
            }
        })
        .child(status_dot(if broken { t.danger } else { t.accent }))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .when(broken, |d| d.text_color(rgb(t.danger)))
                .child(SharedString::from(a.display_name().to_string())),
        )
        .child(
            // 按钮壳：① 截停下压穿透——gpui 内层先外层后派发，行
            // on_mouse_down 若不拦会把 copy 刚选中的新条目覆盖回源条目
            // （2026-11-10 真机实锤，icon_btn 内部亦截断、双保险）；
            // ② 字重显式归位 NORMAL——active 行的 SEMIBOLD 会继承进钮区。
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(crate::ui::tokens::space::SP2))
                .opacity(0.)
                .group_hover(reveal, |s| s.opacity(1.))
                .font_weight(gpui::FontWeight::NORMAL)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(widgets::icon_btn(
                    format!("ag-copy-{}", a.id),
                    weak,
                    "copy",
                    "agents.copy",
                    false,
                    move |c, cx| c.mc_agent_copy(id.clone(), cx),
                ))
                .when(custom, |d| {
                    d.child(widgets::icon_btn(
                        format!("ag-del-{}", a.id),
                        weak,
                        "trash",
                        "agents.delete",
                        false,
                        {
                            let del_id = a.id.clone();
                            move |c, cx| c.mc_agent_ask_delete(del_id.clone(), cx)
                        },
                    ))
                }),
        )
        .into_any_element()
}

/// 子代理【删除】确认浮层（2026-11-10 加，custom_models::mj_delete_confirm_overlay
/// 同款机制）：入口 = 右表单 [删除] / 损坏态 [删除]（先置
/// `mc_agent_delete_confirm`），确认后走 [`Chat::mc_agent_delete`] 真删（进回收站）。
pub(crate) fn agent_delete_confirm_overlay(
    chat: &Chat,
    cx: &mut Context<Chat>,
) -> Option<gpui::AnyElement> {
    let id = chat.cfg.mc_agent_delete_confirm.clone()?;
    // 文案带显示名（name = 点名句柄）；条目已不在清单（损坏清掉等）回落 id
    let name = chat
        .cfg
        .mc_agents
        .iter()
        .find(|a| a.id == id)
        .map(|a| a.display_name().to_string())
        .unwrap_or_else(|| id.clone());
    let weak = cx.entity().downgrade();
    let message = crate::i18n::tf("agents.delete_confirm", &[("name", name)]);
    Some(crate::ui::overlay::confirm(
        &chat.dlg.dialog_focus,
        message,
        {
            let weak = weak.clone();
            move |_, cx| {
                let _ = weak.update(cx, |c, cx| c.mc_agent_delete_close(false, cx));
            }
        },
        move |_, cx| {
            let _ = weak.update(cx, |c, cx| c.mc_agent_delete_close(true, cx));
        },
    ))
}
