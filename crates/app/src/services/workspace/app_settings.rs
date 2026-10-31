//! 应用外观设置（006 界面设置）：`pi-flash-app-settings.json` ——主题 /
//! 图标主题 / 字体 + 派生 getter 群（模型偏好/发送模式/dev flags/自动更新等）
//! + 会话内容宽度。自 workspace.rs 拆出（090 重构）。

use super::*;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

// ---------------------------------------------------------------------------
// app settings (006 界面设置): theme / icon theme / fonts + lang / sound
// ---------------------------------------------------------------------------

/// One font slot of the appearance settings (006): family + pt size.
/// `None` fields mean "unset" so defaults stay code-side.
#[derive(Debug, Clone, PartialEq)]
pub struct FontSpec {
    pub family: String,
    pub size: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
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
    /// v60: startup load time window in days (7/14/30) — sessions whose
    /// last activity falls inside the window form the psp list
    pub load_window_days: Option<u64>,
    /// v54 其他页: restore last workspace + session on startup
    pub restore: Option<bool>,
    /// 界面页: 展示思考（关（默认）=新思考块收成一行；开=默认展开全文）
    pub show_thinking: Option<bool>,
    /// 界面页: 文件树 git 标识（023 默认关——清爽目录树；开 = M/A/D/R/U/C
    /// 徽标 + 目录变更点）
    pub git_markers: Option<bool>,
    /// 其他页: 各项目默认显示的会话数量（psp 初始页大小，3-10，默认 10）
    pub session_display_count: Option<u64>,
    /// 其他页: 启动自动折叠阈值（小时）——项目内会话最后活动时间全部早于
    /// 阈值即折叠。档位 24/72/168（1/3/7 天），默认 168（7 天）
    pub auto_collapse_hours: Option<u64>,
    /// @ 文件索引缓存有效期（秒，1-3600，默认 60）
    pub at_index_ttl_secs: Option<u64>,
    /// @ 文件索引缓存的最多项目数（每 cwd 一份，1-100，默认 10）
    pub at_index_max_projects: Option<u64>,
    /// 042：默认模型（设置-模型页下列表五角星）。PF 自有默认——不写 pi 的
    /// settings.json（系统 pi / pi-web 的地盘），spawn 时以 `--model` 落到
    /// 新会话。两键齐才算设置了默认。
    pub default_provider: Option<String>,
    pub default_model: Option<String>,
    /// 081 其他页: 自动更新（默认 on——off 时只提示新版本号 ↑ 不下载）
    pub auto_update: Option<bool>,
    /// 052 其他页: 会话自动清理天数——最后活动超出该天数的非置顶会话
    /// 启动时移入回收站。默认 0 = 关闭（2026-10-18 用户定夺），设置后钳制 [0,100]。
    pub auto_clean_days: Option<u64>,
    /// 界面页: Markdown 所见即所得（024 Live Preview 装饰；默认关——
    /// 已知回车崩溃与标题行距问题未修，用户显式开启才挂装饰）
    pub md_live_preview: Option<bool>,
    /// 界面页: 部分操作隐藏（dock 两操作栏不常用 icon 常态透明、hover
    /// 操作栏才显影；默认 on）
    pub autohide_icons: Option<bool>,
    /// 界面页: 会话内容宽度 px（消息列 / composer 胶囊 / 新会话页内容带 /
    /// ask 问卷卡共用的单一内容列宽，pi-web `--chat-content-max-width`
    /// parity；缺键 = [`CHAT_WIDTH_DEFAULT`]）
    pub chat_content_width: Option<u64>,
    /// 090 排队护栏: 运行中发送方式——"queue"（默认）= 扣 PF 本地队列、
    /// AgentSettled 才逐条发出；"steer" = prompt+steer 立即插进当前轮。
    /// 非法值/缺键都落 queue。
    pub send_mode: Option<String>,
}

fn app_settings_path() -> Option<PathBuf> {
    // pi-flash 自有目录（010-启动.md §4）
    pi_link::paths::app_settings_file()
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
                    theme: map
                        .get("theme")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
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
                    load_window_days: map.get("load_window_days").and_then(|v| v.as_u64()),
                    restore: map.get("startup_restore").and_then(|v| v.as_bool()),
                    show_thinking: map.get("show_thinking").and_then(|v| v.as_bool()),
                    git_markers: map.get("git_markers").and_then(|v| v.as_bool()),
                    session_display_count: map
                        .get("session_display_count")
                        .and_then(|v| v.as_u64()),
                    auto_collapse_hours: map.get("auto_collapse_hours").and_then(|v| v.as_u64()),
                    at_index_ttl_secs: map.get("at_index_ttl_secs").and_then(|v| v.as_u64()),
                    at_index_max_projects: map
                        .get("at_index_max_projects")
                        .and_then(|v| v.as_u64()),
                    default_provider: map
                        .get("default_provider")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    default_model: map
                        .get("default_model")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    auto_update: map.get("auto_update").and_then(|v| v.as_bool()),
                    auto_clean_days: map
                        .get("auto_clean_days")
                        .and_then(|v| v.as_u64())
                        .map(|v| v.min(100)),
                    md_live_preview: map.get("md_live_preview").and_then(|v| v.as_bool()),
                    autohide_icons: map.get("autohide_icons").and_then(|v| v.as_bool()),
                    chat_content_width: map.get("chat_content_width").and_then(|v| v.as_u64()),
                    send_mode: map
                        .get("send_mode")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
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
    let Some(path) = app_settings_path() else {
        return;
    };
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
    if let Some(v) = s.load_window_days {
        obj.insert("load_window_days".into(), Value::Number(v.into()));
    }
    if let Some(v) = s.restore {
        obj.insert("startup_restore".into(), Value::Bool(v));
    }
    if let Some(v) = s.show_thinking {
        obj.insert("show_thinking".into(), Value::Bool(v));
    }
    if let Some(v) = s.git_markers {
        obj.insert("git_markers".into(), Value::Bool(v));
    }
    if let Some(v) = s.session_display_count {
        obj.insert("session_display_count".into(), Value::Number(v.into()));
    }
    if let Some(v) = s.auto_collapse_hours {
        obj.insert("auto_collapse_hours".into(), Value::Number(v.into()));
    }
    if let Some(v) = s.at_index_ttl_secs {
        obj.insert("at_index_ttl_secs".into(), Value::Number(v.into()));
    }
    if let Some(v) = s.at_index_max_projects {
        obj.insert("at_index_max_projects".into(), Value::Number(v.into()));
    }
    if let Some(v) = &s.default_provider {
        obj.insert("default_provider".into(), Value::String(v.clone()));
    }
    if let Some(v) = &s.default_model {
        obj.insert("default_model".into(), Value::String(v.clone()));
    }
    if let Some(v) = s.auto_update {
        obj.insert("auto_update".into(), Value::Bool(v));
    }
    if let Some(v) = s.auto_clean_days {
        obj.insert("auto_clean_days".into(), Value::Number(v.into()));
    }
    if let Some(v) = s.md_live_preview {
        obj.insert("md_live_preview".into(), Value::Bool(v));
    }
    if let Some(v) = s.autohide_icons {
        obj.insert("autohide_icons".into(), Value::Bool(v));
    }
    if let Some(v) = s.chat_content_width {
        obj.insert("chat_content_width".into(), Value::Number(v.into()));
    }
    if let Some(v) = &s.send_mode {
        obj.insert("send_mode".into(), Value::String(v.clone()));
    }
    save_map_to(&path, &obj);
}

/// 042：默认模型读取（两键齐生效；键不存在 = 未设置默认）。
pub fn default_model_pref() -> Option<(String, String)> {
    let s = app_settings();
    Some((s.default_provider?, s.default_model?))
}

/// 090 排队护栏：运行中发送方式（设置-其他）。返回 "queue"（默认，含缺键
/// /非法值）或 "steer"。
pub fn send_mode_pref() -> &'static str {
    match app_settings().send_mode.as_deref() {
        Some("steer") => "steer",
        _ => "queue",
    }
}

/// 090：发送方式写入（设置页两档分段按钮）。
pub fn save_send_mode(mode: &str) {
    let mut s = app_settings();
    s.send_mode = Some(mode.to_string());
    save_app_settings(&s);
}

/// 042：默认模型写入（设置页五角星 / inputpanel 星标共用；None = 取消）。
/// 落盘即生效——新会话 spawn 读最新值。
pub fn set_default_model_pref(pref: Option<(String, String)>) {
    let mut s = app_settings();
    match pref {
        Some((p, m)) => {
            s.default_provider = Some(p);
            s.default_model = Some(m);
        }
        None => {
            s.default_provider = None;
            s.default_model = None;
        }
    }
    save_app_settings(&s);
}

/// Startup load time window in days (settings-其他「加载时间窗口」档位
/// 7/14/30, default 7) — the recents list filters on it.
pub fn load_window_days() -> u64 {
    match app_settings().load_window_days.unwrap_or(7) {
        14 => 14,
        30 => 30,
        _ => 7,
    }
}

/// Sessions whose tail is read into memory at startup (fixed, not a
/// setting — the load window is the user-facing knob).
pub const TAIL_PRELOAD: usize = 10;

/// v54 其他: startup restore toggle (default off).
/// 启动恢复开关：on = 落最近活动项目并**直接加载**该项目最后会话；
/// off = 落同一项目但停在新会话页（项目记忆与开关无关，恒生效）。
/// 默认 on（「启动恢复最后会话」是默认体验；显式关过的用户保留其设置）。
pub fn startup_restore() -> bool {
    app_settings().restore.unwrap_or(true)
}

/// 其他页: 各项目默认显示的会话数量（psp 初始页大小，3-10，默认 10）。
pub fn session_display_count() -> usize {
    app_settings()
        .session_display_count
        .unwrap_or(10)
        .clamp(3, 10) as usize
}

/// 其他页: 启动自动折叠阈值（小时；档位 24/72/168 = 1/3/7 天，默认 168）。
/// 启动装载项目集后，项目内会话最后活动时间全部早于该阈值 → 该项目组折叠。
pub fn auto_collapse_hours() -> u64 {
    match app_settings().auto_collapse_hours.unwrap_or(168) {
        24 => 24,
        72 => 72,
        _ => 168,
    }
}

/// @ 文件索引缓存有效期（秒；默认 60，clamp 1-3600）。
pub fn at_index_ttl_secs() -> u64 {
    app_settings()
        .at_index_ttl_secs
        .unwrap_or(60)
        .clamp(1, 3600)
}

/// @ 文件索引缓存的最多项目数（每 cwd 一份；默认 10，clamp 1-100）。
pub fn at_index_max_projects() -> usize {
    app_settings()
        .at_index_max_projects
        .unwrap_or(10)
        .clamp(1, 100) as usize
}

/// 界面页: 展示思考（关（默认）=新思考块收成一行；开=默认展开全文）。
pub fn show_thinking() -> bool {
    app_settings().show_thinking.unwrap_or(false)
}

/// 界面页: 部分操作隐藏（dock 会话控制栏「列表切换」与文件树操作栏五颗
/// 功能 icon 常态透明，hover 各自操作栏才显影；默认 on）。渲染期每帧读，
/// `app_settings()` 走进程内缓存，开销可忽略。
pub fn autohide_icons() -> bool {
    app_settings().autohide_icons.unwrap_or(true)
}

/// 开发旗标快照（dev_flags::KNOWN 全键，进程内首读一次）。渲染期每帧调用
/// 安全；文件改动重启（重新 cargo run）生效。
fn dev_flag_snapshot() -> &'static std::collections::HashMap<String, bool> {
    static SNAPSHOT: OnceLock<std::collections::HashMap<String, bool>> = OnceLock::new();
    SNAPSHOT.get_or_init(|| {
        pi_link::dev_flags::KNOWN
            .iter()
            .map(|key| (key.to_string(), pi_link::dev_flags::enabled(key)))
            .collect()
    })
}

/// 开发旗标查询（键 = 模块名，见 pi-link `dev_flags::KNOWN`；缺键 = false）。
pub fn dev_flag_enabled(key: &str) -> bool {
    dev_flag_snapshot().get(key).copied().unwrap_or(false)
}

/// subagent 功能总开关（开发旗标，默认关；dev-flags.json /
/// PI_FLASH_SUBAGENTS 控制）。
pub fn subagents_enabled() -> bool {
    dev_flag_enabled("subagents")
}

/// 040-1 Pi 扩展市场入口开关（开发旗标，**默认关 = 发版不可见**）。
/// 关态 = topbar 「菜单」三横钮不渲染、`open_pi_ext_market` 直接返回，
/// 任何入口（菜单/自动化）都摸不到市场页。
pub fn pi_ext_market_enabled() -> bool {
    dev_flag_enabled("pi_ext_market")
}

/// topbar「菜单」三横钮是否渲染：**恒显**（2026-10-23 用户禁令：不许删菜单 icon）。
/// 菜单是多入口的壳，里面 090-4「Pi-Flash 插件」是主线常驻项，按钮不能跟着
/// 别的页面的 dev-flag 一起消失；各市场项自己的显隐照旧由 menu.rs 管。
pub fn market_menu_enabled() -> bool {
    true
}

/// 文件树 git 标识开关（设置-界面；默认关 = 清爽目录树，023 定案）。
pub fn git_markers() -> bool {
    app_settings().git_markers.unwrap_or(false)
}

/// 081 其他页: 自动更新开关（默认 on）。
pub fn auto_update() -> bool {
    app_settings().auto_update.unwrap_or(true)
}

/// 052 其他页: 会话自动清理天数（默认 0 = 关闭——2026-10-18 用户定夺：自动删
/// 会话太凶，显式设置后才生效；读入口统一钳制 [0,100]，存量脏值也在
/// 这里被拦住）。
pub fn auto_clean_days() -> u64 {
    app_settings().auto_clean_days.unwrap_or(0).min(100)
}

/// 界面页: Markdown 所见即所得（024 Live Preview 装饰；默认关 = md 源码态
/// 走普通编辑器路径。已知回车崩溃与标题行距问题未修，显式开启才挂装饰）。
pub fn md_live_preview() -> bool {
    app_settings().md_live_preview.unwrap_or(false)
}

// ---- 会话内容宽度（pi-web `--chat-content-max-width` parity）------------
//
// 单一内容列宽：消息列、composer 胶囊、新会话页内容带、ask 问卷卡全部读
// 这里（此前各自硬编码 920），改一处即整列同宽、不会再错位。
// 阈值取法（2026-10-29 用户定案：1200 封顶、步进 20）：
// - 默认 920 = 本仓历史观感（pi-web 默认 820，我们不跟随改默认值）；
// - 最小 560 = 阅列底线（默认字号 15px 下约 37 汉字/行，用户气泡 85% 宽
//   约 30 字），再窄正文与代码块就开始一句一折行；pi-web 的 min 直接等于
//   默认（只许加宽），我们放开收窄——PF 左 dock 吃宽，收窄是真实需求；
// - 最大 1200 = 用户定案封顶（用户否决了按行字数/按窗口推导的方案：列宽
//   写的是 `w_full().max_w(cap)`，cap 只是封顶，与屏幕分辨率无关，小屏自动
//   取可用宽）；
// - 步进 20 = 用户定案（560→1200 共 32 档，一次拖动就能落位，存盘值整齐）。
pub const CHAT_WIDTH_MIN: u32 = 560;
pub const CHAT_WIDTH_DEFAULT: u32 = 920;
pub const CHAT_WIDTH_MAX: u32 = 1200;
/// 滑块步进（20px，用户定案；min/default/max 均为 20 的整数倍）
pub const CHAT_WIDTH_STEP: u32 = 20;

/// 会话内容宽度（px，已钳制；渲染期多次读取，只读缓存）。
pub fn chat_content_width() -> f32 {
    clamp_chat_width(
        app_settings()
            .chat_content_width
            .unwrap_or(CHAT_WIDTH_DEFAULT as u64),
    ) as f32
}

fn clamp_chat_width(v: u64) -> u32 {
    (v as u32).clamp(CHAT_WIDTH_MIN, CHAT_WIDTH_MAX)
}

/// 拖动中的实时预览：只改内存缓存不落盘（每帧一个 MouseMove，逐帧写
/// JSON 没必要），松手再 [`save_app_settings_now`] 落一次。
pub fn preview_chat_content_width(width: u32) {
    with_app_settings_cache(|cache| {
        let mut s = (*cache).clone().unwrap_or_default();
        s.chat_content_width = Some(clamp_chat_width(width as u64) as u64);
        *cache = Some(s);
    });
}

/// 把内存缓存（拖拽预览后的值）落盘。
pub fn save_app_settings_now() {
    save_app_settings(&app_settings());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::workspace::load_map_from;
    use std::path::PathBuf;

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
