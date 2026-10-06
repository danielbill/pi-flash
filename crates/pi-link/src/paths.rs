//! pi-flash 自有文件路径 —— **不污染 pi**（010-启动.md §4）。
//!
//! pi-flash 自己写的文件全部落 `~/.pi-flash/`；`~/.pi/agent/` 只留给 pi 自己
//! （`sessions/`、`settings.json`、`models.json`、`models-store.json`、`skills/`、
//! `npm/`、`auth.json` …）。
//!
//! - 目录解析：`PI_FLASH_DIR` 优先（开发/测试隔离），否则 `%USERPROFILE%\.pi-flash`
//! - 旧文件迁移：老的 `~/.pi/agent/pi-flash-*.json` 在新路径缺失时原样 `rename`
//!   过来（不复制、不改内容）；新路径已存在则不动旧文件、不做清理。
//!   迁移必须在任何读者（sessions 扫描器 / recents 单例 / workspace 记忆缓存）
//!   首次读盘**之前**跑 —— 由 `startup` 在启动第一步调用。

use std::path::{Path, PathBuf};

/// pi-flash 配置目录（可能不存在）。
pub fn dir() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("PI_FLASH_DIR") {
        if !d.trim().is_empty() {
            return Some(PathBuf::from(d));
        }
    }
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).ok()?;
    Some(Path::new(&home).join(".pi-flash"))
}

/// pi-flash 配置目录，写前确保存在。
pub fn ensure_dir() -> Option<PathBuf> {
    let d = dir()?;
    let _ = std::fs::create_dir_all(&d);
    Some(d)
}

/// 自有文件全路径（目录不存在也返回路径，写方负责 create_dir_all）。
fn file(name: &str) -> Option<PathBuf> {
    Some(dir()?.join(name))
}

/// `~/.pi-flash/workspace.json` — 工作区记忆（每项目 last_open、布局、UI 状态）。
pub fn workspace_file() -> Option<PathBuf> {
    file("workspace.json")
}

/// `~/.pi-flash/app-settings.json` — 主题/语言/提示音/字体/加载窗口/恢复开关。
pub fn app_settings_file() -> Option<PathBuf> {
    file("app-settings.json")
}

/// `~/.pi-flash/session-index.json` — 会话摘要指纹索引（tail-seek 缓存）。
pub fn session_index_file() -> Option<PathBuf> {
    file("session-index.json")
}

/// `~/.pi-flash/session-recents.json` — 最近活跃会话清单（加载窗口过滤用）。
pub fn session_recents_file() -> Option<PathBuf> {
    file("session-recents.json")
}

/// `~/.pi-flash/catalog-cache.json` — 模型目录 + 扩展命令缓存（010-启动.md §4.1）。
pub fn catalog_cache_file() -> Option<PathBuf> {
    file("catalog-cache.json")
}

/// 迁移表：新文件名 ← 旧文件名（`~/.pi/agent/pi-flash-<旧>`）。
const MIGRATION: &[(&str, &str)] = &[
    ("workspace.json", "workspace.json"),
    ("app-settings.json", "app-settings.json"),
    ("session-index.json", "session-index.json"),
    ("session-recents.json", "session-recents.json"),
];
/// 旧目录：`~/.pi/agent`（迁移源目录）。
fn legacy_dir() -> Option<PathBuf> {
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).ok()?;
    Some(Path::new(&home).join(".pi").join("agent"))
}

/// 迁移实现（目录显式注入，便于测试且不碰进程环境）：逐个把旧文件原样搬过来。
fn migrate_into(new_dir: &Path, legacy_dir: &Path) -> usize {
    let mut moved = 0;
    for (new_name, legacy_name) in MIGRATION {
        let new_path = new_dir.join(new_name);
        if new_path.exists() {
            continue;
        }
        let old_path = legacy_dir.join(format!("pi-flash-{legacy_name}"));
        if !old_path.is_file() {
            continue;
        }
        if std::fs::rename(&old_path, &new_path).is_ok() {
            moved += 1;
        }
    }
    moved
}

/// 一次性迁移（启动第一步）：新文件缺失且旧文件存在 → 原样 `rename`。
/// 返回搬过来的文件数。任何一步失败都只是少搬一个文件，不阻塞启动。
pub fn migrate_legacy_files() -> usize {
    let Some(dir) = ensure_dir() else { return 0 };
    let Some(old_dir) = legacy_dir() else { return 0 };
    migrate_into(&dir, &old_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 迁移的四个动作：搬家成功 / 目标已在则跳过 / 旧文件缺失则跳过 / rename 而非复制。
    /// 目录显式注入，不碰进程环境（并行测试安全）。
    #[test]
    fn migration_moves_legacy_files_without_touching_env() {
        let base = std::env::temp_dir().join(format!(
            "pi-link-paths-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let new_dir = base.join(".pi-flash");
        let old_dir = base.join(".pi").join("agent");
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::write(old_dir.join("pi-flash-workspace.json"), "{\"old\":1}").unwrap();
        // 目标已存在的那个：旧文件必须保留、新内容不能被覆盖
        std::fs::write(old_dir.join("pi-flash-session-recents.json"), "{\"old\":1}").unwrap();
        std::fs::create_dir_all(&new_dir).unwrap();
        std::fs::write(new_dir.join("session-recents.json"), "{\"new\":1}").unwrap();

        let moved = migrate_into(&new_dir, &old_dir);
        assert_eq!(moved, 1, "只有 workspace.json 该搬（recents 目标已在）");
        assert_eq!(
            std::fs::read_to_string(new_dir.join("workspace.json")).unwrap(),
            "{\"old\":1}"
        );
        assert!(
            !old_dir.join("pi-flash-workspace.json").exists(),
            "旧文件是 rename 搬家，不是复制"
        );
        assert!(old_dir.join("pi-flash-session-recents.json").exists(), "目标已在则跳过");
        assert_eq!(
            std::fs::read_to_string(new_dir.join("session-recents.json")).unwrap(),
            "{\"new\":1}"
        );
        // 幂等：再跑一次没有任何可搬的
        assert_eq!(migrate_into(&new_dir, &old_dir), 0);
        let _ = std::fs::remove_dir_all(&base);
    }
}
