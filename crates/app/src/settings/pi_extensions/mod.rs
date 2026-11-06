//! Extensions tab（040-Pi的扩展管理）：「扩展」= pi 的 package（settings.json
//! `packages` 条目）。顶部安装区（整条 `pi install …` 命令可直接粘贴，全局
//! only）+ 左侧已装列表（350px，行内开关 + 组头总开关，内容左右各留 15px）+
//! 右侧五行详情（说明取包内 package.json 的 description）。安装/卸载走
//! vendored pi CLI 后台线程，完成后 op 泵清 busy 并刷新列表（main.rs）。
//!
//! itlq.2 拆分（只搬不改）：本文件 = Chat 动作 + 页骨架 + 卸载确认浮层；
//! panes = 安装区/左右栏/更新按钮渲染。

use super::*;

pub(crate) mod panes;

pub(crate) use panes::{pl_install_bar, pl_sidebar, pl_detail};

impl Chat {
    /// 启停单个扩展：资源数组置空 = 停用，恢复字符串条目 = 启用
    /// （pi-web disable 语义），写全局 settings.json。
    pub(crate) fn mc_toggle_package(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        let Some(entry) = self.cfg.mc_pkgs_global.get(ix) else {
            return;
        };
        let source = pi_link::skills::entry_source(entry);
        let next: Vec<serde_json::Value> = self
            .cfg
            .mc_pkgs_global
            .iter()
            .enumerate()
            .map(|(i, e)| {
                if i != ix {
                    return e.clone();
                }
                if pi_link::skills::entry_disabled(e) {
                    serde_json::Value::String(source.clone())
                } else {
                    serde_json::json!({
                        "source": source,
                        "extensions": [], "skills": [], "prompts": [], "themes": []
                    })
                }
            })
            .collect();
        let path = pi_link::config::settings_path();
        if let Err(e) = pi_link::config::write_packages(&path, next.clone()) {
            self.mc_set_error(&crate::i18n::tf("settings.write_failed", &[("e", e)]), cx);
            return;
        }
        // globals 是启动时的内存副本：不同步的话 reload_settings_panel 拷回旧列表
        self.pool.globals.packages = next;
        self.reload_settings_panel();
        cx.notify();
    }

    /// 总开关：统一开启/关闭全部扩展（040 定稿，不做资源过滤豁免）。
    pub(crate) fn mc_toggle_packages_bulk(&mut self, enable: bool, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        let mut changed = 0usize;
        let next: Vec<serde_json::Value> = self
            .cfg
            .mc_pkgs_global
            .iter()
            .map(|e| {
                if pi_link::skills::entry_disabled(e) == !enable {
                    return e.clone(); // 已在目标状态
                }
                changed += 1;
                if enable {
                    serde_json::Value::String(pi_link::skills::entry_source(e))
                } else {
                    serde_json::json!({
                        "source": pi_link::skills::entry_source(e),
                        "extensions": [], "skills": [], "prompts": [], "themes": []
                    })
                }
            })
            .collect();
        if changed == 0 {
            return;
        }
        if let Err(e) =
            pi_link::config::write_packages(&pi_link::config::settings_path(), next.clone())
        {
            self.mc_set_error(&crate::i18n::tf("settings.write_failed", &[("e", e)]), cx);
            return;
        }
        self.pool.globals.packages = next;
        self.reload_settings_panel();
        cx.notify();
    }

    /// 安装（040：全局 only）。来源支持整条 `pi install …` 粘贴
    /// （normalize_source 剥前缀）；后台跑 vendored pi CLI，转圈在安装
    /// 按钮，完成后 op 泵清零并刷新列表。op 进行中忽略新请求。
    pub(crate) fn mc_install_package(&mut self, source: String, cx: &mut Context<Self>) {
        if self.cfg.pkg_op.is_some() {
            return;
        }
        self.mc_clear_error(cx);
        let source = pi_link::skills::normalize_source(&source);
        if source.is_empty() {
            self.mc_set_error(&tr("extensions.source_placeholder"), cx);
            return;
        }
        self.cfg.pkg_op = Some(crate::settings::pi_extensions::PkgOp::Install);
        self.mc_cli_op(
            vec!["install".to_string(), source.clone()],
            crate::i18n::tf("extensions.installed", &[("source", source.clone())]),
            cx,
        );
    }

    /// 卸载（040：移除按钮换成垃圾箱 icon，转圈在该垃圾桶上）。
    pub(crate) fn mc_remove_package(&mut self, source: String, cx: &mut Context<Self>) {
        if self.cfg.pkg_op.is_some() {
            return;
        }
        self.mc_clear_error(cx);
        self.cfg.pkg_op = Some(crate::settings::pi_extensions::PkgOp::Remove(
            source.clone(),
        ));
        self.mc_cli_op(
            vec!["remove".to_string(), source.clone()],
            crate::i18n::tf("extensions.removed", &[("source", source.clone())]),
            cx,
        );
    }

    /// 点垃圾桶：弹确认浮层（040，统一居中确认组件）。
    pub(crate) fn mc_ask_remove_package(&mut self, source: String, cx: &mut Context<Self>) {
        if self.cfg.pkg_op.is_some() {
            return;
        }
        self.cfg.pkg_confirm_remove = Some(source);
        cx.notify();
    }

    /// 确认浮层的「确认 / 取消」：ok=true 才真删。
    pub(crate) fn mc_remove_dialog_close(&mut self, ok: bool, cx: &mut Context<Self>) {
        let Some(source) = self.cfg.pkg_confirm_remove.take() else {
            return;
        };
        if ok {
            self.mc_remove_package(source, cx);
        } else {
            cx.notify();
        }
    }
}
/// 扩展页：顶部安装区 + 下方 左列表（300px）/ 右详情。
pub(crate) fn mc_pi_extensions_view(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
    install_input: &gpui::Entity<TextInput>,
    error: &Option<String>,
) -> gpui::AnyElement {
    let t = T();
    let entries: Vec<(usize, &serde_json::Value)> =
        chat.cfg.mc_pkgs_global.iter().enumerate().collect();
    div()
        .flex()
        .flex_col()
        .w_full()
        .h_full()
        .min_h_0()
        .child(pl_install_bar(
            weak,
            install_input,
            error,
            matches!(
                chat.cfg.pkg_op,
                Some(crate::settings::pi_extensions::PkgOp::Install)
            ),
            t,
        ))
        .child(two_pane(
            pl_sidebar(chat, weak, section, &entries, t),
            pl_detail(chat, weak, section, &entries, t),
        ))
        .into_any_element()
}

fn two_pane(sidebar: gpui::AnyElement, detail: gpui::AnyElement) -> gpui::AnyElement {
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .child(sidebar)
        .child(detail)
        .into_any_element()
}

/// 安装区（040）：输入框（400px，整条命令可粘贴，左缘对齐下方列表面板）
/// + 安装按钮；提示行在输入框下方。安装中按钮转圈禁用，错误行紧随其下。

/// 长路径折行（040）：分隔符后插零宽空格，让无空格的路径也能断行。
fn breakable_path(path: &str) -> String {
    path.replace('\\', "\\\u{200b}").replace('/', "/\u{200b}")
}

/// 卸载确认浮层（040）：统一 `overlay::confirm` 组件，自动居中。
pub(crate) fn remove_confirm_overlay(
    chat: &Chat,
    cx: &mut Context<Chat>,
) -> Option<gpui::AnyElement> {
    let src = chat.cfg.pkg_confirm_remove.as_ref()?;
    let name = pi_link::skills::display_source(src).to_string();
    let weak = cx.entity().downgrade();
    let message = crate::i18n::tf("extensions.uninstall_confirm", &[("name", name)]);
    Some(crate::ui::overlay::confirm(
        &chat.dlg.dialog_focus,
        message,
        {
            let weak = weak.clone();
            move |_, cx| {
                let _ = weak.update(cx, |c, cx| c.mc_remove_dialog_close(false, cx));
            }
        },
        move |_, cx| {
            let _ = weak.update(cx, |c, cx| c.mc_remove_dialog_close(true, cx));
        },
    ))
}

/// 扩展页后台 CLI op（040）：转圈落点不同——Install 在安装按钮，
/// Remove 在对应包的垃圾桶上；082 增 Update——Some(source) = 单包（转圈在
/// 详情【更新】按钮），None = 全部（转圈在列表 title【更新全部】按钮）；
/// op 泵收到完成消息即清零。
#[derive(Debug, Clone)]
pub(crate) enum PkgOp {
    Install,
    Remove(String),
    /// 082 更新：Some(source) = 单包；None = 全部
    Update(Option<String>),
}
