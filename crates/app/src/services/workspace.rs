//! Workspace memory + preferences (pi-web lib/workspace-memory.ts +
//! tab-session.ts parity): ~/.pi/agent/pi-flash-workspace.json holds the
//! last-open session per workspace, the globally-last workspace, and
//! language/sound preferences.

use std::path::Path;
use std::path::PathBuf;

/// Key holding the globally-last active workspace (startup target).
const WS_LAST_KEY: &str = "__last";

/// Persisted language preference (workspace memory file, global key).
const WS_LANG_KEY: &str = "__lang";

/// Persisted language preference (workspace memory file, global key).
const WS_SOUND_KEY: &str = "__sound";

/// Per-workspace "last open session" memory (pi-web workspace-memory parity).
/// Stored at ~/.pi/agent/pi-flash-workspace.json as { "<cwd>": "<session path>" };
/// an empty string means "this workspace was left on a blank new session".
fn workspace_memory_path() -> Option<PathBuf> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()?;
    Some(
        Path::new(&home)
            .join(".pi")
            .join("agent")
            .join("pi-flash-workspace.json"),
    )
}

fn load_workspace_memory() -> serde_json::Map<String, serde_json::Value> {
    let Some(path) = workspace_memory_path() else {
        return serde_json::Map::new();
    };
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(_) => return serde_json::Map::new(),
    };
    let parsed = match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(v) => v,
        Err(_) => return serde_json::Map::new(),
    };
    let mut out = serde_json::Map::new();
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

fn save_workspace_memory(map: &serde_json::Map<String, serde_json::Value>) {
    let Some(path) = workspace_memory_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(raw) = serde_json::to_string_pretty(map) {
        let _ = std::fs::write(path, raw);
    }
}

/// Normalized workspace key: Path components joined with "\\", so
/// "D:/a/b" and "D:\a\b" share one memory slot (Windows path parity).
fn ws_key(cwd: &str) -> String {
    // String-level normalization: "/" -> "\\", strip trailing separator,
    // case-fold (Windows paths are case-insensitive).
    let mut k = cwd.replace('/', "\\");
    while k.ends_with('\\') {
        k.pop();
    }
    k.to_ascii_lowercase()
}

/// Workspace-key-aware path equality.
pub fn same_ws(a: &str, b: &str) -> bool {
    ws_key(a) == ws_key(b)
}

/// Path equality via the same string-level normalization as same_ws
/// (Windows Path::components is not reusable as a key).
pub fn same_path(a: &Path, b: &Path) -> bool {
    same_ws(&a.to_string_lossy(), &b.to_string_lossy())
}

/// Remember `session_path` as the last open session for `cwd` and mark it
/// as the globally-last active workspace (startup restore target).
pub fn set_last_open(cwd: &str, session_path: &str) {
    let mut map = load_workspace_memory();
    let key = ws_key(cwd);
    map.insert(WS_LAST_KEY.to_string(), serde_json::Value::String(key.clone().into()));
    map.insert(key, serde_json::Value::String(session_path.into()));
    save_workspace_memory(&map);
}

pub fn load_sound_pref() -> bool {
    load_workspace_memory()
        .get(WS_SOUND_KEY)
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

pub fn save_sound_pref(on: bool) {
    let mut map = load_workspace_memory();
    map.insert(WS_SOUND_KEY.to_string(), serde_json::Value::Bool(on));
    save_workspace_memory(&map);
}

/// Agent-run finished notification sound (Windows MessageBeep; no-op elsewhere).
pub fn play_notify_sound() {
    #[cfg(windows)]
    {
        // MB_ICONASTERISK = 0x40 — the system "asterisk" notification sound
        const MB_ICONASTERISK: u32 = 0x0000_0040;
        unsafe {
            #[link(name = "user32")]
            unsafe extern "system" {
                fn MessageBeep(wtype: u32) -> i32;
            }
            MessageBeep(MB_ICONASTERISK);
        }
    }
}

pub fn load_lang_pref() -> Option<usize> {
    load_workspace_memory()
        .get(WS_LANG_KEY)
        .and_then(|v| v.as_u64())
        .map(|v| v.min(2) as usize)
}

pub fn save_lang_pref(ix: usize) {
    let mut map = load_workspace_memory();
    map.insert(WS_LANG_KEY.to_string(), serde_json::Value::Number((ix as u64).into()));
    save_workspace_memory(&map);
}

pub fn set_last_workspace(cwd: &str) {
    let mut map = load_workspace_memory();
    map.insert(
        WS_LAST_KEY.to_string(),
        serde_json::Value::String(ws_key(cwd).into()),
    );
    save_workspace_memory(&map);
}

/// The workspace the app was last used in, if known.
pub fn get_last_workspace() -> Option<String> {
    load_workspace_memory()
        .get(WS_LAST_KEY)?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Remember that `cwd` was left on a blank new session.
pub fn clear_last_open(cwd: &str) {
    let mut map = load_workspace_memory();
    map.insert(ws_key(cwd), serde_json::Value::String(String::new()));
    save_workspace_memory(&map);
}

/// The remembered session path for `cwd`, if any.
pub fn get_last_open(cwd: &str) -> Option<String> {
    load_workspace_memory()
        .get(&ws_key(cwd))?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}
