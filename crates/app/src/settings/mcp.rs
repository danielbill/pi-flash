//! MCP tab (pi-web McpConfig subset)：全局 + 项目 mcp.json 的服务器列表
//! （组开关 / 启停 / 移除 / exposure 切换），添加面板（JSON / URL / 命令行
//! 粘贴解析 + 预览），底栏「已开启 n/m」+ 刷新。测试连接 / OAuth 登录 /
//! codemode 为 pi-web 服务端能力，暂不复刻。

use pi_link::mcp::{Scope, ServerEntry};

use super::*;

/// 侧栏条目键：`p:name` / `g:name`（全局与项目可同名，键必须区分）。
pub(crate) fn section_key(s: &ServerEntry) -> String {
    format!("{}:{}", if s.scope == Scope::Project { "p" } else { "g" }, s.name)
}

impl Chat {
    pub(crate) fn mcp_select(&mut self, key: String, cx: &mut Context<Self>) {
        if let Some(st) = self.settings.clone() {
            st.update(cx, |s, cx| {
                s.section = key;
                s.error = None;
                cx.notify();
            });
        }
    }

    /// 启停一个服务器（写它所属的 mcp.json）。
    pub(crate) fn mcp_toggle(&mut self, entry: ServerEntry, enable: bool, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        if let Err(e) = pi_link::mcp::set_enabled(&entry.file, &entry.name, enable) {
            return self.mc_set_error(&e, cx);
        }
        self.reload_settings_panel();
        cx.notify();
    }

    /// 移除一个服务器。
    pub(crate) fn mcp_remove(&mut self, entry: ServerEntry, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        match pi_link::mcp::remove(&entry.file, &entry.name) {
            Ok(_) => {
                if let Some(st) = self.settings.clone() {
                    st.update(cx, |s, _| s.section = "__mcp_add__".into());
                }
                self.reload_settings_panel();
                cx.notify();
            }
            Err(e) => self.mc_set_error(&e, cx),
        }
    }

    /// exposure 四选（codemode 为默认 = 删除键）。
    pub(crate) fn mcp_set_exposure(&mut self, entry: ServerEntry, exposure: &str, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        if let Err(e) = pi_link::mcp::set_exposure(&entry.file, &entry.name, exposure) {
            return self.mc_set_error(&e, cx);
        }
        self.reload_settings_panel();
        cx.notify();
    }

    /// 组开关：对本组每个服务器写 enabled（pi-web set-enabled parity）。
    pub(crate) fn mcp_bulk(&mut self, scope: Scope, enable: bool, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        let mut failed = 0usize;
        for entry in self.mcp_servers.iter().filter(|s| s.scope == scope) {
            if entry.enabled == enable {
                continue;
            }
            if let Err(e) = pi_link::mcp::set_enabled(&entry.file, &entry.name, enable) {
                failed += 1;
                let _ = e;
            }
        }
        if failed > 0 {
            self.mc_set_error(&crate::i18n::tf("{count} 个服务器未能更改", &[("count", failed.to_string())]), cx);
        }
        self.reload_settings_panel();
        cx.notify();
    }

    /// 添加面板提交：解析 → 写入目标 mcp.json（同名替换）→ 选中它。
    pub(crate) fn mcp_add_submit(&mut self, cx: &mut Context<Self>) {
        let Some(st) = self.settings.clone() else { return };
        let (text, name_override, project) = {
            let s = st.read(cx);
            (
                s.mcp_add.read(cx).value().trim().to_string(),
                s.mcp_name.read(cx).value().trim().to_string(),
                s.mcp_scope_project,
            )
        };
        self.mc_clear_error(cx);
        let (name, config) = match pi_link::mcp::parse_server_input(&text) {
            Ok((suggested, config, _)) => {
                (if name_override.is_empty() { suggested } else { name_override }, config)
            }
            Err(e) => return self.mc_set_error(&e, cx),
        };
        if !pi_link::mcp::valid_name(&name) {
            return self.mc_set_error(tr("服务器名称只能使用字母、数字、_ 和 -"), cx);
        }
        let path = if project {
            pi_link::mcp::project_path(&self.cwd)
        } else {
            pi_link::mcp::global_path()
        };
        match pi_link::mcp::add(&path, &name, config) {
            Ok(_) => {
                self.reload_settings_panel();
                let key = self
                    .mcp_servers
                    .iter()
                    .find(|s| s.name == name)
                    .map(section_key)
                    .unwrap_or("__mcp_add__".into());
                if let Some(st) = self.settings.clone() {
                    st.update(cx, |s, cx| {
                        s.section = key;
                        s.mcp_add.update(cx, |ti, cx| ti.set_value(String::new(), cx));
                        s.mcp_name.update(cx, |ti, cx| ti.set_value(String::new(), cx));
                        cx.notify();
                    });
                }
                cx.notify();
            }
            Err(e) => self.mc_set_error(&e, cx),
        }
    }

    /// 示例按钮 → 填充添加框。
    pub(crate) fn mcp_fill_example(&mut self, example: &str, cx: &mut Context<Self>) {
        if let Some(st) = self.settings.clone() {
            let input = st.read(cx).mcp_add.clone();
            input.update(cx, |ti, cx| ti.set_value(example.to_string(), cx));
            cx.notify();
        }
    }
}

/// MCP tab：分栏 + 底栏。
#[allow(clippy::too_many_arguments)]
pub(crate) fn mc_mcp_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
    mcp_add_input: &gpui::Entity<TextInput>,
    mcp_add_value: &str,
    mcp_name: &gpui::Entity<TextInput>,
    mcp_name_value: &str,
    mcp_scope_project: bool,
    error: &Option<String>,
) -> gpui::AnyElement {
    let sb = mcp_sidebar(chat, weak, section);
    let detail = mcp_detail(
        chat, weak, section, mcp_add_input, mcp_add_value, mcp_name, mcp_name_value,
        mcp_scope_project, error,
    );
    let enabled = chat.mcp_servers.iter().filter(|s| s.enabled).count();
    let total = chat.mcp_servers.len();
    let status: gpui::AnyElement = if chat.mcp_errors.is_empty() {
        div().child(SharedString::from(crate::i18n::tf(
            "已开启 {enabled}/{total} 个服务器",
            &[("enabled", enabled.to_string()), ("total", total.to_string())],
        ))).into_any_element()
    } else {
        error_note(&chat.mcp_errors.join("；"))
    };
    div()
        .flex()
        .flex_col()
        .w_full()
        .h_full()
        .min_h_0()
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .child(sb)
                .child(detail),
        )
        .child(footer(
            Some(status),
            vec![config_button("mcp-refresh", weak, &tr("刷新"), Btn::Secondary, true, false, |c, cx| {
                c.reload_settings_panel();
                cx.notify();
            })],
        ))
        .into_any_element()
}

/// 侧栏：项目 / 全局 两组（组开关）+ 添加 MCP。
fn mcp_sidebar(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
) -> gpui::AnyElement {
    let t = T();
    // px7 + 行内 px8 = 内容左右 15px（与右侧详情 p15 等距，040 扩展页定稿）
    let mut list = sidebar_list().px(px(7.));
    for (label, scope) in [(tr("全局"), Scope::Global), (tr("项目"), Scope::Project)] {
        let items: Vec<&ServerEntry> = chat.mcp_servers.iter().filter(|s| s.scope == scope).collect();
        let enabled = items.iter().filter(|s| s.enabled).count();
        let total = items.len();
        list = list.child(group_header(
            label,
            Some(group_switch(
                format!("mcp-bulk-{}", if scope == Scope::Project { "p" } else { "g" }),
                weak,
                format!("{enabled}/{total}"),
                total > 0 && enabled == total,
                total == 0,
                move |c, cx| c.mcp_bulk(scope, enabled != total, cx),
            )),
        ));
        if items.is_empty() {
            list = list.child(
                div()
                    .px(px(8.))
                    .pb(px(4.))
                    .text_size(crate::appearance::ui_size(11.))
                    .text_color(rgb(t.text_dim))
                    .child(tr("没有服务器")),
            );
            continue;
        }
        for entry in items {
            let key = section_key(entry);
            let active = key == section;
            let weak_item = weak.clone();
            let row_key = key.clone();
            list = list.child(
                widgets::sidebar_item(format!("mcp-{}", key), active)
                    // 行背景压平（040：列表无底色，选中态只靠加粗+深字色，hover 保留）
                    .bg(rgb(t.bg))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let _ = weak_item.update(cx, |c, cx| c.mcp_select(row_key.clone(), cx));
                    })
                    .child(status_dot(if entry.enabled { t.accent } else { t.border }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(SharedString::from(entry.name.clone())),
                    )
                    .child(if !entry.enabled {
                        div()
                            .px(px(4.))
                            .py(px(1.))
                            .rounded(px(3.))
                            .bg(gpui::hsla(0., 0., 0.5, 0.12))
                            .text_size(crate::appearance::ui_size(9.))
                            .text_color(rgb(t.text_dim))
                            .child(tr("已关闭"))
                            .into_any_element()
                    } else {
                        div().into_any_element()
                    }),
            );
        }
    }

    // 列表底色压平（040：与页面同色，选中态只靠字重字色）
    sidebar_shell("mc-sidebar")
        .bg(rgb(t.bg))
        .child(list)
        .child(list_action("mcp-add", weak, &tr("添加 MCP"), section == "__mcp_add__", |c, cx| {
            c.mcp_select("__mcp_add__".into(), cx)
        }))
        .into_any_element()
}

/// 详情：添加面板 / 服务器详情 / 空态。
#[allow(clippy::too_many_arguments)]
fn mcp_detail(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
    mcp_add_input: &gpui::Entity<TextInput>,
    mcp_add_value: &str,
    mcp_name: &gpui::Entity<TextInput>,
    mcp_name_value: &str,
    mcp_scope_project: bool,
    error: &Option<String>,
) -> gpui::AnyElement {
    let selected = chat
        .mcp_servers
        .iter()
        .find(|s| section_key(s) == section)
        .cloned();
    match selected {
        Some(entry) => mcp_server_detail(chat, weak, entry, error),
        None if section == "__mcp_add__" => {
            mcp_add_panel(chat, weak, mcp_add_input, mcp_add_value, mcp_name, mcp_name_value, mcp_scope_project, error)
        }
        None => {
            let t = T();
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .p(px(15.))
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(crate::i18n::tf(
                    "还没有 MCP 服务器。{global} 和项目 .pi/mcp.json 中的服务器会显示在这里。",
                    &[("global", pi_link::mcp::global_path().to_string_lossy().to_string())],
                )))
                .into_any_element()
        }
    }
}

/// 服务器详情：头部操作 + 字段网格 + exposure 四选。
fn mcp_server_detail(
    _chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    entry: ServerEntry,
    error: &Option<String>,
) -> gpui::AnyElement {
    let t = T();
    let project = entry.scope == Scope::Project;
    // 详情内边距与列表 15px 统一（040 扩展页定稿；detail_shell 默认 p20）
    let mut detail = detail_shell("mc-detail")
        .p(px(15.))
        .child(
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .min_h(px(28.))
            .child(scope_tag(if project { tr("项目") } else { tr("全局") }, project))
            .child(
                div()
                    .text_size(crate::appearance::ui_size(13.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text))
                    .child(SharedString::from(entry.name.clone())),
            )
            .child(div().flex_1())
            .child(config_switch("mcp-sw", weak, entry.enabled, false, {
                let e = entry.clone();
                move |c, cx| c.mcp_toggle(e.clone(), !e.enabled, cx)
            }))
            .child(config_button("mcp-remove", weak, &tr("移除"), Btn::Danger, true, false, {
                let e = entry.clone();
                move |c, cx| c.mcp_remove(e.clone(), cx)
            })),
    );
    if let Some(err) = error {
        detail = detail.child(error_note(err));
    }
    detail = detail
        .child(grid_row(
            &tr("简介"),
            div().child(SharedString::from(
                entry.description.clone().unwrap_or_else(|| "—".into()),
            )),
        ))
        .child(grid_row(
            &tr("传输方式"),
            div().child(SharedString::from(entry.transport().to_string())),
        ));
    if let Some(cmd) = &entry.command {
        let full = if entry.args.is_empty() {
            cmd.clone()
        } else {
            format!("{cmd} {}", entry.args.join(" "))
        };
        detail = detail.child(grid_row(&tr("命令"), mono_text(full, false)));
    }
    if let Some(url) = &entry.url {
        detail = detail.child(grid_row("URL", mono_text(url.clone(), false)));
    }
    if let Some(cwd) = &entry.cwd {
        detail = detail.child(grid_row(&tr("工作目录"), mono_text(cwd.clone(), false)));
    }
    if !entry.env_names.is_empty() {
        detail = detail.child(grid_row(
            &tr("环境变量"),
            mono_text(entry.env_names.join("  "), false),
        ));
    }
    if !entry.header_names.is_empty() {
        detail = detail.child(grid_row(
            &tr("请求头"),
            mono_text(entry.header_names.join("  "), false),
        ));
    }
    // exposure 四选（codemode = 默认）
    let exposure = entry.exposure.clone().unwrap_or_else(|| "codemode".into());
    const EXPOSURES: [(&str, &str); 4] = [
        ("codemode", "代码模式"),
        ("deferred", "工具搜索"),
        ("direct", "直接声明"),
        ("hidden", "已隐藏"),
    ];
    detail = detail
        .child(field(
            &tr("工具暴露"),
            div().flex().flex_wrap().gap(px(6.)).children(
                EXPOSURES.iter().map(|(value, label)| {
                    let active = exposure == *value;
                    let weak_opt = weak.clone();
                    let e = entry.clone();
                    div()
                        .id(SharedString::from(format!("mcp-exp-{value}")))
                        .h(px(28.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .rounded(px(5.))
                        .border_1()
                        .border_color(rgb(if active { t.accent } else { t.border }))
                        .bg(rgb(if active { t.bg_selected } else { t.bg_panel }))
                        .text_size(crate::appearance::ui_size(11.))
                        .text_color(rgb(if active { t.text } else { t.text_muted }))
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak_opt.update(cx, |c, cx| {
                                c.mcp_set_exposure(e.clone(), value, cx)
                            });
                        })
                        .child(SharedString::from(label.to_string()))
                }),
            ),
        ))
        .child(grid_row(
            &tr("文件"),
            mono_text(entry.file.to_string_lossy().to_string(), true),
        ));
    detail.into_any_element()
}

/// 添加面板：scope 双选 + 目标路径 + 粘贴解析 + 预览 + 名称 + 添加。
#[allow(clippy::too_many_arguments)]
fn mcp_add_panel(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    mcp_add_input: &gpui::Entity<TextInput>,
    mcp_add_value: &str,
    mcp_name: &gpui::Entity<TextInput>,
    mcp_name_value: &str,
    mcp_scope_project: bool,
    error: &Option<String>,
) -> gpui::AnyElement {
    let t = T();
    let weak_scope = weak.clone();
    const EXAMPLES: [&str; 3] = [
        r#"{"mcpServers":{"docs":{"url":"https://example.com/mcp"}}}"#,
        "npx -y @modelcontextprotocol/server-filesystem .",
        "https://mcp.example.com/sse",
    ];
    let target = if mcp_scope_project {
        pi_link::mcp::project_path(&chat.cwd)
    } else {
        pi_link::mcp::global_path()
    };

    // 详情内边距与列表 15px 统一（040 扩展页定稿；detail_shell 默认 p20）
    let mut detail = detail_shell("mc-detail")
        .p(px(15.))
        .child(section_title(&tr("添加 MCP")))
        .child(note("粘贴 JSON / http(s) URL / 命令行，或 `pi mcp add …`；添加后写入 mcp.json"))
        .child(mcp_add_input.clone())
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child({
                    let mk = |ix: &'static str, label: &str, project: bool| {
                        let weak_opt = weak_scope.clone();
                        div()
                            .id(SharedString::from(format!("mcp-scope-{ix}")))
                            .h(px(28.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded(px(5.))
                            .border_1()
                            .border_color(rgb(if mcp_scope_project == project { t.accent } else { t.border }))
                            .bg(rgb(if mcp_scope_project == project { t.bg_selected } else { t.bg_panel }))
                            .text_size(crate::appearance::ui_size(11.))
                            .text_color(rgb(t.text))
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let _ = weak_opt.update(cx, |c, cx| {
                                    if let Some(st) = c.settings.clone() {
                                        st.update(cx, |s, cx| {
                                            s.mcp_scope_project = project;
                                            cx.notify();
                                        });
                                    }
                                });
                            })
                            .child(SharedString::from(label.to_string()))
                    };
                    div().flex().gap(px(6.)).child(mk("g", tr("全局"), false)).child(mk("p", tr("项目"), true))
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .font_family(crate::markdown::MONO_FAMILY)
                        .text_size(crate::appearance::ui_size(10.))
                        .text_color(rgb(t.text_dim))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(SharedString::from(target.to_string_lossy().to_string())),
                ),
        );

    // 实时解析预览
    let parsed = if mcp_add_value.trim().is_empty() {
        None
    } else {
        pi_link::mcp::parse_server_input(mcp_add_value).ok()
    };
    if let Some((suggested, config, source)) = parsed {
        let transport = if config.get("url").is_some() { "HTTP" } else { "stdio" };
        let target_desc = config
            .get("url")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| {
                config.get("command").and_then(|v| v.as_str()).map(|cmd| {
                    let args: Vec<&str> = config
                        .get("args")
                        .and_then(|a| a.as_array())
                        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                        .unwrap_or_default();
                    if args.is_empty() { cmd.to_string() } else { format!("{cmd} {}", args.join(" ")) }
                })
            })
            .unwrap_or_default();
        let source_label = match source {
            pi_link::mcp::ParsedSource::Json => "JSON",
            pi_link::mcp::ParsedSource::Url => "URL",
            pi_link::mcp::ParsedSource::CommandLine => "命令行",
        };
        let preview_name = if mcp_name_value.is_empty() { suggested.clone() } else { mcp_name_value.to_string() };
        detail = detail
            .child(
                div()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(t.border))
                    .bg(rgb(t.bg_panel))
                    .p(px(10.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(section_title(&tr("预览")))
                    .child(grid_row(&tr("识别为"), div().child(source_label)))
                    .child(grid_row(&tr("传输方式"), div().child(transport)))
                    .child(grid_row(&tr("URL / 命令"), mono_text(target_desc, false)))
                    .child(grid_row(&tr("名称"), div().child(SharedString::from(preview_name)))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(field(&tr("名称"), div().w(px(240.)).child(mcp_name.clone())))
                    .child(note("留空使用识别出的名称")),
            );
    }

    detail = detail
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(6.))
                .children(EXAMPLES.iter().map(|ex| {
                    let weak_ex = weak.clone();
                    div()
                        .id(SharedString::from(format!("mcp-ex-{ex}")))
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
                            let _ = weak_ex.update(cx, |c, cx| c.mcp_fill_example(ex, cx));
                        })
                        .child(SharedString::from(*ex))
                })),
        )
        .children(error.as_ref().map(|e| error_note(e)))
        .child(
            div()
                .pt(px(4.))
                .child(config_button("mcp-add-go", weak, &tr("添加"), Btn::Primary, false, mcp_add_value.trim().is_empty(), |c, cx| {
                    c.mcp_add_submit(cx)
                })),
        );
    detail.into_any_element()
}
