//! Owns the pi sidecar process and its JSONL plumbing.

use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command as StdCommand, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use futures::channel::mpsc::{UnboundedReceiver, unbounded};

use crate::protocol::{Command, Event, parse_line};
use crate::vendor;

/// stderr 尾部缓冲上限：崩溃原因（node OOM / unhandled rejection / 扩展
/// fatal）只需尾部几行，整段留存反而吃内存
const STDERR_TAIL_CAP: usize = 16 * 1024;

pub struct PiSession {
    /// 进程句柄挂在共享锁后：exit-waiter 线程跨线程 reap 退出码
    ///（std 无法在别的线程 `wait()`），Drop 时 `take()` 接管击杀
    child: Arc<Mutex<Option<Child>>>,
    cmd_tx: mpsc::Sender<String>,
    seq: AtomicU64,
    stderr_tail: Arc<Mutex<Vec<u8>>>,
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
pub fn spawn(
    cwd: &Path,
    extra_args: &[&str],
    envs: &[(String, String)],
) -> Result<(PiSession, UnboundedReceiver<Event>), String> {
    let cli = vendor::cli_path().ok_or_else(|| {
        "vendored pi not found — run `npm ci` inside vendor/pi (see PORT_PLAN.md)".to_string()
    })?;

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
    cmd.args(["--mode", "rpc"])
        .args(extra_args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    // stderr 恒 piped + 尾部缓冲：pi fatal（扩展崩溃 / node OOM /
    // unhandled rejection）只出现在这里——之前 Stdio::null()（仅
    // PI_FLASH_RPC_LOG 时才接）时进程死亡完全无迹可寻。
    cmd.stderr(Stdio::piped());
    // 042-1：**不再注入 env、不再挂 pf-providers 扩展**。凭据由 pi 自己从
    // auth.json 的标记条目执行 `pif-secret` 取回（钥匙库），模型定义由 pi
    // 自己读 models.json。spawn 只负责起进程。
    // 045 M1：subagent 扩展。开发旗标开着且文件存在才挂（`-ne` 下显式 `-e`
    // 仍生效），模型因此多出 `agent` 工具；不挂只是没有该工具，不影响启动。
    // 默认关（dev-flags.json / PI_FLASH_SUBAGENTS 控制，重启生效）。
    if crate::subagents::dev_enabled() {
        if let Some(ext) = crate::subagents::ext_path() {
            cmd.arg("-e").arg(ext);
        }
    }
    // 执行域（90-6）唯一例外：PF_SVC_CONFIG = 宿主自带 shim 的配置指针（无凭据）
    if !envs.is_empty() {
        cmd.envs(envs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
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

    // stderr drainer：常驻（而非仅 wire log 时），写尾部环形缓冲 + 可选落日志
    let stderr_tail: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    if let Some(mut err) = stderr {
        let tail = stderr_tail.clone();
        let log_err = rpc_log.clone();
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            use std::io::Read as _;
            while let Ok(n) = err.read(&mut buf) {
                if n == 0 {
                    break;
                }
                {
                    let mut t = tail.lock().unwrap();
                    t.extend_from_slice(&buf[..n]);
                    let over = t.len().saturating_sub(STDERR_TAIL_CAP);
                    if over > 0 {
                        t.drain(..over);
                    }
                }
                if let Some(log) = &log_err {
                    let text = String::from_utf8_lossy(&buf[..n]);
                    let mut f = log.lock().unwrap();
                    let _ = write!(f, "[stderr] {text}");
                }
                // PI_FLASH_PI_STDERR=1：pi 侧 stderr（扩展 console.error 等）直通
                // 宿主 stderr——插件通信排障用（[wxbridge] 等）。env 门控，
                // 缺省关闭：release 零输出烟测门禁不变。
                if std::env::var("PI_FLASH_PI_STDERR").as_deref() == Ok("1") {
                    let text = String::from_utf8_lossy(&buf[..n]);
                    eprint!("[pi-stderr] {text}");
                }
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
    // waiter 的 sender 克隆要在 reader spawn 前取（reader 会 move 走原值）
    let waiter_tx = event_tx.clone();
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
                            let _ = writeln!(
                                f,
                                "=== unparsed ({} bytes): {}",
                                line.len(),
                                &line[..line.len().min(200)]
                            );
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

    // 进程句柄挂到共享锁后：exit-waiter 跨线程 reap 退出码（std 无法在
    // 别的线程 wait()），Drop 时 take() 接管击杀——waiter 见 None 即退出，
    // 主动关闭不发死亡事件
    let child_shared: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(Some(child)));
    let waiter_child = child_shared.clone();
    thread::Builder::new()
        .name("pi-rpc-exit-waiter".into())
        .spawn(move || {
            loop {
                thread::sleep(Duration::from_millis(300));
                let status = {
                    let mut guard = waiter_child.lock().unwrap();
                    match guard.as_mut() {
                        Some(child) => child.try_wait().ok().flatten(),
                        None => break, // Drop 已接管（主动关闭）：不发死亡事件
                    }
                };
                if let Some(status) = status {
                    // 合成事件：进程死亡不再只是 stdout 静默关闭。发完后本线程
                    // 掉落 sender——通道在 reader 也收尾后才关闭，app 侧 pump
                    // 尾部先拿到退出码再走轮终收尾。
                    let _ = waiter_tx.unbounded_send(Event::ProcessExited {
                        code: status.code().map(i64::from),
                    });
                    break;
                }
            }
        })
        .expect("spawn pi rpc exit waiter");

    Ok((
        PiSession {
            child: child_shared,
            cmd_tx,
            seq: AtomicU64::new(0),
            stderr_tail,
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

    pub fn id(&self) -> Option<u32> {
        self.child.lock().unwrap().as_ref().map(Child::id)
    }

    /// stderr 尾部（崩溃原因：node OOM / unhandled rejection / 扩展 fatal
    /// 只出现在这里）。进程死亡轮终时由 app 侧留痕。
    pub fn stderr_tail(&self) -> String {
        let buf = self.stderr_tail.lock().unwrap();
        String::from_utf8_lossy(&buf).into_owned()
    }
}

impl Drop for PiSession {
    fn drop(&mut self) {
        // take() 先把 child 从 waiter 手里拿走（它见 None 即退出）；然后
        // closing stdin requests orderly shutdown; kill is the hard fallback
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
