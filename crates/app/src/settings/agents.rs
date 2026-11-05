//! Settings-子代理页（044-1 §2）：Agent 定义管理（实例层之外的「类」层）。
//!
//! 形态与 042 模型管理同构：左列表（内置/自定义两组）+ 右表单。表单化
//! 只覆盖 v1 已定字段集；未知字段解析时透传、写回时拼回（见
//! `pi_link::agents`）。
//!
//! 尺寸/配色逐值照抄 `docs/UI设计/设置-子代理页-原型.html`（已过稿）：列表
//! 宽 400（[`widgets::LIST_W`]）、行 30px 圆点 7px、label 96px 上沿对齐、
//! 输入 30px、系统提示 textarea 170px、下拉宽 250。

use gpui::prelude::FluentBuilder;

use super::*;
use pi_link::agents::{AgentOrigin, AgentProfile, MaxTurnsTier};

/// 新建态的 section 哨兵（同 MCP 页 `__mcp_add__`）。
const AG_NEW: &str = "__agents_new__";

/// 工具档位六选一（044-0：继承/chat-only/read-only/default/full/full+；含描述）。
fn tool_tiers() -> [(&'static str, &'static str); 6] {
    [
        (tr("agents.inherit_tools"), ""),
        ("chat-only", "无工具，纯对话"),
        ("read-only", "4 个只读内置工具"),
        ("default", "4 个内置工具"),
        ("full", "全部内置工具，无扩展"),
        ("full+", "全部工具 + 自选扩展/MCP"),
    ]
}

/// label 96px 上沿对齐的横排行（原型 `.row`）。`field()` 是竖排，不适用。
fn agent_row(label: &str, control: impl gpui::IntoElement, top_align: bool) -> gpui::AnyElement {
    let t = T();
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .flex_shrink_0()
        .child(
            div()
                .w(px(96.))
                .flex_shrink_0()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text))
                .when(top_align, |d| d.items_start().pt(px(8.)))
                .child(SharedString::from(label.to_string())),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(control),
        )
        .into_any_element()
}

/// 下拉触发按钮（原型 `.sel-btn`）：30px、bg_panel、右侧 chevron。
fn sel_trigger(value: &str, open: bool) -> gpui::AnyElement {
    let t = T();
    div()
        .w(px(250.))
        .h(px(30.))
        .flex_shrink_0()
        .px(px(10.))
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.bg_panel))
        .flex()
        .items_center()
        .text_size(crate::appearance::ui_size(12.))
        .text_color(rgb(t.text))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(SharedString::from(value.to_string())),
        )
        .child(icon(
            if open { "chevron-up" } else { "chevron-down" },
            12.,
            t.text_dim,
        ))
        .into_any_element()
}

impl Chat {
    /// 选中一个 Agent 并把字段装进表单（导航预填与行点击共用）。
    pub(crate) fn mc_agent_select(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(a) = self.cfg.mc_agents.iter().find(|a| a.id == id).cloned() else {
            return;
        };
        let name = a.id.clone();
        let desc = a.description.clone();
        let prompt = a.prompt_text();
        let model = a.model.clone();
        let tools = a.tools_tier().to_string();
        let turns = a.max_turns_tier();
        let turns_n = match turns {
            MaxTurnsTier::Custom(n) => n.to_string(),
            _ => String::new(),
        };
        let inject = a.inject_enabled();
        let (ag_extensions, ag_mcp, ag_skills) = (
            a.extensions.clone(),
            a.mcp_servers.clone(),
            a.skills.clone(),
        );
        if let Some(st) = self.dlg.settings.clone() {
            st.update(cx, |s, cx| {
                s.section = id;
                s.ag_name.update(cx, |ti, cx| ti.set_value(name, cx));
                s.ag_desc.update(cx, |ti, cx| ti.set_value(desc, cx));
                s.ag_prompt.update(cx, |ti, cx| ti.set_value(prompt, cx));
                s.ag_turns_n.update(cx, |ti, cx| ti.set_value(turns_n, cx));
                s.ag_model = model;
                s.ag_tools = tools;
                s.ag_turns = turns;
                s.ag_inject = inject;
                s.ag_extensions = ag_extensions;
                s.ag_mcp = ag_mcp;
                s.ag_skills = ag_skills;
                s.ag_popup = None;
                s.ag_dirty = false;
                cx.notify();
            });
        }
        cx.notify();
    }

    /// 进入新建态：清空表单 + section 哨兵。
    pub(crate) fn mc_agent_new(&mut self, cx: &mut Context<Self>) {
        if let Some(st) = self.dlg.settings.clone() {
            st.update(cx, |s, cx| {
                s.section = AG_NEW.into();
                s.ag_name
                    .update(cx, |ti, cx| ti.set_value(String::new(), cx));
                s.ag_desc
                    .update(cx, |ti, cx| ti.set_value(String::new(), cx));
                s.ag_prompt
                    .update(cx, |ti, cx| ti.set_value(String::new(), cx));
                s.ag_turns_n
                    .update(cx, |ti, cx| ti.set_value(String::new(), cx));
                s.ag_model = None;
                s.ag_tools = "inherit".into();
                s.ag_turns = MaxTurnsTier::Hundred;
                s.ag_inject = true;
                s.ag_extensions = Vec::new();
                s.ag_mcp = Vec::new();
                s.ag_skills = Vec::new();
                s.ag_popup = None;
                s.ag_dirty = false;
                cx.notify();
            });
        }
        cx.notify();
    }

    /// 读表单 → 组 AgentProfile → 写回 md（保存/新建共用）。
    pub(crate) fn mc_agent_save(&mut self, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        let Some(st) = self.dlg.settings.clone() else {
            return;
        };
        let (
            section,
            name,
            desc,
            prompt,
            turns_n,
            model,
            tools,
            turns,
            inject,
            ag_extensions,
            ag_mcp,
            ag_skills,
        ) = st.update(cx, |s, cx| {
            (
                s.section.clone(),
                s.ag_name.read(cx).value().trim().to_string(),
                s.ag_desc.read(cx).value().trim().to_string(),
                s.ag_prompt.read(cx).value().to_string(),
                s.ag_turns_n.read(cx).value().trim().to_string(),
                s.ag_model.clone(),
                s.ag_tools.clone(),
                s.ag_turns,
                s.ag_inject,
                s.ag_extensions.clone(),
                s.ag_mcp.clone(),
                s.ag_skills.clone(),
            )
        });
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            self.mc_set_error(&tr("agents.name_invalid"), cx);
            return;
        }
        // 保留原条目的未知字段（编辑时不丢 v1 之外的手写键）
        let prev_unknown = self
            .cfg
            .mc_agents
            .iter()
            .find(|a| a.id == section)
            .map(|a| a.unknown.clone())
            .unwrap_or_default();
        let max_turns = match turns {
            MaxTurnsTier::Fifty => Some(50),
            MaxTurnsTier::Hundred => Some(100),
            MaxTurnsTier::Unlimited => Some(0),
            MaxTurnsTier::Custom(_) => turns_n.parse::<u32>().ok(),
        };
        let prompt_trimmed = prompt.trim().to_string();
        let profile = AgentProfile {
            id: name.clone(),
            description: desc,
            model,
            tools: if tools == "inherit" {
                None
            } else {
                Some(tools)
            },
            max_turns,
            inject_agents_md: Some(inject),
            system_prompt: None,
            append_system_prompt: if prompt_trimmed.is_empty() {
                None
            } else {
                Some(prompt_trimmed)
            },
            unknown: prev_unknown,
            extensions: ag_extensions,
            mcp_servers: ag_mcp,
            skills: ag_skills,
            origin: AgentOrigin::Custom,
            path: None,
            error: None,
        };
        let dir = pi_link::agents::agents_dir();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.mc_set_error(&e.to_string(), cx);
            return;
        }
        let path = dir.join(format!("{name}.md"));
        // 改名：旧文件进回收站（id 变了 = 换了一个定义）
        if section != AG_NEW && section != name {
            let old = dir.join(format!("{section}.md"));
            if old.exists() {
                let _ = trash::delete(&old);
            }
        }
        if let Err(e) = std::fs::write(&path, pi_link::agents::serialize_agent(&profile)) {
            self.mc_set_error(&e.to_string(), cx);
            return;
        }
        self.reload_settings_panel();
        let _ = st.update(cx, |s, cx| {
            s.section = name;
            s.ag_dirty = false;
            cx.notify();
        });
        cx.notify();
    }

    /// 删除自定义条目（进回收站）。
    pub(crate) fn mc_agent_delete(&mut self, id: String, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        let Some(a) = self.cfg.mc_agents.iter().find(|a| a.id == id).cloned() else {
            return;
        };
        if !a.origin.is_custom() {
            return;
        }
        if let Some(path) = a.path {
            if let Err(e) = trash::delete(&path) {
                self.mc_set_error(&e.to_string(), cx);
                return;
            }
        }
        self.reload_settings_panel();
        if let Some(st) = self.dlg.settings.clone() {
            let next = self
                .cfg
                .mc_agents
                .first()
                .map(|a| a.id.clone())
                .unwrap_or_default();
            let _ = st.update(cx, |s, _| s.section = next);
        }
        // 选中下一条，装进表单
        let next = self
            .cfg
            .mc_agents
            .first()
            .map(|a| a.id.clone())
            .unwrap_or_default();
        if !next.is_empty() {
            self.mc_agent_select(next, cx);
        }
    }

    /// 内置「复制为自定义」：fork 进自定义目录（同名则覆盖自定义侧）。
    pub(crate) fn mc_agent_copy_builtin(&mut self, id: String, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        let Some(a) = self.cfg.mc_agents.iter().find(|a| a.id == id).cloned() else {
            return;
        };
        let dir = pi_link::agents::agents_dir();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.mc_set_error(&e.to_string(), cx);
            return;
        }
        let path = dir.join(format!("{id}.md"));
        let mut copy = a;
        copy.origin = AgentOrigin::Custom;
        if let Err(e) = std::fs::write(&path, pi_link::agents::serialize_agent(&copy)) {
            self.mc_set_error(&e.to_string(), cx);
            return;
        }
        self.reload_settings_panel();
        self.mc_agent_select(id, cx);
    }

    /// 损坏态「打开文件修复」：源码编辑器打开（表单化的唯一源码逃生口）。
    pub(crate) fn mc_agent_open_fix(&mut self, path: String, cx: &mut Context<Self>) {
        self.open_file_tab(PathBuf::from(path), cx);
    }
}

/// 子代理页：左列表（内置/自定义两组）+ 右表单。
pub(crate) fn mc_agents_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
    d: &SettingsFormData,
) -> (gpui::AnyElement, gpui::AnyElement) {
    let t = T();
    let builtins: Vec<&AgentProfile> = chat
        .cfg
        .mc_agents
        .iter()
        .filter(|a| !a.origin.is_custom())
        .collect();
    let customs: Vec<&AgentProfile> = chat
        .cfg
        .mc_agents
        .iter()
        .filter(|a| a.origin.is_custom())
        .collect();

    // ---- 左列表 ----
    let mut list = sidebar_list().px(px(7.));
    // 模块设置（044-0）：并发/深度，写 app_settings，spawn 时 env 传导
    list = list.child(module_settings_block(weak));
    list = list.child(
        div()
            .h(px(1.))
            .mx(px(4.))
            .my(px(6.))
            .bg(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x66))),
    );
    if !builtins.is_empty() {
        list = list.child(group_header(
            &format!("{} {}", tr("agents.builtin_group"), builtins.len()),
            None,
        ));
        for a in &builtins {
            list = list.child(agent_item(weak, a, section == a.id));
        }
    }
    // 自定义组头：计数 + 右靠「新建」按钮
    list = list.child(group_header(
        &format!("{} {}", tr("agents.custom_group"), customs.len()),
        Some(config_button(
            "ag-new",
            weak,
            &tr("agents.new"),
            Btn::Secondary,
            true,
            false,
            |c, cx| c.mc_agent_new(cx),
        )),
    ));
    for a in &customs {
        list = list.child(agent_item(weak, a, section == a.id));
    }
    if customs.is_empty() {
        list = list.child(
            div()
                .p(px(12.))
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_dim))
                .child(tr("agents.empty")),
        );
    }

    let sidebar = sidebar_shell("mc-sidebar-agents")
        .w(px(LIST_W))
        .bg(rgb(t.bg))
        .child(list)
        .into_any_element();

    // ---- 右表单 ----
    let detail = agent_detail(chat, weak, section, d);
    (sidebar, detail)
}

/// 列表行：7px 圆点 + 名称（+ 覆盖内置徽标）。
fn agent_item(weak: &gpui::WeakEntity<Chat>, a: &AgentProfile, active: bool) -> gpui::AnyElement {
    let t = T();
    let id = a.id.clone();
    let builtins: Vec<AgentProfile> = pi_link::agents::builtin_agents();
    let overrides = a.overrides_builtin(&builtins);
    let broken = a.error.is_some();
    widgets::sidebar_item(format!("ag-{}", a.id), active)
        .bg(rgb(t.bg))
        .on_mouse_down(MouseButton::Left, {
            let weak = weak.clone();
            let sel = id.clone();
            move |_, _, cx| {
                let _ = weak.update(cx, |c, cx| c.mc_agent_select(sel.clone(), cx));
            }
        })
        .child(status_dot(if broken { t.danger } else { t.accent }))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .when(broken, |d| d.text_color(rgb(t.danger)))
                .child(SharedString::from(a.id.clone())),
        )
        .when(overrides, |d| {
            d.child(
                div()
                    .px(px(5.))
                    .py(px(1.))
                    .rounded(px(3.))
                    .bg(gpui::hsla(0., 0., 0.5, 0.12))
                    .text_size(crate::appearance::ui_size(10.))
                    .text_color(rgb(t.text_dim))
                    .child(tr("agents.overrides_builtin")),
            )
        })
        .into_any_element()
}

/// 右栏表单：新建 / 损坏 / 内置只读 / 自定义可编辑 四态。
fn agent_detail(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
    d: &SettingsFormData,
) -> gpui::AnyElement {
    let t = T();
    let SettingsFormData {
        ag_name,
        ag_desc,
        ag_prompt,
        ag_turns_n,
        ag_model,
        ag_tools,
        ag_turns,
        ag_inject,
        ag_extensions,
        ag_mcp,
        ag_skills,
        ag_popup,
        ag_dd,
        ag_dirty,
        ag_name_value,
        ag_desc_value,
        ag_prompt_value,
        ..
    } = d;

    // 新建态：空表单（可编辑）
    let (agent, is_new) = if section == AG_NEW {
        (None, true)
    } else {
        (
            chat.cfg.mc_agents.iter().find(|a| a.id == section).cloned(),
            false,
        )
    };
    let builtin = agent
        .as_ref()
        .map(|a| !a.origin.is_custom())
        .unwrap_or(false);
    let readonly = builtin; // 内置只读灰化
    let broken = agent.as_ref().and_then(|a| a.error.clone());

    let mut head = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .flex_shrink_0()
        .child(
            div()
                .w(px(96.))
                .flex_shrink_0()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text))
                .child(tr("agents.name")),
        )
        .child({
            // 只读态渲染静态文本（TextInput 无禁用 API，原型 .frm.ro 整表单禁交互）
            if readonly {
                div()
                    .w(px(250.))
                    .h(px(30.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .opacity(0.62)
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_size(crate::appearance::ui_size(12.))
                    .text_color(rgb(t.text))
                    .child(SharedString::from(ag_name_value.clone()))
            } else {
                div().w(px(250.)).flex_shrink_0().child(ag_name.clone())
            }
        })
        .child(div().flex_1())
        .when(!is_new, |d| {
            // 删除（仅自定义）/ 复制为自定义（仅内置）
            if builtin {
                d.child(config_button(
                    "ag-copy",
                    weak,
                    &tr("agents.copy_custom"),
                    Btn::Secondary,
                    true,
                    false,
                    {
                        let id = agent.as_ref().map(|a| a.id.clone()).unwrap_or_default();
                        move |c, cx| c.mc_agent_copy_builtin(id.clone(), cx)
                    },
                ))
            } else {
                d.child(config_button(
                    "ag-del",
                    weak,
                    &tr("agents.delete"),
                    Btn::Danger,
                    true,
                    false,
                    {
                        let id = agent.as_ref().map(|a| a.id.clone()).unwrap_or_default();
                        move |c, cx| c.mc_agent_delete(id.clone(), cx)
                    },
                ))
            }
        })
        .when(is_new || !builtin, |d| {
            d.child(config_button(
                "ag-save",
                weak,
                &tr("agents.save"),
                Btn::Primary,
                true,
                false,
                |c, cx| c.mc_agent_save(cx),
            ))
        });

    let dirty_dot = div()
        .size(px(6.))
        .rounded_full()
        .flex_shrink_0()
        .bg(rgb(t.accent));
    if *ag_dirty && !readonly {
        head = head.child(dirty_dot);
    }

    let mut form = div().flex().flex_col().gap(px(12.)).mt(px(12.)).flex_1();

    // 损坏态：红条 + 打开文件修复 + 删除
    if let Some(err) = broken {
        form = form
            .child(error_note(&format!("{}：{err}", tr("agents.broken"))))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(config_button(
                        "ag-fix",
                        weak,
                        &tr("agents.open_fix"),
                        Btn::Secondary,
                        true,
                        false,
                        {
                            let path = agent
                                .as_ref()
                                .and_then(|a| a.path.clone())
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_default();
                            move |c, cx| c.mc_agent_open_fix(path.clone(), cx)
                        },
                    ))
                    .child(config_button(
                        "ag-del-broken",
                        weak,
                        &tr("agents.delete"),
                        Btn::Danger,
                        true,
                        false,
                        {
                            let id = agent.as_ref().map(|a| a.id.clone()).unwrap_or_default();
                            move |c, cx| c.mc_agent_delete(id.clone(), cx)
                        },
                    )),
            );
        return detail_shell_with(head, form);
    }

    // 描述
    form = form.child(agent_row(
        &tr("agents.desc"),
        if readonly {
            div()
                .w_full()
                .opacity(0.62)
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(ag_desc_value.clone()))
        } else {
            div().w_full().child(ag_desc.clone())
        },
        true,
    ));

    // 模型下拉（首项「继承」+ 分隔线 + 模型列表）
    let model_opts = chat
        .mc_sorted_enabled()
        .iter()
        .map(|m| {
            let key = format!("{}/{}", m.provider, m.id);
            let label = if m.name.is_empty() {
                key.clone()
            } else {
                m.name.clone()
            };
            (key, label)
        })
        .collect::<Vec<_>>();
    form = form.child(agent_row(
        &tr("agents.model"),
        model_dropdown(
            weak,
            &model_opts,
            ag_model,
            *ag_popup,
            ag_dd.clone(),
            readonly,
        ),
        false,
    ));

    // 工具权限下拉（044-0 六档）
    form = form.child(agent_row(
        &tr("agents.tools"),
        tools_dropdown(weak, ag_tools, *ag_popup, ag_dd.clone(), readonly),
        false,
    ));

    // full+ 档：自选 pi-ext / MCP 服务器（044-0 工具定制）；skills 定制恒显。
    // 只读态（内置）不渲染清单（内置不配扩展）。
    if !readonly {
        if ag_tools.as_str() == "full+" {
            form = form
                .child(agent_row(
                    &tr("agents.ext_pick"),
                    pick_list(
                        weak,
                        "ext",
                        pi_link::extensions::personal_extensions(),
                        ag_extensions,
                    ),
                    false,
                ))
                .child(agent_row(
                    &tr("agents.mcp_pick"),
                    {
                        let cwd = chat.ws.cwd.clone().unwrap_or_default();
                        let (servers, _) = pi_link::mcp::load(Some(std::path::Path::new(&cwd)));
                        let opts = servers
                            .iter()
                            .filter(|s| s.enabled)
                            .map(|s| s.name.clone())
                            .collect::<Vec<_>>();
                        pick_list(weak, "mcp", opts, ag_mcp)
                    },
                    false,
                ));
        }
        form = form.child(agent_row(
            &tr("agents.skills_pick"),
            {
                let cwd = chat.ws.cwd.clone().unwrap_or_default();
                let agent_dir = pi_link::config::agent_dir();
                let home_agents = agent_dir
                    .parent()
                    .map(|p| p.join("..").join(".agents").join("skills"))
                    .map(|p| dunce::canonicalize(&p).unwrap_or(p))
                    .unwrap_or_else(|| agent_dir.clone());
                let settings_value = pi_link::config::read_json(&pi_link::config::settings_path())
                    .unwrap_or(serde_json::json!({}));
                let entries = pi_link::skills::discover_skills(
                    std::path::Path::new(&cwd),
                    &agent_dir,
                    &home_agents,
                    &settings_value,
                );
                let opts = entries.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
                pick_list(weak, "skill", opts, ag_skills)
            },
            false,
        ));
    }

    // 系统提示 textarea（170px）；只读态渲染静态文本块
    form = form.child(agent_row(
        &tr("agents.prompt"),
        if readonly {
            div()
                .w_full()
                .h(px(170.))
                .rounded(px(6.))
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.bg_panel))
                .p(px(8.))
                .px(px(10.))
                .opacity(0.62)
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text))
                .child(SharedString::from(ag_prompt_value.clone()))
        } else {
            div()
                .w_full()
                .h(px(170.))
                .rounded(px(6.))
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.bg_panel))
                .child(ag_prompt.clone())
        },
        true,
    ));

    // 最大轮次 seg（50/100/不限/指定）+ 注入AGENTS.md 开关
    let seg = turns_seg(weak, ag_turns, ag_turns_n, readonly);
    let inject_label = div()
        .text_size(crate::appearance::ui_size(12.))
        .text_color(rgb(t.text))
        .child(tr("agents.inject_agents_md"));
    let inject_sw = config_switch("ag-inject", weak, *ag_inject, readonly, |c, cx| {
        if let Some(st) = c.dlg.settings.clone() {
            let _ = st.update(cx, |s, cx| {
                s.ag_inject = !s.ag_inject;
                s.ag_dirty = true;
                cx.notify();
            });
        }
    });
    form = form.child(agent_row(
        &tr("agents.max_turns"),
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .w_full()
            .child(seg)
            .child(div().flex_1())
            .child(inject_label)
            .child(inject_sw),
        false,
    ));

    if readonly {
        form = form.child(note(&tr("agents.readonly_builtin")));
    }

    detail_shell_with(head, form)
}

/// detail 容器（head + form）。
fn detail_shell_with(head: gpui::Div, form: gpui::Div) -> gpui::AnyElement {
    div()
        .id("mc-agents-detail")
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_y_scroll()
        .p(px(15.))
        .text_size(crate::appearance::ui_size(12.))
        .flex()
        .flex_col()
        .child(head)
        .child(form)
        .into_any_element()
}

/// 最大轮次四档 seg（原型 `.seg`）+ 指定时的数字输入。
fn turns_seg(
    weak: &gpui::WeakEntity<Chat>,
    turns: &MaxTurnsTier,
    turns_n: &gpui::Entity<TextInput>,
    readonly: bool,
) -> gpui::AnyElement {
    let t = T();
    let tiers: [(MaxTurnsTier, &'static str); 4] = [
        (MaxTurnsTier::Fifty, "50"),
        (MaxTurnsTier::Hundred, "100"),
        (MaxTurnsTier::Unlimited, tr("agents.unlimited")),
        (MaxTurnsTier::Custom(0), tr("agents.specify")),
    ];
    let is_custom = matches!(turns, MaxTurnsTier::Custom(_));
    let mut seg = div()
        .flex()
        .flex_shrink_0()
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.bg_panel))
        .overflow_hidden();
    for (tier, label) in tiers {
        let on = match (tier, turns) {
            (MaxTurnsTier::Custom(_), MaxTurnsTier::Custom(_)) => true,
            (a, b) => {
                std::mem::discriminant(&a) == std::mem::discriminant(b)
                    && !matches!(a, MaxTurnsTier::Custom(_))
            }
        };
        let weak_item = weak.clone();
        seg = seg.child(
            div()
                .h(px(28.))
                .px(px(14.))
                .flex()
                .items_center()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(if on { t.text } else { t.text_muted }))
                .bg(rgb(if on { t.bg_selected } else { t.bg_panel }))
                .when(on, |d| d.font_weight(gpui::FontWeight::SEMIBOLD))
                .when(!readonly, |d| {
                    d.cursor_pointer()
                        .hover(|s| s.bg(rgb(t.bg_hover)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak_item.update(cx, |c, cx| {
                                if let Some(st) = c.dlg.settings.clone() {
                                    st.update(cx, |s, cx| {
                                        s.ag_turns = tier;
                                        if matches!(tier, MaxTurnsTier::Custom(_)) {
                                            if s.ag_turns_n.read(cx).value().trim().is_empty() {
                                                s.ag_turns_n.update(cx, |ti, cx| {
                                                    ti.set_value("25".into(), cx)
                                                });
                                            }
                                        }
                                        s.ag_dirty = true;
                                        cx.notify();
                                    });
                                }
                            });
                        })
                })
                .child(SharedString::from(label.to_string())),
        );
    }
    let mut out = div().flex().items_center().gap(px(8.)).child(seg);
    if is_custom && !readonly {
        out = out.child(div().w(px(70.)).flex_shrink_0().child(turns_n.clone()));
    }
    out.into_any_element()
}

/// 模型下拉：首项「继承」+ 分隔线 + 启用模型列表（`provider/id`）。
#[allow(clippy::too_many_arguments)]
fn model_dropdown(
    weak: &gpui::WeakEntity<Chat>,
    models: &[(String, String)],
    model: &Option<String>,
    popup: Option<u8>,
    dd: gpui::Entity<DropdownState>,
    readonly: bool,
) -> gpui::AnyElement {
    let t = T();
    let open = popup == Some(0);
    let label = match model {
        Some(m) => m.clone(),
        None => tr("agents.inherit").to_string(),
    };
    let models = models.to_vec();
    let weak_toggle = weak.clone();
    let weak_dismiss = weak.clone();
    let weak_out = weak.clone();
    let trigger = sel_trigger(&label, open);
    let ro_toggle = readonly;
    crate::ui::dropdown(
        "ag-model-dd",
        &dd,
        open && !readonly,
        move |_, cx| {
            if ro_toggle {
                return;
            }
            let _ = weak_toggle.update(cx, |c, cx| {
                if let Some(st) = c.dlg.settings.clone() {
                    st.update(cx, |s, cx| {
                        s.ag_popup = if s.ag_popup == Some(0) { None } else { Some(0) };
                        cx.notify();
                    });
                }
            });
        },
        move |_, cx| {
            let _ = weak_dismiss.update(cx, |c, cx| {
                if let Some(st) = c.dlg.settings.clone() {
                    st.update(cx, |s, cx| {
                        if s.ag_popup.take().is_some() {
                            cx.notify();
                        }
                    });
                }
            });
        },
        trigger,
        move || {
            let mut menu = div()
                .w(px(250.))
                .py(px(3.))
                .bg(rgb(t.bg))
                .border_1()
                .border_color(rgb(t.border))
                .rounded(px(8.))
                .shadow_lg()
                .overflow_hidden();
            // 「继承」项
            menu = menu.child(dd_item(
                weak_out.clone(),
                tr("agents.inherit").to_string(),
                model.is_none(),
                Some(String::new()),
                0,
            ));
            // 分隔线
            menu = menu.child(div().h(px(1.)).my(px(4.)).bg(rgb(t.border)));
            for (value, name) in &models {
                let selected = model.as_deref() == Some(value.as_str());
                menu = menu.child(dd_item(
                    weak_out.clone(),
                    name.clone(),
                    selected,
                    Some(value.clone()),
                    0,
                ));
            }
            menu.into_any_element()
        },
    )
}

/// 工具权限下拉（四档，带描述）。
/// 多选清单（044-0 工具定制/skills 定制）：checkbox + 名称行，选中态读
/// `selected`，点击经 `kind` 分派到 Settings 态的对应清单翻转。空清单显
/// 暂无提示。`kind`: "ext"（pi-ext 路径）/ "mcp"（服务器名）/ "skill"。
fn pick_list(
    weak: &gpui::WeakEntity<Chat>,
    kind: &'static str,
    options: Vec<String>,
    selected: &[String],
) -> gpui::AnyElement {
    let t = T();
    let mut col = div()
        .id(SharedString::from(format!("ag-picks-{kind}")))
        .w_full()
        .max_h(px(150.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(px(2.));
    if options.is_empty() {
        col = col.child(
            div()
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_dim))
                .child(tr("agents.pick_empty")),
        );
    }
    for opt in options {
        let checked = selected.iter().any(|s| s == &opt);
        let weak_row = weak.clone();
        let opt_label = opt.clone();
        let opt_for_id = opt.clone();
        col = col.child(
            div()
                .id(SharedString::from(format!(
                    "ag-pick-{kind}-{}",
                    sanitize_id(&opt_for_id)
                )))
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(4.))
                .py(px(3.))
                .rounded(px(crate::ui::tokens::radius::INLINE))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                    let _ = weak_row.update(cx, |c, cx| {
                        if let Some(st) = c.dlg.settings.clone() {
                            let _ = st.update(cx, |s, cx| {
                                let slot = match kind {
                                    "ext" => &mut s.ag_extensions,
                                    "mcp" => &mut s.ag_mcp,
                                    _ => &mut s.ag_skills,
                                };
                                match slot.iter().position(|x| x == &opt) {
                                    Some(ix) => {
                                        slot.remove(ix);
                                    }
                                    None => slot.push(opt.clone()),
                                }
                                s.ag_dirty = true;
                                cx.notify();
                            });
                        }
                        cx.notify();
                    });
                })
                .child(crate::ui::kit::checkbox(checked))
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_size(crate::appearance::ui_size(11.5))
                        .text_color(rgb(if checked { t.text } else { t.text_dim }))
                        .child(SharedString::from(opt_label)),
                ),
        );
    }
    col.into_any_element()
}

/// 元素 id 消毒：扩展路径/服务器名可能带 `/\:.#` 等字符，gpui 元素 id
/// 只留字母数字与 `_`。
fn sanitize_id(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// 模块设置块（044-0）：子代理模块的三项目之一对——最大并发（2-5）与
/// 嵌套深度（1-3）。写 app_settings（spawn 时 env 传导给 pf-subagents），
/// 立即落盘。第三项「模块开关」= 开发旗标（044-0 拍板默认开），不在
/// 此处提供 UI（PI_FLASH_SUBAGENTS / dev-flags.json 控制）。
/// 模块设置块（044-0）：子代理模块的并发/深度两项——最大并发（2-5，默认 3，
/// glm 侧限 3）与嵌套深度（1-3，默认 1 禁止套娃）。写 app_settings 立即落盘，
/// spawn 时 env（PI_SUBAGENTS_CAPACITY / PI_SUBAGENTS_MAX_DEPTH）传导给
/// pf-subagents。第三项「模块开关」= 开发旗标（044-0 拍板默认开），UI 不
/// 暴露（PI_FLASH_SUBAGENTS / dev-flags.json 控制）。
fn module_settings_block(weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    let t = T();
    let cap = crate::services::workspace::subagent_capacity() as u32;
    let depth = crate::services::workspace::subagent_max_depth() as u32;
    let stepper = |weak2: &gpui::WeakEntity<Chat>,
                   id: &'static str,
                   label: &str,
                   v: u32,
                   min: u32,
                   max: u32,
                   set: fn(u64)| {
        let mk = |bid: String,
                  txt: &'static str,
                  to: u32,
                  disabled: bool,
                  weak3: &gpui::WeakEntity<Chat>| {
            config_button(
                bid,
                weak3,
                txt,
                Btn::Secondary,
                true,
                disabled,
                move |_, cx| {
                    set(to as u64);
                    cx.notify();
                },
            )
        };
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(4.))
            .py(px(3.))
            .child(
                div()
                    .w(px(64.))
                    .flex_shrink_0()
                    .text_size(crate::appearance::ui_size(11.))
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(label.to_string())),
            )
            .child(mk(
                format!("{id}-dec"),
                "\u{2212}",
                v.saturating_sub(1).max(min),
                v <= min,
                weak2,
            ))
            .child(
                div()
                    .w(px(20.))
                    .text_center()
                    .text_size(crate::appearance::ui_size(12.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text))
                    .child(SharedString::from(v.to_string())),
            )
            .child(mk(
                format!("{id}-inc"),
                "+",
                (v + 1).min(max),
                v >= max,
                weak2,
            ))
    };
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(
            div()
                .px(px(4.))
                .pt(px(4.))
                .pb(px(2.))
                .text_size(crate::appearance::ui_size(10.))
                .text_color(rgb(t.text_faint))
                .child(tr("agents.module_settings")),
        )
        .child(stepper(
            weak,
            "ag-cap",
            &tr("agents.capacity"),
            cap,
            2,
            5,
            crate::services::workspace::set_subagent_capacity,
        ))
        .child(stepper(
            weak,
            "ag-depth",
            &tr("agents.depth"),
            depth,
            1,
            3,
            crate::services::workspace::set_subagent_max_depth,
        ))
        .into_any_element()
}

fn tools_dropdown(
    weak: &gpui::WeakEntity<Chat>,
    tools: &str,
    popup: Option<u8>,
    dd: gpui::Entity<DropdownState>,
    readonly: bool,
) -> gpui::AnyElement {
    let t = T();
    let open = popup == Some(1);
    let label = tool_tiers()
        .iter()
        .find(|(v, _)| *v == tools)
        .map(|(v, _)| (*v).to_string())
        .unwrap_or_else(|| tools.to_string());
    let weak_toggle = weak.clone();
    let weak_dismiss = weak.clone();
    let weak_out = weak.clone();
    let trigger = sel_trigger(&label, open);
    let ro_toggle = readonly;
    crate::ui::dropdown(
        "ag-tools-dd",
        &dd,
        open && !readonly,
        move |_, cx| {
            if ro_toggle {
                return;
            }
            let _ = weak_toggle.update(cx, |c, cx| {
                if let Some(st) = c.dlg.settings.clone() {
                    st.update(cx, |s, cx| {
                        s.ag_popup = if s.ag_popup == Some(1) { None } else { Some(1) };
                        cx.notify();
                    });
                }
            });
        },
        move |_, cx| {
            let _ = weak_dismiss.update(cx, |c, cx| {
                if let Some(st) = c.dlg.settings.clone() {
                    st.update(cx, |s, cx| {
                        if s.ag_popup.take().is_some() {
                            cx.notify();
                        }
                    });
                }
            });
        },
        trigger,
        move || {
            let mut menu = div()
                .w(px(250.))
                .py(px(3.))
                .bg(rgb(t.bg))
                .border_1()
                .border_color(rgb(t.border))
                .rounded(px(8.))
                .shadow_lg()
                .overflow_hidden();
            for (value, desc) in tool_tiers() {
                let selected = value == tools;
                let mut item = dd_item_with_desc(
                    weak_out.clone(),
                    value.to_string(),
                    desc.to_string(),
                    selected,
                    Some(value.to_string()),
                    1,
                );
                let _ = &mut item;
                menu = menu.child(item);
            }
            menu.into_any_element()
        },
    )
}

/// 下拉项（原型 `.dd-it`）：左 ✓ 占位 + 名称。
fn dd_item(
    weak: gpui::WeakEntity<Chat>,
    label: String,
    selected: bool,
    value: Option<String>,
    which: u8,
) -> gpui::AnyElement {
    dd_item_inner(weak, label, String::new(), selected, value, which)
}

/// 下拉项带右侧描述（工具档位用）。
fn dd_item_with_desc(
    weak: gpui::WeakEntity<Chat>,
    label: String,
    desc: String,
    selected: bool,
    value: Option<String>,
    which: u8,
) -> gpui::AnyElement {
    dd_item_inner(weak, label, desc, selected, value, which)
}

fn dd_item_inner(
    weak: gpui::WeakEntity<Chat>,
    label: String,
    desc: String,
    selected: bool,
    value: Option<String>,
    which: u8,
) -> gpui::AnyElement {
    let t = T();
    div()
        .id(SharedString::from(format!("ag-ddit-{which}-{label}")))
        .h(px(30.))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(crate::appearance::ui_size(12.))
        .text_color(rgb(if selected { t.accent } else { t.text }))
        .bg(rgb(if selected { t.bg_selected } else { t.bg }))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| {
                if let Some(st) = c.dlg.settings.clone() {
                    let val = value.clone().unwrap_or_default();
                    st.update(cx, |s, cx| {
                        match which {
                            0 => s.ag_model = if val.is_empty() { None } else { Some(val) },
                            1 => s.ag_tools = val,
                            _ => {}
                        }
                        s.ag_popup = None;
                        s.ag_dirty = true;
                        cx.notify();
                    });
                }
            });
        })
        .child(div().w(px(12.)).flex_shrink_0().when(selected, |d| {
            d.child(
                div()
                    .text_size(crate::appearance::ui_size(12.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(rgb(t.accent))
                    .child("✓"),
            )
        }))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(SharedString::from(label)),
        )
        .when(!desc.is_empty(), |d| {
            d.child(
                div()
                    .flex_shrink_0()
                    .text_size(crate::appearance::ui_size(11.))
                    .text_color(rgb(t.text_faint))
                    .child(SharedString::from(desc)),
            )
        })
        .into_any_element()
}
