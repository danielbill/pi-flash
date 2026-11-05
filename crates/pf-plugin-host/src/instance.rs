use super::*;

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

    /// 90-9 §3/§10（契约 v5）：装载入口 = `load(plugin-ctx)`，无 init 回退
    /// （本版一刀切，两在役包同步改造）。ctx = JSON 串，每次调用给完整一份。
    /// 方法名 load_ctx：与组件装载器 [`Self::load`]（wasm_path）区分。
    pub fn load_ctx(&mut self, plugin_ctx: &str) -> anyhow::Result<()> {
        let _ = &self.engine;
        let _ = &self.component;
        // 90-4：ctx.dir = `<pkg>/data`（无版本维度）。instantiate 时只能从
        // wasm 路径猜（`<ver>/data` = 版本层——090-4 版本层布局下劈叉），
        // 这里改听 ctx JSON 的 `plugin.dataDir`：app 侧从 manifest.data_dir()
        // 构造，是包层的唯一真源。真机 2026-11-03：壳 storage 落版本层、
        // pi ext 落包层，镜像劈叉（icon 灰 / qr·unload 信号互相看不见）。
        if let Some(store) = self.store.as_mut()
            && let Some(dir) = ctx_data_dir(plugin_ctx)
        {
            store.data_mut().data_dir = dir;
        }
        if let (Some(store), Some(b)) = (self.store.as_mut(), self.bindings.as_mut()) {
            // WIT：load 返回 Err = 插件自拒（如宿主证明验签失败）→ 拒载并透传原因（失败不静默）。
            match b.call_load(store, plugin_ctx) {
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

    /// 90-9 §2 `unload()`（world 必需导出，本版起非能力面探测）：停用/卸载/
    /// 更新覆盖/退出前的最后一次调用。允许空实现（插件侧 no-op）。Err =
    /// wasm trap 等——best-effort，宿主只落 plugin.log，不等插件确认存盘
    /// （关不掉的插件不能卡住宿主）。
    pub fn unload(&mut self) -> Result<(), String> {
        let Some(store) = self.store.as_mut() else {
            return Ok(());
        };
        let Some(b) = self.bindings.as_mut() else {
            return Ok(());
        };
        b.call_unload(store).map_err(|e| e.to_string())
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
