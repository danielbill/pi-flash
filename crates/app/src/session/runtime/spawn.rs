//! pi 进程 spawn 参数装配：`--session`/工具预设/扩展挂载/模型思考旗标/
//! 执行域 shim 全在这一个函数收口；挂载源注册过滤（tc6）判定件同住。

use futures::channel::mpsc::UnboundedReceiver;
use pi_link::protocol::Event;

use super::*;

/// 挂载源放行判定（tc6）：插件包源（`/plugins/<pkg>/…`）须注册在案且未停用；
/// 非插件包源（PF 扩展等）不过问。
pub(super) fn ext_source_allowed(src: &str, ledger: &[pi_link::pf_plugins::LedgerEntry]) -> bool {
    let norm = src.replace('\\', "/");
    if !norm.contains("/plugins/") {
        return true;
    }
    ext_source_registered(src, ledger)
}

/// 挂载源注册状态检查（90-9 §8 摘除面，tc6）：src 形如 `<…>/plugins/<pkg>/…`
/// → 包目录须有清单（未卸载）且台账未停用。清单两档布局都认（090-10 版本层
/// `<pkg>/<ver>/pf-plugin.toml` / 平铺 dev `<pkg>/pf-plugin.toml`）——只认顶层
/// 会把已装包误判成已卸载，ext 被 spawn 过滤（真机 2026-11-02 wxbot 出码无人执行）。
pub(super) fn ext_source_registered(
    src: &str,
    ledger: &[pi_link::pf_plugins::LedgerEntry],
) -> bool {
    let norm = src.replace('\\', "/");
    let Some(idx) = norm.rfind("/plugins/") else {
        return false;
    };
    let rest = &norm[idx + "/plugins/".len()..];
    let Some(pkg) = rest.split(['/', '\\']).next() else {
        return false;
    };
    if pkg.is_empty() {
        return false;
    }
    let pkg_dir = std::path::Path::new(&norm[..idx + "/plugins/".len() + pkg.len()]);
    if pi_link::service_ext::pkg_manifest_path(pkg_dir).is_none() {
        return false; // 已卸载
    }
    // pkg 是**磁盘目录名**（`@scope__slug` 或 dev 平铺短名）；台账 name 是
    // scope 名（`@scope/slug`）。dir_aliases 只吃 scope 名（单向 /→__），
    // 喂目录名恒等返回、永不命中——market 插件 ext 全被这里过滤掉
    // （真机 2026-11-02：wxbot 出码无人执行）。双向口径都比一遍。
    ledger.iter().any(|l| {
        !l.disabled
            && (pi_link::pf_plugins::dir_name(&l.name) == pkg
                || l.name == pkg
                || l.name.rsplit('/').next() == Some(pkg))
    })
}

impl SessionRuntime {
    /// Spawn (or respawn) the pi process for this session. `background`
    /// callers wrap this on the background executor; returns the event
    /// receiver for the pump.
    pub(crate) fn spawn(&mut self) -> Option<UnboundedReceiver<Event>> {
        // 只读观察窗永不 spawn（044-0）：子会话的驱动权归父 pi 进程，这里
        // 再起一个进程 = 第二驱动（分叉账本/双花 token）。所有 spawn 入口
        // （open 预热/ensure_spawned/发送链）都汇到这一道闸。
        if self.observer {
            return None;
        }
        let mut extra: Vec<String> = Vec::new();
        if let Some(f) = &self.file {
            extra.push("--session".into());
            extra.push(f.to_string_lossy().into());
        }
        // tc6：挂载源注册过滤的台账（custom 档 ext_sources + store 清单共用）
        let ledger = pi_link::pf_plugins::load_ledger();
        // 090-10 版本层自愈：ext_sources 里的挂载路径可能指着上一版本的
        // 目录（运行中升级插件 / 台账自愈前的存量）——先重解析再拼参数，
        // 免得两段（custom 档 / 台账段）各挂一份或挂上死的 `-e`
        let (healed_ext, ext_changed) = pi_link::session_ext::resolve_sources(&self.ext_sources);
        if ext_changed {
            self.ext_sources = healed_ext;
        }
        match self.tools_preset.as_str() {
            // spawn-arg tool selection (pi-web uses in-process
            // setActiveToolsByName; the RPC surface has no live switch).
            // "configured" sends nothing — pi resolves settings.json
            // defaultTools exactly like the CLI does.
            "chat-only" => extra.push("--no-tools".into()),
            "read-only" => {
                extra.push("--tools".into());
                // present_plan 必须点名（031-1）：--tools 是注册级 allowlist，
                // 不点名的扩展工具连注册都进不去
                extra.push(format!(
                    "read,grep,find,ls,{}",
                    crate::mode::PRESENT_PLAN_TOOL
                ));
            }
            "default" => {
                extra.push("--tools".into());
                extra.push(format!(
                    "read,bash,edit,write,{}",
                    crate::mode::PRESENT_PLAN_TOOL
                ));
            }
            // full：内置 7 件（--tools）+ agent（045 subagent，模块运行时
            // 开关开着才点名——allowlist 指向不存在的注册工具没有意义）
            // + 精确集（-ne + 个人扩展 + 内置扩展）⇒ 插件一个不装 ⇒
            // 系统提示词零插件注入（034 §1.2）
            "full" => {
                extra.push("--tools".into());
                // present_plan 同上（031-1 注册级 allowlist 点名）
                let tools = if crate::services::workspace::subagent_module_active() {
                    format!(
                        "bash,read,edit,write,grep,find,ls,agent,{}",
                        crate::mode::PRESENT_PLAN_TOOL
                    )
                } else {
                    format!(
                        "bash,read,edit,write,grep,find,ls,{}",
                        crate::mode::PRESENT_PLAN_TOOL
                    )
                };
                extra.push(tools);
                extra.extend(crate::session::tools_recipe::full_args(&self.cwd));
            }
            // 自定义（原 full+plugins）：不发 --tools（注册级 allowlist 会把
            // 插件工具挡在注册之外），走 -ne + 逐条 -e + full_activate.ts
            "custom" => {
                // tc6：ext_sources 同过注册过滤（创建/rebind 链路可能带回
                // 已停用插件的残留源）
                let ext_sources: Vec<String> = {
                    let mut v = self.ext_sources.clone();
                    v.retain(|src| ext_source_allowed(src, &ledger));
                    v
                };
                extra.extend(crate::session::tools_recipe::full_plugin_args(
                    &ext_sources,
                    &self.cwd,
                ));
            }
            _ => {}
        }
        // 外部委派面与 `agent` 撞车（2026-11-08 真机）：billion-context 扩展会注册
        // `acp_delegate` / `_wait` / `_cancel`（默认异步 + runId，与 PF 的
        // dispatch-and-return 语义相反）。模型拿 childId 去 `acp_delegate_wait`
        // 报错、再把 `subagent_list` 当状态轮询，最后 `bash sleep 30`。
        // 关它靠 `pi.subagents.enabled=false`（044-5 §4.7），但那份配置会被
        // bili 自己的 web UI 整份重写成它的 schema（已实测丢键），所以这里在
        // spawn 参数上再钉一次：denylist 对扩展工具同样生效。
        extra.push("--exclude-tools".into());
        extra.push("acp_delegate,acp_delegate_wait,acp_delegate_cancel".into());
        // 会话级扩展清单（031 session-ext.json；契约 v2.4/v3 pi_extensions
        // 的挂载通道）：**所有档位统一消费**——此前只有 custom 档读它，
        // 插件 ext 在 full/default 档下永远挂不上（-e 缺席，出码/收发无人
        // 执行）。已挂过的（custom 档经 ext_sources）不重复。
        if let Some(store) = pi_link::session_ext::store_path() {
            // 停用/卸载后的残留挂载条目必须失效（真机 2026-10-31：面板 off 重启
            // 后 ext 照跑还能收发——store 只增不减，spawn 前按注册状态过滤；
            // 插件仓 tc6 锁回归）。版本换代同样自愈 + 回写（090-10）：台账里
            // 的绝对路径带版本目录，插件升级后旧目录被删 → 死 `-e` 让 pi 起
            // 不来（真机 2026-11-03 重启后微信插件全哑的根因）
            let raw = pi_link::session_ext::read_for(&store, &self.key);
            let (mounts, changed) = pi_link::session_ext::resolve_sources(&raw);
            if changed && let Err(e) = pi_link::session_ext::write_for(&store, &self.key, &mounts) {
                eprintln!("[session-ext] 挂载台账自愈回写失败（{}）: {e}", self.key);
            }
            for src in mounts {
                // 契约 v4：插件的 pi/*.mjs 是执行域入口，再当扩展挂载会整批 -e 失败
                if pi_link::service_ext::is_exec_entry(std::path::Path::new(&src)) {
                    continue;
                }
                if !ext_source_allowed(&src, &ledger) {
                    continue;
                }
                if !self
                    .ext_sources
                    .iter()
                    .any(|e| pi_link::session_ext::same_source(e, &src))
                {
                    extra.push("-e".into());
                    extra.push(src);
                }
            }
        }
        // 计划模式扩展（031-1 P0）：全档位 `-e` 挂载——闸门在 pi 进程内
        // （tool_call 打回写操作，无工具集切换），机制与定位见 crate::mode。
        extra.extend(crate::mode::mount_args());
        // pi 原则：不逐次审批工具调用（vendor security.md），任何档位都
        // 不挂 permission 门禁；扩展自己的 ctx.ui.* 请求仍照常呈现（060 翻案）。
        // draft picks made before the process existed ride the spawn flags
        // (pi CLI parity: --model provider/id, --thinking level)
        // 042：无手选时落 PF 默认模型——只在全新 draft（`file` 空）生效，
        // 重绑/恢复的会话不强行切模型
        let model = match self.pending_model.take() {
            Some(m) => Some(m),
            None if self.file.is_none() => self.default_model.clone(),
            None => None,
        };
        if let Some((provider, id)) = model {
            extra.push("--model".into());
            extra.push(format!("{provider}/{id}"));
        }
        if let Some(level) = self.thinking_override.clone() {
            extra.push("--thinking".into());
            extra.push(level);
        }
        // 执行域挂载（90-6）：存在实例配置 → 挂宿主自带 shim + 交付配置指针。
        // 配置由 Chat 侧随插件装/卸同步（svc_sync_configs）；spawn 只消费。
        let mut envs: Vec<(String, String)> = Vec::new();
        // 005 §② 身份注入：ext 的会话身份由宿主给定（= 挂载台账键 / 命令
        // env session.key 规范键），插件不再用 uuid/getSessionFile/argv 猜
        // （首条消息前 undefined + uuid 随重启漂移，门控必失配——真机
        // 2026-11-03）。所有 pi 扩展通用。
        envs.push(("PF_SESSION_KEY".into(), self.key.clone()));
        // 044-0 模块设置传导：并发/深度从设置页到 pf-subagents（env 覆盖
        // 扩展默认值；模块开关关着 = 空 vec，委派停用无意义）。
        envs.extend(crate::services::workspace::subagent_envs());
        if let Some(base) = pi_link::paths::dir() {
            if let Some(cfg) = pi_link::service_ext::config_path_if_any(&base, &self.key) {
                if cfg.is_file() {
                    match pi_link::service_ext::shim_path() {
                        Some(shim) => {
                            extra.push("-e".into());
                            extra.push(shim.to_string_lossy().into_owned());
                            // 信箱残留清零：上一轮已消费事件不得重放（至多一次）
                            if let Some(token) =
                                pi_link::service_ext::token_if_any(&base, &self.key)
                            {
                                pi_link::service_ext::reset_spools(&base, &token);
                            }
                            pi_link::service_ext::note_shim(
                                &base,
                                &format!("挂载 shim={} cfg={}", shim.display(), cfg.display()),
                            );
                            envs.push(("PF_SVC_CONFIG".into(), cfg.to_string_lossy().into_owned()));
                        }
                        None => {
                            eprintln!("[pf-svc] 本构建缺执行域 shim，service 落空（{}）", self.key);
                            pi_link::service_ext::note_shim(
                                &base,
                                &format!("shim 定位失败 key={}", self.key),
                            );
                        }
                    }
                }
            }
        }
        self.agent.spawn_with(
            &self.cwd,
            &extra,
            &envs,
            // 044-0：模块运行时开关（开发旗标 && 用户面）——关着就不挂
            // pf-subagents，子代理四件工具不注册（设置页关开关的另一半
            // 在 `Chat::apply_subagent_switch`：空闲会话立即重绑）。
            crate::services::workspace::subagent_module_active(),
        )
    }
}
