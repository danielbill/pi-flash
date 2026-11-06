//! PF 插件宿主——面板形态打开（契约 v2.4 dock）：状态栏 tab → dock 面板 /
//! 内容区预览页签（单例），与插件认领会话（new_draft，002 设置会话泛化）。

use gpui::AppContext;
use serde_json::json;

use crate::Chat;

use super::*;

impl Chat {
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
}
