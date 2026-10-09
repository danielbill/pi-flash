//! 扩展勾选菜单（040 定稿 + 2026-10 改版：勾选即生效）。
//!
//! 入口 = 工具胶囊**右边**的 `plug` 按钮，**仅「full+」档激活**（其余档
//! 扩展本来就不挂载，置灰）。语义：
//!
//! - **反映管理态**：设置页启用的包是基础扩展集，打开时**自动勾选**；
//!   按名称排序、去掉 `npm:` 前缀展示。
//! - **勾选即生效**：没有「确认」按钮——每勾/取消一项，清单当即写入
//!   `runtime.ext_sources`（按钮计数【扩展(n)】实时变）并落
//!   `~/.pi-flash/session-ext.json`（每个对话一份）。重绑进程统一走
//!   `pending_ext_sources`：下一轮 `send_input` 空闲发送前重绑
//!   （`--session` 复接不丢消息）——连点 N 项也只重绑一次，且运行中
//!   不掐轮（pi 的 RPC 没有运行时换包命令，重绑是唯一通路）。

use gpui::{AnyElement, MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::theme::theme as T;

/// 勾选集 + 滚动位（勾选当即落地，不存在「取消丢弃」）。
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

/// 行高（10 行限高 = `ROW_H × 10`）。
const ROW_H: f32 = 30.;

impl Chat {
    /// 扩展按钮点击：已开则收，未开则按守卫打开。
    pub(crate) fn toggle_plugin_menu_at(
        &mut self,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.plugin_picker.is_some() {
            self.plugin_picker_cancel(cx);
            return;
        }
        self.pill_anchor = Some(crate::PillBtns::anchor(&self.pill_btn.ext, at));
        self.open_plugin_picker(cx);
    }

    /// 打开菜单。守卫：档位必须是「full+」（其余档扩展本来就不挂）。
    /// 040：每轮对话前都可重调。
    ///
    /// 勾选态**只对齐会话数组**（`ext_sources`，用户定稿）：全局启用集只在
    /// 「新会话创建」时复制成会话初始清单（`SessionRuntime::new`），菜单
    /// 不再并回全局集——否则取消的勾在重开时被全局集顶回去（回显 bug）。
    pub(crate) fn open_plugin_picker(&mut self, cx: &mut gpui::Context<Self>) {
        if self.rt().read(cx).tool_preset_key() != "custom" {
            self.pill_menu = None;
            self.set_status(crate::i18n::tr("扩展只能在 full+ 档勾选").to_string(), cx);
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
        }
        self.apply_ext_selection(cx);
        cx.notify();
    }

    /// 勾选即生效：把当前勾选集写入 runtime（按钮计数实时变）并落台账。
    /// 重绑时机 = 下一轮 `send_input` 空闲发送前（见模块注释）。
    fn apply_ext_selection(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(picker) = self.plugin_picker.as_ref() else {
            return;
        };
        // 宇宙序 = 全局 → 项目，去重（同一来源出现在两个 scope 时 `-e` 只发一次），
        // 再按展示名排序，与面板呈现一致
        let mut sources: Vec<String> = Vec::new();
        for v in self.mc_pkgs_global.iter().chain(self.mc_pkgs_project.iter()) {
            let src = pi_link::skills::entry_source(v);
            if src.is_empty() || !picker.pending.contains(&src) || sources.contains(&src) {
                continue;
            }
            sources.push(src);
        }
        sources.sort_by_key(|s| pi_link::skills::display_source(s).to_lowercase());
        let rt = self.rt();
        let key = rt.read(cx).key.clone();
        rt.update(cx, |r, _| {
            r.ext_sources = sources.clone();
            r.pending_ext_sources = Some(sources.clone());
        });
        // 每个对话保存一份：落 ~/.pi-flash/session-ext.json，重启后按会话恢复
        if let Some(path) = pi_link::session_ext::store_path() {
            if let Err(e) = pi_link::session_ext::write_for(&path, &key, &sources) {
                self.set_status(crate::i18n::tf("扩展清单保存失败: {e}", &[("e", e)]), cx);
                return;
            }
        }
        if rt.read(cx).agent_running {
            // 运行中不掐轮，只提示生效时机
            self.set_status(crate::i18n::tr("扩展清单将在下一轮生效").to_string(), cx);
        }
    }
}

/// 面板元素（`None` = 没开）。挂在 app root 上，锚点同工具胶囊菜单。
pub(crate) fn view(chat: &Chat, weak: &gpui::WeakEntity<Chat>, window: &gpui::Window) -> Option<AnyElement> {
    let picker = chat.plugin_picker.as_ref()?;
    let t = T();
    let ui = crate::appearance::ui_size;
    let selected = |src: &str| picker.pending.contains(src);

    // ---- 宇宙 = 全局 ∪ 项目包去重；先按展示名排序，再稳定地把已勾选置顶 ----
    let mut items: Vec<(String, bool)> = Vec::new(); // (来源, 项目)
    for v in chat.mc_pkgs_global.iter().chain(chat.mc_pkgs_project.iter()) {
        let src = pi_link::skills::entry_source(v);
        if src.is_empty() || items.iter().any(|(s, _)| *s == src) {
            continue;
        }
        let project = chat
            .mc_pkgs_project
            .iter()
            .any(|p| pi_link::skills::entry_source(p) == src);
        items.push((src, project));
    }
    items.sort_by_key(|(s, _)| pi_link::skills::display_source(s).to_lowercase());
    items.sort_by_key(|(s, _)| !selected(s));
    let rows: Vec<AnyElement> = items
        .iter()
        .map(|(src, project)| {
            let tokens = chat.ext_tokens.get(src).map(|e| e.ext.max(0) as u64);
            plugin_row(weak, src, selected(src), *project, tokens, t)
        })
        .collect();
    let n_rows = rows.len();

    let mut body = rows;
    if body.is_empty() {
        body.push(
            div()
                .px(px(10.))
                .py(px(12.))
                .text_size(ui(11.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(crate::i18n::tr("没有已配置的扩展")))
                .into_any_element(),
        );
    }
    // 10 行限高：内容不到就按内容高（不出滚动条），超出才截到 300px 滚动
    let list_h = if n_rows == 0 { 60. } else { (n_rows as f32 * ROW_H).min(ROW_H * 10.) };

    // 标题行右侧：当前勾选集的说明 token 合计（040 延迟加载测得多少算多少）
    let total: i64 = items
        .iter()
        .filter(|(s, _)| selected(s))
        .filter_map(|(s, _)| chat.ext_tokens.get(s))
        .map(|e| e.ext)
        .sum();

    let header = div()
        .px(px(10.))
        .pt(px(10.))
        .pb(px(6.))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .text_size(ui(12.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text))
                .child(SharedString::from(crate::i18n::tr("选择扩展"))),
        )
        .when(total > 0, |d| {
            // 不用 ml_auto（实测 CJK 标签参与测量时 auto margin 收不满，合计
            // 墨缘差 8px）——数字作**最后一个子元素**，同下方行一样的机制
            // 被 flex 顶到内容右缘，与行内 token 列共线
            d.child(div().flex_1())
                .child(
                    div()
                        .text_size(ui(10.))
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(crate::i18n::tr("合计："))),
                )
                .child(
                    div()
                        .font_family(crate::markdown::MONO_FAMILY)
                        .text_size(ui(10.))
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(crate::services::format::fmt_thousand(
                            total.max(0) as u64,
                        ))),
                )
        });

    // 锚点算法与工具胶囊菜单一致（统一定位：按钮居中 + 5px）
    let vp = window.viewport_size();
    let gap = px(5.);
    // 040：列表加宽 100px 容纳右侧 token 标注（320 → 420）
    let panel_w = px(420.);
    let (bottom, left) = match chat.pill_anchor {
        Some(a) => {
            let bottom = (vp.height - a.top + gap).max(px(8.));
            let mut left = a.center_x - panel_w / 2.;
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
                    .pb(px(6.))
                    .h(px(list_h))
                    .overflow_y_scroll()
                    .track_scroll(&picker.scroll)
                    .flex()
                    .flex_col()
                    .children(body),
            ),
    );
    Some(layer.child(card).into_any_element())
}

/// 勾选行：14px 勾选框 + 展示名（去 `npm:`，等宽字体）+ 可选「项目」小标
/// + 右侧说明 token 数（040 延迟加载测得才显示，不带单位）。
fn plugin_row(
    weak: &gpui::WeakEntity<Chat>,
    source: &str,
    checked: bool,
    project: bool,
    tokens: Option<u64>,
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
                .when(
                    checked,
                    |d| d.child(crate::ui::icon("check", 10., t.accent_contrast)),
                ),
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
                .text_color(rgb(t.text))
                .child(SharedString::from(
                    pi_link::skills::display_source(source).to_string(),
                )),
        );
    if project {
        row = row.child(tag(crate::i18n::tr("项目"), crate::settings::widgets::indigo_bg(), crate::settings::widgets::indigo_fg()));
    }
    if let Some(n) = tokens {
        row = row.child(
            div()
                .flex_shrink_0()
                .font_family(crate::markdown::MONO_FAMILY)
                .text_size(ui(10.))
                .text_color(rgb(t.text_faint))
                .child(SharedString::from(crate::services::format::fmt_thousand(n))),
        );
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
