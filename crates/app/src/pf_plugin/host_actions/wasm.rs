//! PF 插件宿主——wasm 事件桥（加密包）：render 回灌 / page-active 钩子 /
//! broadcast 派发 / 组件事件 on-event。与 JS 插件同一条渲染管线（树解释器），
//! wasm 不另立渲染模型（Paint 直绘已废）。

use pf_plugin_host::PluginAction;

use crate::Chat;

use super::*;

impl Chat {
    /// wasm `render()` → 组件树（与 JS 同一 schema，`panel::resolve` 统一渲染）。
    pub(crate) fn plugin_wasm_render(&mut self, ix: usize, cx: &mut gpui::Context<Self>) {
        let result = {
            let entry = &mut self.plugins.plugins[ix];
            entry
                .wasm
                .as_mut()
                .map(|i| i.render_json())
                .unwrap_or_else(|| Ok(String::new()))
        };
        match result {
            Ok(json) => {
                let v: serde_json::Value =
                    serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
                let entry = &mut self.plugins.plugins[ix];
                entry.body = Some(v);
                entry.tree_loaded = true;
                cx.notify();
            }
            Err(e) => self.plugin_broken(ix, format!("{e}"), cx),
        }
    }

    /// 组件状态广播派发（契约 v2.4 broadcast_events）。纯数据回调：
    /// 窗口相关动作（run/edit/终端）在无窗口面不可达即丢弃，Toast 照走；
    /// 处理完照重渲染规则刷新面板（wasm = render 回灌）。
    /// PF 钩子 page_actived：把「当前页会话 key」推给每个 JS 插件
    /// （PF 独有概念，pi 进程没有“当前页”；插件据此记住身份，禁止再猜）。
    pub(crate) fn plugin_page_active(
        &mut self,
        session_key: &str,
        cx: &mut gpui::Context<Self>,
    ) -> usize {
        if session_key.is_empty() {
            return 0;
        }
        let env = serde_json::json!({ "session": { "key": session_key } }).to_string();
        let mut notified = 0usize;
        let n = self.plugins.plugins.len();
        for ix in 0..n {
            // 能力面（8tb.2）：PF 钩子全员派发，载体无关——JS 未导出由胶水
            // 回 "0" 记日志；wasm 走事件通道 kind="page-active"（wit v3；
            // 旧 wasm 不认识即忽略，无害）
            let ok = {
                let e = &self.plugins.plugins[ix];
                !e.disabled
            };
            if !ok || !self.plugin_ready(ix, cx) {
                continue;
            }
            let is_wasm = self.plugins.plugins[ix].wasm.is_some();
            let r: Result<String, String> = if is_wasm {
                let (res, acts) = {
                    let entry = &mut self.plugins.plugins[ix];
                    let Some(inst) = entry.wasm.as_mut() else {
                        continue;
                    };
                    let res = inst.on_event("page-active", "", &env);
                    let acts = inst.pump_actions();
                    (res, acts)
                };
                // 无 window 面：Toast 照走，其余丢弃（broadcast 同口径；
                // page-active 只作废 chrome/徽标缓存，不触发面板重渲）
                for a in acts {
                    match a {
                        PluginAction::Toast(m) => self.set_status(m, cx),
                        other => {
                            eprintln!("[pf-plugin] page-active 期间动作被忽略: {other:?}")
                        }
                    }
                }
                res.map(|()| "1".to_string()).map_err(|e| e.to_string())
            } else {
                let entry = &self.plugins.plugins[ix];
                let host = entry.host.as_ref().unwrap().clone();
                host.borrow_mut().actions.clear();
                let r = entry.js.as_ref().unwrap().page_active(&env);
                host.borrow_mut().actions.clear();
                r
            };
            match r {
                Err(e) => self.plugin_broken(ix, e, cx),
                Ok(hit) => {
                    if hit == "1" {
                        notified += 1;
                    } else if cfg!(debug_assertions) {
                        eprintln!("[pf-plugin] page_actived 未导出（ix={ix}）");
                    }
                }
            }
        }
        self.plugins.row_cache.clear();
        self.plugins.fn_bar_cache.clear();
        // 每拍派发的追踪行，只在 debug 构建打：release 包零输出是烟测门禁
        // （0.3.2 实录：这一行 3 秒刷 ~50 条直接把烟测打红）
        if cfg!(debug_assertions) {
            eprintln!(
                "[pf-plugin] page_actived 派发 key={} 插件数={} 通知={}",
                session_key, n, notified
            );
        }
        notified
    }

    pub(crate) fn plugin_broadcast_fire(
        &mut self,
        ix: usize,
        name: &str,
        payload: &str,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self.plugin_ready(ix, cx) {
            return;
        }
        if self.plugins.plugins[ix].wasm.is_some() {
            let (res, actions) = {
                let entry = &mut self.plugins.plugins[ix];
                let Some(inst) = entry.wasm.as_mut() else {
                    return;
                };
                (
                    inst.on_event("broadcast", name, payload),
                    inst.pump_actions(),
                )
            };
            match res {
                Ok(()) => {
                    for a in actions {
                        match a {
                            PluginAction::Toast(m) => self.set_status(m, cx),
                            // 意图调用可达（newDraft 无窗口依赖）；窗口类动作
                            // 丢弃（broadcast 纯数据回调口径，与 JS 分支一致）
                            PluginAction::NewDraft { prefix, cwd } => {
                                if let Err(e) = self.plugin_new_draft(ix, &prefix, &cwd, cx) {
                                    self.set_status(format!("newDraft 失败：{e}"), cx);
                                }
                            }
                            _ => {}
                        }
                    }
                    self.plugin_wasm_render(ix, cx);
                }
                Err(e) => self.plugin_broken(ix, format!("{e}"), cx),
            }
            return;
        }
        let (result, actions) = {
            let entry = &mut self.plugins.plugins[ix];
            let host = entry.host.as_ref().unwrap().clone();
            host.borrow_mut().actions.clear();
            let js = entry.js.as_ref().unwrap();
            let r = js.fire_broadcast(name, payload);
            let drained = host.borrow_mut().actions.drain(..).collect::<Vec<_>>();
            (r, drained)
        };
        match result {
            Ok(()) => {
                for a in actions {
                    if let JsAction::Toast(m) = a {
                        self.set_status(m, cx);
                    }
                }
                self.plugin_refresh(ix, cx);
            }
            Err(e) => self.plugin_broken(ix, e, cx),
        }
    }

    /// wasm 面板事件回灌（`contributes.*` 回调统一走这里 → on-event）。
    /// 事件处理完必重渲染（插件数据可能改了）。
    pub(crate) fn plugin_wasm_event(
        &mut self,
        ix: usize,
        kind: &str,
        id: &str,
        path: &str,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.plugin_sync_quick_pins();
        let (res, actions) = {
            let entry = &mut self.plugins.plugins[ix];
            let Some(inst) = entry.wasm.as_mut() else {
                return;
            };
            (inst.on_event(kind, id, path), inst.pump_actions())
        };
        match res {
            Ok(()) => {
                self.plugin_wasm_dispatch(ix, actions, window, cx);
                self.plugin_wasm_render(ix, cx);
            }
            Err(e) => self.plugin_broken(ix, format!("{e}"), cx),
        }
    }
}
