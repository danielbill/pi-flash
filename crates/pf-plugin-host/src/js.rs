//! QuickJS 插件运行时（beads 1im 双轨之一；契约 docs/PF组件规范/02 §3–§5）。
//!
//! 与同 crate 的 wasm 路线（lib.rs，原样保留）并行：这里加载清单 `entry`
//! 指向的 main.js（ESM、零 import、唯一能力面 = ctx），调 activate / render，
//! 右键与面板事件回灌插件。设计取向：**编排在 JS 胶水，Rust 只过原语**——
//! ctx 对象、onMenu 注册表、render 结果序列化全在一段 eval 的胶水里完成，
//! Rust 侧只暴露 String/bool 形状的宿主函数，rquickjs API 面积最小。
//!
//! 线程模型：rquickjs Runtime 非 Send，实例固定在 gpui 主线程（挂在 app
//! 的插件注册表上）；宿主动作类调用（run/edit/…）**入队不执行**，由调用方
//! 在 JS 返回后统一消费——同 wasm 侧 `PluginAction` 通道先例，避免在 gpui
//! 借用栈里嵌套重入。

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use rquickjs::function::Func;
use rquickjs::{Context, Function, Module, Runtime};

/// 宿主能力面（02 规范 §3）。app 侧实现；提问类同步应答，动作类只入队。
pub trait JsHost {
    /// `ctx.isExecutive`：宿主按后缀/平台判定（script_exec 同表）。
    fn is_executive(&mut self, path: &str) -> bool;
    /// `ctx.fileIcon`：不透明引用，宿主解析，插件只透传。
    fn file_icon(&mut self, path: &str) -> String;
    /// `ctx.store.read`：整读 store.json 文本；缺失/坏文件 → "{}"。
    fn store_read(&mut self) -> String;
    /// `ctx.store.write`：整写（宿主保证原子：临时文件 + rename）。
    fn store_write(&mut self, json: String);
    /// 动作入队：JS 调用返回后由宿主消费（见 [`JsAction`]）。
    fn queue(&mut self, action: JsAction);
}

/// 延迟动作（02 §3 动作类）。消费方按动作名分派宿主实现。
#[derive(Debug, Clone)]
pub enum JsAction {
    /// 021 执行链，新终端 tab
    Run(String),
    /// 编辑器/查看器打开
    Edit(String),
    /// 资源管理器定位
    RevealInExplorer(String),
    /// 轻提示
    Toast(String),
}

/// 一个已装载的 JS 插件实例（全局单例：面板开着时右键复用同一份 store）。
pub struct JsPlugin {
    _rt: Runtime,
    ctx: Context,
    /// 保活宿主（闭包里持的是它的 Rc 克隆）
    _host: Rc<RefCell<dyn JsHost>>,
}

impl JsPlugin {
    /// 装载：读 entry 源码 → 声明为 ES 模块并求值 → 注入宿主原语与胶水。
    /// `store_path` 注入 `ctx.store.path`（只读字符串）。
    pub fn load(
        entry: &Path,
        store_path: &str,
        host: Rc<RefCell<dyn JsHost>>,
    ) -> Result<Self, String> {
        let src = std::fs::read_to_string(entry)
            .map_err(|e| format!("entry 不可读（{}）: {e}", entry.display()))?;

        let rt = Runtime::new().map_err(|e| e.to_string())?;
        // v1 内存上限：插件逻辑是清单/store 级小计算，32MB 足够宽裕
        rt.set_memory_limit(32 * 1024 * 1024);
        let ctx = Context::full(&rt).map_err(|e| e.to_string())?;

        let glue_host = host.clone();
        ctx.with(|ctx| -> Result<(), rquickjs::Error> {
            // —— 宿主原语（只过 String/bool）——
            let g = ctx.globals();
            let h = glue_host.clone();
            g.set(
                "__pf_is_executive",
                Func::from(move |path: String| -> bool { h.borrow_mut().is_executive(&path) }),
            )?;
            let h = glue_host.clone();
            g.set(
                "__pf_file_icon",
                Func::from(move |path: String| -> String { h.borrow_mut().file_icon(&path) }),
            )?;
            let h = glue_host.clone();
            g.set(
                "__pf_store_read",
                Func::from(move || -> String { h.borrow_mut().store_read() }),
            )?;
            let h = glue_host.clone();
            g.set(
                "__pf_store_write",
                Func::from(move |json: String| {
                    h.borrow_mut().store_write(json);
                }),
            )?;
            for (name, ctor) in [
                ("__pf_run", JsAction::Run as fn(String) -> JsAction),
                ("__pf_edit", JsAction::Edit as fn(String) -> JsAction),
                (
                    "__pf_reveal",
                    JsAction::RevealInExplorer as fn(String) -> JsAction,
                ),
                ("__pf_toast", JsAction::Toast as fn(String) -> JsAction),
            ] {
                let h = glue_host.clone();
                g.set(
                    name,
                    Func::from(move |arg: String| {
                        h.borrow_mut().queue(ctor(arg));
                    }),
                )?;
            }

            // —— 插件模块（ESM）——
            let module = Module::declare(ctx.clone(), "pf-plugin-entry", src.into_bytes())?;
            let (evaluated, _promise) = module.eval()?;
            let ns = evaluated.namespace();
            g.set("__pf_exports", ns)?;

            // —— 胶水：ctx 对象 + 事件注册表 + 宿主入口 ——
            let store_path_js = json_escape(store_path);
            let _: () = ctx.eval(format!(
                r#"
globalThis.__pf_ctx = {{
  isExecutive: (p) => globalThis.__pf_is_executive(p),
  fileIcon: (p) => globalThis.__pf_file_icon(p),
  store: {{
    read: () => JSON.parse(globalThis.__pf_store_read() || "{{}}"),
    write: (o) => globalThis.__pf_store_write(JSON.stringify(o)),
    path: "{store_path_js}",
  }},
  run: (p) => globalThis.__pf_run(p),
  edit: (p) => globalThis.__pf_edit(p),
  revealInExplorer: (p) => globalThis.__pf_reveal(p),
  toast: (m) => globalThis.__pf_toast(m == null ? "" : String(m)),
  onMenu: (id, fn) => {{ globalThis["__pf_menu_" + id] = fn; }},
  onPanelOpen: (fn) => {{ globalThis.__pf_panel_open_fn = fn; }},
}};
globalThis.__pf_activate = () => {{
  if (typeof globalThis.__pf_exports.activate === "function")
    globalThis.__pf_exports.activate(globalThis.__pf_ctx);
}};
globalThis.__pf_render_str = () =>
  JSON.stringify(globalThis.__pf_exports.render(globalThis.__pf_ctx));
globalThis.__pf_fire_menu = (id, path, fileIcon) => {{
  const fn = globalThis["__pf_menu_" + id];
  if (typeof fn === "function") fn({{ path, fileIcon }});
}};
globalThis.__pf_fire_panel_open = () => {{
  if (typeof globalThis.__pf_panel_open_fn === "function")
    globalThis.__pf_panel_open_fn();
}};
"#,
            ))?;
            Ok(())
        })
        .map_err(|e| e.to_string())?;

        Ok(Self {
            _rt: rt,
            ctx,
            _host: host,
        })
    }

    /// `activate(ctx)`：注册 onMenu / onPanelOpen（无 activate 导出 = 纯渲染插件，放行）。
    pub fn activate(&self) -> Result<(), String> {
        self.ctx
            .with(|ctx| -> Result<(), rquickjs::Error> {
                let f: Function = ctx.globals().get("__pf_activate")?;
                f.call::<(), ()>(())?;
                Ok(())
            })
            .map_err(|e| e.to_string())
    }

    /// `render(ctx)` → `{body, rows}` 的 JSON 文本（01 规范组件树 + 数据行）。
    pub fn render_json(&self) -> Result<String, String> {
        self.ctx
            .with(|ctx| -> Result<String, rquickjs::Error> {
                let f: Function = ctx.globals().get("__pf_render_str")?;
                f.call::<(), String>(())
            })
            .map_err(|e| e.to_string())
    }

    /// 触发右键菜单回调 `onMenu(id, {path, fileIcon})`（未注册 = no-op）。
    pub fn fire_menu(&self, id: &str, path: &str, file_icon: &str) -> Result<(), String> {
        self.ctx
            .with(|ctx| -> Result<(), rquickjs::Error> {
                let f: Function = ctx.globals().get("__pf_fire_menu")?;
                f.call::<(&str, &str, &str), ()>((id, path, file_icon))?;
                Ok(())
            })
            .map_err(|e| e.to_string())
    }

    /// 触发面板激活回调 `onPanelOpen()`（未注册 = no-op）。
    pub fn fire_panel_open(&self) -> Result<(), String> {
        self.ctx
            .with(|ctx| -> Result<(), rquickjs::Error> {
                let f: Function = ctx.globals().get("__pf_fire_panel_open")?;
                f.call::<(), ()>(())?;
                Ok(())
            })
            .map_err(|e| e.to_string())
    }
}

/// 注入 JS 字符串字面量的最小转义（`\ " ` 与控制符；清单路径是本地明文，够用）。
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试宿主：store 内存态 + 动作入队（同 app 侧 ChatHost 形状）。
    struct TestHost {
        store: String,
        actions: Vec<JsAction>,
    }
    impl JsHost for TestHost {
        fn is_executive(&mut self, path: &str) -> bool {
            path.ends_with(".ps1") || path.ends_with(".bat")
        }
        fn file_icon(&mut self, path: &str) -> String {
            if path.ends_with(".md") {
                "file-text".into()
            } else {
                "terminal".into()
            }
        }
        fn store_read(&mut self) -> String {
            self.store.clone()
        }
        fn store_write(&mut self, json: String) {
            self.store = json;
        }
        fn queue(&mut self, action: JsAction) {
            self.actions.push(action);
        }
    }

    /// 与插件仓 service/plugin/main.js 同契约的最小插件源码。
    const PLUGIN: &str = r#"
let opened = false;
export function activate(ctx) {
  ctx.onMenu("set-quick", ({ path, fileIcon }) => {
    const store = ctx.store.read();
    const pins = Array.isArray(store.pins) ? store.pins : [];
    if (pins.some((p) => p.path === path)) { ctx.toast("dup"); return; }
    pins.push({ path, fileIcon, isExec: ctx.isExecutive(path) });
    ctx.store.write({ ...store, pins });
    ctx.toast("pinned");
  });
  ctx.onPanelOpen(() => { opened = true; });
}
export function render(ctx) {
  const pins = ctx.store.read().pins ?? [];
  return {
    body: { type: "list", layout: "grid",
            item: { icon: "$fileIcon", label: "$name", tip: "$path",
                    action: "$auto", arg: "$path" } },
    rows: pins.map((p) => ({ path: p.path, name: p.path.split(/[\\/]/).pop(),
                             fileIcon: p.fileIcon, isExec: p.isExec })),
  };
}
"#;

    #[test]
    fn quickjs_full_chain() {
        let host = Rc::new(RefCell::new(TestHost {
            store: "{}".into(),
            actions: vec![],
        }));
        let dir = std::env::temp_dir().join("pf-js-test-main.js");
        std::fs::write(&dir, PLUGIN).unwrap();
        let plugin = JsPlugin::load(&dir, r"C:\store\store.json", host.clone()).expect("load");
        plugin.activate().expect("activate");

        // 右键设为快捷（exe + 文档各一）+ 去重
        plugin
            .fire_menu("set-quick", r"D:\p\build.ps1", "terminal")
            .expect("menu1");
        plugin
            .fire_menu("set-quick", r"D:\p\README.md", "file-text")
            .expect("menu2");
        plugin
            .fire_menu("set-quick", r"D:\p\build.ps1", "terminal")
            .expect("menu-dup");
        let toasts: Vec<_> = host
            .borrow()
            .actions
            .iter()
            .map(|a| match a {
                JsAction::Toast(m) => m.clone(),
                _ => String::new(),
            })
            .collect();
        assert_eq!(toasts, ["pinned", "pinned", "dup"]);
        assert!(host.borrow().store.contains("build.ps1"));

        // 面板打开 + render
        plugin.fire_panel_open().expect("panel open");
        let json = plugin.render_json().expect("render");
        let v: serde_json::Value = serde_json::from_str(&json).expect("render 返回合法 JSON");
        assert_eq!(v["body"]["layout"], "grid");
        assert_eq!(v["body"]["item"]["action"], "$auto");
        let rows = v["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["name"], "build.ps1");
        assert_eq!(rows[0]["isExec"], true);
        assert_eq!(rows[1]["isExec"], false);

        // 异常插件：render 抛错 → Err（app 侧 toast + 会话级禁用）
        let bad = std::env::temp_dir().join("pf-js-test-bad.js");
        std::fs::write(&bad, "export function render(ctx) { no_such_fn(); }").unwrap();
        let plugin = JsPlugin::load(&bad, "{}", host.clone()).expect("load bad");
        assert!(plugin.render_json().is_err());
        let _ = std::fs::remove_file(&dir);
        let _ = std::fs::remove_file(&bad);
    }
}
