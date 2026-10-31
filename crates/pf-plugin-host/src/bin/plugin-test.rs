//! plugin-test —— 内层 wasm 插件的**离线测试驱动器**（通用 runner，零插件身份）。
//!
//! 归属：PF 仓（pf-plugin-host 附带 bin）。它只提供"装载/验签激活/调钩子/
//! 收动作"的宿主原语；**测试场景与断言全部住在插件仓**（pi-flash-plugins，
//! 见其 `inner/scripts/wasm-test.mjs`），通过 stdin JSONL 指令流驱动本进程。
//! 分工红线：PF 不出现任何插件的身份信息（id/wasm/proof 全由调用方传入）；
//! 插件仓不复制宿主运行时（wasmtime/WIT 绑定留在 PF），两端以本协议解耦。
//!
//! 用法：
//! ```text
//! plugin-test.exe            # stdin 逐行读指令，stdout 逐行回 JSON；EOF 退出
//! ```
//!
//! 指令（一行一条 JSON）：
//! - `{"op":"load","wasm":"<path>","cache":"<dir>"}` —— 装载组件（cache 可省 =
//!   每次全量编译；给了 = sha256 编译缓存，未命中编一次后毫秒级命中）。
//!   回执带钩子探测面 `has_fn_bar/has_session_row/has_render_popup`。
//! - `{"op":"init","id":"<plugin-id>","proof":"<proof JSON 串>"}` —— 验签激活
//!   （契约 v5：调 `load(plugin-ctx)`，ctx 由 runner 现组；op 名沿用 init）。
//!   proof 由调用方签发（签发流程见内部发布文档），runner 不持有任何密钥。**拒载后实例即弃**（真机同语义）：重测需重新 load。
//! - `{"op":"fn_bar","env":"<env JSON 串>"}` —— 回 `value` = chrome JSON | null。
//! - `{"op":"session_row","env":"<env JSON 串>"}` —— 回 `value` = 徽标 JSON | null。
//! - `{"op":"render_popup"}` —— 回 `value` = 弹窗组件树 JSON。
//! - `{"op":"render"}` —— v2 render 面。
//! - `{"op":"on_event","kind":"...","id":"...","path":"..."}` —— 事件
//!   （command/broadcast/page-active/…；path 位按契约放 env JSON 串）。
//! - `{"op":"pump_actions"}` —— 回 `actions` = 排水的宿主动作
//!   （open-popup/toast/open-file/run-script/new-draft）。
//! - `{"op":"unload"}` —— 90-9 §2 停用/退出前最后一次调用（L2 时序断言用）。
//!
//! 结果（一行一条 JSON）：`{"op":...,"ok":true,...}` / `{"op":...,"ok":false,"err":...}`。
//!
//! 内建近似 `measure-text`（字符数 × size）：与 app 真字体的 advance 有偏差，
//! 但居中/排版类断言够准（插件仓用同口径算期望值）。quick-pin 面恒空。

use std::cell::RefCell;
use std::io::{BufRead, Write};
use std::path::PathBuf;

use pf_plugin_host::{PluginAction, PluginInstance};

thread_local! {
    /// init 指令注入的证明材料。`HostProofFn = fn() -> String` 拿不到闭包
    /// 数据，走 thread_local 中转（与宿主 QUICK_PINS_FN 同模式）。
    static RUNTIME_PROOF: RefCell<String> = const { RefCell::new(String::new()) };
}

fn runtime_proof() -> String {
    RUNTIME_PROOF.with(|p| p.borrow().clone())
}

/// 近似文本测量：字符数 × 字号。真机是 gpui 逐字符 advance（kerning 不计），
/// 等宽近似对「N 字 × S px」的居中断言同口径。
fn approx_measure_text(content: &str, size: f32) -> f32 {
    content.chars().count() as f32 * size
}

fn no_quick_pins() -> Vec<String> {
    Vec::new()
}

/// 离线路径派生（`install_path_hooks` 缺省时 `is_executive` 恒 false，会把
/// 全部快捷项归进文档区）：扩展名白名单判可执行；图标取小写扩展名
/// （真机是图标主题映射，L2 不断言图标值，L3 覆盖）。
fn approx_is_exec(path: &str) -> bool {
    matches!(
        path.rsplit('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "exe" | "bat" | "cmd" | "com" | "ps1" | "msi" | "lnk" | "scr"
    )
}

fn approx_file_icon(path: &str) -> String {
    path.rsplit('.').next().unwrap_or("").to_ascii_lowercase()
}

fn emit(value: serde_json::Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{value}");
    let _ = out.flush();
}

fn main() {
    pf_plugin_host::install_proof_hook(runtime_proof);
    pf_plugin_host::install_host_hooks(no_quick_pins, approx_measure_text);
    pf_plugin_host::install_path_hooks(approx_file_icon, approx_is_exec);
    let stdin = std::io::stdin();
    let mut inst: Option<PluginInstance> = None;
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let cmd: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                emit(serde_json::json!({"ok": false, "err": format!("指令非合法 JSON: {e}")}));
                continue;
            }
        };
        let op = cmd["op"].as_str().unwrap_or_default().to_string();
        match handle(&op, &cmd, &mut inst) {
            Ok(v) => emit(v),
            Err(e) => emit(serde_json::json!({"op": op, "ok": false, "err": e})),
        }
    }
}

type R = Result<serde_json::Value, String>;

fn need_str<'a>(cmd: &'a serde_json::Value, key: &str, op: &str) -> Result<&'a str, String> {
    cmd[key]
        .as_str()
        .ok_or_else(|| format!("{op} 需要 {key}（字符串）"))
}

fn handle(op: &str, cmd: &serde_json::Value, inst: &mut Option<PluginInstance>) -> R {
    match op {
        "load" => {
            let wasm: PathBuf = need_str(cmd, "wasm", op)?.into();
            let loaded = match cmd["cache"].as_str() {
                Some(c) => PluginInstance::load_cached(&wasm, &PathBuf::from(c)),
                None => PluginInstance::load(&wasm),
            };
            let i = loaded.map_err(|e| format!("装载失败: {e}"))?;
            let face = serde_json::json!({
                "ok": true,
                "has_fn_bar": i.has_fn_bar(),
                "has_session_row": i.has_session_row(),
                "has_render_popup": i.has_render_popup(),
            });
            *inst = Some(i);
            Ok(face)
        }
        "init" => {
            let id = need_str(cmd, "id", op)?;
            let proof = need_str(cmd, "proof", op)?;
            RUNTIME_PROOF.with(|p| *p.borrow_mut() = proof.to_string());
            let i = inst.as_mut().ok_or_else(|| "init 前先 load".to_string())?;
            // 契约 v5：装载入口 = load(plugin-ctx)。runner 现组一份最小 ctx
            //（session null / cwd = wasm 同目录），case 断言装载与验签语义。
            let data_dir = cmd["wasm"]
                .as_str()
                .and_then(|w| {
                    std::path::Path::new(w)
                        .parent()
                        .map(|d| d.join("data").to_string_lossy().into_owned())
                })
                .unwrap_or_default();
            let ctx = serde_json::json!({
                "plugin": {"id": id, "version": "", "dataDir": data_dir},
                "pf": {"version": "plugin-test", "locale": "zh-CN"},
                "session": null,
                "cwd": "",
            })
            .to_string();
            match i.load_ctx(&ctx) {
                Ok(()) => Ok(serde_json::json!({"ok": true})),
                // 拒载后实例即弃（真机同语义）：验签门只此一次
                Err(e) => {
                    *inst = None;
                    Ok(serde_json::json!({"ok": false, "err": format!("{e}")}))
                }
            }
        }
        "fn_bar" => {
            let env = need_str(cmd, "env", op)?;
            let i = inst.as_mut().ok_or_else(|| "先 load+init".to_string())?;
            match i.fn_bar_json(env) {
                Ok(v) => Ok(serde_json::json!({"ok": true, "value": v})),
                Err(e) => Ok(serde_json::json!({"ok": false, "err": e})),
            }
        }
        "session_row" => {
            let env = need_str(cmd, "env", op)?;
            let i = inst.as_mut().ok_or_else(|| "先 load+init".to_string())?;
            match i.session_row_json(env) {
                Ok(v) => Ok(serde_json::json!({"ok": true, "value": v})),
                Err(e) => Ok(serde_json::json!({"ok": false, "err": e})),
            }
        }
        "render_popup" => {
            let i = inst.as_mut().ok_or_else(|| "先 load+init".to_string())?;
            match i.render_popup_json() {
                Ok(v) => Ok(serde_json::json!({"ok": true, "value": v})),
                Err(e) => Ok(serde_json::json!({"ok": false, "err": e})),
            }
        }
        "render" => {
            let i = inst.as_mut().ok_or_else(|| "先 load+init".to_string())?;
            match i.render_json() {
                Ok(v) => Ok(serde_json::json!({"ok": true, "value": v})),
                Err(e) => Ok(serde_json::json!({"ok": false, "err": format!("{e}")})),
            }
        }
        "on_event" => {
            let kind = need_str(cmd, "kind", op)?;
            let id = cmd["id"].as_str().unwrap_or_default();
            let path = cmd["path"].as_str().unwrap_or_default();
            let i = inst.as_mut().ok_or_else(|| "先 load+init".to_string())?;
            i.on_event(kind, id, path)
                .map(|_| serde_json::json!({"ok": true}))
                .map_err(|e| format!("{e}"))
        }
        "pump_actions" => {
            let i = inst.as_ref().ok_or_else(|| "先 load+init".to_string())?;
            let actions: Vec<serde_json::Value> =
                i.pump_actions().into_iter().map(action_json).collect();
            Ok(serde_json::json!({"ok": true, "actions": actions}))
        }
        "unload" => {
            // 90-9 §2：停用/卸载/退出前的最后一次调用。L2 断言它不崩、
            // 调用后动作排水为空（插件不得在 unload 里申请动作）。
            let i = inst.as_mut().ok_or_else(|| "先 load+init".to_string())?;
            i.unload().map(|_| serde_json::json!({"ok": true}))
        }
        other => Err(format!("未知 op: {other}")),
    }
}

fn action_json(a: PluginAction) -> serde_json::Value {
    match a {
        PluginAction::Toast(msg) => serde_json::json!({"kind": "toast", "text": msg}),
        PluginAction::OpenFile(path) => serde_json::json!({"kind": "open-file", "path": path}),
        PluginAction::RunScript(path) => serde_json::json!({"kind": "run-script", "path": path}),
        PluginAction::NewDraft { prefix, cwd } => {
            serde_json::json!({"kind": "new-draft", "prefix": prefix, "cwd": cwd})
        }
        PluginAction::OpenPopup(id) => serde_json::json!({"kind": "open-popup", "id": id}),
    }
}
