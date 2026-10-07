//! 本地租约锁（P1）：保证同一个 `bot_token` 只有一个轮询者在跑。
//!
//! 三件套语义照搬 ZCode `channelRuntime.ts:86-240`：
//! - `owner.json`（pid/botId/nonce/createdAt）+ `lease-<nonce>`（mtime 当心跳）**写在唯一临时目录里，
//!   再原子 rename 成正式锁目录** —— 竞争者不会把没初始化完的锁误判成 stale；
//! - 心跳每 `HEARTBEAT_MS` 刷一次 `lease-<nonce>` 的 mtime；
//! - 抢占时只有「租约新鲜」才让路，否则删掉陈旧锁目录重试。
//!
//! 与 ZCode 的唯一差异：ZCode 用 `process.kill(pid, 0)` 判活 + 租约新鲜两个条件，
//! 这里只用**租约新鲜度**。理由：std 没有跨平台的 kill(pid,0)，而心跳 10s 刷一次 mtime，
//! 租约 30s 未刷在本机语义上就等价于持有者已死（崩溃/挂起两种情况都能覆盖）。
//!
//! Windows 注意：`rename` 到已存在的目录返回 `ERROR_ACCESS_DENIED`（ZCode 注释里的 EPERM），
//! 所以只有**正式锁路径确实存在**时才进冲突分支，否则照常报错——避免把目录权限问题
//! 误判成可接管的锁。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

pub const LEASE_MS: u64 = 30_000;
pub const HEARTBEAT_MS: u64 = 10_000;

/// 清理重试档（对齐 ZCode `BOT_RUNTIME_LOCK_CLEANUP_RETRY_DELAYS_MS`）。
const CLEANUP_RETRY_DELAYS_MS: [u64; 3] = [100, 200, 300];
/// 锁目录：`<base>/bots-runtime-locks/<namespace>/<sha256(key)>.lock`。
const LOCK_ROOT: &str = "bots-runtime-locks";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Owner {
    pid: u32,
    bot_id: String,
    nonce: String,
    created_at: u64,
}

/// 已持有的锁。`release()` 或 `drop` 都会停心跳并只在 owner 仍是自己时删锁目录。
pub struct BotRuntimeLock {
    lock_path: PathBuf,
    owner: Owner,
    /// 通知心跳线程退出。**必须用 channel，不能用「置标志位 + sleep」**——
    /// 后者会让 `release()` 白等到心跳睡完（默认 10s），停轮询就卡 10 秒。
    stop: mpsc::Sender<()>,
    heartbeat: Option<JoinHandle<()>>,
}

impl BotRuntimeLock {
    pub fn release(mut self) {
        self.teardown();
    }

    fn teardown(&mut self) {
        let _ = self.stop.send(());
        if let Some(h) = self.heartbeat.take() {
            let _ = h.join();
        }
        if read_owner(&self.lock_path).as_ref() == Some(&self.owner) {
            let _ = remove_dir_with_retry(&self.lock_path);
        }
    }
}

impl Drop for BotRuntimeLock {
    fn drop(&mut self) {
        // 探针/轮询线程正常退出路径（含 `?` 早退）都走这里，
        // 否则下次启动要白等租约过期。
        self.teardown();
    }
}

/// 默认参数版本：P1 轮询用。
pub fn acquire(namespace: &str, lock_key: &str, bot_id: &str) -> Result<Option<BotRuntimeLock>, String> {
    acquire_with(namespace, lock_key, bot_id, HEARTBEAT_MS, LEASE_MS)
}

/// 可注入心跳/租约时长，便于单测（真实时长太长，测试等不起）。
pub fn acquire_with(
    namespace: &str,
    lock_key: &str,
    bot_id: &str,
    heartbeat_ms: u64,
    lease_ms: u64,
) -> Result<Option<BotRuntimeLock>, String> {
    let lock_path = lock_path(namespace, lock_key);
    let owner = Owner {
        pid: std::process::id(),
        bot_id: bot_id.to_string(),
        nonce: nonce(),
        created_at: now_ms(),
    };
    let parent = lock_path.parent().ok_or("锁路径缺少父目录")?;
    fs::create_dir_all(parent).map_err(|e| format!("创建锁根目录失败: {e}"))?;

    for _ in 0..2 {
        let pending = PathBuf::from(format!(
            "{}.{}.pending",
            lock_path.to_string_lossy(),
            owner.nonce
        ));
        let _ = fs::remove_dir_all(&pending);

        // 锁目录与 owner 文件必须作为一个完整状态对外可见（ZCode 同款 bugfix）。
        fs::create_dir(&pending).map_err(|e| format!("创建 pending 锁失败: {e}"))?;
        write_owner(&pending, &owner)?;
        fs::write(pending.join(format!("lease-{}", owner.nonce)), b"")
            .map_err(|e| format!("写 lease 失败: {e}"))?;

        let outcome = fs::rename(&pending, &lock_path);
        // pending 目录在任何分支都要清掉（ZCode 的 finally）。
        let result = match outcome {
            Ok(()) => Ok(Some(start_heartbeat(lock_path.clone(), owner.clone(), heartbeat_ms))),
            Err(rename_err) => {
                if !lock_path.exists() {
                    Err(format!("rename 锁失败（锁路径不存在，非竞争）: {rename_err}"))
                } else {
                    // 竞争分支：只有「租约仍新鲜」才让路。
                    if let Some(current) = read_owner(&lock_path) {
                        let lease_at = lease_mtime(&lock_path, &current.nonce);
                        let age = now_ms().saturating_sub(lease_at);
                        if lease_at > 0 && age < lease_ms {
                            Ok(None)
                        } else {
                            remove_dir_with_retry(&lock_path)?;
                            continue;
                        }
                    } else {
                        remove_dir_with_retry(&lock_path)?;
                        continue;
                    }
                }
            }
        };
        let _ = fs::remove_dir_all(&pending);
        return result;
    }
    Ok(None)
}

fn start_heartbeat(lock_path: PathBuf, owner: Owner, heartbeat_ms: u64) -> BotRuntimeLock {
    let (stop, rx) = mpsc::channel::<()>();
    let lease_path = lock_path.join(format!("lease-{}", owner.nonce));
    let interval = Duration::from_millis(heartbeat_ms.max(1));
    let handle = thread::spawn(move || loop {
        // 收到 stop（Ok / Disconnected）立刻退；只有超时才刷一次 lease。
        match rx.recv_timeout(interval) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => touch(&lease_path),
        }
    });
    BotRuntimeLock {
        lock_path,
        owner,
        stop,
        heartbeat: Some(handle),
    }
}

/// 锁根目录（仅用于告诉用户「谁在占着」）。
pub fn lock_root_display() -> String {
    crate::state::base_dir().join(LOCK_ROOT).display().to_string()
}

fn lock_path(namespace: &str, lock_key: &str) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(lock_key.trim().as_bytes());
    let digest = hex::encode(hasher.finalize());
    crate::state::base_dir()
        .join(LOCK_ROOT)
        .join(namespace)
        .join(format!("{digest}.lock"))
}

fn write_owner(dir: &Path, owner: &Owner) -> Result<(), String> {
    let body = serde_json::json!({
        "pid": owner.pid,
        "botId": owner.bot_id,
        "nonce": owner.nonce,
        "createdAt": owner.created_at,
    });
    fs::write(
        dir.join("owner.json"),
        serde_json::to_string_pretty(&body).unwrap_or_default(),
    )
    .map_err(|e| format!("写 owner.json 失败: {e}"))
}

fn read_owner(lock_path: &Path) -> Option<Owner> {
    let raw = fs::read_to_string(lock_path.join("owner.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    Some(Owner {
        pid: v.get("pid")?.as_u64()? as u32,
        bot_id: v.get("botId")?.as_str()?.to_string(),
        nonce: v.get("nonce")?.as_str()?.to_string(),
        created_at: v.get("createdAt")?.as_u64().unwrap_or(0),
    })
}

fn lease_mtime(lock_path: &Path, nonce: &str) -> u64 {
    fs::metadata(lock_path.join(format!("lease-{nonce}")))
        .and_then(|m| m.modified())
        .map(|t| t.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0))
        .unwrap_or(0)
}

/// 刷新 lease 文件 mtime（等价 ZCode `utimes(leasePath, now, now)`）。
fn touch(path: &Path) {
    if let Ok(f) = fs::OpenOptions::new().write(true).open(path) {
        let _ = f.set_modified(SystemTime::now());
    } else {
        let _ = fs::write(path, b"");
    }
}

fn remove_dir_with_retry(path: &Path) -> Result<(), String> {
    let mut last: Option<std::io::Error> = None;
    for attempt in 0..=CLEANUP_RETRY_DELAYS_MS.len() {
        match fs::remove_dir_all(path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => {
                last = Some(e);
                if attempt < CLEANUP_RETRY_DELAYS_MS.len() {
                    thread::sleep(Duration::from_millis(CLEANUP_RETRY_DELAYS_MS[attempt]));
                }
            }
        }
    }
    Err(format!(
        "清理锁目录失败 {}: {}",
        path.display(),
        last.map(|e| e.to_string()).unwrap_or_default()
    ))
}

fn nonce() -> String {
    format!("{:x}{:08x}", now_ms(), std::process::id())
}

/// 测试收尾：删掉本测试留下的空 namespace 目录，避免在真实配置目录里堆空壳。
#[cfg(test)]
fn cleanup_ns(ns: &str) {
    let _ = fs::remove_dir_all(
        crate::state::base_dir()
            .join(LOCK_ROOT)
            .join(ns),
    );
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用隔离目录：`PI_FLASH_DIR` 由 `pi_link::paths::dir()` 读取，
    /// 而它在进程内缓存不了 —— 直接给一个独立 namespace 即可，不污染真实锁。
    fn ns(tag: &str) -> String {
        format!("wxprobe-test-{tag}-{}", std::process::id())
    }

    #[test]
    fn second_acquire_is_rejected_while_held() {
        let ns = ns("held");
        let a = acquire_with(&ns, "key", "bot", 10_000, 30_000)
            .unwrap()
            .expect("首次获取应成功");
        let b = acquire_with(&ns, "key", "bot", 10_000, 30_000).unwrap();
        assert!(b.is_none(), "持锁期间第二次获取必须被拒绝");
        a.release();
        let c = acquire_with(&ns, "key", "bot", 10_000, 30_000)
            .unwrap()
            .expect("release 后必须能立即重取");
        c.release();
        cleanup_ns(&ns);
    }

    #[test]
    fn different_keys_do_not_collide() {
        let ns = ns("keys");
        let a = acquire_with(&ns, "token-a", "bot", 10_000, 30_000).unwrap().unwrap();
        let b = acquire_with(&ns, "token-b", "bot", 10_000, 30_000).unwrap().unwrap();
        a.release();
        b.release();
        cleanup_ns(&ns);
    }

    #[test]
    fn stale_lease_is_stolen() {
        let ns = ns("stale");
        let a = acquire_with(&ns, "key", "bot", 10_000, 30_000).unwrap().unwrap();
        // 模拟持有者崩溃：把 lease mtime 拨到 60s 前（远超 30s 租约）。
        let lock = lock_path(&ns, "key");
        let lease = lock.join(format!("lease-{}", a.owner.nonce));
        let stale = SystemTime::now() - Duration::from_secs(60);
        fs::File::options()
            .write(true)
            .open(&lease)
            .unwrap()
            .set_modified(stale)
            .unwrap();

        let b = acquire_with(&ns, "key", "bot", 10_000, 30_000)
            .unwrap()
            .expect("租约过期后必须能接管");
        b.release();
        // `a` 仍持有句柄但锁已被接管，其 release 不该删掉别人的锁。
        a.release();
        assert!(
            !lock_path(&ns, "key").exists(),
            "接管方 release 后锁目录应被清除"
        );
        cleanup_ns(&ns);
    }

    #[test]
    fn drop_releases_without_explicit_call() {
        let ns = ns("drop");
        {
            let _a = acquire_with(&ns, "key", "bot", 10_000, 30_000).unwrap().unwrap();
        }
        assert!(
            !lock_path(&ns, "key").exists(),
            "作用域结束（含 ? 早退）必须自动还锁，否则下次启动要白等 30s"
        );
        cleanup_ns(&ns);
    }

    #[test]
    fn release_returns_immediately_instead_of_waiting_for_heartbeat() {
        use std::time::Instant;
        let ns = ns("fast");
        // 心跳故意设 3s：若 release 仍在等心跳线程睡完，这里必然超过 1.5s 而失败。
        let a = acquire_with(&ns, "key", "bot", 3_000, 30_000).unwrap().unwrap();
        let started = Instant::now();
        a.release();
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(1_500),
            "release 不该等满一个心跳周期，实测 {elapsed:?}"
        );
        cleanup_ns(&ns);
    }

    #[test]
    fn owner_file_is_complete_before_visible() {
        let ns = ns("atomic");
        let _a = acquire_with(&ns, "key", "bot", 10_000, 30_000).unwrap().unwrap();
        let owner = read_owner(&lock_path(&ns, "key")).expect("锁可见时 owner 必须已完整");
        assert_eq!(owner.bot_id, "bot");
        assert_eq!(owner.pid, std::process::id());
        assert!(!owner.nonce.is_empty());
        drop(_a);
        cleanup_ns(&ns);
    }

    #[test]
    fn twenty_start_stop_cycles_keep_the_lock_exactly_once() {
        // 对应 P1 验收「重复启停 20 次」：每轮必须立刻拿到锁、结束后立刻释放，
        // 既不能残留锁（否则下一轮要白等 30s 租约），也不能被自己上一轮的残留挡住。
        let ns = ns("soak");
        for cycle in 0..20 {
            let lock = acquire_with(&ns, "key", "bot", 50, 30_000)
                .unwrap()
                .unwrap_or_else(|| panic!("第 {cycle} 轮应能立刻拿到锁"));
            assert!(
                acquire_with(&ns, "key", "bot", 50, 30_000).unwrap().is_none(),
                "第 {cycle} 轮持锁期间必须拒绝第二个抢锁者"
            );
            drop(lock);
            assert!(
                !lock_path(&ns, "key").exists(),
                "第 {cycle} 轮结束后锁目录必须已清除"
            );
        }
        cleanup_ns(&ns);
    }
}
