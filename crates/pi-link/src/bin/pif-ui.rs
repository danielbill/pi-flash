//! pif-ui —— pi-flash UI 自动化服务的 CLI 驱动器（agent 调试用，同源
//! token-count 先例：pi-link 下的调试 bin）。
//!
//! 用法（先启动带自动化服务的 app：PI_FLASH_AUTOMATION=auto）：
//!   pif-ui list                       # 列出本机实例
//!   pif-ui info                       # app 级信息
//!   pif-ui snapshot [surface]         # 数据化界面（app/sessions/session/
//!                                     # composer/files/git/settings/dialogs）
//!   pif-ui exec <method> [json参数]    # 任意 op，如 exec session.send
//!   pif-ui keys "ctrl-s"              # 合成按键（走真实键位表）
//!   pif-ui type <text>                # composer.set_text 的糖
//!   pif-ui wait --path a.b --eq v [--timeout 30]   # 轮询快照直到相等
//!
//! 实例发现：`~/.pi-flash/automation/<pid>.json`（最新者优先）；可用
//! `--pid N` 指定实例、`--addr 127.0.0.1:PORT --token TOK` 直连、或环境
//! 变量 `PI_FLASH_AUTOMATION_FILE` 指向实例文件。

use pi_link::automation::{
    self, method, AutomationError, AutomationRecord, InstanceInfo, PROTO_VERSION,
};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const USAGE: &str = "\
pif-ui — pi-flash UI 自动化 CLI

用法: pif-ui [全局选项] <命令> [参数]   （全局选项必须在命令之前）

命令:
  list                      列出本机 pi-flash 自动化实例
  info                      app 级信息
  snapshot [surface]        数据化界面快照（app/sessions/session/composer/
                            files/git/settings/dialogs，缺省=全部概要）
  exec <method> [json]      执行 op（参数为 JSON 对象）
  keys <组合键>              合成按键，如 \"ctrl-s\"、\"escape\"
  type <text>               设置 composer 文本
  wait --path <a.b.c> --eq <值> [--timeout 秒] [--interval 毫秒]
                            轮询 ui.snapshot 直到指定路径的值相等（成功只打印
                            命中值；--timeout 为 wait 自己的等待上限）

全局选项:
  --pid <N>                 指定实例（缺省选最新活着的一个）
  --addr <HOST:PORT>        直连地址（配合 --token，跳过实例发现）
  --token <TOK>             直连 token
  --timeout <秒>            单次请求读超时（缺省 15）
  -h | help                 本帮助";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "-h" || args[0] == "help" || args[0] == "--help" {
        println!("{USAGE}");
        std::process::exit(if args.is_empty() { 2 } else { 0 });
    }

    // 全局旗标只认子命令**之前**的段（此后 --timeout 等归子命令自己）
    let mut pid: Option<u32> = None;
    let mut addr: Option<String> = None;
    let mut token: Option<String> = None;
    let mut timeout = 15u64;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--pid" => {
                pid = args.get(i + 1).and_then(|v| v.parse().ok());
                i += 2;
            }
            "--addr" => {
                addr = args.get(i + 1).cloned();
                i += 2;
            }
            "--token" => {
                token = args.get(i + 1).cloned();
                i += 2;
            }
            "--timeout" => {
                timeout = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(15);
                i += 2;
            }
            _ => break,
        }
    }
    let Some(cmd) = args.get(i).cloned() else {
        println!("{USAGE}");
        std::process::exit(2);
    };
    let cargs = &args[i + 1..];

    let run = || -> Result<(), String> {
        match cmd.as_str() {
            "list" => cmd_list(),
            "info" => with_conn(pid, addr, token, timeout, |c| {
                c.call(method::APP_INFO, &json!({})).map(print_json)
            }),
            "snapshot" => {
                let surface = cargs.first().cloned().unwrap_or_default();
                with_conn(pid, addr, token, timeout, |c| {
                    let params = if surface.is_empty() {
                        json!({})
                    } else {
                        json!({"surface": surface})
                    };
                    c.call(method::UI_SNAPSHOT, &params).map(print_json)
                })
            }
            "exec" => {
                let m = cargs.first().ok_or("exec 需要 method 参数")?;
                let params: Value = match cargs.get(1) {
                    Some(s) => serde_json::from_str(s)
                        .map_err(|e| format!("参数不是合法 JSON: {e}"))?,
                    None => json!({}),
                };
                with_conn(pid, addr, token, timeout, |c| c.call(m, &params).map(print_json))
            }
            "keys" => {
                let keys = cargs.first().ok_or("keys 需要组合键参数，如 \"ctrl-s\"")?;
                with_conn(pid, addr, token, timeout, |c| {
                    c.call(method::INPUT_KEYS, &json!({"keys": keys})).map(print_json)
                })
            }
            "type" => {
                let text = cargs.join(" ");
                with_conn(pid, addr, token, timeout, |c| {
                    c.call(method::COMPOSER_SET_TEXT, &json!({"text": text})).map(print_json)
                })
            }
            "wait" => cmd_wait(pid, addr, token, timeout, cargs),
            _ => Err(format!("未知命令: {cmd}\n{USAGE}")),
        }
    };
    if let Err(e) = run() {
        eprintln!("pif-ui: {e}");
        std::process::exit(1);
    }
}

fn print_json(v: Value) {
    // EPIPE（下游 head 关管道）不算错，静默退出
    let _ = writeln!(std::io::stdout(), "{}", serde_json::to_string_pretty(&v).unwrap_or_default());
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// 一条已握手的连接；wait 复用同一条轮询。
struct Conn {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
}

impl Conn {
    fn write_line(&mut self, line: &str) -> Result<(), String> {
        self.stream
            .write_all(format!("{line}\n").as_bytes())
            .map_err(|e| format!("写连接失败: {e}"))
    }

    /// 读到指定 id 的 Response 为止（跳过服务端可能推送的其它记录）。
    fn read_response(&mut self, id: &str, timeout: Duration) -> Result<Value, String> {
        self.stream.set_read_timeout(Some(timeout)).map_err(|e| e.to_string())?;
        loop {
            let mut line = String::new();
            let n = self
                .reader
                .read_line(&mut line)
                .map_err(|e| format!("读连接失败: {e}"))?;
            if n == 0 {
                return Err("连接已关闭（app 可能退出了）".into());
            }
            match automation::parse_line(&line) {
                Some(AutomationRecord::Response { id: rid, ok, result, error }) if rid == id => {
                    return if ok {
                        Ok(result.unwrap_or_else(|| json!({})))
                    } else {
                        let e = error.unwrap_or(AutomationError::new("internal", ""));
                        Err(format!("服务端错误 [{}]: {}", e.code, e.message))
                    };
                }
                Some(_) => continue,
                None => continue, // 坏行忽略（与服务端同纪律）
            }
        }
    }

    fn call(&mut self, m: &str, params: &Value) -> Result<Value, String> {
        let id = format!("cli-{}", now_ms());
        self.write_line(&automation::request_record(&id, m, params).to_string())?;
        self.read_response(&id, Duration::from_secs(15))
    }
}

fn handshake(stream: TcpStream, token: &str, timeout: Duration) -> Result<Conn, String> {
    stream.set_nodelay(true).ok();
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| format!("设超时失败: {e}"))?;
    let reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let mut conn = Conn { stream, reader };
    // hello
    let mut line = String::new();
    conn.reader.read_line(&mut line).map_err(|e| format!("读 hello 失败: {e}"))?;
    match automation::parse_line(&line) {
        Some(AutomationRecord::Hello { proto, .. }) if proto == PROTO_VERSION => {}
        Some(AutomationRecord::Hello { proto, .. }) => {
            return Err(format!("协议版本不匹配: 服务端 {proto}, CLI {PROTO_VERSION}"))
        }
        _ => return Err("服务端首行不是 hello".into()),
    }
    conn.write_line(&automation::auth_record(token).to_string())?;
    let mut line = String::new();
    conn.reader.read_line(&mut line).map_err(|e| format!("读 auth 应答失败: {e}"))?;
    match automation::parse_line(&line) {
        Some(AutomationRecord::AuthOk) => Ok(conn),
        Some(AutomationRecord::AuthError { code, message }) => {
            Err(format!("认证失败 [{code}]: {message}"))
        }
        _ => Err("服务端 auth 应答不可识别".into()),
    }
}

/// 找实例 → 连接 → 执行。选择序：--addr 直连 > PI_FLASH_AUTOMATION_FILE >
/// 实例目录里最新探活成功的一个（--pid 过滤）。
fn with_conn(
    pid: Option<u32>,
    addr: Option<String>,
    token: Option<String>,
    timeout: u64,
    f: impl FnOnce(&mut Conn) -> Result<(), String>,
) -> Result<(), String> {
    let (addr, token) = match (addr, token) {
        (Some(a), t) => (a, t.ok_or("--addr 直连必须同时给 --token")?),
        _ => discover(pid)?,
    };
    let stream = TcpStream::connect(&addr).map_err(|e| format!("连不上 {addr}: {e}"))?;
    let mut conn = handshake(stream, &token, Duration::from_secs(timeout))?;
    f(&mut conn)
}

fn discover(pid: Option<u32>) -> Result<(String, String), String> {
    if let Ok(f) = std::env::var("PI_FLASH_AUTOMATION_FILE") {
        let text = std::fs::read_to_string(&f).map_err(|e| format!("读 {f}: {e}"))?;
        let v: Value = serde_json::from_str(&text).map_err(|e| format!("解析 {f}: {e}"))?;
        return Ok((
            format!("127.0.0.1:{}", v["port"].as_u64().unwrap_or_default()),
            v["token"].as_str().unwrap_or_default().to_string(),
        ));
    }
    let candidates: Vec<(InstanceInfo, PathBuf)> = automation::read_instances()
        .into_iter()
        .filter(|(i, _)| pid.is_none_or(|p| i.pid == p))
        .collect();
    if candidates.is_empty() {
        return Err(match pid {
            Some(p) => format!("没有 pid={p} 的自动化实例（app 需以 PI_FLASH_AUTOMATION=auto 启动）"),
            None => "没有找到自动化实例（app 需以 PI_FLASH_AUTOMATION=auto 启动）".into(),
        });
    }
    // 新的在前（read_instances 已排序），逐个探活
    for (info, file) in candidates {
        let addr = format!("127.0.0.1:{}", info.port);
        let alive = TcpStream::connect_timeout(
            &addr.parse().map_err(|_| format!("实例文件端口坏: {file:?}"))?,
            Duration::from_millis(300),
        )
        .is_ok();
        if alive {
            return Ok((addr, info.token));
        }
        // 死文件：顺手清掉
        let _ = std::fs::remove_file(&file);
    }
    Err("实例登记都在但连不上（app 可能已退出）".into())
}

fn cmd_list() -> Result<(), String> {
    let all = automation::read_instances();
    if all.is_empty() {
        println!("（无实例登记；app 需以 PI_FLASH_AUTOMATION=auto 启动）");
        return Ok(());
    }
    for (info, file) in all {
        let alive = TcpStream::connect_timeout(
            &format!("127.0.0.1:{}", info.port)
                .parse()
                .map_err(|_| format!("端口坏: {file:?}"))?,
            Duration::from_millis(300),
        )
        .is_ok();
        println!(
            "pid={} port={} alive={} started={}ms ago file={}",
            info.pid,
            info.port,
            alive,
            now_ms().saturating_sub(info.started_at_ms),
            file.display()
        );
    }
    Ok(())
}

fn cmd_wait(
    pid: Option<u32>,
    addr: Option<String>,
    token: Option<String>,
    timeout: u64,
    cargs: &[String],
) -> Result<(), String> {
    let mut path = String::new();
    let mut eq = String::new();
    let mut total = 30u64;
    let mut interval = 250u64;
    let mut i = 0;
    while i < cargs.len() {
        match cargs[i].as_str() {
            "--path" => path = cargs.get(i + 1).cloned().unwrap_or_default(),
            "--eq" => eq = cargs.get(i + 1).cloned().unwrap_or_default(),
            "--timeout" => total = cargs.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(30),
            "--interval" => interval = cargs.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(250),
            _ => {}
        }
        i += if matches!(cargs[i].as_str(), "--path" | "--eq" | "--timeout" | "--interval") { 2 } else { 1 };
    }
    if path.is_empty() {
        return Err("wait 需要 --path <点分路径>".into());
    }
    // --eq 先按 JSON 解析，失败按字符串比（snapshot 里多为字符串/布尔/数字）
    let expect: Value = serde_json::from_str(&eq).unwrap_or_else(|_| Value::String(eq.clone()));

    let (addr, token) = match (addr, token) {
        (Some(a), t) => (a, t.ok_or("--addr 直连必须同时给 --token")?),
        _ => discover(pid)?,
    };
    let stream = TcpStream::connect(&addr).map_err(|e| format!("连不上 {addr}: {e}"))?;
    let mut conn = handshake(stream, &token, Duration::from_secs(timeout))?;

    let deadline = std::time::Instant::now() + Duration::from_secs(total);
    let id = format!("cli-wait-{}", now_ms());
    loop {
        conn.write_line(&automation::request_record(&id, method::UI_SNAPSHOT, &json!({})).to_string())?;
        let snap = conn.read_response(&id, Duration::from_secs(timeout))?;
        if let Some(got) = lookup(&snap, &path) {
            if *got == expect {
                // 只打印命中值：全量快照可能很大，断言场景要的是这一格
                let _ = writeln!(std::io::stdout(), "{}", serde_json::to_string_pretty(got).unwrap_or_default());
                return Ok(());
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!(
                "等待超时（{total}s）: {path} 仍不等于 {eq}\n最后快照: {}",
                serde_json::to_string(&snap).unwrap_or_default()
            ));
        }
        std::thread::sleep(Duration::from_millis(interval));
    }
}

/// 点分路径取值：对象键名 / 数组下标。
fn lookup<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = v;
    for seg in path.split('.') {
        match cur {
            Value::Object(m) => cur = m.get(seg)?,
            Value::Array(a) => cur = a.get(seg.parse::<usize>().ok()?)?,
            _ => return None,
        }
    }
    Some(cur)
}
