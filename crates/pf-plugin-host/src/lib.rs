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
}

#[derive(Debug, Clone)]
pub enum PluginAction {
    Toast(String),
    OpenFile(String),
    RunScript(String),
}

// ---------------------------------------------------------------------------
// 宿主数据/服务回调钩子（app 侧注入）
// ---------------------------------------------------------------------------

pub type QuickPinListFn = fn() -> Vec<String>;
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

/// nonceTag 派生（js.rs / 内层插件 pf-inner-verify 同算法，公开）：
/// `hex(sha256(proof ‖ 0x00 ‖ nonce))`。防静态硬编码 dump，线性抬价非绝对防护。
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

fn measure_text_width(text: &str, size: f32) -> f32 {
    MEASURE_FN.with(|c| c.borrow().map(|f| f(text, size)).unwrap_or(0.0))
}

// ---------------------------------------------------------------------------
// 一个插件实例
// ---------------------------------------------------------------------------

pub struct PluginInstance {
    engine: Engine,
    component: Component,
    store: Option<Store<HostState>>,
    bindings: Option<Plugin>,
    rx: mpsc::Receiver<PluginAction>,
}

/// 进程内共享的 wasmtime 引擎：`Engine::new` 要建 cranelift ISA/寄存器分配器，
/// 每插件一份纯属浪费（且编译本身已是秒级，别再叠一层）。缺省配置即可——
/// wasmtime 36 默认开 component model（原代码显式开的设置保持等价）。
fn shared_engine() -> anyhow::Result<&'static Engine> {
    if let Some(e) = SHARED_ENGINE.get() {
        return Ok(e);
    }
    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config)?;
    Ok(SHARED_ENGINE.get_or_init(|| engine))
}

static SHARED_ENGINE: std::sync::OnceLock<Engine> = std::sync::OnceLock::new();

/// wasm 内容指纹（缓存键）：sha256 十六进制。
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    let mut s = String::with_capacity(64);
    for b in Sha256::digest(bytes) {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// 编译产物落盘（原子写：tmp + rename，崩溃不留半截产物）。
/// 并发写同一份 = rename 失败即放弃（对方那份同样有效）；写完顺手淘汰旧产物。
fn write_artifact(cache_dir: &Path, bytes: &[u8], artifact: &Path) {
    let _ = std::fs::create_dir_all(cache_dir);
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp = cache_dir.join(format!(
        ".tmp-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    if std::fs::write(&tmp, bytes)
        .and_then(|_| std::fs::rename(&tmp, artifact))
        .is_err()
    {
        let _ = std::fs::remove_file(&tmp);
    }
    prune_artifacts(cache_dir);
}

/// 只留最近 8 份产物（按 mtime）：插件换版后旧产物自然淘汰，缓存目录不无限涨。
fn prune_artifacts(dir: &Path) {
    const KEEP: usize = 8;
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut items: Vec<(std::time::SystemTime, PathBuf)> = rd
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "cwasm"))
        .filter_map(|e| {
            let t = e.metadata().ok()?.modified().ok()?;
            Some((t, e.path()))
        })
        .collect();
    if items.len() <= KEEP {
        return;
    }
    items.sort_by_key(|(t, _)| std::cmp::Reverse(*t)); // 新 → 旧
    for (_, p) in items.into_iter().skip(KEEP) {
        let _ = std::fs::remove_file(p);
    }
}

impl PluginInstance {
    /// 加载插件 wasm 组件。**重活**：`Component::from_file` 是 cranelift 全量
    /// 编译（4.6 MB 组件秒级），**只许在后台线程调用**——落在 UI 线程就是
    /// 启动卡死数秒（app 侧曾踩）。热路径请用 [`Self::load_cached`]。
    pub fn load(wasm_path: &Path) -> anyhow::Result<Self> {
        let engine = shared_engine()?.clone();
        let component = Component::from_file(&engine, wasm_path)?;
        Self::instantiate(engine, component, wasm_path)
    }

    /// 带编译缓存的加载（启动/热路径）。命中 = **mmap 反序列化**（毫秒级，
    /// 不读不编译）；未命中 = 编译一次、产物落进 `cache_dir`，此后一直命中。
    ///
    /// 缓存键 = wasm 内容 sha256；产物自身还带着 wasmtime 的
    /// 版本/编译配置/目标校验（不符即 `Err`）——失配就重编，不会用错产物。
    /// 语义不变：每次仍 `instantiate + init`（验签门照跑）。
    pub fn load_cached(wasm_path: &Path, cache_dir: &Path) -> anyhow::Result<Self> {
        let engine = shared_engine()?.clone();
        let wasm = std::fs::read(wasm_path)?;
        let artifact = cache_dir.join(format!("{}.cwasm", sha256_hex(&wasm)));
        // SAFETY：产物只由本方 `precompile_component` 写出，路径在 PF 自有目录
        let component = match unsafe { Component::deserialize_file(&engine, &artifact) } {
            Ok(c) => c,
            Err(e) => {
                if artifact.is_file() {
                    eprintln!(
                        "[pf-plugin-host] 编译缓存失效，重编 {}：{e}",
                        artifact.display()
                    );
                }
                let bytes = engine.precompile_component(&wasm)?;
                write_artifact(cache_dir, &bytes, &artifact);
                // SAFETY：同一份自产产物字节（刚编译，形态即 precompile 输出）
                unsafe { Component::deserialize(&engine, &bytes) }?
            }
        };
        Self::instantiate(engine, component, wasm_path)
    }

    /// 缓存命中判定（懒装载热路径用）：wasm 内容 sha256 对应的产物是否已在
    /// 盘。命中 = 主线程 `load_cached` 毫秒级同步装；未命中 = 编译秒级重活，
    /// 调用方必须转后台。
    pub fn has_cached_artifact(wasm_path: &Path, cache_dir: &Path) -> bool {
        let Ok(wasm) = std::fs::read(wasm_path) else {
            return false;
        };
        cache_dir
            .join(format!("{}.cwasm", sha256_hex(&wasm)))
            .is_file()
    }

    /// 预编译进缓存（安装后预热用；任意线程可调）。返回是否真的编了。
    /// 目的在于让**首次启动**也走缓存命中，不现场编一秒级。
    pub fn precompile_to_cache(wasm_path: &Path, cache_dir: &Path) -> anyhow::Result<bool> {
        let wasm = std::fs::read(wasm_path)?;
        let artifact = cache_dir.join(format!("{}.cwasm", sha256_hex(&wasm)));
        if artifact.is_file() {
            return Ok(false);
        }
        let bytes = shared_engine()?.precompile_component(&wasm)?;
        write_artifact(cache_dir, &bytes, &artifact);
        Ok(true)
    }

    /// 引擎 + 组件 → 实例（链接宿主 import、建 store、实例化）。
    fn instantiate(engine: Engine, component: Component, wasm_path: &Path) -> anyhow::Result<Self> {
        let mut linker: Linker<HostState> = Linker::new(&engine);
        pf::plugin::host::add_to_linker::<HostState, wasmtime::component::HasSelf<HostState>>(
            &mut linker,
            |state| state,
        )?;
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;

        let (tx, rx) = mpsc::channel();
        let data_dir = wasm_path
            .parent()
            .map(|d| d.join("data"))
            .unwrap_or_else(|| std::path::PathBuf::from("data"));
        let mut store = Store::new(
            &engine,
            HostState {
                table: ResourceTable::new(),
                ctx: wasmtime_wasi::WasiCtxBuilder::new().build(),
                tx,
                data_dir,
            },
        );

        let bindings = {
            let instance = linker.instantiate(&mut store, &component)?;
            Plugin::new(&mut store, &instance)?
        };
        Ok(Self {
            engine,
            store: Some(store),
            bindings: Some(bindings),
            rx,
            component,
        })
    }

    pub fn init(&mut self, name: &str) -> anyhow::Result<()> {
        let _ = &self.engine;
        let _ = &self.component;
        if let (Some(store), Some(b)) = (self.store.as_mut(), self.bindings.as_mut()) {
            // WIT：init 返回 Err = 插件自拒（如宿主证明验签失败）→ 拒载并透传原因（失败不静默）。
            match b.call_init(store, name) {
                Ok(Ok(())) => Ok(()),
                Ok(Err(reason)) => Err(anyhow::anyhow!("插件拒载: {reason}")),
                Err(e) => Err(e.into()),
            }
        } else {
            Ok(())
        }
    }

    /// 宿主→插件事件（WIT `on-event`）：`kind` = panel-open / menu / command / action。
    /// 契约里所有 `contributes.*` 回调都汇到这一条，以后加贡献项不改 WIT。
    pub fn on_event(&mut self, kind: &str, id: &str, path: &str) -> anyhow::Result<()> {
        if let (Some(store), Some(b)) = (self.store.as_mut(), self.bindings.as_mut()) {
            let ev = pf::plugin::types::PluginEvent {
                kind: kind.to_string(),
                id: id.to_string(),
                path: path.to_string(),
            };
            b.call_on_event(store, &ev)?;
        }
        Ok(())
    }

    /// 组件树 JSON（WIT `render` → string，与 JS `render(ctx)` 返回值同一 schema）。
    /// 宿主侧 `panel::resolve` 统一解释——wasm 不另立渲染模型。
    pub fn render_json(&mut self) -> anyhow::Result<String> {
        match (self.store.as_mut(), self.bindings.as_mut()) {
            (Some(store), Some(b)) => Ok(b.call_render(store)?),
            _ => Ok(String::new()),
        }
    }

    pub fn pump_actions(&self) -> Vec<PluginAction> {
        let mut out = Vec::new();
        while let Ok(a) = self.rx.try_recv() {
            out.push(a);
        }
        out
    }
}

/// 插件面板已装目录描述（app 用）。
pub struct PluginMeta {
    pub slug: String,
    pub title: String,
    pub wasm_path: std::path::PathBuf,
}

pub fn scan_plugins_dir(dir: &Path) -> Vec<PluginMeta> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in rd.flatten() {
        let d = e.path();
        if !d.is_dir() {
            continue;
        }
        let manifest = d.join("pf-plugin.toml");
        if !manifest.is_file() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            continue;
        };
        // 极简解析：name/title/wasm 三行（v1 不引 toml 库）
        let mut name = None;
        let mut title = None;
        let mut wasm = None;
        for line in text.lines() {
            let line = line.trim();
            if let Some(v) = line.strip_prefix("name = ") {
                name = Some(unquote(v));
            } else if let Some(v) = line.strip_prefix("title = ") {
                title = Some(unquote(v));
            } else if let Some(v) = line.strip_prefix("wasm = ") {
                wasm = Some(unquote(v));
            }
        }
        let wasm = wasm.unwrap_or_else(|| "plugin.wasm".to_string());
        let wasm_path = d.join(&wasm);
        if !wasm_path.is_file() {
            continue;
        }
        out.push(PluginMeta {
            slug: name.unwrap_or_else(|| "unknown".into()),
            title: title.unwrap_or_else(|| "插件".into()),
            wasm_path,
        });
    }
    out
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    v.trim_matches('"').trim_matches('\'').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proof_official() -> String {
        std::env::var("PF_TEST_PROOF_OFFICIAL").unwrap_or_default()
    }
    fn proof_placeholder() -> String {
        r#"{"kid":"","proof":"PROOF_PLACEHOLDER"}"#.into()
    }
    fn proof_forged() -> String {
        std::env::var("PF_TEST_PROOF_FORGED").unwrap_or_default()
    }

    /// 内层插件证明门（cq9）：官方 proof → init Ok；占位符/伪造 → Err 带可读原因。
    /// 由 inner/scripts/e2e.mjs 注入 PF_TEST_INNER_WASM 等环境后运行；缺环境直接跳过。
    #[test]
    fn inner_plugin_proof_gate() {
        let Ok(wasm) = std::env::var("PF_TEST_INNER_WASM") else {
            return;
        };
        let Ok(_official) = std::env::var("PF_TEST_PROOF_OFFICIAL") else {
            return;
        };
        let forged = std::env::var("PF_TEST_PROOF_FORGED").unwrap_or_default();
        install_proof_hook(proof_official);
        let path = std::path::PathBuf::from(wasm);
        let name = "demo-inner";

        // ① 官方 proof → 正常激活

        let mut inst = PluginInstance::load(&path).expect("load");
        inst.init(name).expect("官方 proof 必须激活成功");
        // 激活后 render 应有产出（多点校验过的路径）
        assert!(!inst.render().is_empty(), "激活后应有渲染输出");

        // ② 占位符（fork 自建/无 key 构建）→ 拒载 + 原因
        install_proof_hook(proof_placeholder);
        let mut inst = PluginInstance::load(&path).expect("load");
        let err = inst.init(name).expect_err("占位符必须拒载");
        assert!(err.to_string().contains("官方构建"), "原因要可读: {err}");

        // ③ 伪造（换钥签名）→ 拒载 + 原因
        if !forged.is_empty() {
            install_proof_hook(proof_forged);
            let mut inst = PluginInstance::load(&path).expect("load");
            let err = inst.init(name).expect_err("伪造必须拒载");
            assert!(err.to_string().contains("验签失败"), "原因要可读: {err}");
        }
    }
}
