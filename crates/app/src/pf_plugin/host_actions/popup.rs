//! PF 插件宿主——弹窗生命周期（契约 v3 popups）：打开 / 关闭 / 1s 重取轮询
//! （挂 120ms 主泵）/ 内容重取与伴随动作落地。

use pf_plugin_host::PluginAction;
use pf_plugin_host::js::JsAction;

use crate::Chat;

use super::*;

impl Chat {
    /// 弹窗打开（契约 v3）：内容 = render_popup(ctx)（wasm = render()）；
    /// 一次一个（开新的关旧的）。同插件重复 open = 重取内容。
    pub(crate) fn plugin_popup_open(
        &mut self,
        ix: usize,
        popup_id: Option<&str>,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self.plugin_ready(ix, cx) {
            return;
        }
        // 代数自增：杀掉上一轮 poll（重复 open 不堆叠 1s 轮询循环）
        self.plugins.popup_gen += 1;
        let (id, title) = {
            let entry = &self.plugins.plugins[ix];
            let pc = match popup_id {
                Some(id) => entry.manifest.popups.iter().find(|p| p.id == id),
                None => entry.manifest.popups.first(),
            };
            let Some(pc) = pc else {
                self.set_status(
                    format!(
                        "插件 {} 没有弹窗 {}",
                        entry.manifest.id,
                        popup_id.unwrap_or("")
                    ),
                    cx,
                );
                return;
            };
            (pc.id.clone(), pick_lang(&pc.title_zh, &pc.title_en))
        };
        // 能力面取体（8tb.2）：JS render_popup / wasm render-popup（未导出
        // 退回 render 面）同一取数口，解析与开窗逻辑不分支。
        let (result, js_actions, wasm_actions) = {
            let entry = &mut self.plugins.plugins[ix];
            entry.hook_render_popup()
        };
        match result {
            Ok(json) => {
                let body = serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
                // 契约约定：render_popup 返回 null = 此刻无弹窗内容（如绑定
                // 已成功）→ 不开/自动关
                if body.is_null() {
                    self.plugin_popup_close(cx);
                    return;
                }
                self.plugins.popup = Some(PluginPopupOpen {
                    ix,
                    id,
                    title,
                    tree_loaded: true,
                    body: Some(body),
                });
                // bind 回调里 openPopup 之外若还带了别的动作，照走；
                // wasm 意图（open-popup）同落点
                self.plugin_dispatch(ix, js_actions, window, cx);
                if !wasm_actions.is_empty() {
                    self.plugin_wasm_dispatch(ix, wasm_actions, window, cx);
                }
            }
            Err(e) => self.plugin_broken(ix, e, cx),
        }
        cx.notify();
        let popup_gen = self.plugins.popup_gen;
        self.plugin_popup_poll(ix, popup_gen, cx);
    }

    /// 弹窗关闭（随手关：点外/×；act.closePopup 同路）。
    pub(crate) fn plugin_popup_close(&mut self, cx: &mut gpui::Context<Self>) {
        if cfg!(debug_assertions) {
            eprintln!("[pf-plugin] popup close (trace)");
        }
        if self.plugins.popup.take().is_some() {
            self.plugins.popup_gen += 1;
            // chrome 缓存作废：弹窗动作（如绑定成功）→ 弹窗自动收，此刻功能栏
            // icon 必须立刻变色。不失效就要等下一个会话事件才亮
            self.plugins.fn_bar_cache.clear();
            cx.notify();
        }
    }

    /// 弹窗打开期间的低频重取（1s，旧版宿主泵采样 parity）：ext 写镜像
    /// （过期白遮/新码/开关态）无回调链可触发，轮询是弹窗场景唯一时效保障；
    /// 弹窗关闭即停。停用/坏插件自动断流。
    pub(crate) fn plugin_popup_poll(
        &mut self,
        ix: usize,
        popup_gen: u64,
        _cx: &mut gpui::Context<Self>,
    ) {
        // 重取节拍挂到 120ms 主泵（startup::spawn_pump_120ms → popup_tick）。
        // 早先在这里自起 cx.spawn 循环，实测「首拍后即不再进」——弹窗内容永远
        // 停在打开那一刻的旧帧，画布也就永远不带 refresh-qr 命令：超时白遮后
        // 点画布毫无反应。不再自开节拍。
        self.plugins.popup_poll_gen = popup_gen;
        self.plugins.popup_poll_ix = Some(ix);
        self.plugins.popup_poll_at = Some(std::time::Instant::now());
    }

    /// 弹窗 1s 重取（由 120ms 主泵驱动；弹窗关/gen 变即自动失效）。
    pub(crate) fn plugin_popup_tick(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(ix) = self.plugins.popup_poll_ix else {
            return;
        };
        let Some(p) = self.plugins.popup.as_ref() else {
            self.plugins.popup_poll_ix = None;
            return;
        };
        if p.ix != ix || !p.tree_loaded || self.plugins.popup_gen != self.plugins.popup_poll_gen {
            self.plugins.popup_poll_ix = None;
            return;
        }
        let due = self
            .plugins
            .popup_poll_at
            .map(|t| t.elapsed() >= std::time::Duration::from_millis(1000))
            .unwrap_or(true);
        if !due {
            return;
        }
        self.plugins.popup_poll_at = Some(std::time::Instant::now());
        if cfg!(debug_assertions) {
            eprintln!("[popup-tick] reload ix={ix}");
        }
        self.plugin_popup_reload(ix, cx);
    }

    /// 弹窗内容重取（回调链返回后的重渲染规则：弹窗开着就跟着新态）。
    /// 能力面取体（8tb.2）：JS/wasm 同一取数口，解析不分支。
    pub(crate) fn plugin_popup_reload(&mut self, ix: usize, cx: &mut gpui::Context<Self>) {
        if self.plugins.popup.as_ref().map(|p| p.ix) != Some(ix) {
            return;
        }
        // 防御：js/wasm 可能已被异常隔离（plugin_broken）或重扫描清掉——
        // 弹窗场景绝不打崩宿主，收弹窗即可
        if !self.plugin_ready(ix, cx) {
            self.plugin_popup_close(cx);
            return;
        }
        let (result, actions, wasm_actions) = {
            let entry = &mut self.plugins.plugins[ix];
            entry.hook_render_popup()
        };
        // wasm 意图：本面无 window，只放行 Toast，其余丢弃（broadcast 同口径；
        // 弹窗体不该再开弹窗）
        for a in wasm_actions {
            match a {
                PluginAction::Toast(m) => self.set_status(m, cx),
                other => eprintln!("[pf-plugin] popup 重取期间 wasm 动作被忽略: {:?}", other),
            }
        }
        // 重取期间的动作：徽标是渲染的伴随声明，必须落地（否则列表永远没徽标）；
        // 其余动作忽略（与 render 面同口径）。
        for a in actions {
            match a {
                JsAction::SetBadge {
                    session,
                    icon,
                    tone,
                } => {
                    if !session.is_empty() {
                        let owner = self.plugins.plugins[ix].manifest.id.clone();
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
                    let owner = self.plugins.plugins[ix].manifest.id.clone();
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
                other => eprintln!("[pf-plugin] popup 重取期间的动作被忽略: {other:?}"),
            }
        }
        match result {
            Ok(json) => {
                let body: serde_json::Value =
                    serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
                // null = 插件判定弹窗该收了（如绑定成功瞬间）→ 自动关
                if body.is_null() {
                    self.plugin_popup_close(cx);
                    return;
                }
                if cfg!(debug_assertions) {
                    eprintln!(
                        "[popup-tick] body={}",
                        serde_json::to_string(&body)
                            .unwrap_or_default()
                            .chars()
                            .take(120)
                            .collect::<String>()
                    );
                }
                if let Some(p) = self.plugins.popup.as_mut() {
                    p.body = Some(body);
                    p.tree_loaded = true;
                }
                cx.notify();
            }
            Err(e) => self.plugin_broken(ix, e, cx),
        }
    }
}
