//! PF 插件运行时：wasmtime 加载 quick-panel 这类自绘插件组件。
//!
//! 协议（pf-plugin.wit）：插件产 display list（rect/text 原语），文本由宿主
//! 塑形（measure-text import 需要宿主 text system，走线程局部 App 指针）。
//! 动作类 import（open-file/run-script/toast...）走通道，由调用方消费，
//! 避免在 gpui 借用栈里嵌套重入。

pub mod js;

use std::path::Path;
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
    fn toggle_quick_pin(&mut self, path: String) {
        let _ = self.tx.send(PluginAction::ToggleQuickPin(path));
    }
    fn quick_pins(&mut self) -> Vec<String> {
        quick_pin_list()
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
    ToggleQuickPin(String),
}

// ---------------------------------------------------------------------------
// 宿主数据/服务回调钩子（app 侧注入）
// ---------------------------------------------------------------------------

pub type QuickPinListFn = fn() -> Vec<String>;
pub type MeasureTextFn = fn(&str, f32) -> f32;
/// 090-5 L3：官方宿主证明材料钩子，返回紧凑 JSON `{"kid":"…","proof":"…"}`。
pub type HostProofFn = fn() -> String;

thread_local! {
    static QUICK_PINS_FN: std::cell::RefCell<Option<QuickPinListFn>> = const { std::cell::RefCell::new(None) };
    static MEASURE_FN: std::cell::RefCell<Option<MeasureTextFn>> = const { std::cell::RefCell::new(None) };
    static PROOF_FN: std::cell::RefCell<Option<HostProofFn>> = const { std::cell::RefCell::new(None) };
}

/// app 启动时注入宿主能力实现。
pub fn install_host_hooks(quick: QuickPinListFn, measure: MeasureTextFn) {
    QUICK_PINS_FN.with(|c| *c.borrow_mut() = Some(quick));
    MEASURE_FN.with(|c| *c.borrow_mut() = Some(measure));
}

/// 注入官方构建证明材料（app 侧从 build.rs 注入的常量组装；见 crates/app/build.rs）。
/// **未注入 = 占位符（fail-closed）**：内层插件 init 验签必失败 → 拒载。
pub fn install_proof_hook(f: HostProofFn) {
    PROOF_FN.with(|c| *c.borrow_mut() = Some(f));
}

fn proof_material() -> String {
    PROOF_FN.with(|c| {
        c.borrow()
            .map(|f| f())
            .unwrap_or_else(|| r#"{"kid":"","proof":"PROOF_PLACEHOLDER"}"#.into())
    })
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

fn quick_pin_list() -> Vec<String> {
    QUICK_PINS_FN.with(|c| c.borrow().map(|f| f()).unwrap_or_default())
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

impl PluginInstance {
    /// 加载插件 wasm 组件（首次调用会做编译，耗时一次性）。
    pub fn load(wasm_path: &Path) -> anyhow::Result<Self> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        let engine = Engine::new(&config)?;
        let component = Component::from_file(&engine, wasm_path)?;

        let mut linker: Linker<HostState> = Linker::new(&engine);
        pf::plugin::host::add_to_linker::<HostState, wasmtime::component::HasSelf<HostState>>(
            &mut linker,
            |state| state,
        )?;
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;

        let (tx, rx) = mpsc::channel();
        let mut store = Store::new(
            &engine,
            HostState {
                table: ResourceTable::new(),
                ctx: wasmtime_wasi::WasiCtxBuilder::new().build(),
                tx,
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

    pub fn on_resize(&mut self, w: f32, h: f32) -> anyhow::Result<()> {
        if let (Some(store), Some(b)) = (self.store.as_mut(), self.bindings.as_mut()) {
            b.call_on_resize(store, w, h)?;
        }
        Ok(())
    }

    pub fn on_mouse(&mut self, phase: i32, button: i32, x: f32, y: f32) -> anyhow::Result<()> {
        if let (Some(store), Some(b)) = (self.store.as_mut(), self.bindings.as_mut()) {
            let ph = match phase {
                1 => pf::plugin::types::Phase::Down,
                2 => pf::plugin::types::Phase::Up,
                _ => pf::plugin::types::Phase::Move,
            };
            let bt = match button {
                1 => pf::plugin::types::Button::Right,
                _ => pf::plugin::types::Button::Left,
            };
            b.call_on_mouse(store, ph, bt, x, y)?;
        }
        Ok(())
    }

    pub fn wants_repaint(&mut self) -> bool {
        if let (Some(store), Some(b)) = (self.store.as_mut(), self.bindings.as_mut()) {
            return b.call_wants_repaint(store).unwrap_or(false);
        }
        false
    }

    pub fn render(&mut self) -> Vec<pf::plugin::types::Paint> {
        if let (Some(store), Some(b)) = (self.store.as_mut(), self.bindings.as_mut()) {
            return b.call_render(store).unwrap_or_default();
        }
        Vec::new()
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
