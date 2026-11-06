//! Models tab 右栏详情渲染：API KEY 表单、通路探查行、可用模型区、
//! 空 section 与启用 section 推导。

use super::*;

/// Models detail by section kind (catalog provider / custom editor / add).
#[allow(clippy::too_many_arguments)]
pub(super) fn mc_models_detail(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    selected: &str,
    d: &super::SettingsFormData,
    error: &Option<String>,
    enabled_set: &std::collections::HashSet<String>,
) -> gpui::AnyElement {
    let t = T();
    // 模型详情弹窗打开时（section 已变成 mj:m:/mj:o:），右栏底图保持**来源视图**：
    // 内置商的模型来自合并列表/内置详情（section = 商 id），不能把底图换成
    // 自定义商编辑器（那会显出一个不存在的草稿商）
    if let Some(back) = chat.cfg.mj_return.clone() {
        let is_model = selected.starts_with("mj:m:") || selected.starts_with("mj:o:");
        if is_model && back != selected {
            return mc_models_detail(chat, weak, &back, d, error, enabled_set);
        }
    }
    // 新建 Provider 草稿（042-1）：纯 UI，未落盘；name = 空 → 编辑器无删除钮
    if selected == "mj:new" {
        return super::custom_models::mj_provider_editor(chat, weak, "", d, error, t);
    }
    // custom provider / model editors
    if let Some(name) = selected.strip_prefix("mj:p:") {
        return super::custom_models::mj_provider_editor(chat, weak, name, d, error, t);
    }
    if let Some(rest) = selected.strip_prefix("mj:o:") {
        // 内置模型的覆盖式详情：右栏底图同样是 Provider 详情（section 原文
        // 不能当 provider 名，否则标题会显示成 mj:o:xxx:yyy）
        if let Some((name, _)) = rest.split_once(':') {
            return super::custom_models::mj_provider_editor(chat, weak, name, d, &None, t);
        }
    }
    if let Some(rest) = selected.strip_prefix("mj:m:") {
        // 模型编辑器已改居中 500×500 小弹窗（render.rs 顶层挂
        // mj_model_modal_overlay，section = mj:m:{prov}:{ix} 即打开态）；
        // 右栏底图保持 Provider 详情，错误归弹窗显示
        if let Some((name, _)) = rest.rsplit_once(':') {
            return super::custom_models::mj_provider_editor(chat, weak, name, d, &None, t);
        }
    }
    // catalog provider detail（API Key；OAuth 归 pi，PF 只管自有账本）
    let provider = selected.to_string();
    // 042-1：可用模型 = 唯一目录（models.json + models-store + 进程答案），
    // 不再有 registry 静态目录保底
    let models: Vec<pi_link::protocol::ModelInfo> = chat.provider_models_for_list(&provider);
    // 详情内边距与列表 15px 统一（040 扩展页定稿；detail_shell 默认 p20）
    let detail = detail_shell("mc-detail").p(px(15.));

    // header：Provider 名称（左）+ 操作钮（右，同一行）：[移除][保存]，间隔 10px。
    // 保存钮原本贴在 API Key 输入框右缘——输入框改全宽后统一收到这里。
    let remove_provider = provider.clone();
    let save_provider = provider.clone();
    let detail = detail.child(
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .min_h(px(28.))
            .child(
                div()
                    .text_size(crate::appearance::ui_size(13.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text))
                    .child(SharedString::from(
                        crate::ui::provider_display_name(&provider).to_string(),
                    )),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(detail_button(
                        "mc-remove",
                        weak,
                        &tr("action.remove"),
                        crate::ui::button::BtnVariant::Danger,
                        false,
                        move |c, cx| c.mc_ask_remove_provider(remove_provider.clone(), cx),
                    ))
                    .child(detail_button(
                        "mc-key-save",
                        weak,
                        &tr("action.save"),
                        crate::ui::button::BtnVariant::Primary,
                        false,
                        move |c, cx| {
                            let key = c
                                .dlg
                                .settings
                                .as_ref()
                                .map(|st| st.read(cx).key_input.clone())
                                .map(|input| input.read(cx).value().to_string())
                                .unwrap_or_default();
                            c.mc_save_key(save_provider.clone(), key, cx);
                        },
                    )),
            ),
    );
    // 详情页唯一信息栏（用户定稿）：按钮下方、右对齐，同一时刻只有一条
    let detail = detail.when(error.is_some(), |d| {
        d.child(error_note_right(error.as_deref().unwrap_or("")))
    });

    // API Key 段：标题 + 输入**全宽**（右缘内置眼睛，掩码显隐已存 key）；
    // 保存钮在 headr 行（见上）
    let weak_eye = weak.clone();
    let detail = detail
        .child(section_title("API Key"))
        .child(
            div().flex().items_center().child(
                div()
                    .relative()
                    .w_full()
                    .min_w_0()
                    .child(d.key_input.clone())
                    .child(
                        div()
                            .id("mc-key-eye")
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
                                        let input = st.read(cx).key_input.clone();
                                        let visible = st.update(cx, |s, cx| {
                                            s.key_visible = !s.key_visible;
                                            cx.notify();
                                            s.key_visible
                                        });
                                        input.update(cx, |ti, cx| ti.set_masked(!visible, cx));
                                    }
                                });
                            })
                            .child(crate::ui::icon(
                                if d.key_visible { "eye-off" } else { "eye" },
                                14.,
                                t.text_muted,
                            )),
                    ),
            ),
        )
        // API Key 与可用模型之间的分隔线
        .child(
            div()
                .h(px(1.))
                .w_full()
                .bg(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x66))),
        );

    // 可用模型区 + 底部（detail_shell 收尾）
    mc_enabled_section(
        chat,
        weak,
        &provider,
        &models,
        enabled_set,
        &d.model_filter,
        &d.model_filter_value,
        d.model_free_only,
        detail,
    )
    .into_any_element()
}

/// 空目录占位：模型数为 0 时模型区不再整体消失（原来连同「获取模型」钮、
/// Running 指示一起被藏掉，保存 key 后"没动静"也无处重试）。表内商给状态
/// 行 + 常驻获取钮；表外商（无列模型端点）说明列表从何而来。
fn mc_empty_models_section(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    provider: &str,
    detail: gpui::Stateful<gpui::Div>,
) -> gpui::Stateful<gpui::Div> {
    if chat.builtin_catalog_target(provider).is_none() {
        return detail.child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .pt(px(10.))
                .child(note(&tr("models.no_auto_fetch"))),
        );
    }
    let status: gpui::AnyElement = match chat.cfg.mc_probe.get(provider) {
        Some(McProbe::Running) => note(&tr("extensions.fetching")),
        Some(McProbe::Failed(e)) => div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(error_note(e))
            .child(note(&tr("models.proxy_hint")))
            .into_any_element(),
        Some(McProbe::Done) => note(&tr("models.probe_empty_catalog")).into_any_element(),
        None if chat.mc_configured(provider) => note(&tr("models.fetch_hint")).into_any_element(),
        None => note(&tr("models.fetch_hint_no_key")).into_any_element(),
    };
    let fetch_provider = provider.to_string();
    let detail = detail.child(
        div().flex().flex_col().gap(px(8.)).pt(px(10.)).child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().flex_1().min_w_0().child(status))
                // 获取钮隐藏不删（Chat::BUILTIN_FETCH_UI）
                .children(Chat::BUILTIN_FETCH_UI.then(|| {
                    detail_button(
                        "mc-fetch-empty",
                        weak,
                        &tr("extensions.fetch_models"),
                        crate::ui::button::BtnVariant::Secondary,
                        matches!(chat.cfg.mc_probe.get(provider), Some(McProbe::Running)),
                        move |c, cx| c.mc_fetch_models(fetch_provider.clone(), cx),
                    )
                })),
        ),
    );
    detail
}

/// 可用模型区（EnabledModelsSection parity）：标题+计数+只看free开关+批量钮
/// +筛选+行开关。
///
/// 列表用 uniform_list 虚拟化——只构建可见的 ~12 行。overflow div 全量
/// 构建 400 行 × ~12 元素、每敲一字筛选全量重建，是设置页卡顿的来源。
/// 行高固定 36px；启用查询走调用方一次构建的 HashSet（原来是每行线性扫
/// enabled 全表，400×400 次字符串比较/帧）。
#[allow(clippy::too_many_arguments)]
fn mc_enabled_section(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    provider: &str,
    models: &[pi_link::protocol::ModelInfo],
    enabled_set: &std::collections::HashSet<String>,
    model_filter: &gpui::Entity<crate::TextInput>,
    model_filter_value: &str,
    free_only: bool,
    detail: gpui::Stateful<gpui::Div>,
) -> gpui::Stateful<gpui::Div> {
    if models.is_empty() {
        return mc_empty_models_section(chat, weak, provider, detail);
    }
    let t = T();
    let enabled_count = models
        .iter()
        .filter(|m| {
            let r = format!("{}/{}", m.provider, m.id);
            enabled_set.contains(&r)
        })
        .count();
    // 标题行：可用模型 N/总数（左） + 只看free 开关 + rotate-ccw 图标钮（右，
    // 实拉该商目录；开关紧贴刷新钮左侧）
    let running = matches!(chat.cfg.mc_probe.get(provider), Some(McProbe::Running));
    let fetch_provider = provider.to_string();
    let fetch_weak = weak.clone();
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
            // 右侧聚簇：只看free（开关在左、文字在右）+ 刷新钮；文字与刷新钮 10px
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child({
                        // 只看free：开 = 列表只显示 free 模型（筛选叠加隐藏的 free
                        // 关键字）；状态在 SettingsPanel，切换即整面板重渲染
                        let weak_free = weak.clone();
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(config_switch(
                                "mc-free-only",
                                &weak_free,
                                free_only,
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
                    // 刷新钮隐藏不删（2026-11-01 定稿：内置商列表 = pi 自带注册表，
                    // 无需拉取；Chat::BUILTIN_FETCH_UI 翻开即复现）
                    .children(Chat::BUILTIN_FETCH_UI.then(|| {
                        // 纯 icon 获取钮：与移除/保存同宽 84 同高 32（右缘对齐）
                        let el = crate::ui::button::button_base(
                            "mc-fetch",
                            crate::ui::button::BtnSize::Lg,
                            crate::ui::button::BtnVariant::Secondary,
                        )
                        .w(px(84.))
                        .child(crate::ui::icon("rotate-ccw", 16., t.text));
                        if running {
                            el.opacity(0.5).into_any_element()
                        } else {
                            let weak = fetch_weak.clone();
                            let fp = fetch_provider.clone();
                            el.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let _ = weak.update(cx, |c, cx| c.mc_fetch_models(fp.clone(), cx));
                            })
                            .into_any_element()
                        }
                    })),
            ),
    );
    if chat.cfg.mc_project_scope {
        section = section.child(note(tr("models.project_override_dotpi")));
    }

    // 筛选（>8 个模型时出现；值经快照传入）。「只看free」开着时在用户
    // 关键字之外叠加隐藏的 free 关键字（AND 语义，不写进输入框）
    let shown = model_list_shown(models, model_filter_value, free_only);
    if models.len() > 8 {
        section = section.child(model_filter.clone());
    }
    // pi-web 行规格：min-height 36 + padding 6/9 + 浏览器默认行高（≈1.2，
    // 实际约 37px，开关右对齐、分隔线通栏）。行高不能写死——ui_size 随用户
    // 界面字号缩放，行高从两行缩放后的文本尺寸推导（上下各 6px 呼吸），
    // panel=12 时 ≈38px 与 pi-web 对齐，调大界面字号也不会再挤。
    let list = model_list_vlist(
        weak,
        provider,
        &shown,
        "mc-model-list",
        "mc",
        chat.cfg.mc_state.all_enabled,
        enabled_set,
        chat.cfg.mc_project_scope,
        &chat.cfg.mc_state.pins,
    );
    detail.child(section.child(list))
}
