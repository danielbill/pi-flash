//! PF 插件宿主 Chat 方法·装载与面板侧（090 重构自 mod.rs 拆出）：
//! scan 落位 / svc 投递 / 状态栏 tab / dock 与预览页签 / 认领会话 / 撤下与
//! 重装载 / 懒加载就绪链（plugin_ready/wasm_ready/refresh）。

use super::wasm_hooks::{ensure_wasm_hooks, has_official_proof};

use std::cell::RefCell;
use std::rc::Rc;

use gpui::AppContext;
use pf_plugin_host::PluginInstance;
use pf_plugin_host::js::{JsAction, JsPlugin};
use serde_json::json;

use crate::Chat;

use super::*;

impl Chat {
    /// 启动扫描落位（spawn_plugin_scan 回调）。
    pub(crate) fn plugins_set(&mut self, plugins: Vec<PluginEntry>, cx: &mut gpui::Context<Self>) {
        self.plugins.plugins = plugins;
        // 执行域实例集随清单落位同步（装/卸/更新都走这里重扫，90-6）
        svc::svc_sync_configs(self);
        cx.notify();
    }

    /// 执行域 svc→UI 投递（90-6 通道）：outbox 泵回调呈现面 onMessage 后重渲。
    /// wasm 载体面（on-event kind="svc-message"）随 ③-b 接线。
    pub(crate) fn svc_deliver(
        &mut self,
        plugin_id: &str,
        payload_json: &str,
        meta_json: &str,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(ix) = self
            .plugins
            .plugins
            .iter()
            .position(|e| e.manifest.id == plugin_id)
        else {
            return;
        };
        // wasm 载体：on-event kind="svc-message"，path = {payload, meta}
        if self.plugins.plugins[ix].wasm.is_some() {
            let path = format!("{{\"payload\":{payload_json},\"meta\":{meta_json}}}");
            let res = {
                let Some(inst) = self.plugins.plugins[ix].wasm.as_mut() else {
                    return;
                };
                let r = inst.on_event("svc-message", plugin_id, &path);
                let acts = inst.pump_actions();
                (r, acts)
            };
            match res.0 {
                Ok(()) => {
                    self.plugin_wasm_render(ix, cx);
                }
                Err(e) => self.plugin_broken(ix, format!("{e}"), cx),
            }
            return;
        }
        let Some(js) = self.plugins.plugins[ix].js.as_ref() else {
            eprintln!("[pf-svc] 推送到达但呈现面未装载（{plugin_id}），丢弃");
            return;
        };
        if let Err(e) = js.fire_svc_message(payload_json, meta_json) {
            eprintln!("[pf-svc] onMessage 投递失败（{plugin_id}）: {e}");
            return;
        }
        cx.notify();
    }

    /// 状态栏插件 tab 点击：左列切到该插件面板（懒加载链 02 §4：
    /// 建 ctx → onPanelOpen → render → 行缓存）。重复点击 = 重新激活（刷新）。
    pub(crate) fn open_plugin_dock(&mut self, ix: usize, cx: &mut gpui::Context<Self>) {
        // 已停用的插件不开面板（tab 本身已隐藏，这里只是兼作硬门）
        if self.plugins.plugins.get(ix).is_none_or(|e| e.disabled) {
            return;
        }
        self.layout.dock_panel = crate::DockPanel::Plugin(ix);
        // 广播「面板打开首拍」语义：清快照 → 下一拍必发当前状态
        broadcast::reset(&mut self.plugins);
        self.plugin_refresh(ix, cx);
    }

    /// 该插件的（第一个）面板是否声明为内容区预览页签（契约 v2.4 dock）。
    pub(crate) fn plugin_dock_is_tab(&self, ix: usize) -> bool {
        self.plugins
            .plugins
            .get(ix)
            .is_some_and(|e| e.manifest.tab_page().is_some())
    }

    /// 某会话 key 是否被任一插件认领（过滤/last_open 排除的唯一判据）。
    pub(crate) fn plugin_claimed(&self, key: &str) -> bool {
        self.plugins.claims.iter().any(|c| c.key == key)
    }

    /// 新建插件认领会话（契约 v2.4 sessions；002「设置会话」步骤的泛化）。
    /// 名字 = 前缀 + 15 位时间戳（唯一性归宿主）；过滤清单 owner = 插件 id
    /// （hidden_sessions 天然 owner 泛化，消费方零改动）；包内 pi 扩展挂到
    /// 该会话的 session_ext（spawn 时 `-e` 装载）。cwd = 插件自持的工作
    /// 目录（002 模块独立：与桌面 cwd 互不联动，错误项目问题无从发生）。
    /// 建完经 broadcast "claim" 回推 key（插件存指针）。
    pub(crate) fn plugin_new_draft(
        &mut self,
        ix: usize,
        prefix: &str,
        cwd: &str,
        cx: &mut gpui::Context<Self>,
    ) -> Result<String, String> {
        if prefix.is_empty() {
            return Err("prefix 不能为空".into());
        }
        let cwd_path = std::path::PathBuf::from(cwd);
        if !cwd_path.is_dir() {
            return Err(format!("cwd 不是目录：{cwd}"));
        }
        let Some(entry) = self.plugins.plugins.get(ix) else {
            return Err("插件不存在".into());
        };
        if entry.disabled {
            return Err("插件已停用".into());
        }
        // 清单对账（契约 v2.4 `[[contributes.sessions]] prefix`）：声明过槽位的
        // 插件，实际认领前缀不在声明里就留痕——不阻断（运行期真相是
        // ctx.newDraft 传入的 prefix，声明只是清单面的可审计位）。
        let declared = &entry.manifest.sessions;
        if !declared.is_empty() && !declared.iter().any(|s| s.prefix == prefix) {
            let list: Vec<&str> = declared.iter().map(|s| s.prefix.as_str()).collect();
            eprintln!(
                "[pf-plugin] 认领前缀与清单声明不一致（plugin={}，declared=[{}]，actual={prefix}）",
                entry.manifest.id,
                list.join(", ")
            );
        }
        let plugin_id = entry.manifest.id.clone();
        let name = claim_session_name(prefix);
        let key = format!("draft-{}", self.pool.draft_seq);
        self.pool.draft_seq += 1;
        let rt = cx.new(|_| {
            let mut r =
                crate::session::runtime::SessionRuntime::new(key.clone(), cwd_path.clone(), None);
            r.default_model = crate::services::workspace::default_model_pref();
            r.status = crate::i18n::tr("session.new").to_string();
            r
        });
        self.pool.runtimes.insert(key.clone(), rt.clone());
        self.subscribe_runtime(&rt, cx);
        // 执行域（v4）：实例随会话 spawn 由宿主自带 shim 统一挂载（runtime 侧
        // 读实例配置），这里不再做 per-claim 挂载（90-6 §9 一步替换）。
        // 认领槽替换（一个插件同时只有一条进行中线）+ 过滤清单（owner = 插件 id）
        let prefix = prefix.to_string();
        self.plugins.claims.retain(|c| c.plugin_id != plugin_id);
        self.plugins.claims.push(Claim {
            plugin_id: plugin_id.clone(),
            prefix: prefix.clone(),
            key: key.clone(),
        });
        pi_link::hidden_sessions::set_owned(&plugin_id, Some(&name), None, Some(&prefix));
        // 结果回推（契约 broadcast_events.claim）：插件存指针（newDraft 泛化）
        let payload = json!({"key": key, "name": name, "cwd": cwd}).to_string();
        self.plugin_broadcast_fire(ix, broadcast::CLAIM, &payload, cx);
        Ok(key)
    }

    /// 打开插件预览页签（契约 v2.4 `dock = "tab"`）：单例（同 pkg+page
    /// 已开 → 激活）、整页声明式树。懒加载链与 dock 同源（plugin_refresh），
    /// 广播首拍同步复位。入口 = 状态栏点击（清单 dock 归宿主解释，
    /// 插件不感知形态）。
    pub(crate) fn open_plugin_tab(&mut self, ix: usize, cx: &mut gpui::Context<Self>) {
        // 已停用的插件不开页签（同 dock 硬门）
        let Some(entry) = self.plugins.plugins.get(ix) else {
            return;
        };
        if entry.disabled {
            return;
        };
        let Some((pkg, page)) = entry
            .manifest
            .tab_page()
            .map(|(p, g)| (p.to_string(), g.to_string()))
        else {
            return;
        };
        // 单例：同 pkg+page 已开 → 激活既有页签（不重排，082 顺序稳定）
        let tix = self.ws.panel_tabs.iter().position(
            |t| matches!(t, crate::PanelTab::Plugin { pkg: p, page: g } if p == &pkg && g == &page),
        );
        let tix = match tix {
            Some(tix) => tix,
            None => {
                self.ws
                    .panel_tabs
                    .push(crate::PanelTab::Plugin { pkg, page });
                self.ws.panel_tabs.len() - 1
            }
        };
        self.activate_panel_tab(tix, cx);
        self.set_content_view(crate::ContentView::Plugin);
        // 广播首拍 + 懒加载链（与 dock 同源）
        broadcast::reset(&mut self.plugins);
        self.plugin_refresh(ix, cx);
    }

    /// 台账包名 → 运行态条目下标。对账走清单的**顶层包目录名**
    /// （市场布局 `@scope__name` 与 dev 平铺 `name` 两种别名都认）——
    /// 拿 `manifest.dir.file_name()` 比是版本号，永远比不中。
    pub(crate) fn plugin_ix_by_pkg(&self, name: &str) -> Option<usize> {
        let aliases = pi_link::pf_plugins::dir_aliases(name);
        self.plugins
            .plugins
            .iter()
            .position(|e| aliases.iter().any(|a| *a == e.manifest.pkg))
    }

    /// 运行态就地撤下（停用 / 卸载共用）：丢 JS 运行时与组件树缓存、收起右键
    /// 菜单；**左 dock 正显示它就退回默认面板**。
    ///
    /// 不撤组件树就是「停用了左面板还在照旧渲染」：`plugin_panel_view` 只认
    /// `tree_loaded` + 缓存树，不看 disabled 位（090-4 v5 踩过）。
    pub(crate) fn plugin_evict(&mut self, ix: usize) {
        let docked = self.layout.dock_panel == crate::DockPanel::Plugin(ix);
        // 认领槽随停用一井撤：过滤清单同步摘除（会话文件保留，靠前缀仍藏得住）
        if let Some(entry) = self.plugins.plugins.get(ix) {
            let id = entry.manifest.id.clone();
            self.plugins.claims.retain(|c| c.plugin_id != id);
            pi_link::hidden_sessions::clear_owned(&id);
            // 90-9 §8 摘除：会话挂载清单里本插件的 ext 条目全量摘掉——残留会
            // 在下次 spawn 把 ext 带回进程
            let _ = pi_link::session_ext::purge_exec_entries(&id);
        }
        if let Some(entry) = self.plugins.plugins.get_mut(ix) {
            // 90-9 §6：停用 = 唯一销毁入口，unload() 先于丢运行时（卸载/更新
            // 覆盖走 evict 同得 unload）
            entry_unload(entry);
            entry.disabled = true;
            entry.js = None;
            entry.wasm = None;
            entry.wasm_focus = None;
            entry.host = None;
            entry.body = None;
            entry.tree_loaded = false;
            entry.cell_menu = None;
            entry.section_scrolls.borrow_mut().clear();
        }
        if docked {
            self.layout.dock_panel = crate::DockPanel::Default;
        }
    }

    /// PF 退出扇出（90-9 §6）：全体已装载插件 `unload()`，**总预算 1 秒**；
    /// 超预算的剩余插件各记一条「退出预算用尽」。退出顺序 = unload → 停 pi
    /// 子进程（随进程退出终止）→ 存窗口状态——unload 先行，带 `pi_extensions`
    /// 的插件要在 pi 进程还活着时说完最后一句话（90-9 §8，不提供 ack）。
    /// 由 `main.rs` 的 `on_window_should_close` 首位调用。
    pub(crate) fn plugin_unload_all_for_quit(&mut self) {
        const BUDGET: std::time::Duration = std::time::Duration::from_secs(1);
        let t_all = std::time::Instant::now();
        for entry in &mut self.plugins.plugins {
            if entry.js.is_none() && entry.wasm.is_none() {
                continue; // 未装载过：无 load 即无 unload，配对不能凭空造
            }
            let dir = entry.manifest.dir.clone();
            if t_all.elapsed() >= BUDGET {
                lifecycle::log_unload(&dir, false, 0, "PF 退出预算用尽，未执行 unload");
                continue;
            }
            entry_unload(entry);
        }
    }

    /// 运行态（重）装载单个插件：撤旧条目 + 按新清单重建（安装 / 更新 / 启用
    /// 共用）。返回 false = 清单为空（调用方决定报错文案）。
    ///
    /// **不整体替换 `plugins.plugins`**：那会清掉其他插件的 JS 运行态，还会让
    /// dock 存的 `Plugin(ix)` 位置错位。
    pub(crate) fn plugin_replace_by_pkg(
        &mut self,
        name: &str,
        manifests: Vec<PluginManifest>,
    ) -> bool {
        if let Some(ix) = self.plugin_ix_by_pkg(name) {
            self.plugin_evict(ix);
            self.plugins.plugins.remove(ix);
            // 下标位移：插件面板存的是位置，任何一条被摘都退默认
            if matches!(self.layout.dock_panel, crate::DockPanel::Plugin(_)) {
                self.layout.dock_panel = crate::DockPanel::Default;
            }
        }
        if manifests.is_empty() {
            return false;
        }
        self.plugins.plugins.extend(plugin_entries(manifests));
        true
    }

    /// 后台按台账包名重扫清单（只出这一个包）→ 主线程 `plugin_replace_by_pkg`
    /// 落位。安装完 / 启用即生效，不靠重启；扫不到就走市场页头段错误行。
    pub(crate) fn reload_plugin_by_pkg(&mut self, name: String, cx: &mut gpui::Context<Self>) {
        let pkg = name.clone();
        let scan = cx.background_executor().spawn(async move {
            let aliases = pi_link::pf_plugins::dir_aliases(&pkg);
            scan_manifests()
                .into_iter()
                .filter(|m| aliases.iter().any(|a| *a == m.pkg))
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let manifests = scan.await;
            let _ = this.update(cx, |c, cx| {
                if !c.plugin_replace_by_pkg(&name, manifests) {
                    let msg = crate::i18n::tf("market.pf.enable_failed", &[("name", name.clone())]);
                    c.aux.market.pf.error = Some(msg);
                }
                cx.notify();
            });
        })
        .detach();
    }

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
        let dir = entry.manifest.dir.join("data");
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

    /// 内层 wasm 插件懒装载（加密包渲染面）：主线程 `load_cached` —— cwasm
    /// 缓存命中 = mmap 反序列化 + 实例化，毫秒级（启动预热已把编译搬走），
    /// 首次开面板是用户显式动作，这个量级无感。失败 = 会话级禁用（与 JS 同款）。
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
        // 安装时已预编译过 → 这里命中是常态（mmap 反序列化，毫秒级）。
        let cache = pi_link::pf_plugins::plugin_cache_for(&entry.manifest.dir);
        // **装载中守卫（所有路径之前）**：后台编译期间 entry.wasm 仍是 None，
        // 而徽标/面板/快照渲染链每拍都调 plugin_ready——无守卫时每拍再 spawn
        // 一个后台装载任务，真机实测 1 秒内 47 条 load（全量编译 4-6s/条，
        // 2026-10-31 e2e 日志）。守卫只放在后台分支还不够：后台 load_cached
        // 中途把产物写进缓存后 cached 翻 true，同步路径会绕过分支再装一份
        // （双实例双日志）——Loading 态一律拒绝，无论缓存命中与否。
        if entry.state == lifecycle::State::Loading {
            return false;
        }
        // 缓存未命中（手工摆包/旧版安装）：编译秒级重活不许进主线程 →
        // 转后台装载，装好回主线程填实例+渲染；此刻返回 false = 「…」。
        let cached = pf_plugin_host::PluginInstance::has_cached_artifact(&wasm_path, &cache);
        if !cached {
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
            return false;
        }
        // [perf] UI 线程同步装载要留实测数字（铁律：热路径不靠感觉）
        entry_state(entry, lifecycle::Ev::LoadStart);
        let t0 = std::time::Instant::now();
        let mut inst = match PluginInstance::load_cached(&wasm_path, &cache) {
            Ok(i) => i,
            Err(e) => {
                entry_state(entry, lifecycle::Ev::LoadErr);
                lifecycle::log_load(
                    &entry.manifest.dir,
                    false,
                    t0.elapsed().as_millis(),
                    &e.to_string(),
                );
                entry.disabled = true;
                self.set_status(format!("插件 {id} 出错：{e}"), cx);
                return false;
            }
        };
        let load = t0.elapsed();
        if let Err(e) = inst.load_ctx(&plugin_ctx_json(&id, &entry.manifest, None)) {
            entry_state(entry, lifecycle::Ev::LoadErr);
            lifecycle::log_load(
                &entry.manifest.dir,
                false,
                t0.elapsed().as_millis(),
                &e.to_string(),
            );
            entry.disabled = true;
            self.set_status(format!("插件 {id} 拒载：{e}"), cx);
            return false;
        }
        lifecycle::log_load(&entry.manifest.dir, true, load.as_millis(), "");
        entry_state(entry, lifecycle::Ev::LoadOk);
        eprintln!(
            "[perf] wasm 懒装载(UI线程) {id}: load_cached={load:?} init={:?} caller={}",
            t0.elapsed() - load,
            std::backtrace::Backtrace::force_capture()
                .to_string()
                .lines()
                .nth(2)
                .unwrap_or("")
                .trim(),
        );
        entry.wasm = Some(inst);
        entry.wasm_focus = Some(cx.focus_handle());
        true
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
            }
            Err(e) => self.plugin_broken(ix, e, cx),
        }
    }
}
