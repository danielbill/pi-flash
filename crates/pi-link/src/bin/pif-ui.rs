//! pif-ui —— pi-flash UI 自动化服务的 CLI 驱动器（agent 调试用，同源
//! token-count 先例：pi-link 下的调试 bin）。
//!
//! 用法（先启动带自动化服务的 app：PI_FLASH_AUTOMATION=auto）：
//!   pif-ui list                       # 列出本机实例
//!   pif-ui info                       # app 级信息
//!   pif-ui snapshot [surface] [--only k]   # 数据化界面（可裁剪顶层键）
//!   pif-ui exec <method> [--arg k=v]...    # op 执行（CLI 组 JSON，免三层转义）
//!   pif-ui keys "ctrl-s"              # 合成按键（走真实键位表）
//!   pif-ui type <text>                # composer.set_text 的糖
//!   pif-ui shot [输出.png]            # 窗口截图（进程内渲染回读，非 OS 抢屏）
//!   pif-ui wait --path a.b[0].c --eq v [--surface s] [--timeout 30]
//!               [--contains 子串 | --truthy]    # 轮询快照直到断言成立
//!   pif-ui clean                      # 清死登记（仅明确拒绝连接的）
//!
//! Windows 路径**用正斜杠**（C:/x/y），或交给 --arg 免手拼 JSON。
//! 实例发现：`~/.pi-flash/automation/<pid>.json`（最新优先）；`--pid N`
//! 指定、`--addr H:P --token T` 直连、`PI_FLASH_AUTOMATION_FILE` 指向实例
//! 文件。判死规则：只有 ConnectionRefused 才删登记（防误杀活实例）。

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
  list                          列出本机 pi-flash 自动化实例
  info                          app 级信息
  snapshot [surface] [--only k] 数据化界面快照（app/sessions/session/composer/
                                files/git/settings/dialogs；--only 只留指定
                                顶层键，可重复，大面裁剪用）
  exec <method> [json] [--arg k=v]... [--params-file f.json]
                                执行 op。参数三种给法：位置 JSON、--arg
                                键值（值合法 JSON 则按 JSON，否则字符串；
                                可重复、覆盖前者）、--params-file 整个
                                params 从文件读
  keys <组合键>                  合成按键，如 \"ctrl-s\"、\"escape\"
  type <text>                   设置 composer 文本
  shot [输出.png]               截取窗口最近一帧为 PNG（进程内渲染回读，
                                非 OS 抢屏，窗口被遮挡/最小化也能截；
                                缺省写 <配置目录>/automation/shots/）
  wait --path <a.b[0].c>        轮询 ui.snapshot 直到断言成立（成功只打印
       (--eq 值 | --contains 子串 | --truthy)
       [--surface s] [--timeout 秒] [--interval 毫秒]
                                命中值。路径支持对象键与数组下标
                                （a.b[0].c 或 a.b.0.c）；断言三选一：
                                --eq 相等 / --contains 字符串化后包含 /
                                --truthy 非空非假
  clean                         清理死登记（只删明确拒绝连接的；其余保留
                                并标注，token 只在登记里，误删=失联）

Windows 路径建议用正斜杠 C:/x/y，或直接 --arg path=C:/x/y 免转义。

全局选项:
  --pid <N>                     指定实例（缺省选最新活着的一个）
  --addr <HOST:PORT>            直连地址（配合 --token，跳过实例发现）
  --token <TOK>                 直连 token
  --timeout <秒>                单次请求读超时（缺省 15）
  -h | help                     本帮助";

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
            "clean" => cmd_clean(),
            "info" => with_conn(pid, addr, token, timeout, |c| {
                c.call(method::APP_INFO, &json!({})).map(print_json)
            }),
            "snapshot" => {
                let (pos, flags) = split_args(cargs, &["only"]);
                let surface = pos.first().cloned().unwrap_or_default();
                let mut params = json!({});
                if !surface.is_empty() {
                    params["surface"] = json!(surface);
                }
                let only: Vec<String> = flags
                    .iter()
                    .filter(|(k, _)| k == "only")
                    .map(|(_, v)| v.clone())
                    .collect();
                if !only.is_empty() {
                    params["only"] = if only.len() == 1 {
                        json!(only[0])
                    } else {
                        json!(only)
                    };
                }
                with_conn(pid, addr, token, timeout, |c| {
                    c.call(method::UI_SNAPSHOT, &params).map(print_json)
                })
            }
            "exec" => {
                let (pos, flags) = split_args(cargs, &["arg", "params-file"]);
                let m = pos.first().ok_or("exec 需要 method 参数")?;
                let mut params: Value = if let Some(f) = flag_last(&flags, "params-file") {
                    let text = std::fs::read_to_string(&f).map_err(|e| format!("读 {f}: {e}"))?;
                    serde_json::from_str(&text).map_err(|e| format!("{f} 不是合法 JSON: {e}"))?
                } else if let Some(s) = pos.get(1) {
                    serde_json::from_str(s).map_err(|e| format!("参数不是合法 JSON: {e}"))?
                } else {
                    json!({})
                };
                if !params.is_object() {
                    return Err("params 必须是 JSON 对象".into());
                }
                // --arg k=v：值先按 JSON 解析、失败按字符串（路径免转义的关键）
                for (_k, v) in flags.iter().filter(|(k, _)| *k == "arg") {
                    let (key, raw) = match v.split_once('=') {
                        Some((k, r)) => (k.to_string(), r),
                        None => return Err(format!("--arg 需要 k=v 形式，得到 {v:?}")),
                    };
                    let val = serde_json::from_str::<Value>(raw)
                        .unwrap_or_else(|_| Value::String(raw.to_string()));
                    params[key] = val;
                }
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
            "shot" => {
                let mut params = json!({});
                if let Some(out) = cargs.first() {
                    params["path"] = json!(out);
                }
                with_conn(pid, addr, token, timeout, |c| {
                    c.call(method::UI_SCREENSHOT, &params).map(print_json)
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

/// 子命令参数切分：值型旗标（--k v）收进列表（可重复，保序），其余位置参数。
fn split_args(args: &[String], value_flags: &[&str]) -> (Vec<String>, Vec<(String, String)>) {
    let mut pos = Vec::new();
    let mut flags = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if let Some(name) = a.strip_prefix("--") {
            if let Some(_) = value_flags.iter().find(|f| **f == name) {
                if let Some(v) = args.get(i + 1) {
                    flags.push((name.to_string(), v.clone()));
                    i += 2;
                    continue;
                }
            }
        }
        pos.push(a.clone());
        i += 1;
    }
    (pos, flags)
}

fn flag_last<'a>(flags: &'a [(String, String)], name: &str) -> Option<&'a str> {
    flags.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
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

/// 判死规则见 automation::port_is_definitely_dead：只有明确拒绝连接才删
/// 登记，其它失败保留文件、直接当作候选返回（让真实连接去暴露真错）。
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
    // 新的在前（read_instances 已排序）
    for (info, file) in candidates {
        let addr = format!("127.0.0.1:{}", info.port);
        if automation::port_is_definitely_dead(&addr) {
            let _ = std::fs::remove_file(&file); // 明确没人监听，登记是死的
            continue;
        }
        return Ok((addr, info.token));
    }
    Err("实例登记都在但端口明确拒绝连接（app 已退出？登记已清，重试或 pif-ui list）".into())
}

fn cmd_list() -> Result<(), String> {
    let all = automation::read_instances();
    if all.is_empty() {
        println!("（无实例登记；app 需以 PI_FLASH_AUTOMATION=auto 启动）");
        return Ok(());
    }
    for (info, file) in all {
        let addr = format!("127.0.0.1:{}", info.port);
        // 只标注不删除（删留给 clean / discover 的明确判死）
        let state = if automation::port_is_definitely_dead(&addr) {
            "dead"
        } else {
            "alive"
        };
        println!(
            "pid={} port={} {state} started={}ms ago file={}",
            info.pid,
            info.port,
            now_ms().saturating_sub(info.started_at_ms),
            file.display()
        );
    }
    Ok(())
}

fn cmd_clean() -> Result<(), String> {
    let all = automation::read_instances();
    if all.is_empty() {
        println!("（无实例登记）");
        return Ok(());
    }
    for (info, file) in all {
        let addr = format!("127.0.0.1:{}", info.port);
        if automation::port_is_definitely_dead(&addr) {
            let _ = std::fs::remove_file(&file);
            println!("removed pid={} port={}（明确拒绝连接）", info.pid, info.port);
        } else {
            println!("kept pid={} port={}（活着或无法确认死亡，保守保留）", info.pid, info.port);
        }
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
    let truthy_flag = cargs.iter().any(|a| a == "--truthy");
    let (pos, flags) = split_args(cargs, &["path", "eq", "contains", "timeout", "interval", "surface"]);
    // split_args 会把 --truthy 当位置参数，剔除
    let _ = pos;
    let flag = |name: &str| flags.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    let path = flag("path").ok_or("wait 需要 --path <点分路径>")?;
    let eq = flag("eq");
    let contains = flag("contains");
    let surface = flag("surface").unwrap_or_default();
    let total: u64 = flag("timeout").and_then(|v| v.parse().ok()).unwrap_or(30);
    let interval: u64 = flag("interval").and_then(|v| v.parse().ok()).unwrap_or(250);
    let n_asserts = eq.is_some() as u8 + contains.is_some() as u8 + truthy_flag as u8;
    if n_asserts != 1 {
        return Err("断言三选一：--eq 值 / --contains 子串 / --truthy".into());
    }
    // --eq 先按 JSON 解析，失败按字符串比（snapshot 里多为字符串/布尔/数字）
    let expect: Value = eq
        .as_deref()
        .map(|s| serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.to_string())))
        .unwrap_or(Value::Null);

    let (addr, token) = match (addr, token) {
        (Some(a), t) => (a, t.ok_or("--addr 直连必须同时给 --token")?),
        _ => discover(pid)?,
    };
    let stream = TcpStream::connect(&addr).map_err(|e| format!("连不上 {addr}: {e}"))?;
    let mut conn = handshake(stream, &token, Duration::from_secs(timeout))?;

    let params = if surface.is_empty() {
        json!({})
    } else {
        json!({"surface": surface})
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(total);
    let id = format!("cli-wait-{}", now_ms());
    loop {
        conn.write_line(&automation::request_record(&id, method::UI_SNAPSHOT, &params).to_string())?;
        let snap = conn.read_response(&id, Duration::from_secs(timeout))?;
        let hit = lookup(&snap, &path).map(|got| {
            if let Some(eq) = &eq {
                // --eq 比较前也做一次字符串宽容：true/false 数字等走 JSON，其余字符串
                let _ = eq;
                *got == expect
            } else if let Some(sub) = &contains {
                serde_json::to_string(got)
                    .map(|s| s.contains(sub.as_str()))
                    .unwrap_or(false)
            } else {
                value_truthy(got)
            }
        });
        if hit == Some(true) {
            if let Some(got) = lookup(&snap, &path) {
                let _ = writeln!(
                    std::io::stdout(),
                    "{}",
                    serde_json::to_string_pretty(got).unwrap_or_default()
                );
            }
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            // 截断最后快照：全量可到 MB 级，错误信息里 1.5KB 足够定位
            let mut last = serde_json::to_string(&snap).unwrap_or_default();
            if last.len() > 1500 {
                last.truncate(1500);
                last.push_str("…(截断)");
            }
            return Err(format!("等待超时（{total}s）: {path} 断言仍不成立\n最后快照: {last}"));
        }
        std::thread::sleep(Duration::from_millis(interval));
    }
}

/// 点分路径取值：对象键名 / 数组下标（`a.b[0].c` 与 `a.b.0.c` 等价）。
fn lookup<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    // [0] → .0（去中括号），再按 . 切并滤空段（容忍 a..b / 开头点）
    let norm: String = path.replace('[', ".").replace(']', "");
    let mut cur = v;
    for seg in norm.split('.').filter(|s| !s.is_empty()) {
        match cur {
            Value::Object(m) => cur = m.get(seg)?,
            Value::Array(a) => cur = a.get(seg.parse::<usize>().ok()?)?,
            _ => return None,
        }
    }
    Some(cur)
}

/// --truthy 判定：非 null/false/0/空串/空数组 即真（对象视为真——存在即信息）。
fn value_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_supports_dotted_and_bracket_paths() {
        let v = json!({"tabs": [{"file": "a.md", "dirty": false}, {"file": "b.md"}], "app": {"booted": true}});
        assert_eq!(lookup(&v, "app.booted"), Some(&json!(true)));
        assert_eq!(lookup(&v, "tabs.0.file"), Some(&json!("a.md")));
        assert_eq!(lookup(&v, "tabs[0].file"), Some(&json!("a.md")));
        assert_eq!(lookup(&v, "tabs[1].dirty"), None); // 键不存在
        assert_eq!(lookup(&v, "tabs.5.file"), None); // 越界
        assert_eq!(lookup(&v, "app.booted.x"), None); // 标量再下钻
        assert_eq!(lookup(&v, ".app..booted"), Some(&json!(true))); // 容忍空段
        assert_eq!(lookup(&v, "[0].x", ), None); // 根不是数组
    }

    #[test]
    fn truthy_matrix() {
        assert!(value_truthy(&json!(true)));
        assert!(value_truthy(&json!(1)));
        assert!(value_truthy(&json!("x")));
        assert!(value_truthy(&json!([0])));
        assert!(value_truthy(&json!({"a": 1})));
        assert!(!value_truthy(&json!(false)));
        assert!(!value_truthy(&json!(null)));
        assert!(!value_truthy(&json!(0)));
        assert!(!value_truthy(&json!("")));
        assert!(!value_truthy(&json!([])));
    }

    #[test]
    fn split_args_collects_repeatable_value_flags() {
        let args: Vec<String> = ["file.open", "--arg", "path=C:/x/y.md", "--arg", "all=true", "leftover"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (pos, flags) = split_args(&args, &["arg"]);
        assert_eq!(pos, vec!["file.open".to_string(), "leftover".to_string()]);
        assert_eq!(flags.len(), 2);
        assert_eq!(flags[0], ("arg".to_string(), "path=C:/x/y.md".to_string()));
        // 值按 JSON 解析的路径在 exec 主流程；此处只验切分
    }
}
