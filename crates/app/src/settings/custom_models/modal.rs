//! 模型编辑器弹窗（mj_model_modal_overlay）：自定义商模型行/【+模型】
//! 打开的小浮层，含能力勾选项构件。

use super::*;

/// 自定义模型详情/编辑小弹窗（500×600 居中，2026-10-27 定稿 500×500；
/// 042-2 加价格区后 +100）：
/// 标题栏 = chrome 头条 + 下划线（big_card 标准），标题「模型详情」+ ✕；
/// 页脚右对齐 [测试][保存][移除] 三钮同宽 84（间隔 10）；
/// 内容区 = 模型ID（左）+【自动填参】（右）→ 上下文窗口 / 最大输出 →
/// 价格四框（042-2）→ 能力勾选（文本恒选不可改 / 图片）→ **唯一一条**提示栏
///（表单错误 / 测试结果 / 填参结果三选一）。名称、reasoning、
/// 兼容等交【自动填参】从 models.dev 落（内部实现不暴露）。
/// `section = mj:m:{prov}:{ix}` 即打开态：本函数由 render.rs 顶层挂载，叠在
/// 设置弹窗之上，右栏底图保持 Provider 详情；关闭（✕/点外/ESC）= 退回
/// `mj:p:{prov}`（重播 provider 回填，模态随 section 撤下）。
pub(crate) fn mj_model_modal_overlay(
    chat: &Chat,
    cx: &mut Context<Chat>,
) -> Option<gpui::AnyElement> {
    let panel = chat.dlg.settings.as_ref()?;
    let selected = panel.read(cx).section.clone();
    // 两种目标：`mj:m:{prov}:{ix}`（models.json 条目）/ `mj:o:{prov}:{id}`
    //（内置模型覆盖，写 modelOverrides）
    let over = mj_parse_override(&selected);
    let (name, ix) = match &over {
        Some((p, _)) => (p.clone(), 0usize),
        None => {
            let rest = selected.strip_prefix("mj:m:")?;
            let (n, i) = rest.rsplit_once(':')?;
            (n.to_string(), i.parse::<usize>().ok()?)
        }
    };
    let over_id = over.as_ref().map(|(_, id)| id.clone());
    // 硬保证：详情页显示的就是当前 section 的模型。表单变量若还残留着别的
    // section（关闭/切换时没清干净，或任何缓存污染），这里立刻按盘上真值
    // 重填一次——绝不再把上一个模型的值端给用户看。
    if chat.cfg.mj_form_for.as_deref() != Some(selected.as_str()) {
        let weak_fix = cx.entity().downgrade();
        let sec = selected.clone();
        cx.defer(move |cx| {
            let _ = weak_fix.update(cx, |c, cx| {
                let now = c
                    .dlg
                    .settings
                    .as_ref()
                    .map(|st| st.read(cx).section.clone());
                // 期间又切走了 → 不再回填旧 section
                if now.as_deref() == Some(sec.as_str()) {
                    c.mj_select(sec.clone(), cx);
                }
            });
        });
    }
    // 内置模型 = 只读参数视图：不给自动填参 / 测试 / 保存 / 移除，输入框禁用
    let read_only = over_id.is_some();
    let d = super::SettingsFormData::snapshot(panel.read(cx), cx);
    let error = panel.read(cx).error.clone();
    let t = T();
    let weak = cx.entity().downgrade();
    // 关闭返回来源 section（点列表进详情时记下）；从【+模型】草稿进 → 退回
    // `mj:p:商`（旧行为）
    let back = chat.cfg.mj_return.clone();
    let dismiss = {
        let weak = weak.clone();
        let n = name.clone();
        move |_: &mut gpui::Window, cx: &mut gpui::App| {
            let _ = weak.update(cx, |c, cx| {
                let target = back.clone().unwrap_or_else(|| format!("mj:p:{n}"));
                c.cfg.mj_return = None;
                // 先清模型表单变量：关掉详情不留任何上个模型的值
                c.mj_clear_model_form(cx);
                c.mj_select(target, cx);
            });
        }
    };

    // 测试钮即时态 + 状态行文案（表单错误 / 测试结果 / 自动填参结果）
    let test_state = chat.cfg.mj_test.clone();
    let testing = matches!(test_state, Some(MjTestState::Running));
    let test_note: Option<(String, bool)> = match &test_state {
        Some(MjTestState::Running) => Some((tr("models.test_running").to_string(), false)),
        Some(MjTestState::Ok {
            latency_ms,
            status,
            text,
        }) => Some((
            if text.trim().is_empty() {
                crate::i18n::tf(
                    "models.test_passed_short",
                    &[
                        ("ms", latency_ms.to_string()),
                        ("status", status.to_string()),
                    ],
                )
            } else {
                crate::i18n::tf(
                    "models.test_passed",
                    &[
                        ("ms", latency_ms.to_string()),
                        ("status", status.to_string()),
                        ("text", text.clone()),
                    ],
                )
            },
            true,
        )),
        Some(MjTestState::Err(e)) => Some((e.clone(), false)),
        None => None,
    };
    // 标题栏（chrome 头条 + 下划线，big_card 同规格）：标题「模型详情」+ ✕；
    // 操作按钮不在这里（用户定稿：[测试][保存][移除] 在页脚右对齐）
    let header = div()
        .flex_shrink_0()
        .h(px(40.))
        .pl(px(15.))
        .pr(px(8.))
        .flex()
        .items_center()
        .gap(px(8.))
        .bg(rgb(t.chrome))
        .rounded_t(px(12.))
        .border_b_1()
        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x73)))
        .child(section_title(&tr("models.detail_title")))
        .child(div().flex_1())
        .child(crate::ui::overlay::close_btn(
            "mj-model-close",
            t,
            dismiss.clone(),
        ));

    // 页脚：[测试][移除][保存] 右对齐（标准 §5.1 按钮，经 detail_button 三钮
    // 同宽 84；2026-11-01 用户定稿：移除在保存左侧）；状态行（表单错误 /
    // 测试 / 自动填参结果）+ 三个设置行在滚动体
    let footer = div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_end()
        .gap(px(10.))
        .px(px(15.))
        .pb(px(15.))
        .child(super::models::detail_button(
            "mj-test",
            &weak,
            &tr("models.test"),
            crate::ui::button::BtnVariant::Secondary,
            testing || over_id.is_some(),
            {
                let n = name.clone();
                move |c, cx| c.mj_test_start(n.clone(), ix, cx)
            },
        ))
        .child(super::models::detail_button(
            "mj-mdel",
            &weak,
            &tr("action.remove"),
            crate::ui::button::BtnVariant::Danger,
            false,
            {
                let n = name.clone();
                move |c, cx| c.mj_delete_model(n.clone(), ix, cx)
            },
        ))
        .child(super::models::detail_button(
            "mj-mapply",
            &weak,
            &tr("action.save"),
            crate::ui::button::BtnVariant::Primary,
            false,
            {
                let n = name.clone();
                move |c, cx| c.mj_apply_model(n.clone(), ix, cx)
            },
        ));

    // 提示栏**只有一个**（用户定稿）：同一时刻只显示一条，位置在能力行下方。
    // 优先级：表单错误 > 测试结果 > 自动填参结果；查询中提示贴按钮。
    // 填参成功不占提示栏（用户定稿）：只有查不到/网络错才提示。
    let filling = matches!(chat.cfg.mj_models_dev_state, MjCatalogState::Running);
    let catalog_line: Option<(String, bool)> = match &chat.cfg.mj_models_dev_state {
        MjCatalogState::Done => None,
        MjCatalogState::Err(e) => Some((e.clone(), false)),
        MjCatalogState::Running | MjCatalogState::Idle => None,
    };
    let status_line: Option<(String, bool)> = error
        .as_ref()
        .map(|e| (e.clone(), false))
        .or(test_note.clone())
        .or(catalog_line);

    // id 必须与底层 Provider 详情（mc-detail）区分：同一帧渲染两份同 id 容器
    // 会让 gpui 的元素状态互相踩（点击落不到弹窗内的按钮）
    let detail =
        detail_shell("mj-model-detail")
            .p(px(15.))
            // 模型ID（左半列）+ 自动填参（右半列，标准 §5.1 按钮与 input 底缘
            // 对齐）；两列与下行 上下文/最大输出 同宽——ID input 等长对齐
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(field(&tr("custom_models.field_id"), d.mj_id.clone())),
                    )
                    .children((!read_only).then(|| {
                        let n = name.clone();
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_end()
                            .justify_end()
                            .child({
                                let _ = &n;
                                let el = crate::ui::button::button_base(
                                    "mj-autofill",
                                    crate::ui::button::BtnSize::Lg,
                                    crate::ui::button::BtnVariant::Secondary,
                                )
                                .w(px(84.))
                                .child(SharedString::from(
                                    if filling {
                                        tr("models.catalog_filling")
                                    } else {
                                        tr("models.autofill")
                                    }
                                    .to_string(),
                                ));
                                if filling {
                                    el.opacity(0.5).into_any_element()
                                } else {
                                    let weak_fill = weak.clone();
                                    el.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                        let _ = weak_fill.update(cx, |c, cx| {
                                            c.mj_catalog_fill(n.clone(), ix, cx)
                                        });
                                    })
                                    .into_any_element()
                                }
                            })
                    })),
            )
            // 上下文窗口 / 最大输出
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(field(&tr("custom_models.field_context"), d.mj_ctx.clone())),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(field(&tr("models.max_output"), d.mj_max_tokens.clone())),
                    ),
            )
            // 价格（042-2）：手填显式覆盖；自动填参回显目录价（只填空/零）。
            // 2×2 网格，宽窄与上下文行一致
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .child(section_title(&tr("models.cost_per_m")))
                    .child(
                        div()
                            .flex()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(field(&tr("models.cost_input"), d.mj_cost_in.clone())),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(field(&tr("models.cost_output"), d.mj_cost_out.clone())),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(10.))
                            .child(
                                div().flex_1().min_w_0().child(field(
                                    &tr("models.cost_cache_read"),
                                    d.mj_cost_cr.clone(),
                                )),
                            )
                            .child(div().flex_1().min_w_0().child(field(
                                &tr("models.cost_cache_write"),
                                d.mj_cost_cw.clone(),
                            ))),
                    ),
            )
            // 推理（用户定稿：能力之前；单勾选落 model.reasoning）
            .child(div().flex().items_center().gap(px(16.)).child(
                mj_cap_checkbox(
                    &tr("models.reasoning"),
                    d.mj_reasoning,
                    &weak,
                    (!read_only).then_some(|c: &mut Chat, cx: &mut Context<Chat>| {
                        c.mj_toggle_reasoning(cx)
                    }),
                ),
            ))
            // 能力：文本恒选不可改；图片勾选落 model.input
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .child(section_title(&tr("models.capabilities")))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(16.))
                            .child(mj_cap_checkbox(&tr("models.cap_text"), true, &weak, None))
                            .child(mj_cap_checkbox(
                                &tr("models.cap_image"),
                                d.mj_image,
                                &weak,
                                (!read_only).then_some(|c: &mut Chat, cx: &mut Context<Chat>| {
                                    c.mj_toggle_image(cx)
                                }),
                            )),
                    ),
            )
            // 提示栏（唯一一条）：表单错误 / 测试结果 / 填参结果三选一
            .children(status_line.as_ref().map(
                |(text, ok)| {
                    if *ok { note(text) } else { error_note(text) }
                },
            ));
    // 弹窗装配：遮罩 + 外点/ESC 关（都退回 Provider 详情）+ 居中 500×600
    // 卡片（窗框对齐 big_card：chrome 头条 + 圆角 + 阴影）；控制栏固定，表单区滚动
    // 042-2 加价格四框后高 +100（用户验收定稿），read_only 同步放宽
    let overlay = crate::ui::overlay::layer(true, Some(&chat.dlg.dialog_focus), dismiss)
        .flex()
        .items_center()
        .justify_center()
        .child(crate::ui::overlay::stop_click(
            div()
                .flex_shrink_0()
                .w(px(500.))
                .when(!read_only, |d| d.h(px(600.)))
                .when(read_only, |d| d.max_h(px(660.)))
                .bg(rgb(t.bg))
                .border_1()
                .border_color(rgb(t.border))
                .rounded(px(12.))
                .shadow_lg()
                .flex()
                .flex_col()
                .overflow_hidden()
                .child(header)
                .child(div().flex_1().min_h_0().flex().child(detail))
                .children((!read_only).then_some(footer)),
        ));
    Some(overlay.into_any_element())
}

/// 能力勾选项（checkbox §5.4：14、r3、勾 10 = accent_contrast；视觉同
/// pi_extension_picker 的既核勾选框）。`toggle` 传 None = 恒选不可改
///（文本：模型输入天然含 text）。
fn mj_cap_checkbox(
    label: &str,
    checked: bool,
    weak: &gpui::WeakEntity<Chat>,
    toggle: Option<fn(&mut Chat, &mut gpui::Context<Chat>)>,
) -> gpui::AnyElement {
    let t = T();
    let mut el = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            div()
                .size(px(crate::ui::tokens::checkbox::SIZE))
                .flex_shrink_0()
                .rounded(px(3.))
                .border_1()
                .border_color(rgb(if checked { t.accent } else { t.border }))
                .bg(rgb(if checked { t.accent } else { t.bg_panel }))
                .flex()
                .items_center()
                .justify_center()
                .when(checked, |dd| {
                    dd.child(crate::ui::icon(
                        "check",
                        crate::ui::tokens::checkbox::CHECK,
                        t.accent_contrast,
                    ))
                }),
        )
        .child(
            div()
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(if toggle.is_none() { t.text_dim } else { t.text }))
                .child(SharedString::from(label.to_string())),
        );
    if let Some(toggle) = toggle {
        el = el
            .cursor_pointer()
            .hover(|s| s.opacity(0.85))
            .on_mouse_down(MouseButton::Left, {
                let weak = weak.clone();
                move |_, _, cx| {
                    let _ = weak.update(cx, |c, cx| toggle(c, cx));
                }
            });
    }
    el.into_any_element()
}
