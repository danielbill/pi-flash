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

/// 列表行：7px 圆点 + 名称（+ 覆盖内置徽标）。
fn agent_item(weak: &gpui::WeakEntity<Chat>, a: &AgentProfile, active: bool) -> gpui::AnyElement {
    let t = T();
    let id = a.id.clone();
    let builtins: Vec<AgentProfile> = pi_link::agents::builtin_agents();
    let overrides = a.overrides_builtin(&builtins);
    let broken = a.error.is_some();
    widgets::sidebar_item(format!("ag-{}", a.id), active)
        .bg(rgb(t.bg))
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
                .child(SharedString::from(a.id.clone())),
        )
        .when(overrides, |d| {
            d.child(
                div()
                    .px(px(5.))
                    .py(px(1.))
                    .rounded(px(3.))
                    .bg(gpui::hsla(0., 0., 0.5, 0.12))
                    .text_size(crate::appearance::ui_size(10.))
                    .text_color(rgb(t.text_dim))
                    .child(tr("agents.overrides_builtin")),
            )
        })
        .into_any_element()
}
