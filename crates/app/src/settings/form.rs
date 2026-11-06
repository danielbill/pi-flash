//! 设置表单快照与整页渲染：SettingsFormData（无 cx 读取的纯数据帧）+
//! render_settings 六页分派 + 两栏基座与左导航。

use super::*;

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
    pub mj_ctx: gpui::Entity<TextInput>,
    pub mj_image: bool,
    pub mj_reasoning: bool,
    pub mj_max_tokens: gpui::Entity<TextInput>,
    pub mj_cost_in: gpui::Entity<TextInput>,
    pub mj_cost_out: gpui::Entity<TextInput>,
    pub mj_cost_cr: gpui::Entity<TextInput>,
    pub mj_cost_cw: gpui::Entity<TextInput>,
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
    pub ag_extensions: Vec<String>,
    pub ag_mcp: Vec<String>,
    pub ag_skills: Vec<String>,
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
            mj_ctx: p.mj_ctx.clone(),
            mj_image: p.mj_image,
            mj_reasoning: p.mj_reasoning,
            mj_max_tokens: p.mj_max_tokens.clone(),
            mj_cost_in: p.mj_cost_in.clone(),
            mj_cost_out: p.mj_cost_out.clone(),
            mj_cost_cr: p.mj_cost_cr.clone(),
            mj_cost_cw: p.mj_cost_cw.clone(),
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
            ag_extensions: p.ag_extensions.clone(),
            ag_mcp: p.ag_mcp.clone(),
            ag_skills: p.ag_skills.clone(),
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
    let __pane_t = std::time::Instant::now();
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
    crate::perf_span("frame.settings.pane", __pane_t);

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
fn nav_items(tab: u8, weak_close: gpui::WeakEntity<Chat>) -> Vec<gpui::AnyElement> {
    let t = T();
    // tr() 传 &'static 键返回 &'static；显式标注避免数组被推断成临时生命周期
    let mut items: Vec<(u8, &'static str, &'static str)> = vec![
        (TAB_GENERAL, tr("settings.tab.appearance"), "monitor"),
        (TAB_MODELS, tr("settings.tab.models"), "cpu"),
        (TAB_SKILLS, tr("settings.tab.skills"), "layers"),
        (TAB_PLUGINS, tr("settings.tab.extensions"), "plug"),
        (TAB_AGENTS, tr("settings.tab.agents"), "bot"),
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
