//! Settings-子代理页（044-1 §2）：Agent 定义管理（实例层之外的「类」层）。
//!
//! T 型布局（2026-11-07 定稿）：首行「全局设置」通栏（模块开关 + 最大并发，
//! 见 dropdowns::global_settings_bar）+ 左列表（内置/自定义两组）+ 右表单。
//! 表单化只覆盖 v1 已定字段集；未知字段解析时透传、写回时拼回（见
//! `pi_link::agents`）。
//!
//! 尺寸/配色逐值照抄 `docs/UI设计/设置-子代理页-原型.html`（已过稿）：列表
//! 宽 400（[`widgets::LIST_W`]）、行 30px 圆点 7px、label 96px 上沿对齐、
//! 输入 30px、系统提示 textarea 170px、下拉宽 250。
//!
//! itlq.2 按区块拆子模块（只搬不改）：本文件 = Chat 动作与共享构件
//! （AG_NEW 哨兵 / agent_row / sel_trigger / tool_tiers）；view = 页骨架
//! （T 型装配）；detail = 右表单；dropdowns = 全局设置顶条与各下拉勾选件。

use gpui::prelude::FluentBuilder;

use super::*;
use pi_link::agents::{AgentOrigin, AgentProfile, MaxTurnsTier};

pub(crate) mod detail;
pub(crate) mod dropdowns;
pub(crate) mod view;

pub(crate) use view::mc_agents_view;

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
