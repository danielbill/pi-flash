//! Settings panel (v54 设计版): 70%×98% 居中弹窗，自带 36px topbar；左导航
//! 200px 六页签带图标（界面/模型/技能/子代理/插件/其他）。表单状态由
//! SettingsPanel entity 持有；mc_*/sa_* 动作仍在 Chat 上（单一 RPC 属主）。

pub(crate) mod general;
pub(crate) mod misc;
pub(crate) mod models;
pub(crate) mod plugins;
pub(crate) mod skills;
pub(crate) mod subagents;

use general::mc_general_view;
use misc::mc_misc_view;
use plugins::mc_plugins_view;
use skills::mc_skills_view;
use subagents::mc_subagents_view;

use super::*;
pub(crate) use crate::ui::{icon, icon_hover, DropdownState};

/// The settings modal's form state (pi-web SettingsPanel own-state parity).
pub(crate) struct SettingsPanel {
    /// modal focus (escape + click-away target)
    pub focus: gpui::FocusHandle,
    /// 0 界面 · 1 模型 · 2 技能 · 3 子代理 · 4 插件 · 5 其他
    pub tab: u8,
    /// selected entry in the tab's sidebar (provider id / skill path /
    /// package source / "__add__" for the install form)
    pub section: String,
    pub key_input: gpui::Entity<TextInput>,
    pub key_visible: bool,
    pub install_input: gpui::Entity<TextInput>,
    pub install_scope_project: bool,
    /// subagents tab: maxConcurrent input value
    pub sa_input: gpui::Entity<TextInput>,
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
            install_input: cx.new(|cx| TextInput::new(cx).placeholder(tr("来源"))),
            install_scope_project: false,
            sa_input: cx.new(|cx| TextInput::new(cx).numeric(true)),
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
    pub install_input: gpui::Entity<TextInput>,
    pub install_scope_project: bool,
    pub sa_input: gpui::Entity<TextInput>,
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
            install_input: p.install_input.clone(),
            install_scope_project: p.install_scope_project,
            sa_input: p.sa_input.clone(),
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
    let SettingsFormData { focus: _focus, tab, section, key_input, key_visible, install_input, install_scope_project, sa_input, error, font_popup, font_dd, font_filter, font_filter_value, size_popup, size_dd } =
        d.clone();
    let weak_close = weak.clone();

    // 页内容：界面=单列 rows；其余=自带左右分栏
    let pane: gpui::AnyElement = match tab {
        0 => mc_general_view(chat, weak, font_popup, &font_dd, &font_filter, &font_filter_value, size_popup, &size_dd),
        1 => {
            let (sidebar, detail) =
                crate::settings::models::mc_models_view(chat, weak, &section, &key_input, key_visible, &error);
            two_pane(sidebar, detail)
        }
        2 => {
            let (sidebar, detail) = mc_skills_view(chat, weak, &section);
            two_pane(sidebar, detail)
        }
        3 => {
            let (sidebar, detail) = mc_subagents_view(chat, weak, &section, &sa_input);
            two_pane(sidebar, detail)
        }
        4 => {
            let (sidebar, detail) = mc_plugins_view(chat, weak, &section, &install_input, install_scope_project);
            two_pane(sidebar, detail)
        }
        _ => mc_misc_view(chat, weak),
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
                .into_any_element(),
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

/// 左导航六项（icon + label；激活 = bg_selected + 600 + icon accent）。
fn nav_items(tab: u8, weak_close: gpui::WeakEntity<Chat>) -> Vec<gpui::AnyElement> {
    let t = T();
    [
        (0u8, "界面", "monitor"),
        (1, "模型", "layers"),
        (2, "技能", "wand"),
        (3, "子代理", "bot"),
        (4, "插件", "plug"),
        (5, "其他", "ellipsis-v"),
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
                    let next_section = match ix {
                        1 => c.mc_provider_ids().first().cloned().unwrap_or_default(),
                        2 => c.mc_skills.first().map(|s| s.path.to_string_lossy().to_string()).unwrap_or_default(),
                        4 => c.mc_pkgs_global.first()
                            .or_else(|| c.mc_pkgs_project.first())
                            .map(pi_link::skills::entry_source)
                            .unwrap_or_else(|| "__add__".into()),
                        3 => c.sa_profiles.first().map(|p| p.name.clone()).unwrap_or_default(),
                        _ => String::new(),
                    };
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
