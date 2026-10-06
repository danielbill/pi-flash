//! Plugins tab (pi-web PluginsConfig subset)：全局/项目分组 + 组头批量开关
//! （设置了资源过滤的包在关组时保持启用），详情网格（状态/资源/安装路径），
//! 安装面板带示例；安装/移除仍走 vendored pi CLI。


use super::*;

impl Chat {
    /// Enable/disable a package: zero out its resource arrays (pi-web
    /// disable parity) in the owning scope's settings.json.
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

    /// 组头批量开关。关组时「设置了资源过滤」的包保持启用
    /// (pi-web packagesToSwitch / filteredPackagesKeptOn parity)。
    pub(crate) fn mc_toggle_packages_bulk(&mut self, scope_project: bool, enable: bool, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        let list = if scope_project { &self.mc_pkgs_project } else { &self.mc_pkgs_global };
        let mut kept_filtered = 0usize;
        let mut changed = 0usize;
        let next: Vec<serde_json::Value> = list
            .iter()
            .enumerate()
            .map(|(_ix, e)| {
                let disabled = pi_link::skills::entry_disabled(e);
                if disabled == !enable {
                    return e.clone(); // already in the target state
                }
                // partial resource filters keep such packages on
                if !enable && entry_filtered(e) {
                    kept_filtered += 1;
                    return e.clone();
                }
                changed += 1;
                if enable {
                    serde_json::Value::String(pi_link::skills::entry_source(e))
                } else {
                    serde_json::json!({
                        "source": pi_link::skills::entry_source(e),
                        "extensions": [], "skills": [], "prompts": [], "themes": []
                    })
                }
            })
            .collect();
        if changed == 0 {
            return;
        }
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
        if kept_filtered > 0 {
            self.mc_set_error(
                &crate::i18n::tf(
                    "{count} 个设置了资源过滤的包保持启用",
                    &[("count", kept_filtered.to_string())],
                ),
                cx,
            );
        }
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
        if let Some(st) = self.settings.clone() {
            st.update(cx, |s, _| s.section = "__add__".into());
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

    /// 示例按钮 → 填充安装输入框。
    pub(crate) fn mc_fill_install_example(&mut self, example: &str, cx: &mut Context<Self>) {
        if let Some(st) = self.settings.clone() {
            let input = st.read(cx).install_input.clone();
            input.update(cx, |ti, cx| ti.set_value(example.to_string(), cx));
            cx.notify();
        }
    }
}

/// 「资源过滤」包：对象条目且至少一个资源数组非空（partial disable 之外）。
fn entry_filtered(entry: &serde_json::Value) -> bool {
    let Some(obj) = entry.as_object() else { return false };
    ["extensions", "skills", "prompts", "themes"]
        .iter()
        .any(|k| obj.get(*k).and_then(|v| v.as_array()).map(|a| !a.is_empty()).unwrap_or(false))
}

/// 安装路径 best-effort 推断（pi CLI 约定：全局 ~/.pi/agent/{npm,git}）。
fn install_path_hint(source: &str) -> String {
    let agent_dir = pi_link::config::agent_dir();
    if let Some(pkg) = source.strip_prefix("npm:") {
        return agent_dir.join("npm").join("node_modules").join(pkg).to_string_lossy().to_string();
    }
    if let Some(url) = source.strip_prefix("git:") {
        let repo = url.trim_end_matches(".git").rsplit('/').next().unwrap_or("repo");
        return agent_dir.join("git").join(repo).to_string_lossy().to_string();
    }
    source.to_string()
}

/// Plugins tab: scope-grouped package list + install form / package detail.
pub(crate) fn mc_plugins_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
    install_input: &gpui::Entity<TextInput>,
    install_scope_project: bool,
) -> gpui::AnyElement {
    let t = T();
    let entries: Vec<(bool, usize, &serde_json::Value)> = chat
        .mc_pkgs_global
        .iter()
        .enumerate()
        .map(|(i, v)| (false, i, v))
        .chain(chat.mc_pkgs_project.iter().enumerate().map(|(i, v)| (true, i, v)))
        .collect();

    let sb = pl_sidebar(chat, weak, section, &entries, t);
    let detail = pl_detail(chat, weak, section, &entries, install_input, install_scope_project, t);

    // 底栏：资源总计 + 刷新（单独一行拼在分栏下）
    let (mut ext, mut sk, mut pr, mut th) = (0usize, 0usize, 0usize, 0usize);
    for (_, _, v) in &entries {
        if pi_link::skills::entry_disabled(v) {
            continue;
        }
        let (a, b, c, d) = pi_link::skills::entry_resource_counts(v);
        ext += a;
        sk += b;
        pr += c;
        th += d;
    }
    let footer = footer(
        Some(
            div()
                .font_family(crate::markdown::MONO_FAMILY)
                .child(SharedString::from(crate::i18n::tf(
                    "{ext}扩展 · {sk}技能 · {pr}提示词 · {th}主题",
                    &[
                        ("ext", ext.to_string()),
                        ("sk", sk.to_string()),
                        ("pr", pr.to_string()),
                        ("th", th.to_string()),
                    ],
                )))
                .into_any_element(),
        ),
        vec![config_button("pl-refresh", weak, &tr("刷新"), Btn::Secondary, true, false, |c, cx| {
            c.reload_settings_panel();
            cx.notify();
        })],
    );
    div()
        .flex()
        .flex_col()
        .w_full()
        .h_full()
        .min_h_0()
        .child(two_pane(sb, detail))
        .child(footer)
        .into_any_element()
}

fn two_pane(sidebar: gpui::AnyElement, detail: gpui::AnyElement) -> gpui::AnyElement {
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .child(sidebar)
        .child(detail)
        .into_any_element()
}

/// Plugins sidebar：全局 / 项目 两组，组头 {enabled}/{total} + 批量开关。
fn pl_sidebar(
    _chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
    entries: &[(bool, usize, &serde_json::Value)],
    t: &crate::theme::Theme,
) -> gpui::AnyElement {
    let mut list = sidebar_list();
    for (label, scope_project) in [(tr("全局"), false), (tr("项目"), true)] {
        let items: Vec<&(bool, usize, &serde_json::Value)> =
            entries.iter().filter(|(proj, _, _)| *proj == scope_project).collect();
        if items.is_empty() {
            continue;
        }
        let enabled = items.iter().filter(|(_, _, v)| !pi_link::skills::entry_disabled(v)).count();
        let total = items.len();
        list = list.child(group_header(
            label,
            Some(group_switch(
                format!("pl-bulk-{}", if scope_project { "p" } else { "g" }),
                weak,
                format!("{enabled}/{total}"),
                enabled == total,
                false,
                move |c, cx| c.mc_toggle_packages_bulk(scope_project, enabled != total, cx),
            )),
        ));
        for (proj, ix, v) in items {
            let src = pi_link::skills::entry_source(v);
            let disabled = pi_link::skills::entry_disabled(v);
            let active = src == section;
            let weak_item = weak.clone();
            let item_src = src.clone();
            list = list.child(
                widgets::sidebar_item(format!("pkg-{ix}-{}", src), active)
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let _ = weak_item.update(cx, |c, cx| {
                            if let Some(st) = c.settings.clone() {
                                st.update(cx, |s, cx| {
                                    s.section = item_src.clone();
                                    s.error = None;
                                    cx.notify();
                                });
                            }
                        });
                    })
                    .child(status_dot(if disabled { t.border } else { t.accent }))
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
                            .px(px(4.))
                            .py(px(1.))
                            .rounded(px(3.))
                            .bg(widgets::indigo_bg())
                            .text_size(crate::appearance::ui_size(9.))
                            .text_color(widgets::indigo_fg())
                            .child(tr("项目"))
                            .into_any_element()
                    } else {
                        div().into_any_element()
                    }),
            );
        }
    }

    sidebar_shell("mc-sidebar")
        .child(list)
        .child(list_action("pkg-add", weak, &tr("添加插件"), section == "__add__", |c, cx| {
            if let Some(st) = c.settings.clone() {
                st.update(cx, |s, cx| {
                    s.section = "__add__".into();
                    s.error = None;
                    cx.notify();
                });
            }
        }))
        .into_any_element()
}

/// Plugins detail：安装面板（示例）或包详情网格。
fn pl_detail(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
    entries: &[(bool, usize, &serde_json::Value)],
    install_input: &gpui::Entity<TextInput>,
    install_scope_project: bool,
    t: &crate::theme::Theme,
) -> gpui::AnyElement {
    if section == "__add__" || entries.is_empty() {
        return pl_add_panel(chat, weak, install_input, install_scope_project, t);
    }
    let Some((proj, ix, v)) = entries
        .iter()
        .find(|(_, _, v)| pi_link::skills::entry_source(v) == section)
        .map(|(p, i, v)| (*p, *i, *v))
        .or_else(|| entries.first().map(|(p, i, v)| (*p, *i, *v)))
    else {
        return div()
            .flex_1()
            .p(px(20.))
            .text_size(crate::appearance::ui_size(12.))
            .text_color(rgb(t.text_dim))
            .child(tr("没有已配置的插件"))
            .into_any_element();
    };
    let src = pi_link::skills::entry_source(v);
    let disabled = pi_link::skills::entry_disabled(v);
    let (ext, sk, pr, th) = pi_link::skills::entry_resource_counts(v);
    let detail = detail_shell("mc-detail")
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .min_h(px(28.))
                .child(scope_tag(if proj { tr("项目") } else { tr("全局") }, proj))
                .child(
                    div()
                        .font_family(crate::markdown::MONO_FAMILY)
                        .text_size(crate::appearance::ui_size(12.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(t.text))
                        .child(SharedString::from(src.clone())),
                )
                .child(if disabled {
                    div()
                        .px(px(5.))
                        .py(px(1.))
                        .rounded(px(3.))
                        .bg(gpui::hsla(0., 0., 0.5, 0.12))
                        .text_size(crate::appearance::ui_size(10.))
                        .text_color(rgb(t.text_dim))
                        .child(tr("已禁用"))
                        .into_any_element()
                } else {
                    div().into_any_element()
                })
                .child(div().flex_1())
                .child(config_switch("pkg-switch", weak, !disabled, false, move |c, cx| {
                    c.mc_toggle_package(proj, ix, cx)
                }))
                .child(config_button("pkg-remove", weak, &tr("移除"), Btn::Danger, true, false, {
                    let s = src.clone();
                    move |c, cx| c.mc_remove_package(proj, s.clone(), cx)
                })),
        )
        .child(grid_row(
            &tr("状态"),
            div().child(if disabled {
                SharedString::from(tr("已禁用（资源不加载）").to_string())
            } else {
                SharedString::from(tr("已启用").to_string())
            }),
        ))
        .child(grid_row(
            &tr("来源"),
            mono_text(src.clone(), false),
        ))
        .child(grid_row(
            &tr("资源"),
            div()
                .font_family(crate::markdown::MONO_FAMILY)
                .child(SharedString::from(crate::i18n::tf(
                    "{ext}扩展 · {sk}技能 · {pr}提示词 · {th}主题",
                    &[
                        ("ext", ext.to_string()),
                        ("sk", sk.to_string()),
                        ("pr", pr.to_string()),
                        ("th", th.to_string()),
                    ],
                ))),
        ))
        .child(grid_row(&tr("安装路径"), mono_text(install_path_hint(&src), true)))
        .child(note("移除/安装通过 vendored pi CLI 执行（pi remove/install），完成后自动刷新"))
        .into_any_element();
    detail
}

/// 安装面板（ConfigAddSourcePanel 精简）：scope 双选 + 输入 + 示例 + 安装。
fn pl_add_panel(
    _chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    install_input: &gpui::Entity<TextInput>,
    install_scope_project: bool,
    t: &crate::theme::Theme,
) -> gpui::AnyElement {
    const EXAMPLES: [&str; 3] = [
        "npm:@scope/pi-plugin",
        "git:https://github.com/user/repo",
        "C:\\path\\to\\plugin",
    ];
    let weak_scope = weak.clone();
    let weak_examples = weak.clone();
    detail_shell("mc-detail")
        .child(section_title(&tr("添加插件")))
        .child(note("目录：pi.dev/packages；支持 npm:@scope/pkg · git:URL · 本地绝对路径"))
        .child(install_input.clone())
        .child(
            div()
                .flex()
                .gap(px(6.))
                .child({
                    let mk = |ix: &'static str, label: &str, project: bool| {
                        let weak_opt = weak_scope.clone();
                        div()
                            .id(SharedString::from(format!("pkg-scope-{ix}")))
                            .h(px(28.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded(px(5.))
                            .border_1()
                            .border_color(rgb(if install_scope_project == project { t.accent } else { t.border }))
                            .bg(rgb(if install_scope_project == project { t.bg_selected } else { t.bg_panel }))
                            .text_size(crate::appearance::ui_size(11.))
                            .text_color(rgb(t.text))
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let _ = weak_opt.update(cx, |c, cx| {
                                    if let Some(st) = c.settings.clone() {
                                        st.update(cx, |s, cx| {
                                            s.install_scope_project = project;
                                            cx.notify();
                                        });
                                    }
                                });
                            })
                            .child(SharedString::from(label.to_string()))
                    };
                    div().child(mk("g", tr("全局"), false)).child(mk("p", tr("项目"), true))
                })
                .child(div().flex_1())
                .child(config_button("pkg-install-go", weak, &tr("安装"), Btn::Primary, false, false, |c, cx| {
                    let st = c.settings.clone();
                    let src = st
                        .as_ref()
                        .map(|st| st.read(cx).install_input.clone())
                        .map(|input| input.read(cx).value().to_string())
                        .unwrap_or_default();
                    let proj = st
                        .as_ref()
                        .map(|st| st.read(cx).install_scope_project)
                        .unwrap_or(false);
                    c.mc_install_package(src, proj, cx);
                })),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(6.))
                .child(note(&tr("示例：")))
                .children(EXAMPLES.iter().map(|ex| {
                    let weak_ex = weak_examples.clone();
                    div()
                        .id(SharedString::from(format!("pl-ex-{ex}")))
                        .px(px(8.))
                        .py(px(3.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(rgb(t.border))
                        .font_family(crate::markdown::MONO_FAMILY)
                        .text_size(crate::appearance::ui_size(10.))
                        .text_color(rgb(t.text_dim))
                        .cursor_pointer()
                        .hover(|s| s.border_color(rgb(t.accent)).text_color(rgb(t.text)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak_ex.update(cx, |c, cx| c.mc_fill_install_example(ex, cx));
                        })
                        .child(SharedString::from(*ex))
                })),
        )
        .child(note("安装位置：全局 ~/.pi/agent/{npm,git}；项目 <工作区>/.pi/agent/{npm,git}"))
        .into_any_element()
}
