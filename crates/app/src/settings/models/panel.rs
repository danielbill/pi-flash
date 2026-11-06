//! 面板生命周期与选中流：open_settings 六页入口、面板重载、错误条、
//! Provider 选中/移除确认、模型引用清单与 Provider 选择弹窗。

use super::*;

impl Chat {
    pub(crate) fn open_settings(&mut self, tab: u8, cx: &mut Context<Self>) {
        self.reload_settings_panel();
        let section = super::SettingsPanel::prefill_section(self, tab);
        let panel = cx.new(|cx| {
            let mut panel = SettingsPanel::new(cx);
            panel.tab = tab;
            panel.section = section.clone();
            panel
        });
        // 043 MCP 页：打开即回填选中行的编辑表单（与点行同一条链路）
        if tab == super::TAB_MCP {
            let data = self.mcp_backfill_data(&section);
            panel.update(cx, |s, cx| super::mcp::mcp_apply_backfill(s, data, cx));
        }
        // 模型页：归属判定（内置 vs 自定义）依赖全量注册表——缓存命中同步
        // 可用，否则后台一次性 dump（幂等；与 provider 弹窗同款）
        if tab == super::TAB_MODELS {
            self.ensure_registry(cx);
        }
        self.dlg.settings = Some(panel);
        cx.notify();
    }
}
impl Chat {
    /// 把启动装载的全局态 + 当前项目的项目上下文**安装**进设置页字段
    /// （010-启动.md §1/§5/§7）。纯内存赋值：不再逐项扫盘——skills / packages /
    /// mcp 都来自启动时已装好的 `globals` 与 `project_ctx`；
    /// 切项目命中集合就直接切，未命中时由 `project_ctx_for` 现算一次并纳入。
    pub(crate) fn reload_settings_panel(&mut self) {
        self.reload_model_defaults();
        // 凭据（PF 自有账本 pf-auth.json；auth.json 归 pi，PF 不读）
        // 042-1：凭据清单来自 pi 的 auth.json（PF 标记条目 + 用户明文 + oauth），
        // 不再有 PF 私有账本
        self.cfg.mc_creds = pi_link::auth_json::kinds().unwrap_or_default();
        self.cfg.mj_probe_cache = services::custom_models_store::read();
        // 存量 models.json 形状自愈（残缺 cost 让 pi 整份拒载）——先于对账
        self.mj_repair_models_json();
        // 存量对账（0.3.2 勾选漏写盘善后）：显式勾选却没进 models.json 的
        // 自定义商模型补物化——仓装载之后、其余字段之前；无事可做零写盘
        self.mj_reconcile_custom_selection();
        // 会话预设 `configured` 对应的工具清单（settings.json defaultTools）
        self.cfg.mc_default_tools = self.pool.globals.default_tools.clone();
        self.cfg.mc_pkgs_global = self.pool.globals.packages.clone();
        let ctx = self.project_ctx_now();
        self.cfg.mc_pkgs_project = ctx.packages;
        self.cfg.mc_project_scope = ctx.project_scope;
        // 技能表盘上现读：project_ctx 是启动缓存，本页开关写盘后拿它回读
        // 只会得到旧列表（“点了没反应”）。同时回写缓存，让 composer 的
        // skill 命令跟手。
        self.cfg.mc_skills = self.skills_fresh();
        // 子代理页：盘上现读（~/.pi/agent/agents/*.md + 内置；自定义在前，
        // 同名覆盖内置）——同 skills/MCP，写盘后必须重读才跟手
        self.cfg.mc_agents = pi_link::agents::list_all_agents();
        // 043：MCP 盘上现读——project_ctx 是启动缓存，本页写盘后重读只会
        // 拷到旧列表（040 globals 内存副本同款教训）
        let (mcp_servers, mcp_errors) = pi_link::mcp::load(self.project());
        self.cfg.mcp_servers = mcp_servers;
        self.cfg.mcp_errors = mcp_errors;
        // 自定义 provider 账本**不留副本**：各读点现读 providers.json，
        // 这里只把「读失败」记成错误条（文件缺失是正常空账本）
        self.cfg.mc_mj_error = pi_link::models_json::read().err();
    }

    pub(crate) fn mc_set_error(&mut self, msg: &str, cx: &mut Context<Self>) {
        if let Some(st) = self.dlg.settings.clone() {
            st.update(cx, |s, _| s.error = Some(msg.to_string()));
        }
        cx.notify();
    }

    pub(crate) fn mc_clear_error(&mut self, cx: &mut Context<Self>) {
        if let Some(st) = self.dlg.settings.clone() {
            let had = st.read(cx).error.is_some();
            if had {
                st.update(cx, |s, cx| {
                    s.error = None;
                    cx.notify();
                });
            }
        }
    }
}
impl Chat {
    /// 选中一个 catalog provider：已配置的把已存 key 解出预填进输入框
    /// （掩码态，眼睛展开）；未配置清空。筛选同步重置（换 provider 重置表单）。
    pub(crate) fn mc_select_provider(&mut self, id: String, cx: &mut Context<Self>) {
        let saved = pi_link::auth_json::resolve_secret(&id);
        if let Some(st) = self.dlg.settings.clone() {
            st.update(cx, |s, cx| {
                s.section = id;
                s.error = None;
                s.key_input.update(cx, |ti, cx| {
                    ti.set_value(saved.unwrap_or_default(), cx);
                    ti.set_masked(true, cx);
                });
                s.model_filter
                    .update(cx, |ti, cx| ti.set_value(String::new(), cx));
                cx.notify();
            });
        }
    }

    /// 042 详情头【移除】：确认后删 key + 合并列表除名。仅内置商走这里
    /// （自定义商的【移除】在表单头，删整条账本）。
    pub(crate) fn mc_ask_remove_provider(&mut self, provider: String, cx: &mut Context<Self>) {
        self.cfg.mc_remove_confirm = Some(provider);
        cx.notify();
    }

    pub(crate) fn mc_remove_provider_close(&mut self, ok: bool, cx: &mut Context<Self>) {
        let Some(provider) = self.cfg.mc_remove_confirm.take() else {
            return;
        };
        if !ok {
            cx.notify();
            return;
        }
        // 042-1【移除】= 断开连接：删掉该商的 api_key 凭据（用户已确认）。
        // 列表成员资格靠真实状态维持（内置商 = 有凭据；自定义商 = 有条目），
        // 凭据没了商家自然从列表消失——不需要任何隐藏清单。oauth 归 pi，不动。
        if let Err(e) = pi_link::auth_json::delete_api_key(&provider) {
            self.mc_set_error(&e, cx);
            return;
        }
        self.cfg.mc_probe.remove(&provider);
        self.reload_settings_panel();
        let first = self.mc_provider_ids().first().cloned().unwrap_or_default();
        self.mc_select_provider(first, cx);
        cx.notify();
    }
}
/// 042 详情头【移除】的确认浮层（与扩展卸载确认同款机制，叠在设置弹窗之上）。
pub(crate) fn remove_provider_confirm_overlay(
    chat: &Chat,
    cx: &mut Context<Chat>,
) -> Option<gpui::AnyElement> {
    let provider = chat.cfg.mc_remove_confirm.clone()?;
    let name = crate::ui::provider_display_name(&provider).to_string();
    let weak = cx.entity().downgrade();
    let message = crate::i18n::tf("extensions.remove_key_confirm", &[("name", name)]);
    Some(crate::ui::overlay::confirm(
        &chat.dlg.dialog_focus,
        message,
        {
            let weak = weak.clone();
            move |_, cx| {
                let _ = weak.update(cx, |c, cx| c.mc_remove_provider_close(false, cx));
            }
        },
        move |_, cx| {
            let _ = weak.update(cx, |c, cx| c.mc_remove_provider_close(true, cx));
        },
    ))
}

// ---------------------------------------------------------------------------
// 模型引用与 Provider 选择弹窗（P2 归并自 actions_terminal/actions_rename）
// ---------------------------------------------------------------------------

impl Chat {
    /// `provider/modelId` refs of every available model, display order.
    /// Reads the shared catalog of the current project (models_by_cwd).
    pub(crate) fn mc_refs(&self) -> Vec<String> {
        self.catalog_with_ledger()
            .iter()
            .map(|m| format!("{}/{}", m.provider, m.id))
            .collect()
    }

    /// 042 ProviderPicker 弹窗构造：搜索输入每敲一字通知重渲染（清单按值
    /// 过滤在渲染期读取快照）。ESC 交给浮层公共基座，这里只兜输入框内路径。
    /// 打开即确保全量注册表在拉（缓存命中则同步可用）。
    pub(crate) fn provider_picker_dialog(&mut self, cx: &mut Context<Self>) -> Dialog {
        self.ensure_registry(cx);
        let weak_esc = cx.entity().downgrade();
        let input = cx.new(|cx| TextInput::new(cx).placeholder(tr("model.search_provider")));
        input.update(cx, |ti, _| {
            ti.set_on_escape(Box::new(move |cx| {
                // same dispatch-lease hazard as submit: closing the dialog
                // drops the dispatching input — defer the dismissal
                let weak = weak_esc.clone();
                cx.defer(move |cx| {
                    let _ = weak.update(cx, |c, cx| {
                        c.dlg.dialog = None;
                        cx.notify();
                    });
                });
            }));
        });
        Dialog::ProviderPicker {
            input,
            scroll: gpui::ScrollHandle::new(),
        }
    }
}
