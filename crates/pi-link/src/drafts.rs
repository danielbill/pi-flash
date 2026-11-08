//! 新会话页草稿持久化（2026-11-07）。
//!
//! 「新会话页」= 每项目一枚稳定草稿（未晋级的 draft runtime，app 侧
//! `session_ops::new_session_with` 保证复用）。它的 InputPanel 输入跨
//! app 重启保留：草稿文本按 cwd 哈希落 `drafts/` 目录。
//!
//! - 只存文本。附件（pending_images）是 GPUI 渲染源指针（Arc），无法
//!   跨进程保留——水合回来只有文字。
//! - 写入时机（app 侧）：composer on_change 节流落（打字路）+
//!   switch_to 切走即落（flush 点）。发送晋级（on_file_bound）即删。
//! - 读取时机：草稿铸造时水合一次；复用既有草稿绝不回灌（内存里
//!   runtime.input 是活值真源）。
//! - cwd 字符串按打开时的原样参与哈希：同一项目换大小写重开视为
//!   新键（丢草稿可接受，v1 不做归一）。

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 落盘节流：打字路每键触发 on_change，实际写盘至多 3s 一次。
/// switch_to / 晋级删除是立即 flush 点，尾部丢失窗口 ≤ 3s。
const SAVE_THROTTLE: Duration = Duration::from_secs(3);

/// 上次节流落盘时刻（全局：PF 单主窗口，多窗口共享同一节流窗无害）
static LAST_SAVE: Mutex<Option<Instant>> = Mutex::new(None);

fn drafts_dir() -> Option<PathBuf> {
    crate::paths::dir().map(|d| d.join("drafts"))
}

/// cwd → 草稿文件路径。文件名 = SHA-256(cwd) 前 16 字节的 hex（32 字符，
/// Windows 路径合法）；内容 = 原文 UTF-8。
fn file_for(cwd: &str) -> Option<PathBuf> {
    use sha2::{Digest, Sha256};
    let hex: String = Sha256::digest(cwd.as_bytes())
        .iter()
        .take(16)
        .map(|b| format!("{b:02x}"))
        .collect();
    Some(drafts_dir()?.join(format!("{hex}.txt")))
}

/// 读草稿（空文件 / 缺文件 / 不可读 → None）
pub fn load(cwd: &str) -> Option<String> {
    load_at(&file_for(cwd)?)
}

/// 立即落盘（switch_to flush 点 / 兜底）
pub fn save(cwd: &str, text: &str) {
    if let Some(p) = file_for(cwd) {
        save_at(&p, text);
    }
}

/// 节流落盘（打字路每键触发）
pub fn save_throttled(cwd: &str, text: &str) {
    {
        let mut last = LAST_SAVE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(t) = *last {
            if t.elapsed() < SAVE_THROTTLE {
                return;
            }
        }
        *last = Some(Instant::now());
    }
    save(cwd, text);
}

/// 草稿退役（发送晋级成会话 = 输入已被消费）
pub fn clear(cwd: &str) {
    if let Some(p) = file_for(cwd) {
        let _ = std::fs::remove_file(p);
    }
}

fn load_at(p: &Path) -> Option<String> {
    std::fs::read_to_string(p).ok().filter(|s| !s.is_empty())
}

fn save_at(p: &Path, text: &str) {
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(p, text);
}

#[cfg(test)]
mod drafts_tests {
    use super::*;

    fn tmp_file(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pf-drafts-test-{}-{}", tag, std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        d.join("draft.txt")
    }

    #[test]
    fn roundtrip_and_clear() {
        let p = tmp_file("roundtrip");
        assert_eq!(load_at(&p), None);
        save_at(&p, "你好，帮我写个 TODO 应用\n第二行");
        assert_eq!(
            load_at(&p).as_deref(),
            Some("你好，帮我写个 TODO 应用\n第二行")
        );
        // 空文件读回 None（水合守卫）
        save_at(&p, "");
        assert_eq!(load_at(&p), None);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn file_name_stable_and_distinct() {
        let a = file_for("D:\\proj-a").unwrap();
        let b = file_for("D:\\proj-b").unwrap();
        let a2 = file_for("D:\\proj-a").unwrap();
        assert_eq!(a, a2, "同 cwd 同文件");
        assert_ne!(a, b, "不同 cwd 不同文件");
        let name = a.file_name().unwrap().to_str().unwrap();
        assert_eq!(name.len(), 36, "32 hex + .txt");
        assert!(name.ends_with(".txt"));
    }
}
