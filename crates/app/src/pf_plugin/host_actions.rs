//! PF 插件宿主 Chat 方法·动作侧（090 重构自 mod.rs 拆出）：右键菜单 /
//! 命令回环 / fn_bar / 弹窗生命周期 / 动作分派（dispatch）与 wasm 事件桥 /
//! 会话徽标（plugin_badge）/ 会话功能栏插件 icon。

use std::path::{Path, PathBuf};

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};
use pf_plugin_host::PluginAction;
use pf_plugin_host::js::JsAction;
use serde_json::Value;

use crate::Chat;
use crate::ui::tooltip::WithTip;

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

    /// 契约 v3 fn_bar：会话功能栏插件 icon 批量取数（带 1s TTL 缓存——
    /// 功能栏每帧渲染，调用不进每帧）。按能力面取（清单 fn_bar 声明 +
    /// 插件导出，载体无关）；停用/坏插件不出。
    pub(crate) fn plugin_fn_bar_chrome(
        &mut self,
        session_key: &str,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<(usize, SharedString, String, String)> {
        const TTL: std::time::Duration = std::time::Duration::from_millis(1000);
        let mut out = Vec::new();
        let mut miss: Vec<usize> = Vec::new();
        for (ix, e) in self.plugins.plugins.iter().enumerate() {
            if e.manifest.fn_bar.is_none() || e.disabled {
                continue;
            }
            let hit = self.plugins.fn_bar_cache.iter().find(|c| {
                c.pkg == e.manifest.pkg && c.session_key == session_key && c.at.elapsed() < TTL
            });
            match hit {
                Some(c) => {
                    let path = contrib_icon_path(&e.manifest.id, &c.icon);
                    out.push((ix, path, c.tone.clone(), c.tip.clone()));
                }
                None => miss.push(ix),
            }
        }
        for ix in miss {
            // 未就绪的插件不阻塞功能栏（懒加载链；首拍拿不到下一拍再试）
            if !self.plugin_ready(ix, cx) {
                continue;
            }
            let pkg = self.plugins.plugins[ix].manifest.pkg.clone();
            let env = serde_json::json!({ "session": { "key": session_key } }).to_string();
            let (result, actions) = {
                let entry = &mut self.plugins.plugins[ix];
                entry.hook_fn_bar(&env)
            };
            // chrome 取数期间的动作丢弃（fn_bar 是纯 chrome，不该有副作用）。
            // 例外：会话列表徽标（SetBadge/ClearBadge）就是 chrome 的伴随声明——
            // 不放行的话，已绑会话的列表 icon 永远打不上（002 时间位徽标）。
            for a in &actions {
                match a {
                    JsAction::SetBadge {
                        session,
                        icon,
                        tone,
                    } => {
                        if !session.is_empty() {
                            let owner = self.plugins.plugins[ix].manifest.id.clone();
                            self.plugins.badges.retain(|b| b.owner != owner); // 一插件一条
                            self.plugins.badges.push(PluginBadge {
                                owner,
                                session: session.clone(),
                                icon: icon.clone(),
                                tone: tone.clone(),
                            });
                        }
                    }
                    JsAction::ClearBadge { session } => {
                        let owner = self.plugins.plugins[ix].manifest.id.clone();
                        if session.is_empty() {
                            self.plugins.badges.retain(|b| b.owner != owner);
                        } else {
                            self.plugins
                                .badges
                                .retain(|b| !(b.owner == owner && b.session == *session));
                        }
                    }
                    _ => eprintln!("[pf-plugin] fn_bar 期间的动作被忽略: {a:?}"),
                }
            }
            match result {
                Ok(Some(json)) => {
                    let v: serde_json::Value =
                        serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
                    let icon = v
                        .get("icon")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let tone = v
                        .get("tone")
                        .and_then(Value::as_str)
                        .unwrap_or("text.faint")
                        .to_string();
                    // tip 是文案对象 {zh,en} 或纯字符串，统一解成本语言串
                    let tip = match v.get("tip") {
                        Some(Value::Object(_)) => {
                            let zh = v["tip"]["zh"].as_str().unwrap_or("").to_string();
                            let en = v["tip"]["en"].as_str().map(|s| s.to_string());
                            pick_lang(&zh, &en)
                        }
                        Some(Value::String(s)) => s.clone(),
                        _ => String::new(),
                    };
                    self.plugins
                        .fn_bar_cache
                        .retain(|c| !(c.pkg == pkg && c.session_key == session_key));
                    self.plugins.fn_bar_cache.push(FnBarCache {
                        pkg: pkg.clone(),
                        session_key: session_key.to_string(),
                        icon: icon.clone(),
                        tone: tone.clone(),
                        tip: tip.clone(),
                        at: std::time::Instant::now(),
                    });
                    if !icon.is_empty() {
                        let path = contrib_icon_path(&self.plugins.plugins[ix].manifest.id, &icon);
                        out.push((ix, path, tone, tip));
                    }
                }
                // 未导出 fn_bar：负缓存一轮（TTL 内不再问）
                Ok(None) => self.plugins.fn_bar_cache.push(FnBarCache {
                    pkg: pkg.clone(),
                    session_key: session_key.to_string(),
                    icon: String::new(),
                    tone: String::new(),
                    tip: String::new(),
                    at: std::time::Instant::now(),
                }),
                Err(e) => self.plugin_broken(ix, e, cx),
            }
        }
        out
    }

    /// fn_bar icon 点击：command 回环（env 带 {session:{key}}，契约 v3）。
    /// 绑定目标 = 当前会话：先把包内 pi 扩展写进该会话扩展清单（出码/收发
    /// 本体在 ext，004 §5.2），进程空闲即重启挂载，再回环命令。
    pub(crate) fn plugin_fn_bar_click(
        &mut self,
        ix: usize,
        session_key: &str,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        // v3 面（回滚）：绑定目标 = 当前会话——先把包内 pi 扩展写进该会话
        // 扩展清单（出码/收发本体在 ext），进程空闲即重启挂载，再回环命令。
        // 执行域（v4）代码保留但仅对声明 contributes.service 的包生效；
        // v3 包走 plugin_attach_pi_exts（v3 镜像协议回滚，2026-10-30 用户令）。
        {
            let has_service = self
                .plugins
                .plugins
                .get(ix)
                .map(|e| !e.manifest.service.is_empty())
                .unwrap_or(false);
            if !has_service {
                let pf_key = self.pool.active_key.clone();
                self.plugin_attach_pi_exts(ix, &pf_key, cx);
                self.ensure_active_process(cx);
            }
        }
        let Some(cmd) = self
            .plugins
            .plugins
            .get(ix)
            .and_then(|e| e.manifest.fn_bar.as_ref())
            .map(|f| f.command.clone())
        else {
            return;
        };
        let env = serde_json::json!({ "session": { "key": session_key } }).to_string();
        self.plugin_command_fire_env(ix, &cmd, &env, window, cx);
    }

    /// 该会话是否有「绑定中的服务实例」（闲置回收豁免判据）：
    /// 注册表里有 token + 实例配置在位即算——绑定存在 = 轮询器必须活着。
    pub(crate) fn has_live_service(&self, key: &str, _cx: &mut gpui::Context<Self>) -> bool {
        let Some(base) = pi_link::paths::dir() else {
            return false;
        };
        pi_link::service_ext::config_path_if_any(&base, key).is_some()
    }

    /// 摘除活跃运行时里本插件的 ext（attach 逆操作，90-9 §8）：evict/停用后
    /// 运行中的进程还带着已挂载 ext——空闲即就地重启摘掉；忙 → spawn 前过滤
    /// （ext_source_registered）+ 清单已 purged，下次 spawn 生效。
    pub(crate) fn plugin_detach_active_ext(&mut self, ix: usize, cx: &mut gpui::Context<Self>) {
        let ext_paths: Vec<String> = match self.plugins.plugins.get(ix) {
            Some(e) => e
                .manifest
                .pi_extensions
                .iter()
                .map(|rel| e.manifest.dir.join(rel).to_string_lossy().into_owned())
                .collect(),
            None => return,
        };
        if ext_paths.is_empty() {
            return;
        }
        let key = self.pool.active_key.clone();
        let Some(rt) = self.pool.runtimes.get(&key).cloned() else {
            return;
        };
        let busy = rt.read(cx).agent_running;
        if busy {
            return; // 下次 spawn 生效
        }
        rt.update(cx, |r, cx| {
            r.ext_sources.retain(|e| !ext_paths.iter().any(|p| e == p));
            r.shutdown_process();
            r.ensure_spawned(cx);
        });
    }

    /// 包内 pi 扩展 → 当前会话扩展清单（契约 v3：绑定目标 = 用户当前普通
    /// 会话，挂载改在绑定时完成）。会话进程空闲 → 立即重启挂载；忙 → 清单
    /// 已写，下次 spawn 生效。
    pub(crate) fn plugin_attach_pi_exts(
        &mut self,
        ix: usize,
        pf_session_key: &str,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(entry) = self.plugins.plugins.get(ix) else {
            return;
        };
        if entry.manifest.pi_extensions.is_empty() || pf_session_key.is_empty() {
            return;
        }
        let pi_exts: Vec<String> = entry
            .manifest
            .pi_extensions
            .iter()
            .map(|rel| entry.manifest.dir.join(rel).to_string_lossy().into_owned())
            .collect();
        // 台账**合并写**（勿整表覆写）：会话自选的 npm 清单与插件挂载通道
        // 同住一份台账，覆写 = 把用户勾选抹掉（真机 2026-10-31：观宏知微
        // 会话台账只剩一条插件路径，就是被覆写没的）
        let Some(store) = pi_link::session_ext::store_path() else {
            return;
        };
        let mut merged = pi_link::session_ext::read_for(&store, pf_session_key);
        if pi_exts.iter().all(|p| merged.iter().any(|h| h == p)) {
            return; // 清单已含，无需重启
        }
        for p in &pi_exts {
            if !merged.iter().any(|h| h == p) {
                merged.push(p.clone());
            }
        }
        if let Err(e) = pi_link::session_ext::write_for(&store, pf_session_key, &merged) {
            eprintln!("[pf-plugin] pi 扩展挂载失败（{pf_session_key}）: {e}");
            return;
        }
        eprintln!(
            "[pf-plugin] pi 扩展已入会话清单（{} → {}），空闲重启挂载",
            entry.manifest.id, pf_session_key
        );
        // 空闲会话立即重启挂载（忙 = 有任务在跑，不动，下次 spawn 生效）。
        // **不 push 进 ext_sources**：挂载本体在 spawn 前按台账统一 `-e`
        // （runtime spawn 的 store 消费段，所有档位）；ext_sources 只装用户
        // 勾选的 scheme 源——路径型条目混进去会把胶囊计数和勾选面板搞劈叉
        // （真机 2026-10-31：面板勾 2 项、胶囊显 [3]）
        if let Some(rt) = self.pool.runtimes.get(pf_session_key).cloned() {
            let busy = rt.read(cx).agent_running;
            if !busy {
                rt.update(cx, |r, cx| {
                    r.shutdown_process();
                    r.ensure_spawned(cx);
                });
            } else {
                let msg = crate::i18n::tf(
                    "plugin.ext_wait_idle",
                    &[("name", entry.manifest.id.clone())],
                );
                self.set_status(msg, cx);
            }
        }
    }

    /// 确保当前会话的 pi 进程已起（执行域实例随之激活）。
    /// 幂等：已在跑直接返回；起不来就安静跳过（调用方继续走，不阻塞 UI）。
    pub(crate) fn ensure_active_process(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(rt) = self.pool.runtimes.get(&self.pool.active_key).cloned() else {
            return;
        };
        let _ = rt.update(cx, |r, cx| {
            r.ensure_spawned(cx);
        });
        // 进程起来后实例配置才被消费；顺手同步一次（幂等）
        svc::svc_sync_configs(self);
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
        eprintln!("[pf-plugin] popup close (trace)");
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
        eprintln!("[popup-tick] reload ix={ix}");
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
                eprintln!(
                    "[popup-tick] body={}",
                    serde_json::to_string(&body)
                        .unwrap_or_default()
                        .chars()
                        .take(120)
                        .collect::<String>()
                );
                if let Some(p) = self.plugins.popup.as_mut() {
                    p.body = Some(body);
                    p.tree_loaded = true;
                }
                cx.notify();
            }
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

    // -------------------------------------------------------------------
    // 内层 wasm 插件（加密包）：**与 JS 插件同一条渲染管线**——`render()` 返回
    // 01 规范组件树 JSON，宿主 `panel::resolve` 统一解释；事件走 on-event。
    // wasm 不另立渲染模型（Paint 直绘已废）。
    // -------------------------------------------------------------------

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
                    } else {
                        eprintln!("[pf-plugin] page_actived 未导出（ix={ix}）");
                    }
                }
            }
        }
        self.plugins.row_cache.clear();
        self.plugins.fn_bar_cache.clear();
        eprintln!(
            "[pf-plugin] page_actived 派发 key={} 插件数={} 通知={}",
            session_key, n, notified
        );
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

impl Chat {
    /// 会话列表徽标（契约 v3）：命中该会话的**全部**徽标 → [(icon 资产路径, tone)]，
    /// 每插件一条（owner 唯一）。绘制归宿主：从右到左、间隔 10px 全画出（用户定夺
    /// 2026-10-28），不裁不并。
    pub(crate) fn plugin_badge(
        &mut self,
        session: &std::path::Path,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<(SharedString, String)> {
        let key = session.to_string_lossy().into_owned();
        let mut out: Vec<(SharedString, String)> = badges_for(&self.plugins.badges, session);
        const TTL: std::time::Duration = std::time::Duration::from_millis(1000);
        let n = self.plugins.plugins.len();
        for ix in 0..n {
            // 能力面（8tb.2）：徽标是每行渲染结果，载体无关——JS 未导出 /
            // wasm 未实现都回 None（负缓存 TTL 内不再问）
            let (pkg, capable) = {
                let e = &self.plugins.plugins[ix];
                (e.manifest.pkg.clone(), !e.disabled)
            };
            if !capable {
                continue;
            }
            if let Some(c) = self
                .plugins
                .row_cache
                .iter()
                .find(|c| c.pkg == pkg && c.session_key == key && c.at.elapsed() < TTL)
            {
                if let Some((icon, tone)) = &c.badge {
                    let id = self.plugins.plugins[ix].manifest.id.clone();
                    out.push((contrib_icon_path(&id, icon), tone.clone()));
                }
                continue;
            }
            if !self.plugin_ready(ix, cx) {
                continue;
            }
            let env = serde_json::json!({ "session": { "key": key } }).to_string();
            let (r, _) = {
                let entry = &mut self.plugins.plugins[ix];
                entry.hook_session_row(&env)
            };
            let badge = match r {
                Ok(Some(json)) => {
                    let v: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
                    let icon = v
                        .get("icon")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let tone = v
                        .get("tone")
                        .and_then(Value::as_str)
                        .unwrap_or("accent")
                        .to_string();
                    eprintln!(
                        "[pf-plugin] session_list_render 命中 pkg={} session={} icon={}",
                        self.plugins.plugins[ix].manifest.pkg, key, icon
                    );
                    if icon.is_empty() {
                        None
                    } else {
                        let id = self.plugins.plugins[ix].manifest.id.clone();
                        out.push((contrib_icon_path(&id, &icon), tone.clone()));
                        Some((icon, tone))
                    }
                }
                _ => None,
            };
            self.plugins.row_cache.push(RowCache {
                pkg,
                session_key: key.clone(),
                badge,
                at: std::time::Instant::now(),
            });
        }
        out
    }
}

/// 会话功能栏插件 icon（契约 v3 fn_bar）：每插件一颗 icon 钮（tone 着色、
/// tip 悬浮），点击 = fn_bar.command 回环（env 带当前会话键）。
pub(crate) fn fn_bar_plugin_elements(
    chat: &mut Chat,
    cx: &mut gpui::Context<Chat>,
) -> Vec<gpui::AnyElement> {
    // 当前会话键：**以标签页的会话路径为准**（= 用户看到的那个会话）。
    // 不能用「活跃 runtime 的 state.session_file」兜底：新建/刚切过去的会话还没
    // 起自己的 pi 进程时，runtime 池里的可能是上一个会话 → 点 icon 绑到旧会话
    // 。runtime 状态只做最后兜底。
    let session_key = {
        let tab = chat.pool.active_key.to_string();
        if tab.ends_with(".jsonl") {
            tab
        } else {
            chat.pool
                .runtimes
                .get(&chat.pool.active_key)
                .and_then(|rt| {
                    rt.read(cx)
                        .state
                        .as_ref()
                        .and_then(|s| s.session_file.clone().or_else(|| s.session_id.clone()))
                })
                .unwrap_or_default()
        }
    };
    let chromes = chat.plugin_fn_bar_chrome(&session_key, cx);
    let weak = cx.entity().downgrade();
    chromes
        .into_iter()
        .map(|(ix, icon_path, tone, tip)| {
            let t = crate::appearance::theme::theme();
            // accent（插件的「已绑定/激活」态语义）固定映射主题绿
            // （appearance::UNREAD）——kitchen 等主题 accent 是红，直接透传
            // 会与错误色混淆。其余按 token 原样映射（text.faint = 未绑默认
            // 灰，2026-10-29 用户定夺；曾一律 text_muted 近黑，灰不可辨）。
            let color = match tone.as_str() {
                "accent" => crate::appearance::theme::UNREAD,
                "text.faint" => t.text_faint,
                "text.muted" => t.text_muted,
                _ => t.text_muted,
            };
            let weak = weak.clone();
            let key = session_key.clone();
            let mut el = div()
                .id(SharedString::from(format!("pf-fnbar-{ix}")))
                .flex()
                .items_center()
                .justify_center()
                .w(px(32.))
                .h(px(32.))
                .mr(px(20.)) // 按钮右侧留 20px（不压缩 icon，用户定夺）
                .rounded(px(6.))
                .cursor_pointer()
                .hover(move |s| s.bg(rgb(t.bg_hover)));
            if !tip.is_empty() {
                el = el.with_tip_text(tip);
            }
            el.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                let _ = weak.update(cx, |c, cx| {
                    c.plugin_fn_bar_click(ix, &key, window, cx);
                    cx.notify();
                });
            })
            .child(
                gpui::svg()
                    .path(icon_path)
                    .text_color(rgb(color))
                    .size(px(24.)), // fn 槽 icon = XL 档 24（用户定夺 2026-10-28）
            )
            .into_any_element()
        })
        .collect()
}
