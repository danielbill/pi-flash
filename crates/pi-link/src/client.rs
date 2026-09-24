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

    let mut cmd = StdCommand::new("node");
    cmd.arg(&cli)
        .args(["--mode", "rpc", "--no-session"])
        .args(extra_args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
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

    // writer thread: owns pi's stdin
    let (cmd_tx, cmd_rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let mut stdin = stdin;
        for line in cmd_rx {
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
            if let Some(event) = parse_line(&line) {
                if event_tx.unbounded_send(event).is_err() {
                    break;
                }
            }
            // non-JSON noise (ANSI title sequences etc.) is intentionally dropped
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
