//! 模型列表 vlist 装配（行高/可见性/高度档）与 Provider 移除确认浮层。

use super::*;

/// 可用模型行的行高（内置 / 自定义共用；ui_size 随界面字号缩放）。
pub(crate) fn model_list_row_h() -> f32 {
    (f32::from(crate::appearance::ui_size(11.)) + f32::from(crate::appearance::ui_size(10.))) * 1.25
        + 12.
}

/// 可用模型列表的筛选（内置 / 自定义共用）：关键字匹配名称或 id，「只看free」
/// 在关键字之外叠加隐藏的 free 匹配（AND 语义，不写进输入框）。
pub(crate) fn model_list_shown(
    models: &[pi_link::protocol::ModelInfo],
    filter: &str,
    free_only: bool,
) -> Vec<pi_link::protocol::ModelInfo> {
    let q = filter.trim().to_lowercase();
    models
        .iter()
        .filter(|m| {
            let hit = q.is_empty()
                || m.id.to_lowercase().contains(&q)
                || m.name.to_lowercase().contains(&q);
            hit && (!free_only
                || m.id.to_lowercase().contains("free")
                || m.name.to_lowercase().contains("free"))
        })
        .cloned()
        .collect()
}

/// 可用模型列表（**内置商与自定义商共用同一行实现**，行为不许分叉）：
/// 两行行（名称 + 等宽 id）；点行 = 打开模型详情弹窗（内置模型 → 只读覆盖
/// 视图 / 自定义商 → 账本条目）；点 id 行 = 复制 `provider/id`
/// （stop_propagation，不连带开弹窗）；行右 = 启用开关（自己的点击）。
/// `list_id` / `id_prefix` 区分同帧两处渲染（内置详情 / 自定义详情），元素 id
/// 撞车会让 gpui 元素状态互相踩。
pub(crate) fn model_list_vlist(
    weak: &gpui::WeakEntity<Chat>,
    provider: &str,
    shown: &[pi_link::protocol::ModelInfo],
    list_id: &'static str,
    id_prefix: &'static str,
    all_enabled: bool,
    enabled_set: &std::collections::HashSet<String>,
    project_scope: bool,
    pins: &[(String, String)],
) -> gpui::AnyElement {
    let t = T();
    let row_h = model_list_row_h();
    let shown: Vec<pi_link::protocol::ModelInfo> = shown.to_vec();
    let enabled_rows = enabled_set.clone();
    let pins = pins.to_vec();
    let weak_rows = weak.clone();
    let prov = provider.to_string();
    vlist(
        list_id,
        shown.len(),
        row_h,
        VListHeight::Capped(360.),
        true,
        true,
        tr("models.no_matching_models"),
        None,
        move |ix, _window, _cx| {
            let m = &shown[ix];
            let r = format!("{}/{}", m.provider, m.id);
            let is_enabled = all_enabled || enabled_rows.contains(&r);
            let pin = pins.iter().find(|(p, _)| p == &r).map(|(_, l)| l.clone());
            // 点行 → 模型详情（与左栏合并列表同一入口，`mj_open_model` 自己分流
            // 账本条目 / 内置覆盖两种模式）
            let open_ref = (prov.clone(), m.id.clone());
            let weak_row = weak_rows.clone();
            let ref_click = r.clone();
            div()
                .id(SharedString::from(format!("{id_prefix}-row-{ix}")))
                // uniform_list 的行是 fit-content（taffy 根节点不拉伸）：
                // 不写 w_full 分隔线只有内容宽、开关贴在文字右边
                .w_full()
                .h(px(row_h))
                // 左 12 内容留白；右 24 = 12 内容 + 12 滚动条让位（不压 switch）
                .pl(px(12.))
                .pr(px(24.))
                .flex()
                .items_center()
                .gap(px(8.))
                .when(ix > 0, |d| d.border_t_1().border_color(rgb(t.border)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak_row.update(cx, |c, cx| {
                                let (prov, id) = &open_ref;
                                c.mj_open_model(prov.clone(), id.clone(), cx);
                            });
                        })
                        .child(
                            div()
                                .text_size(crate::appearance::ui_size(11.))
                                .line_height(gpui::relative(1.25))
                                .text_color(rgb(t.text))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(SharedString::from(if m.name.is_empty() {
                                    m.id.clone()
                                } else {
                                    m.name.clone()
                                })),
                        )
                        .child(
                            div()
                                .font_family(crate::editor::markdown::MONO_FAMILY)
                                .text_size(crate::appearance::ui_size(10.))
                                .line_height(gpui::relative(1.25))
                                .text_color(rgb(t.text_dim))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    // 点 id = 复制模型引用（不连带开详情弹窗）
                                    cx.stop_propagation();
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        ref_click.clone(),
                                    ));
                                })
                                .child(SharedString::from(m.id.clone())),
                        ),
                )
                .children(pin.map(|p| {
                    div()
                        .px(px(4.))
                        .py(px(1.))
                        .rounded(px(3.))
                        .bg(widgets::indigo_bg())
                        .text_size(crate::appearance::ui_size(9.))
                        .text_color(widgets::indigo_fg())
                        .child(SharedString::from(p))
                }))
                .child(config_switch(
                    format!("{id_prefix}-sw-{ix}"),
                    &weak_rows,
                    is_enabled,
                    project_scope,
                    {
                        let r = r.clone();
                        move |c, cx| c.mc_toggle_model(r.clone(), !is_enabled, cx)
                    },
                ))
                .into_any_element()
        },
    )
}
