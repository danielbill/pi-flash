//! PF 插件宿主 Chat 方法·装载与面板侧（090 重构自 mod.rs 拆出）。
//!
//! 文件组织（超规二次拆分，impl Chat 块形式）：
//! - 本文件：scan 落位 / svc 投递 / 台账对账 / 撤下与退出扇出 / 单包重装载
//! - [`tabs`]：状态栏 tab / dock 与预览页签打开 / 认领会话（new_draft）
//! - [`ready`]：懒加载就绪链（plugin_ready / wasm_ready / refresh）

mod ready;
mod tabs;

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
            // 005 §③：运行许可收线（宿主协议文件，ext 泵见 off 自我隔离——
            // 停渠道 + 发下线通知 + 待机，不再用 unload_ts 时间戳炸弹）
            crate::pf_plugin::pf_runtime_write(&entry.manifest, false, "disable");
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
        // 005 §③：退出前先收运行许可（宿主协议文件）——带 pi_extensions 的
        // 插件 ext 泵 200ms 内见 off，停渠道 + 发下线通知（在 1s 预算窗口内），
        // 随后进程树整体消亡
        for entry in &self.plugins.plugins {
            if !entry.manifest.pi_extensions.is_empty() {
                crate::pf_plugin::pf_runtime_write(&entry.manifest, false, "quit");
            }
        }
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
        // 全量摘除同包条目（2026-12 真机：曾只摘 position 第一条，配合来源
        // 未明的重复条目 → fn_bar 双 icon；条目唯一性在此收口，残留了几条
        // 就摘几条，>1 时打取证日志指认）。
        let mut removed = 0usize;
        while let Some(ix) = self.plugin_ix_by_pkg(name) {
            self.plugin_evict(ix);
            self.plugins.plugins.remove(ix);
            removed += 1;
            // 下标位移：插件面板存的是位置，任何一条被摘都退默认
            if matches!(self.layout.dock_panel, crate::DockPanel::Plugin(_)) {
                self.layout.dock_panel = crate::DockPanel::Default;
            }
        }
        if removed > 1 && cfg!(debug_assertions) {
            eprintln!(
                "[pf-plugin] 异常：{name} 落位前已存在 {removed} 条运行条目（重复装载，来源待查）"
            );
        }
        if manifests.is_empty() {
            return false;
        }
        self.plugins.plugins.extend(plugin_entries(manifests));
        true
    }

    /// 后台按台账包名重扫清单（只出这一个包）→ 主线程 `plugin_replace_by_pkg`
    /// 落位。安装完 / 启用即生效，不靠重启；扫不到就走市场页头段错误行。
    ///
    /// `reattach`：重建后把包内 pi 扩展重挂回**当前会话**（更新链路用——
    /// 更新前 evict 已 purge 会话清单 + 摘活跃进程 ext，重载必须补回；全新
    /// 安装/启用走各自原有挂载时机，传 false）。
    pub(crate) fn reload_plugin_by_pkg(
        &mut self,
        name: String,
        reattach: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        let pkg = name.clone();
        let scan = cx.background_executor().spawn(async move {
            let aliases = pi_link::pf_plugins::dir_aliases(&pkg);
            let outcome = scan_manifests_full();
            let manifests = outcome
                .manifests
                .into_iter()
                .filter(|m| aliases.iter().any(|a| *a == m.pkg))
                .collect::<Vec<_>>();
            // 本包的失效条目一并带回：装上了 = 空（提示消失）；仍装不上 = 新原因
            let stale = outcome
                .stale
                .into_iter()
                .filter(|s| s.name == pkg)
                .collect::<Vec<_>>();
            (manifests, stale)
        });
        cx.spawn(async move |this, cx| {
            let (manifests, stale) = scan.await;
            let _ = this.update(cx, |c, cx| {
                c.aux.market.pf.stale.retain(|s| s.name != name);
                c.aux.market.pf.stale.extend(stale);
                if !c.plugin_replace_by_pkg(&name, manifests) {
                    let msg = crate::i18n::tf("market.pf.enable_failed", &[("name", name.clone())]);
                    c.aux.market.pf.error = Some(msg);
                } else if reattach && let Some(ix) = c.plugin_ix_by_pkg(&name) {
                    let key = c.pool.active_key.clone();
                    // 更新链 = 状态跃迁：force 重启自愈存量脏状态（停用周期
                    // 摘条目口径失配年代残留的「已含」短路）
                    c.plugin_attach_pi_exts(ix, &key, true, cx);
                    c.ensure_active_process(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
}
