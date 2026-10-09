//! Subagents tab (pi-web AgentsConfig parity)：顶部全局特性条（内置开关 +
//! 并发子代理数），左栏运行列表 + 按 scope 分组的档案 + 新建子代理，右栏
//! 档案表单（工具/资源勾选即时写盘，文本字段走「保存」）。

use pi_link::subagents::SubagentScope;

use super::*;
use crate::SubagentRun;

/// bool 字段编号（sa_toggle_bool 用）：0 load_skills · 1 load_extensions ·
/// 2 inherit_context · 3 run_in_background。
const BOOL_LOAD_SKILLS: u8 = 0;
const BOOL_LOAD_EXTENSIONS: u8 = 1;
const BOOL_INHERIT: u8 = 2;
const BOOL_BACKGROUND: u8 = 3;

impl Chat {
    // -----------------------------------------------------------------------
    // subagents (pi-web subagents.ts / AgentSessionPanel parity)
    // -----------------------------------------------------------------------

    pub(crate) fn sa_selected(&self, section: &str) -> Option<&pi_link::subagents::SubagentProfile> {
        self.sa_profiles.iter().find(|p| &p.name == section)
    }

    /// 选中档案 → 清 sa_new 并填充编辑器输入。
    pub(crate) fn sa_select(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(profile) = self.sa_profiles.iter().find(|p| p.name == name).cloned() else {
            return;
        };
        if let Some(st) = self.settings.clone() {
            st.update(cx, |s, cx| {
                s.sa_new = false;
                s.section = profile.name.clone();
                s.error = None;
                cx.notify();
            });
        }
        self.sa_fill_editor(&profile, cx);
    }

    /// 把档案字段写进编辑器输入（选中与面板打开时共用）。
    pub(crate) fn sa_fill_editor(
        &mut self,
        profile: &pi_link::subagents::SubagentProfile,
        cx: &mut Context<Self>,
    ) {
        let Some(st) = self.settings.clone() else { return };
        st.update(cx, |s, cx| {
            s.sa_name.update(cx, |ti, cx| ti.set_value(profile.name.clone(), cx));
            s.sa_display.update(cx, |ti, cx| ti.set_value(profile.display_name.clone(), cx));
            s.sa_desc.update(cx, |ti, cx| ti.set_value(profile.description.clone(), cx));
            s.sa_prompt.update(cx, |ti, cx| ti.set_value(profile.system_prompt.clone(), cx));
            s.sa_model
                .update(cx, |ti, cx| ti.set_value(profile.model.clone().unwrap_or_default(), cx));
            s.sa_turns.update(cx, |ti, cx| {
                ti.set_value(profile.max_turns.map(|v| v.to_string()).unwrap_or_default(), cx)
            });
            cx.notify();
        });
    }

    /// 新建子代理表单。
    pub(crate) fn sa_begin_create(&mut self, cx: &mut Context<Self>) {
        if let Some(st) = self.settings.clone() {
            st.update(cx, |s, cx| {
                s.sa_new = true;
                s.section = "__new__".into();
                s.error = None;
                for input in [&s.sa_name, &s.sa_display, &s.sa_desc, &s.sa_prompt, &s.sa_model, &s.sa_turns] {
                    input.update(cx, |ti, cx| ti.set_value(String::new(), cx));
                }
                cx.notify();
            });
        }
    }

    /// Persist the agents global settings (builtInEnabled / maxConcurrent).
    pub(crate) fn sa_save_settings(&mut self, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        let max = self
            .settings
            .as_ref()
            .map(|st| st.read(cx).sa_input.clone())
            .and_then(|input| input.read(cx).value().parse::<u32>().ok())
            .unwrap_or(self.sa_settings.max_concurrent)
            .clamp(1, 32);
        self.sa_settings.max_concurrent = max;
        if let Err(e) = pi_link::subagents::write_settings(
            &pi_link::config::agent_dir(),
            &self.sa_settings,
        ) {
            self.mc_set_error(&crate::i18n::tf("写入 agents/settings.json 失败: {e}", &[("e", e)]), cx);
            return;
        }
        self.reload_settings_panel();
        cx.notify();
    }

    /// 工具勾选（即时写盘；内置档案只读）。
    pub(crate) fn sa_toggle_tool(&mut self, name: String, tool: String, cx: &mut Context<Self>) {
        let Some(mut profile) = self.sa_selected(&name).cloned() else { return };
        if profile.scope == SubagentScope::Builtin || profile.file_path.is_none() {
            return;
        }
        if profile.tools.contains(&tool) {
            profile.tools.retain(|t| t != &tool);
        } else {
            profile.tools.push(tool);
        }
        self.sa_write_profile(profile, cx);
    }

    /// bool 字段勾选（即时写盘）。
    pub(crate) fn sa_toggle_bool(&mut self, name: String, field: u8, cx: &mut Context<Self>) {
        let Some(mut profile) = self.sa_selected(&name).cloned() else { return };
        if profile.scope == SubagentScope::Builtin || profile.file_path.is_none() {
            return;
        }
        match field {
            BOOL_LOAD_SKILLS => profile.load_skills = !profile.load_skills,
            BOOL_LOAD_EXTENSIONS => profile.load_extensions = !profile.load_extensions,
            BOOL_INHERIT => profile.inherit_context = !profile.inherit_context,
            BOOL_BACKGROUND => profile.run_in_background = !profile.run_in_background,
            _ => {}
        }
        self.sa_write_profile(profile, cx);
    }

    /// 文本字段保存（名称/显示名/描述/系统指令/模型/最大轮数）。
    pub(crate) fn sa_save_text(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(mut profile) = self.sa_selected(&name).cloned() else { return };
        if profile.scope == SubagentScope::Builtin || profile.file_path.is_none() {
            return;
        }
        let Some(st) = self.settings.clone() else { return };
        let (display, desc, prompt, model, turns) = {
            let s = st.read(cx);
            let val = |e: &gpui::Entity<TextInput>| e.read(cx).value().trim().to_string();
            (
                val(&s.sa_display),
                val(&s.sa_desc),
                s.sa_prompt.read(cx).value().to_string(),
                val(&s.sa_model),
                val(&s.sa_turns),
            )
        };
        profile.display_name = if display.is_empty() { profile.name.clone() } else { display };
        profile.description = desc;
        profile.system_prompt = prompt;
        profile.model = (!model.is_empty()).then_some(model);
        profile.max_turns = turns.parse::<u32>().ok();
        self.sa_write_profile(profile, cx);
    }

    /// 新建档案落盘（scope 决定目录）。
    pub(crate) fn sa_create(&mut self, cx: &mut Context<Self>) {
        let Some(st) = self.settings.clone() else { return };
        let (name, display, desc, prompt, model, turns, project) = {
            let s = st.read(cx);
            let val = |e: &gpui::Entity<TextInput>| e.read(cx).value().trim().to_string();
            (
                val(&s.sa_name),
                val(&s.sa_display),
                val(&s.sa_desc),
                s.sa_prompt.read(cx).value().to_string(),
                val(&s.sa_model),
                val(&s.sa_turns),
                s.sa_scope_project,
            )
        };
        self.mc_clear_error(cx);
        if name.is_empty()
            || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return self.mc_set_error(tr("子代理 ID 只能使用字母、数字、_ 和 -"), cx);
        }
        let agent_dir = pi_link::config::agent_dir();
        let path = if project {
            self.cwd.join(".pi").join("agents").join(format!("{name}.md"))
        } else {
            agent_dir.join("agents").join(format!("{name}.md"))
        };
        if path.exists() {
            return self.mc_set_error(&crate::i18n::tf("{path} 已存在", &[("path", path.to_string_lossy().to_string())]), cx);
        }
        let profile = pi_link::subagents::SubagentProfile {
            name: name.clone(),
            display_name: if display.is_empty() { name.clone() } else { display },
            description: desc,
            system_prompt: prompt,
            tools: pi_link::subagents::TOOL_OPTIONS.iter().map(|s| s.to_string()).collect(),
            load_skills: true,
            load_extensions: true,
            model: (!model.is_empty()).then_some(model),
            thinking: None,
            max_turns: turns.parse::<u32>().ok(),
            inherit_context: true,
            run_in_background: false,
            enabled: true,
            scope: if project { SubagentScope::Project } else { SubagentScope::Global },
            file_path: Some(path),
            overridden: false,
        };
        self.sa_write_profile(profile.clone(), cx);
        if let Some(st) = self.settings.clone() {
            st.update(cx, |s, cx| {
                s.sa_new = false;
                cx.notify();
            });
        }
        self.sa_select(profile.name, cx);
    }

    /// 写档案文件 + 重载（toggle/create/save 共用出口）。
    fn sa_write_profile(&mut self, profile: pi_link::subagents::SubagentProfile, cx: &mut Context<Self>) {
        let Some(path) = profile.file_path.clone() else { return };
        if let Err(e) = pi_link::subagents::write_profile_file(&path, &profile) {
            self.mc_set_error(&crate::i18n::tf("写入 profile 失败: {e}", &[("e", e)]), cx);
            return;
        }
        self.reload_settings_panel();
        cx.notify();
    }

    /// Enable/disable a profile: built-ins go into disabledBuiltIns, file
    /// profiles flip the `enabled` frontmatter key.
    pub(crate) fn sa_toggle_profile(&mut self, name: String, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        let Some(profile) = self.sa_profiles.iter().find(|p| p.name == name).cloned() else {
            return;
        };
        if profile.scope == SubagentScope::Builtin {
            if profile.enabled {
                self.sa_settings.disabled_built_ins.push(profile.name.clone());
            } else {
                self.sa_settings
                    .disabled_built_ins
                    .retain(|n| n != &profile.name);
            }
            if let Err(e) = pi_link::subagents::write_settings(
                &pi_link::config::agent_dir(),
                &self.sa_settings,
            ) {
                self.mc_set_error(&e, cx);
                return;
            }
        } else if let Some(path) = &profile.file_path {
            let mut next = profile.clone();
            next.enabled = !profile.enabled;
            if let Err(e) = pi_link::subagents::write_profile_file(path, &next) {
                self.mc_set_error(&crate::i18n::tf("写入 profile 失败: {e}", &[("e", e)]), cx);
                return;
            }
        }
        self.reload_settings_panel();
        cx.notify();
    }

    pub(crate) fn sa_delete_profile(&mut self, name: String, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        let Some(profile) = self.sa_profiles.iter().find(|p| p.name == name).cloned() else {
            return;
        };
        if let Some(path) = &profile.file_path {
            if let Err(e) = std::fs::remove_file(path) {
                self.mc_set_error(&crate::i18n::tf("删除失败: {e}", &[("e", e.to_string())]), cx);
                return;
            }
        }
        self.reload_settings_panel();
        if let Some(st) = self.settings.clone() {
            let first = self.sa_profiles.first().map(|p| p.name.clone()).unwrap_or_default();
            st.update(cx, |s, _| s.section = first);
        }
        cx.notify();
    }

    /// Run a profile: spawn a child vendored-pi RPC session with the
    /// profile's system prompt / tool allowlist / model / thinking level.
    pub(crate) fn sa_run(&mut self, name: String, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        let Some(profile) = self.sa_profiles.iter().find(|p| p.name == name).cloned() else {
            return;
        };
        let mut args: Vec<String> = Vec::new();
        if !profile.system_prompt.trim().is_empty() {
            args.push("--system-prompt".into());
            args.push(profile.system_prompt.trim().to_string());
        }
        if !profile.tools.is_empty() {
            args.push("--tools".into());
            args.push(profile.tools.join(","));
        }
        if let Some(model) = &profile.model {
            if !model.is_empty() {
                args.push("--model".into());
                args.push(model.clone());
            }
        }
        if let Some(thinking) = &profile.thinking {
            if !thinking.is_empty() {
                args.push("--thinking".into());
                args.push(thinking.clone());
            }
        }
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let (session, events) = match pi_link::client::spawn(
            &self.cwd,
            &arg_refs,
            crate::services::workspace::load_extensions_enabled(),
        ) {
            Ok(pair) => pair,
            Err(e) => {
                self.mc_set_error(&crate::i18n::tf("子代理启动失败: {e}", &[("e", e)]), cx);
                return;
            }
        };
        self.sa_run_seq += 1;
        let id = self.sa_run_seq;
        self.sa_runs.push(SubagentRun {
            id,
            profile: profile.name.clone(),
            status: 0,
            last_text: String::new(),
            session: Some(session),
        });
        cx.notify();
        // pump the child session's events until it settles
        let run_id = id;
        cx.spawn(async move |this, cx| {
            let mut events = events;
            while let Some(ev) = events.next().await {
                let settled = matches!(ev, pi_link::protocol::Event::AgentSettled);
                let alive = this
                    .update(cx, |c, cx| c.on_subagent_event(run_id, ev, cx))
                    .is_ok();
                if !alive || settled {
                    break;
                }
            }
        })
        .detach();
    }

    pub(crate) fn on_subagent_event(
        &mut self,
        run_id: usize,
        event: pi_link::protocol::Event,
        cx: &mut Context<Self>,
    ) {
        let Some(run) = self.sa_runs.iter_mut().find(|r| r.id == run_id) else { return };
        match event {
            pi_link::protocol::Event::AgentEnd { .. } => {
                if run.status == 0 {
                    run.status = 1;
                    if let Some(session) = &run.session {
                        let _ = session.send(&pi_link::protocol::Command::GetLastAssistantText);
                    }
                    cx.notify();
                }
            }
            pi_link::protocol::Event::Response { command, success, data, .. }
                if command == "get_last_assistant_text" =>
            {
                if success {
                    run.last_text = data
                        .as_ref()
                        .and_then(|d| d["text"].as_str())
                        .unwrap_or("")
                        .to_string();
                    cx.notify();
                }
            }
            _ => {}
        }
    }

    pub(crate) fn sa_abort_run(&mut self, run_id: usize, cx: &mut Context<Self>) {
        if let Some(run) = self.sa_runs.iter_mut().find(|r| r.id == run_id) {
            if run.status == 0 {
                if let Some(session) = &run.session {
                    let _ = session.send(&pi_link::protocol::Command::Abort);
                }
                run.status = 3;
                cx.notify();
            }
        }
    }
}

/// Subagents tab：特性条 + 分栏（新建表单 / 档案表单 / 运行详情）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn mc_subagents_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
    sa_input: &gpui::Entity<TextInput>,
    sa_new: bool,
    sa_scope_project: bool,
    sa_name: &gpui::Entity<TextInput>,
    sa_display: &gpui::Entity<TextInput>,
    sa_desc: &gpui::Entity<TextInput>,
    sa_prompt: &gpui::Entity<TextInput>,
    sa_model: &gpui::Entity<TextInput>,
    sa_turns: &gpui::Entity<TextInput>,
) -> gpui::AnyElement {
    let t = T();
    let fea_on = chat.sa_settings.builtin_enabled;

    let mut col = div().flex().flex_col().w_full().h_full().min_h_0();
    // 特性条（pi-web agents-feature-setting）；与扩展页安装栏同款：
    // pl15/pr20 与列表/详情内容缘对齐，底部划线
    col = col.child(
        div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(12.))
            .pl(px(15.))
            .pr(px(20.))
            .pt(px(6.))
            .pb(px(10.))
            .border_b_1()
            .border_color(rgb(t.border))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(crate::appearance::ui_size(13.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(t.text))
                            .child(tr("启用内置子代理")),
                    )
                    .child(
                        div()
                            .text_size(crate::appearance::ui_size(11.))
                            .text_color(rgb(t.text_dim))
                            .child(tr("提供会话内的 Agent 工具与三个内置档案")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(crate::appearance::ui_size(11.))
                            .text_color(rgb(t.text_muted))
                            .child(tr("并发子代理数")),
                    )
                    .child(div().w(px(52.)).child(sa_input.clone()))
                    .child(config_button("sa-max-save", weak, &tr("保存"), Btn::Secondary, true, false, |c, cx| {
                        c.sa_save_settings(cx)
                    }))
                    .child(config_switch("sa-fea-switch", weak, fea_on, false, move |c, cx| {
                        c.sa_settings.builtin_enabled = !c.sa_settings.builtin_enabled;
                        if let Err(e) = pi_link::subagents::write_settings(
                            &pi_link::config::agent_dir(),
                            &c.sa_settings,
                        ) {
                            c.mc_set_error(&e, cx);
                            return;
                        }
                        c.reload_settings_panel();
                        cx.notify();
                    })),
            ),
    );

    let sb = sa_sidebar(chat, weak, section);
    let detail = sa_detail(
        chat, weak, section, sa_new, sa_scope_project,
        sa_name, sa_display, sa_desc, sa_prompt, sa_model, sa_turns,
    );
    col.child(two_pane(sb, detail)).into_any_element()
}

/// two_pane 的本页版本（mod.rs 的 private 复制品，签名相同）。
fn two_pane(sidebar: gpui::AnyElement, detail: gpui::AnyElement) -> gpui::AnyElement {
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .child(sidebar)
        .child(detail)
        .into_any_element()
}

/// Subagents sidebar: runs first, then profiles by scope + 新建子代理.
fn sa_sidebar(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
) -> gpui::AnyElement {
    let t = T();
    // px7 + 行内 px8 = 内容左右 15px（与右侧详情 p15 等距，040 扩展页定稿）
    let mut list = sidebar_list().px(px(7.));
    if !chat.sa_runs.is_empty() {
        list = list.child(group_header(&tr("运行"), None));
        for run in &chat.sa_runs {
            let active = section == format!("run-{}", run.id);
            let (dot, status_text) = match run.status {
                0 => (t.accent, tr("运行中")),
                1 => (GREEN, tr("已完成")),
                2 => (0xf87171, tr("失败")),
                _ => (0xfacc15, tr("已中止")),
            };
            let weak_item = weak.clone();
            let sel = format!("run-{}", run.id);
            list = list.child(
                widgets::sidebar_item(format!("sa-run-{}", run.id), active)
                    // 行背景压平（040：列表无底色，选中态只靠加粗+深字色，hover 保留）
                    .bg(rgb(t.bg))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let _ = weak_item.update(cx, |c, cx| {
                            if let Some(st) = c.settings.clone() {
                                st.update(cx, |s, cx| {
                                    s.section = sel.clone();
                                    s.error = None;
                                    cx.notify();
                                });
                            }
                        });
                    })
                    .child(status_dot(dot))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(SharedString::from(run.profile.clone())),
                    )
                    .child(
                        div()
                            .text_size(crate::appearance::ui_size(10.))
                            .text_color(rgb(t.text_dim))
                            .child(status_text),
                    ),
            );
        }
    }
    for (label, scope) in [
        (tr("内置"), SubagentScope::Builtin),
        (tr("全局"), SubagentScope::Global),
        (tr("工作区"), SubagentScope::Workspace),
        (tr("项目"), SubagentScope::Project),
    ] {
        let items: Vec<&pi_link::subagents::SubagentProfile> =
            chat.sa_profiles.iter().filter(|p| p.scope == scope).collect();
        if items.is_empty() {
            continue;
        }
        list = list.child(group_header(label, None));
        for p in items {
            let active = p.name == section && section != "__new__";
            let weak_item = weak.clone();
            let name = p.name.clone();
            list = list.child(
                widgets::sidebar_item(format!("sa-prof-{}", p.name), active)
                    .bg(rgb(t.bg))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let _ = weak_item.update(cx, |c, cx| c.sa_select(name.clone(), cx));
                    })
                    .child(status_dot(if p.enabled { GREEN } else { t.border }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(SharedString::from(p.display_name.clone())),
                    )
                    .child(if p.overridden {
                        div()
                            .text_size(crate::appearance::ui_size(9.))
                            .text_color(rgb(t.text_dim))
                            .child(tr("覆盖"))
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
        .child(list_action("sa-new", weak, &tr("新建子代理"), section == "__new__", |c, cx| {
            c.sa_begin_create(cx)
        }))
        .into_any_element()
}

/// Detail: new-profile form / profile form / run detail / empty.
#[allow(clippy::too_many_arguments)]
fn sa_detail(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
    sa_new: bool,
    sa_scope_project: bool,
    sa_name: &gpui::Entity<TextInput>,
    sa_display: &gpui::Entity<TextInput>,
    sa_desc: &gpui::Entity<TextInput>,
    sa_prompt: &gpui::Entity<TextInput>,
    sa_model: &gpui::Entity<TextInput>,
    sa_turns: &gpui::Entity<TextInput>,
) -> gpui::AnyElement {
    if let Some(run_str) = section.strip_prefix("run-") {
        let run = run_str.parse::<usize>().ok().and_then(|id| chat.sa_runs.iter().find(|r| r.id == id));
        return match run {
            None => div()
                .flex_1()
                .p(px(15.))
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t_dim()))
                .child(tr("运行已结束"))
                .into_any_element(),
            Some(run) => sa_run_body(run, weak),
        };
    }
    if sa_new && section == "__new__" {
        return sa_create_form(chat, weak, sa_scope_project, sa_name, sa_display, sa_desc, sa_prompt, sa_model, sa_turns);
    }
    if let Some(p) = chat.sa_selected(section).cloned() {
        return sa_profile_form(chat, weak, p, sa_display, sa_desc, sa_prompt, sa_model, sa_turns);
    }
    div()
        .flex_1()
        .p(px(15.))
        .text_size(crate::appearance::ui_size(12.))
        .text_color(rgb(t_dim()))
        .child(tr("选择或创建一个子代理配置"))
        .into_any_element()
}

fn t_dim() -> u32 {
    T().text_dim
}

/// 新建表单（scope 切换 + 全字段 + 创建）。
#[allow(clippy::too_many_arguments)]
fn sa_create_form(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    sa_scope_project: bool,
    sa_name: &gpui::Entity<TextInput>,
    sa_display: &gpui::Entity<TextInput>,
    sa_desc: &gpui::Entity<TextInput>,
    sa_prompt: &gpui::Entity<TextInput>,
    sa_model: &gpui::Entity<TextInput>,
    sa_turns: &gpui::Entity<TextInput>,
) -> gpui::AnyElement {
    let t = T();
    let weak_scope = weak.clone();
    // 详情内边距与列表 15px 统一（040 扩展页定稿；detail_shell 默认 p20）
    detail_shell("mc-detail")
        .p(px(15.))
        .child(section_title(&tr("新建子代理")))
        .child(
            // scope 双选 + 路径预览
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child({
                    let mk = |ix: &'static str, label: &'static str, project: bool| {
                        let weak_opt = weak_scope.clone();
                        div()
                            .id(SharedString::from(format!("sa-scope-{ix}")))
                            .h(px(28.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded(px(5.))
                            .border_1()
                            .border_color(rgb(if sa_scope_project == project { t.accent } else { t.border }))
                            .bg(rgb(if sa_scope_project == project { t.bg_selected } else { t.bg_panel }))
                            .text_size(crate::appearance::ui_size(11.))
                            .text_color(rgb(t.text))
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let _ = weak_opt.update(cx, |c, cx| {
                                    if let Some(st) = c.settings.clone() {
                                        st.update(cx, |s, cx| {
                                            s.sa_scope_project = project;
                                            cx.notify();
                                        });
                                    }
                                });
                            })
                            .child(label)
                    };
                    div().flex().gap(px(6.)).child(mk("g", "全局", false)).child(mk("p", "项目", true))
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
                        .child(SharedString::from(if sa_scope_project {
                            format!("{}\\.pi\\agents\\<ID>.md", chat.cwd.display())
                        } else {
                            "~\\.pi\\agent\\agents\\<ID>.md".to_string()
                        })),
                ),
        )
        .child(
            div()
                .flex()
                .gap(px(10.))
                .child(field(&tr("ID"), div().w(px(200.)).child(sa_name.clone())))
                .child(field(&tr("显示名称"), div().w(px(200.)).child(sa_display.clone()))),
        )
        .child(field(&tr("描述"), div().w(px(430.)).child(sa_desc.clone())))
        .child(field(&tr("系统指令"), prompt_editor(sa_prompt, false)))
        .child(
            div()
                .flex()
                .gap(px(10.))
                .child(field(&tr("指定模型"), div().w(px(240.)).child(sa_model.clone())))
                .child(field(&tr("最大轮次"), div().w(px(90.)).child(sa_turns.clone()))),
        )
        .child(note("工具 / 资源默认全开，创建后可在档案页调整"))
        .child(
            div()
                .pt(px(4.))
                .child(config_button("sa-create", weak, &tr("创建"), Btn::Primary, false, false, |c, cx| {
                    c.sa_create(cx)
                })),
        )
        .into_any_element()
}

/// 档案表单（header 操作 + 字段 + 即时勾选 + 文本保存）。
fn sa_profile_form(
    _chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    p: pi_link::subagents::SubagentProfile,
    sa_display: &gpui::Entity<TextInput>,
    sa_desc: &gpui::Entity<TextInput>,
    sa_prompt: &gpui::Entity<TextInput>,
    sa_model: &gpui::Entity<TextInput>,
    sa_turns: &gpui::Entity<TextInput>,
) -> gpui::AnyElement {
    let t = T();
    let builtin = p.scope == SubagentScope::Builtin;
    let editable = !builtin && p.file_path.is_some();
    let name = p.name.clone();

    let tools_row = div().flex().flex_wrap().gap(px(8.)).children(
        pi_link::subagents::TOOL_OPTIONS.iter().map(|tool| {
            let checked = p.tools.iter().any(|x| x == tool);
            check_chip(
                format!("sa-tool-{name}-{tool}"),
                weak,
                tool,
                checked,
                !editable,
                {
                    let n = name.clone();
                    let tl = tool.to_string();
                    move |c, cx| c.sa_toggle_tool(n.clone(), tl.clone(), cx)
                },
            )
        }),
    );

    // 详情内边距与列表 15px 统一（040 扩展页定稿；detail_shell 默认 p20）
    let mut detail = detail_shell("mc-detail")
        .p(px(15.))
        // header：scope + 名称 + 路径 | 启用开关 + 运行 + 删除
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .min_h(px(28.))
                .child(scope_tag(p.scope.label(), p.scope == SubagentScope::Project))
                .child(
                    div()
                        .font_family(crate::markdown::MONO_FAMILY)
                        .text_size(crate::appearance::ui_size(11.))
                        .text_color(rgb(t.text_dim))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(SharedString::from(
                            p.file_path
                                .as_ref()
                                .map(|f| f.to_string_lossy().to_string())
                                .unwrap_or_else(|| tr("内置配置").to_string()),
                        )),
                )
                .child(div().flex_1())
                .child(config_switch("sa-switch", weak, p.enabled, false, {
                    let n = name.clone();
                    move |c, cx| c.sa_toggle_profile(n.clone(), cx)
                }))
                .child(config_button("sa-run", weak, &tr("运行"), Btn::Primary, true, false, {
                    let n = name.clone();
                    move |c, cx| c.sa_run(n.clone(), cx)
                }))
                .children((!builtin).then(|| {
                    config_button("sa-delete", weak, &tr("删除"), Btn::Danger, true, false, {
                        let n = name.clone();
                        move |c, cx| c.sa_delete_profile(n.clone(), cx)
                    })
                })),
        )
        .child(
            div()
                .flex()
                .gap(px(10.))
                .child(field(
                    &tr("显示名称"),
                    if builtin {
                        mono_text(p.display_name.clone(), false)
                    } else {
                        div().w(px(200.)).child(sa_display.clone()).into_any_element()
                    },
                ))
                .child(field(
                    &tr("指定模型"),
                    if builtin {
                        mono_text(p.model.clone().unwrap_or_else(|| tr("跟随父会话").into()), false)
                    } else {
                        div().w(px(240.)).child(sa_model.clone()).into_any_element()
                    },
                ))
                .child(field(
                    &tr("最大轮次"),
                    if builtin {
                        mono_text(p.max_turns.map(|x| x.to_string()).unwrap_or_else(|| "∞".into()), false)
                    } else {
                        div().w(px(90.)).child(sa_turns.clone()).into_any_element()
                    },
                )),
        )
        .child(field(
            &tr("描述"),
            if builtin {
                div()
                    .text_size(crate::appearance::ui_size(12.))
                    .text_color(rgb(t.text_muted))
                    .child(SharedString::from(p.description.clone()))
                    .into_any_element()
            } else {
                div().w(px(520.)).child(sa_desc.clone()).into_any_element()
            },
        ))
        .child(field(
            &tr("系统指令"),
            if builtin {
                if p.system_prompt.is_empty() {
                    note("（空）").into_any_element()
                } else {
                    mono_text(p.system_prompt.clone(), false).into_any_element()
                }
            } else {
                prompt_editor(sa_prompt, false).into_any_element()
            },
        ))
        .child(field(&tr("工具"), tools_row))
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(8.))
                .child(check_chip(
                    format!("sa-bs-{name}"),
                    weak,
                    &tr("加载技能"),
                    p.load_skills,
                    !editable,
                    {
                        let n = name.clone();
                        move |c, cx| c.sa_toggle_bool(n.clone(), BOOL_LOAD_SKILLS, cx)
                    },
                ))
                .child(check_chip(
                    format!("sa-be-{name}"),
                    weak,
                    &tr("加载扩展"),
                    p.load_extensions,
                    !editable,
                    {
                        let n = name.clone();
                        move |c, cx| c.sa_toggle_bool(n.clone(), BOOL_LOAD_EXTENSIONS, cx)
                    },
                )),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(8.))
                .child(check_chip(
                    format!("sa-bi-{name}"),
                    weak,
                    &tr("继承父会话上下文"),
                    p.inherit_context,
                    !editable,
                    {
                        let n = name.clone();
                        move |c, cx| c.sa_toggle_bool(n.clone(), BOOL_INHERIT, cx)
                    },
                ))
                .child(check_chip(
                    format!("sa-bb-{name}"),
                    weak,
                    &tr("默认在后台运行"),
                    p.run_in_background,
                    !editable,
                    {
                        let n = name.clone();
                        move |c, cx| c.sa_toggle_bool(n.clone(), BOOL_BACKGROUND, cx)
                    },
                )),
        );

    if editable {
        detail = detail.child(
            div()
                .pt(px(4.))
                .child(config_button("sa-save-text", weak, &tr("保存"), Btn::Secondary, false, false, {
                    let n = name.clone();
                    move |c, cx| c.sa_save_text(n.clone(), cx)
                })),
        );
    }
    detail.into_any_element()
}

/// 系统指令编辑区（高 195px 的文本输入壳）。
fn prompt_editor(input: &gpui::Entity<TextInput>, disabled: bool) -> gpui::AnyElement {
    let t = T();
    let mut shell = div()
        .id("sa-prompt-editor")
        .w(px(560.))
        .h(px(160.))
        .rounded(px(5.))
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.bg_panel))
        .p(px(6.))
        .overflow_y_scroll();
    if disabled {
        shell = shell.opacity(0.55);
    }
    shell.child(input.clone()).into_any_element()
}

/// Run detail body（保留 v54 行为：状态 + 中止 + 输出）。
fn sa_run_body(run: &SubagentRun, weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    let t = T();
    let abort_id = run.id;
    let (status_text, status_color) = match run.status {
        0 => (tr("运行中"), t.accent),
        1 => (tr("已完成"), GREEN),
        2 => (tr("失败"), 0xf87171),
        _ => (tr("已中止"), 0xfacc15),
    };
    // 详情内边距与列表 15px 统一（040 扩展页定稿；detail_shell 默认 p20）
    let mut detail = detail_shell("mc-detail")
        .p(px(15.))
        .child(
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
                    .child(SharedString::from(run.profile.clone())),
            )
            .child(
                div()
                    .text_size(crate::appearance::ui_size(11.))
                    .text_color(rgb(status_color))
                    .child(status_text),
            )
            .child(div().flex_1())
            .children((run.status == 0).then(|| {
                config_button("sa-abort", weak, &tr("中止"), Btn::Danger, true, false, move |c, cx| {
                    c.sa_abort_run(abort_id, cx)
                })
            })),
    );
    if !run.last_text.is_empty() {
        detail = detail.child(
            div()
                .flex()
                .flex_col()
                .gap(px(5.))
                .child(section_title(&tr("输出")))
                .child(
                    div()
                        .p(px(9.))
                        .rounded(px(6.))
                        .border_1()
                        .border_color(rgb(t.border))
                        .bg(rgb(t.bg_panel))
                        .font_family(crate::markdown::MONO_FAMILY)
                        .text_size(crate::appearance::ui_size(11.))
                        .text_color(rgb(t.text))
                        .flex()
                        .flex_col()
                        .children(run.last_text.lines().map(|l| {
                            div().child(SharedString::from(l.to_string()))
                        })),
                ),
        );
    }
    detail.into_any_element()
}
