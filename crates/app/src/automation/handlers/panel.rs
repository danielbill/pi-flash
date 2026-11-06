//! panel/content/terminal 组：dock 面板切换、内容区视图切换、工作区终端。

use gpui::{Context, Window};
use serde_json::{Value, json};

use super::{OpResult, bad, ok};
use crate::{Chat, ContentView, DockPanel};
use pi_link::automation::method;

pub(super) fn dispatch(
    chat: &mut Chat,
    window: &mut Window,
    cx: &mut Context<Chat>,
    m: &str,
    params: &Value,
) -> OpResult {
    match m {
        method::PANEL_DOCK => {
            let panel = params
                .get("panel")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"panel\": \"default|git|plugin\"}"))?;
            let plugin_ix = params.get("ix").and_then(Value::as_u64).unwrap_or(0) as usize;
            if panel == "plugin" {
                // 走 open_plugin_dock（与点状态栏 tab 同语义）：触发插件懒加载链
                // （建 ctx/装 wasm → onPanelOpen → render 缓存）；直接赋值
                // dock_panel 会绕过它，插件面板永远停在「…」占位。
                // dock="tab" 形态同步路由到预览页签（与状态栏点击同口径）
                if chat
                    .plugins
                    .plugins
                    .get(plugin_ix)
                    .is_none_or(|e| e.disabled)
                {
                    return Err(bad(format!("插件 ix={plugin_ix} 不存在或已停用")));
                }
                if chat.plugin_dock_is_tab(plugin_ix) {
                    chat.open_plugin_tab(plugin_ix, cx);
                } else {
                    chat.open_plugin_dock(plugin_ix, cx);
                }
                return ok();
            }
            chat.layout.dock_panel = DockPanel::from_dock_arg(panel, plugin_ix)
                .ok_or_else(|| bad(format!("未知 dock 面板: {panel:?}")))?;
            cx.notify();
            ok()
        }
        method::CONTENT_VIEW => {
            let view = params
                .get("view")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"view\": \"chat|term|file\"}"))?;
            match view {
                "chat" => chat.set_content_view(ContentView::Chat),
                "term" => chat.set_content_view(ContentView::Term),
                "file" => chat.set_content_view(ContentView::File),
                // 040-1：市场页走真开页路径（建搜索框 + ensure_loaded 拉首页）
                "pi-native-market" => chat.open_pi_native_market(cx),
                // 090-4：PF 插件市场同口径（建输入框 + 重读台账）
                "pf-market" => chat.open_pf_market(cx),
                "search" => chat.open_project_search(cx),
                other => return Err(bad(format!("未知内容视图: {other:?}"))),
            }
            cx.notify();
            ok()
        }
        method::TERMINAL_OPEN => {
            // 打开（或聚焦）工作区终端；键入回显断言读 term 快照面
            chat.open_terminal(None, window, cx);
            Ok(json!({
                "ok": true,
                "terminals": chat.ws.terminals.len(),
                "active_terminal": chat.ws.active_terminal,
                "content_view": match chat.layout.content_view {
                    ContentView::Chat => "chat",
                    ContentView::Term => "term",
                    ContentView::File => "file",
                    ContentView::Changelog => "changelog",
                    ContentView::PiNativeMarket => "pi-native-market",
                    ContentView::PfMarket => "pf-market",
                    ContentView::Search => "search",
                    ContentView::SessionSearch => "session_search",
                    ContentView::Plugin => "plugin",
                },
            }))
        }

        _ => Err((
            "unknown_method".into(),
            format!("未知 method: {m}（清单见 pi_link::automation::method）"),
        )),
    }
}
