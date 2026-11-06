//! PF 插件宿主——懒加载就绪链（02 §4）：plugin_ready（JS ctx 首建 / wasm
//! 懒装载）、后台编译守卫、验签拒载标记、refresh（onPanelOpen + render +
//! 伴随动作落地）。

use std::cell::RefCell;
use std::rc::Rc;

use pf_plugin_host::PluginInstance;
use pf_plugin_host::js::{JsAction, JsPlugin};

use crate::Chat;

use super::wasm_hooks::{ensure_wasm_hooks, has_official_proof};
use super::*;

impl Chat {
    /// 懒加载就绪：runtime 已建或现建；禁用/失败返回 false（已 toast）。
    pub(crate) fn plugin_ready(&mut self, ix: usize, cx: &mut gpui::Context<Self>) -> bool {
        let Some(entry) = self.plugins.plugins.get_mut(ix) else {
            return false;
        };
        if entry.disabled {
            return false;
        }
        if entry.manifest.wasm.is_some() {
            if entry.wasm.is_some() {
                return true;
            }
            return self.plugin_wasm_ready(ix, cx);
        }
        if entry.js.is_some() {
            return true;
        }
        // 首用建 ctx：entry 取第一个 panel 声明（v1 单面板）；私有目录 data/
        let panel = entry.manifest.panels.first().cloned();
        let entry_rel = panel
            .as_ref()
            .map(|p| p.entry.clone())
            .unwrap_or_else(|| "main.js".into());
        let dir = entry.manifest.data_dir();
        let _ = std::fs::create_dir_all(&dir); // 插件自管目录：PF 只建，不解析
        let dir_str = dir.to_string_lossy().into_owned();
        let id = entry.manifest.id.clone();
        let host = Rc::new(RefCell::new(ChatHost {
            id: id.clone(),
            dir,
            actions: Vec::new(),
        }));
        // 分发态字节码包：entry 换成 plugin.qbc，加载器走 QuickJS 字节码
        let bytecode_rel = entry.manifest.bytecode.clone();
        let entry_path = entry
            .manifest
            .dir
            .join(bytecode_rel.as_deref().unwrap_or(&entry_rel));
        // 正版门（分发态）：字节码包只在官方构建里加载（非官方 = 占位证明 → 拒载）
        if bytecode_rel.is_some() && !has_official_proof() {
            entry.disabled = true;
            self.set_status(format!("插件 {id} 拒载：需要官方构建的 PF"), cx);
            return false;
        }
        // 90-9：load 事件 = serving 实例的装载+激活，全程耗时进 plugin.log
        entry_state(entry, lifecycle::Ev::LoadStart);
        let t0 = std::time::Instant::now();
        let loaded = match bytecode_rel {
            Some(_) => JsPlugin::load_bytecode(&entry_path, &dir_str, host.clone()),
            None => JsPlugin::load(&entry_path, &dir_str, host.clone()),
        };
        match loaded {
            Ok(js) => {
                let mut failed = None;
                if let Err(e) = js.activate() {
                    failed = Some(format!("插件 {id} 出错：{e}"));
                }
                if failed.is_none() {
                    entry.js = Some(js);
                    entry.host = Some(host);
                    lifecycle::log_load(&entry.manifest.dir, true, t0.elapsed().as_millis(), "");
                    entry_state(entry, lifecycle::Ev::LoadOk);
                    // 装载补拍（B6）：page-active 先于调用方的 onPanelOpen/
                    // render 落地，page_id 在面板首帧前就位
                    self.plugin_catchup_page_active(ix, cx);
                    return true;
                }
                entry_state(entry, lifecycle::Ev::LoadErr);
                lifecycle::log_load(
                    &entry.manifest.dir,
                    false,
                    t0.elapsed().as_millis(),
                    failed.as_deref().unwrap_or(""),
                );
                entry.disabled = true;
                self.set_status(failed.unwrap(), cx);
                false
            }
            Err(e) => {
                entry_state(entry, lifecycle::Ev::LoadErr);
                lifecycle::log_load(&entry.manifest.dir, false, t0.elapsed().as_millis(), &e);
                entry.disabled = true;
                self.set_status(format!("插件 {id} 出错：{e}"), cx);
                false
            }
        }
    }

    /// 内层 wasm 插件懒装载（加密包渲染面）：**一律后台装载**（B8，2026-11-06
    /// 流式卡死修复）——缓存命中 = mmap 反序列化毫秒级；未命中/产物失效 =
    /// cranelift 全量编译秒级，两种量级都不许进 UI 线程。面板打开显「…」
    /// 占位，装载完成回主线程补拍 + 渲染。曾有的「缓存命中走 UI 线程同步
    /// 装」在产物失效（wasmtime 版本/配置失配致 `has_cached_artifact` 假阳
    /// 性）时会在 UI 线程现场重编数秒；后台预编译持 compile_lock 时同步路
    /// 径等锁同样冻结（instance.rs 真机事故记录）。失败 = 会话级禁用（与
    /// JS 同款）。
    pub(crate) fn plugin_wasm_ready(&mut self, ix: usize, cx: &mut gpui::Context<Self>) -> bool {
        ensure_wasm_hooks(cx);
        self.plugin_sync_quick_pins();
        let Some(entry) = self.plugins.plugins.get_mut(ix) else {
            return false;
        };
        let id = entry.manifest.id.clone();
        let wasm_path = entry
            .manifest
            .dir
            .join(entry.manifest.wasm.as_deref().unwrap_or("plugin.wasm"));
        // 编译产物缓存绑在插件安装目录（卸载即清、整目录拷走跟着走）。
        let cache = pi_link::pf_plugins::plugin_cache_for(&entry.manifest.dir);
        // **装载中守卫（所有路径之前）**：后台装载期间 entry.wasm 仍是 None，
        // 而面板/快照渲染链每拍都调 plugin_ready——无守卫时每拍再 spawn
        // 一个后台装载任务，真机实测 1 秒内 47 条 load（全量编译 4-6s/条，
        // 2026-10-31 e2e 日志）。Loading 态一律拒绝。
        if entry.state == lifecycle::State::Loading {
            return false;
        }
        entry_state(entry, lifecycle::Ev::LoadStart);
        let manifest = entry.manifest.clone();
        cx.spawn(async move |this, cx| {
            let t0 = std::time::Instant::now();
            let res = cx
                .background_executor()
                .spawn(async move {
                    let inst = PluginInstance::load_cached(&wasm_path, &cache);
                    inst.and_then(|mut i| {
                        let ctx = plugin_ctx_json(&id, &manifest, None);
                        i.load_ctx(&ctx).map(|()| i)
                    })
                })
                .await;
            let ms = t0.elapsed().as_millis();
            let _ = this.update(cx, |c, cx| {
                let Some(entry) = c.plugins.plugins.get_mut(ix) else {
                    return;
                };
                match res {
                    Ok(inst) => {
                        lifecycle::log_load(&entry.manifest.dir, true, ms, "");
                        entry_state(entry, lifecycle::Ev::LoadOk);
                        entry.wasm = Some(inst);
                        entry.wasm_focus = Some(cx.focus_handle());
                        // [perf] 铁律：热路径实测数字（main.rs perf_mark 同款）
                        if std::env::var("PI_FLASH_PERF").is_ok() {
                            eprintln!("[perf] wasm 后台装载完成 {ms}ms");
                        }
                        // 装载补拍（B6）：page-active 先落地再首渲染，
                        // page_id 在面板首帧前就位
                        c.plugin_catchup_page_active(ix, cx);
                        c.plugin_wasm_render(ix, cx);
                    }
                    Err(e) => {
                        entry_state(entry, lifecycle::Ev::LoadErr);
                        lifecycle::log_load(&entry.manifest.dir, false, ms, &e.to_string());
                        entry.disabled = true;
                        let id = entry.manifest.id.clone();
                        c.set_status(format!("插件 {id} 出错：{e}"), cx);
                    }
                }
            });
        })
        .detach();
        false
    }

    /// 后台验签门拒载的包 → 会话级禁用（tab 隐藏、面板点不开；090-5 L3 fail-closed）。
    pub(crate) fn plugin_mark_rejected(&mut self, ids: &[String]) {
        for e in &mut self.plugins.plugins {
            if e.manifest.wasm.is_some() && ids.contains(&e.manifest.id) {
                e.disabled = true;
            }
        }
    }

    /// onPanelOpen + render → rows 缓存（02 §4 懒加载链）。
    pub(crate) fn plugin_refresh(&mut self, ix: usize, cx: &mut gpui::Context<Self>) {
        let t0 = std::time::Instant::now();
        if !self.plugin_ready(ix, cx) {
            return;
        }
        // 内层 wasm 插件：render() → Paint 缓存（画布路径，与 JS 组件树分叉）。
        // 不需要 panel-open 事件：render 每次都现读插件自己的存储。
        if self.plugins.plugins[ix].wasm.is_some() {
            self.plugin_wasm_render(ix, cx);
            // 契约 v3：弹窗开着且属于本插件 → 体跟着新 render
            if self.plugins.popup.as_ref().map(|p| p.ix) == Some(ix) {
                let body = self.plugins.plugins.get(ix).and_then(|e| e.body.clone());
                if let Some(p) = self.plugins.popup.as_mut() {
                    p.body = body;
                    p.tree_loaded = p.body.is_some();
                }
                cx.notify();
            }
            return;
        }
        let (result, actions) = {
            let entry = &mut self.plugins.plugins[ix];
            let host = entry.host.as_ref().unwrap().clone();
            host.borrow_mut().actions.clear();
            let js = entry.js.as_ref().unwrap();
            let r = js.fire_panel_open().and_then(|()| js.render_json());
            let drained = host.borrow_mut().actions.drain(..).collect::<Vec<_>>();
            (r, drained)
        };
        for a in actions {
            // 徽标是渲染的伴随声明（同 fn_bar）：render_popup 里也会同步徽标，
            // 一律丢弃会让列表徽标彻底打不上。
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
                other => eprintln!("[pf-plugin] render/onPanelOpen 期间的动作被忽略: {other:?}"),
            }
        }
        match result {
            Ok(json) => {
                let entry = &mut self.plugins.plugins[ix];
                let v: serde_json::Value =
                    serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
                // v2：render 直接返回组件树（根节点），没有 { body, rows } 外壳
                entry.body = Some(v);
                entry.tree_loaded = true;
                // 契约 v3：弹窗开着且属于本插件 → 内容跟着新态（refresh-qr 等回环后弹窗即新态）
                self.plugin_popup_reload(ix, cx);
                cx.notify();
                // [perf] 铁律：热路径实测数字（PI_FLASH_PERF 门控；2026-11-06
                // 卡死修复附带——refresh 链含 onPanelOpen+render 两次同步
                // JS/wasm 调用，流式期间是否成为突发全靠这个数字说话）
                if std::env::var("PI_FLASH_PERF").is_ok() {
                    eprintln!("[perf] plugin_refresh ix={ix} 耗时 {:?}", t0.elapsed());
                }
            }
            Err(e) => self.plugin_broken(ix, e, cx),
        }
    }
}
