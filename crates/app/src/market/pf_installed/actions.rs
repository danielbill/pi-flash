//! PF 插件市场【已安装】页签——启停 / 卸载动作与卸载确认浮层（090-4 v5）。
//!
//! 启停 / 卸载的完整链路陷阱（台账名 ↔ 运行态条目只认 `PluginManifest::pkg`、
//! Windows 下先撤运行态再删目录等）见 `docs/agents/market.md`。

use pi_link::pf_plugins;

use crate::Chat;
use crate::i18n::tf;
use crate::market::pf_market::index_snapshot;

impl Chat {
    /// 启停单插件（090-4 v5）：台账 disabled 位落盘 + 运行态立即生效。
    /// 停用 = 台账落盘 + `plugin_evict`（丢 JS 运行时与组件树缓存、左 dock
    /// 正显示它就退默认）——状态栏 tab / 文件树插件菜单 / 左面板全部隐身；
    /// 启用 = 有条目则复位，无条目（启动时被扫描跳过）只补装这一个。
    /// 失败进头段错误行。
    pub(crate) fn pf_toggle_plugin(
        &mut self,
        name: String,
        enable: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if let Err(e) = pf_plugins::set_disabled(&name, !enable) {
            self.aux.market.pf.error = Some(e);
            cx.notify();
            return;
        }
        if let Some(l) = self
            .aux
            .market
            .pf
            .ledger
            .iter_mut()
            .find(|l| l.name == name)
        {
            l.disabled = !enable;
        }
        // 台账包名 → 运行态条目：只认顶层包目录名（版本层目录名是版本号，
        // 拿它比永远比不中——上一版停用点了没反应就是因为这个）
        let hit = self.plugin_ix_by_pkg(&name);
        match (enable, hit) {
            (true, Some(ix)) => {
                if let Some(entry) = self.plugins.plugins.get_mut(ix) {
                    entry.disabled = false;
                }
                // 重新装载：ext 自动重挂当前会话——绑定态在镜像里原样保留，
                // ext 重挂后门控命中、渠道自恢复（免重扫）。force=true：ext 的
                // unload 水位是进程终态，停用→启用必须重启 pi 给它新 BOOT_TS
                self.plugin_attach_pi_exts(ix, &self.pool.active_key.clone(), true, cx);
                self.ensure_active_process(cx);
            }
            (true, None) => {
                // 启动时被停用过滤、运行态无条目：后台重扫**只补装这一个**
                // （内建 `plugin_replace_by_pkg`，不整体替换 plugins_set）
                self.reload_plugin_by_pkg(name.clone(), false, cx);
                // 启用即预热编译产物：下次启动验签走缓存命中（不现场编）
                crate::pf_plugin::spawn_precompile_installed();
            }
            // 停用：运行态就地撤下（含左 dock 退默认），否则面板还照旧渲染
            (false, Some(ix)) => {
                self.plugin_evict(ix);
                // 摘除活跃运行时里的 ext（空闲即重启；忙 = 下次 spawn 生效）
                self.plugin_detach_active_ext(ix, cx);
            }
            (false, None) => {} // 未装载，无运行态可撤
        }
        cx.notify();
    }

    /// 点垃圾桶：弹确认浮层（040 统一确认组件；渲染在 shell/render 顶层）。
    pub(crate) fn pf_ask_remove(&mut self, name: String, cx: &mut gpui::Context<Self>) {
        if self.aux.market.pf.removing.is_some() {
            return;
        }
        self.aux.market.pf.confirm_remove = Some(name);
        cx.notify();
    }

    /// 确认浮层的「确认 / 取消」：ok=true 才真删。
    pub(crate) fn pf_remove_dialog_close(&mut self, ok: bool, cx: &mut gpui::Context<Self>) {
        let Some(name) = self.aux.market.pf.confirm_remove.take() else {
            return;
        };
        if ok {
            self.pf_uninstall(name, cx);
        } else {
            cx.notify();
        }
    }

    /// 卸载：后台线程删目录 + 摘台账（幂等），完成重读台账刷新列表；
    /// **成功则同账号把运行态条目一并摘掉**（否则状态栏 tab / 左 dock 面板 /
    /// 文件树菜单 / JS 运行时全活着，删了还能用直到重启）。
    /// 失败原因进头段错误行（pf_install 同款通道）。
    fn pf_uninstall(&mut self, name: String, cx: &mut gpui::Context<Self>) {
        if self.aux.market.pf.removing.is_some() {
            return;
        }
        self.aux.market.pf.removing = Some(name.clone());
        self.aux.market.pf.error = None;
        // 先就地撤运行态（drop wasm 实例 → 释放 .cache/*.cwasm 的 mmap），
        // 否则后台删目录必被 Windows 拒绝（os error 32，文件被占用）。
        if let Some(ix) = self.plugin_ix_by_pkg(&name) {
            self.plugin_evict(ix);
            self.plugins.plugins.remove(ix);
            if matches!(self.layout.dock_panel, crate::DockPanel::Plugin(_)) {
                self.layout.dock_panel = crate::DockPanel::Default;
            }
        }
        let pkg_name = name.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { pf_plugins::uninstall(&name) })
                .await;
            let _ = this.update(cx, |c, cx| {
                let ok = result.is_ok();
                c.aux.market.pf.removing = None;
                c.aux.market.pf.error = result.err();
                c.aux.market.pf.ledger = pf_plugins::load_ledger();
                if ok {
                    // 失效提示随包走：卸了就不再提醒更新
                    c.aux.market.pf.stale.retain(|s| s.name != pkg_name);
                }
                if c.aux
                    .market
                    .pf
                    .section
                    .as_deref()
                    .is_some_and(|s| !c.aux.market.pf.ledger.iter().any(|l| l.name == s))
                {
                    c.aux.market.pf.section = None;
                }
                if ok && let Some(ix) = c.plugin_ix_by_pkg(&pkg_name) {
                    let id = c.plugins.plugins[ix].manifest.id.clone();
                    c.plugin_evict(ix);
                    c.plugins.plugins.remove(ix);
                    crate::pf_plugin::plugin_forget(&id);
                    // 下标位移：插件面板存的是位置，任何一条被摘都退默认
                    if matches!(c.layout.dock_panel, crate::DockPanel::Plugin(_)) {
                        c.layout.dock_panel = crate::DockPanel::Default;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// 卸载确认浮层（040 同款机制，叠在一切之上；shell/render 顶层挂）。
pub(crate) fn remove_confirm_overlay(
    chat: &Chat,
    cx: &mut gpui::Context<Chat>,
) -> Option<gpui::AnyElement> {
    let name = chat.aux.market.pf.confirm_remove.as_ref()?;
    let display = index_snapshot()
        .entry(name)
        .map(|e| e.display.zh.clone())
        .unwrap_or_else(|| name.clone());
    let message = tf("market.pf.uninstall_confirm", &[("name", display)]);
    let weak = cx.entity().downgrade();
    Some(crate::ui::overlay::confirm(
        &chat.dlg.dialog_focus,
        message,
        {
            let weak = weak.clone();
            move |_, cx| {
                let _ = weak.update(cx, |c, cx| c.pf_remove_dialog_close(false, cx));
            }
        },
        move |_, cx| {
            let _ = weak.update(cx, |c, cx| c.pf_remove_dialog_close(true, cx));
        },
    ))
}
