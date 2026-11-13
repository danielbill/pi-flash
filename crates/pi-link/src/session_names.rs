//! 会话名台账（PF 自有，pi-flash-n3k8 方案 B，pi-flash-pkhr）：会话改名在
//! PF 侧即时落账，显示优先级 = **PF 名 → 会话文件 session_info → 首条消息
//! 预览**；pi 侧只在会话进程在场时尽力同步一条 set_session_name，冷会话
//! 不再为改名 spawn 临时进程（2026-11-13 用户定案）。
//!
//! 文件：`<PF 数据目录>/session-names.json`
//! ```json
//! { "version": 1, "entries": [ { "path": "D:\\…\\x.jsonl", "name": "新名字" } ] }
//! ```
//!
//! 为什么不直接写 pi 会话文件：session_info 是 pi 的树账本条目（id/parentId
//! 语义归 pi 管），PF 手写 = 复制 pi 的条目格式，pi 升级即漂移。台账是
//! **显示层**真值：扫描器照旧读文件侧名字（pi CLI / pi-web 里改的名 PF
//! 同样看得到），PF 名只是显示层覆盖——台账丢失/换机器时文件侧名字还在，
//! 不丢数据。
//!
//! key 用**文件路径**不用 session id：PF 全仓身份都是路径（pool key /
//! recents / pins / hidden 同口径），且 pi fork 的分支文件 header 带
//! parentSession 同 id，路径才唯一。查找前按 hidden_sessions 口径归一
//! （`/`→`\` + 小写），扫描器形态与历史字符串形态命中同一行。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use serde_json::{Value, json};

use crate::paths;
use crate::sessions::SessionInfo;

/// 一行台账。`path` 存**原始形态**（prune 的 `is_file` 用真实路径判，
/// 不赌归一形态在各文件系统上的行为）。
#[derive(Debug, Clone)]
struct Row {
    path: PathBuf,
    name: String,
}

/// norm(path) → Row。
type Map = HashMap<String, Row>;

pub fn file_path() -> Option<PathBuf> {
    paths::dir().map(|d| d.join("session-names.json"))
}

fn norm(s: &str) -> String {
    s.replace('/', "\\").to_lowercase()
}

// ---------------------------------------------------------------------------
// IO（file 注入形，公开封装走 file_path()；测试用 temp 夹具直打内层）
// ---------------------------------------------------------------------------

fn load_map_at(file: &Path) -> Map {
    let Ok(text) = std::fs::read_to_string(file) else {
        return Map::new();
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return Map::new();
    };
    v["entries"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| {
                    let p = e["path"].as_str()?;
                    let n = e["name"].as_str()?.trim();
                    if n.is_empty() {
                        return None; // 空名不进账（= 清除语义，set 时已摘行）
                    }
                    let row = Row {
                        path: PathBuf::from(p),
                        name: n.to_string(),
                    };
                    Some((norm(p), row))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn save_map_at(file: &Path, map: &Map) {
    let entries: Vec<Value> = map
        .values()
        .map(|r| {
            json!({
                "path": r.path.to_string_lossy(),
                "name": r.name,
            })
        })
        .collect();
    let value = json!({ "version": 1, "entries": entries });
    // 自有目录可能还没建（首启/迁移后）——写前确保存在
    let _ = paths::ensure_dir();
    if let Err(err) = crate::config::write_json(file, &value) {
        eprintln!("[session-names] 写台账失败 {}: {err}", file.display());
    }
}

/// 死行对账：会话文件已不存在的条目摘除。挂在 `set` 里做（改名低频、
/// 账小，一次 stat/行），不在列表热路径上付。
fn prune_missing(map: &mut Map) {
    map.retain(|_, row| row.path.is_file());
}

fn set_at(file: &Path, path: &Path, name: &str) {
    let mut map = load_map_at(file);
    let key = norm(path.to_string_lossy().as_ref());
    let trimmed = name.trim();
    if trimmed.is_empty() {
        map.remove(&key); // 空名 = 清除（pi setSessionName 同语义）
    } else {
        map.insert(
            key,
            Row {
                path: path.to_path_buf(),
                name: trimmed.to_string(),
            },
        );
    }
    prune_missing(&mut map);
    save_map_at(file, &map);
    invalidate_cache();
}

fn remove_at(file: &Path, path: &Path) {
    let mut map = load_map_at(file);
    if map.remove(&norm(path.to_string_lossy().as_ref())).is_some() {
        save_map_at(file, &map);
        invalidate_cache();
    }
}

fn get_at(file: &Path, path: &Path) -> Option<String> {
    cached_map_at(file)
        .get(&norm(path.to_string_lossy().as_ref()))
        .map(|r| r.name.clone())
}

// ---------------------------------------------------------------------------
// 进程内缓存（mtime 门控）：overlay 挂在四个扫描出口上，消息每轮 ListDirty
// 都会过一遍，重复 parse 几 KB JSON 不值得——`(mtime,size)` 指纹没变就复用
// 上次解析。写方只有 PF 自己（每次写必变 mtime/大小），指纹失配自动重读。
// ---------------------------------------------------------------------------

type Fingerprint = (u64, u32, u64);

/// 模块级缓存槽（mtime 门控）：cached_map_at 读写、写路径 invalidate。
/// 键含**文件路径**：生产里 file_path() 恒同一路径，测试里多夹具并行，
/// 只按指纹会互串（两个不同文件指纹不可能相同纯属巧合，不能赌）。
static CACHE: Mutex<Option<(PathBuf, Fingerprint, Map)>> = Mutex::new(None);

fn fingerprint_of(meta: Option<&std::fs::Metadata>) -> Fingerprint {
    match meta.and_then(|m| m.modified().ok()) {
        Some(t) if t != UNIX_EPOCH => {
            let dur = t.duration_since(UNIX_EPOCH).unwrap_or_default();
            (
                dur.as_secs(),
                dur.subsec_nanos(),
                meta.map(|m| m.len()).unwrap_or(0),
            )
        }
        // 缺文件/epoch 归零：与真指纹撞不上（真实文件 mtime 不会是 0）
        _ => (0, 0, 0),
    }
}

fn cached_map_at(file: &Path) -> Map {
    let fp = fingerprint_of(std::fs::metadata(file).ok().as_ref());
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((cached_file, cached_fp, map)) = guard.as_ref() {
        if cached_file == file && *cached_fp == fp {
            return map.clone();
        }
    }
    let map = load_map_at(file);
    *guard = Some((file.to_path_buf(), fp, map.clone()));
    map
}

fn invalidate_cache() {
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    *guard = None;
}

// ---------------------------------------------------------------------------
// 公开 API
// ---------------------------------------------------------------------------

/// 改名落账（trim 后空名 = 摘行），顺带 prune 死行。
pub fn set(path: &Path, name: &str) {
    let Some(file) = file_path() else { return };
    set_at(&file, path, name);
}

/// 摘行（会话删除 / 052 清理）。无行时不动文件。
pub fn remove(path: &Path) {
    let Some(file) = file_path() else { return };
    remove_at(&file, path);
}

/// 查询（RenameReady 预填等显示场景）。
pub fn get(path: &Path) -> Option<String> {
    let file = file_path()?;
    get_at(&file, path)
}

/// 扫描出口统一覆盖（read.rs 四个 list 函数）：PF 名优先，无行/空名不动
/// ——文件侧 session_info 照旧透出。
pub fn overlay(infos: &mut [SessionInfo]) {
    if infos.is_empty() {
        return;
    }
    let Some(file) = file_path() else { return };
    let map = cached_map_at(&file);
    if map.is_empty() {
        return;
    }
    overlay_map(&map, infos);
}

fn overlay_map(map: &Map, infos: &mut [SessionInfo]) {
    for s in infos.iter_mut() {
        if let Some(row) = map.get(&norm(s.path.to_string_lossy().as_ref())) {
            let n = row.name.trim();
            if !n.is_empty() {
                s.name = Some(n.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::testing::temp_root;
    use std::time::SystemTime;

    fn info(path: &Path, name: Option<&str>) -> SessionInfo {
        SessionInfo {
            path: path.to_path_buf(),
            id: "id-1".into(),
            cwd: "D:\\proj".into(),
            modified: SystemTime::now(),
            preview: "首条消息".into(),
            message_count: 3,
            name: name.map(str::to_string),
        }
    }

    #[test]
    fn set_get_remove_roundtrip_and_norm_key() {
        let root = temp_root("snames");
        let file = root.join("names.json");
        let sess = root.join("a.jsonl");
        std::fs::write(&sess, "{}").unwrap();

        set_at(&file, &sess, "  ST股分析  ");
        // 归一命中：大小写 + 分隔符漂移（同一文件的正斜杠写法）都能查到
        // 同一行；norm 只归一分隔符/大小写，不归一目录结构
        assert_eq!(
            get_at(&file, &root.join("A.JSONL")),
            Some("ST股分析".into())
        );
        let slashed = PathBuf::from(root.to_string_lossy().replace('\\', "/") + "/a.jsonl");
        assert_eq!(get_at(&file, &slashed), Some("ST股分析".into()));
        // 回读文件结构
        let map = load_map_at(&file);
        assert_eq!(map.len(), 1);
        assert_eq!(map.values().next().unwrap().name, "ST股分析");

        remove_at(&file, &sess);
        assert_eq!(get_at(&file, &sess), None);
        assert!(load_map_at(&file).is_empty());
        // 无行可摘 = 不写文件（从未存在过的账不落盘）
        let fresh = root.join("fresh.json");
        remove_at(&fresh, &sess);
        assert!(!fresh.exists());
    }

    #[test]
    fn empty_name_clears_row() {
        let root = temp_root("snames-empty");
        let file = root.join("names.json");
        let sess = root.join("a.jsonl");
        std::fs::write(&sess, "{}").unwrap();

        set_at(&file, &sess, "名字");
        set_at(&file, &sess, "   "); // trim 后空 = 摘行
        assert_eq!(get_at(&file, &sess), None);
        assert!(load_map_at(&file).is_empty());
    }

    #[test]
    fn set_prunes_dead_rows_but_keeps_live() {
        let root = temp_root("snames-prune");
        let file = root.join("names.json");
        let live = root.join("live.jsonl");
        std::fs::write(&live, "{}").unwrap();
        let dead = root.join("gone.jsonl"); // 从不创建

        // 死行在 set 当场就被 prune 摘掉（每次 set 自带死行对账）
        set_at(&file, &dead, "死行");
        assert_eq!(get_at(&file, &dead), None);
        // 活行保留，账里只剩一行
        set_at(&file, &live, "活行");
        assert_eq!(get_at(&file, &live), Some("活行".into()));
        assert_eq!(load_map_at(&file).len(), 1);
    }

    #[test]
    fn overlay_prefers_pf_name_and_keeps_file_name() {
        let root = temp_root("snames-overlay");
        let renamed = root.join("r.jsonl");
        let untouched = root.join("u.jsonl");
        let mut infos = vec![
            info(&renamed, Some("文件侧旧名".into())),
            info(&untouched, Some("pi 改的名".into())),
        ];
        let mut map = Map::new();
        map.insert(
            norm(renamed.to_string_lossy().as_ref()),
            Row {
                path: renamed.clone(),
                name: "PF 新名".into(),
            },
        );
        overlay_map(&map, &mut infos);
        // 有台账行：PF 名覆盖文件侧名
        assert_eq!(infos[0].name.as_deref(), Some("PF 新名"));
        // 无台账行：文件侧名原样透出（pi CLI / pi-web 改的名不丢）
        assert_eq!(infos[1].name.as_deref(), Some("pi 改的名"));
    }

    #[test]
    fn fingerprint_distinguishes_missing_and_present() {
        let root = temp_root("snames-fp");
        let missing = root.join("nope.json");
        assert_eq!(
            fingerprint_of(std::fs::metadata(&missing).ok().as_ref()),
            (0, 0, 0)
        );
        std::fs::write(&missing, "{}").unwrap();
        assert_ne!(
            fingerprint_of(std::fs::metadata(&missing).ok().as_ref()),
            (0, 0, 0)
        );
    }
}
