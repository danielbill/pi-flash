//! MCP 勾选菜单（043 定稿：完全对照 plugin_picker，勾选即生效）。
//!
//! 入口 = 扩展按钮右边的 `server` 按钮（全局 mcp.json 无条目时整个
//! 隐藏）。与扩展按钮的两处刻意差异：
//!
//! - **不设档位守卫**：扩展只在 full+ 档挂载，MCP 各档都装载
//!   （builtin:mcp 全档放行），故按钮常亮可点。
//! - **清单是全局文件不是会话参数**：勾选当即写 mcp.json `enabled`，
//!   并标记 `pending_mcp_reload`，下一轮 `send_input` 空闲发送前重绑生效
//!   （新进程启动时重读清单）。注意：**无论运行与否都必须标记**——
//!   MCP 装载只在 pi 进程启动时读 mcp.json（扩展加载期 + session_start），
//!   空闲但活着的进程永远不会自己重读；「空闲 spawn 本就直接读盘」只对
//!   还没 spawn 的会话成立。运行中由 `rebind_pending` 自动推迟到空闲发送
//!   （flag 不清，等到那一刻为止）——原先只在 agent_running 时标记，空闲
//!   改动在活进程上永远不生效（真机 2026-11-03：取消勾选 ghzw 后下一轮
//!   发送 adapter 照旧挂载）。

use gpui::{AnyElement, MouseButton, SharedString, div, prelude::*, px, rgb};

use super::Chat;
use super::picker_shell::{
    PAD, capped_list, capped_list_h, empty_row, picker_card, pill_anchor_edges,
};
use crate::appearance::theme::theme as T;

/// 行高（10 行限高 = `ROW_H × 10`，与 pi_extension_picker 一致）。
use super::picker_shell::ROW_H;

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
        self.dlg.pill_anchor = None;
        self.dlg.mcp_picker = Some(McpPicker::new());
        cx.notify();
    }

    /// MCP 按钮点击：已开则收，未开则打开（无档位守卫）。
    pub(crate) fn toggle_mcp_menu_at(
        &mut self,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.dlg.mcp_picker.is_some() {
            self.dlg.mcp_picker = None;
            cx.notify();
            return;
        }
        self.dlg.pill_anchor = Some(crate::PillBtns::anchor(&self.dlg.pill_btn.mcp, at));
        self.dlg.mcp_picker = Some(McpPicker::new());
        cx.notify();
    }

    /// 勾选即生效：写 mcp.json enabled；运行中标记重绑（下一轮发送前）。
    pub(crate) fn mcp_picker_toggle(&mut self, name: &str, cx: &mut gpui::Context<Self>) {
        let Some(entry) = self
            .cfg
            .mcp_servers
            .iter()
            .find(|s| s.scope == pi_link::mcp::Scope::Global && s.name == name)
            .cloned()
        else {
            return;
        };
        if let Err(e) = pi_link::mcp::set_enabled(&entry.file, &entry.name, !entry.enabled) {
            self.set_status(crate::i18n::tf("mcp.save_failed", &[("e", e)]), cx);
            return;
        }
        self.reload_settings_panel();
        // 无论运行与否都标记（见模块头注释）；运行中由 rebind_pending 推迟
        self.mark_mcp_reload(cx);
        self.set_status(crate::i18n::tr("mcp.next_turn").to_string(), cx);
        cx.notify();
    }

    /// mcp.json 已变更：标记活跃会话在下一轮空闲发送前重绑（幂等）。
    /// 运行中不清 flag、不掐轮 —— `rebind_pending` 开头的 agent_running
    /// 分支原样保留 flag，等到空闲发送那刻才 kill+respawn。
    pub(crate) fn mark_mcp_reload(&mut self, cx: &mut gpui::Context<Self>) {
        let rt = self.rt();
        rt.update(cx, |r, _| r.pending_mcp_reload = true);
    }
}

/// MCP 菜单条目（composer 菜单与设置·子代理页共用，2026-11-07 收口）：
/// 全局 scope 已启用服务器，按名称排序，`is_selected` 者稳定置顶。
/// 返回 (名称, 传输类型)。
pub(crate) fn mcp_menu_items(
    chat: &Chat,
    is_selected: &impl Fn(&str) -> bool,
) -> Vec<(String, String)> {
    let mut items: Vec<&pi_link::mcp::ServerEntry> = chat
        .cfg
        .mcp_servers
        .iter()
        .filter(|s| s.scope == pi_link::mcp::Scope::Global)
        .collect();
    items.sort_by(|a, b| a.name.cmp(&b.name));
    items.sort_by_key(|s| !is_selected(&s.name));
    items
        .into_iter()
        .map(|s| (s.name.clone(), s.transport().to_string()))
        .collect()
}

/// MCP 菜单头（标题 + 已启用 n/total）：两侧共用同一版式。
pub(crate) fn mcp_menu_header(enabled: usize, total: usize) -> gpui::Div {
    let t = T();
    let ui = crate::appearance::ui_size;
    div()
        .px(px(PAD))
        .pt(px(PAD))
        .pb(px(PAD))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .text_size(ui(12.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text))
                .child(SharedString::from(crate::i18n::tr("mcp.select"))),
        )
        .child(div().flex_1())
        .child(
            div()
                .text_size(ui(10.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(crate::i18n::tf(
                    "mcp.enabled_count",
                    &[("n", enabled.to_string()), ("total", total.to_string())],
                ))),
        )
}

/// 面板元素（`None` = 没开）。挂在 app root 上，锚点同扩展菜单。
pub(crate) fn view(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    window: &gpui::Window,
) -> Option<AnyElement> {
    let picker = chat.dlg.mcp_picker.as_ref()?;
    let t = T();

    let is_selected = |name: &str| {
        chat.cfg
            .mcp_servers
            .iter()
            .any(|s| s.name == name && s.enabled)
    };
    let items = mcp_menu_items(chat, &is_selected);
    let rows: Vec<AnyElement> = items
        .iter()
        .map(|(name, transport)| {
            let name = name.clone();
            let pick = name.clone();
            mcp_row(
                weak,
                &name,
                is_selected(&name),
                transport,
                move |c, cx| c.mcp_picker_toggle(&pick, cx),
                t,
            )
        })
        .collect();
    let n_rows = rows.len();

    let mut body = rows;
    if body.is_empty() {
        body.push(empty_row(SharedString::from(crate::i18n::tr(
            "mcp.none_configured",
        ))));
    } // 10 行限高：内容不到就按内容高（不出滚动条），超出才截到 300px 滚动
    let list_h = capped_list_h(n_rows);

    let enabled_n = chat
        .cfg
        .mcp_servers
        .iter()
        .filter(|s| s.scope == pi_link::mcp::Scope::Global && s.enabled)
        .count();
    let header = mcp_menu_header(enabled_n, items.len());

    // 锚点算法与扩展菜单一致（统一定位：按钮居中 + 半屏定向，唯一落点
    // pill_anchor_edges）
    let vp = window.viewport_size();
    let panel_w = px(320.);
    let pos = pill_anchor_edges(chat.dlg.pill_anchor, vp, panel_w);

    let layer = crate::ui::menu::menu_layer(None, weak, |c, _cx| c.dlg.mcp_picker = None);

    let card = crate::ui::overlay::stop_click(
        picker_card(pos, panel_w, t)
            .child(header)
            .child(capped_list("mcp-picker-list", &picker.scroll, list_h, body)),
    );
    Some(layer.child(card).into_any_element())
}

/// 勾选行：14px 勾选框 + 名称（等宽）+ 右侧传输类型小标。`on_toggle` 由
/// 调用方写入各自状态（composer 写 runtime，设置页写表单清单）。
pub(crate) fn mcp_row(
    weak: &gpui::WeakEntity<Chat>,
    name: &str,
    checked: bool,
    transport: &str,
    on_toggle: impl Fn(&mut Chat, &mut gpui::Context<Chat>) + 'static,
    t: &'static crate::appearance::theme::Theme,
) -> AnyElement {
    let ui = crate::appearance::ui_size;
    let row_weak = weak.clone();
    div()
        .id(SharedString::from(format!("mcp-row-{name}")))
        .h(px(ROW_H))
        .px(px(PAD))
        .flex()
        .items_center()
        .gap(px(8.))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            // 行内点击不冒泡到「点外关闭」
            cx.stop_propagation();
            let _ = row_weak.update(cx, |c, cx| on_toggle(c, cx));
        })
        .child(crate::ui::kit::checkbox(checked))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .font_family(crate::editor::markdown::MONO_FAMILY)
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
