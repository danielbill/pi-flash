//! Settings panel：左导航页签（界面/模型/技能/扩展/MCP/其他）。
//! 内部页面按 pi-web 最新版重构（路径条/分组侧栏/详情表单/底栏）。表单
//! 状态由 SettingsPanel entity 持有；mc_*/mcp_* 动作仍在 Chat 上（单一
//! RPC 属主）。子代理页已移除——社区子代理扩展经「扩展」页安装即用，
//! 不再建模任何 subagent 配置。

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
    Btn, LIST_W, SwitchMark, config_button, config_switch, detail_shell, error_note,
    error_note_right, field, group_header, group_switch, mono_text, note, note_right, scope_tag,
    section_title, set_row, sidebar_list, sidebar_shell, status_dot, stepper, switch_base,
    width_slider,
};

/// 页签序：0 界面 · 1 模型 · 2 技能 · 3 扩展 · 4 MCP · 5 其他 · 8 快捷键。
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
    pub mj_ctx: gpui::Entity<TextInput>,
    /// 能力：图片输入（文本恒选）
    pub mj_image: bool,
    /// 推理（保存恒显式写 model.reasoning，防 ensure_model_defaults 补 true）
    pub mj_reasoning: bool,
    /// 最大输出 tokens
    pub mj_max_tokens: gpui::Entity<TextInput>,
    /// 价格四键（$/MTok，042-2）：手填显式覆盖；自动填参只填空/零
    pub mj_cost_in: gpui::Entity<TextInput>,
    pub mj_cost_out: gpui::Entity<TextInput>,
    pub mj_cost_cr: gpui::Entity<TextInput>,
    pub mj_cost_cw: gpui::Entity<TextInput>,
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
    /// 工具档位（inherit/chat-only/read-only/default/full/full+，044-0 六档）
    pub ag_tools: String,
    /// 最大轮次档
    pub ag_turns: pi_link::agents::MaxTurnsTier,
    pub ag_inject: bool,
    /// full+ 档选中的 pi-ext 路径（044-0 工具定制）
    pub ag_extensions: Vec<String>,
    /// full+ 档选中的 MCP 服务器名（044-0 工具定制）
    pub ag_mcp: Vec<String>,
    /// 选中的 skills（044-0 skills 定制；空 = 注入系统开启的）
    pub ag_skills: Vec<String>,
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
            mj_ctx: cx.new(|cx| TextInput::new(cx).numeric(true)),
            mj_image: false,
            mj_reasoning: false,
            mj_max_tokens: cx.new(|cx| TextInput::new(cx).numeric(true)),
            // 价格四框（042-2）不用 numeric()：它只放行 ASCII 数字、滤掉小数点
            //（"0.15"→"015"）。小数是常态，校验交给保存时的 parse::<f64>
            mj_cost_in: cx.new(|cx| TextInput::new(cx)),
            mj_cost_out: cx.new(|cx| TextInput::new(cx)),
            mj_cost_cr: cx.new(|cx| TextInput::new(cx)),
            mj_cost_cw: cx.new(|cx| TextInput::new(cx)),
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
            ag_extensions: Vec::new(),
            ag_mcp: Vec::new(),
            ag_skills: Vec::new(),
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
        let inputs: Vec<&gpui::Entity<TextInput>> = vec![
            &self.key_input,
            &self.model_filter,
            &self.enabled_filter,
            &self.mj_name,
            &self.mj_base,
            &self.mj_key,
            &self.mj_id,
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

// itlq.2 拆分：表单快照与整页渲染（SettingsFormData / render_settings /
// 两栏基座与左导航）迁至 form.rs，原路径经 re-export 保持。
pub(crate) mod form;
pub(crate) use form::{SettingsFormData, render_settings};
