//! Workspace 持久化存储层（ARCHITECTURE 三层配置边界）：唯一碰 IO 的层——
//! workspace.json 的 map 读写管线（原子写 + 进程内缓存）+ ws 归一化/路径工具。
//! 上层 ws_state / app_settings 只经这里落盘。自 workspace.rs 拆出（090 重构）。

use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::Value;

pub(crate) const WS_LAST_KEY: &str = "__last";

/// Persisted language preference (legacy home: workspace memory file; the
/// canonical home is now app_settings.json, loads fall back to this key).
pub(crate) const WS_LANG_KEY: &str = "__lang";

/// Persisted sound preference (legacy home, same migration as lang).
pub(crate) const WS_SOUND_KEY: &str = "__sound";

/// 内容区 tab 带存档（`__tabs`）：一个项目关停/离开时的自由 tab 清单 +
/// 激活位 + 前台视图。启动恢复（startup::spawn_boot_restore）按 cwd 读回。
pub(crate) const WS_TABS_KEY: &str = "__tabs";

/// tab 带存档条目上限（防手滑开几十个终端后重启 = 几十个 conpty 进程，
/// 启动铁律不许）。超限截尾（先保打开序靠前的）。
pub const TABS_SAVE_CAP: usize = 32;

// ---------------------------------------------------------------------------
// core: path-injected map IO (tests use these directly; no cache)
// ---------------------------------------------------------------------------

pub(crate) fn load_map_from(path: &Path) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    let Ok(raw) = std::fs::read_to_string(path) else {
        return out;
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&raw) else {
        return out;
    };
    if let Some(obj) = parsed.as_object() {
        for (k, v) in obj {
            // `__last` stays as-is; workspace keys get normalized once.
            if k == WS_LAST_KEY {
                out.insert(k.clone(), v.clone());
            } else {
                out.insert(ws_key(k), v.clone());
            }
        }
    }
    out
}

/// Plain object load without workspace-key normalization (app_settings.json
/// keys are setting names, not paths).
pub(crate) fn load_plain_map_from(path: &Path) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    let Ok(raw) = std::fs::read_to_string(path) else {
        return out;
    };
    if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(&raw) {
        out = obj;
    }
    out
}

/// Atomic map write (tmp + rename through pi_link::config::write_json) with
/// a no-op skip when the serialized content is unchanged.
pub(crate) fn save_map_to(path: &Path, map: &serde_json::Map<String, Value>) -> bool {
    let Ok(new_raw) = serde_json::to_string_pretty(map) else {
        return false;
    };
    if let Ok(current) = std::fs::read_to_string(path) {
        if current == new_raw {
            return false;
        }
    }
    // 自有目录可能还没建（首启 / 迁移后）——写前确保存在
    let _ = pi_link::paths::ensure_dir();
    let value = Value::Object(map.clone());
    pi_link::config::write_json(path, &value).is_ok()
}

/// Normalized workspace key: Path components joined with "\\", so
/// "D:/a/b" and "D:\a\b" share one memory slot (Windows path parity).
pub(crate) fn ws_key(cwd: &str) -> String {
    // String-level normalization: "/" -> "\\", strip trailing separator,
    // case-fold (Windows paths are case-insensitive).
    let mut k = cwd.replace('/', "\\");
    while k.ends_with('\\') {
        k.pop();
    }
    k.to_ascii_lowercase()
}

// ---------------------------------------------------------------------------
// workspace memory (read once per process, cached)
// ---------------------------------------------------------------------------

pub(crate) fn memory_path() -> Option<PathBuf> {
    // pi-flash 自有目录（010-启动.md §4）：不再往 ~/.pi/agent 写
    pi_link::paths::workspace_file()
}

/// Shared in-process cache for the workspace memory map (one static for
/// both read and write paths — startup used to hit the file four times:
/// lang, sound, last workspace, last open).
pub(crate) fn with_memory_cache<T>(
    f: impl FnOnce(&mut Option<serde_json::Map<String, Value>>) -> T,
) -> T {
    static CACHE: Mutex<Option<serde_json::Map<String, Value>>> = Mutex::new(None);
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut guard)
}

pub(crate) fn memory() -> serde_json::Map<String, Value> {
    with_memory_cache(|cache| {
        if cache.is_none() {
            let map = memory_path().map(|p| load_map_from(&p)).unwrap_or_default();
            *cache = Some(map);
        }
        cache.as_ref().expect("cache just filled").clone()
    })
}

/// Update cache + atomic file write. Returns true when the file changed.
pub(crate) fn set_memory(map: &serde_json::Map<String, Value>) -> bool {
    with_memory_cache(|cache| *cache = Some(map.clone()));
    match memory_path() {
        Some(p) => save_map_to(&p, map),
        None => false,
    }
}

/// Workspace-key-aware path equality.
pub fn same_ws(a: &str, b: &str) -> bool {
    ws_key(a) == ws_key(b)
}

/// Public ws-key conversion (psp collapsed-group persistence).
pub fn same_ws_key(cwd: &str) -> String {
    ws_key(cwd)
}

/// Path equality via the same string-level normalization as same_ws
/// (Windows Path::components is not reusable as a key).
pub fn same_path(a: &Path, b: &Path) -> bool {
    same_ws(&a.to_string_lossy(), &b.to_string_lossy())
}

/// 项目行/组名：目录名，无尾件（根盘等）回落全路径。
pub fn project_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

/// `path` 是否在 `root` 之下（含 root 本身）：与 same_path 同一套字符串级
/// 规范化（分隔符统一为 `\`、去尾分隔符、大小写折叠），所以 Windows 上
/// `d:/a\b` 与 `D:\A` 的层级关系判得对。023 用它决定「这个目录是否已被
/// workspace 的递归 watch 覆盖」，从而决定要不要给 cwd 外文件单挂监听。
pub fn is_under(path: &Path, root: &Path) -> bool {
    let p = ws_key(&path.to_string_lossy());
    let r = ws_key(&root.to_string_lossy());
    p == r || p.starts_with(&format!("{r}\\"))
}

/// 磁盘改动指纹 (mtime, len)（023 外部改动检测基准）：打开/保存/确认时
/// 记录，fs 泵信号到达时对比。mtime 不支持（罕见 FS）返回 None = 永不误报。
pub fn file_sig(path: &Path) -> Option<(std::time::SystemTime, u64)> {
    let md = std::fs::metadata(path).ok()?;
    Some((md.modified().ok()?, md.len()))
}

/// 应用自有目录（载荷目录 / npm 包根，见 `pi_link::paths::is_app_install_path`）
/// 的 cwd 不进工作区记忆：0.1.x 更新交接 spawn 不设 cwd，新进程继承旧载荷
/// 目录曾被记成 `__last`，之后每次启动都被恢复成 payload 项目。
pub(crate) fn app_owned_cwd(cwd: &str) -> bool {
    pi_link::paths::is_app_install_path(Path::new(cwd))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpfile(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pi-flash-ws-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }
    #[test]
    fn map_io_roundtrip_and_noop_skip() {
        let path = tmpfile("ws.json");
        let mut map = serde_json::Map::new();
        map.insert("k".into(), Value::String("v".into()));
        assert!(save_map_to(&path, &map));
        // identical content -> skipped write
        assert!(!save_map_to(&path, &map));
        let loaded = load_map_from(&path);
        assert_eq!(loaded.get("k").unwrap(), "v");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
    #[test]
    fn atomic_write_leaves_no_tmp() {
        let path = tmpfile("atomic.json");
        let mut map = serde_json::Map::new();
        map.insert("a".into(), Value::Bool(true));
        save_map_to(&path, &map);
        assert!(path.is_file());
        assert!(
            !path.with_extension("json.tmp").exists(),
            "tmp renamed away"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
    #[test]
    fn is_under_normalizes_separators_and_case() {
        // cwd 内（正/反斜杠、大小写都要判对）
        assert!(is_under(Path::new("D:\\proj\\src"), Path::new("D:\\proj")));
        assert!(is_under(Path::new("d:/proj/src"), Path::new("D:\\PROJ")));
        assert!(is_under(Path::new("D:\\proj"), Path::new("D:\\proj\\")));
        // 前缀像但不是子目录（proj2 不能被 proj 覆盖）
        assert!(!is_under(Path::new("D:\\proj2"), Path::new("D:\\proj")));
        // 工作区外（另一个盘/目录）
        assert!(!is_under(
            Path::new("D:\\my_obsidian\\vault"),
            Path::new("D:\\ai_workspace")
        ));
    }
}
