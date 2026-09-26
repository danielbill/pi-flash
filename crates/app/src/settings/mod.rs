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
        cx: &gpui::App,
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
    // provider groups in available-models order
    let provider_ids = chat.mc_provider_ids();
    let selected = if section.is_empty() {
        provider_ids.first().cloned().unwrap_or_default()
    } else {
        section.clone()
    };

    // ---- sidebar ---------------------------------------------------------
    let sidebar = div()
        .id("mc-sidebar")
        .w(px(240.))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(t.bg_panel))
        .border_r_1()
        .border_color(rgb(t.border))
        .p(px(6.))
        .pt(px(8.))
        .overflow_y_scroll()
        .children(provider_ids.iter().map(|p| {
            let active = *p == selected;
            let models: Vec<&pi_link::protocol::ModelInfo> = chat
                .available_models
                .iter()
                .filter(|m| &m.provider == p)
                .collect();
            let total = models.len();
            let enabled = models
                .iter()
                .filter(|m| {
                    let r = format!("{}/{}", m.provider, m.id);
                    chat.mc_state.enabled.iter().any(|e| e == &r)
                })
                .count();
            let configured = chat.mc_configured(p);
            let weak_item = weak.clone();
            let pid = p.clone();
            div()
                .id(SharedString::from(format!("mc-side-{p}")))
                .h(px(30.))
                .px(px(8.))
                .rounded(px(5.))
                .flex()
                .items_center()
                .gap_2()
                .text_size(px(12.))
                .cursor_pointer()
                .bg(if active { rgb(t.bg_selected) } else { rgb(t.bg_panel) })
                .font_weight(if active {
                    gpui::FontWeight::SEMIBOLD
                } else {
                    gpui::FontWeight::NORMAL
                })
                .text_color(if active { rgb(t.text) } else { rgb(t.text_muted) })
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_item.update(cx, |c, cx| {
                        if let Some(st) = c.settings.clone() {
                            st.update(cx, |s, cx| {
                                s.section = pid.clone();
                                s.error = None;
                                cx.notify();
                            });
                        }
                    });
                })
                .child(
                    div()
                        .size(px(6.))
                        .rounded_full()
                        .flex_shrink_0()
                        .bg(if configured { rgb(0x4ade80) } else { rgb(t.border) }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(SharedString::from(p.clone())),
                )
                .child(if enabled < total {
                    div()
                        .font_family("Consolas")
                        .text_size(px(10.))
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(format!("{enabled}/{total}")))
                        .into_any_element()
                } else {
                    div().into_any_element()
                })
                .into_any_element()
        }));

    // ---- detail pane -----------------------------------------------------
    let models: Vec<pi_link::protocol::ModelInfo> = chat
        .available_models
        .iter()
        .filter(|m| m.provider == selected)
        .cloned()
        .collect();
    let prov_refs: Vec<String> = models
        .iter()
        .map(|m| format!("{}/{}", m.provider, m.id))
        .collect();
    let enabled_count = prov_refs
        .iter()
        .filter(|r| chat.mc_state.enabled.contains(r))
        .count();
    let configured = chat.mc_configured(&selected);
    let oauth = chat.mc_oauth(&selected);

    let detail = div()
        .id("mc-detail")
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_y_scroll()
        .p(px(20.))
        .text_size(px(12.))
        .flex()
        .flex_col()
        .gap_4();

    // provider header: name + status
    let (status_text, status_color) = if oauth {
        (tr("OAuth 已登录"), 0x4ade80)
    } else if configured {
        (tr("API Key 已配置"), 0x4ade80)
    } else {
        (tr("未配置"), t.text_dim)
    };
    let detail = detail.child(
        div()
            .flex()
            .items_center()
            .gap_2()
            .min_h(px(28.))
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text))
                    .child(SharedString::from(selected.clone())),
            )
            .child(div().size(px(7.)).rounded_full().bg(rgb(status_color)))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(status_text.to_string())),
            ),
    );

    // error box
    let detail = if let Some(err) = &error {
        detail.child(
            div()
                .py(px(7.))
                .px(px(9.))
                .rounded(px(5.))
                .border_1()
                .border_color(rgb(0xef4444))
                .text_size(px(11.))
                .text_color(rgb(0xef4444))
                .child(SharedString::from(err.clone())),
        )
    } else {
        detail
    };

    let mut detail = detail;

    // ---- credential section ---------------------------------------------
    if oauth {
        let weak_logout = weak.clone();
        let logout_provider = selected.clone();
        detail = detail.child(
            div()
                .flex()
                .flex_col()
                .gap(px(5.))
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(rgb(t.text_muted))
                        .child(tr("凭据")),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(t.text_dim))
                        .child(tr("登录凭据存储于 ~/.pi/agent/auth.json（与 pi 共用）")),
                )
                .child(
                    div()
                        .id("mc-logout")
                        .h(px(28.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .rounded(px(5.))
                        .border_1()
                        .border_color(rgb(0xef4444))
                        .bg(gpui::hsla(0., 0.84, 0.6, 0.06))
                        .text_size(px(11.))
                        .text_color(rgb(0xef4444))
                        .cursor_pointer()
                        .hover(|s| s.bg(gpui::hsla(0., 0.84, 0.6, 0.12)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak_logout.update(cx, |c, cx| {
                                c.mc_logout(logout_provider.clone(), cx)
                            });
                        })
                        .child(tr("退出登录")),
                ),
        );
    } else {
        let weak_save = weak.clone();
        let weak_del = weak.clone();
        let save_provider = selected.clone();
        let del_provider = selected.clone();
        detail = detail.child(
            div()
                .flex()
                .flex_col()
                .gap(px(5.))
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(rgb(t.text_muted))
                        .child("API Key"),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(key_input.clone()),
                        )
                        .child(
                            div()
                                .id("mc-key-eye")
                                .w(px(30.))
                                .h(px(30.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(5.))
                                .border_1()
                                .border_color(rgb(t.border))
                                .text_size(px(11.))
                                .text_color(rgb(t.text_muted))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(t.bg_hover)))
                                .on_mouse_down(MouseButton::Left, {
                                    let weak_eye = weak.clone();
                                    move |_, _, cx| {
                                        let _ = weak_eye.update(cx, |c, cx| {
                                            if let Some(st) = c.settings.clone() {
                                                let input = st.read(cx).key_input.clone();
                                                let visible = st.update(cx, |s, cx| {
                                                    s.key_visible = !s.key_visible;
                                                    cx.notify();
                                                    s.key_visible
                                                });
                                                input.update(cx, |ti, cx| ti.set_masked(!visible, cx));
                                            }
                                        });
                                    }
                                })
                                .child(if key_visible { tr("隐藏") } else { tr("显示") }),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            div()
                                .id("mc-key-save")
                                .h(px(28.))
                                .px(px(10.))
                                .flex()
                                .items_center()
                                .rounded(px(5.))
                                .border_1()
                                .border_color(rgb(t.accent))
                                .bg(rgb(t.accent))
                                .text_size(px(11.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgb(t.accent_contrast))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(t.accent_hover)))
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    let _ = weak_save.update(cx, |c, cx| {
                                        let key = c
                                            .settings
                                            .as_ref()
                                            .map(|st| st.read(cx).key_input.clone())
                                            .and_then(|input| {
                                                Some(input.read(cx).value().to_string())
                                            })
                                            .unwrap_or_default();
                                        c.mc_save_key(save_provider.clone(), key, cx);
                                    });
                                })
                                .child(if configured { tr("更新") } else { tr("保存") }),
                        )
                        .child(if configured {
                            div()
                                .id("mc-key-del")
                                .h(px(28.))
                                .px(px(10.))
                                .flex()
                                .items_center()
                                .rounded(px(5.))
                                .border_1()
                                .border_color(rgb(0xef4444))
                                .bg(gpui::hsla(0., 0.84, 0.6, 0.06))
                                .text_size(px(11.))
                                .text_color(rgb(0xef4444))
                                .cursor_pointer()
                                .hover(|s| s.bg(gpui::hsla(0., 0.84, 0.6, 0.12)))
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    let _ = weak_del.update(cx, |c, cx| {
                                        c.mc_delete_key(del_provider.clone(), cx)
                                    });
                                })
                                .child(tr("删除"))
                                .into_any_element()
                        } else {
                            div().into_any_element()
                        }),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(t.text_dim))
                        .child(tr("密钥写入 ~/.pi/agent/auth.json（与 pi 共用）；新 provider 的模型需重启 pi-flash 后出现在列表")),
                ),
        );
    }

    // ---- enabled models section ------------------------------------------
    if !models.is_empty() {
        let shown: Vec<&pi_link::protocol::ModelInfo> = models.iter().collect();
        let weak_bulk_on = weak.clone();
        let weak_bulk_off = weak.clone();
        let bulk_provider = selected.clone();

        let mut section_col = div().flex().flex_col().gap(px(8.)).pt(px(10.)).child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(t.text))
                        .child(tr("已启用模型")),
                )
                .child(
                    div()
                        .flex_grow()
                        .font_family("Consolas")
                        .text_size(px(10.))
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(format!(
                            "{}/{}",
                            enabled_count,
                            models.len()
                        ))),
                )
                .child(
                    div()
                        .id("mc-bulk-on")
                        .h(px(28.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .rounded(px(5.))
                        .border_1()
                        .border_color(rgb(t.border))
                        .text_size(px(11.))
                        .text_color(rgb(t.text_muted))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                        .on_mouse_down(MouseButton::Left, {
                            let bp = bulk_provider.clone();
                            move |_, _, cx| {
                                let _ = weak_bulk_on.update(cx, |c, cx| {
                                    c.mc_toggle_provider(&bp, true, cx)
                                });
                            }
                        })
                        .child(tr("全部启用")),
                )
                .child(
                    div()
                        .id("mc-bulk-off")
                        .h(px(28.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .rounded(px(5.))
                        .border_1()
                        .border_color(rgb(t.border))
                        .text_size(px(11.))
                        .text_color(rgb(t.text_muted))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                        .on_mouse_down(MouseButton::Left, {
                            let bp = bulk_provider.clone();
                            move |_, _, cx| {
                                let _ = weak_bulk_off.update(cx, |c, cx| {
                                    c.mc_toggle_provider(&bp, false, cx)
                                });
                            }
                        })
                        .child(tr("全部停用")),
                ),
        );
        if chat.mc_project_scope {
            section_col = section_col.child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(t.text_dim))
                    .child(tr("项目级 settings.json 覆盖了 enabledModels，此面板只读")),
            );
        }
        // rows
        let weak_rows = weak.clone();
        let enabled_now: Vec<String> = chat.mc_state.enabled.clone();
        let pins: Vec<(String, String)> = chat.mc_state.pins.clone();
        let all_enabled = chat.mc_state.all_enabled;
        let last_one = enabled_count == 1;
        let mut list = div()
            .id("mc-model-list")
            .max_h(px(360.))
            .rounded(px(6.))
            .border_1()
            .border_color(rgb(t.border))
            .bg(rgb(t.bg_panel))
            .overflow_y_scroll();
        if shown.is_empty() {
            list = list.child(
                div()
                    .p(px(12.))
                    .text_size(px(11.))
                    .text_color(rgb(t.text_dim))
                    .child(tr("没有匹配的模型")),
            );
        }
        for (ix, m) in shown.iter().enumerate() {
            let r = format!("{}/{}", m.provider, m.id);
            let is_enabled = all_enabled || enabled_now.iter().any(|e| e == &r);
            let pin = pins.iter().find(|(p, _)| p == &r).map(|(_, l)| l.clone());
            let row_last = last_one && is_enabled;
            let weak_row = weak_rows.clone();
            let ref_str = r.clone();
            let ref_click = r.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("mc-row-{ix}")))
                    .min_h(px(36.))
                    .py(px(6.))
                    .px(px(9.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(ix > 0, |d| d.border_t_1().border_color(rgb(t.border)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(t.text))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(SharedString::from(m.name.clone())),
                            )
                            .child(
                                div()
                                    .font_family("Consolas")
                                    .text_size(px(10.))
                                    .text_color(rgb(t.text_dim))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .on_mouse_down(MouseButton::Left, {
                                        let r2 = ref_click.clone();
                                        move |_, _, cx| {
                                            // click the id to copy the ref
                                            cx.write_to_clipboard(
                                                gpui::ClipboardItem::new_string(r2.clone()),
                                            );
                                        }
                                    })
                                    .child(SharedString::from(m.id.clone())),
                            ),
                    )
                    .children(pin.map(|p| {
                        div()
                            .px(px(4.))
                            .py(px(1.))
                            .rounded(px(3.))
                            .bg(gpui::hsla(0.63, 0.86, 0.62, 0.12))
                            .text_size(px(9.))
                            .text_color(gpui::hsla(0.63, 0.86, 0.62, 0.8))
                            .child(SharedString::from(p))
                    }))
                    .child({
                        // ConfigSwitch 32×18 (pi-web .config-switch)
                        let knob_left = if is_enabled { px(14.) } else { px(2.) };
                        let on = is_enabled;
                        div()
                            .id(SharedString::from(format!("mc-sw-{ix}")))
                            .w(px(32.))
                            .h(px(18.))
                            .flex_shrink_0()
                            .rounded(px(9.))
                            .border_1()
                            .border_color(if on { rgb(t.accent) } else { rgb(t.border) })
                            .bg(if on { rgb(t.accent) } else { rgb(t.bg_selected) })
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .ml(knob_left)
                                    .size(px(12.))
                                    .rounded_full()
                                    .bg(if on { rgb(t.bg) } else { rgb(t.text_muted) }),
                            )
                            .when(!row_last, |sw| {
                                sw.cursor_pointer()
                                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                        cx.stop_propagation();
                                        let _ = weak_row.update(cx, |c, cx| {
                                            c.mc_toggle_model(ref_str.clone(), !on, cx)
                                        });
                                    })
                            })
                            .when(row_last, |sw| sw.opacity(0.55))
                            .into_any_element()
                    }),
            );
        }
        let _ = weak_rows;
        section_col = section_col.child(list);
        detail = detail.child(section_col);
    }
    (sidebar.into_any_element(), detail.into_any_element())
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
