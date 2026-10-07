//! 插件勾选菜单（034 定稿）：输入栏「插件」按钮点开的三层勾选面板。
//!
//! 入口 = 工具胶囊**右边**的 `plug` 按钮，**仅「自定义」档激活**（其余档置灰）。
//! 三层结构（用户定稿）：
//!
//! ```text
//! 已选中 n          ← 当前会话清单里的包（项目来源带「项目」小标）
//! ── 项目 · 未选中 n  ← 项目 settings 里、还没勾的
//! ── 全局 · 未选中 n  ← 全局 settings 里、还没勾的
//! ```
//!
//! 勾选是**临时态**，底部「取消 / 选择并切换」才落地：确认 → 写
//! `runtime.ext_sources` → 落 `~/.pi-flash/session-ext.json`（每个对话一份）→
//! 走 `mc_set_tools_preset("custom")` 既有重绑链路（整表重读、状态栏提示复用）。
//!
//! 两条守卫：① 非自定义档点不动（按钮本来就置灰，这里是兜底）；② **会话一旦
//! 有消息就不能改**（用户定稿：对话中途无法修改自定义），只能新开对话时定。

use gpui::{AnyElement, MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::settings::widgets::{Btn, config_button, group_header};
use crate::theme::theme as T;

/// 临时勾选 + 滚动位；确认前不碰 runtime（取消/点外 = 全丢）。
pub(crate) struct PluginPicker {
    /// 勾选中的 `entry_source()` 集合
    pub(crate) pending: std::collections::HashSet<String>,
    /// 限高滚动位置（跨帧复用，参照 vlist「血案」：滚动句柄不能每帧重建）
    pub(crate) scroll: gpui::ScrollHandle,
}

impl PluginPicker {
    fn new(selected: impl IntoIterator<Item = String>) -> Self {
        Self {
            pending: selected.into_iter().collect(),
            scroll: gpui::ScrollHandle::new(),
        }
    }
}

/// 行高（10 行限高 = `ROW_H × 10`）；分组头高同设置页 `group_header` 节奏。
const ROW_H: f32 = 30.;
const GROUP_H: f32 = 28.;

impl Chat {
    /// 插件按钮点击：已开则收（丢弃临时勾选），未开则按守卫打开。
    pub(crate) fn toggle_plugin_menu_at(
        &mut self,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.plugin_picker.is_some() {
            self.plugin_picker_cancel(cx);
            return;
        }
        self.pill_anchor = Some(at);
        self.open_plugin_picker(cx);
    }

    /// 打开菜单。守卫：档位必须是「自定义」；会话必须还没开聊（消息为空）。
    pub(crate) fn open_plugin_picker(&mut self, cx: &mut gpui::Context<Self>) {
        if self.rt().read(cx).tool_preset_key() != "custom" {
            self.pill_menu = None;
            self.set_status(crate::i18n::tr("插件只能在自定义档勾选").to_string(), cx);
            return;
        }
        if !self.rt().read(cx).messages.is_empty() {
            self.pill_menu = None;
            self.set_status(crate::i18n::tr("对话中途无法修改自定义").to_string(), cx);
            return;
        }
        let seed = self.rt().read(cx).ext_sources.clone();
        self.pill_menu = None;
        self.plugin_picker = Some(PluginPicker::new(seed));
        cx.notify();
    }

    pub(crate) fn plugin_picker_cancel(&mut self, cx: &mut gpui::Context<Self>) {
        self.plugin_picker = None;
        cx.notify();
    }

    pub(crate) fn plugin_picker_toggle(&mut self, source: &str, cx: &mut gpui::Context<Self>) {
        if let Some(p) = self.plugin_picker.as_mut() {
            if !p.pending.remove(source) {
                p.pending.insert(source.to_string());
            }
            cx.notify();
        }
    }

    /// 「选择并切换」：写 runtime → 落会话台账 → 重绑进程。
    pub(crate) fn plugin_picker_confirm(&mut self, cx: &mut gpui::Context<Self>) {
        if self.plugin_picker.is_none() {
            return;
        }
        if self.rt().read(cx).agent_running {
            // 面板只在空闲/未开聊时可开；真跑到这里就按既有提示语拦截
            self.set_status(crate::i18n::tr("运行中不能更换工具预设").to_string(), cx);
            return;
        }
        let Some(picker) = self.plugin_picker.take() else {
            return;
        };
        // 表序 = 全局 → 项目，去重（同一来源出现在两个 scope 时 `-e` 只发一次）
        let mut sources: Vec<String> = Vec::new();
        for v in self.mc_pkgs_global.iter().chain(self.mc_pkgs_project.iter()) {
            let src = pi_link::skills::entry_source(v);
            if src.is_empty() || !picker.pending.contains(&src) || sources.contains(&src) {
                continue;
            }
            sources.push(src);
        }
        let rt = self.rt();
        let key = rt.read(cx).key.clone();
        rt.update(cx, |r, _| r.ext_sources = sources.clone());
        // 每个对话保存一份：落 ~/.pi-flash/session-ext.json，重启后按会话恢复
        if let Some(path) = pi_link::session_ext::store_path() {
            if let Err(e) = pi_link::session_ext::write_for(&path, &key, &sources) {
                self.set_status(crate::i18n::tf("插件清单保存失败: {e}", &[("e", e)]), cx);
            }
        }
        self.mc_set_tools_preset("custom", cx);
    }
}

/// 面板元素（`None` = 没开）。挂在 app root 上，锚点同工具胶囊菜单。
pub(crate) fn view(chat: &Chat, weak: &gpui::WeakEntity<Chat>, window: &gpui::Window) -> Option<AnyElement> {
    let picker = chat.plugin_picker.as_ref()?;
    let t = T();
    let ui = crate::appearance::ui_size;
    let selected = |src: &str| picker.pending.contains(src);

    // ---- 三层：已选中 / 项目未选中 / 全局未选中 ----
    let mut rows: Vec<AnyElement> = Vec::new();
    let mut n_rows = 0usize;
    let mut n_groups = 0usize;

    let section = |title: String, items: Vec<(String, bool, bool)>, rows: &mut Vec<AnyElement>, n_rows: &mut usize, n_groups: &mut usize| {
        if items.is_empty() {
            return;
        }
        *n_rows += items.len();
        *n_groups += 1;
        rows.push(group_header(&title, None));
        for (src, disabled, project) in items {
            let checked = selected(&src);
            rows.push(plugin_row(weak, &src, checked, disabled, project, t));
        }
    };

    // 1) 已选中（全局 + 项目表序），项目来源带「项目」小标
    let mut sel_items: Vec<(String, bool, bool)> = Vec::new();
    for v in chat.mc_pkgs_global.iter().chain(chat.mc_pkgs_project.iter()) {
        let src = pi_link::skills::entry_source(v);
        let project = chat
            .mc_pkgs_project
            .iter()
            .any(|p| pi_link::skills::entry_source(p) == src);
        if src.is_empty() || !selected(&src) || sel_items.iter().any(|(s, _, _)| *s == src) {
            continue;
        }
        sel_items.push((src, pi_link::skills::entry_disabled(v), project));
    }
    let n_sel = sel_items.len();
    section(
        crate::i18n::tf("已选中 {n}", &[("n", n_sel.to_string())]),
        sel_items,
        &mut rows,
        &mut n_rows,
        &mut n_groups,
    );

    // 2) 项目未选中
    let proj_items: Vec<(String, bool, bool)> = chat
        .mc_pkgs_project
        .iter()
        .map(|v| {
            (
                pi_link::skills::entry_source(v),
                pi_link::skills::entry_disabled(v),
                true,
            )
        })
        .filter(|(src, _, _)| !src.is_empty() && !selected(src))
        .collect();
    let n_proj = proj_items.len();
    section(
        crate::i18n::tf("项目 · 未选中 {n}", &[("n", n_proj.to_string())]),
        proj_items,
        &mut rows,
        &mut n_rows,
        &mut n_groups,
    );

    // 3) 全局未选中
    let glob_items: Vec<(String, bool, bool)> = chat
        .mc_pkgs_global
        .iter()
        .map(|v| {
            (
                pi_link::skills::entry_source(v),
                pi_link::skills::entry_disabled(v),
                false,
            )
        })
        .filter(|(src, _, _)| !src.is_empty() && !selected(src))
        .collect();
    let n_glob = glob_items.len();
    section(
        crate::i18n::tf("全局 · 未选中 {n}", &[("n", n_glob.to_string())]),
        glob_items,
        &mut rows,
        &mut n_rows,
        &mut n_groups,
    );

    if rows.is_empty() {
        rows.push(
            div()
                .px(px(10.))
                .py(px(12.))
                .text_size(ui(11.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(crate::i18n::tr("没有已配置的插件")))
                .into_any_element(),
        );
    }
    // 10 行限高：内容不到就按内容高（不出滚动条），超出才截到 300px 滚动
    let content_h = n_rows as f32 * ROW_H + n_groups as f32 * GROUP_H;
    let list_h = if n_rows == 0 { 60. } else { content_h.min(ROW_H * 10.) };

    let header = div()
        .px(px(10.))
        .pt(px(10.))
        .pb(px(6.))
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(
            div()
                .text_size(ui(12.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text))
                .child(SharedString::from(crate::i18n::tr("本会话插件清单"))),
        )
        .child(
            div()
                .text_size(ui(10.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(crate::i18n::tr(
                    "仅本会话生效，不改动设置页的全局插件开关",
                ))),
        );

    let footer = div()
        .flex()
        .items_center()
        .justify_end()
        .gap(px(6.))
        .px(px(10.))
        .py(px(8.))
        .border_t_1()
        .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x80)))
        .child(config_button(
            "pp-cancel",
            weak,
            &crate::i18n::tr("取消"),
            Btn::Secondary,
            true,
            false,
            |c, cx| c.plugin_picker_cancel(cx),
        ))
        .child(config_button(
            "pp-confirm",
            weak,
            &crate::i18n::tr("选择并切换"),
            Btn::Primary,
            true,
            false,
            |c, cx| c.plugin_picker_confirm(cx),
        ));

    // 锚点算法与工具胶囊菜单一致（bottom = 视口高 − 胶囊 y + gap，左缘钳制）
    let vp = window.viewport_size();
    let gap = px(6.);
    let panel_w = px(320.);
    let (bottom, left) = match chat.pill_anchor {
        Some(p) => {
            let bottom = (vp.height - p.y + gap).max(px(8.));
            let mut left = p.x - px(8.);
            if left + panel_w > vp.width - px(8.) {
                left = vp.width - panel_w - px(8.);
            }
            (bottom, left.max(px(8.)))
        }
        None => (px(64.), vp.width - panel_w - px(24.)),
    };

    let dismiss_weak = weak.clone();
    let layer = crate::ui::overlay::layer(false, None, move |_w, cx| {
        let _ = dismiss_weak.update(cx, |c, cx| c.plugin_picker_cancel(cx));
    });

    let card = crate::ui::overlay::stop_click(
        div()
            .absolute()
            .bottom(bottom)
            .left(left)
            .w(panel_w)
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(t.border))
            .bg(rgb(t.bg))
            .shadow_lg()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .id("pp-list")
                    .h(px(list_h))
                    .overflow_y_scroll()
                    .track_scroll(&picker.scroll)
                    .flex()
                    .flex_col()
                    .children(rows),
            )
            .child(footer),
    );
    Some(layer.child(card).into_any_element())
}

/// 勾选行：14px 勾选框 + 来源（等宽字体）+ 可选「项目」「已禁用」小标。
/// 禁用只是设置页的全局开关状态，`-e` 是独立加载路径（031 边界），照样可勾。
fn plugin_row(
    weak: &gpui::WeakEntity<Chat>,
    source: &str,
    checked: bool,
    disabled: bool,
    project: bool,
    t: &'static crate::theme::Theme,
) -> AnyElement {
    let ui = crate::appearance::ui_size;
    let row_weak = weak.clone();
    let src = source.to_string();
    let mut row = div()
        .id(SharedString::from(format!("pp-row-{source}")))
        .h(px(ROW_H))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(8.))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            // 行内点击不冒泡到「点外关闭」
            cx.stop_propagation();
            let _ = row_weak.update(cx, |c, cx| c.plugin_picker_toggle(&src, cx));
        })
        .child(
            div()
                .size(px(14.))
                .flex_shrink_0()
                .rounded(px(3.))
                .border_1()
                .border_color(rgb(if checked { t.accent } else { t.border }))
                .bg(rgb(if checked { t.accent } else { t.bg_panel }))
                .flex()
                .items_center()
                .justify_center()
                .when(checked, |d| d.child(crate::ui::icon("check", 10., 0xffffff))),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .font_family(crate::markdown::MONO_FAMILY)
                .text_size(ui(11.))
                .text_color(rgb(if disabled { t.text_dim } else { t.text }))
                .child(SharedString::from(source.to_string())),
        );
    if project {
        row = row.child(tag(crate::i18n::tr("项目"), crate::settings::widgets::indigo_bg(), crate::settings::widgets::indigo_fg()));
    }
    if disabled {
        row = row.child(tag(
            crate::i18n::tr("已禁用"),
            gpui::hsla(0., 0., 0.5, 0.12),
            gpui::hsla(0., 0., 0.9, 0.6),
        ));
    }
    row.into_any_element()
}

fn tag(
    label: &str,
    bg: gpui::Hsla,
    fg: gpui::Hsla,
) -> AnyElement {
    div()
        .flex_shrink_0()
        .px(px(4.))
        .py(px(1.))
        .rounded(px(3.))
        .bg(bg)
        .text_size(crate::appearance::ui_size(9.))
        .text_color(fg)
        .child(SharedString::from(label.to_string()))
        .into_any_element()
}
