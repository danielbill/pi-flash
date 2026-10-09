//! Skills tab (pi-web SkillsConfig parity)：项目/全局分组，组头 {可见}/
//! {总数} + 批量开关（写各 SKILL.md 的 disable-model-invocation），休眠
//! （对模型隐藏）技能排组内最后；右栏详情 + 单技能开关。

use gpui::prelude::FluentBuilder;

use super::*;

impl Chat {
    pub(crate) fn mc_toggle_skill(&mut self, path: String, disable: bool, cx: &mut Context<Self>) {
        self.mc_toggle_skills_bulk(vec![path], disable, cx);
    }

    /// 批量启停（组头开关）：逐个写 SKILL.md，失败的保持原状并计数报错
    /// (pi-web PATCH /api/skills {filePaths[]} parity)。
    pub(crate) fn mc_toggle_skills_bulk(&mut self, paths: Vec<String>, disable: bool, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        let mut failed = 0usize;
        for path in &paths {
            let p = PathBuf::from(path);
            if let Err(e) = pi_link::skills::set_disable_invocation(&p, disable) {
                failed += 1;
                let _ = e;
            }
        }
        if failed > 0 {
            self.mc_set_error(
                &crate::i18n::tf(
                    "{total} 个技能中有 {count} 个未能更改",
                    &[("total", paths.len().to_string()), ("count", failed.to_string())],
                ),
                cx,
            );
        }
        self.reload_settings_panel();
        cx.notify();
    }
}

/// 休眠（对模型隐藏）技能排后面。
fn order_by_dormancy<'a>(items: &'a [&'a pi_link::skills::SkillEntry]) -> Vec<&'a &'a pi_link::skills::SkillEntry> {
    let mut out: Vec<&&pi_link::skills::SkillEntry> = items.iter().collect();
    out.sort_by_key(|s| s.disable_invocation);
    out
}

/// Skills tab: project/global groups with bulk switches + detail.
pub(crate) fn mc_skills_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
) -> (gpui::AnyElement, gpui::AnyElement) {
    let t = T();
    // px7 + 行内 px8 = 内容左右 15px（与右侧详情 p15 等距，040 扩展页定稿）
    let mut list = sidebar_list().px(px(7.));
    for (label, scope) in [
        (tr("项目"), pi_link::skills::SkillScope::Project),
        (tr("全局"), pi_link::skills::SkillScope::Global),
    ] {
        let items: Vec<&pi_link::skills::SkillEntry> =
            chat.mc_skills.iter().filter(|s| s.scope == scope).collect();
        if items.is_empty() {
            continue;
        }
        let visible = items.iter().filter(|s| !s.disable_invocation).count();
        let all_visible = visible == items.len();
        let bulk_paths: Vec<String> = items
            .iter()
            .map(|s| s.path.to_string_lossy().to_string())
            .collect();
        list = list.child(group_header(
            label,
            Some(group_switch(
                format!("skill-bulk-{}", if scope == pi_link::skills::SkillScope::Project { "p" } else { "g" }),
                weak,
                format!("{visible}/{}", items.len()),
                all_visible,
                false,
                move |c, cx| c.mc_toggle_skills_bulk(bulk_paths.clone(), all_visible, cx),
            )),
        ));
        for sk in order_by_dormancy(&items) {
            let active = sk.path.to_string_lossy() == section;
            let weak_item = weak.clone();
            let path = sk.path.to_string_lossy().to_string();
            list = list.child(
                widgets::sidebar_item(format!("skill-{}", sk.name), active)
                    // 行背景压平（040：列表无底色，选中态只靠加粗+深字色，hover 保留）
                    .bg(rgb(t.bg))
                    .on_mouse_down(MouseButton::Left, {
                        let handler = widgets::select_section(&weak_item, path.clone());
                        move |_, w, cx| handler(w, cx)
                    })
                    .child(status_dot(if sk.disable_invocation { t.border } else { t.accent }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .when(sk.disable_invocation, |d| d.text_color(rgb(t.text_dim)))
                            .child(SharedString::from(sk.name.clone())),
                    ),
            );
        }
    }
    if chat.mc_skills.is_empty() {
        list = list.child(
            div()
                .p(px(12.))
                .text_size(crate::appearance::ui_size(11.))
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
            .p(px(15.))
            .text_size(crate::appearance::ui_size(12.))
            .text_color(rgb(t.text_dim))
            .child(tr("没有找到技能"))
            .into_any_element(),
        Some(sk) => {
            let project = sk.scope == pi_link::skills::SkillScope::Project;
            let visible = !sk.disable_invocation;
            let sw_path = sk.path.to_string_lossy().to_string();
            div()
                .id("mc-detail")
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_y_scroll()
                .p(px(15.))
                .text_size(crate::appearance::ui_size(12.))
                .flex()
                .flex_col()
                .gap_4()
                // 头部：scope 徽标 + 相对路径 + 右上单技能开关
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .min_h(px(28.))
                        .child(scope_tag(if project { tr("项目") } else { tr("全局") }, project))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .font_family(crate::markdown::MONO_FAMILY)
                                .text_size(crate::appearance::ui_size(11.))
                                .text_color(rgb(t.text_dim))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(SharedString::from(sk.path.to_string_lossy().to_string())),
                        )
                        .child(config_switch("skill-switch", weak, visible, false, move |c, cx| {
                            c.mc_toggle_skill(sw_path.clone(), visible, cx)
                        })),
                )
                .child(field(&tr("名称"), mono_text(sk.name.clone(), false)))
                .child(field(
                    &tr("描述"),
                    div()
                        .text_size(crate::appearance::ui_size(12.))
                        .text_color(rgb(t.text_muted))
                        .child(SharedString::from(sk.description.clone())),
                ))
                .child(note(if visible {
                    "在模型提示词中可见；开关关闭后进入休眠（对模型隐藏，仍可手动调用）"
                } else {
                    "对模型隐藏，仍可手动调用；开关打开后恢复可见"
                }))
                .into_any_element()
        }
    };
    (
        sidebar_shell("mc-sidebar")
            // 列表底色压平（040：与页面同色，选中态只靠字重字色）
            .bg(rgb(t.bg))
            .child(list)
            .into_any_element(),
        detail.into_any_element(),
    )
}
