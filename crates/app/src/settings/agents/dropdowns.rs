//! 子代理表单勾选构件与全局设置顶条：skills pick_list、sanitize_id、
//! 「全局设置」行（模块开关 + 最大并发）。模型/思考/工具档/ext/mcp 五个
//! 下拉已迁 menus.rs（InputPanel 组件复用，2026-11-07）。

use super::*;

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
            move |c, cx| {
                crate::services::workspace::set_subagent_module_switch(!on);
                // 工具清单随 spawn 走：空闲会话立刻重绑、运行中的下一轮生效
                // （否则设置页关了开关，已有进程里的 `agent`/`subagent_*`
                // 仍在注入——2026-11-08 真机 bug）
                c.apply_subagent_switch(cx);
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
