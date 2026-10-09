//! MCP tab（043 定稿）：只做全局配置（`~/.pi/agent/mcp.json`）。左侧注册
//! 列表（总开关 + 单独开关，点行进编辑、再点回新增），右侧【配置MCP】表单
//! ——新增与编辑同一张：名称 + 8 行粘贴框（JSON / URL / 命令行 / pi mcp add
//! 四形态自动识别）+ 四种纯文本示例。保存 = 解析归一写盘；改名 = 删旧增新。
//! 一切改动只对新会话生效。测试连接 / OAuth 登录为二期。

use pi_link::mcp::{Scope, ServerEntry};

use super::*;

/// 侧栏条目键：`g:name`（043 起只列全局；键格式保留 scope 前缀兼容旧 state）。
pub(crate) fn section_key(s: &ServerEntry) -> String {
    format!("{}:{}", if s.scope == Scope::Project { "p" } else { "g" }, s.name)
}

impl Chat {
    /// 编辑回填数据：选中行 → (名称, 原始 JSON)。原始 JSON 保 env 值与
    /// 未知字段（ServerEntry 重建会丢）。
    pub(crate) fn mcp_backfill_data(&self, key: &str) -> Option<(String, String)> {
        let e = self.mcp_servers.iter().find(|s| section_key(s) == key)?;
        let raw = pi_link::mcp::raw_entry(&e.file, &e.name).ok().flatten()?;
        let text = serde_json::to_string_pretty(&raw).ok()?;
        Some((e.name.clone(), text))
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

    /// 【保存】提交：解析 → 新增 / 编辑（改名删旧增新）→ 只写全局
    /// mcp.json（粘贴 JSON 里的 exposure 等字段原样保留）。043：一切改动
    /// 只对新会话生效。
    pub(crate) fn mcp_save_submit(&mut self, cx: &mut Context<Self>) {
        let Some(st) = self.settings.clone() else { return };
        let (text, name_override, section) = {
            let s = st.read(cx);
            (
                s.mcp_add.read(cx).value().trim().to_string(),
                s.mcp_name.read(cx).value().trim().to_string(),
                s.section.clone(),
            )
        };
        self.mc_clear_error(cx);
        if text.is_empty() {
            return self.mc_set_error("粘贴 JSON、URL 或命令行", cx);
        }
        let (name, config) = match pi_link::mcp::parse_server_input(&text) {
            Ok((suggested, config, _)) => {
                (if name_override.is_empty() { suggested } else { name_override }, config)
            }
            Err(e) => return self.mc_set_error(&e, cx),
        };
        if !pi_link::mcp::valid_name(&name) {
            return self.mc_set_error(tr("服务器名称只能使用字母、数字、_ 和 -"), cx);
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
    data: Option<(String, String)>,
    cx: &mut gpui::Context<SettingsPanel>,
) {
    match data {
        Some((name, text)) => {
            s.mcp_name.update(cx, |ti, cx| ti.set_value(name, cx));
            s.mcp_add.update(cx, |ti, cx| ti.set_value(text, cx));
        }
        None => {
            s.mcp_name.update(cx, |ti, cx| ti.set_value(String::new(), cx));
            s.mcp_add.update(cx, |ti, cx| ti.set_value(String::new(), cx));
        }
    }
}

/// MCP tab：分栏（043：无底栏，对齐扩展页）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn mc_mcp_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
    mcp_add_input: &gpui::Entity<TextInput>,
    mcp_add_value: &str,
    mcp_name: &gpui::Entity<TextInput>,
    error: &Option<String>,
) -> gpui::AnyElement {
    let sb = mcp_sidebar(chat, weak, section);
    let detail = mcp_form(
        chat, weak, section, mcp_add_input, mcp_add_value, mcp_name, error,
    );
    div()
        .flex()
        .w_full()
        .h_full()
        .min_h_0()
        .child(sb)
        .child(detail)
        .into_any_element()
}

/// 侧栏：全局一组（总开关 + 单独开关）。043：项目配置不管理，无底部按钮
/// ——点已选中的行回到新增表单。
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
    let empty = items.is_empty();
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
                    // 点已选中的行 → 回新增表单（043：无「添加 MCP」按钮）
                    let next = if active { "__mcp_add__".to_string() } else { row_key.clone() };
                    let _ = weak_item.update(cx, |c, cx| c.mcp_select(next, cx));
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
    let mut shell =
        sidebar_shell("mc-sidebar").w(px(LIST_W)).bg(rgb(t.bg)).child(list);
    // 空态：placeholder 色，水平垂直居中
    if empty {
        shell = shell.child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_size(crate::appearance::ui_size(11.))
                        .text_color(rgb(t.text_faint))
                        .child(tr("没有服务器")),
                ),
        );
    }
    shell.into_any_element()
}

/// 右侧【配置MCP】：新增 / 编辑同一张表单（043 定稿四行）。
#[allow(clippy::too_many_arguments)]
fn mcp_form(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
    mcp_add_input: &gpui::Entity<TextInput>,
    mcp_add_value: &str,
    mcp_name: &gpui::Entity<TextInput>,
    error: &Option<String>,
) -> gpui::AnyElement {
    let t = T();
    let editing = chat
        .mcp_servers
        .iter()
        .find(|s| section_key(s) == section && s.scope == Scope::Global)
        .cloned();

    // 四种说明（043：纯文本，取消点击互动；单行单串与输入区左缘对齐）
    const EXAMPLES: [&str; 4] = [
        r#"JSON：{"mcpServers":{"docs":{"url":"https://example.com/mcp"}}}"#,
        "URL：https://mcp.example.com/mcp",
        "命令行：npx -y @modelcontextprotocol/server-filesystem .",
        "pi命令：pi mcp add docs --url https://example.com/mcp",
    ];

    // 详情内边距与列表 15px 统一（040 扩展页定稿；detail_shell 默认 p20）
    let mut form = detail_shell("mc-detail")
        .p(px(15.))
        // 第一行：MCP名称 标签 + [移除（编辑态）] + 保存（空文本置灰）
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(section_title(&tr("MCP名称")))
                .child(div().flex_1())
                .children(editing.clone().map(|e| {
                    config_button("mcp-remove", weak, &tr("移除"), Btn::Danger, true, false,
                        move |c, cx| c.mcp_remove(e.clone(), cx))
                }))
                .child(config_button("mcp-save", weak, &tr("保存"), Btn::Primary, false, mcp_add_value.trim().is_empty(), |c, cx| {
                    c.mcp_save_submit(cx)
                })),
        )
        // 第二行：名称输入（全宽）
        .child(
            div()
                .mb(px(10.))
                .child(mcp_name.clone()),
        )
        // 第三行：服务配置标签
        .child(section_title(&tr("MCP配置")));
    if let Some(err) = error {
        form = form.child(error_note(err));
    }
    if !chat.mcp_errors.is_empty() {
        form = form.child(error_note(&chat.mcp_errors.join("；")));
    }
    form = form
        // 第四行：8 行粘贴框
        .child(mcp_add_input.clone())
        // 第五行：四种说明（纯文本）
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.))
                .mt(px(10.))
                .children(EXAMPLES.iter().map(|line| {
                    div()
                        .w_full()
                        .font_family(crate::markdown::MONO_FAMILY)
                        .text_size(crate::appearance::ui_size(10.))
                        .text_color(rgb(t.text_dim))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(SharedString::from(line.to_string()))
                })),
        );
    form.into_any_element()
}
