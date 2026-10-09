//! MCP 勾选菜单（043 定稿：完全对照 plugin_picker，勾选即生效）。
//!
//! 入口 = 扩展按钮右边的 `server` 按钮（全局 mcp.json 无条目时整个
//! 隐藏）。与扩展按钮的两处刻意差异：
//!
//! - **不设档位守卫**：扩展只在 full+ 档挂载，MCP 各档都装载
//!   （builtin:mcp 全档放行），故按钮常亮可点。
//! - **清单是全局文件不是会话参数**：勾选当即写 mcp.json `enabled`，
//!   运行中标记 `pending_mcp_reload`，下一轮 `send_input` 空闲发送前
//!   重绑生效（新进程 session_start 重读清单）；空闲时本就无需重绑。

use gpui::{AnyElement, MouseButton, SharedString, div, prelude::*, px, rgb};

use super::Chat;
use crate::theme::theme as T;

/// 行高（10 行限高 = `ROW_H × 10`，与 plugin_picker 一致）。
const ROW_H: f32 = 30.;

/// 打开时的滚动位（勾选当即落盘，无「取消丢弃」态可存）。
pub(crate) struct McpPicker {
    pub(crate) scroll: gpui::ScrollHandle,
}

impl McpPicker {
    fn new() -> Self {
        Self {
            scroll: gpui::ScrollHandle::new(),
        }
    }
}

impl Chat {
    /// 自动化入口（无点击坐标）：锚点缺省走面板的右下兜底定位。
    pub(crate) fn open_mcp_picker(&mut self, cx: &mut gpui::Context<Self>) {
        self.pill_anchor = None;
        self.mcp_picker = Some(McpPicker::new());
        cx.notify();
    }

    /// MCP 按钮点击：已开则收，未开则打开（无档位守卫）。
    pub(crate) fn toggle_mcp_menu_at(
        &mut self,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.mcp_picker.is_some() {
            self.mcp_picker = None;
            cx.notify();
            return;
        }
        self.pill_anchor = Some(crate::PillBtns::anchor(&self.pill_btn.mcp, at));
        self.mcp_picker = Some(McpPicker::new());
        cx.notify();
    }

    /// 勾选即生效：写 mcp.json enabled；运行中标记重绑（下一轮发送前）。
    pub(crate) fn mcp_picker_toggle(&mut self, name: &str, cx: &mut gpui::Context<Self>) {
        let Some(entry) = self
            .mcp_servers
            .iter()
            .find(|s| s.scope == pi_link::mcp::Scope::Global && s.name == name)
            .cloned()
        else {
            return;
        };
        if let Err(e) = pi_link::mcp::set_enabled(&entry.file, &entry.name, !entry.enabled) {
            self.set_status(crate::i18n::tf("MCP 保存失败: {e}", &[("e", e)]), cx);
            return;
        }
        self.reload_settings_panel();
        let rt = self.rt();
        if rt.read(cx).agent_running {
            // 运行中不掐轮：标记重绑，下一轮 send_input 空闲发送前生效
            rt.update(cx, |r, _| r.pending_mcp_reload = true);
            self.set_status(crate::i18n::tr("MCP 清单将在下一轮生效").to_string(), cx);
        }
        cx.notify();
    }
}

/// 面板元素（`None` = 没开）。挂在 app root 上，锚点同扩展菜单。
pub(crate) fn view(chat: &Chat, weak: &gpui::WeakEntity<Chat>, window: &gpui::Window) -> Option<AnyElement> {
    let picker = chat.mcp_picker.as_ref()?;
    let t = T();
    let ui = crate::appearance::ui_size;

    // ---- 全局条目：按名称排序，再稳定地把已勾选置顶 ----
    let mut items: Vec<&pi_link::mcp::ServerEntry> = chat
        .mcp_servers
        .iter()
        .filter(|s| s.scope == pi_link::mcp::Scope::Global)
        .collect();
    items.sort_by(|a, b| a.name.cmp(&b.name));
    items.sort_by_key(|s| !s.enabled);
    let rows: Vec<AnyElement> = items
        .iter()
        .map(|s| mcp_row(weak, &s.name, s.enabled, s.transport(), t))
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
                .child(SharedString::from(crate::i18n::tr("没有已配置的 MCP 服务器")))
                .into_any_element(),
        );
    }
    // 10 行限高：内容不到就按内容高（不出滚动条），超出才截到 300px 滚动
    let list_h = if n_rows == 0 { 60. } else { (n_rows as f32 * ROW_H).min(ROW_H * 10.) };

    let enabled_n = items.iter().filter(|s| s.enabled).count();
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
                .child(SharedString::from(crate::i18n::tr("选择 MCP"))),
        )
        .child(div().flex_1())
        .child(
            div()
                .text_size(ui(10.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(crate::i18n::tf(
                    "已开启 {n}/{total}",
                    &[("n", enabled_n.to_string()), ("total", items.len().to_string())],
                ))),
        );

    // 锚点算法与扩展菜单一致（统一定位：按钮居中 + 5px）
    let vp = window.viewport_size();
    let gap = px(5.);
    let panel_w = px(320.);
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
        let _ = dismiss_weak.update(cx, |c, _cx| c.mcp_picker = None);
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
                    .id("mcp-picker-list")
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

/// 勾选行：14px 勾选框 + 名称（等宽）+ 右侧传输类型小标。
fn mcp_row(
    weak: &gpui::WeakEntity<Chat>,
    name: &str,
    checked: bool,
    transport: &str,
    t: &'static crate::theme::Theme,
) -> AnyElement {
    let ui = crate::appearance::ui_size;
    let row_weak = weak.clone();
    let owned = name.to_string();
    div()
        .id(SharedString::from(format!("mcp-row-{name}")))
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
            let _ = row_weak.update(cx, |c, cx| c.mcp_picker_toggle(&owned, cx));
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
                .text_color(rgb(t.text))
                .child(SharedString::from(name.to_string())),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_size(ui(9.))
                .text_color(rgb(t.text_faint))
                .child(SharedString::from(transport.to_string())),
        )
        .into_any_element()
}
