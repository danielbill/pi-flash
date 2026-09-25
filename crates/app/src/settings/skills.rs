//! Skills tab.

use super::*;

impl Chat {
    pub(crate) fn mc_toggle_skill(&mut self, path: String, disable: bool, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        let p = PathBuf::from(&path);
        if let Err(e) = pi_link::skills::set_disable_invocation(&p, disable) {
            self.mc_set_error(&crate::i18n::tf("写入 SKILL.md 失败: {e}", &[("e", e)]), cx);
            return;
        }
        self.reload_settings_panel();
        cx.notify();
    }

}

/// Skills tab: project/global grouped sidebar + detail with the
/// visible-to-model switch (SKILL.md frontmatter).
pub(crate) fn mc_skills_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
) -> (gpui::AnyElement, gpui::AnyElement) {
    let t = T();
    let mut sb = div()
        .id("mc-sidebar")
        .w(px(240.))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(t.bg_panel))
        .border_r_1()
        .border_color(rgb(t.border))
        .p(px(6.))
        .pt(px(8.))
        .overflow_y_scroll();
    for (label, scope) in [(tr("项目"), pi_link::skills::SkillScope::Project), (tr("全局"), pi_link::skills::SkillScope::Global)] {
        let items: Vec<&pi_link::skills::SkillEntry> =
            chat.mc_skills.iter().filter(|s| s.scope == scope).collect();
        if items.is_empty() {
            continue;
        }
        sb = sb.child(
            div()
                .px(px(8.))
                .pt(px(6.))
                .pb(px(2.))
                .text_size(px(10.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(label.to_string())),
        );
        for sk in items {
            let active = sk.path.to_string_lossy() == section;
            let weak_item = weak.clone();
            let path = sk.path.to_string_lossy().to_string();
            sb = sb.child(
                div()
                    .id(SharedString::from(format!("skill-{}", sk.name)))
                    .h(px(30.))
                    .px(px(8.))
                    .rounded(px(5.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(12.))
                    .cursor_pointer()
                    .bg(if active { rgb(t.bg_selected) } else { rgb(t.bg_panel) })
                    .font_weight(if active { gpui::FontWeight::SEMIBOLD } else { gpui::FontWeight::NORMAL })
                    .text_color(if active { rgb(t.text) } else { rgb(t.text_muted) })
                    .hover(|s| s.bg(rgb(t.bg_hover)))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let _ = weak_item.update(cx, |c, cx| {
                            if let Some(st) = c.settings.clone() {
                                                st.update(cx, |s, cx| {
                                                    s.section = path.clone();
                                                    s.error = None;
                                                    cx.notify();
                                                });
                                            }
                        });
                    })
                    .child(
                        div()
                            .size(px(6.))
                            .rounded_full()
                            .flex_shrink_0()
                            .bg(if sk.disable_invocation { rgb(t.border) } else { rgb(0x4ade80) }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(SharedString::from(sk.name.clone())),
                    ),
            );
        }
    }
    if chat.mc_skills.is_empty() {
        sb = sb.child(
            div()
                .p(px(12.))
                .text_size(px(11.))
                .text_color(rgb(t.text_dim))
                .child(tr("没有找到技能（扫描项目 .pi/skills、.agents/skills 与全局目录）")),
        );
    }

    let selected = chat
        .mc_skills
        .iter()
        .find(|s| s.path.to_string_lossy() == section)
        .or_else(|| chat.mc_skills.first());
    let detail = match selected {
        None => div()
            .flex_1()
            .p(px(20.))
            .text_size(px(12.))
            .text_color(rgb(t.text_dim))
            .child(tr("没有找到技能"))
            .into_any_element(),
        Some(sk) => {
            let scope_tag = if sk.scope == pi_link::skills::SkillScope::Project {
                (tr("项目"), gpui::hsla(0.63, 0.86, 0.62, 0.12), gpui::hsla(0.63, 0.86, 0.62, 0.8))
            } else {
                (tr("全局"), gpui::hsla(0., 0., 0.5, 0.12), rgb(t.text_dim).into())
            };
            let weak_sw = weak.clone();
            let sw_path = sk.path.to_string_lossy().to_string();
            let visible = !sk.disable_invocation;
            div()
                .id("mc-detail")
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_y_scroll()
                .p(px(20.))
                .text_size(px(12.))
                .flex()
                .flex_col()
                .gap_4()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .min_h(px(28.))
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgb(t.text))
                                .child(SharedString::from(sk.name.clone())),
                        )
                        .child(
                            div()
                                .px(px(5.))
                                .py(px(1.))
                                .rounded(px(3.))
                                .bg(scope_tag.1)
                                .text_size(px(10.))
                                .text_color(scope_tag.2)
                                .child(scope_tag.0),
                        ),
                )
                .child(
                    div()
                        .font_family("Consolas")
                        .text_size(px(11.))
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(sk.path.to_string_lossy().to_string())),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(t.text_muted))
                        .child(SharedString::from(sk.description.clone())),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .min_h(px(36.))
                        .child(
                            div()
                                .text_size(px(11.))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgb(t.text_muted))
                                .child(if visible { tr("对模型可见") } else { tr("已隐藏（仍可手动调用）") }),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .id("skill-switch")
                                .w(px(32.))
                                .h(px(18.))
                                .rounded(px(9.))
                                .border_1()
                                .border_color(if visible { rgb(t.accent) } else { rgb(t.border) })
                                .bg(if visible { rgb(t.accent) } else { rgb(t.bg_selected) })
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .child(
                                    div()
                                        .ml(if visible { px(14.) } else { px(2.) })
                                        .size(px(12.))
                                        .rounded_full()
                                        .bg(if visible { rgb(t.bg) } else { rgb(t.text_muted) }),
                                )
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    let _ = weak_sw.update(cx, |c, cx| {
                                        c.mc_toggle_skill(sw_path.clone(), visible, cx)
                                    });
                                }),
                        ),
                )
                .into_any_element()
        }
    };
    (sb.into_any_element(), detail.into_any_element())
}

