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

/// reader 线程按白名单丢弃的无消费者 WireEvent 计数（047-内存削减 P1-2）
static DROPPED_WIRE: AtomicU64 = AtomicU64::new(0);

/// ext-ui 全链路留痕（#问题卡不弹，2026-11-04 真机两次复现）：wire 送达锚点 /
/// pool 路由与 parked 队列 / bind 迁移碰撞 / ask_card 分支统一走这里。
/// release 无控制台 stderr 不可见——必须落盘 `<PF dir>/logs/ext-ui.log`，
/// 追加写、全局互斥防多线程交错；路径不可得时退 stderr。事件低频，无热路径。
pub fn ext_trace(msg: &str) {
    static LOCK: Mutex<()> = Mutex::new(());
    let _g = LOCK.lock();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let line = format!("[ext-ui] ts={ts} {msg}\n");
    if let Some(dir) = crate::paths::dir() {
        let logs = dir.join("logs");
        if std::fs::create_dir_all(&logs).is_ok() {
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(logs.join("ext-ui.log"))
            {
                let _ = f.write_all(line.as_bytes());
                return;
            }
        }
    }
    eprint!("{line}");
}

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
/// ：它在默认档路径（自定义/full）上本就无效，
/// 逃生口语义由档位系统承担。历史背景：曾恒定 `-ne` 防宿主扩展崩钉版
/// pi（系统 pi 1.0 的 auto-router.ts fatal 0.87.1 RPC），v70.3 改默认
/// 加载并把隔离降级为设置页开关。
///
/// `mount_subagents` = 宿主是否允许挂载 045 子代理扩展。工具面是 **spawn
/// 参数**（RPC 没有运行时切换）：设置-子代理页模块开关关闭时宿主传 false，
/// 扩展不挂载 ⇒ `agent` / `subagent_*` 工具压根不注册（真机 2026-11-08：
/// 关了开关工具还在，因为旧实现在这里只看开发旗标）。开发旗标仍是兜底
/// 与门（[`crate::subagents::mount_target`]）。
pub fn spawn(
    cwd: &Path,
    extra_args: &[&str],
    envs: &[(String, String)],
    mount_subagents: bool,
    mount_pfctx_manager: bool,
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
    // cwd 兜底：无项目/草稿会话的 cwd 可能为空——`current_dir("")` 在
    // Windows 直接 os error 123（文件名语法不正确），pi 一次都起不来
    // （L3 隔离实例实测 2026-11-03：全部 spawn 静默失败，ext/工具面全哑）。
    // 落到进程当前目录（boot 侧已重定向数据目录，无 projects 语境）。
    let cwd: &Path = if cwd.as_os_str().is_empty() {
        &Path::new(".")
    } else {
        cwd
    };
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
    // 045 M1：subagent 扩展。宿主意愿（PF 设置-子代理页模块开关）与开发
    // 旗标都开着、且文件存在才挂（`-ne` 下显式 `-e` 仍生效），模型因此多出
    // `agent` 工具；不挂只是没有该工具，不影响启动。工具清单是 spawn 参数，
    // 关掉开关要重绑进程才消失（app 侧 `Chat::apply_subagent_switch`）。
    if let Some(ext) = crate::subagents::mount_target(mount_subagents) {
        cmd.arg("-e").arg(ext);
    }
    // 001-4：pfctx_manager 设置工具。宿主意愿 = 设置-更多【自我调试】
    // （hub `agent.self_debug`，默认开），关掉即不挂（工具清单是 spawn 参数，
    // 切换由 app 侧仿 apply_subagent_switch 重绑生效）。
    if let Some(ext) = crate::pfctx_manager::mount_target(mount_pfctx_manager) {
        cmd.arg("-e").arg(ext);
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
        // PI_FLASH_PI_STDERR=1：pi 侧 stderr（扩展 console.error 等）直通
        // 宿主 stderr——插件通信排障用（[wxbridge] 等）。env 门控，
        // 缺省关闭：release 零输出烟测门禁不变。（env 只读一次——drainer
        // 是 4KB 块的热循环，逐块 env::var 白付系统调用，047-内存削减 P0-7）
        let pi_stderr_passthrough = std::env::var("PI_FLASH_PI_STDERR").as_deref() == Ok("1");
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
                if pi_stderr_passthrough {
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
                        // 047-P1-2 kind 白名单：无消费者 WireEvent 掐在 reader
                        //（白名单依据见 wire::wire_event_allowed），不进无界
                        // 通道积压。丢弃走 ext_trace 文件留痕（首条 + 每
                        // 1000 条），wire log 不受影响（原始行在解析前已落）。
                        if let Event::WireEvent { kind, .. } = &event {
                            if !crate::protocol::wire_event_allowed(kind) {
                                let n = DROPPED_WIRE.fetch_add(1, Ordering::Relaxed);
                                if n == 0 || n % 1000 == 0 {
                                    ext_trace(&format!(
                                        "wire 无消费者丢弃 kind={kind}（累计 {}）",
                                        n + 1
                                    ));
                                }
                                continue;
                            }
                        }
                        // ext-ui 链路留痕（2026-11-04 #问题卡不弹）：阻塞类
                        // extension_ui_request 一旦在宿主侧被吞，pi 永久挂起且
                        // 界面零痕迹——本行是链路最上游的「确已送达」锚点，
                        // 与 pool/ask_card 的下游 trace 用同一前缀可对账。
                        if let Event::ExtensionUi(req) = &event {
                            use crate::protocol::ExtUiMethod;
                            let m = match &req.method {
                                ExtUiMethod::Select { .. } => "select",
                                ExtUiMethod::Confirm { .. } => "confirm",
                                ExtUiMethod::Input { .. } => "input",
                                ExtUiMethod::Editor { .. } => "editor",
                                _ => "fire-and-forget",
                            };
                            ext_trace(&format!("wire: id={} method={m}", req.id));
                        }
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
