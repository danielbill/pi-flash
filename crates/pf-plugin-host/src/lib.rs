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
    /// 能力面（契约 v3 钩子导出，8tb.2）：None = 本包未导出该钩子（旧 wasm
    /// 包）→ 调用方按「无」处理，与 JS 未导出同形。
    hook_fn_bar: Option<wasmtime::component::TypedFunc<(String,), (String,)>>,
    hook_session_row: Option<wasmtime::component::TypedFunc<(String,), (String,)>>,
    hook_render_popup: Option<wasmtime::component::TypedFunc<(), (String,)>>,
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

/// 按路径的编译单飞锁（8tb.4 真机 10-29）：冷缓存时「启动验签门」与「UI 懒
/// 装载」会并发各编一份 cwasm（秒级 cranelift 抢满 CPU → 揭幕后 UI 冻结数秒）。
/// 同路径编译持同一把锁，进锁后**先复查产物是否已出现**（双检），后者直接
/// mmap 前者的成果。
fn compile_lock(path: &Path) -> std::sync::Arc<std::sync::Mutex<()>> {
    static LOCKS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<PathBuf, std::sync::Arc<std::sync::Mutex<()>>>>,
    > = std::sync::OnceLock::new();
    let map = LOCKS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut g = map.lock().unwrap();
    g.entry(path.to_path_buf())
        .or_insert_with(|| std::sync::Arc::new(std::sync::Mutex::new(())))
        .clone()
}

/// 编译耗时实测（铁律：热路径要带数字）。进程内只报一次/路径。
fn log_compile_perf(path: &Path, elapsed: std::time::Duration) {
    static REPORTED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<PathBuf>>> =
        std::sync::OnceLock::new();
    let set = REPORTED.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()));
    if set.lock().unwrap().insert(path.to_path_buf()) {
        eprintln!("[perf] cwasm 编译 {}: {elapsed:?}", path.display());
    }
}

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
        // 同路径编译单飞：持锁后复查产物（并发方可能已编好落盘）
        let _compile_guard = compile_lock(wasm_path);
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
                let t = std::time::Instant::now();
                let bytes = engine.precompile_component(&wasm)?;
                log_compile_perf(wasm_path, t.elapsed());
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
        let _compile_guard = compile_lock(wasm_path);
        if artifact.is_file() {
            return Ok(false); // 并发方已编好（双检）
        }
        let t = std::time::Instant::now();
        let bytes = shared_engine()?.precompile_component(&wasm)?;
        log_compile_perf(wasm_path, t.elapsed());
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

        let (bindings, hook_fn_bar, hook_session_row, hook_render_popup) = {
            let instance = linker.instantiate(&mut store, &component)?;
            // 能力面探测（契约 v3 钩子导出）：world 不声明它们（旧 wasm 包
            // 兼容——声明即强要求，缺导出的包在 Plugin::new 拒载），导出存在
            // 才接。旧包探不到 = 功能关；新包多出的导出对 v2 bindgen 无害。
            let hook_fn_bar = instance
                .get_typed_func::<(String,), (String,)>(&mut store, "fn-bar")
                .ok();
            let hook_session_row = instance
                .get_typed_func::<(String,), (String,)>(&mut store, "session-row")
                .ok();
            let hook_render_popup = instance
                .get_typed_func::<(), (String,)>(&mut store, "render-popup")
                .ok();
            let bindings = Plugin::new(&mut store, &instance)?;
            (bindings, hook_fn_bar, hook_session_row, hook_render_popup)
        };
        Ok(Self {
            engine,
            store: Some(store),
            bindings: Some(bindings),
            hook_fn_bar,
            hook_session_row,
            hook_render_popup,
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

    /// 契约 v3 fn-bar（能力面）：chrome JSON {icon,tone,tip}；未导出 →
    /// `Ok(None)`（与 JS 未导出同形）；空串/"null" → None（无 chrome）。
    pub fn fn_bar_json(&mut self, env: &str) -> Result<Option<String>, String> {
        let Some(f) = self.hook_fn_bar else {
            return Ok(None);
        };
        let Some(store) = self.store.as_mut() else {
            return Ok(None);
        };
        let s = f
            .call(&mut *store, (env.to_string(),))
            .and_then(|(r,)| f.post_return(&mut *store).map(|()| r))
            .map_err(|e| e.to_string())?;
        Ok(hook_optional(s))
    }

    /// 契约 v3 session-row（能力面）：徽标 JSON {icon,tone}；未导出 →
    /// `Ok(None)`；空串/"null" → None（本行无徽标）。
    pub fn session_row_json(&mut self, env: &str) -> Result<Option<String>, String> {
        let Some(f) = self.hook_session_row else {
            return Ok(None);
        };
        let Some(store) = self.store.as_mut() else {
            return Ok(None);
        };
        let s = f
            .call(&mut *store, (env.to_string(),))
            .and_then(|(r,)| f.post_return(&mut *store).map(|()| r))
            .map_err(|e| e.to_string())?;
        Ok(hook_optional(s))
    }

    /// 契约 v3 render-popup（能力面）：弹窗内联树 JSON；空串 → `"null"`
    /// （= JS render_popup 返回 null = 弹窗该收了，app 侧解析与 JS 同路）。
    /// 未导出 → 退回 v2 render 面（wit v3 前弹窗体 = render() 的既有形态）。
    pub fn render_popup_json(&mut self) -> Result<String, String> {
        if let Some(f) = self.hook_render_popup {
            let Some(store) = self.store.as_mut() else {
                return Ok("null".to_string());
            };
            let s = f
                .call(&mut *store, ())
                .and_then(|(r,)| f.post_return(&mut *store).map(|()| r))
                .map_err(|e| e.to_string())?;
            return Ok(if s.is_empty() { "null".to_string() } else { s });
        }
        self.render_json().map_err(|e| e.to_string())
    }

    pub fn pump_actions(&self) -> Vec<PluginAction> {
        let mut out = Vec::new();
        while let Ok(a) = self.rx.try_recv() {
            out.push(a);
        }
        out
    }

    // ---- 能力面查询（8tb.2；测试与日志用）：包是否导出契约 v3 钩子 ----

    pub fn has_fn_bar(&self) -> bool {
        self.hook_fn_bar.is_some()
    }

    pub fn has_session_row(&self) -> bool {
        self.hook_session_row.is_some()
    }

    pub fn has_render_popup(&self) -> bool {
        self.hook_render_popup.is_some()
    }
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

#[cfg(test)]
mod hook_tests {
    use super::*;

    #[test]
    fn hook_optional_nulls() {
        assert_eq!(hook_optional(String::new()), None);
        assert_eq!(hook_optional("null".into()), None);
        assert_eq!(hook_optional("NULL".into()), None);
        assert_eq!(hook_optional(" {}".into()), Some(" {}".to_string()));
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
        // 激活后渲染应有产出（多点校验过的路径；090-5 起 Paint 直绘面退役，回归组件树契约）
        assert!(
            !inst.render_json().expect("render").is_empty(),
            "激活后应有渲染输出"
        );

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

    /// 契约 v3 能力面（8tb.2）：v3 包探测命中三钩子且类型吻合（类型不符 =
    /// 探测 None，所以 has=true + 调用 Ok 即签名验证过）；v2 旧包探不到 =
    /// 功能关、装载不受影响（向后兼容的核心承诺）。缺环境跳过；由 W4
    /// 部署链注入 PF_TEST_V3_WASM / PF_TEST_V2_WASM 后运行。
    #[test]
    fn wasm_v3_capability_face() {
        let Ok(v3) = std::env::var("PF_TEST_V3_WASM") else {
            return;
        };
        install_proof_hook(proof_placeholder); // 不 init，纯装载 + 探测
        // 拷进独立临时目录：测试各自的 data/ 互不串扰（插件 storage = 包目录/data）
        let dir = std::env::temp_dir().join(format!("pf-v3-cap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wasm = dir.join("plugin.wasm");
        std::fs::copy(&v3, &wasm).unwrap();
        let mut p = PluginInstance::load(&wasm).expect("load v3");
        assert!(p.has_fn_bar(), "v3 包应探到 fn-bar");
        assert!(p.has_session_row(), "v3 包应探到 session-row");
        assert!(p.has_render_popup(), "v3 包应探到 render-popup");
        // 签名吻合：调用不 trap、lift/lower 走通（桩返回 "null" → None）
        assert_eq!(p.fn_bar_json("{}").unwrap(), None);
        assert_eq!(p.session_row_json("{}").unwrap(), None);
        let _ = p.render_popup_json().expect("render-popup 调用应成功");

        // 官方 proof（可选注入）→ init 验签激活后，钩子应回真值：插件 id 由
        // PF_TEST_PLUGIN_ID 注入（PF 不内置任何插件身份）；只验**宿主契约**
        // 形状（chrome 三键、未绑无徽标、弹窗是组件树），不验插件自己的文案/图标
        if let (Ok(_), Ok(plugin_id)) = (
            std::env::var("PF_TEST_PROOF_OFFICIAL"),
            std::env::var("PF_TEST_PLUGIN_ID"),
        ) {
            install_proof_hook(proof_official);
            let mut p = PluginInstance::load(&wasm).expect("load v3");
            p.init(&plugin_id).expect("官方 proof 必须激活");
            let chrome = p.fn_bar_json("{}").unwrap().expect("chrome 应有值");
            let v: serde_json::Value = serde_json::from_str(&chrome).unwrap();
            assert_eq!(v["tone"], "text.faint", "未绑 = 灰");
            assert!(
                v["tip"]["zh"].as_str().is_some_and(|s| !s.is_empty()),
                "未绑态应有引导 tip"
            );
            assert!(
                v["icon"].as_str().is_some_and(|s| !s.is_empty()),
                "chrome 应带图标"
            );
            assert_eq!(p.session_row_json("{}").unwrap(), None, "未绑 = 无徽标");
            let popup = p.render_popup_json().unwrap();
            let tree: serde_json::Value = serde_json::from_str(&popup).unwrap();
            assert_eq!(tree["type"], "column", "弹窗体应是组件树");
        }

        // v2 旧包：探不到新钩子，装载与 v2 render 面不受影响
        if let Ok(v2) = std::env::var("PF_TEST_V2_WASM") {
            let mut p = PluginInstance::load(std::path::Path::new(&v2)).expect("load v2");
            assert!(!p.has_fn_bar() && !p.has_session_row() && !p.has_render_popup());
            p.render_json().expect("v2 render 面应可用");
        }
    }
}

#[cfg(test)]
mod compile_single_flight_tests {
    use super::*;

    /// 冷缓存 + 双线程并发 load_cached：编译必须只发生一次（单飞锁 + 双检）。
    /// 由 inner e2e 注入 PF_TEST_V3_WASM 运行；--nocapture 下应只见一条
    /// 「[perf] cwasm 编译」。
    #[test]
    fn concurrent_load_compiles_once() {
        let Ok(v3) = std::env::var("PF_TEST_V3_WASM") else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("pf-host-single-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wasm = dir.join("plugin.wasm");
        std::fs::copy(&v3, &wasm).unwrap();
        let cache = dir.join(".cache");
        // 不 init：load_cached 只验装载，不触证明门

        let wasm1 = wasm.clone();
        let cache1 = cache.clone();
        let t1 = std::thread::spawn(move || {
            let mut i = PluginInstance::load_cached(&wasm1, &cache1).expect("t1 load");
            i.init("t1").ok();
        });
        let wasm2 = wasm.clone();
        let cache2 = cache.clone();
        let t2 = std::thread::spawn(move || {
            let mut i = PluginInstance::load_cached(&wasm2, &cache2).expect("t2 load");
            i.init("t2").ok();
        });
        t1.join().unwrap();
        t2.join().unwrap();
        // 两份实例都装载成功（文件级断言：产物存在且唯一）
        let n = std::fs::read_dir(&cache)
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "cwasm"))
            .count();
        assert_eq!(n, 1, "并发装载后产物应只有一份");
    }
}
