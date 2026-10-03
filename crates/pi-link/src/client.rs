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
    // -ne (no extensions): vendored pin = self-contained distribution;
    // extensions belong to the host's own ~/.pi/agent (written against
    // whatever pi the host happens to run) and can crash OUR pin (real
    // case: system pi 1.0's auto-router.ts fatals a 0.87.1 RPC). Keep the
    // isolation even when pins coincide. (pi hint: "pi -ne")
    cmd.arg(&cli)
        .args(["-ne", "--mode", "rpc"])
        .args(extra_args)
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
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if let Some(log) = &rpc_log {
                let mut f = log.lock().unwrap();
                let _ = writeln!(f, "< {line}");
            }
            if let Some(event) = parse_line(&line) {
                if event_tx.unbounded_send(event).is_err() {
                    break;
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
    });

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
