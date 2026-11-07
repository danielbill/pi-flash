//! 子代理表单勾选构件与全局设置顶条：skills pick_list、sanitize_id、
//! 「全局设置」行（模块开关 + 最大并发）。模型/思考/工具档/ext/mcp 五个
//! 下拉已迁 menus.rs（InputPanel 组件复用，2026-11-07）。

use super::*;

/// 多选清单（skills 定制）：checkbox + 名称行，选中态读 `selected`，点击
/// 翻转 Settings 态的对应清单。空清单显暂无提示。（ext/mcp 清单已并入
/// 「工具」行三菜单。）
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
