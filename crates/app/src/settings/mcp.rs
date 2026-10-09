//! MCP tab（043 定稿）：只做全局配置（`~/.pi/agent/mcp.json`）。左侧注册
//! 列表（总开关 + 单独开关，点行进编辑），右侧统一【配置MCP】表单——
//! 新增与编辑同一张：名称 + 8 行粘贴框（JSON / URL / 命令行 / pi mcp add
//! 四形态自动识别）+ 实时解析反馈 + exposure 四选 + 四种纯文本示例。
//! 保存 = 解析归一写盘；改名 = 删旧增新。一切改动只对新会话生效。
//! 测试连接 / OAuth 登录为二期（`pi mcp list --json` / `pi mcp login`）。

use pi_link::mcp::{Scope, ServerEntry};

use super::*;

/// 侧栏条目键：`g:name`（043 起只列全局；键格式保留 scope 前缀兼容旧 state）。
pub(crate) fn section_key(s: &ServerEntry) -> String {
    format!("{}:{}", if s.scope == Scope::Project { "p" } else { "g" }, s.name)
}

/// exposure 四选（codemode = 默认 = 删键）。
const EXPOSURES: [(&str, &str); 4] = [
    ("codemode", "代码模式"),
    ("deferred", "工具搜索"),
    ("direct", "直接声明"),
    ("hidden", "已隐藏"),
];

impl Chat {
    /// 编辑回填数据：选中行 → (名称, 原始 JSON, exposure 索引)。原始 JSON
    /// 保 env 值与未知字段（ServerEntry 重建会丢）。
    pub(crate) fn mcp_backfill_data(&self, key: &str) -> Option<(String, String, u8)> {
        let e = self.mcp_servers.iter().find(|s| section_key(s) == key)?;
        let raw = pi_link::mcp::raw_entry(&e.file, &e.name).ok().flatten()?;
        let text = serde_json::to_string_pretty(&raw).ok()?;
        let ix = EXPOSURES
            .iter()
            .position(|(v, _)| Some(v.to_string()) == e.exposure)
            .unwrap_or(0);
        Some((e.name.clone(), text, ix as u8))
    }

    pub(crate) fn mcp_select(&mut self, key: String, cx: &mut Context<Self>) {
        let data = self.mcp_backfill_data(&key);
        if let Some(st) = self.settings.clone() {
            st.update(cx, |s, cx| {
                s.section = key;
                s.error = None;
                mcp_apply_backfill(s, data, cx);
                cx.notify();
            });
        }
    }

    /// 启停一个服务器（行内开关；写它所属的 mcp.json）。
    pub(crate) fn mcp_toggle(&mut self, entry: ServerEntry, enable: bool, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        if let Err(e) = pi_link::mcp::set_enabled(&entry.file, &entry.name, enable) {
            return self.mc_set_error(&e, cx);
        }
        self.reload_settings_panel();
        cx.notify();
    }

    /// 移除一个服务器 → 回到新增表单。
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

    /// 总开关：全局组逐个写 enabled。
    pub(crate) fn mcp_bulk(&mut self, enable: bool, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        let mut failed = 0usize;
        for entry in self.mcp_servers.iter().filter(|s| s.scope == Scope::Global) {
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

    /// 【保存】提交：解析 → exposure 写入 → 新增 / 编辑（改名删旧增新）
    /// → 只写全局 mcp.json。043：一切改动只对新会话生效。
    pub(crate) fn mcp_save_submit(&mut self, cx: &mut Context<Self>) {
        let Some(st) = self.settings.clone() else { return };
        let (text, name_override, exposure_ix, section) = {
            let s = st.read(cx);
            (
                s.mcp_add.read(cx).value().trim().to_string(),
                s.mcp_name.read(cx).value().trim().to_string(),
                s.mcp_exposure,
                s.section.clone(),
            )
        };
        self.mc_clear_error(cx);
        if text.is_empty() {
            return self.mc_set_error("粘贴 JSON、URL 或命令行", cx);
        }
        let (name, mut config) = match pi_link::mcp::parse_server_input(&text) {
            Ok((suggested, config, _)) => {
                (if name_override.is_empty() { suggested } else { name_override }, config)
            }
            Err(e) => return self.mc_set_error(&e, cx),
        };
        if !pi_link::mcp::valid_name(&name) {
            return self.mc_set_error(tr("服务器名称只能使用字母、数字、_ 和 -"), cx);
        }
        // exposure 四选是保存期的 source of truth（codemode = 删键）
        if let Some(obj) = config.as_object_mut() {
            if exposure_ix == 0 {
                obj.remove("exposure");
            } else {
                obj.insert("exposure".into(), serde_json::Value::String(EXPOSURES[exposure_ix as usize].0.into()));
            }
        }
        let path = pi_link::mcp::global_path();
        // 编辑模式且改了名 → 删旧条目（旧条目必在全局文件）
        if let Some(old) = self
            .mcp_servers
            .iter()
            .find(|s| section_key(s) == section && s.scope == Scope::Global)
            .cloned()
        {
            if old.name != name {
                if let Err(e) = pi_link::mcp::remove(&old.file, &old.name) {
                    return self.mc_set_error(&e, cx);
                }
            }
        }
        match pi_link::mcp::add(&path, &name, config) {
            Ok(_) => {
                self.reload_settings_panel();
                let key = self
                    .mcp_servers
                    .iter()
                    .find(|s| s.name == name && s.scope == Scope::Global)
                    .map(section_key)
                    .unwrap_or("__mcp_add__".into());
                if let Some(st) = self.settings.clone() {
                    st.update(cx, |s, _| s.section = key);
                }
                self.set_status(tr("已保存，对新会话生效").to_string(), cx);
                cx.notify();
            }
            Err(e) => self.mc_set_error(&e, cx),
        }
    }
}

/// 把回填数据写进表单（`None` = 新增模式，清空）。
pub(crate) fn mcp_apply_backfill(
    s: &mut SettingsPanel,
    data: Option<(String, String, u8)>,
    cx: &mut gpui::Context<SettingsPanel>,
) {
    match data {
        Some((name, text, ix)) => {
            s.mcp_name.update(cx, |ti, cx| ti.set_value(name, cx));
            s.mcp_add.update(cx, |ti, cx| ti.set_value(text, cx));
            s.mcp_exposure = ix;
        }
        None => {
            s.mcp_name.update(cx, |ti, cx| ti.set_value(String::new(), cx));
            s.mcp_add.update(cx, |ti, cx| ti.set_value(String::new(), cx));
            s.mcp_exposure = 0;
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
    mcp_exposure: u8,
    error: &Option<String>,
) -> gpui::AnyElement {
    let sb = mcp_sidebar(chat, weak, section);
    let detail = mcp_form(
        chat, weak, section, mcp_add_input, mcp_add_value, mcp_name, mcp_name_value,
        mcp_exposure, error,
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

/// 侧栏：全局一组（总开关）+ 添加 MCP。043：项目配置不管理。
fn mcp_sidebar(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
) -> gpui::AnyElement {
    let t = T();
    // px7 + 行内 px8 = 内容左右 15px（与右侧详情 p15 等距，040 扩展页定稿）
    let mut list = sidebar_list().px(px(7.));
    let items: Vec<&ServerEntry> = chat
        .mcp_servers
        .iter()
        .filter(|s| s.scope == Scope::Global)
        .collect();
    let enabled = items.iter().filter(|s| s.enabled).count();
    let total = items.len();
    list = list.child(group_header(
        tr("全局"),
        Some(group_switch(
            "mcp-bulk-g",
            weak,
            format!("{enabled}/{total}"),
            total > 0 && enabled == total,
            total == 0,
            move |c, cx| c.mcp_bulk(enabled != total, cx),
        ),
    )));
    if items.is_empty() {
        list = list.child(
            div()
                .px(px(8.))
                .pb(px(4.))
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_dim))
                .child(tr("没有服务器")),
        );
    }
    for entry in items {
        let key = section_key(entry);
        let active = key == section;
        let weak_item = weak.clone();
        let row_key = key.clone();
        let weak_sw = weak.clone();
        let sw_entry = entry.clone();
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
                })
                // 单独开关（043：总开关 + 单独开关；点击不触发行选中）
                .child(config_switch(
                    SharedString::from(format!("mcp-row-sw-{}", entry.name)),
                    &weak_sw,
                    entry.enabled,
                    false,
                    move |c, cx| c.mcp_toggle(sw_entry.clone(), !sw_entry.enabled, cx),
                )),
        );
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

/// 右侧【配置MCP】：新增 / 编辑同一张表单（043 定稿五行）。
#[allow(clippy::too_many_arguments)]
fn mcp_form(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
    mcp_add_input: &gpui::Entity<TextInput>,
    mcp_add_value: &str,
    mcp_name: &gpui::Entity<TextInput>,
    mcp_name_value: &str,
    mcp_exposure: u8,
    error: &Option<String>,
) -> gpui::AnyElement {
    let t = T();
    let editing = chat
        .mcp_servers
        .iter()
        .find(|s| section_key(s) == section && s.scope == Scope::Global)
        .cloned();

    // 实时解析反馈（识别形态徽标 + 摘要；失败红字 + 保存置灰）
    let trimmed = mcp_add_value.trim();
    let parsed = if trimmed.is_empty() {
        None
    } else {
        Some(pi_link::mcp::parse_server_input(trimmed))
    };
    let parse_ok = matches!(parsed, Some(Ok(_)));
    let feedback: gpui::AnyElement = match &parsed {
        None => note("支持四种粘贴形态，见下方示例").into_any_element(),
        Some(Ok((suggested, config, source))) => {
            let cli_prefix = ["pi ", "claude ", "codex ", "gemini "]
                .iter()
                .any(|p| trimmed.to_lowercase().starts_with(p))
                && trimmed.to_lowercase().contains(" mcp add");
            let source_label = match source {
                pi_link::mcp::ParsedSource::Json => "JSON",
                pi_link::mcp::ParsedSource::Url => "URL",
                pi_link::mcp::ParsedSource::CommandLine if cli_prefix => "pi 命令",
                pi_link::mcp::ParsedSource::CommandLine => "命令行",
            };
            let transport = if config.get("url").is_some() { "HTTP" } else { "stdio" };
            let target = config
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
            let name = if mcp_name_value.is_empty() { suggested.as_str() } else { mcp_name_value };
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .px(px(6.))
                        .py(px(2.))
                        .rounded(px(4.))
                        .bg(rgb(t.bg_selected))
                        .text_size(crate::appearance::ui_size(10.))
                        .text_color(rgb(t.accent))
                        .child(SharedString::from(format!("✓ {}", source_label))),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .font_family(crate::markdown::MONO_FAMILY)
                        .text_size(crate::appearance::ui_size(10.))
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(format!(
                            "{name} · {transport} · {target}"
                        ))),
                )
                .into_any_element()
        }
        Some(Err(e)) => error_note(e).into_any_element(),
    };

    // 四种说明（043：纯文本，取消点击互动）
    const EXAMPLES: [(&str, &str); 4] = [
        ("例1 JSON", r#"{"mcpServers":{"docs":{"url":"https://example.com/mcp"}}}"#),
        ("例2 URL", "https://mcp.example.com/mcp"),
        ("例3 命令行", "npx -y @modelcontextprotocol/server-filesystem ."),
        ("例4 pi命令", "pi mcp add docs --url https://example.com/mcp"),
    ];

    // 详情内边距与列表 15px 统一（040 扩展页定稿；detail_shell 默认 p20）
    let mut form = detail_shell("mc-detail")
        .p(px(15.))
        // 第一行：title + [移除（编辑态）] + 保存（解析失败/空文本置灰）
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .mb(px(10.))
                .child(section_title(&tr("配置MCP")))
                .child(div().flex_1())
                .children(editing.clone().map(|e| {
                    config_button("mcp-remove", weak, &tr("移除"), Btn::Danger, true, false,
                        move |c, cx| c.mcp_remove(e.clone(), cx))
                }))
                .child(config_button("mcp-save", weak, &tr("保存"), Btn::Primary, false, !parse_ok, |c, cx| {
                    c.mcp_save_submit(cx)
                })),
        );
    if let Some(err) = error {
        form = form.child(error_note(err));
    }
    form = form
        // 第二行：名称
        .child(field(
            &tr("MCP名称"),
            div().w(px(280.)).child(mcp_name.clone()),
        ))
        // 第三行：8 行粘贴框
        .child(field(&tr("服务器配置"), mcp_add_input.clone()))
        // 实时解析反馈
        .child(feedback)
        // exposure 四选（保存期写入 config）
        .child(field(
            &tr("工具暴露"),
            div().flex().flex_wrap().gap(px(6.)).children(
                EXPOSURES.iter().enumerate().map(|(ix, (_, label))| {
                    let active = mcp_exposure as usize == ix;
                    let weak_opt = weak.clone();
                    div()
                        .id(SharedString::from(format!("mcp-exp-{ix}")))
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
                                if let Some(st) = c.settings.clone() {
                                    st.update(cx, |s, cx| {
                                        s.mcp_exposure = ix as u8;
                                        cx.notify();
                                    });
                                }
                            });
                        })
                        .child(SharedString::from(label.to_string()))
                }),
            ),
        ))
        // 第四行：四种说明（纯文本）
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.))
                .mt(px(4.))
                .children(EXAMPLES.iter().map(|(label, example)| {
                    div()
                        .flex()
                        .gap(px(6.))
                        .min_w_0()
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_size(crate::appearance::ui_size(10.))
                                .text_color(rgb(t.text_muted))
                                .child(SharedString::from(label.to_string())),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .font_family(crate::markdown::MONO_FAMILY)
                                .text_size(crate::appearance::ui_size(10.))
                                .text_color(rgb(t.text_dim))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(SharedString::from(example.to_string())),
                        )
                })),
        );
    form.into_any_element()
}
