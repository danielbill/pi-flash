//! Settings panel：左导航页签（界面/模型/技能/扩展/MCP/其他）。
//! 内部页面按 pi-web 最新版重构（路径条/分组侧栏/详情表单/底栏）。表单
//! 状态由 SettingsPanel entity 持有；mc_*/mcp_* 动作仍在 Chat 上（单一
//! RPC 属主）。子代理页已移除——社区子代理扩展经「扩展」页安装即用，
//! 不再建模任何 subagent 配置。原「微信 Bot」页签已删除——内容迁左列
//! 「微信」面板（`remote_control::panel`，060，2026-10-23 定夺）。

pub(crate) mod about;
pub(crate) mod agents;
pub(crate) mod custom_models;
pub(crate) mod general;
pub(crate) mod mcp;
pub(crate) mod misc;
pub(crate) mod models;
pub(crate) mod pi_extensions;
pub(crate) mod shortcuts;
pub(crate) mod skills;
pub(crate) mod widgets;

pub(crate) use models::{MjCatalogState, MjSpeedState, MjTestState};

use general::mc_general_view;
use misc::mc_misc_view;

use super::*;
pub(crate) use crate::ui::{DropdownState, VListHeight, icon, vlist};
pub(crate) use widgets::{
    Btn, LIST_W, SwitchMark, config_button, config_switch, detail_shell, error_note, field,
    group_header, group_switch, mono_text, note, scope_tag, section_title, set_row, sidebar_list,
    sidebar_shell, status_dot, switch_base, width_slider,
};

/// 页签序：0 界面 · 1 模型 · 2 技能 · 3 扩展 · 4 MCP · 5 其他 · 8 快捷键
/// （原 6 微信 Bot 页签已删除，内容迁左列「微信」面板——2026-10-23 定）。
pub(crate) const TAB_GENERAL: u8 = 0;
pub(crate) const TAB_MODELS: u8 = 1;
pub(crate) const TAB_SKILLS: u8 = 2;
pub(crate) const TAB_PLUGINS: u8 = 3;
pub(crate) const TAB_MCP: u8 = 4;
/// 子代理页（044-1 §2）：Agent 定义管理（内置/自定义）。
pub(crate) const TAB_AGENTS: u8 = 9;
pub(crate) const TAB_MISC: u8 = 5;
pub(crate) const TAB_ABOUT: u8 = 7;
pub(crate) const TAB_SHORTCUTS: u8 = 8;

/// 思考等级映射的档位（pi-web THINKING_LEVELS 顺序与配色）。
pub(crate) const TLM_LEVELS: [&str; 7] =
    ["off", "minimal", "low", "medium", "high", "xhigh", "max"];
pub(crate) const TLM_LEVEL_COUNT: usize = 7;

/// 思考档映射一行的档位态：不设（Default）/ 禁用（null）/ 自定义映射（字符串）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MjTlmMode {
    Omit,
    Null,
    Custom,
}

/// 思考档映射一行（档位态 + 自定义值输入；输入仅在 Custom 态读值）。
#[derive(Clone)]
pub(crate) struct MjTlmRow {
    pub mode: MjTlmMode,
    pub value: gpui::Entity<TextInput>,
}

/// headers 编辑一行（name/value 两个输入框）。
#[derive(Clone)]
pub(crate) struct MjHeaderRow {
    pub name: gpui::Entity<TextInput>,
    pub value: gpui::Entity<TextInput>,
}

/// The settings modal's form state (pi-web SettingsPanel own-state parity).
pub(crate) struct SettingsPanel {
    /// modal focus (escape + click-away target)
    pub focus: gpui::FocusHandle,
    pub tab: u8,
    /// selected entry in the tab's sidebar (provider id / skill path /
    /// package source / mcp "scope:name" / "__add__" & "__new__" forms)
    pub section: String,
    // -- 模型页：API key 输入 + 可用模型筛选 + models.json 编辑器（保存前缓冲）
    pub key_input: gpui::Entity<TextInput>,
    pub key_visible: bool,
    pub model_filter: gpui::Entity<TextInput>,
    /// 042：合并列表的模型筛选（启用总数 ≥ MODEL_FILTER_MIN 才显示）
    pub enabled_filter: gpui::Entity<TextInput>,
    /// 可用模型区「只看free」开关（开 = 列表只显示 free 模型，
    /// 筛选叠加隐藏的 free 关键字；纯面板内视图态，不落盘）
    pub model_free_only: bool,
    pub mj_name: gpui::Entity<TextInput>,
    pub mj_base: gpui::Entity<TextInput>,
    pub mj_key: gpui::Entity<TextInput>,
    pub mj_id: gpui::Entity<TextInput>,
    pub mj_mname: gpui::Entity<TextInput>,
    pub mj_ctx: gpui::Entity<TextInput>,
    pub mj_reasoning: bool,
    // -- 042 M2 自定义 provider/model 编辑器扩展（pi-web parity）
    /// headers 编辑行（name, value 输入框对；provider/model 编辑器共用槽，
    /// mj_select 按当前 section 回填）
    pub mj_headers: Vec<MjHeaderRow>,
    /// 思考等级映射 7 档行状态（顺序 = [`TLM_LEVELS`]）
    pub mj_tlm: [MjTlmRow; TLM_LEVEL_COUNT],
    /// 模型编辑器：高级折叠区展开态
    pub mj_adv_open: bool,
    /// 模型级 API 覆盖（4 = 沿用 Provider；0..=3 = API_OPTIONS 档位）
    pub mj_api_override: u8,
    /// 兼容：DeepSeek 思考兼容
    pub mj_deepseek: bool,
    /// 兼容：developer role（effective 默认 true；取消勾选才落显式 false）
    pub mj_devrole: bool,
    /// 能力：图片输入
    pub mj_image: bool,
    /// 最大输出 tokens
    pub mj_max_tokens: gpui::Entity<TextInput>,
    /// 价格编辑态 + 四输入（每百万）
    pub mj_cost_edit: bool,
    pub mj_cost_in: gpui::Entity<TextInput>,
    pub mj_cost_out: gpui::Entity<TextInput>,
    pub mj_cost_cr: gpui::Entity<TextInput>,
    pub mj_cost_cw: gpui::Entity<TextInput>,
    /// API 下拉（pi-web Select 对齐）：0 = Provider 表单，1 = 模型 API 覆盖；
    /// None = 全关（两处不同帧出现，共享一个开关槽即可）
    pub mj_api_popup: Option<u8>,
    /// API 下拉的外点收起守卫
    pub mj_api_dd: gpui::Entity<DropdownState>,
    /// 自定义商 API Key 的显隐（掩码眼睛；mj_select 回填即回掩码态）
    pub mj_key_visible: bool,
    // -- 扩展页安装表单（040：整条 pi install 命令可直接粘贴）
    pub install_input: gpui::Entity<TextInput>,
    // -- MCP 页【配置MCP】表单（043：粘贴框 + 名称，全局 only）
    pub mcp_add: gpui::Entity<TextInput>,
    pub mcp_name: gpui::Entity<TextInput>,
    // -- 子代理页表单（044-1 §2）
    pub ag_name: gpui::Entity<TextInput>,
    pub ag_desc: gpui::Entity<TextInput>,
    pub ag_prompt: gpui::Entity<TextInput>,
    pub ag_turns_n: gpui::Entity<TextInput>,
    /// 模型（None = 键省略 = 继承会话模型）
    pub ag_model: Option<String>,
    /// 工具档位（inherit/read-only/default/full）
    pub ag_tools: String,
    /// 最大轮次档
    pub ag_turns: pi_link::agents::MaxTurnsTier,
    pub ag_inject: bool,
    /// 哪个下拉开着（0=模型 1=工具；None=全关）
    pub ag_popup: Option<u8>,
    /// 下拉外点收起守卫（模型/工具两个共享）
    pub ag_dd: gpui::Entity<DropdownState>,
    /// 有未保存改动（表单头黄点 + 保存键可用）
    pub ag_dirty: bool,
    pub error: Option<String>,
    /// 界面页：打开的字体下拉（槽位 ix；None=全关）
    pub font_popup: Option<usize>,
    /// 字体下拉组件的外点收起守卫（三个触发按钮共享）
    pub font_dd: gpui::Entity<DropdownState>,
    /// 字体下拉顶部的筛选输入
    pub font_filter: gpui::Entity<TextInput>,
    /// 界面页：打开的字号三档下拉（槽位 ix；None=全关）
    pub size_popup: Option<usize>,
    /// 字号下拉的外点收起守卫（三个触发按钮共享）
    pub size_dd: gpui::Entity<DropdownState>,
}

/// 建一个 TextInput（面板字段共用的小工厂）。值在保存/应用时才读的
/// 字段用这个即可。
fn panel_input(
    placeholder: &'static str,
    cx: &mut gpui::Context<SettingsPanel>,
) -> gpui::Entity<TextInput> {
    cx.new(|cx| TextInput::new(cx).placeholder(tr(placeholder)))
}

/// 建一个「实时」TextInput：每敲一字通知面板重渲染（可用模型筛选、
/// MCP 添加预览这类边输边变的面板）。不挂 on_change 的输入框能收键，
/// 但面板拿不到新值快照，UI 纹丝不动——font 筛选（v60）与这里的差别。
fn panel_live_input(
    placeholder: &'static str,
    cx: &mut gpui::Context<SettingsPanel>,
) -> gpui::Entity<TextInput> {
    let weak = cx.weak_entity();
    cx.new(|cx| {
        TextInput::new(cx)
            .placeholder(tr(placeholder))
            .on_change(Box::new(move |_, cx| {
                if let Some(p) = weak.upgrade() {
                    p.update(cx, |_, cx| cx.notify());
                }
            }))
    })
}

impl SettingsPanel {
    pub(crate) fn new(cx: &mut gpui::Context<Self>) -> Self {
        // 筛选输入每敲一字 → 通知面板重渲染（列表按值过滤在渲染期读取快照）
        let weak_filter = cx.weak_entity();
        let weak_esc = cx.weak_entity();
        let font_filter = cx.new(|cx| {
            TextInput::new(cx)
                .placeholder(tr("settings.search_fonts"))
                .select_all_on_focus()
                .on_change(Box::new(move |_, cx| {
                    if let Some(p) = weak_filter.upgrade() {
                        p.update(cx, |_, cx| cx.notify());
                    }
                }))
                .on_escape(Box::new(move |cx| {
                    if let Some(p) = weak_esc.upgrade() {
                        p.update(cx, |s, cx| {
                            s.font_popup = None;
                            cx.notify();
                        });
                    }
                }))
        });
        Self {
            focus: cx.focus_handle(),
            tab: 0,
            section: String::new(),
            key_input: cx.new(|cx| TextInput::new(cx).masked(true)),
            key_visible: false,
            model_filter: panel_live_input(tr("settings.filter_models"), cx),
            enabled_filter: panel_live_input(tr("settings.filter_enabled"), cx),
            model_free_only: false,
            mj_name: panel_input("provider-name", cx),
            mj_base: panel_input("https://api.example.com/v1", cx),
            mj_key: cx.new(|cx| TextInput::new(cx).masked(true)),
            mj_id: panel_input("model-id", cx),
            mj_mname: panel_input(tr("settings.model_display_name"), cx),
            mj_ctx: cx.new(|cx| TextInput::new(cx).numeric(true)),
            mj_reasoning: false,
            mj_headers: Vec::new(),
            mj_tlm: std::array::from_fn(|_| MjTlmRow {
                mode: MjTlmMode::Omit,
                value: cx.new(|cx| TextInput::new(cx)),
            }),
            mj_adv_open: false,
            mj_api_override: 4,
            mj_deepseek: false,
            mj_devrole: true,
            mj_image: false,
            mj_max_tokens: cx.new(|cx| TextInput::new(cx).numeric(true)),
            mj_cost_edit: false,
            mj_cost_in: cx.new(|cx| TextInput::new(cx)),
            mj_cost_out: cx.new(|cx| TextInput::new(cx)),
            mj_cost_cr: cx.new(|cx| TextInput::new(cx)),
            mj_cost_cw: cx.new(|cx| TextInput::new(cx)),
            mj_api_popup: None,
            mj_api_dd: cx.new(|_| DropdownState::new()),
            mj_key_visible: false,
            install_input: cx
                .new(|cx| TextInput::new(cx).placeholder(tr("extensions.source_example"))),
            mcp_add: {
                let weak_add = cx.weak_entity();
                cx.new(|cx| {
                    TextInput::new(cx)
                        .placeholder(tr("mcp.form_placeholder"))
                        .on_change(Box::new(move |_, cx| {
                            if let Some(p) = weak_add.upgrade() {
                                p.update(cx, |_, cx| cx.notify());
                            }
                        }))
                        .multiline()
                })
            },
            mcp_name: panel_live_input(tr("mcp.field_name"), cx),
            ag_name: panel_live_input("", cx),
            ag_desc: panel_live_input("", cx),
            ag_prompt: {
                let weak_ag = cx.weak_entity();
                cx.new(|cx| {
                    TextInput::new(cx)
                        .placeholder(tr("agents.prompt_ph"))
                        .multiline()
                        .on_change(Box::new(move |_, cx| {
                            if let Some(p) = weak_ag.upgrade() {
                                p.update(cx, |s, cx| {
                                    s.ag_dirty = true;
                                    cx.notify();
                                });
                            }
                        }))
                })
            },
            ag_turns_n: {
                let weak_tn = cx.weak_entity();
                cx.new(|cx| {
                    TextInput::new(cx)
                        .numeric(true)
                        .placeholder("25")
                        .on_change(Box::new(move |_, cx| {
                            if let Some(p) = weak_tn.upgrade() {
                                p.update(cx, |s, cx| {
                                    s.ag_dirty = true;
                                    cx.notify();
                                });
                            }
                        }))
                })
            },
            ag_model: None,
            ag_tools: "inherit".into(),
            ag_turns: pi_link::agents::MaxTurnsTier::Hundred,
            ag_inject: true,
            ag_popup: None,
            ag_dd: cx.new(|_| DropdownState::new()),
            ag_dirty: false,
            error: None,
            font_popup: None,
            font_dd: cx.new(|_| DropdownState::new()),
            font_filter,
            size_popup: None,
            size_dd: cx.new(|_| DropdownState::new()),
        }
    }

    /// 打开某槽位的字体下拉（弹层锚定由 dropdown 组件负责），清空筛选。
    /// 筛选框聚焦走 focus_soon（渲染期 best-effort）。
    pub(crate) fn open_font_popup(&mut self, slot: usize, cx: &mut gpui::Context<Self>) {
        self.font_popup = Some(slot);
        self.font_filter.update(cx, |ti, tcx| {
            ti.set_value(String::new(), tcx);
            ti.focus_soon(tcx);
        });
        cx.notify();
    }

    pub(crate) fn close_font_popup(&mut self, cx: &mut gpui::Context<Self>) {
        if self.font_popup.take().is_some() {
            cx.notify();
        }
    }

    /// 焦点是否落在面板（含其全部输入框）内 —— Chat 焦点策略链用它判断
    /// 要不要把焦点拽回 dialog_focus。**新增输入框自动覆盖**，不再维护
    /// 白名单（v60 字体筛选、v70 模型/MCP 各输入框先后被"每帧抢焦点"
    /// 咬过的教训：白名单必漏）。
    pub(crate) fn focus_within(&self, window: &gpui::Window, cx: &gpui::App) -> bool {
        let mut inputs: Vec<&gpui::Entity<TextInput>> = vec![
            &self.key_input,
            &self.model_filter,
            &self.enabled_filter,
            &self.mj_name,
            &self.mj_base,
            &self.mj_key,
            &self.mj_id,
            &self.mj_mname,
            &self.mj_ctx,
            &self.mj_max_tokens,
            &self.mj_cost_in,
            &self.mj_cost_out,
            &self.mj_cost_cr,
            &self.mj_cost_cw,
            &self.install_input,
            &self.mcp_add,
            &self.mcp_name,
            &self.font_filter,
            &self.ag_name,
            &self.ag_desc,
            &self.ag_prompt,
            &self.ag_turns_n,
        ];
        for row in &self.mj_headers {
            inputs.push(&row.name);
            inputs.push(&row.value);
        }
        for row in &self.mj_tlm {
            inputs.push(&row.value);
        }
        self.focus.is_focused(window)
            || inputs
                .iter()
                .any(|e| e.read(cx).focus_handle_in(cx).is_focused(window))
    }

    /// 切页签时的 section 预填（open_settings 与左导航点击共用）。
    pub(crate) fn prefill_section(chat: &Chat, tab: u8) -> String {
        match tab {
            TAB_MODELS => chat.mc_provider_ids().first().cloned().unwrap_or_default(),
            TAB_SKILLS => chat
                .cfg
                .mc_skills
                .first()
                .map(|s| s.path.to_string_lossy().to_string())
                .unwrap_or_default(),
            TAB_PLUGINS => chat
                .cfg
                .mc_pkgs_global
                .first()
                .map(pi_link::skills::entry_source)
                .unwrap_or_default(),
            // 子代理页：默认选第一个自定义条目（无则第一个内置）
            TAB_AGENTS => chat
                .cfg
                .mc_agents
                .iter()
                .find(|a| a.origin.is_custom())
                .or_else(|| chat.cfg.mc_agents.first())
                .map(|a| a.id.clone())
                .unwrap_or_default(),
            // 043：MCP 页打开**一律进新增模式**——section 预选第一行会让
            // 「新增」误走编辑改名路径（静默删掉被预选的条目，实测踩过）；
            // 编辑只能由点行进入
            TAB_MCP => "__mcp_add__".into(),
            _ => String::new(),
        }
    }
}

/// Stack snapshot of the panel state for one render pass (the views are
/// free functions without cx access to the entity).
#[derive(Clone)]
pub(crate) struct SettingsFormData {
    pub focus: gpui::FocusHandle,
    pub tab: u8,
    pub section: String,
    pub key_input: gpui::Entity<TextInput>,
    pub key_visible: bool,
    pub model_filter: gpui::Entity<TextInput>,
    pub enabled_filter: gpui::Entity<TextInput>,
    pub model_free_only: bool,
    pub enabled_filter_value: String,
    pub model_filter_value: String,
    pub mj_name: gpui::Entity<TextInput>,
    pub mj_base: gpui::Entity<TextInput>,
    pub mj_key: gpui::Entity<TextInput>,
    pub mj_id: gpui::Entity<TextInput>,
    pub mj_mname: gpui::Entity<TextInput>,
    pub mj_ctx: gpui::Entity<TextInput>,
    pub mj_reasoning: bool,
    pub mj_headers: Vec<MjHeaderRow>,
    pub mj_tlm: [MjTlmRow; TLM_LEVEL_COUNT],
    pub mj_adv_open: bool,
    pub mj_api_override: u8,
    pub mj_deepseek: bool,
    pub mj_devrole: bool,
    pub mj_image: bool,
    pub mj_max_tokens: gpui::Entity<TextInput>,
    pub mj_cost_edit: bool,
    pub mj_cost_in: gpui::Entity<TextInput>,
    pub mj_cost_out: gpui::Entity<TextInput>,
    pub mj_cost_cr: gpui::Entity<TextInput>,
    pub mj_cost_cw: gpui::Entity<TextInput>,
    pub mj_api_popup: Option<u8>,
    pub mj_api_dd: gpui::Entity<DropdownState>,
    pub mj_key_visible: bool,
    pub install_input: gpui::Entity<TextInput>,
    pub mcp_add: gpui::Entity<TextInput>,
    pub mcp_name: gpui::Entity<TextInput>,
    pub mcp_add_value: String,
    pub ag_name: gpui::Entity<TextInput>,
    pub ag_desc: gpui::Entity<TextInput>,
    pub ag_prompt: gpui::Entity<TextInput>,
    pub ag_turns_n: gpui::Entity<TextInput>,
    pub ag_model: Option<String>,
    pub ag_tools: String,
    pub ag_turns: pi_link::agents::MaxTurnsTier,
    pub ag_inject: bool,
    pub ag_popup: Option<u8>,
    pub ag_dd: gpui::Entity<DropdownState>,
    pub ag_dirty: bool,
    /// 表单三个文本域的值快照（只读态渲染静态文本用；视图无 cx）
    pub ag_name_value: String,
    pub ag_desc_value: String,
    pub ag_prompt_value: String,
    pub error: Option<String>,
    pub font_popup: Option<usize>,
    pub font_dd: gpui::Entity<DropdownState>,
    pub font_filter: gpui::Entity<TextInput>,
    /// 渲染期读不到 cx，过滤在快照时完成（按当前筛选值）
    pub font_filter_value: String,
    pub size_popup: Option<usize>,
    pub size_dd: gpui::Entity<DropdownState>,
}

impl SettingsFormData {
    pub(crate) fn snapshot(panel: &SettingsPanel, cx: &gpui::App) -> Self {
        let p = panel;
        Self {
            focus: p.focus.clone(),
            tab: p.tab,
            section: p.section.clone(),
            key_input: p.key_input.clone(),
            key_visible: p.key_visible,
            model_filter: p.model_filter.clone(),
            model_filter_value: p.model_filter.read(cx).value().to_string(),
            enabled_filter: p.enabled_filter.clone(),
            enabled_filter_value: p.enabled_filter.read(cx).value().to_string(),
            model_free_only: p.model_free_only,
            mj_name: p.mj_name.clone(),
            mj_base: p.mj_base.clone(),
            mj_key: p.mj_key.clone(),
            mj_id: p.mj_id.clone(),
            mj_mname: p.mj_mname.clone(),
            mj_ctx: p.mj_ctx.clone(),
            mj_reasoning: p.mj_reasoning,
            mj_headers: p.mj_headers.clone(),
            mj_tlm: p.mj_tlm.clone(),
            mj_adv_open: p.mj_adv_open,
            mj_api_override: p.mj_api_override,
            mj_deepseek: p.mj_deepseek,
            mj_devrole: p.mj_devrole,
            mj_image: p.mj_image,
            mj_max_tokens: p.mj_max_tokens.clone(),
            mj_cost_edit: p.mj_cost_edit,
            mj_cost_in: p.mj_cost_in.clone(),
            mj_cost_out: p.mj_cost_out.clone(),
            mj_cost_cr: p.mj_cost_cr.clone(),
            mj_cost_cw: p.mj_cost_cw.clone(),
            mj_api_popup: p.mj_api_popup,
            mj_api_dd: p.mj_api_dd.clone(),
            mj_key_visible: p.mj_key_visible,
            install_input: p.install_input.clone(),
            mcp_add: p.mcp_add.clone(),
            mcp_name: p.mcp_name.clone(),
            mcp_add_value: p.mcp_add.read(cx).value().to_string(),
            ag_name: p.ag_name.clone(),
            ag_desc: p.ag_desc.clone(),
            ag_prompt: p.ag_prompt.clone(),
            ag_turns_n: p.ag_turns_n.clone(),
            ag_model: p.ag_model.clone(),
            ag_tools: p.ag_tools.clone(),
            ag_turns: p.ag_turns,
            ag_inject: p.ag_inject,
            ag_popup: p.ag_popup,
            ag_dd: p.ag_dd.clone(),
            ag_dirty: p.ag_dirty,
            ag_name_value: p.ag_name.read(cx).value().to_string(),
            ag_desc_value: p.ag_desc.read(cx).value().to_string(),
            ag_prompt_value: p.ag_prompt.read(cx).value().to_string(),
            error: p.error.clone(),
            font_popup: p.font_popup,
            font_dd: p.font_dd.clone(),
            font_filter: p.font_filter.clone(),
            font_filter_value: p.font_filter.read(cx).value().to_string(),
            size_popup: p.size_popup,
            size_dd: p.size_dd.clone(),
        }
    }
}

pub(crate) fn render_settings(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    d: &SettingsFormData,
    window: &gpui::Window,
) -> gpui::AnyElement {
    let t = T();
    let SettingsFormData {
        focus: _focus,
        tab,
        section,
        install_input,
        mcp_add,
        mcp_add_value,
        mcp_name,
        error,
        font_popup,
        font_dd,
        font_filter,
        font_filter_value,
        size_popup,
        size_dd,
        ..
    } = d.clone();
    let weak_close = weak.clone();

    // 页内容：界面/其他=单列 rows（滚动在 body）；其余页自带左右分栏 +
    // 可选顶条/底栏（自身 h_full，body 不滚动）
    let pane: gpui::AnyElement = match tab {
        TAB_MODELS => crate::settings::models::mc_models_view(chat, weak, d),
        TAB_SKILLS => {
            let (sidebar, detail) = crate::settings::skills::mc_skills_view(chat, weak, &section);
            two_pane(sidebar, detail)
        }
        TAB_AGENTS => {
            let (sidebar, detail) =
                crate::settings::agents::mc_agents_view(chat, weak, &section, d);
            two_pane(sidebar, detail)
        }
        TAB_PLUGINS => crate::settings::pi_extensions::mc_pi_extensions_view(
            chat,
            weak,
            &section,
            &install_input,
            &error,
        ),
        TAB_MCP => crate::settings::mcp::mc_mcp_view(
            chat,
            weak,
            &section,
            &mcp_add,
            &mcp_add_value,
            &mcp_name,
            &error,
        ),
        TAB_MISC => mc_misc_view(chat, weak),
        TAB_ABOUT => crate::settings::about::mc_about_view(),
        TAB_SHORTCUTS => crate::settings::shortcuts::mc_shortcuts_view(window),
        _ => mc_general_view(
            chat,
            weak,
            font_popup,
            &font_dd,
            &font_filter,
            &font_filter_value,
            size_popup,
            &size_dd,
        ),
    };

    // 界面/其他页保持旧行为：body 滚动 + 大内边距；分栏页 body 不滚动、
    // pane 自身 h_full（顶条/底栏/侧栏滚动各自管理）
    let single_column =
        tab == TAB_GENERAL || tab == TAB_MISC || tab == TAB_ABOUT || tab == TAB_SHORTCUTS;
    let body = if single_column {
        let mut body_el = div()
            .id("mc-body")
            .flex_1()
            .min_w_0()
            .overflow_y_scroll()
            .pt(px(22.))
            .pl(px(30.))
            .pr(px(30.))
            .pb(px(30.));
        // 快捷键页：内容上下居中。居中必须做在 body 自身的 flex 布局里
        // （justify_center）——taffy block 布局不解析子元素百分比 min_h，
        // pane 里写 min_h_full 拿不到 body 高度，等于没居中
        if tab == TAB_SHORTCUTS {
            body_el = body_el.flex().flex_col().justify_center();
        }
        body_el.child(pane).into_any_element()
    } else {
        // 分栏页：列表与详情统一 15px 内边距（与内容缘等距），包装层不再
        // 另加 20（否则左侧多出 20 不对称）；分栏页全部全出血，顶条/底栏
        // 的横线与侧栏竖线直通卡缘（040 扩展页定稿样式推广到各分栏页）
        let hpad = if matches!(tab, TAB_PLUGINS | TAB_MODELS | TAB_SKILLS | TAB_MCP) {
            0.
        } else {
            20.
        };
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .pt(px(14.))
            .px(px(hpad))
            .pb(px(16.))
            .child(pane)
            .into_any_element()
    };

    // 浮层公共基座（遮挡不穿透 + 外点关闭 + ESC 关闭）；卡片自身
    // stop_propagation，dropdown 弹层开着时其外点监听在 capture 阶段先关
    // 弹层并拦下事件
    let weak_dismiss = weak_close.clone();
    let overlay = crate::ui::overlay::layer(true, Some(&chat.dlg.dialog_focus), move |_w, cx| {
        let _ = weak_dismiss.update(cx, |c, cx| {
            c.dlg.settings = None;
            cx.notify();
        });
    })
    .flex()
    .items_center()
    .justify_center()
    // 窗框走公共基座（设置弹窗是这套大卡片窗框的原始形态，系统提示词 /
    // 工具定义两个面板直接复用它）
    .child(crate::ui::overlay::big_card(
        // 设置弹窗没有标题（左导航即身份）
        "",
        Some(
            div()
                // §7 设置主导航列 160（2026-10-10 定夺恢复——P3 曾误按
                // sidebar_shell 的 240 改宽，两处是不同的面）
                .w(px(160.))
                .flex_shrink_0()
                .bg(rgb(t.nav))
                // 左导航贴弹窗左下角，同理自己倒左下角
                .rounded_bl(px(10.))
                .border_r_1()
                .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x66)))
                .flex()
                .flex_col()
                .p(px(8.))
                .pt(px(12.))
                .children(nav_items(tab, weak_close.clone()))
                .into_any_element(),
        ),
        body,
        t,
        {
            let weak_close = weak_close.clone();
            move |_w, cx| {
                let _ = weak_close.update(cx, |c, cx| {
                    c.dlg.settings = None;
                    cx.notify();
                });
            }
        },
    ));
    overlay.into_any_element()
}

fn two_pane(sidebar: gpui::AnyElement, detail: gpui::AnyElement) -> gpui::AnyElement {
    div()
        .flex()
        .w_full()
        .h_full()
        .min_h_0()
        .child(sidebar)
        .child(detail)
        .into_any_element()
}

/// 左导航（icon + label；激活 = bg_selected + 600 + icon accent）。
/// 原「微信 Bot」页签（TAB_REMOTE，历史名「远程控制」）已永久删除
///（2026-10-23 定夺）——内容迁左列「微信」面板（`remote_control::panel`），
/// 不留死分支；要恢复只能重写页面。
fn nav_items(tab: u8, weak_close: gpui::WeakEntity<Chat>) -> Vec<gpui::AnyElement> {
    let t = T();
    // tr() 传 &'static 键返回 &'static；显式标注避免数组被推断成临时生命周期
    let mut items: Vec<(u8, &'static str, &'static str)> = vec![
        (TAB_GENERAL, tr("settings.tab.appearance"), "monitor"),
        (TAB_MODELS, tr("settings.tab.models"), "cpu"),
        (TAB_SKILLS, tr("settings.tab.skills"), "layers"),
        (TAB_PLUGINS, tr("settings.tab.extensions"), "plug"),
        (TAB_AGENTS, tr("settings.tab.agents"), "user-round"),
        (TAB_MCP, tr("settings.tab.mcp"), "server"),
        (TAB_MISC, tr("settings.tab.misc"), "ellipsis"),
        (TAB_ABOUT, tr("settings.tab.about"), "info"),
        (TAB_SHORTCUTS, tr("settings.tab.shortcuts"), "keyboard"),
    ];
    // 子代理页随功能总开关隐藏（开发旗标默认关；渲染分支保留，重开=删掉
    // 本行过滤）
    if !crate::services::workspace::subagents_enabled() {
        items.retain(|(ix, _, _)| *ix != TAB_AGENTS);
    }
    items
        .iter()
        .map(|(ix, label, icon_name)| {
            let ix = *ix; // 值捕获：数组无 const promotion，闭包不能带栈引用
            let active = tab == ix;
            let weak_tab = weak_close.clone();
            div()
                .id(SharedString::from(format!("mc-nav-{ix}")))
                .flex()
                .items_center()
                .gap(px(9.))
                .px(px(10.))
                .py(px(7.5))
                .mb(px(1.))
                .rounded(px(8.))
                .text_size(crate::appearance::ui_size(13.))
                .text_color(if active {
                    rgb(t.text)
                } else {
                    rgb(t.text_muted)
                })
                .when(active, |d| {
                    d.bg(rgb(t.bg_selected))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                })
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_tab.update(cx, |c, cx| {
                        let next_section = SettingsPanel::prefill_section(c, ix);
                        let is_mcp = ix == TAB_MCP;
                        if let Some(st) = c.dlg.settings.clone() {
                            st.update(cx, |s, cx| {
                                s.tab = ix;
                                s.error = None;
                                // MCP 页 section 连带回填走 mcp_select（下同）
                                if !is_mcp {
                                    s.section = next_section.clone();
                                }
                                cx.notify();
                            });
                        }
                        if is_mcp {
                            c.mcp_select(next_section, cx);
                        }
                        // 模型页签：面板内切入同样确保 registry（静态目录保底）
                        // 在拉（幂等；open_settings 与 provider 弹窗同款挂点）
                        if ix == TAB_MODELS {
                            c.ensure_registry(cx);
                        }
                    });
                })
                .child(icon(
                    icon_name,
                    15.,
                    if active { t.accent } else { t.text_muted },
                ))
                .child(SharedString::from(*label))
                .into_any_element()
        })
        .collect()
}
