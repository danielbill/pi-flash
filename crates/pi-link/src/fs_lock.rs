//! `proper-lockfile` 协议复刻（与 pi 的 `AuthStorage` / pi-web 的
//! `provider-credential-store` 共用同一把锁）。
//!
//! 锁对象 = `<target>.lock` **目录**：`create_dir` 成功即持锁，删除目录即释放。
//! stale：锁目录 mtime 超过 30s 视为孤儿（持锁进程已死），删除后接管。
//!
//! 为什么必须逐位对齐：pi 自己也在写 `auth.json`（OAuth 轮换是「锁内写完
//! refresh token 才放锁」的强约束），PF 用别的锁协议 = 两个写者互不设防。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// 协议规定的孤儿判定窗口（proper-lockfile 默认 30s）。
pub const STALE: Duration = Duration::from_secs(30);

pub struct FileLock {
    dir: PathBuf,
    held: bool,
}

impl FileLock {
    /// 长任务（> stale/2）持锁时续心跳；PF 的写入都在毫秒级，一般用不到。
    pub fn touch(&self) {
        if !self.held {
            return;
        }
        // 触摸目录：重写 mtime（create_dir 已存在会失败，用 file 方式兜底）
        let probe = self.dir.join(".touch");
        let _ = std::fs::write(&probe, b"");
        let _ = std::fs::remove_file(&probe);
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        if self.held {
            let _ = std::fs::remove_dir(&self.dir);
            self.held = false;
        }
    }
}

pub fn lock_path(target: &Path) -> PathBuf {
    let mut s = target.as_os_str().to_os_string();
    s.push(".lock");
    PathBuf::from(s)
}

fn age(dir: &Path) -> Option<Duration> {
    let modified = std::fs::metadata(dir).ok()?.modified().ok()?;
    SystemTime::now().duration_since(modified).ok()
}

/// 超时取锁（协议默认 stale 窗口）。
pub fn lock(target: &Path, timeout: Duration) -> Result<FileLock, String> {
    lock_with_stale(target, timeout, STALE)
}

/// 可注入 stale 窗口（单测用）。
pub fn lock_with_stale(
    target: &Path,
    timeout: Duration,
    stale: Duration,
) -> Result<FileLock, String> {
    if let Some(parent) = target.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let dir = lock_path(target);
    let start = Instant::now();
    let mut delay = Duration::from_millis(100);
    let mut took_over = false;
    loop {
        if std::fs::create_dir(&dir).is_ok() {
            return Ok(FileLock { dir, held: true });
        }
        // 孤儿锁：只接管一次，避免两个等待者互相删对方刚建的锁
        if !took_over {
            let stale_now = matches!(age(&dir), Some(a) if a > stale);
            if stale_now && std::fs::remove_dir_all(&dir).is_ok() {
                took_over = true;
                continue;
            }
        }
        let elapsed = start.elapsed();
        if elapsed >= timeout {
            return Err(format!("等待文件锁超时：{}", dir.display()));
        }
        std::thread::sleep(delay.min(timeout.saturating_sub(elapsed)));
        delay = (delay * 2).min(Duration::from_secs(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pf-lock-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("target.json")
    }

    #[test]
    fn lock_is_exclusive_and_released_on_drop() {
        let target = tmp("exclusive");
        let first = lock(&target, Duration::from_millis(500)).unwrap();
        let second = lock(&target, Duration::from_millis(250));
        assert!(second.is_err(), "第二把锁必须失败");
        let lock_dir = lock_path(&target);
        assert!(lock_dir.exists());
        drop(first);
        assert!(!lock_dir.exists(), "释放后锁目录必须消失");
        let third = lock(&target, Duration::from_millis(500)).unwrap();
        drop(third);
    }

    #[test]
    fn stale_lock_is_taken_over_once() {
        let target = tmp("stale");
        let lock_dir = lock_path(&target);
        std::fs::create_dir_all(&lock_dir).unwrap();
        // stale 窗口降到 1ms：等 20ms 即成为孤儿
        std::thread::sleep(Duration::from_millis(20));
        let guard = lock_with_stale(
            &target,
            Duration::from_millis(500),
            Duration::from_millis(1),
        )
        .expect("孤儿锁必须可接管");
        drop(guard);
        assert!(!lock_dir.exists());
    }

    #[test]
    fn lock_path_appends_suffix_without_extension() {
        let p = lock_path(Path::new("/tmp/a/b.json"));
        assert_eq!(p.file_name().unwrap(), "b.json.lock");
    }
}
