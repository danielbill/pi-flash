//! Settings panel family (pi-web SettingsPanel + Models/Skills/Agents/
//! Plugins/ToolDefinitions Config components). The SettingsPanel entity owns
//! the form state (tab/section/inputs/error) with its own focus; the mc_*/
//! sa_* action methods stay on Chat (single RPC owner).

pub(crate) mod general;
pub(crate) mod models;
pub(crate) mod plugins;
pub(crate) mod skills;
pub(crate) mod subagents;
pub(crate) mod tools;

use general::mc_general_view;
use plugins::mc_plugins_view;
use skills::mc_skills_view;
use subagents::mc_subagents_view;
use tools::mc_tools_view;

use super::*;

/// The settings modal's form state (pi-web SettingsPanel own-state parity).
pub(crate) struct SettingsPanel {
    /// modal focus (escape + click-away target)
    pub focus: gpui::FocusHandle,
    /// 0 models · 1 skills · 2 plugins · 3 tools · 4 subagents
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
}

impl SettingsPanel {
    pub(crate) fn new(cx: &mut gpui::Context<Self>) -> Self {
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
}

impl SettingsFormData {
    pub(crate) fn snapshot(
        panel: &SettingsPanel,
        _cx: &gpui::App,
    ) -> Self {
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
        }
    }
}

pub(crate) fn render_settings(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    d: &SettingsFormData,
) -> gpui::AnyElement {
    let t = T();
    let SettingsFormData { focus: _focus, tab, section, key_input, key_visible, install_input, install_scope_project, sa_input, error } =
        d.clone();
    let weak_close = weak.clone();

    let (sidebar, detail) = if tab == 0 {
        crate::settings::models::mc_models_view(chat, weak, &section, &key_input, key_visible, &error)
    } else if tab == 1 {
        mc_skills_view(chat, weak, &section)
    } else if tab == 2 {
        mc_plugins_view(chat, weak, &section, &install_input, install_scope_project)
    } else if tab == 4 {
        mc_subagents_view(chat, weak, &section, &sa_input)
    } else if tab == 5 {
        mc_general_view(chat, weak)
    } else {
        mc_tools_view(chat, weak)
    };

    div()
        .absolute()
        .inset_0()
        .bg(gpui::hsla(0., 0., 0., 0.35))
        .track_focus(&chat.dialog_focus)
        .on_key_down({
            let weak = weak_close.clone();
            move |ev: &KeyDownEvent, _w, cx| {
                if ev.keystroke.key == "escape" {
                    let _ = weak.update(cx, |this, cx| {
                        this.settings = None;
                        cx.notify();
                    });
                }
            }
        })
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(1080.))
                .max_h(px(700.))
                .bg(rgb(t.bg))
                .border_1()
                .border_color(rgb(t.border))
                .rounded(px(8.))
                .shadow_lg()
                .flex()
                .flex_col()
                .overflow_hidden()
                // settings tab strip (pi-web SettingsPanel: 96px tabs, 24x2 accent underline)
                .child(
                    div()
                        .h(px(50.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .border_b_1()
                        .border_color(rgb(t.border))
                        .children([tr("模型"), tr("技能"), tr("插件"), tr("工具"), tr("子代理"), tr("通用")].iter().enumerate().map(|(i, label)| {
                            let active = tab as usize == i;
                            let weak_tab = weak_close.clone();
                            div()
                                .id(SharedString::from(format!("mc-tab-{i}")))
                                .w(px(96.))
                                .h_full()
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .gap(px(3.))
                                .text_size(px(12.))
                                .cursor_pointer()
                                .text_color(if active { rgb(t.text) } else { rgb(t.text_muted) })
                                .hover(|s| s.bg(rgb(t.bg_hover)))
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    let _ = weak_tab.update(cx, |c, cx| {
                                        let next_section = match i {
                                            0 => c.mc_provider_ids().first().cloned().unwrap_or_default(),
                                            1 => c.mc_skills.first().map(|s| s.path.to_string_lossy().to_string()).unwrap_or_default(),
                                            2 => c.mc_pkgs_global.first()
                                                .or_else(|| c.mc_pkgs_project.first())
                                                .map(pi_link::skills::entry_source)
                                                .unwrap_or_else(|| "__add__".into()),
                                            4 => c.sa_profiles.first().map(|p| p.name.clone()).unwrap_or_default(),
                                            _ => String::new(),
                                        };
                                        if let Some(st) = c.settings.clone() {
                                            st.update(cx, |s, cx| {
                                                s.tab = i as u8;
                                                s.section = next_section;
                                                s.error = None;
                                                cx.notify();
                                            });
                                        }
                                    });
                                })
                                .child(SharedString::from((*label).to_string()))
                                .child(if active {
                                    div().w(px(24.)).h(px(2.)).bg(rgb(t.accent))
                                } else {
                                    div().h(px(2.))
                                })
                                .into_any_element()
                        }))
                        .child(div().flex_1())
                        .child(
                            div()
                                .id("mc-close")
                                .mr(px(14.))
                                .size(px(30.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(5.))
                                .text_size(px(14.))
                                .text_color(rgb(t.text_muted))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(t.bg_hover)))
                                .on_mouse_down(MouseButton::Left, {
                                    let weak = weak_close.clone();
                                    move |_, _, cx| {
                                        let _ = weak.update(cx, |c, cx| {
                                            c.settings = None;
                                            cx.notify();
                                        });
                                    }
                                })
                                .child(icon("x", 14., t.text_muted)),
                        ),
                )
                // split view
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .child(sidebar)
                        .child(detail),
                ),
        )
        .into_any_element()
}
