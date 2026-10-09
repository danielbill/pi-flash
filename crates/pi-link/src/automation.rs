//! pi-flash 应用内 UI 自动化协议（NDJSON over TCP 127.0.0.1）。
//!
//! 目的：agent 不抢真实屏幕/鼠标——app 内嵌一个小服务，外界用「指令」拿
//! 数据化界面（JSON 快照）、执行界面操作（方法直调 / 合成按键）。本模块只
//! 管三件事，服务端逻辑在 crates/app/src/automation：
//! - 线记录编解码（对齐 protocol.rs 的手写 to_record/parse 风格）
//! - method 常量（v1 op 清单的唯一事实源）
//! - 实例发现文件（`<配置目录>/automation/<pid>.json`：多实例时 CLI 靠它
//!   找到端口 + token；进程死亡留下死文件，由启动方/CLI 探活清理）
//!
//! 连接时序：服务端先发 `hello`，客户端回 `auth`，服务端 `auth_ok` 后
//! 开始接受 `Request`（`{"id","method","params"}`），逐条回 `Response`
//! （`{"id","ok",...}`）。一行一记录；坏行忽略（读方 continue 不断连，
//! 同 client.rs 纪律）。

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// 线协议版本；不兼容改动 +1，客户端按 hello 协商。
pub const PROTO_VERSION: u32 = 1;

/// v1 op 清单。CLI 与服务端共用，防止两边字符串漂移。
pub mod method {
    pub const APP_INFO: &str = "app.info";
    pub const UI_SNAPSHOT: &str = "ui.snapshot";
    pub const APP_QUIT: &str = "app.quit";
    pub const SESSION_NEW: &str = "session.new";
    pub const SESSION_OPEN: &str = "session.open";
    pub const SESSION_SWITCH: &str = "session.switch";
    pub const SESSION_DELETE: &str = "session.delete";
    pub const SESSION_SEND: &str = "session.send";
    pub const SESSION_STEER: &str = "session.steer";
    pub const SESSION_FOLLOWUP: &str = "session.followup";
    pub const SESSION_ABORT: &str = "session.abort";
    pub const COMPOSER_SET_TEXT: &str = "composer.set_text";
    pub const PANEL_DOCK: &str = "panel.dock";
    pub const CONTENT_VIEW: &str = "content.view";
    pub const FILE_OPEN: &str = "file.open";
    /// 023 fileView：保存 / 关闭 / 直调编辑器值（模拟用户编辑，走 setter
    /// 同 composer 惯例）/ md 渲染↔源码切换
    pub const FILE_SAVE: &str = "file.save";
    pub const FILE_CLOSE: &str = "file.close";
    pub const FILE_SET_TEXT: &str = "file.set_text";
    pub const FILE_VIEW_MODE: &str = "file.view_mode";
    pub const FILES_TOGGLE_DIR: &str = "files.toggle_dir";
    pub const GIT_STAGE: &str = "git.stage";
    pub const GIT_UNSTAGE: &str = "git.unstage";
    pub const GIT_COMMIT: &str = "git.commit";
    pub const GIT_PUSH: &str = "git.push";
    pub const GIT_REFRESH: &str = "git.refresh";
    pub const GIT_SET_TAB: &str = "git.set_tab";
    pub const SETTINGS_OPEN: &str = "settings.open";
    pub const SETTINGS_CLOSE: &str = "settings.close";
    /// 040 扩展页：直调安装（UI 测试用；source 支持整条 `pi install …`）
    pub const SETTINGS_INSTALL_EXT: &str = "settings.install_ext";
    /// 040 扩展页：弹卸载确认浮层 / 应答确认（UI 测试用）
    pub const SETTINGS_REMOVE_EXT: &str = "settings.remove_ext";
    pub const SETTINGS_PKG_REMOVE_CONFIRM: &str = "settings.pkg_remove_confirm";
    pub const THEME_SET: &str = "theme.set";
    pub const LANG_SET: &str = "lang.set";
    pub const DIALOG_CLOSE: &str = "dialog.close";
    /// 060 远程控制：开扫码弹窗并发起取码（UI 测试用；内容现读 remote.qr）
    /// 模型选择弹窗：开（与 inputpanel「模型 ∨」同一个构造路径；UI 测试用，
    /// 列表内容读 dialogs 面的 `model_rows`）
    pub const MODEL_PICKER_OPEN: &str = "model.picker_open";
    pub const WX_QR_OPEN: &str = "wx.qr_open";
    pub const PROJECT_SWITCH: &str = "project.switch";
    /// 004 打开项目菜单（psp icon / 012 操作栏同源入口；UI 测试用）
    pub const PROJECT_PICKER_OPEN: &str = "project.picker_open";
    pub const INPUT_KEYS: &str = "input.keys";
    pub const INPUT_FOCUS: &str = "input.focus";
    /// 工具预设直调（UI 测试用：full 档的精确集 / 自定义档的重绑都走这条）
    pub const SESSION_TOOLS_PRESET: &str = "session.tools_preset";
    /// 031 插件选择面板：打开 / 勾选 / 确认（UI 测试用）
    pub const PLUGIN_PICKER_OPEN: &str = "plugin_picker.open";
    pub const PLUGIN_PICKER_TOGGLE: &str = "plugin_picker.toggle";
    pub const PLUGIN_PICKER_CANCEL: &str = "plugin_picker.cancel";
    /// 043 MCP 勾选面板：打开 / 勾选 / 收起（UI 测试用）
    pub const MCP_PICKER_OPEN: &str = "mcp_picker.open";
    pub const MCP_PICKER_TOGGLE: &str = "mcp_picker.toggle";
    pub const MCP_PICKER_CANCEL: &str = "mcp_picker.cancel";
    /// 043 设置页 MCP 表单：填字段并走真实 mcp_save_submit 保存链路
    pub const SETTINGS_MCP_SAVE: &str = "settings.mcp_save";
    /// 截屏：窗口最近一帧渲染回读为 PNG（进程内 D3D11 staging readback，
    /// 非 OS 抢屏；窗口被遮挡/最小化也能截）。params: {path?}，缺省写
    /// <配置目录>/automation/shots/。返回 {path,width,height,bytes}。
    pub const UI_SCREENSHOT: &str = "ui.screenshot";
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationError {
    pub code: String,
    pub message: String,
}

impl AutomationError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.to_string(), message: message.into() }
    }
}

/// 一行线记录。Unknown 兜底未知 type（同 protocol.rs：不静默丢）。
#[derive(Debug, Clone, PartialEq)]
pub enum AutomationRecord {
    Hello { proto: u32, app: String, pid: u32, version: String },
    Auth { token: String },
    AuthOk,
    AuthError { code: String, message: String },
    Request { id: String, method: String, params: Value },
    Response { id: String, ok: bool, result: Option<Value>, error: Option<AutomationError> },
    Unknown(Value),
}

pub fn hello_record(proto: u32, app: &str, pid: u32, version: &str) -> Value {
    json!({"type": "hello", "proto": proto, "app": app, "pid": pid, "version": version})
}

pub fn auth_record(token: &str) -> Value {
    json!({"type": "auth", "token": token})
}

pub fn auth_ok_record() -> Value {
    json!({"type": "auth_ok"})
}

pub fn auth_error_record(code: &str, message: &str) -> Value {
    json!({"type": "auth_error", "error": {"code": code, "message": message}})
}

pub fn request_record(id: &str, method: &str, params: &Value) -> Value {
    json!({"id": id, "method": method, "params": params})
}

pub fn response_ok_record(id: &str, result: Value) -> Value {
    json!({"id": id, "ok": true, "result": result})
}

pub fn response_error_record(id: &str, err: &AutomationError) -> Value {
    json!({"id": id, "ok": false, "error": {"code": err.code, "message": err.message}})
}

fn parse_error(v: &Value) -> Option<AutomationError> {
    let e = v.get("error")?;
    Some(AutomationError {
        code: e.get("code").and_then(Value::as_str).unwrap_or("internal").to_string(),
        message: e.get("message").and_then(Value::as_str).unwrap_or("").to_string(),
    })
}

/// 空行/非 JSON → None（调用方忽略该行，同 protocol::parse_line 纪律）。
pub fn parse_line(line: &str) -> Option<AutomationRecord> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    let ty = v.get("type").and_then(Value::as_str);
    match ty {
        Some("hello") => Some(AutomationRecord::Hello {
            proto: v.get("proto").and_then(Value::as_u64).unwrap_or(0) as u32,
            app: v.get("app").and_then(Value::as_str).unwrap_or("").to_string(),
            pid: v.get("pid").and_then(Value::as_u64).unwrap_or(0) as u32,
            version: v.get("version").and_then(Value::as_str).unwrap_or("").to_string(),
        }),
        Some("auth") => Some(AutomationRecord::Auth {
            token: v.get("token").and_then(Value::as_str).unwrap_or("").to_string(),
        }),
        Some("auth_ok") => Some(AutomationRecord::AuthOk),
        Some("auth_error") => {
            let e = parse_error(&v)?;
            Some(AutomationRecord::AuthError { code: e.code, message: e.message })
        }
        // 请求/响应不带 type 字段：靠 method / ok 判别（和 pi 线上习惯一致，
        // 记录本体即身份）
        None => {
            if let (Some(id), Some(m)) = (
                v.get("id").and_then(Value::as_str),
                v.get("method").and_then(Value::as_str),
            ) {
                Some(AutomationRecord::Request {
                    id: id.to_string(),
                    method: m.to_string(),
                    params: v.get("params").cloned().unwrap_or_else(|| json!({})),
                })
            } else if let Some(id) = v.get("id").and_then(Value::as_str) {
                let ok = v.get("ok").and_then(Value::as_bool)?;
                Some(AutomationRecord::Response {
                    id: id.to_string(),
                    ok,
                    result: v.get("result").cloned().filter(|_| ok),
                    error: parse_error(&v).filter(|_| !ok),
                })
            } else {
                Some(AutomationRecord::Unknown(v))
            }
        }
        Some(_) => Some(AutomationRecord::Unknown(v)),
    }
}

// ---------------------------------------------------------------------------
// 实例发现文件
// ---------------------------------------------------------------------------

/// 一个活过/活着的 pi-flash 实例登记。
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceInfo {
    pub pid: u32,
    pub port: u16,
    pub token: String,
    /// epoch ms，多实例时 CLI 选最新。
    pub started_at_ms: u64,
}

pub fn instances_dir_in(base: &Path) -> PathBuf {
    base.join("automation")
}

pub fn instances_dir() -> Option<PathBuf> {
    crate::paths::dir().map(|d| instances_dir_in(&d))
}

fn instance_file_in(base: &Path, pid: u32) -> PathBuf {
    instances_dir_in(base).join(format!("{pid}.json"))
}

pub fn write_instance_file_in(base: &Path, info: &InstanceInfo) -> std::io::Result<PathBuf> {
    let dir = instances_dir_in(base);
    std::fs::create_dir_all(&dir)?;
    let path = instance_file_in(base, info.pid);
    std::fs::write(
        &path,
        serde_json::to_string(&json!({
            "pid": info.pid,
            "port": info.port,
            "token": info.token,
            "started_at_ms": info.started_at_ms,
        }))
        .expect("instance info serializes"),
    )?;
    Ok(path)
}

pub fn write_instance_file(info: &InstanceInfo) -> Option<PathBuf> {
    let base = crate::paths::ensure_dir()?;
    write_instance_file_in(&base, info).ok()
}

pub fn remove_instance_file(pid: u32) {
    if let Some(base) = crate::paths::dir() {
        let _ = std::fs::remove_file(instance_file_in(&base, pid));
    }
}

/// 读出全部登记（含解析失败的跳过）；按 started_at_ms 新的在前。
pub fn read_instances_in(base: &Path) -> Vec<(InstanceInfo, PathBuf)> {
    let dir = instances_dir_in(base);
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
        let (Some(pid), Some(port)) = (
            v.get("pid").and_then(Value::as_u64),
            v.get("port").and_then(Value::as_u64),
        ) else {
            continue;
        };
        if port == 0 || port > u16::MAX as u64 {
            continue;
        }
        out.push((
            InstanceInfo {
                pid: pid as u32,
                port: port as u16,
                token: v.get("token").and_then(Value::as_str).unwrap_or("").to_string(),
                started_at_ms: v.get("started_at_ms").and_then(Value::as_u64).unwrap_or(0),
            },
            path,
        ));
    }
    out.sort_by(|a, b| b.0.started_at_ms.cmp(&a.0.started_at_ms));
    out
}

pub fn read_instances() -> Vec<(InstanceInfo, PathBuf)> {
    match crate::paths::dir() {
        Some(base) => read_instances_in(&base),
        None => Vec::new(),
    }
}

/// 探活判死：**只有明确没人监听（ConnectionRefused）才判死**，其余失败
/// （超时、Winsock 起不来、防火墙…）一律保守认为活着。
///
/// 为什么这么严：token 只存在登记文件里，删了登记 = 实例永久失联（只能
/// 杀进程重启）。实测踩坑：探活方脚本环境缺 `SystemRoot` 时 Winsock 报
/// 错，实例其实活得好好的——「顺手清理」误杀了登记。误留死文件的代价
/// （discover 跳过它试下一个）远小于误删活登记。
pub fn port_is_definitely_dead(addr: &str) -> bool {
    let Ok(a) = addr.parse() else { return true }; // 地址本身解析不了 = 登记坏
    match std::net::TcpStream::connect_timeout(&a, std::time::Duration::from_millis(300)) {
        Ok(_) => false,
        Err(e) => e.kind() == std::io::ErrorKind::ConnectionRefused,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse_value;

    fn parse(line: &str) -> Option<AutomationRecord> {
        // 走和线上一致的深 JSON 安全解析器
        let v: Value = parse_value(line).ok()?;
        parse_line(&serde_json::to_string(&v).ok()?)
    }

    #[test]
    fn hello_auth_roundtrip() {
        let h = hello_record(PROTO_VERSION, "pi-flash", 4812, "0.1.1");
        assert_eq!(
            parse(&h.to_string()),
            Some(AutomationRecord::Hello {
                proto: 1,
                app: "pi-flash".into(),
                pid: 4812,
                version: "0.1.1".into(),
            })
        );
        let a = auth_record("tok");
        assert_eq!(parse(&a.to_string()), Some(AutomationRecord::Auth { token: "tok".into() }));
        assert_eq!(parse(&auth_ok_record().to_string()), Some(AutomationRecord::AuthOk));
        let ae = auth_error_record("unauthorized", "bad token");
        assert_eq!(
            parse(&ae.to_string()),
            Some(AutomationRecord::AuthError {
                code: "unauthorized".into(),
                message: "bad token".into(),
            })
        );
    }

    #[test]
    fn request_response_roundtrip() {
        let req = request_record("cli-1", method::UI_SNAPSHOT, &json!({"surface": "files"}));
        assert_eq!(
            parse(&req.to_string()),
            Some(AutomationRecord::Request {
                id: "cli-1".into(),
                method: "ui.snapshot".into(),
                params: json!({"surface": "files"}),
            })
        );
        // params 缺省 = {}
        let bare = json!({"id": "r2", "method": "app.info"});
        assert_eq!(
            parse(&bare.to_string()),
            Some(AutomationRecord::Request {
                id: "r2".into(),
                method: "app.info".into(),
                params: json!({}),
            })
        );
        let ok = response_ok_record("cli-1", json!({"cwd": "/w"}));
        assert_eq!(
            parse(&ok.to_string()),
            Some(AutomationRecord::Response {
                id: "cli-1".into(),
                ok: true,
                result: Some(json!({"cwd": "/w"})),
                error: None,
            })
        );
        let err = response_error_record("cli-1", &AutomationError::new("unknown_method", "nope"));
        assert_eq!(
            parse(&err.to_string()),
            Some(AutomationRecord::Response {
                id: "cli-1".into(),
                ok: false,
                result: None,
                error: Some(AutomationError { code: "unknown_method".into(), message: "nope".into() }),
            })
        );
    }

    #[test]
    fn garbage_lines_are_ignored() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("   \n"), None);
        assert_eq!(parse("not json at all"), None);
        assert_eq!(parse("\u{1b}[2Jclear"), None);
    }

    #[test]
    fn unknown_records_are_kept() {
        // 未知 type → Unknown（不静默丢，排查线协议漂移靠它）
        let v = json!({"type": "from_the_future", "x": 1});
        assert_eq!(parse(&v.to_string()), Some(AutomationRecord::Unknown(v)));
        // 无 id 无 type 的 JSON 也归 Unknown
        let v = json!({"foo": 1});
        assert_eq!(parse(&v.to_string()), Some(AutomationRecord::Unknown(v)));
    }

    #[test]
    fn instance_file_roundtrip_and_order() {
        let base = std::env::temp_dir().join(format!("pif-auto-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let a = InstanceInfo { pid: 11, port: 5001, token: "ta".into(), started_at_ms: 100 };
        let b = InstanceInfo { pid: 22, port: 5002, token: "tb".into(), started_at_ms: 200 };
        write_instance_file_in(&base, &a).unwrap();
        write_instance_file_in(&base, &b).unwrap();
        let got = read_instances_in(&base);
        assert_eq!(got.len(), 2);
        // 新的在前
        assert_eq!(got[0].0, b);
        assert_eq!(got[1].0, a);
        // 坏文件跳过不 panic
        std::fs::write(instance_file_in(&base, 33), "garbage{").unwrap();
        assert_eq!(read_instances_in(&base).len(), 2);
        let _ = std::fs::remove_dir_all(&base);
    }
}
