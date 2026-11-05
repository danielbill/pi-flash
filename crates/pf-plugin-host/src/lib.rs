//! PF 插件运行时：wasmtime 加载 quick-panel 这类自绘插件组件。
//!
//! 协议（pf-plugin.wit）：插件产 display list（rect/text 原语），文本由宿主
//! 塑形（measure-text import 需要宿主 text system，走线程局部 App 指针）。
//! 动作类 import（open-file/run-script/toast...）走通道，由调用方消费，
//! 避免在 gpui 借用栈里嵌套重入。

pub mod js;

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use wasmtime::component::{Component, Linker, ResourceTable, bindgen};
use wasmtime::{Config, Engine, Store};

bindgen!({
    path: "wit/pf-plugin.wit",
    world: "plugin",
});

/// import 侧实现：动作不入队，等待调用方在泵里处理。
pub struct HostState {
    pub table: ResourceTable,
    pub ctx: wasmtime_wasi::WasiCtx,
    pub tx: mpsc::Sender<PluginAction>,
    /// 插件私有数据目录（02 §3 自管存储：`<插件目录>/data/`，宿主只建不解析）
    pub data_dir: std::path::PathBuf,
}

impl wasmtime_wasi::WasiView for HostState {
    fn ctx(&mut self) -> wasmtime_wasi::WasiCtxView<'_> {
        wasmtime_wasi::WasiCtxView {
            ctx: &mut self.ctx,
            table: &mut self.table,
        }
    }
}

impl pf::plugin::host::Host for HostState {
    fn measure_text(&mut self, content: String, size: f32) -> f32 {
        measure_text_width(&content, size)
    }
    fn toast(&mut self, msg: String) {
        let _ = self.tx.send(PluginAction::Toast(msg));
    }
    fn open_file(&mut self, path: String) {
        let _ = self.tx.send(PluginAction::OpenFile(path));
    }
    fn run_script(&mut self, path: String) {
        let _ = self.tx.send(PluginAction::RunScript(path));
    }
    fn quick_pins(&mut self) -> Vec<String> {
        quick_pin_list()
    }
    fn file_icon(&mut self, path: String) -> String {
        file_icon_of(&path)
    }
    fn is_executive(&mut self, path: String) -> bool {
        is_exec_of(&path)
    }
    fn storage_read(&mut self, key: String) -> String {
        match storage_path(&self.data_dir, &key) {
            Some(p) => std::fs::read_to_string(p).unwrap_or_default(),
            None => String::new(),
        }
    }
    fn storage_write(&mut self, key: String, data: String) {
        if let Some(p) = storage_path(&self.data_dir, &key) {
            let _ = std::fs::create_dir_all(&self.data_dir);
            let _ = std::fs::write(p, data);
        }
    }
    fn get_host_proof(&mut self, nonce: String) -> pf::plugin::host::HostProof {
        // 090-5 L3：宿主只递数据不判定；nonceTag 现算（算法公开，插件 wasm 内复算）。
        let v: serde_json::Value =
            serde_json::from_str(&proof_material()).unwrap_or(serde_json::Value::Null);
        let kid = v["kid"].as_str().unwrap_or("").to_string();
        let proof = v["proof"]
            .as_str()
            .unwrap_or("PROOF_PLACEHOLDER")
            .to_string();
        let nonce_tag = nonce_tag(&proof, &nonce);
        pf::plugin::host::HostProof {
            kid,
            proof,
            nonce_tag,
        }
    }
    fn new_draft(&mut self, prefix: String, cwd: String) {
        // 意图调用（契约 v2.4 ctx.newDraft 同面）：只入队，建线在调用方消费
        // PluginAction::NewDraft 时进行；结果经 broadcast "claim" 回推。
        let _ = self.tx.send(PluginAction::NewDraft { prefix, cwd });
    }
    fn svc_post(&mut self, anchor_or_key: String, payload: String) -> bool {
        svc_post_bridge(&anchor_or_key, &payload)
    }
    fn svc_query(&mut self, anchor_or_key: String) -> String {
        svc_query_bridge(&anchor_or_key)
    }
    fn open_popup(&mut self, id: String) {
        // 契约 v3 act.openPopup（wasm 面，8tb.2）：只入队，开窗在调用方消费
        // PluginAction::OpenPopup 时进行（与 JS JsAction::OpenPopup 同落点）。
        let _ = self.tx.send(PluginAction::OpenPopup(id));
    }
}

#[derive(Debug, Clone)]
pub enum PluginAction {
    Toast(String),
    OpenFile(String),
    RunScript(String),
    /// 意图调用：新建插件认领会话（契约 v2.4 sessions；JS 侧 JsAction::NewDraft
    /// 同语义）。宿主建线后经 broadcast "claim" 事件回推 key。
    NewDraft {
        prefix: String,
        cwd: String,
    },
    /// 契约 v3 act.openPopup：开插件声明的 popups 条目（与 JS
    /// JsAction::OpenPopup 同落点；非模态，窗体归宿主）。
    OpenPopup(String),
}

// ---------------------------------------------------------------------------
// 宿主数据/服务回调钩子（app 侧注入）
// ---------------------------------------------------------------------------

pub type QuickPinListFn = fn() -> Vec<String>;

/// 执行域桥（90-6）：wasm 插件的 ctx.service 面。app 侧注入真实实现。
pub type SvcPostFn = fn(&str, &str) -> bool;
pub type SvcQueryFn = fn(&str) -> String;

thread_local! {
    static SVC_POST_FN: std::cell::RefCell<Option<SvcPostFn>> = const { std::cell::RefCell::new(None) };
    static SVC_QUERY_FN: std::cell::RefCell<Option<SvcQueryFn>> = const { std::cell::RefCell::new(None) };
}

/// 注入执行域能力（启动一次；未注入 = 拒收，fail-closed）。
pub fn install_svc_hooks(post: SvcPostFn, query: SvcQueryFn) {
    SVC_POST_FN.with(|c| *c.borrow_mut() = Some(post));
    SVC_QUERY_FN.with(|c| *c.borrow_mut() = Some(query));
}

fn svc_post_bridge(key: &str, payload: &str) -> bool {
    SVC_POST_FN.with(|c| c.borrow().map(|f| f(key, payload)).unwrap_or(false))
}

fn svc_query_bridge(key: &str) -> String {
    SVC_QUERY_FN.with(|c| {
        c.borrow()
            .map(|f| f(key))
            .unwrap_or_else(|| "absent".into())
    })
}
/// 路径派生能力（与 JS 插件 ctx.fileIcon / ctx.isExecutive 同源）
pub type FileIconFn = fn(&str) -> String;
pub type IsExecFn = fn(&str) -> bool;
pub type MeasureTextFn = fn(&str, f32) -> f32;
/// 090-5 L3：官方宿主证明材料钩子，返回紧凑 JSON `{"kid":"…","proof":"…"}`。
pub type HostProofFn = fn() -> String;

thread_local! {
    static QUICK_PINS_FN: std::cell::RefCell<Option<QuickPinListFn>> = const { std::cell::RefCell::new(None) };
    static MEASURE_FN: std::cell::RefCell<Option<MeasureTextFn>> = const { std::cell::RefCell::new(None) };
    static FILE_ICON_FN: std::cell::RefCell<Option<FileIconFn>> = const { std::cell::RefCell::new(None) };
    static IS_EXEC_FN: std::cell::RefCell<Option<IsExecFn>> = const { std::cell::RefCell::new(None) };
}

/// 090-5 L3：官方宿主证明材料钩子——**进程级，不是 thread_local**。
///
/// 组件编译是秒级重活（4.6 MB 组件实测 ~2s），插件装载必须在后台线程跑，
/// UI 线程零阻塞；而 init 验签时插件会回调 `get_host_proof`，钩子若按线程
/// 存，后台线程取不到材料 → 正常插件被误判拒载。故材料钩子存全局；
/// measure/quick-pin 仍按线程（贴 gpui 文本系统与 UI 状态，只许主线程调）。
static PROOF_FN: std::sync::RwLock<Option<HostProofFn>> = std::sync::RwLock::new(None);

/// app 启动时注入宿主能力实现。
pub fn install_host_hooks(quick: QuickPinListFn, measure: MeasureTextFn) {
    QUICK_PINS_FN.with(|c| *c.borrow_mut() = Some(quick));
    MEASURE_FN.with(|c| *c.borrow_mut() = Some(measure));
}

/// 注入官方构建证明材料（app 侧从 build.rs 注入的常量组装；见 crates/app/build.rs）。
/// **未注入 = 占位符（fail-closed）**：内层插件 init 验签必失败 → 拒载。
/// 钩子进程级：可从任意线程注入（后台验签线程同样生效）。
pub fn install_proof_hook(f: HostProofFn) {
    if let Ok(mut c) = PROOF_FN.write() {
        *c = Some(f);
    }
}

fn proof_material() -> String {
    PROOF_FN
        .read()
        .ok()
        .and_then(|c| *c)
        .map(|f| f())
        .unwrap_or_else(|| r#"{"kid":"","proof":"PROOF_PLACEHOLDER"}"#.into())
}

/// nonceTag 派生（js / 内层插件 pf-inner-verify 同算法，公开）：
/// `hex(sha256(proof ‖ 0x00 ‖ nonce))`。
pub fn nonce_tag(proof: &str, nonce: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(proof.as_bytes());
    h.update([0u8]);
    h.update(nonce.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// 路径派生能力注入（app 启动时；与 JS 插件同一套服务实现）。
pub fn install_path_hooks(file_icon: FileIconFn, is_exec: IsExecFn) {
    FILE_ICON_FN.with(|c| *c.borrow_mut() = Some(file_icon));
    IS_EXEC_FN.with(|c| *c.borrow_mut() = Some(is_exec));
}

fn file_icon_of(path: &str) -> String {
    FILE_ICON_FN.with(|c| c.borrow().map(|f| f(path)).unwrap_or_default())
}

fn is_exec_of(path: &str) -> bool {
    IS_EXEC_FN.with(|c| c.borrow().map(|f| f(path)).unwrap_or(false))
}

fn quick_pin_list() -> Vec<String> {
    QUICK_PINS_FN.with(|c| c.borrow().map(|f| f()).unwrap_or_default())
}

/// 存储 key 安全化（02 §3 越界拒绝，与 app 侧 ChatHost::safe_path 同规则）：
/// 只许本目录下的普通文件名。
fn storage_path(data_dir: &Path, key: &str) -> Option<PathBuf> {
    let bad = key.is_empty()
        || key.contains('/')
        || key.contains('\\')
        || key.contains("..")
        || key.starts_with('.');
    if bad {
        eprintln!("[pf-plugin-host] storage key 越界拒绝: {key}");
        return None;
    }
    Some(data_dir.join(key))
}

/// plugin-ctx JSON 里的 `plugin.dataDir`（90-4 包层真源；app 侧
/// `plugin_ctx_json` 从 `manifest.data_dir()` 构造）。缺省 / 空串 = None
///（保持 instantiate 时的 wasm 旁猜测值，旧 ctx 兼容）。
fn ctx_data_dir(plugin_ctx: &str) -> Option<PathBuf> {
    let v: serde_json::Value = serde_json::from_str(plugin_ctx).ok()?;
    let dir = v.get("plugin")?.get("dataDir")?.as_str()?;
    (!dir.is_empty()).then(|| PathBuf::from(dir))
}

fn measure_text_width(text: &str, size: f32) -> f32 {
    MEASURE_FN.with(|c| c.borrow().map(|f| f(text, size)).unwrap_or(0.0))
}

/// 钩子返回的「无」归一：空串 / "null" → None（与 JS glue 的 "null" 同语义，
/// app 侧不需要知道载体）。
fn hook_optional(s: String) -> Option<String> {
    let t = s.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("null") {
        None
    } else {
        Some(s)
    }
}

/// 契约 v5 最小 plugin-ctx（宿主自测用；session null / cwd 空）。
#[cfg(test)]
fn test_ctx(id: &str) -> String {
    serde_json::json!({
        "plugin": {"id": id, "version": "", "dataDir": ""},
        "pf": {"version": "host-test", "locale": "zh-CN"},
        "session": null,
        "cwd": "",
    })
    .to_string()
}

mod instance;
mod meta;

pub use instance::PluginInstance;
pub use meta::{PluginMeta, scan_plugins_dir};

#[cfg(test)]
mod compile_single_flight_tests;
#[cfg(test)]
mod ctx_data_dir_tests;
#[cfg(test)]
mod hook_tests;
#[cfg(test)]
mod tests;
