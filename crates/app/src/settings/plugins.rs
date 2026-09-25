//! Plugins tab (install/remove via pi CLI).

use super::*;

    /// Enable/disable a package: zero out its resource arrays (pi-web
    /// disable parity) in the owning scope's settings.json.
impl Chat {
    pub(crate) fn mc_toggle_package(&mut self, scope_project: bool, ix: usize, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        let list = if scope_project { &self.mc_pkgs_project } else { &self.mc_pkgs_global };
        let Some(entry) = list.get(ix) else { return };
        let source = pi_link::skills::entry_source(entry);
        let next: Vec<serde_json::Value> = list
            .iter()
            .enumerate()
            .map(|(i, e)| {
                if i != ix {
                    return e.clone();
                }
                if pi_link::skills::entry_disabled(e) {
                    // enable: restore the plain source entry (loader re-resolves)
                    serde_json::Value::String(source.clone())
                } else {
                    // disable: keep the entry but load nothing
                    serde_json::json!({
                        "source": source,
                        "extensions": [], "skills": [], "prompts": [], "themes": []
                    })
                }
            })
            .collect();
        let path = if scope_project {
            pi_link::config::project_settings_path(&self.cwd)
        } else {
            pi_link::config::settings_path()
        };
        if let Err(e) = pi_link::config::write_packages(&path, next) {
            self.mc_set_error(&crate::i18n::tf("写入 settings.json 失败: {e}", &[("e", e)]), cx);
            return;
        }
        self.reload_settings_panel();
        cx.notify();
    }

    /// Install/remove via the vendored pi CLI, on a background thread; the
    /// result lands on the op pump (status line + panel refresh).
    pub(crate) fn mc_install_package(&mut self, source: String, scope_project: bool, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        let source = pi_link::skills::normalize_source(&source);
        if source.is_empty() {
            self.mc_set_error(tr("请输入插件来源（npm: / git: / 本地路径）"), cx);
            return;
        }
        let mut args = vec!["install".to_string(), source.clone()];
        if scope_project {
            args.push("-l".to_string());
        }
        self.mc_cli_op(args, crate::i18n::tf("已安装 {source}", &[("source", source.clone())]), cx);
        if let Some(Dialog::Settings { section, .. }) = &mut self.dialog {
            *section = "__add__".into();
        }
    }

    pub(crate) fn mc_remove_package(&mut self, scope_project: bool, source: String, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        let mut args = vec!["remove".to_string(), source.clone()];
        if scope_project {
            args.push("-l".to_string());
        }
        self.mc_cli_op(args, crate::i18n::tf("已移除 {source}", &[("source", source.clone())]), cx);
    }

}

/// Plugins tab: scope-grouped package list + install form / package detail.
pub(crate) fn mc_plugins_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
    install_input: &gpui::Entity<TextInput>,
    install_scope_project: bool,
) -> (gpui::AnyElement, gpui::AnyElement) {
    let t = T();
    let entries: Vec<(bool, usize, &serde_json::Value)> = chat
        .mc_pkgs_global
        .iter()
        .enumerate()
        .map(|(i, v)| (false, i, v))
        .chain(chat.mc_pkgs_project.iter().enumerate().map(|(i, v)| (true, i, v)))
        .collect();

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
        .overflow_y_scroll()
        .children(entries.iter().map(|(proj, ix, v)| {
            let src = pi_link::skills::entry_source(v);
            let disabled = pi_link::skills::entry_disabled(v);
            let active = src == section;
            let weak_item = weak.clone();
            let item_src = src.clone();
            let item_proj = *proj;
            div()
                .id(SharedString::from(format!("pkg-{ix}-{}", src)))
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
                        if let Some(Dialog::Settings { section, error, .. }) = &mut c.dialog {
                            *section = item_src.clone();
                            *error = None;
                            let _ = item_proj;
                            cx.notify();
                        }
                    });
                })
                .child(
                    div()
                        .size(px(6.))
                        .rounded_full()
                        .flex_shrink_0()
                        .bg(if disabled { rgb(t.border) } else { rgb(t.accent) }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(SharedString::from(src.clone())),
                )
                .child(if *proj {
                    div()
                        .px(px(5.))
                        .py(px(1.))
                        .rounded(px(3.))
                        .bg(gpui::hsla(0.63, 0.86, 0.62, 0.12))
                        .text_size(px(9.))
                        .text_color(gpui::hsla(0.63, 0.86, 0.62, 0.85))
                        .child(tr("项目"))
                        .into_any_element()
                } else {
                    div().into_any_element()
                })
        }));

    // "add plugin" list action (ConfigListAction parity)
    let weak_add = weak.clone();
    sb = sb.child(
        div()
            .id("pkg-add")
            .mt_auto()
            .px(px(6.))
            .pt(px(8.))
            .border_t_1()
            .border_color(rgb(t.border))
            .child(
                div()
                    .h(px(30.))
                    .px(px(8.))
                    .rounded(px(5.))
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .text_size(px(12.))
                    .cursor_pointer()
                    .text_color(if section == "__add__" { rgb(t.accent) } else { rgb(t.text_dim) })
                    .hover(|s| s.bg(rgb(t.bg_hover)))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let _ = weak_add.update(cx, |c, cx| {
                            if let Some(Dialog::Settings { section, error, .. }) = &mut c.dialog {
                                *section = "__add__".into();
                                *error = None;
                                cx.notify();
                            }
                        });
                    })
                    .child(icon("plus", 13., t.text_dim))
                    .child(tr("添加插件")),
            ),
    );

    let detail = if section == "__add__" || entries.is_empty() {
        // install form
        let weak_scope = weak.clone();
        let weak_go = weak.clone();
        let scope_project = install_scope_project;
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
            .gap_3()
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text))
                    .child(tr("添加插件")),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(t.text_dim))
                    .child(tr("npm:@scope/pi-plugin · git:https://... · /绝对路径")),
            )
            .child(install_input.clone())
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child({
                        let weak_g = weak_scope.clone();
                        div()
                            .id("pkg-scope-global")
                            .h(px(28.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded(px(5.))
                            .border_1()
                            .border_color(if !scope_project { rgb(t.accent) } else { rgb(t.border) })
                            .bg(if !scope_project { rgb(t.bg_selected) } else { rgb(t.bg_panel) })
                            .text_size(px(11.))
                            .text_color(rgb(t.text))
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let _ = weak_g.update(cx, |c, cx| {
                                    if let Some(Dialog::Settings { install_scope_project, .. }) = &mut c.dialog {
                                        *install_scope_project = false;
                                        cx.notify();
                                    }
                                });
                            })
                            .child(tr("全局"))
                    })
                    .child({
                        let weak_p = weak_scope.clone();
                        div()
                            .id("pkg-scope-project")
                            .h(px(28.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded(px(5.))
                            .border_1()
                            .border_color(if scope_project { rgb(t.accent) } else { rgb(t.border) })
                            .bg(if scope_project { rgb(t.bg_selected) } else { rgb(t.bg_panel) })
                            .text_size(px(11.))
                            .text_color(rgb(t.text))
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let _ = weak_p.update(cx, |c, cx| {
                                    if let Some(Dialog::Settings { install_scope_project, .. }) = &mut c.dialog {
                                        *install_scope_project = true;
                                        cx.notify();
                                    }
                                });
                            })
                            .child(tr("项目"))
                    }),
            )
            .child(
                div()
                    .id("pkg-install-go")
                    .w(px(96.))
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(5.))
                    .border_1()
                    .border_color(rgb(t.accent))
                    .bg(rgb(t.accent))
                    .text_size(px(12.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.accent_contrast))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(t.accent_hover)))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let _ = weak_go.update(cx, |c, cx| {
                            let src = c
                                .dialog
                                .as_ref()
                                .and_then(|d| match d {
                                    Dialog::Settings { install_input, .. } => {
                                        Some(install_input.read(cx).value().to_string())
                                    }
                                    _ => None,
                                })
                                .unwrap_or_default();
                            let proj = c
                                .dialog
                                .as_ref()
                                .and_then(|d| match d {
                                    Dialog::Settings { install_scope_project, .. } => Some(*install_scope_project),
                                    _ => None,
                                })
                                .unwrap_or(false);
                            c.mc_install_package(src, proj, cx);
                        });
                    })
                    .child(tr("安装")),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(t.text_dim))
                    .child(tr("安装位置：全局 ~/.pi/agent/{npm,git}；项目 <工作区>/.pi/agent/{npm,git}")),
            )
            .into_any_element()
    } else {
        // package detail
        let Some((proj, ix, v)) = entries
            .iter()
            .find(|(_, _, v)| pi_link::skills::entry_source(v) == section)
            .map(|(p, i, v)| (*p, *i, *v))
            .or_else(|| entries.first().map(|(p, i, v)| (*p, *i, *v)))
        else {
            return (
                sb.into_any_element(),
                div().flex_1().p(px(20.)).text_size(px(12.)).text_color(rgb(t.text_dim)).child(tr("没有已配置的插件")).into_any_element(),
            );
        };
        let src = pi_link::skills::entry_source(v);
        let disabled = pi_link::skills::entry_disabled(v);
        let (ext, sk, pr, th) = pi_link::skills::entry_resource_counts(v);
        let weak_sw = weak.clone();
        let weak_del = weak.clone();
        let (sw_proj, sw_ix, sw_disabled) = (proj, ix, disabled);
        let (del_proj, del_src) = (proj, src.clone());
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
                            .child(SharedString::from(src.clone())),
                    )
                    .child(
                        div()
                            .px(px(5.))
                            .py(px(1.))
                            .rounded(px(3.))
                            .bg(if proj { gpui::hsla(0.63, 0.86, 0.62, 0.12) } else { gpui::hsla(0., 0., 0.5, 0.12) })
                            .text_size(px(10.))
                            .text_color(if proj { gpui::hsla(0.63, 0.86, 0.62, 0.85) } else { rgb(t.text_dim).into() })
                            .child(if proj { tr("项目") } else { tr("全局") }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(11.))
                    .text_color(rgb(t.text_dim))
                    .child(if disabled { tr("已停用") } else { tr("已加载") })
                    .child(
                        div()
                            .font_family("Consolas")
                            .child(SharedString::from(format!("ext {ext} · skills {sk} · prompts {pr} · themes {th}"))),
                    ),
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
                            .child(if disabled { tr("已停用（资源不加载）") } else { tr("已启用") }),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("pkg-switch")
                            .w(px(32.))
                            .h(px(18.))
                            .rounded(px(9.))
                            .border_1()
                            .border_color(if !disabled { rgb(t.accent) } else { rgb(t.border) })
                            .bg(if !disabled { rgb(t.accent) } else { rgb(t.bg_selected) })
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .child(
                                div()
                                    .ml(if !disabled { px(14.) } else { px(2.) })
                                    .size(px(12.))
                                    .rounded_full()
                                    .bg(if !disabled { rgb(t.bg) } else { rgb(t.text_muted) }),
                            )
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let _ = weak_sw.update(cx, |c, cx| {
                                    c.mc_toggle_package(sw_proj, sw_ix, cx);
                                    let _ = sw_disabled;
                                });
                            }),
                    ),
            )
            .child(
                div()
                    .id("pkg-remove")
                    .w(px(64.))
                    .h(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(5.))
                    .border_1()
                    .border_color(rgb(0xef4444))
                    .bg(gpui::hsla(0., 0.84, 0.6, 0.06))
                    .text_size(px(11.))
                    .text_color(rgb(0xef4444))
                    .cursor_pointer()
                    .hover(|s| s.bg(gpui::hsla(0., 0.84, 0.6, 0.12)))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let _ = weak_del.update(cx, |c, cx| c.mc_remove_package(del_proj, del_src.clone(), cx));
                    })
                    .child(tr("移除")),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(t.text_dim))
                    .child(tr("移除/安装通过 vendored pi CLI 执行（pi remove/install）")),
            )
            .into_any_element()
    };
    (sb.into_any_element(), detail.into_any_element())
}

