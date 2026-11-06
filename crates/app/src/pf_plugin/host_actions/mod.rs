//! PF 插件宿主 Chat 方法·动作侧（090 重构自 mod.rs 拆出）。
//!
//! 文件组织（超规二次拆分，impl Chat 块形式）：
//! - 本文件：右键菜单 / 命令回环 / 动作分派（dispatch）与 wasm 动作分派 /
//!   快照同步 / 异常熔断（plugin_broken）
//! - [`wasm`]：wasm 事件桥（render / page-active / broadcast / on-event）
//! - [`popup`]：弹窗生命周期（open/close/poll/tick/reload）
//! - [`fn_bar`]：会话功能栏 chrome 与点击回环 + 页面元素装配
//! - [`ext_mount`]：包内 pi 扩展挂载 / 摘除 / 存活判定（闲置回收豁免）
//! - [`badge`]：会话列表徽标（plugin_badge）

mod badge;
mod ext_mount;
mod fn_bar;
mod popup;
mod wasm;

pub(crate) use fn_bar::fn_bar_plugin_elements;

use std::path::{Path, PathBuf};

use pf_plugin_host::PluginAction;
use pf_plugin_host::js::JsAction;

use crate::Chat;

use super::*;

impl Chat {
    /// 右键菜单点击：onMenu(id, {path, fileIcon}) → 动作分派 → 面板开着则重渲染。
    pub(crate) fn plugin_menu_fire(
        &mut self,
        ix: usize,
        menu_id: &str,
        path: &Path,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self.plugin_ready(ix, cx) {
            return;
        }
        // 内层 wasm 插件：同一事件通道（on-event kind=menu）
        if self.plugins.plugins[ix].wasm.is_some() {
            let p = path.to_string_lossy().into_owned();
            self.plugin_wasm_event(ix, "menu", menu_id, &p, window, cx);
            return;
        }
        let (result, actions) = {
            let entry = &mut self.plugins.plugins[ix];
            let host = entry.host.as_ref().unwrap().clone();
            host.borrow_mut().actions.clear();
            let fi = crate::services::file_icons::get_icon(path).to_string();
            let js = entry.js.as_ref().unwrap();
            let r = js.fire_menu(menu_id, &path.to_string_lossy(), &fi);
            let drained = host.borrow_mut().actions.drain(..).collect::<Vec<_>>();
            (r, drained)
        };
        match result {
            Ok(()) => self.plugin_dispatch(ix, actions, window, cx),
            Err(e) => self.plugin_broken(ix, e, cx),
        }
    }

    /// 组件菜单项回调（01 §4.2：不带 action 的 menu 项 → 插件改自己的数据）。
    /// 返回后由 `plugin_dispatch` 统一 `plugin_refresh` 重渲染（02 §3 重渲染规则）。
    pub(crate) fn plugin_command_fire(
        &mut self,
        ix: usize,
        command_id: &str,
        path: &str,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self.plugin_ready(ix, cx) {
            return;
        }
        // 内层 wasm 插件：同一事件通道（on-event kind=command，path 裸串与
        // JS fire_command 同面）。上一版缺这个分支：wasm 条目 entry.js = None，
        // 下方 unwrap 直接 panic——且 panic 发生在 window_procedure 里
        // cannot unwind，app 整体退出（quick-panel 右键「移除快捷」必崩，
        // 真机 2026-10-31）。姊妹函数（refresh/menu_fire/command_fire_env/
        // broadcast/page_active）都有 wasm 早退分支，唯此漏。
        if self.plugins.plugins[ix].wasm.is_some() {
            self.plugin_wasm_event(ix, "command", command_id, path, window, cx);
            return;
        }
        let (result, actions) = {
            let entry = &mut self.plugins.plugins[ix];
            let host = entry.host.as_ref().unwrap().clone();
            host.borrow_mut().actions.clear();
            let js = entry.js.as_ref().unwrap();
            let r = js.fire_command(command_id, path);
            let drained = host.borrow_mut().actions.drain(..).collect::<Vec<_>>();
            (r, drained)
        };
        match result {
            Ok(()) => self.plugin_dispatch(ix, actions, window, cx),
            Err(e) => self.plugin_broken(ix, e, cx),
        }
    }

    /// 命令回调（env 负载形态，契约 v3）：与 plugin_command_fire 同链，
    /// JS 走 fire_command_env，wasm 走 on-event（env 串放 path 位）。
    pub(crate) fn plugin_command_fire_env(
        &mut self,
        ix: usize,
        command_id: &str,
        env_json: &str,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self.plugin_ready(ix, cx) {
            return;
        }
        // 命令回环后插件状态已变（绑定指针/开关态）：chrome 缓存作废重建
        // ——两载体同时机（8tb.2：wasm 命令回环原先不清，导致绑定后 icon 滞灰）
        self.plugins.fn_bar_cache.clear();
        if self.plugins.plugins[ix].wasm.is_some() {
            self.plugin_wasm_event(ix, "command", command_id, env_json, window, cx);
            return;
        }
        let (result, actions) = {
            let entry = &mut self.plugins.plugins[ix];
            let host = entry.host.as_ref().unwrap().clone();
            host.borrow_mut().actions.clear();
            let js = entry.js.as_ref().unwrap();
            let r = js.fire_command_env(command_id, env_json);
            let drained = host.borrow_mut().actions.drain(..).collect::<Vec<_>>();
            (r, drained)
        };
        match result {
            Ok(()) => self.plugin_dispatch(ix, actions, window, cx),
            Err(e) => self.plugin_broken(ix, e, cx),
        }
    }

    /// 动作分派（02 §3 动作类 → 宿主实现；run 目标不可执行 → toast 报错）。
    pub(crate) fn plugin_dispatch(
        &mut self,
        ix: usize,
        actions: Vec<JsAction>,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        for action in actions {
            match action {
                JsAction::Run(p) => {
                    let path = PathBuf::from(&p);
                    match crate::services::script_exec::resolve(&path, self.ws.cwd.as_deref()) {
                        Some(plan) => self.open_script_terminal(path, plan, window, cx),
                        None => self.set_status(format!("无法执行：{}", p), cx),
                    }
                }
                JsAction::Edit(p) => self.open_file_tab(PathBuf::from(&p), cx),
                // 契约 v3 sessions.setBadge / clearBadge：会话列表时间位徽标
                JsAction::SetBadge {
                    session,
                    icon,
                    tone,
                } => {
                    if !session.is_empty() {
                        let owner = self
                            .plugins
                            .plugins
                            .get(ix)
                            .map(|e| e.manifest.id.clone())
                            .unwrap_or_default();
                        // 一个插件只允许一条徽标（002：只绑一个会话）——换绑即覆盖，
                        // 否则列表里会残留旧会话的绿标
                        self.plugins.badges.retain(|b| b.owner != owner);
                        self.plugins.badges.push(PluginBadge {
                            owner,
                            session,
                            icon,
                            tone,
                        });
                        cx.notify();
                    }
                }
                JsAction::ClearBadge { session } => {
                    let owner = self
                        .plugins
                        .plugins
                        .get(ix)
                        .map(|e| e.manifest.id.clone())
                        .unwrap_or_default();
                    let before = self.plugins.badges.len();
                    if session.is_empty() {
                        self.plugins.badges.retain(|b| b.owner != owner);
                    } else {
                        self.plugins
                            .badges
                            .retain(|b| !(b.owner == owner && b.session == session));
                    }
                    if self.plugins.badges.len() != before {
                        cx.notify();
                    }
                }
                JsAction::RevealInExplorer(p) => {
                    crate::services::shell_open::reveal_in_explorer(Path::new(&p))
                }
                JsAction::Toast(m) => self.set_status(m, cx),
                JsAction::OpenUrl(u) => crate::services::shell_open::open_url(&u),
                JsAction::OpenFolder(p) => {
                    crate::services::shell_open::open_in_explorer(std::path::Path::new(&p))
                }
                JsAction::NewSession => self.new_session(cx),
                // 意图调用：新建插件认领会话（契约 v2.4 sessions）。同步无返回值，
                // 建线后经 broadcast "claim" 事件回推 key（动作队列模型）
                JsAction::NewDraft { prefix, cwd } => {
                    if let Err(e) = self.plugin_new_draft(ix, &prefix, &cwd, cx) {
                        self.set_status(format!("newDraft 失败：{e}"), cx);
                    }
                }
                // 只预填 + 聚焦：发不发由用户按发送键决定（插件不代发）
                JsAction::AskPi(text) => match self.composer.composer.clone() {
                    Some(input) => {
                        let fh = input.update(cx, |i, cx| {
                            i.set_value(&text, cx);
                            i.focus_handle_in(cx)
                        });
                        fh.focus(window);
                    }
                    None => self.set_status(text, cx),
                },
                // 契约 v3 popups：开/关插件弹窗（非模态，宿主管窗体）
                JsAction::OpenPopup(id) => self.plugin_popup_open(ix, Some(&id), window, cx),
                JsAction::ClosePopup(id) => {
                    if self.plugins.popup.as_ref().map(|p| (p.ix, p.id.as_str()))
                        == Some((ix, id.as_str()))
                    {
                        self.plugin_popup_close(cx);
                    }
                }
            }
        }
        // 插件数据可能被 onMenu/onCommand 改过：无条件重 render（独立窗口
        // 靠 observe(Chat) 自动重渲，02 §4）
        self.plugin_refresh(ix, cx);
    }

    /// wasm `PluginAction` → 宿主实现（与 JS 动作同款落点，复用实现不复制逻辑）。
    pub(crate) fn plugin_wasm_dispatch(
        &mut self,
        ix: usize,
        actions: Vec<PluginAction>,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        for a in actions {
            match a {
                PluginAction::Toast(m) => self.set_status(m, cx),
                PluginAction::OpenFile(p) => self.open_file_tab(PathBuf::from(&p), cx),
                // 意图调用（契约 v2.4 sessions）：与 JS 分支同款落点——建线
                // 后经 broadcast "claim" 回推，失败经状态栏透传
                PluginAction::NewDraft { prefix, cwd } => {
                    if let Err(e) = self.plugin_new_draft(ix, &prefix, &cwd, cx) {
                        self.set_status(format!("newDraft 失败：{e}"), cx);
                    }
                }
                PluginAction::RunScript(p) => {
                    let path = PathBuf::from(&p);
                    match crate::services::script_exec::resolve(&path, self.ws.cwd.as_deref()) {
                        Some(plan) => self.open_script_terminal(path, plan, window, cx),
                        None => self.set_status(format!("无法执行：{p}"), cx),
                    }
                }
                // 契约 v3 act.openPopup：与 JS JsAction::OpenPopup 同落点
                PluginAction::OpenPopup(id) => self.plugin_popup_open(ix, Some(&id), window, cx),
            }
        }
        // 动作可能改了插件数据（如 toggle pin）：同步快照给后续 render
        self.plugin_sync_quick_pins();
    }

    /// 快照当前项目收藏 → host 钩子（fn 指针拿不到 Chat，走 thread_local 桥）。
    pub(crate) fn plugin_sync_quick_pins(&self) {
        let pins: Vec<String> = self
            .ws
            .pinned
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        QUICK_PIN_SNAP.with(|c| *c.borrow_mut() = pins);
    }

    /// 插件异常隔离（02 §4）：捕获 → toast → 会话级禁用，不影响宿主。
    pub(crate) fn plugin_broken(&mut self, ix: usize, err: String, cx: &mut gpui::Context<Self>) {
        eprintln!("[pf-plugin] 插件熔断 ix={ix}: {err}");
        let Some(entry) = self.plugins.plugins.get_mut(ix) else {
            return;
        };
        entry.disabled = true;
        entry.js = None;
        entry.wasm = None;
        entry.wasm_focus = None;
        entry.host = None;
        let id = entry.manifest.id.clone();
        self.set_status(format!("插件 {id} 出错：{err}"), cx);
    }
}
