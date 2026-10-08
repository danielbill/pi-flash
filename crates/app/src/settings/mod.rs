//! Settings panel (v70)：保持左导航 200px 七页签（界面/模型/技能/子代理/
//! 扩展/MCP/其他）不变，内部页面按 pi-web 最新版重构（路径条/分组侧栏/
//! 详情表单/底栏）。表单状态由 SettingsPanel entity 持有；mc_*/sa_*/mcp_*
//! 动作仍在 Chat 上（单一 RPC 属主）。

pub(crate) mod custom_models;
pub(crate) mod general;
pub(crate) mod mcp;
pub(crate) mod misc;
pub(crate) mod models;
pub(crate) mod plugins;
pub(crate) mod remote;
pub(crate) mod skills;
pub(crate) mod subagents;
pub(crate) mod widgets;

use general::mc_general_view;
use misc::mc_misc_view;

use super::*;
pub(crate) use crate::ui::{VListHeight, DropdownState, icon, vlist};
pub(crate) use widgets::{
    check_chip, config_button, config_switch, detail_shell, error_note, field, footer, grid_row,
    group_header, group_switch, list_action, mono_text, note, scope_tag, section_title,
    sidebar_list, sidebar_shell, status_dot, Btn, GREEN, WARN,
};

/// 页签序：0 界面 · 1 模型 · 2 技能 · 3 子代理 · 4 扩展 · 5 MCP · 6 其他。
pub(crate) const TAB_GENERAL: u8 = 0;
pub(crate) const TAB_MODELS: u8 = 1;
pub(crate) const TAB_SKILLS: u8 = 2;
pub(crate) const TAB_AGENTS: u8 = 3;
pub(crate) const TAB_PLUGINS: u8 = 4;
pub(crate) const TAB_MCP: u8 = 5;
pub(crate) const TAB_MISC: u8 = 6;
/// 8 页签：0 界面 · 1 模型 · 2 技能 · 3 子代理 · 4 扩展 · 5 MCP · 6 其他 · 7 远程控制
pub(crate) const TAB_REMOTE: u8 = 7;

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
    pub mj_name: gpui::Entity<TextInput>,
    pub mj_base: gpui::Entity<TextInput>,
    pub mj_key: gpui::Entity<TextInput>,
    pub mj_id: gpui::Entity<TextInput>,
    pub mj_mname: gpui::Entity<TextInput>,
    pub mj_ctx: gpui::Entity<TextInput>,
    pub mj_api: u8,
    pub mj_reasoning: bool,
    // -- 扩展页安装表单（040：整条 pi install 命令可直接粘贴）
    pub install_input: gpui::Entity<TextInput>,
    // -- 子代理页：maxConcurrent 输入 + 新建表单（scope 切换）
    pub sa_input: gpui::Entity<TextInput>,
    pub sa_new: bool,
    pub sa_scope_project: bool,
    pub sa_name: gpui::Entity<TextInput>,
    pub sa_display: gpui::Entity<TextInput>,
    pub sa_desc: gpui::Entity<TextInput>,
    pub sa_prompt: gpui::Entity<TextInput>,
    pub sa_model: gpui::Entity<TextInput>,
    pub sa_turns: gpui::Entity<TextInput>,
    // -- MCP 页添加表单
    pub mcp_add: gpui::Entity<TextInput>,
    pub mcp_name: gpui::Entity<TextInput>,
    pub mcp_scope_project: bool,
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
                .placeholder(tr("搜索字体…"))
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
            key_input: cx.new(|cx| {
                TextInput::new(cx)
                    .masked(true)
                    .placeholder(tr("ENV 变量、!命令 或明文 key"))
            }),
            key_visible: false,
            model_filter: panel_live_input("筛选模型…", cx),
            mj_name: panel_input("provider-name", cx),
            mj_base: panel_input("https://api.example.com/v1", cx),
            mj_key: cx.new(|cx| {
                TextInput::new(cx)
                    .masked(true)
                    .placeholder(tr("ENV_VAR_NAME、!shell-command 或明文 key"))
            }),
            mj_id: panel_input("model-id", cx),
            mj_mname: panel_input("显示名（可选）", cx),
            mj_ctx: cx.new(|cx| TextInput::new(cx).numeric(true)),
            mj_api: 0,
            mj_reasoning: false,
            install_input: cx.new(|cx| {
                TextInput::new(cx).placeholder(tr("例：pi install npm:pi-web-access"))
            }),
            sa_input: cx.new(|cx| TextInput::new(cx).numeric(true)),
            sa_new: false,
            sa_scope_project: false,
            sa_name: panel_input("子代理 ID", cx),
            sa_display: panel_input("显示名称", cx),
            sa_desc: panel_input("描述", cx),
            sa_prompt: panel_input("系统指令（留空继承）", cx),
            sa_model: panel_input("provider/model（留空跟随父会话）", cx),
            sa_turns: cx.new(|cx| TextInput::new(cx).numeric(true)),
            mcp_add: panel_live_input("JSON、http(s) URL、命令行，或 `pi mcp add …`", cx),
            mcp_name: panel_live_input("服务器名称（字母数字_-）", cx),
            mcp_scope_project: false,
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

    /// 添加 Provider 表单的字段快照（创建动作读取）。
    pub(crate) fn editor_snapshot(&self, cx: &gpui::App) -> (String, String, String, u8) {
        (
            self.mj_name.read(cx).value().to_string(),
            self.mj_base.read(cx).value().to_string(),
            self.mj_key.read(cx).value().to_string(),
            self.mj_api,
        )
    }

    /// 焦点是否落在面板（含其全部输入框）内 —— Chat 焦点策略链用它判断
    /// 要不要把焦点拽回 dialog_focus。**新增输入框自动覆盖**，不再维护
    /// 白名单（v60 字体筛选、v70 模型/MCP 各输入框先后被"每帧抢焦点"
    /// 咬过的教训：白名单必漏）。
    pub(crate) fn focus_within(&self, window: &gpui::Window, cx: &gpui::App) -> bool {
        let inputs = [
            &self.key_input,
            &self.model_filter,
            &self.mj_name,
            &self.mj_base,
            &self.mj_key,
            &self.mj_id,
            &self.mj_mname,
            &self.mj_ctx,
            &self.install_input,
            &self.sa_input,
            &self.sa_name,
            &self.sa_display,
            &self.sa_desc,
            &self.sa_prompt,
            &self.sa_model,
            &self.sa_turns,
            &self.mcp_add,
            &self.mcp_name,
            &self.font_filter,
        ];
        self.focus.is_focused(window)
            || inputs.iter().any(|e| e.read(cx).focus_handle_in(cx).is_focused(window))
    }

    /// 切页签时的 section 预填（open_settings 与左导航点击共用）。
    pub(crate) fn prefill_section(chat: &Chat, tab: u8) -> String {        match tab {
            TAB_MODELS => chat.mc_provider_ids().first().cloned().unwrap_or_default(),
            TAB_SKILLS => chat
                .mc_skills
                .first()
                .map(|s| s.path.to_string_lossy().to_string())
                .unwrap_or_default(),
            TAB_AGENTS => chat
                .sa_profiles
                .first()
                .map(|p| p.name.clone())
                .unwrap_or_default(),
            TAB_PLUGINS => chat
                .mc_pkgs_global
                .first()
                .map(pi_link::skills::entry_source)
                .unwrap_or_default(),
            TAB_MCP => chat
                .mcp_servers
                .first()
                .map(|s| mcp::section_key(s))
                .unwrap_or_else(|| "__mcp_add__".into()),
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
    pub model_filter_value: String,
    pub mj_name: gpui::Entity<TextInput>,
    pub mj_base: gpui::Entity<TextInput>,
    pub mj_key: gpui::Entity<TextInput>,
    pub mj_id: gpui::Entity<TextInput>,
    pub mj_mname: gpui::Entity<TextInput>,
    pub mj_ctx: gpui::Entity<TextInput>,
    pub mj_api: u8,
    pub mj_reasoning: bool,
    pub install_input: gpui::Entity<TextInput>,
    pub sa_input: gpui::Entity<TextInput>,
    pub sa_new: bool,
    pub sa_scope_project: bool,
    pub sa_name: gpui::Entity<TextInput>,
    pub sa_display: gpui::Entity<TextInput>,
    pub sa_desc: gpui::Entity<TextInput>,
    pub sa_prompt: gpui::Entity<TextInput>,
    pub sa_model: gpui::Entity<TextInput>,
    pub sa_turns: gpui::Entity<TextInput>,
    pub mcp_add: gpui::Entity<TextInput>,
    pub mcp_name: gpui::Entity<TextInput>,
    pub mcp_add_value: String,
    pub mcp_name_value: String,
    pub mcp_scope_project: bool,
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
            mj_name: p.mj_name.clone(),
            mj_base: p.mj_base.clone(),
            mj_key: p.mj_key.clone(),
            mj_id: p.mj_id.clone(),
            mj_mname: p.mj_mname.clone(),
            mj_ctx: p.mj_ctx.clone(),
            mj_api: p.mj_api,
            mj_reasoning: p.mj_reasoning,
            install_input: p.install_input.clone(),
            sa_input: p.sa_input.clone(),
            sa_new: p.sa_new,
            sa_scope_project: p.sa_scope_project,
            sa_name: p.sa_name.clone(),
            sa_display: p.sa_display.clone(),
            sa_desc: p.sa_desc.clone(),
            sa_prompt: p.sa_prompt.clone(),
            sa_model: p.sa_model.clone(),
            sa_turns: p.sa_turns.clone(),
            mcp_add: p.mcp_add.clone(),
            mcp_name: p.mcp_name.clone(),
            mcp_add_value: p.mcp_add.read(cx).value().to_string(),
            mcp_name_value: p.mcp_name.read(cx).value().to_string(),
            mcp_scope_project: p.mcp_scope_project,
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
) -> gpui::AnyElement {
    let t = T();
    let SettingsFormData { focus: _focus, tab, section, key_input, key_visible, model_filter, model_filter_value, mj_name, mj_base, mj_key, mj_id, mj_mname, mj_ctx, mj_api, mj_reasoning, install_input, sa_input, sa_new, sa_scope_project, sa_name, sa_display, sa_desc, sa_prompt, sa_model, sa_turns, mcp_add, mcp_add_value, mcp_name, mcp_name_value, mcp_scope_project, error, font_popup, font_dd, font_filter, font_filter_value, size_popup, size_dd } =
        d.clone();
    let weak_close = weak.clone();

    // 页内容：界面/其他=单列 rows（滚动在 body）；其余页自带左右分栏 +
    // 可选顶条/底栏（自身 h_full，body 不滚动）
    let pane: gpui::AnyElement = match tab {
        TAB_MODELS => crate::settings::models::mc_models_view(
            chat, weak, &section, &key_input, key_visible, &model_filter, &model_filter_value,
            &mj_name, &mj_base, &mj_key, &mj_id, &mj_mname, &mj_ctx,
            mj_api, mj_reasoning, &error,
        ),
        TAB_SKILLS => {
            let (sidebar, detail) = crate::settings::skills::mc_skills_view(chat, weak, &section);
            two_pane(sidebar, detail)
        }
        TAB_AGENTS => crate::settings::subagents::mc_subagents_view(
            chat, weak, &section, &sa_input, sa_new, sa_scope_project,
            &sa_name, &sa_display, &sa_desc, &sa_prompt, &sa_model, &sa_turns,
        ),
        TAB_PLUGINS => crate::settings::plugins::mc_plugins_view(
            chat, weak, &section, &install_input, &error,
        ),
        TAB_MCP => crate::settings::mcp::mc_mcp_view(
            chat, weak, &section, &mcp_add, &mcp_add_value, &mcp_name, &mcp_name_value,
            mcp_scope_project, &error,
        ),
        TAB_MISC => mc_misc_view(chat, weak),
        TAB_REMOTE => crate::settings::remote::mc_remote_view(chat, weak),
        _ => mc_general_view(
            chat, weak, font_popup, &font_dd, &font_filter, &font_filter_value,
            size_popup, &size_dd,
        ),
    };

    // 界面/其他页保持旧行为：body 滚动 + 大内边距；分栏页 body 不滚动、
    // pane 自身 h_full（顶条/底栏/侧栏滚动各自管理）
    let single_column = tab == TAB_GENERAL || tab == TAB_MISC;
    let body = if single_column {
        div()
            .id("mc-body")
            .flex_1()
            .min_w_0()
            .overflow_y_scroll()
            .pt(px(22.))
            .pl(px(30.))
            .pr(px(30.))
            .pb(px(30.))
            .child(pane)
            .into_any_element()
    } else {
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .pt(px(14.))
            .pl(px(20.))
            .pr(px(20.))
            .pb(px(16.))
            .child(pane)
            .into_any_element()
    };

    // 浮层公共基座（遮挡不穿透 + 外点关闭 + ESC 关闭）；卡片自身
    // stop_propagation，dropdown 弹层开着时其外点监听在 capture 阶段先关
    // 弹层并拦下事件
    let weak_dismiss = weak_close.clone();
    let overlay = crate::ui::overlay::layer(
        true,
        Some(&chat.dialog_focus),
        move |_w, cx| {
            let _ = weak_dismiss.update(cx, |c, cx| {
                c.settings = None;
                cx.notify();
            });
        },
    )
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
                    .w(px(200.))
                    .flex_shrink_0()
                    .bg(rgb(t.nav))
                    // 左导航贴弹窗左下角，同理自己倒左下角
                    .rounded_bl(px(10.))
                    .border_r_1()
                    .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x66)))
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
                        c.settings = None;
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

/// 左导航七项（icon + label；激活 = bg_selected + 600 + icon accent）。
fn nav_items(tab: u8, weak_close: gpui::WeakEntity<Chat>) -> Vec<gpui::AnyElement> {
    let t = T();
    [
        (TAB_GENERAL, "界面", "monitor"),
        (TAB_MODELS, "模型", "cpu"),
        (TAB_SKILLS, "技能", "layers"),
        (TAB_AGENTS, "子代理", "bot"),
        (TAB_PLUGINS, "扩展", "plug"),
        (TAB_MCP, "MCP", "server"),
        (TAB_MISC, "其他", "ellipsis-v"),
        (TAB_REMOTE, "远程控制", "message-square"),
    ]
    .iter()
    .map(|(ix, label, icon_name)| {
        let active = tab == *ix;
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
            .text_color(if active { rgb(t.text) } else { rgb(t.text_muted) })
            .when(active, |d| {
                d.bg(rgb(t.bg_selected)).font_weight(gpui::FontWeight::SEMIBOLD)
            })
            .cursor_pointer()
            .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = weak_tab.update(cx, |c, cx| {
                    let next_section = SettingsPanel::prefill_section(c, *ix);
                    if let Some(st) = c.settings.clone() {
                        st.update(cx, |s, cx| {
                            s.tab = *ix;
                            s.section = next_section;
                            s.error = None;
                            cx.notify();
                        });
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
