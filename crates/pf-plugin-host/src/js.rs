//! QuickJS 插件运行时（契约 docs/PF组件规范/01+02 v2、schema/plugin-contract.json）。
//!
//! 加载清单 `entry` 指向的 main.js（ESM、零 import、唯一能力面 = ctx），
//! 调 activate / render，右键与面板命令回灌插件。
//!
//! v2 与 v1 的差别（090-9）：
//! - **没有 store.json**：插件数据自管，`ctx.dir` + `ctx.fs.read/write/list`，
//!   Rust 侧只保证目录、原子写、越界拒绝，**从不解析内容**
//! - 新增 `ctx.onCommand`：组件 `menu` 项（不带 action 的）回调插件
//! - `render` 返回**已算好的组件树**，没有 rows / filter / `$auto`
//!
//! 线程模型：rquickjs Runtime 非 Send，实例固定在 gpui 主线程；动作类调用
//! 入队不执行，由调用方在 JS 返回后统一消费。

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use crate::nonce_tag;
use rquickjs::function::Func;
use rquickjs::{Context, Function, Module, Runtime};

/// 宿主能力面（02 §3）。app 侧实现；提问类同步应答，动作类只入队。
pub trait JsHost {
    /// `ctx.isExecutive`：宿主按后缀/平台判定（script_exec 同表）。
    fn is_executive(&mut self, path: &str) -> bool;
    /// `ctx.fileIcon`：不透明引用，宿主解析，插件只透传。
    fn file_icon(&mut self, path: &str) -> String;
    /// `ctx.dir`：插件私有目录（绝对路径）。
    fn dir(&mut self) -> String;
    /// `ctx.fs.read(name)`：整读；缺失/不可读 → 空串（插件自己兜底）。
    fn fs_read(&mut self, name: &str) -> String;
    /// `ctx.fs.write(name, data)`：原子写（临时文件 + rename），只限 `ctx.dir` 内。
    fn fs_write(&mut self, name: &str, data: &str);
    /// `ctx.fs.list()`：目录下文件名。
    fn fs_list(&mut self) -> Vec<String>;
    /// `ctx.service.post(anchor, msg)`（契约 v4 执行域，90-6）：
    /// 发往执行域实例；未运行 → 宿主排队，溢出 = false。默认拒收（无执行域宿主）。
    fn svc_post(&mut self, _anchor: &str, _payload: &str) -> bool {
        false
    }
    /// `ctx.service.query(anchor)` → running | stopped | absent。宿主生命周期事实。
    fn svc_query(&mut self, _anchor: &str) -> String {
        "absent".into()
    }
    /// `ctx.hostProof` 的材料源（090-5 L3）：构建期注入的官方宿主证明，
    /// 紧凑 JSON `{"kid":"…","proof":"…"}`。**默认 = 占位符（fail-closed）**：
    /// 未覆写的宿主实现（含 fork 自建）只会递出验签必失败的占位符。
    fn host_proof(&mut self) -> String {
        "{\"kid\":\"\",\"proof\":\"PROOF_PLACEHOLDER\"}".into()
    }
    /// 动作入队：JS 调用返回后由宿主消费（见 [`JsAction`]）。
    fn queue(&mut self, action: JsAction);
}

/// 延迟动作（02 §3 动作类）。消费方按动作名分派宿主实现。
#[derive(Debug, Clone, PartialEq)]
pub enum JsAction {
    /// 021 执行链，新终端 tab
    Run(String),
    /// 编辑器/查看器打开
    Edit(String),
    /// 资源管理器定位
    RevealInExplorer(String),
    /// 轻提示
    Toast(String),
    /// 系统浏览器打开 URL（01 §8；插件不拼 shell 命令）
    OpenUrl(String),
    /// 资源管理器打开目录
    OpenFolder(String),
    /// 新建会话（不携带内容）
    NewSession,
    /// 意图调用：新建插件认领会话（契约 v2.4 sessions）。ctx 同步无返回值
    /// （动作队列模型），宿主建线后经 broadcast "claim" 事件回推 key。
    NewDraft {
        prefix: String,
        cwd: String,
    },
    /// 把文本填进输入框（**只预填、不代发**：发不发由用户决定）
    AskPi(String),
    /// 打开插件声明的 popups 条目（契约 v3 act.openPopup；非模态，宿主管窗体）
    OpenPopup(String),
    /// 契约 v3：会话列表徽标（落时间位置；owner = 插件 id，多插件互不覆盖）
    SetBadge {
        session: String,
        icon: String,
        tone: String,
    },
    ClearBadge {
        session: String,
    },
    /// 关闭插件弹窗（契约 v3 act.closePopup）
    ClosePopup(String),
}

/// 一个已装载的 JS 插件实例（全局单例：面板开着时右键复用同一份 ctx）。
pub struct JsPlugin {
    _rt: Runtime,
    ctx: Context,
    /// 保活宿主（闭包里持的是它的 Rc 克隆）
    _host: Rc<RefCell<dyn JsHost>>,
    /// JS 调用死线（ms 时间戳；0 = 未布防）。每次入口调用前刷新为
    /// now+超时；超时触发 rquickjs 中断 = JS 失控抛异常而非挂死 UI 线程。
    deadline: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

/// 单次 JS 调用上限（ms）。插件逻辑是清单级小计算，5s 已是百倍宽裕。
const JS_CALL_DEADLINE_MS: u64 = 5_000;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl JsPlugin {
    /// 装载：读 entry 源码 → 声明为 ES 模块并求值 → 注入宿主原语与胶水。
    /// `dir` 注入 `ctx.dir`（插件私有目录，绝对路径）。
    /// 从源码加载（明文标准包）。
    pub fn load(entry: &Path, dir: &str, host: Rc<RefCell<dyn JsHost>>) -> Result<Self, String> {
        let src = std::fs::read_to_string(entry)
            .map_err(|e| format!("entry 不可读（{}）: {e}", entry.display()))?;
        Self::load_src(src, dir, host)
    }

    /// 从 QuickJS **字节码**加载（分发态：源码编译成 `plugin.qbc`，落盘无明文）。
    /// 引擎、ctx 能力面、渲染路径与源码版**完全相同**——同一份 JS 编出来的。
    pub fn load_bytecode(
        entry: &Path,
        dir: &str,
        host: Rc<RefCell<dyn JsHost>>,
    ) -> Result<Self, String> {
        let bytes = std::fs::read(entry)
            .map_err(|e| format!("字节码不可读（{}）: {e}", entry.display()))?;
        Self::load_parts(None, Some(bytes), dir, host)
    }

    fn load_src(src: String, dir: &str, host: Rc<RefCell<dyn JsHost>>) -> Result<Self, String> {
        Self::load_parts(Some(src), None, dir, host)
    }

    fn load_parts(
        src: Option<String>,
        bytecode: Option<Vec<u8>>,
        dir: &str,
        host: Rc<RefCell<dyn JsHost>>,
    ) -> Result<Self, String> {
        let rt = Runtime::new().map_err(|e| e.to_string())?;
        // v1 内存上限：插件逻辑是清单级小计算，32MB 足够宽裕
        rt.set_memory_limit(32 * 1024 * 1024);
        // JS 调用死线：入口方法前 arm（deadline=now+5s），超时中断抛异常
        // ——插件 JS 失控（死循环）不再挂死 UI 线程（2026 真机卡死教训）
        let deadline: std::sync::Arc<std::sync::atomic::AtomicU64> =
            std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        {
            let dl = deadline.clone();
            rt.set_interrupt_handler(Some(Box::new(move || {
                let limit = dl.load(std::sync::atomic::Ordering::Relaxed);
                limit != 0 && now_ms() >= limit
            })));
        }
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
                "__pf_fs_read",
                Func::from(move |name: String| -> String { h.borrow_mut().fs_read(&name) }),
            )?;
            let h = glue_host.clone();
            g.set(
                "__pf_fs_write",
                Func::from(move |name: String, data: String| h.borrow_mut().fs_write(&name, &data)),
            )?;
            let h = glue_host.clone();
            g.set(
                "__pf_svc_post",
                Func::from(move |anchor: String, payload: String| -> bool {
                    h.borrow_mut().svc_post(&anchor, &payload)
                }),
            )?;
            let h = glue_host.clone();
            g.set(
                "__pf_svc_query",
                Func::from(move |anchor: String| -> String { h.borrow_mut().svc_query(&anchor) }),
            )?;
            let h = glue_host.clone();
            g.set(
                "__pf_fs_list",
                Func::from(move || -> String {
                    let names = h.borrow_mut().fs_list();
                    serde_json::to_string(&names).unwrap_or_else(|_| "[]".into())
                }),
            )?;
            // 宿主证明交付（090-5 L3）：宿主只递数据不判定；nonceTag 在此派生，
            // 插件 wasm 内按同一算法复算比对（算法见 nonce_tag 注释，公开）。
            let h = glue_host.clone();
            g.set(
                "__pf_host_proof",
                Func::from(move |nonce: String| -> String {
                    let base = h.borrow_mut().host_proof();
                    let v: serde_json::Value =
                        serde_json::from_str(&base).unwrap_or(serde_json::Value::Null);
                    let kid = v["kid"].as_str().unwrap_or("").to_string();
                    let proof = v["proof"]
                        .as_str()
                        .unwrap_or("PROOF_PLACEHOLDER")
                        .to_string();
                    serde_json::json!({
                        "kid": kid,
                        "proof": proof,
                        "nonceTag": nonce_tag(&proof, &nonce),
                    })
                    .to_string()
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
                ("__pf_open_url", JsAction::OpenUrl as fn(String) -> JsAction),
                (
                    "__pf_open_folder",
                    JsAction::OpenFolder as fn(String) -> JsAction,
                ),
                (
                    "__pf_new_session",
                    (|_s: String| JsAction::NewSession) as fn(String) -> JsAction,
                ),
                ("__pf_ask_pi", JsAction::AskPi as fn(String) -> JsAction),
                // 契约 v3 popups：act.openPopup / act.closePopup
                ("__pf_open_popup", JsAction::OpenPopup as fn(String) -> JsAction),
                ("__pf_close_popup", JsAction::ClosePopup as fn(String) -> JsAction),
            ] {
                let h = glue_host.clone();
                g.set(
                    name,
                    Func::from(move |arg: String| {
                        h.borrow_mut().queue(ctor(arg));
                    }),
                )?;
            }
            // 契约 v3 sessions.setBadge / clearBadge（会话列表时间位徽标）
            let h = glue_host.clone();
            g.set(
                "__pf_set_badge",
                Func::from(move |json: String| {
                    let v: serde_json::Value =
                        serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
                    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
                    h.borrow_mut().queue(JsAction::SetBadge {
                        session: s("session"),
                        icon: s("icon"),
                        tone: if s("tone").is_empty() { "accent".into() } else { s("tone") },
                    });
                }),
            )?;
            let h = glue_host.clone();
            g.set(
                "__pf_clear_badge",
                Func::from(move |json: String| {
                    let v: serde_json::Value =
                        serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
                    h.borrow_mut().queue(JsAction::ClearBadge {
                        session: v.get("session").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    });
                }),
            )?;
            // 双参动作（契约 v2.4 sessions）：新建插件认领会话
            let h = glue_host.clone();
            g.set(
                "__pf_new_draft",
                Func::from(move |prefix: String, cwd: String| {
                    h.borrow_mut().queue(JsAction::NewDraft { prefix, cwd });
                }),
            )?;

            // —— 插件模块（ESM）——
            let module = match (src.clone(), bytecode.clone()) {
                (Some(src), _) => {
                    Module::declare(ctx.clone(), "pf-plugin-entry", src.into_bytes())?
                }
                // SAFETY：字节码由本仓 `qjsc`（同一 quickjs-ng 版本）产出；
                // 版本不符 QuickJS 自身会拒读（JS_ReadObject 校验版本魔数）。
                (None, Some(bc)) => unsafe { Module::load(ctx.clone(), &bc)? },
                (None, None) => return Err(rquickjs::Error::Unknown),
            };
            let (evaluated, _promise) = module.eval()?;
            let ns = evaluated.namespace();
            g.set("__pf_exports", ns)?;

            // —— 胶水：ctx 对象 + 注册表 + 宿主入口 ——
            let dir_js = json_escape(dir);
            let _: () = ctx.eval(format!(
                r#"
globalThis.__pf_ctx = {{
  dir: "{dir_js}",
  fs: {{
    read: (n) => globalThis.__pf_fs_read(String(n)),
    write: (n, v) => {{ globalThis.__pf_fs_write(String(n), v == null ? "" : String(v)); }},
    list: () => JSON.parse(globalThis.__pf_fs_list()),
  }},
  isExecutive: (p) => globalThis.__pf_is_executive(p),
  fileIcon: (p) => globalThis.__pf_file_icon(p),
  hostProof: (n) => JSON.parse(globalThis.__pf_host_proof(n == null ? "" : String(n))),
  run: (p) => globalThis.__pf_run(p),
  edit: (p) => globalThis.__pf_edit(p),
  revealInExplorer: (p) => globalThis.__pf_reveal(p),
  toast: (m) => globalThis.__pf_toast(m == null ? "" : String(m)),
  openUrl: (u) => globalThis.__pf_open_url(String(u)),
  openFolder: (p) => globalThis.__pf_open_folder(String(p)),
  newSession: () => globalThis.__pf_new_session(""),
  newDraft: (p, c) => {{
    globalThis.__pf_new_draft(p == null ? "" : String(p), c == null ? "" : String(c));
  }},
  // 契约 v2.4 会话槽 RPC 面：ctx.sessions.newDraft（扁平 newDraft 为旧兼容别名）
  sessions: {{
    newDraft: (p, c) => globalThis.__pf_new_draft(p == null ? "" : String(p), c == null ? "" : String(c)),
    setBadge: (b) => globalThis.__pf_set_badge(JSON.stringify(b ?? {{}})),
    clearBadge: (b) => globalThis.__pf_clear_badge(JSON.stringify(b ?? {{}})),
  }},
  // 契约 v4 执行域（90-6）：通道/查询；onMessage(payload, meta) 由宿主 outbox 泵投递
  service: {{
    post: (a, m) => globalThis.__pf_svc_post(
      String(a),
      typeof m === "string" ? m : JSON.stringify(m == null ? null : m)
    ),
    onMessage: (h) => {{ globalThis.__pf_svc_msg_fn = h; }},
    query: (a) => globalThis.__pf_svc_query(String(a)),
  }},
  askPi: (t) => globalThis.__pf_ask_pi(t == null ? "" : String(t)),
  // 契约 v3：动作命名空间（扁平面 toast 等保持不动，兼容存量插件）
  act: {{
    openPopup: (id) => globalThis.__pf_open_popup(id == null ? "" : String(id)),
    closePopup: (id) => globalThis.__pf_close_popup(id == null ? "" : String(id)),
  }},
  onMenu: (id, fn) => {{ globalThis["__pf_menu_" + id] = fn; }},
  onCommand: (id, fn) => {{ globalThis["__pf_cmd_" + id] = fn; }},
  onPanelOpen: (fn) => {{ globalThis.__pf_panel_open_fn = fn; }},
  onBroadcast: (name, fn) => {{ globalThis["__pf_bc_" + name] = fn; }},
}};
globalThis.__pf_activate = () => {{
  if (typeof globalThis.__pf_exports.activate === "function")
    globalThis.__pf_exports.activate(globalThis.__pf_ctx);
}};
// 90-9 §2：unload() = 停用/卸载/更新覆盖/退出前的最后一次调用。未导出 =
// no-op（unload 必需但允许空实现；旧包缺导出同空）。
globalThis.__pf_unload = () => {{
  if (typeof globalThis.__pf_exports.unload === "function")
    globalThis.__pf_exports.unload();
}};
globalThis.__pf_render_str = () =>
  JSON.stringify(globalThis.__pf_exports.render(globalThis.__pf_ctx));
globalThis.__pf_fire_menu = (id, path, fileIcon) => {{
  const fn = globalThis["__pf_menu_" + id];
  if (typeof fn === "function") fn({{ path, fileIcon }});
}};
globalThis.__pf_fire_cmd = (id, path) => {{
  const fn = globalThis["__pf_cmd_" + id];
  if (typeof fn === "function") fn({{ path }});
}};
// 契约 v3 命令 env：fn_bar 点击 = {{"session":{{"key":"…"}}}}；switch 翻转 = {{"value":bool}}；
// envJson 空串按 {{}} 解。旧 path 面（__pf_fire_cmd）不动。
globalThis.__pf_fire_cmd_env = (id, envJson) => {{
  const fn = globalThis["__pf_cmd_" + id];
  if (typeof fn === "function") fn(JSON.parse(envJson || "{{}}"));
}};
// 契约 v3 fn_bar：插件导出 fn_bar(ctx, env) → {{icon,tone,tip}}；未导出 = "null"。
globalThis.__pf_fn_bar_str = (envJson) =>
  typeof globalThis.__pf_exports.fn_bar === "function"
    ? JSON.stringify(globalThis.__pf_exports.fn_bar(globalThis.__pf_ctx, JSON.parse(envJson || "{{}}")))
    : "null";
// PF 钩子（schema/pf-hooks.json）：page_active = 页面激活（PF 独有概念，
// pi 一个进程只服务一个会话，没有"当前页"）；session_row = 会话列表行徽标。
globalThis.__pf_page_active_str = (envJson) => {{
  const fn = globalThis.__pf_exports.page_actived;
  if (typeof fn === "function") {{
    fn(globalThis.__pf_ctx, JSON.parse(envJson || "{{}}"));
    return "1";
  }}
  return "0";
}};
globalThis.__pf_session_row_str = (envJson) =>
  typeof globalThis.__pf_exports.session_list_render === "function"
    ? JSON.stringify(globalThis.__pf_exports.session_list_render(globalThis.__pf_ctx, JSON.parse(envJson || "{{}}")))
    : "null";
// 契约 v3 弹窗内容：render_popup 优先，退回 render（旧导出面兼容）。
globalThis.__pf_render_popup_str = () =>
  typeof globalThis.__pf_exports.render_popup === "function"
    ? JSON.stringify(globalThis.__pf_exports.render_popup(globalThis.__pf_ctx))
    : JSON.stringify(globalThis.__pf_exports.render(globalThis.__pf_ctx));
globalThis.__pf_fire_panel_open = () => {{
  if (typeof globalThis.__pf_panel_open_fn === "function")
    globalThis.__pf_panel_open_fn();
}};
globalThis.__pf_fire_broadcast = (name, json) => {{
  const fn = globalThis["__pf_bc_" + name];
  if (typeof fn === "function") fn(JSON.parse(json || "null"));
}};
globalThis.__pf_fire_svc_message = (json, meta) => {{
  if (typeof globalThis.__pf_svc_msg_fn === "function")
    globalThis.__pf_svc_msg_fn(JSON.parse(json || "null"), JSON.parse(meta || "null"));
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
            deadline,
        })
    }

    /// 布防死线：本次入口调用最多 JS_CALL_DEADLINE_MS。
    fn arm_deadline(&self) {
        self.deadline.store(
            now_ms() + JS_CALL_DEADLINE_MS,
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    /// `activate(ctx)`：注册 onMenu / onCommand / onPanelOpen（无 activate 导出 = 纯渲染插件，放行）。
    pub fn activate(&self) -> Result<(), String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> rquickjs::Result<()> {
                let f: Function = ctx.globals().get("__pf_activate")?;
                f.call::<(), ()>(())?;
                Ok(())
            })
            .map_err(|e| e.to_string())
    }

    /// `unload()`（90-9 §2）：停用/卸载/更新覆盖/退出前的最后一次调用。
    /// 未导出 = no-op（`__pf_unload` 胶水恒在，内部按导出判空）。Err 只落
    /// plugin.log，宿主不等待确认（best-effort，关不掉的插件不能卡宿主）。
    pub fn unload(&self) -> Result<(), String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> rquickjs::Result<()> {
                let f: Function = ctx.globals().get("__pf_unload")?;
                f.call::<(), ()>(())?;
                Ok(())
            })
            .map_err(|e| e.to_string())
    }

    /// `render(ctx)` → 组件树 JSON（01 规范；v2 无 rows）。
    pub fn render_json(&self) -> Result<String, String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> Result<String, rquickjs::Error> {
                let f: Function = ctx.globals().get("__pf_render_str")?;
                f.call::<(), String>(())
            })
            .map_err(|e| e.to_string())
    }

    /// 触发右键菜单回调 `onMenu(id, {path, fileIcon})`（未注册 = no-op）。
    pub fn fire_menu(&self, id: &str, path: &str, file_icon: &str) -> Result<(), String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> Result<(), rquickjs::Error> {
                let f: Function = ctx.globals().get("__pf_fire_menu")?;
                f.call::<(&str, &str, &str), ()>((id, path, file_icon))?;
                Ok(())
            })
            .map_err(|e| e.to_string())
    }

    /// 触发命令回调（env 负载形态，契约 v3）：fn_bar 点击 =
    /// {"session":{"key":"…"}}，switch 翻转 = {"value":bool}；未注册 = no-op。
    pub fn fire_command_env(&self, id: &str, env_json: &str) -> Result<(), String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> Result<(), rquickjs::Error> {
                let f: Function = ctx.globals().get("__pf_fire_cmd_env")?;
                f.call::<(&str, &str), ()>((id, env_json))?;
                Ok(())
            })
            .map_err(|e| e.to_string())
    }

    /// 触发组件菜单回调 `onCommand(id, {path})`（未注册 = no-op）。
    pub fn fire_command(&self, id: &str, path: &str) -> Result<(), String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> Result<(), rquickjs::Error> {
                let f: Function = ctx.globals().get("__pf_fire_cmd")?;
                f.call::<(&str, &str), ()>((id, path))?;
                Ok(())
            })
            .map_err(|e| e.to_string())
    }

    /// 调 fn_bar(ctx, env)（契约 v3 功能栏 chrome）。未导出 → Ok(None)。
    /// 返回 JSON 串 {"icon":"…","tone":"…","tip":…}。
    pub fn fn_bar_json(&self, env_json: &str) -> Result<Option<String>, String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> rquickjs::Result<String> {
                let f: Function = ctx.globals().get("__pf_fn_bar_str")?;
                f.call::<(&str,), String>((env_json,))
            })
            .map(|s| if s == "null" { None } else { Some(s) })
            .map_err(|e| e.to_string())
    }

    /// PF 钩子 page_active(ctx, env)：页面激活推送（返回值恒 null，忽略）。
    pub fn page_active(&self, env_json: &str) -> Result<String, String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> rquickjs::Result<String> {
                let f: Function = ctx.globals().get("__pf_page_active_str")?;
                f.call::<(&str,), String>((env_json,))
            })
            .map_err(|e| e.to_string())
    }

    /// PF 钩子 session_row(ctx, env) → null | {icon,tone}（会话列表时间位徽标）。
    pub fn session_row_json(&self, env_json: &str) -> Result<Option<String>, String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> rquickjs::Result<String> {
                let f: Function = ctx.globals().get("__pf_session_row_str")?;
                f.call::<(&str,), String>((env_json,))
            })
            .map(|s| if s == "null" { None } else { Some(s) })
            .map_err(|e| e.to_string())
    }

    /// render_popup(ctx)（契约 v3 弹窗内容；无导出退回 render）→ 树 JSON。
    pub fn render_popup_json(&self) -> Result<String, String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> Result<String, rquickjs::Error> {
                let f: Function = ctx.globals().get("__pf_render_popup_str")?;
                f.call::<(), String>(())
            })
            .map_err(|e| e.to_string())
    }

    /// 触发面板激活回调 `onPanelOpen()`（未注册 = no-op）。
    pub fn fire_panel_open(&self) -> Result<(), String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> Result<(), rquickjs::Error> {
                let f: Function = ctx.globals().get("__pf_fire_panel_open")?;
                f.call::<(), ()>(())?;
                Ok(())
            })
            .map_err(|e| e.to_string())
    }

    /// 触发组件状态广播回调 `onBroadcast(name, payload)`（未注册 = no-op）。
    /// payload = 负载 JSON 串（契约 v2.4 broadcast_events）。
    pub fn fire_broadcast(&self, name: &str, payload: &str) -> Result<(), String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> Result<(), rquickjs::Error> {
                let f: Function = ctx.globals().get("__pf_fire_broadcast")?;
                f.call::<(&str, &str), ()>((name, payload))?;
                Ok(())
            })
            .map_err(|e| e.to_string())
    }

    /// 触发执行域推送回调 `onMessage(payload, meta)`（未注册 = no-op）。
    /// payload = 不透明负载 JSON 串；meta = {anchor, key}（来源实例锚点 + 会话当前键）。
    pub fn fire_svc_message(&self, payload: &str, meta: &str) -> Result<(), String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> Result<(), rquickjs::Error> {
                let f: Function = ctx.globals().get("__pf_fire_svc_message")?;
                f.call::<(&str, &str), ()>((payload, meta))?;
                Ok(())
            })
            .map_err(|e| e.to_string())
    }
}
/// 注入 JS 字符串字面量的最小转义（`\ " ` 与控制符；清单路径是本地明文，够用）。
/// nonceTag 派生（090-5 L3.3 多点校验：activate / render / onCommand 各带一次，
/// 插件 wasm 内按同一算法复算比对）：hex(sha256(proof ‖ 0x00 ‖ nonce))。
/// nonceTag 派生已上移 `crate::nonce_tag`（js / wasm 两路共用同一实现）。

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

    /// 测试宿主：私有目录内存态 + 动作入队（同 app 侧 ChatHost 形状）。
    struct TestHost {
        files: std::collections::HashMap<String, String>,
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
        fn dir(&mut self) -> String {
            "/tmp/pf".into()
        }
        fn fs_read(&mut self, name: &str) -> String {
            self.files.get(name).cloned().unwrap_or_default()
        }
        fn fs_write(&mut self, name: &str, data: &str) {
            self.files.insert(name.to_string(), data.to_string());
        }
        fn fs_list(&mut self) -> Vec<String> {
            self.files.keys().cloned().collect()
        }
        fn queue(&mut self, action: JsAction) {
            self.actions.push(action);
        }
    }

    /// 与插件仓 packages/quick-panel/main.js 同契约的最小插件源码（v2：内联树）。
    const PLUGIN: &str = r#"
const KEY = "pins.json";
const load = (ctx) => { try { const a = JSON.parse(ctx.fs.read(KEY) || "[]"); return Array.isArray(a) ? a : []; } catch { return []; } };
const save = (ctx, pins) => ctx.fs.write(KEY, JSON.stringify(pins));
export function activate(ctx) {
  ctx.onMenu("set-quick", ({ path }) => {
    const pins = load(ctx);
    if (pins.includes(path)) { ctx.toast("dup"); return; }
    save(ctx, [...pins, path]);
    ctx.toast("pinned");
  });
  ctx.onCommand("unpin", ({ path }) => {
    save(ctx, load(ctx).filter((p) => p !== path));
    ctx.toast("unpinned");
  });
}
export function render(ctx) {
  const pins = load(ctx);
  if (pins.length === 0)
    return { type: "text", value: { zh: "空" }, tone: "text.faint" };
  return { type: "column", pad: "SP4", gap: "SP2", children: pins.map((p) => ({
    type: "button", icon: ctx.fileIcon(p), label: p.split(/[\\/]/).pop(), tip: p,
    action: ctx.isExecutive(p) ? "run" : "edit", arg: p,
    menu: [ { id: "unpin", title: { zh: "移除快捷" } } ],
  })) };
}
"#;

    /// 契约 v3 面：fn_bar chrome / fire_command_env / act.openPopup /
    /// render_popup（无导出退回 render）。
    #[test]
    fn v3_fn_bar_command_env_popup() {
        let dir = std::env::temp_dir().join("pf-js-v3-test");
        std::fs::create_dir_all(&dir).unwrap();
        let entry = dir.join("main.js");
        std::fs::write(
            &entry,
            r#"
export function activate(ctx) {
  ctx.onCommand("bind", (env) => {
    ctx.fs.write("env.json", JSON.stringify(env));
  });
  ctx.onCommand("toggle-enabled", (env) => {
    ctx.fs.write("switch.json", JSON.stringify(env));
  });
  ctx.act.openPopup("qr");
}
export function fn_bar(ctx, env) {
  return {
    icon: "assets:demo.svg",
    tone: env.session && env.session.key === "s1" ? "accent" : "text.faint",
    tip: { zh: "示例插件" },
  };
}
export function render_popup(ctx) {
  return { type: "column", children: [
    { type: "switch", value: true, command: "toggle-enabled" },
  ] };
}
export function render(ctx) {
  return { type: "text", value: { zh: "旧面" } };
}
"#,
        )
        .unwrap();
        let host = Rc::new(RefCell::new(TestHost {
            files: Default::default(),
            actions: Vec::new(),
        }));
        let plugin = JsPlugin::load(&entry, "/tmp/pf", host.clone()).expect("load");
        plugin.activate().expect("activate");
        // act.openPopup → 动作入队
        assert_eq!(
            host.borrow().actions.clone(),
            vec![JsAction::OpenPopup("qr".into())]
        );
        // fn_bar chrome：env 带会话键 → 绿；换键 → 灰
        let green = plugin
            .fn_bar_json(r#"{"session":{"key":"s1"}}"#)
            .unwrap()
            .unwrap();
        assert!(
            green.contains("assets:demo.svg") && green.contains("accent"),
            "{green}"
        );
        let grey = plugin
            .fn_bar_json(r#"{"session":{"key":"s2"}}"#)
            .unwrap()
            .unwrap();
        assert!(grey.contains("text.faint"), "{grey}");
        // 命令 env 负载形态：switch 翻转 {value:bool} / bind {session:{key}}
        plugin
            .fire_command_env("toggle-enabled", r#"{"value":false}"#)
            .unwrap();
        assert_eq!(
            host.borrow_mut()
                .files
                .get("switch.json")
                .map(String::as_str),
            Some(r#"{"value":false}"#)
        );
        plugin
            .fire_command_env("bind", r#"{"session":{"key":"sA"}}"#)
            .unwrap();
        assert_eq!(
            host.borrow_mut().files.get("env.json").map(String::as_str),
            Some(r#"{"session":{"key":"sA"}}"#)
        );
        // 弹窗内容：render_popup 优先（不含旧面文案）
        let popup = plugin.render_popup_json().unwrap();
        assert!(
            popup.contains("switch") && !popup.contains("旧面"),
            "{popup}"
        );
        // 未导出 fn_bar 的插件 → None（负缓存语义由 app 侧管）
        let dir2 = std::env::temp_dir().join("pf-js-v3-nofnbar");
        std::fs::create_dir_all(&dir2).unwrap();
        let entry2 = dir2.join("main.js");
        std::fs::write(
            &entry2,
            "export function render(ctx) { return { type: \"text\", value: { zh: \"x\" } }; }",
        )
        .unwrap();
        let host2 = Rc::new(RefCell::new(TestHost {
            files: Default::default(),
            actions: Vec::new(),
        }));
        let p2 = JsPlugin::load(&entry2, "/tmp/pf", host2).expect("load");
        assert!(p2.fn_bar_json("{}").unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    #[test]
    fn host_proof_placeholder_and_nonce_tag() {
        // 未覆写 host_proof 的宿主（TestHost 同 fork 自建款）：占位符 fail-closed
        let dir = std::env::temp_dir().join("pf-js-proof-test");
        std::fs::create_dir_all(&dir).unwrap();
        let entry = dir.join("main.js");
        std::fs::write(
            &entry,
            r#"export function render(ctx) {
  const p = ctx.hostProof("n1");
  return { type: "text", value: p.kid + "|" + p.proof + "|" + p.nonceTag };
}"#,
        )
        .unwrap();
        let host = Rc::new(RefCell::new(TestHost {
            files: Default::default(),
            actions: Vec::new(),
        }));
        let plugin = JsPlugin::load(&entry, "/tmp/pf", host).expect("load");
        let v: serde_json::Value = serde_json::from_str(&plugin.render_json().unwrap()).unwrap();
        let parts: Vec<&str> = v["value"].as_str().unwrap().split('|').collect();
        assert_eq!(parts[0], ""); // 未覆写 → kid 空
        assert_eq!(parts[1], "PROOF_PLACEHOLDER"); // fail-closed：验签必失败
        assert_eq!(parts[2], nonce_tag("PROOF_PLACEHOLDER", "n1")); // 派生一致
        assert_ne!(nonce_tag("P", "n1"), nonce_tag("P", "n2")); // 不同 nonce 不同 tag
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn broadcast_reaches_registered_handler() {
        // 契约 v2.4 broadcast_events：onBroadcast 注册 → fire_broadcast 负载直达
        // → 插件自存状态 → render 可读（差分/派发时序在宿主 broadcast.rs 测）
        let dir = std::env::temp_dir().join("pf-js-broadcast-test");
        std::fs::create_dir_all(&dir).unwrap();
        let entry = dir.join("main.js");
        std::fs::write(
            &entry,
            r#"
let last = null;
export function activate(ctx) {
  ctx.onBroadcast("inputpanel", (payload) => { last = payload; });
}
export function render(ctx) {
  if (!last) return { type: "text", value: "no-broadcast" };
  return { type: "text",
           value: (last.project || "-") + "|" + (last.model ? last.model.provider + "/" + last.model.id : "-") };
}
"#,
        )
        .unwrap();
        let host = Rc::new(RefCell::new(TestHost {
            files: Default::default(),
            actions: Vec::new(),
        }));
        let plugin = JsPlugin::load(&entry, "/tmp/pf", host).expect("load");
        plugin.activate().expect("activate");

        // 未广播 → 无负载
        let v: serde_json::Value = serde_json::from_str(&plugin.render_json().unwrap()).unwrap();
        assert_eq!(v["value"], "no-broadcast");

        // 广播带项目无模型 → 负载直达（可空字段走 null）
        plugin
            .fire_broadcast("inputpanel", r#"{"project":"D:/work","model":null}"#)
            .expect("fire");
        let v: serde_json::Value = serde_json::from_str(&plugin.render_json().unwrap()).unwrap();
        assert_eq!(v["value"], "D:/work|-");

        // 广播项目+模型齐 → 负载更新
        plugin
            .fire_broadcast(
                "inputpanel",
                r#"{"project":"D:/work","model":{"provider":"prov","id":"m1"}}"#,
            )
            .expect("fire");
        let v: serde_json::Value = serde_json::from_str(&plugin.render_json().unwrap()).unwrap();
        assert_eq!(v["value"], "D:/work|prov/m1");

        // 未注册的事件名 → no-op 不报错
        plugin.fire_broadcast("other", r#"{}"#).expect("noop");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sessions_newdraft_contract_namespace() {
        // 契约 v2.4 会话槽：ctx.sessions.newDraft（嵌套面）。扁平 ctx.newDraft
        // 仅旧兼容——插件按契约写法必须可达。
        let dir = std::env::temp_dir().join("pf-js-sessions-test");
        std::fs::create_dir_all(&dir).unwrap();
        let entry = dir.join("main.js");
        std::fs::write(
            &entry,
            r#"
let got = null;
export function activate(ctx) {
  ctx.sessions.newDraft("pf-demo-", "D:/proj/demo");
  got = typeof ctx.sessions.newDraft;
}
export function render(ctx) { return { type: "text", value: got }; }
"#,
        )
        .unwrap();
        let host = Rc::new(RefCell::new(TestHost {
            files: Default::default(),
            actions: Vec::new(),
        }));
        let plugin = JsPlugin::load(&entry, "/tmp/pf", host.clone()).expect("load");
        plugin.activate().expect("activate");
        let drafts: Vec<_> = host
            .borrow()
            .actions
            .iter()
            .filter_map(|a| match a {
                JsAction::NewDraft { prefix, cwd } => Some((prefix.clone(), cwd.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            drafts,
            vec![("pf-demo-".into(), "D:/proj/demo".into())],
            "ctx.sessions.newDraft 必须到达动作队列"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn activate_menu_render_and_command() {
        let dir = std::env::temp_dir().join("pf-js-v2-test");
        std::fs::create_dir_all(&dir).unwrap();
        let entry = dir.join("main.js");
        std::fs::write(&entry, PLUGIN).unwrap();
        let host = Rc::new(RefCell::new(TestHost {
            files: Default::default(),
            actions: Vec::new(),
        }));
        let plugin = JsPlugin::load(&entry, "/tmp/pf", host.clone()).expect("load");
        plugin.activate().expect("activate");

        // 空面板
        let v: serde_json::Value = serde_json::from_str(&plugin.render_json().unwrap()).unwrap();
        assert_eq!(v["type"], "text");

        // 右键设为快捷 ×3（含重复）
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
        // 只存路径，派生值不落盘
        let stored = host.borrow().files.get("pins.json").cloned().unwrap();
        assert_eq!(stored, r#"["D:\\p\\build.ps1","D:\\p\\README.md"]"#);

        // 渲染：值内联 + 动作显式
        plugin.fire_panel_open().expect("panel open");
        let v: serde_json::Value = serde_json::from_str(&plugin.render_json().unwrap()).unwrap();
        assert_eq!(v["type"], "column");
        assert_eq!(v["children"].as_array().unwrap().len(), 2);
        assert_eq!(v["children"][0]["action"], "run");
        assert_eq!(v["children"][0]["label"], "build.ps1");
        assert_eq!(v["children"][1]["action"], "edit");
        assert!(v.get("rows").is_none());

        // onCommand：插件改自己的数据
        plugin
            .fire_command("unpin", r"D:\p\build.ps1")
            .expect("cmd");
        let v: serde_json::Value = serde_json::from_str(&plugin.render_json().unwrap()).unwrap();
        assert_eq!(v["children"].as_array().unwrap().len(), 1);
        assert_eq!(v["children"][0]["label"], "README.md");

        // 异常插件：render 抛错 → Err（app 侧 toast + 会话级禁用）
        let bad = std::env::temp_dir().join("pf-js-v2-bad.js");
        std::fs::write(&bad, "export function render(ctx) { no_such_fn(); }").unwrap();
        let plugin = JsPlugin::load(&bad, "/tmp/pf", host.clone()).expect("load bad");
        assert!(plugin.render_json().is_err());
        let _ = std::fs::remove_file(&bad);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
