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
/// 扩展装载归 spawn 参数（工具预设档）管：full/full+ 档自带 `-ne` + 显式
/// `-e` 精确集；default/read-only/chat-only 档不发 `-ne`（加载
/// `~/.pi/agent` 扩展与 npm 包，pi-web parity，v70.3 拍板）。全局开关已删
/// （2026-10-10 用户定夺）：它在默认档路径（自定义/full）上本就无效，
/// 逃生口语义由档位系统承担。历史背景：曾恒定 `-ne` 防宿主扩展崩钉版
/// pi（系统 pi 1.0 的 auto-router.ts fatal 0.87.1 RPC），v70.3 改默认
/// 加载并把隔离降级为设置页开关。
pub fn spawn(cwd: &Path, extra_args: &[&str]) -> Result<(PiSession, UnboundedReceiver<Event>), String> {
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
    cmd.arg(&cli);
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
    // 051：pf-auth.json（目录 provider 的 injectAs / 降级明文）+ providers.json
    // （自定义 provider 的 `$PF_KEY_*`）解出的密钥注入子进程环境。解不出
    // 的引用跳过（该 provider 会话内报鉴权错），凭据库故障不阻塞 spawn。
    if let Some(pf_auth_path) = crate::pf_auth::path() {
        if let Some(providers_path) = crate::pf_providers::path() {
            cmd.envs(crate::credentials::spawn_env_at(
                &pf_auth_path,
                &providers_path,
                &crate::credentials::KeyringVault,
            ));
        }
    }
    // 051 M1.1：PF 自定义 provider 经官方扩展在 pi 进程内 registerProvider，
    // **不写 pi 的 models.json**。-ne 隔离下显式 -e 照常加载（resource-loader
    // 把 additionalExtensionPaths 与启用集合并），账本为空则不挂扩展。
    if let Some(ext) = crate::pf_providers::ext_path() {
        let has_providers = crate::pf_providers::read()
            .ok()
            .and_then(|doc| {
                doc.get("providers")
                    .and_then(|p| p.as_object())
                    .map(|o| !o.is_empty())
            })
            .unwrap_or(false);
        if has_providers && ext.is_file() {
            cmd.arg("-e").arg(ext);
        }
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
