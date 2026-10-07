//! Owns the pi sidecar process and its JSONL plumbing.

use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command as StdCommand, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
};

use futures::channel::mpsc::{UnboundedReceiver, unbounded};

use crate::protocol::{Command, Event, parse_line};
use crate::vendor;

pub struct PiSession {
    child: Child,
    cmd_tx: mpsc::Sender<String>,
    seq: AtomicU64,
}

/// Spawn the vendored pi in RPC mode bound to `cwd`.
///
/// Only the vendored pi is ever used (never PATH); see [`crate::vendor`].
///
/// `load_extensions = false` spawns with `-ne`: the isolation mode. pi-web
/// loads extensions (npm plugins register providers/tools — freeflow etc.),
/// and the app follows that by default (`AppSettings.load_extensions`,
/// settings·misc switch); `false` is the escape hatch when some extension
/// fatals the RPC session (historical case: system pi 1.0's auto-router.ts).
pub fn spawn(
    cwd: &Path,
    extra_args: &[&str],
    load_extensions: bool,
) -> Result<(PiSession, UnboundedReceiver<Event>), String> {
    let cli = vendor::cli_path()
        .ok_or_else(|| "vendored pi not found — run `npm ci` inside vendor/pi (see PORT_PLAN.md)".to_string())?;

    let node: String = vendor::node_bin();
    // Optional wire log (PI_FLASH_RPC_LOG=<path>): every spawn records its
    // full command line, then ">> " outgoing / "< " incoming lines and the
    // child's stderr — diagnosis for "RPC works in probes, dead in the app".
    // Off by default; scripts/dev.sh turns it on per launch.
    let rpc_log: Option<std::sync::Arc<std::sync::Mutex<std::fs::File>>> =
        std::env::var("PI_FLASH_RPC_LOG")
            .ok()
            .filter(|p| !p.is_empty())
            .and_then(|p| {
                // create(true) doesn't make parent dirs; the path may be
                // relative to the OPENED PROJECT's cwd, so make sure it exists
                if let Some(parent) = std::path::Path::new(&p).parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&p)
                    .ok()
                    .map(|f| std::sync::Arc::new(std::sync::Mutex::new(f)))
            });
    if let Some(log) = &rpc_log {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut f = log.lock().unwrap();
        let _ = writeln!(
            f,
            "=== spawn ts={ts} node={node} cwd={} args={extra_args:?} cli={}",
            cwd.display(),
            cli.display()
        );
    }
    let mut cmd = StdCommand::new(node);
    // no baked-in --no-session: fresh spawns persist by default (pi-web
    // parity: sessions are resumable); pass ["--session", <path>] to resume
    // -ne (no extensions): vendored pin = self-contained distribution;
    // extensions belong to the host's own ~/.pi/agent (written against
    // whatever pi the host happens to run) and can crash OUR pin (real
    // case: system pi 1.0's auto-router.ts fatals a 0.87.1 RPC). Keep the
    // isolation even when pins coincide. (pi hint: "pi -ne")
    cmd.arg(&cli);
    // full+plugins 档自己在 extra args 里带 -ne（精确插件集）；全局隔离
    // 开关也开时不要发第二份（重复无害——D 组实测——但日志干净些）
    let extra_has_ne = extra_args
        .iter()
        .any(|a| *a == "-ne" || *a == "--no-extensions");
    if !load_extensions && !extra_has_ne {
        cmd.arg("-ne");
    }
    cmd.args(["--mode", "rpc"]).args(extra_args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    // stderr is discarded unless the wire log is on — pi fatals (extension
    // crashes etc.) only ever show up there, and it was a blind spot.
    if rpc_log.is_some() {
        cmd.stderr(Stdio::piped());
    } else {
        cmd.stderr(Stdio::null());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn vendored pi ({}): {e}", cli.display()))?;

    let stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take();

    // stderr drainer: only exists when the wire log is on
    if let (Some(log), Some(mut err)) = (rpc_log.clone(), stderr) {
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            use std::io::Read as _;
            while let Ok(n) = err.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let text = String::from_utf8_lossy(&buf[..n]);
                let mut f = log.lock().unwrap();
                let _ = write!(f, "[stderr] {text}");
            }
        });
    }

    // writer thread: owns pi's stdin
    let (cmd_tx, cmd_rx) = mpsc::channel::<String>();
    let log_out = rpc_log.clone();
    thread::spawn(move || {
        let mut stdin = stdin;
        for line in cmd_rx {
            if let Some(log) = &log_out {
                let mut f = log.lock().unwrap();
                let _ = writeln!(f, ">> {line}");
            }
            if stdin.write_all(line.as_bytes()).is_err() || stdin.write_all(b"\n").is_err() {
                break;
            }
            let _ = stdin.flush();
        }
    });

    // reader thread: parse JSONL into typed events
    let (event_tx, event_rx) = unbounded::<Event>();
    // 16MB stack: get_tree 的会话树是深层嵌套（每条 entry 一层），而
    // serde_json::Value 的析构是递归的——3000 条 entry 的会话在默认
    // 2MiB 线程栈上会溢出。std::thread::spawn 默认就是 2MiB。
    thread::Builder::new()
        .name("pi-rpc-reader".into())
        .stack_size(16 << 20)
        .spawn(move || {
            for line in BufReader::new(stdout).lines() {
                // NOT `break`: one non-UTF-8 line (or a JSON value too deep for
                // the parser) would silently kill every LATER event — that is
                // how a missing get_tree / fork response becomes "the button
                // does nothing" with no trace anywhere.
                let Ok(line) = line else {
                    if let Some(log) = &rpc_log {
                        let mut f = log.lock().unwrap();
                        let _ = writeln!(f, "=== stdout line decode error (skipped)");
                    }
                    continue;
                };
                if let Some(log) = &rpc_log {
                    let mut f = log.lock().unwrap();
                    let _ = writeln!(f, "< {line}");
                }
                match parse_line(&line) {
                    Some(event) => {
                        if event_tx.unbounded_send(event).is_err() {
                            break;
                        }
                    }
                    None => {
                        // pi-web parity 依赖的响应（get_tree/fork/get_messages）
                        // 走到这里就是被解析器丢掉了——wire log 留痕，否则
                        // 「按钮点了没反应」在界面上完全无迹可寻
                        if let Some(log) = &rpc_log {
                            let mut f = log.lock().unwrap();
                            let _ = writeln!(f, "=== unparsed ({} bytes): {}", line.len(), &line[..line.len().min(200)]);
                        }
                    }
                }
                // non-JSON noise (ANSI title sequences etc.) is intentionally dropped
            }
            if let Some(log) = &rpc_log {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let mut f = log.lock().unwrap();
                let _ = writeln!(f, "=== stdout EOF ts={ts}");
            }
        })
        .expect("spawn pi rpc reader thread");

    Ok((
        PiSession {
            child,
            cmd_tx,
            seq: AtomicU64::new(0),
        },
        event_rx,
    ))
}

impl PiSession {
    /// Send a command; returns the correlation id used for its response.
    pub fn send(&self, command: &Command) -> Result<String, String> {
        let id = format!("cmd-{}", self.seq.fetch_add(1, Ordering::Relaxed));
        let record = command.to_record(&id);
        self.cmd_tx
            .send(record.to_string())
            .map_err(|_| "pi stdin closed".to_string())?;
        Ok(id)
    }

    pub fn id(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for PiSession {
    fn drop(&mut self) {
        // closing stdin requests orderly shutdown; kill is the hard fallback
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
