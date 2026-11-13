//! `pfctx.log`：pf-context 观测日志（S7，beads pi-flash-h6ai）。
//!
//! **验收免人肉**：启动、写盘、外部重载、投影、problem、domain 开库全部
//! 追加进 `<PF数据目录>/pfctx.log`，真机验收 = 读这一个文件（tail 即可）。
//!
//! 量级：每次**实际**写盘一行（set/flush 的 coalesce 保证低频）、每次重载
//! 一行——不是每次 set/set_preview（内存操作不打日志）。文件超 [`MAX_BYTES`]
//! 滚动为 `pfctx.log.old`（open 时判定，不占写路径）；纯内存态（无数据目录）
//! Logger 为 no-op，测试不打盘。
//!
//! stdout 侧维持原样（`[settings]` / `[perf]` eprintln 走各自门控），本文件
//! 是全量留痕、不受 `PI_FLASH_PERF` 门控。

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

/// 滚动阈值（2MB：按低频写法约数月一滚，滚动在 open 时做、零写路径成本）。
const MAX_BYTES: u64 = 2 * 1024 * 1024;
const NAME: &str = "pfctx.log";

/// 追加式日志器（Hub 级共享：settings/state/domain 同一份）。
#[derive(Default)]
pub(crate) struct Logger {
    /// `None` = 无数据目录（纯内存态）或打开失败（观测日志缺失不碍功能）
    file: Mutex<Option<File>>,
}

impl Logger {
    /// 打开（目录不存在则创建；超限滚动）。`None` = 纯内存态 no-op。
    pub(crate) fn open(dir: Option<&Path>) -> Logger {
        let Some(dir) = dir else {
            return Logger::default();
        };
        // 父目录不存在（临时目录被清理/测试收尾）：不重建，静默 no-op，
        // 否则观测日志会复活已删的测试临时目录。
        if !dir.is_dir() {
            return Logger::default();
        }
        let path = dir.join(NAME);
        if let Ok(md) = std::fs::metadata(&path) {
            if md.len() > MAX_BYTES {
                let old = dir.join("pfctx.log.old");
                let _ = std::fs::remove_file(&old);
                let _ = std::fs::rename(&path, old);
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok();
        let logger = Logger {
            file: Mutex::new(file),
        };
        logger.log(
            "boot",
            &format!(
                "pf-context v{} 打开，settings 目录={}（日志滚动阈值 {}MB）",
                env!("CARGO_PKG_VERSION"),
                dir.display(),
                MAX_BYTES / 1024 / 1024
            ),
        );
        logger
    }

    /// 追加一行（`[RFC3339 ms] [tag] msg`）。失败静默（观测不碍功能）。
    pub(crate) fn log(&self, tag: &str, msg: &str) {
        let Ok(mut guard) = self.file.lock() else {
            return;
        };
        let Some(f) = guard.as_mut() else {
            return;
        };
        let line = format!("[{}] [{}] {}\n", rfc3339_now(), tag, msg);
        let _ = f.write_all(line.as_bytes());
    }
}

fn rfc3339_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::tmp_dir;

    #[test]
    fn writes_tagged_lines_with_timestamps() {
        let dir = tmp_dir("log-writes");
        let logger = Logger::open(Some(&dir));
        logger.log("write", "subagents {\"capacity\":6}");
        logger.log("watch", "外部改动 → 重载完成");
        let text = std::fs::read_to_string(dir.join(NAME)).unwrap();
        assert!(text.contains("[boot] pf-context v"), "{text}");
        assert!(text.contains("[write] subagents"), "{text}");
        assert!(text.contains("[watch] 外部改动"), "{text}");
        // RFC3339 毫秒时间戳
        let line = text.lines().nth(1).unwrap();
        assert!(
            line.starts_with("[20") && line.contains('T') && line.contains('Z'),
            "{line}"
        );
    }

    #[test]
    fn oversize_file_rotates_to_old_on_open() {
        let dir = tmp_dir("log-rotate");
        std::fs::write(dir.join(NAME), vec![b'x'; (MAX_BYTES as usize) + 1]).unwrap();
        let _logger = Logger::open(Some(&dir));
        assert!(dir.join("pfctx.log.old").is_file(), "超限文件滚动为 .old");
        let fresh = std::fs::read(dir.join(NAME)).unwrap().len() as u64;
        assert!(fresh < MAX_BYTES, "新文件从 boot 行重新开始（{fresh}B）");
    }

    #[test]
    fn under_limit_file_is_appended_not_rotated() {
        let dir = tmp_dir("log-append");
        std::fs::write(dir.join(NAME), "previous\n").unwrap();
        let logger = Logger::open(Some(&dir));
        logger.log("write", "x");
        let text = std::fs::read_to_string(dir.join(NAME)).unwrap();
        assert!(text.starts_with("previous\n"), "未超限不滚动");
        assert!(text.contains("[write] x"));
        assert!(!dir.join("pfctx.log.old").exists());
    }

    #[test]
    fn no_dir_is_silent_noop() {
        let logger = Logger::open(None);
        logger.log("write", "ignored"); // 不 panic、不写任何文件
    }
}
