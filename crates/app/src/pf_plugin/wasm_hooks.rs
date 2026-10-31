//! 内层 wasm 插件的宿主钩子与验签/预编译链（090-5；自 mod.rs 拆出，
//! 架构守卫单文件行数红线）。
//!
//! fn 指针拿不到 Chat/文本系统 → 宿主状态仍走 mod.rs 的 thread_local
//! （`TEXT_SYSTEM` / `QUICK_PIN_SNAP`），本文件只做桥接与重活（验签门、
//! 编译产物预编译）。

use std::path::Path;
use std::sync::OnceLock;

use super::PluginManifest;

/// `measure-text`：App 级文本系统逐字符 advance 求和（无 window 依赖；
/// kerning 不计入，插件排版够准）。字体未注册 → 0.0（兑底是插件的责任，
/// 与 fs 缺失同款）。
pub(super) fn wasm_measure_text(content: &str, size: f32) -> f32 {
    let Some(ts) = super::TEXT_SYSTEM.with(|c| c.borrow().clone()) else {
        return 0.0;
    };
    let font = gpui::Font {
        family: crate::appearance::panel_font().family.into(),
        features: gpui::FontFeatures::default(),
        fallbacks: None,
        weight: gpui::FontWeight::NORMAL,
        style: gpui::FontStyle::Normal,
    };
    let id = ts.resolve_font(&font);
    let size = gpui::px(size);
    let mut w = 0.0f32;
    for ch in content.chars() {
        if ch == '\n' {
            continue;
        }
        if let Ok(a) = ts.advance(id, size, ch) {
            w += f32::from(a.width);
        }
    }
    w
}

/// wasm 宿主钩子懒注入（首次 wasm 装载前，主线程）：文本系统 + 快照读口。
static WASM_HOOKS: OnceLock<()> = OnceLock::new();
pub(super) fn ensure_wasm_hooks(cx: &gpui::App) {
    if WASM_HOOKS.set(()).is_err() {
        return;
    }
    super::TEXT_SYSTEM.with(|c| *c.borrow_mut() = Some(cx.text_system().clone()));
    pf_plugin_host::install_host_hooks(super::quick_pin_snap, wasm_measure_text);
 // 幂等再兜一次（启动路径已装；这里保证任何入口先于 init 都有证明材料）
    pf_plugin_host::install_proof_hook(host_proof_material);
}

/// 内层 wasm 插件验签门（090-5 L3）——**只许后台线程调用**。
///
/// init 即验签门；实例即验即弃（运行实例是懒装载的另一份，见
/// plugin_wasm_ready）。返回**拒载包 id**：结论只进 stderr + 回传主线程置
/// disabled，不向用户播报任何正版校验提示。
///
/// **不落 plugin.log**（90-9 §7）：验签实例不是 serving 实例，记 load 会
/// 留下永不闭合的配对，启动崩溃判定全数误报。
///
/// 速度：装载走**插件目录内的**编译产物缓存（`<插件目录>/.cache/<sha256>.cwasm`）——
/// 安装时已预编译（见 [`install_prepare`]），故这里恒命中；之后每次 =
/// mmap 反序列化 + 实例化 + init（毫秒级）。旧的 `load()` 每次启动重编
/// （4.6 MB 组件 7.2 s debug）。
pub(super) fn verify_wasm_plugins(manifests: Vec<PluginManifest>) -> Vec<String> {
 // 证明材料钩子是进程级（pf-plugin-host 里从 thread_local 改成全局），
 // 后台线程 init 时插件回调 get_host_proof 同样取得到材料
    pf_plugin_host::install_proof_hook(host_proof_material);
    let mut rejected = Vec::new();
    for m in manifests {
        let id = m.id.clone();
        let wasm_path = m.dir.join(m.wasm.as_deref().unwrap_or("plugin.wasm"));
        let cache = pi_link::pf_plugins::plugin_cache_for(&m.dir);
        let t0 = std::time::Instant::now();
        let loaded = pf_plugin_host::PluginInstance::load_cached(&wasm_path, &cache);
        let load_elapsed = t0.elapsed();
        let t1 = std::time::Instant::now();
        match loaded {
            Ok(mut inst) => match inst.load_ctx(&super::plugin_ctx_json(&id, &m, None)) {
                Ok(()) => eprintln!(
                    "[pf-plugin] {id} 验签通过（装载 {load_elapsed:?} / init {:?}，后台线程）",
                    t1.elapsed()
                ),
                Err(e) => {
                    eprintln!("[pf-plugin] {id} 拒载：{e}");
                    rejected.push(id);
                }
            },
            Err(e) => {
                eprintln!("[pf-plugin] {id} 装载失败：{e}");
                rejected.push(id);
            }
        }
    }
    rejected
}

/// 安装尾声钩子实现：wasm 包在**安装时**就把 cwasm 编译产物做好，落进
/// `<插件目录>/.cache/`。此后启动、开面板一律 mmap 命中（毫秒级）。
///
/// 非 wasm 包（无 `wasm =` 声明）= 无需预编译；清单解析失败也不在此判死
/// （安装链路已校验过，这里只负责把重活搬完）。
pub(super) fn install_prepare(plugin_dir: &Path) -> Result<(), String> {
    let Ok(m) = super::parse_manifest(plugin_dir) else {
        return Ok(());
    };
    let Some(wasm) = m.wasm.as_deref() else {
        return Ok(());
    };
    let wasm_path = plugin_dir.join(wasm);
    let cache = pi_link::pf_plugins::plugin_cache_for(plugin_dir);
    let t0 = std::time::Instant::now();
    match pf_plugin_host::PluginInstance::precompile_to_cache(&wasm_path, &cache) {
        Ok(_) => {
            eprintln!("[pf-plugin] 安装预编译 {} 完成（{:?}）", m.id, t0.elapsed());
            Ok(())
        }
        Err(e) => Err(format!("预编译失败: {e}")),
    }
}

/// 旧版安装预热（编译产物不在插件目录的历史包）：后台补编一次。
/// 新安装已由 [`install_prepare`] 在安装时做完，这里通常全部命中、立刻返回。
pub(crate) fn spawn_precompile_installed() {
    std::thread::spawn(|| {
        for m in super::scan_manifests()
            .into_iter()
            .filter(|m| m.wasm.is_some())
        {
            let wasm = m.dir.join(m.wasm.as_deref().unwrap_or("plugin.wasm"));
            let cache = pi_link::pf_plugins::plugin_cache_for(&m.dir);
            let t0 = std::time::Instant::now();
            match pf_plugin_host::PluginInstance::precompile_to_cache(&wasm, &cache) {
                Ok(true) => {
                    eprintln!("[pf-plugin] 预编译 {} 完成（{:?}）", m.id, t0.elapsed())
                }
                Ok(false) => {} // 已命中
                Err(e) => eprintln!("[pf-plugin] 预编译 {} 失败：{e}", m.id),
            }
        }
    });
}

/// 本机构建是否带官方宿主证明（build.rs 注入；占位符 = 非官方）。
pub(super) fn has_official_proof() -> bool {
    option_env!("PF_HOST_PROOF").is_some_and(|p| !p.is_empty() && p != "PROOF_PLACEHOLDER")
}

/// 官方构建证明材料（build.rs 把签名材料烙成二进制常量，见 crates/app/build.rs；
/// 未设置的构建 = 占位符 → 装载校验必败 → 拒载）。
pub(super) fn host_proof_material() -> String {
    let proof = option_env!("PF_HOST_PROOF").unwrap_or("PROOF_PLACEHOLDER");
    let kid = option_env!("PF_HOST_PROOF_KID").unwrap_or("");
    serde_json::json!({ "kid": kid, "proof": proof }).to_string()
}
