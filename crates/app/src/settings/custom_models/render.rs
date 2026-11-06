//! 自定义 Provider 详情渲染：删除确认浮层、右栏编辑器
//!（mj_provider_editor）、启用 section 推导（mj_enabled_section）。

use super::*;

/// 自定义商【删除】确认浮层（042-1 UI：删除 = 连同其下所有模型）。
pub(crate) fn mj_delete_confirm_overlay(
    chat: &Chat,
    cx: &mut Context<Chat>,
) -> Option<gpui::AnyElement> {
    let name = chat.cfg.mc_mj_delete_confirm.clone()?;
    let weak = cx.entity().downgrade();
    let message = crate::i18n::tf("custom_models.delete_confirm", &[("name", name)]);
    Some(crate::ui::overlay::confirm(
        &chat.dlg.dialog_focus,
        message,
        {
            let weak = weak.clone();
            move |_, cx| {
                let _ = weak.update(cx, |c, cx| c.mj_delete_provider_close(false, cx));
            }
        },
        move |_, cx| {
            let _ = weak.update(cx, |c, cx| c.mj_delete_provider_close(true, cx));
        },
    ))
}

/// 自定义 Provider 详情页（042-1 定稿：**复用内置 Provider 详情面板** +
/// 自有组件）：头行 = name input +【移除】→ API Key 段（掩码+眼睛，保存
/// 钮已挪操作栏）→ Base URL → API 下拉（默认 auto）→ 操作栏
/// [测速][保存][+模型] + 注释行 → 高级设置（Headers 折叠）→ 分隔线 →
/// 可用模型区（内置同规格，行可点进模型编辑器）。
pub(crate) fn mj_provider_editor(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    name: &str,
    d: &super::SettingsFormData,
    error: &Option<String>,
    t: &crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    // 042-1 门禁：测速只制约【+模型】。【保存】随时可用——新建商可以先把
    // key / baseUrl 存下来（连通性由测速与自动填参负责），否则「测速失败就
    // 什么都存不了」会把半成品配置锁死。
    let add_ok = chat
        .cfg
        .mj_speed
        .as_ref()
        .is_some_and(|st| st.allows_edit());
    let testing = matches!(chat.cfg.mj_speed, Some(MjSpeedState::Running));

    // 两列表单行：左侧标题列（定宽 120），右侧控件列（042-1 蓝框定稿）
    let form_row = move |label: &str, control: gpui::AnyElement| -> gpui::AnyElement {
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .mb(px(10.))
            .child(
                div()
                    .w(px(120.))
                    .flex_shrink_0()
                    .child(section_title(label)),
            )
            .child(div().flex_1().min_w_0().child(control))
            .into_any_element()
    };

    // 草稿态（未保存的新商）：没有可删的对象，不给删除钮
    let is_new = name.is_empty();
    // name 行：标题列 + 控件列（input 半宽；[删除][保存] 与 name input 同列右对齐）
    let remove_provider = name.to_string();
    let name_owned = name.to_string();
    let detail = detail_shell("mc-detail").p(px(15.)).child(form_row(
        &tr("field.name"),
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .w_full()
            .child(
                div()
                    .w(gpui::relative(0.5))
                    .min_w_0()
                    .child(d.mj_name.clone()),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .children((!is_new).then(|| {
                        super::models::detail_button(
                            "mj-del",
                            weak,
                            &tr("action.delete"),
                            crate::ui::button::BtnVariant::Danger,
                            false,
                            move |c, cx| c.mj_ask_delete_provider(remove_provider.clone(), cx),
                        )
                    }))
                    .child(super::models::detail_button(
                        "mj-apply",
                        weak,
                        &tr("action.save"),
                        crate::ui::button::BtnVariant::Primary,
                        false,
                        move |c, cx| c.mj_apply_provider(name_owned.clone(), cx),
                    )),
            )
            .into_any_element(),
    ));

    // API Key 行：控件全宽（右缘眼睛，掩码显隐已存 key）；保存钮在操作栏
    let weak_eye = weak.clone();
    let detail = detail.child(form_row(
        "API Key",
        div()
            .relative()
            .w_full()
            .child(d.mj_key.clone())
            .child(
                div()
                    .id("mj-key-eye")
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(px(8.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .hover(|s| s.opacity(0.7))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let _ = weak_eye.update(cx, |c, cx| {
                            if let Some(st) = c.dlg.settings.clone() {
                                let input = st.read(cx).mj_key.clone();
                                let visible = st.update(cx, |s, cx| {
                                    s.mj_key_visible = !s.mj_key_visible;
                                    cx.notify();
                                    s.mj_key_visible
                                });
                                input.update(cx, |ti, cx| ti.set_masked(!visible, cx));
                            }
                        });
                    })
                    .child(crate::ui::icon(
                        if d.mj_key_visible { "eye-off" } else { "eye" },
                        14.,
                        t.text_muted,
                    )),
            )
            .into_any_element(),
    ));

    // Base URL 行（全宽）。API 类型已下线（042-1 改版）：形状由 baseUrl
    // 推断落盘，用户零选择；误判走模型编辑器的 API 覆盖逃生门
    let detail = detail.child(form_row(
        &tr("custom_models.base_url"),
        d.mj_base.clone().into_any_element(),
    ));

    // 操作栏 [测速][保存][+模型]（042-1）：测速 = 连通后自动保存 + 自动
    // 拉模型；通过前保存/+模型禁用。结果行/失败提示紧跟其下。
    let detail = detail
        .child(
            div()
                .mt(px(10.))
                .flex()
                .items_center()
                .justify_end()
                .gap(px(8.))
                .child(super::models::detail_button(
                    "mj-speedtest",
                    weak,
                    &if testing {
                        tr("models.speed_testing")
                    } else {
                        tr("models.speedtest")
                    },
                    crate::ui::button::BtnVariant::Secondary,
                    testing,
                    {
                        let n = name.to_string();
                        move |c, cx| c.mj_speedtest_start(n.clone(), cx)
                    },
                ))
                .child(super::models::detail_button(
                    "mj-add-model",
                    weak,
                    &format!("+ {}", tr("settings.tab.models")),
                    crate::ui::button::BtnVariant::Secondary,
                    !add_ok || is_new,
                    {
                        let n = name.to_string();
                        move |c, cx| c.mj_add_model(n.clone(), cx)
                    },
                )),
        )
        // 详情页唯一的信息栏（用户定稿）：两按钮下方、右对齐，同一时刻只有
        // 一条。优先级：表单错误 > 测速结果（seeded 通过态不显示）。
        .children(if let Some(e) = error.as_ref() {
            vec![error_note_right(e)]
        } else {
            match chat.cfg.mj_speed.as_ref() {
                Some(MjSpeedState::Running) => vec![note_right(&tr("models.speed_testing"))],
                Some(MjSpeedState::Passed {
                    latency_ms,
                    models,
                    seeded: false,
                }) => vec![note_right(&crate::i18n::tf(
                    "models.speed_pass",
                    &[
                        ("ms", latency_ms.to_string()),
                        ("count", models.to_string()),
                    ],
                ))],
                Some(MjSpeedState::PassedEmpty { .. }) => {
                    vec![error_note_right(&tr("models.autofetch_failed_hint"))]
                }
                Some(MjSpeedState::Failed(e)) => vec![error_note_right(e)],
                _ => Vec::new(),
            }
        });

    // API 段与可用模型区之间的分隔线（内置详情同款）
    let detail = detail.child(
        div()
            .h(px(1.))
            .w_full()
            .bg(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x66))),
    );

    // 可用模型区（内置同规格）；草稿态不渲染——还没保存，没有可挂的模型
    if is_new {
        return detail.into_any_element();
    }
    mj_enabled_section(chat, weak, name, d, t, detail).into_any_element()
}

/// 自定义商的可用模型区（视觉规格对齐内置详情的 [`super::models::mc_enabled_section`]：
/// 标题 N/total + 只看free + [+模型] 钮 + 筛选 + vlist 行开关）。数据源为
/// 统一目录里该自定义商的条目（providers.json 与测速落盘的 provider-catalog
/// 都经 disk_models 合并）；模型行可点进模型编辑器（pi-web 模型行行为）。
fn mj_enabled_section(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    provider: &str,
    d: &super::SettingsFormData,
    _t: &crate::appearance::theme::Theme,
    detail: gpui::Stateful<gpui::Div>,
) -> gpui::Stateful<gpui::Div> {
    // 闭包存进元素需要 'static：主题用静态句柄遮蔽参数（mc_enabled_section 同款）
    let t = T();
    let provider_owned = provider.to_string();
    // 罗列单点（账本 ∪ 目录 ∪ 探测展示缓存；2026-11-01 定稿）
    let models: Vec<pi_link::protocol::ModelInfo> = chat.provider_models_for_list(provider);
    let enabled_set: std::collections::HashSet<String> =
        chat.cfg.mc_state.enabled.iter().cloned().collect();
    let enabled_count = models
        .iter()
        .filter(|m| {
            let r = format!("{}/{}", m.provider, m.id);
            enabled_set.contains(&r)
        })
        .count();
    let mut section = div().flex().flex_col().gap(px(8.)).pt(px(10.)).child(
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(section_title(&crate::i18n::tf(
                "models.available_count",
                &[
                    ("enabled", enabled_count.to_string()),
                    ("total", models.len().to_string()),
                ],
            )))
            .child(div().flex_1())
            // 右侧聚簇：只看free（开关在左、文字在右，6px）+ 刷新钮（10px）
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child({
                        // 只看free：与内置详情共用同一面板状态
                        let weak_free = weak.clone();
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(config_switch(
                                "mj-free-only",
                                &weak_free,
                                d.model_free_only,
                                false,
                                |c, cx| {
                                    if let Some(st) = c.dlg.settings.clone() {
                                        st.update(cx, |s, cx| {
                                            s.model_free_only = !s.model_free_only;
                                            cx.notify();
                                        });
                                    }
                                    cx.notify();
                                },
                            ))
                            .child(
                                div()
                                    .text_size(crate::appearance::ui_size(11.))
                                    .text_color(rgb(t.text_dim))
                                    .child(tr("models.free_only")),
                            )
                    })
                    .child({
                        // 刷新钮（内置详情同位同款 rotate-ccw 84×32）：按已保存条目
                        // 重拉该商模型目录
                        let weak_fetch = weak.clone();
                        let fp = provider_owned.clone();
                        let el = crate::ui::button::button_base(
                            "mj-fetch",
                            crate::ui::BtnSize::Lg,
                            crate::ui::button::BtnVariant::Secondary,
                        )
                        .w(px(84.))
                        .child(crate::ui::icon("rotate-ccw", 16., t.text));
                        el.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ =
                                weak_fetch.update(cx, |c, cx| c.mj_fetch_models(fp.clone(), cx));
                        })
                        .into_any_element()
                    }),
            ),
    );

    // 空目录：只有标题行（无提示文字，042-1 定稿）
    if models.is_empty() {
        return detail.child(section);
    }

    // 筛选（>8 个模型时出现；与内置详情同规则）+「只看free」叠加
    let shown = super::models::model_list_shown(&models, &d.model_filter_value, d.model_free_only);
    if models.len() > 8 {
        section = section.child(d.model_filter.clone());
    }
    // 列表行与内置商详情**同一实现**（super::models::model_list_vlist）：两行行、
    // 点行开模型详情、点 id 复制引用、右侧启用开关。行高/筛选/元素 id 前缀都在
    // 那边统一，这里只管取数据
    let list = super::models::model_list_vlist(
        weak,
        provider,
        &shown,
        "mj-model-list",
        "mj",
        chat.cfg.mc_state.all_enabled,
        &enabled_set,
        chat.cfg.mc_project_scope,
        &chat.cfg.mc_state.pins,
    );
    detail.child(section.child(list))
}
