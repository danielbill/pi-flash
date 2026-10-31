//! PF 插件宿主接线（beads 360；契约 docs/PF组件规范/01+02，设计 090-4）。
//!
//! 职责（02 §6 责任清单）：①启动扫 `<数据目录>/plugins/*/pf-plugin.toml`
//! （极简解析，坏清单跳过记日志）；②状态栏 tab（status_bar.rs 消费）；
//! ③文件树右键【插件 → …】（psp_overlays.rs 消费）；④面板渲染（panel.rs，
//! 组件树先硬编码快捷面板这棵，解析器后续抽象）；⑤ctx/懒加载/异常隔离
//! （本文件，引擎 = `pf_plugin_host::js::JsPlugin`）。
//!
//! 时序（02 §4）：清单启动读（spawn_plugin_scan，后台），ctx 首用时
//! 建；插件异常 → toast + 会话级禁用，不影响宿主。

mod broadcast;
pub(crate) mod lifecycle;
pub(crate) mod panel;
pub(crate) mod svc;
mod wasm_hooks;

pub(crate) use wasm_hooks::spawn_precompile_installed;
use wasm_hooks::{host_proof_material, install_prepare, verify_wasm_plugins};

// ---------------------------------------------------------------------------
// 子模块（090 重构拆分：清单解析 / 运行态条目 / 宿主 Chat 方法·装载与面板 /
// 宿主 Chat 方法·动作与分派）——re-export 维持 `crate::pf_plugin::X` 旧路径
// ---------------------------------------------------------------------------

pub(crate) mod entry;
pub(crate) mod host_actions;
pub(crate) mod host_tabs;
pub(crate) mod manifest;

pub(crate) use entry::*;
pub(crate) use host_actions::*;
pub(crate) use manifest::*;

use std::path::Path;

use gpui::SharedString;

use crate::Chat;

/// 会话键暂缺省 null（壳侧会话身份走镜像分账，ctx.session 留给后续装载点
/// 逐调用填）；cwd = 当前项目目录。
pub(crate) fn plugin_ctx_json(id: &str, m: &PluginManifest, session_key: Option<&str>) -> String {
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    serde_json::json!({
        "plugin": {
            "id": id,
            "version": m.version,
            "dataDir": m.dir.join("data").to_string_lossy(),
        },
        "pf": {"version": env!("CARGO_PKG_VERSION"), "locale": crate::i18n::LANG_LABELS[crate::i18n::lang_ix()]},
        "session": session_key.map(|k| serde_json::json!({"key": k})),
        "cwd": cwd,
    })
    .to_string()
}

/// 状态机转移（90-9 §5）：条目状态只能经 [`lifecycle::transition`] 变更。
/// 非法转移保持原状——状态机不 panic，宿主事实以 plugin.log 为准。
fn entry_state(entry: &mut PluginEntry, e: lifecycle::Ev) {
    if let Some(s) = lifecycle::transition(entry.state, e) {
        entry.state = s;
    }
}

/// 单条目 unload（90-9 §2/§6）：调载体 `unload()`（未导出 = no-op）+ 落
/// plugin.log + 状态 Active→Unloading→Disabled。best-effort：unload 的
/// Err 只进日志，宿主不等确认。条目若无运行时（从未装载过），无事发生、
/// 不落日志——load/unload 必须成对。
pub(crate) fn entry_unload(entry: &mut PluginEntry) {
    let has_runtime = entry.js.is_some() || entry.wasm.is_some();
    if !has_runtime {
        return;
    }
    let dir = entry.manifest.dir.clone();
    entry_state(entry, lifecycle::Ev::UnloadStart);
    let t0 = std::time::Instant::now();
    let mut err: Option<String> = None;
    if let Some(js) = entry.js.as_ref()
        && let Err(e) = js.unload()
    {
        err = Some(e);
    }
    if let Some(w) = entry.wasm.as_mut()
        && let Err(e) = w.unload()
    {
        err = err.or(Some(e));
    }
    lifecycle::log_unload(
        &dir,
        err.is_none(),
        t0.elapsed().as_millis(),
        err.as_deref().unwrap_or(""),
    );
    entry_state(entry, lifecycle::Ev::UnloadDone);
}

/// 清单 → 运行条目（disabled 一律 false 起步：停用已在扫描层被台账过滤；
/// 运行中的禁用/复用走原地翻 entry.disabled 位，090-4 v5）。
pub(crate) fn plugin_entries(manifests: Vec<PluginManifest>) -> Vec<PluginEntry> {
    manifests
        .into_iter()
        .map(|manifest| PluginEntry {
            manifest,
            state: lifecycle::State::Registered,
            disabled: false,
            js: None,
            wasm: None,
            wasm_focus: None,
            host: None,
            tree_loaded: false,
            cell_menu: None,
            split_ratios: std::cell::RefCell::new(Vec::new()),
            split_drag: std::cell::RefCell::new(None),
            split_hover: std::cell::RefCell::new(None),
            section_scrolls: std::cell::RefCell::new(Vec::new()),
            body: None,
        })
        .collect()
}

/// 启动扫描（spawn_boot_tasks 调；首帧后异步落位，微秒级/个）。
///
/// 两段式，**主线程零重活**：
/// ① 清单扫描（后台线程）→ JS 插件条目立即落位（状态栏 tab / 右键【插件】可见）；
/// ② 内层 wasm 插件验签门（090-5 L3）：**揭幕后**才在后台线程发起，且走
/// 编译产物缓存（`load_cached`，命中 = mmap 反序列化 → 毫秒级）。
///
/// **验签结论不向用户展示**（正版校验/激活/拒载一类提示一律不出）——插件
/// 能不能用看它自己的面板表现，启动不播报；结论只进 stderr 供开发查。
pub(crate) fn spawn_plugin_scan(cx: &mut gpui::Context<Chat>) {
    // 组件状态广播（契约 v2.4）：observe Chat 自身，差分派发给订阅插件
    broadcast::attach(cx);
    // 执行域 outbox 泵（90-6 通道）挂到既有 120ms 主泵上（startup::spawn_pump_120ms）：
    // 早先在这里自起一条 cx.spawn 循环，实测「只跑第一拍就再也不进」
    // ，
    // 不再自开节拍，复用那条已验证长跑的循环。
    // 安装尾声钩子：wasm → cwasm 预编译压在安装那一次做完（pi-link 不依赖
    // wasmtime，故由 app 注入）。纯函数指针，零开销。
    pi_link::pf_plugins::install_prepare_hook(install_prepare);
    // 宿主证明钩子**启动即注入**（进程级，线程无关）：晚注入会让先加载的
    // wasm 插件读到 fallback 占位符 → 误判「fork/无证明构建」而拒载。
    pf_plugin_host::install_proof_hook(host_proof_material);
    // 执行域桥（90-6）：wasm 载体 ctx.service ↔ 与 JS 同一条通道实现
    pf_plugin_host::install_svc_hooks(
        |key, payload| svc::svc_post_anon(key, payload),
        |key| svc::svc_query(key),
    );
    // 路径派生能力（与 JS 插件 ctx.fileIcon / ctx.isExecutive 同一套服务）
    pf_plugin_host::install_path_hooks(
        |p| crate::services::file_icons::get_icon(Path::new(p)).to_string(),
        |p| crate::services::script_exec::exec_kind(Path::new(p)).is_some(),
    );
    cx.spawn(async move |this, cx| {
        let manifests = cx
            .background_executor()
            .spawn(async move {
                // 先清无主残留（卸载时被占用留下的 .cache 等），再扫
                pi_link::pf_plugins::sweep_orphans();
                let manifests = scan_manifests();
                // 90-9 §7 启动对账：上次异常退出（plugin.log 存在未闭合
                // load）→ 补一条「上次异常退出，未执行 unload」。后台线程
                // 做掉，主线程零成本。
                for m in &manifests {
                    lifecycle::reconcile_startup(&m.dir);
                }
                manifests
            })
            .await;
        let (wasm, js): (Vec<_>, Vec<_>) = manifests.into_iter().partition(|m| m.wasm.is_some());
        // ① 全部落位（JS+wasm）：状态栏 tab 立即可见；wasm 实例本身仍是懒装载
        // （首次开面板才建，见 plugin_wasm_ready），落位只是清单/入口
        let _ = this.update(cx, |c, cx| {
            let mut entries = plugin_entries(js);
            entries.extend(plugin_entries(wasm.clone()));
            c.plugins_set(entries, cx);
        });
        if wasm.is_empty() {
            return;
        }
        // ② 等揭幕：验签是秒级 CPU 重活，不和启动抢核（揭幕前只有启动页）
        loop {
            if this.update(cx, |c, _| c.pool.booted).unwrap_or(true) {
                break;
            }
            cx.background_executor()
                .timer(std::time::Duration::from_millis(100))
                .await;
        }
        // ③ 验签门：拒载的包置 disabled（tab 隐藏，面板点不开）；通过的无事
        // （实例是懒装载的，这里只做安全把关 + stderr 播报）
        let rejected = cx
            .background_executor()
            .spawn(async move { verify_wasm_plugins(wasm) })
            .await;
        if !rejected.is_empty() {
            let _ = this.update(cx, |c, _| c.plugin_mark_rejected(&rejected));
        }
    })
    .detach();
}

// ---------------------------------------------------------------------------
// 内层 wasm 插件：宿主能力钩子桥（fn 指针拿不到 Chat/文本系统 → thread_local）
// ---------------------------------------------------------------------------

thread_local! {
 /// 当前项目收藏快照（插件 `quick-pins` import 的数据源；fn 指针拿不到
 /// Chat，每次 wasm 面板装载/动作后由 `plugin_sync_quick_pins` 重灌）
    static QUICK_PIN_SNAP: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };
 /// gpui 文本系统（`measure-text` import 用；`Arc`，首装后全程存活）
    static TEXT_SYSTEM: std::cell::RefCell<Option<std::sync::Arc<gpui::TextSystem>>> =
        const { std::cell::RefCell::new(None) };
}

fn quick_pin_snap() -> Vec<String> {
    QUICK_PIN_SNAP.with(|c| c.borrow().clone())
}

/// 插件声明的图标引用 → 可渲染资产路径（内置名 / `assets:x.svg` / 完整路径）。
/// 与面板内 `button.icon` 同一套规则，供清单 `file_menu.icon` 等复用。
pub(crate) fn plugin_icon_asset(plugin_id: &str, icon: &str) -> Option<String> {
    if icon.is_empty() {
        return None;
    }
    if icon.contains('/') {
        return Some(icon.to_string());
    }
    if let Some(name) = icon.strip_prefix("assets:") {
        return Some(format!("plugins/{plugin_id}/assets/{name}"));
    }
    if let Some(name) = icon.strip_prefix("assets/") {
        return Some(format!("plugins/{plugin_id}/{name}"));
    }
    Some(format!("icons/{icon}.svg"))
}

/// 状态栏 tab 图标的共享串（`plugins/<slug>/<assets 相对路径>` 或宿主内置名）。
pub(crate) fn status_bar_icon_path(entry: &PluginEntry) -> Option<SharedString> {
    let sb = entry.manifest.status_bar.as_ref()?;
    Some(contrib_icon_path(&entry.manifest.id, &sb.icon))
}

/// topbar「菜单」条目图标路径（契约 v2.5，eb6）：与 status_bar 同一映射。
pub(crate) fn topbar_menu_icon_path(entry: &PluginEntry) -> Option<SharedString> {
    let tb = entry.manifest.topbar_menu.as_ref()?;
    Some(contrib_icon_path(&entry.manifest.id, &tb.icon))
}
pub(crate) use panel::{cell_menu_layer, plugin_panel_view, plugin_popup_layer};
