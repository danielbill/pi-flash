//! Workspace state + app settings persistence.
//!
//! Three-layer config boundary (ARCHITECTURE.md): pi's own `settings.json`
//! (models/credentials/tools — pi's domain) / `pi-flash-app-settings.json`
//! (app appearance: theme, icon theme, fonts, plus lang/sound preferences,
//! 006 界面设置) / `pi-flash-workspace.json` (runtime state: last-open
//! session per workspace, globally-last workspace, window/dock layout).
//!
//! IO rules: every write is atomic (tmp + rename via pi_link::config),
//! each file is read once per process (in-process cache), and saves that
//! would write identical content are skipped.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::Value;

/// Key holding the globally-last active workspace (startup target).
const WS_LAST_KEY: &str = "__last";

/// Persisted language preference (legacy home: workspace memory file; the
/// canonical home is now app_settings.json, loads fall back to this key).
const WS_LANG_KEY: &str = "__lang";

/// Persisted sound preference (legacy home, same migration as lang).
const WS_SOUND_KEY: &str = "__sound";

/// Window bounds / maximized flag (schema landed in phase A, consumed by
/// the phase-D shell).
const WS_WINDOW_KEY: &str = "__window";

/// Function-panel dock layout: position (left|right), active view, width.
const WS_DOCK_KEY: &str = "__dock";

// ---------------------------------------------------------------------------
// core: path-injected map IO (tests use these directly; no cache)
// ---------------------------------------------------------------------------

fn load_map_from(path: &Path) -> serde_json::Map<String, Value> {
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
fn load_plain_map_from(path: &Path) -> serde_json::Map<String, Value> {
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
fn save_map_to(path: &Path, map: &serde_json::Map<String, Value>) -> bool {
    let Ok(new_raw) = serde_json::to_string_pretty(map) else {
        return false;
    };
    if let Ok(current) = std::fs::read_to_string(path) {
        if current == new_raw {
            return false;
        }
    }
    let value = Value::Object(map.clone());
    pi_link::config::write_json(path, &value).is_ok()
}

fn agent_dir_file(name: &str) -> Option<PathBuf> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()?;
    Some(Path::new(&home).join(".pi").join("agent").join(name))
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

// ---------------------------------------------------------------------------
// workspace memory (read once per process, cached)
// ---------------------------------------------------------------------------

fn memory_path() -> Option<PathBuf> {
    agent_dir_file("pi-flash-workspace.json")
}

/// Shared in-process cache for the workspace memory map (one static for
/// both read and write paths — startup used to hit the file four times:
/// lang, sound, last workspace, last open).
fn with_memory_cache<T>(f: impl FnOnce(&mut Option<serde_json::Map<String, Value>>) -> T) -> T {
    static CACHE: Mutex<Option<serde_json::Map<String, Value>>> = Mutex::new(None);
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut guard)
}

fn memory() -> serde_json::Map<String, Value> {
    with_memory_cache(|cache| {
        if cache.is_none() {
            let map = memory_path()
                .map(|p| load_map_from(&p))
                .unwrap_or_default();
            *cache = Some(map);
        }
        cache.as_ref().expect("cache just filled").clone()
    })
}

/// Update cache + atomic file write. Returns true when the file changed.
fn set_memory(map: &serde_json::Map<String, Value>) -> bool {
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

/// Path equality via the same string-level normalization as same_ws
/// (Windows Path::components is not reusable as a key).
pub fn same_path(a: &Path, b: &Path) -> bool {
    same_ws(&a.to_string_lossy(), &b.to_string_lossy())
}

/// Remember `session_path` as the last open session for `cwd` and mark it
/// as the globally-last active workspace (startup restore target).
pub fn set_last_open(cwd: &str, session_path: &str) {
    let mut map = memory();
    let key = ws_key(cwd);
    map.insert(WS_LAST_KEY.to_string(), Value::String(key.clone().into()));
    map.insert(key, Value::String(session_path.into()));
    set_memory(&map);
}

pub fn load_sound_pref() -> bool {
    app_settings().sound.unwrap_or_else(|| {
        memory()
            .get(WS_SOUND_KEY)
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    })
}

pub fn save_sound_pref(on: bool) {
    let mut s = app_settings();
    s.sound = Some(on);
    save_app_settings(&s);
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
    app_settings().lang.or_else(|| {
        memory()
            .get(WS_LANG_KEY)
            .and_then(|v| v.as_u64())
            .map(|v| v.min(2) as usize)
    })
}

pub fn save_lang_pref(ix: usize) {
    let mut s = app_settings();
    s.lang = Some(ix);
    save_app_settings(&s);
}

pub fn set_last_workspace(cwd: &str) {
    let mut map = memory();
    map.insert(
        WS_LAST_KEY.to_string(),
        Value::String(ws_key(cwd).into()),
    );
    set_memory(&map);
}

/// The workspace the app was last used in, if known.
pub fn get_last_workspace() -> Option<String> {
    memory()
        .get(WS_LAST_KEY)?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Remember that `cwd` was left on a blank new session.
pub fn clear_last_open(cwd: &str) {
    let mut map = memory();
    map.insert(ws_key(cwd), Value::String(String::new()));
    set_memory(&map);
}

/// The remembered session path for `cwd`, if any.
pub fn get_last_open(cwd: &str) -> Option<String> {
    memory()
        .get(&ws_key(cwd))?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

// ---------------------------------------------------------------------------
// window / dock layout state (schema landed in phase A, consumed phase D)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // phase D: shell restores window bounds from here
pub struct WindowState {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub maximized: bool,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // phase D: function panel restores dock position/view
pub struct DockState {
    /// "left" | "right"
    pub position: String,
    /// "sessions" | "files" | "git" | "terminal"
    pub panel: String,
    pub width: f32,
}

#[allow(dead_code)] // phase D consumers
pub fn get_window_state() -> Option<WindowState> {
    let map = memory();
    let v = map.get(WS_WINDOW_KEY)?;
    Some(WindowState {
        x: v.get("x")?.as_f64()?,
        y: v.get("y")?.as_f64()?,
        w: v.get("w")?.as_f64()?,
        h: v.get("h")?.as_f64()?,
        maximized: v.get("max").and_then(|m| m.as_bool()).unwrap_or(false),
    })
}

#[allow(dead_code)] // phase D consumers
pub fn save_window_state(s: &WindowState) {
    let mut map = memory();
    map.insert(
        WS_WINDOW_KEY.to_string(),
        serde_json::json!({
            "x": s.x, "y": s.y, "w": s.w, "h": s.h, "max": s.maximized,
        }),
    );
    set_memory(&map);
}

#[allow(dead_code)] // phase D consumers
pub fn get_dock_state() -> Option<DockState> {
    let map = memory();
    let v = map.get(WS_DOCK_KEY)?;
    Some(DockState {
        position: v.get("pos")?.as_str()?.to_string(),
        panel: v.get("panel")?.as_str()?.to_string(),
        width: v.get("width")?.as_f64()? as f32,
    })
}

#[allow(dead_code)] // phase D consumers
pub fn save_dock_state(s: &DockState) {
    let mut map = memory();
    map.insert(
        WS_DOCK_KEY.to_string(),
        serde_json::json!({
            "pos": s.position, "panel": s.panel, "width": s.width,
        }),
    );
    set_memory(&map);
}

// ---------------------------------------------------------------------------
// app settings (006 界面设置): theme / icon theme / fonts + lang / sound
// ---------------------------------------------------------------------------

/// One font slot of the appearance settings (006): family + pt size.
/// `None` fields mean "unset" so defaults stay code-side.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // phase D: appearance module reads these
pub struct FontSpec {
    pub family: String,
    pub size: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[allow(dead_code)] // phase D: appearance module + settings panel read these
pub struct AppSettings {
    /// theme id ("mist" / "rose" / "one-light" / ... 006 built-ins)
    pub theme: Option<String>,
    /// icon theme id (built-in set is pi-web's own icons)
    pub icon_theme: Option<String>,
    /// chat/session font (≈ zed UI font)
    pub session_font: Option<FontSpec>,
    /// function-panel font
    pub panel_font: Option<FontSpec>,
    /// markdown preview font
    pub markdown_font: Option<FontSpec>,
    /// language index (i18n::TABLE rows)
    pub lang: Option<usize>,
    /// notification sound on/off
    pub sound: Option<bool>,
}

fn app_settings_path() -> Option<PathBuf> {
    agent_dir_file("pi-flash-app-settings.json")
}

fn font_spec_from(v: &Value) -> Option<FontSpec> {
    Some(FontSpec {
        family: v.get("family")?.as_str()?.to_string(),
        size: v.get("size")?.as_f64()? as f32,
    })
}

fn font_spec_to(spec: &FontSpec) -> Value {
    serde_json::json!({ "family": spec.family, "size": spec.size })
}

/// One shared cache for app settings (single static for reads and writes).
fn with_app_settings_cache<T>(f: impl FnOnce(&mut Option<AppSettings>) -> T) -> T {
    static CACHE: Mutex<Option<AppSettings>> = Mutex::new(None);
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut guard)
}

/// Load app_settings.json; reads each file once per process. Absent
/// lang/sound fall back to their legacy workspace-memory keys.
pub fn app_settings() -> AppSettings {
    with_app_settings_cache(|cache| {
        if let Some(s) = cache.as_ref() {
            return s.clone();
        }
        let s = match app_settings_path() {
            Some(p) => {
                let map = load_plain_map_from(&p);
                AppSettings {
                    theme: map.get("theme").and_then(|v| v.as_str()).map(str::to_string),
                    icon_theme: map
                        .get("icon_theme")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    session_font: map.get("session_font").and_then(font_spec_from),
                    panel_font: map.get("panel_font").and_then(font_spec_from),
                    markdown_font: map.get("markdown_font").and_then(font_spec_from),
                    lang: map
                        .get("lang")
                        .and_then(|v| v.as_u64())
                        .map(|v| v.min(2) as usize),
                    sound: map.get("sound").and_then(|v| v.as_bool()),
                }
            }
            None => AppSettings::default(),
        };
        *cache = Some(s.clone());
        s
    })
}

/// Atomic save + cache update; skips the write when nothing changed.
pub fn save_app_settings(s: &AppSettings) {
    with_app_settings_cache(|cache| *cache = Some(s.clone()));
    let Some(path) = app_settings_path() else { return };
    let mut obj = serde_json::Map::new();
    if let Some(v) = &s.theme {
        obj.insert("theme".into(), Value::String(v.clone()));
    }
    if let Some(v) = &s.icon_theme {
        obj.insert("icon_theme".into(), Value::String(v.clone()));
    }
    if let Some(v) = &s.session_font {
        obj.insert("session_font".into(), font_spec_to(v));
    }
    if let Some(v) = &s.panel_font {
        obj.insert("panel_font".into(), font_spec_to(v));
    }
    if let Some(v) = &s.markdown_font {
        obj.insert("markdown_font".into(), font_spec_to(v));
    }
    if let Some(v) = s.lang {
        obj.insert("lang".into(), Value::Number((v as u64).into()));
    }
    if let Some(v) = s.sound {
        obj.insert("sound".into(), Value::Bool(v));
    }
    save_map_to(&path, &obj);
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

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
        assert!(!path.with_extension("json.tmp").exists(), "tmp renamed away");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn window_dock_state_roundtrip_via_map() {
        let path = tmpfile("layout.json");
        let mut map = serde_json::Map::new();
        map.insert(
            WS_WINDOW_KEY.into(),
            serde_json::json!({"x": 10.0, "y": 20.0, "w": 1180.0, "h": 760.0, "max": true}),
        );
        map.insert(
            WS_DOCK_KEY.into(),
            serde_json::json!({"pos": "right", "panel": "git", "width": 300.0}),
        );
        save_map_to(&path, &map);
        let loaded = load_map_from(&path);
        let w = loaded.get(WS_WINDOW_KEY).unwrap();
        assert_eq!(w.get("max").unwrap(), &Value::Bool(true));
        assert_eq!(w.get("w").unwrap(), &serde_json::json!(1180.0));
        let d = loaded.get(WS_DOCK_KEY).unwrap();
        assert_eq!(d.get("pos").unwrap(), "right");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn app_settings_roundtrip() {
        let path = tmpfile("app-settings.json");
        let mut obj = serde_json::Map::new();
        obj.insert("theme".into(), Value::String("mist".into()));
        obj.insert(
            "session_font".into(),
            serde_json::json!({"family": "Inter", "size": 14.5}),
        );
        obj.insert("lang".into(), Value::Number(1u64.into()));
        save_map_to(&path, &obj);
        let map = load_map_from(&path);
        let spec = font_spec_from(map.get("session_font").unwrap()).unwrap();
        assert_eq!(spec.family, "Inter");
        assert_eq!(spec.size, 14.5);
        assert_eq!(map.get("theme").unwrap(), "mist");
        assert_eq!(
            map.get("lang").and_then(|v| v.as_u64()),
            Some(1),
            "lang/sound live in app_settings.json, not the workspace map"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
