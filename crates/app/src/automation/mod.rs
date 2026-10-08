//! 应用内 UI 自动化服务（pi-flash-2kq）：127.0.0.1 上的 NDJSON 指令口。
//!
//! 目的：agent 调试不抢真实屏幕/鼠标——外界用指令拿数据化界面（快照读
//! Chat/SessionRuntime 字段）、执行界面操作（直调 Chat 方法 / 合成按键走
//! 真键位表）。协议与实例发现文件在 pi_link::automation；op 分发在
//! handlers，快照在 snapshot。
//!
//! 线程形制（照 house 模式）：accept 线程 + 每连接一个读线程（握手后把
//! 请求行推 unbounded channel）+ 每连接一个写线程；`cx.spawn` 泵把请求
//! 逐条搬回主线程执行（同 session/runtime.rs attach_pump）。op 在主线程
//! catch_unwind 内跑——自动化触发的 panic 不许带崩 app。
//!
//! 开关：`PI_FLASH_AUTOMATION=<port|auto|1>`，缺省关闭。绑定只落
//! 127.0.0.1，token 随实例文件 `<配置目录>/automation/<pid>.json` 分发。

mod handlers;
mod screenshot;
mod snapshot;

use futures::StreamExt;
use gpui::{AnyWindowHandle, App, AsyncApp, WeakEntity};
use pi_link::automation::{self, AutomationError, AutomationRecord, InstanceInfo};
use std::io::BufRead;
use std::sync::Arc;

use crate::Chat;

/// 连接上送进主线程的请求：（该连接的写通道, 原始行）。
type ReqTx = futures::channel::mpsc::UnboundedSender<(std::sync::mpsc::Sender<String>, String)>;

/// 启动服务。spec = PI_FLASH_AUTOMATION 的值："auto"/"1"/空 → 随机端口。
pub fn start(cx: &mut App, window: AnyWindowHandle, chat: WeakEntity<Chat>, spec: &str) -> Result<(), String> {
    let port: u16 = match spec.trim() {
        "" | "1" | "auto" => 0,
        s => s.parse().map_err(|_| format!("PI_FLASH_AUTOMATION 端口无效: {s:?}"))?,
    };
    prune_stale_instances();
    let listener =
        std::net::TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("绑定 127.0.0.1:{port} 失败: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token = Arc::new(make_token());
    let info = InstanceInfo {
        pid: std::process::id(),
        port,
        token: token.as_ref().clone(),
        started_at_ms: now_ms(),
    };
    let file = automation::write_instance_file(&info);
    let (tx, mut rx) = futures::channel::mpsc::unbounded::<(std::sync::mpsc::Sender<String>, String)>();
    std::thread::Builder::new()
        .name("pif-auto-accept".into())
        .spawn(move || {
            for conn in listener.incoming() {
                match conn {
                    Ok(s) => handle_conn(s, token.clone(), tx.clone()),
                    Err(_) => break,
                }
            }
        })
        .map_err(|e| format!("起 accept 线程失败: {e}"))?;
    cx.spawn(async move |cx: &mut AsyncApp| {
        while let Some((wtx, line)) = rx.next().await {
            // 泵不许因一次窗口更新失败而死：失败原样写回错误响应
            let _ = cx.update(|cx| handle_line(window, &chat, &wtx, cx, &line));
        }
    })
    .detach();
    // token 打进启动日志：登记文件万一丢失/被误清，--addr/--token 还能救
    eprintln!(
        "pi-flash automation: 127.0.0.1:{port} pid={} token={} file={}",
        info.pid,
        info.token,
        file.as_ref().map(|p| p.display().to_string()).unwrap_or_default()
    );
    Ok(())
}

/// 主线程处理一行：解析 → catch_unwind 包住分发 → 回写响应。
fn handle_line(
    window: AnyWindowHandle,
    chat: &WeakEntity<Chat>,
    wtx: &std::sync::mpsc::Sender<String>,
    cx: &mut App,
    line: &str,
) {
    let Some(AutomationRecord::Request { id, method, params }) = automation::parse_line(line) else {
        return;
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        window.update(cx, |_view, window, cx| {
            chat.update(cx, |chat, cx| handlers::dispatch(chat, window, cx, &method, &params))
        })
    }));
    let response = match outcome {
        // 层级（编译器探针确认）：catch_unwind(Box) → window.update(anyhow)
        // → chat.update(anyhow) → dispatch 的 Result<Value, (code, message)>
        Ok(Ok(Ok(Ok(result)))) => automation::response_ok_record(&id, result),
        Ok(Ok(Ok(Err((code, message))))) => {
            automation::response_error_record(&id, &AutomationError { code, message })
        }
        Ok(Ok(Err(e))) | Ok(Err(e)) => automation::response_error_record(
            &id,
            &AutomationError::new("unavailable", format!("窗口或会话已关闭: {e}")),
        ),
        Err(_) => automation::response_error_record(
            &id,
            &AutomationError::new("internal", "op 执行 panic（已拦下，app 不受影响）"),
        ),
    };
    let _ = wtx.send(response.to_string());
}

/// 一个连接 = 读线程本体（写走独立线程）。hello → auth → auth_ok 后进
/// 请求循环；认证失败回错误后断开。
fn handle_conn(stream: std::net::TcpStream, token: Arc<String>, tx: ReqTx) {
    let write_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let (wtx, wrx) = std::sync::mpsc::channel::<String>();
    if std::thread::Builder::new()
        .name("pif-auto-writer".into())
        .spawn(move || {
            use std::io::Write;
            let mut w = write_stream;
            for line in wrx {
                if w.write_all(line.as_bytes()).is_err() || w.write_all(b"\n").is_err() {
                    break;
                }
            }
        })
        .is_err()
    {
        return;
    }
    let _ = wtx.send(automation::hello_record(
        automation::PROTO_VERSION,
        "pi-flash",
        std::process::id(),
        env!("CARGO_PKG_VERSION"),
    )
    .to_string());
    let mut reader = std::io::BufReader::new(stream);
    let ok = (|| {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        match automation::parse_line(&line) {
            Some(AutomationRecord::Auth { token: t }) if t == *token => Some(()),
            _ => None,
        }
    })()
    .is_some();
    if !ok {
        let _ = wtx.send(automation::auth_error_record("unauthorized", "token 不匹配").to_string());
        return;
    }
    let _ = wtx.send(automation::auth_ok_record().to_string());
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if tx.unbounded_send((wtx.clone(), line)).is_err() {
                    break; // 主线程泵已死（app 退出中）
                }
            }
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// token：时间 + pid + 栈地址熵的两次 DefaultHasher（本地回环防误连用，
/// 非安全边界）。
fn make_token() -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    std::time::SystemTime::now().hash(&mut h);
    std::process::id().hash(&mut h);
    let a = h.finish();
    let mut h2 = std::collections::hash_map::DefaultHasher::new();
    a.hash(&mut h2);
    (&a as *const u64 as usize).hash(&mut h2);
    format!("{a:016x}{:016x}", h2.finish())
}

/// 清掉明确死亡的登记（进程崩溃残留）。判死规则同 CLI：只有明确拒绝
/// 连接（ConnectionRefused）才删——其余失败保守保留（token 只在登记里，
/// 误删 = 实例失联）。
fn prune_stale_instances() {
    for (info, file) in automation::read_instances() {
        if automation::port_is_definitely_dead(&format!("127.0.0.1:{}", info.port)) {
            let _ = std::fs::remove_file(&file);
        }
    }
}
