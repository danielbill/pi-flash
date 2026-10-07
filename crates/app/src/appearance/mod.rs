//! Appearance (006 界面设置 / 007 多语言): zed-style theme registry +
//! icon theme + font slots, backed by `app_settings.json`
//! (services::workspace::AppSettings).
//!
//! Layering: `theme.rs` keeps the ACTIVE theme global (`theme()` /
//! `set_by_name`, unchanged API for ~all render sites); this module owns
//! the catalog (id / display name / light-dark family) and the switch
//! path, which also re-maps tokens into gpui-component's theme global —
//! without that remap the widget-library surfaces (inputs, modals) keep
//! the previous theme's colors after a switch.

use gpui::App;

use crate::services::workspace::{self, AppSettings, FontSpec};
use crate::theme::{self, Theme};


/// One catalog entry of the theme registry.
#[derive(Debug, Clone, Copy)]
pub struct ThemeEntry {
    pub id: &'static str,
    pub name: &'static str,
}

/// The built-in catalog (006): pi-web 浅色雾青/蔷薇 + zed One Light/One Dark、
/// nord light/dark、ayu light。调色板源数据在 `assets/themes/`（含 LICENSE）。
pub fn registry() -> &'static [ThemeEntry] {
    &[
        ThemeEntry { id: "mist", name: "雾青" },
        ThemeEntry { id: "rose", name: "蔷薇" },
        ThemeEntry { id: "one-light", name: "One Light" },
        ThemeEntry { id: "nord-light", name: "Nord Light" },
        ThemeEntry { id: "nord-dark", name: "Nord Dark" },
        ThemeEntry { id: "ayu-light", name: "Ayu Light" },
        ThemeEntry { id: "one-dark", name: "One Dark" },
    ]
}

/// Map the active app theme into gpui-component's global theme (extracted
/// from main(); MUST run on every theme switch).
pub fn sync_gpui_tokens(cx: &mut App) {
    gpui_component::theme::init(cx);
    let t: &Theme = theme::theme();
    let tc = gpui_component::theme::Theme::global_mut(cx);
    tc.radius = px(5.);
    // composer 多行滚动条常显（默认 Scrolling=滚动进行中才闪现，用户视为
    // "没有滚动条"）
    tc.scrollbar_show = gpui_component::scroll::ScrollbarShow::Always;
    let c = &mut tc.colors;
    c.background = gpui::rgb(t.bg_panel).into();
    c.foreground = gpui::rgb(t.text).into();
    c.border = gpui::rgb(t.border).into();
    c.input = gpui::rgb(t.border).into();
    c.ring = gpui::rgb(t.accent).into();
    c.caret = gpui::rgb(t.text).into();
    c.accent = gpui::rgb(t.accent).into();
    c.accent_foreground = gpui::rgb(t.accent_contrast).into();
    c.muted = gpui::rgb(t.bg_hover).into();
    c.muted_foreground = gpui::rgb(t.text_dim).into();
    c.secondary = gpui::rgb(t.bg_selected).into();
    c.danger = gpui::rgb(t.danger).into();
    c.danger_hover = gpui::rgb(t.danger_hover).into();
    c.danger_foreground = gpui::rgb(0xffffff).into();
    // 滚动条（ZED Regular 视觉）：**只有 thumb、无 track**——gpui-component
    // 默认 track 色是半透明灰白条（#fafafa80），即截图里"两条滚动条"的第
    // 一条；透明化后只剩 thumb。thumb 用 text 薄纱（贴合各主题，hover 加深）。
    c.scrollbar = gpui::transparent_black().into();
    c.scrollbar_thumb = gpui::rgba((t.text << 8) | 0x33).into();
    c.scrollbar_thumb_hover = gpui::rgba((t.text << 8) | 0x61).into();
    let mut sel: gpui::Hsla = gpui::rgb(t.accent).into();
    sel.a = 0.28;
    c.selection = sel;
    // mode 与应用主题一致（此前恒为系统外观 Dark：组件内部 is_dark 分支
    // 与默认配置选择都会走错；Theme::change 按系统外观初始化后无人纠正）
    let mode = if t.dark {
        gpui_component::theme::ThemeMode::Dark
    } else {
        gpui_component::theme::ThemeMode::Light
    };
    tc.mode = mode;
    // 023 CodeEditor 面色：highlight_theme 是编辑器 gutter/当前行/背景的
    // 专用 token（Theme 默认停在暗色盘——浅色主题下出现黑 gutter/黑条即此）。
    // 语法配色取组件内置明/暗盘，编辑器面色覆写为应用主题 token。
    let mut hl_style = (if t.dark {
        gpui_component::highlighter::HighlightTheme::default_dark()
    } else {
        gpui_component::highlighter::HighlightTheme::default_light()
    })
    .style
    .clone();
    hl_style.editor_background = Some(gpui::rgb(t.bg).into());
    hl_style.editor_foreground = Some(gpui::rgb(t.text).into());
    hl_style.editor_line_number = Some(gpui::rgb(t.text_faint).into());
    hl_style.editor_active_line_number = Some(gpui::rgb(t.text).into());
    let mut active_line: gpui::Hsla = gpui::rgb(t.text).into();
    active_line.a = 0.05;
    hl_style.editor_active_line = Some(active_line);
    tc.highlight_theme = std::sync::Arc::new(gpui_component::highlighter::HighlightTheme {
        name: "pi-flash".into(),
        appearance: mode,
        style: hl_style,
    });
}

use gpui::px;

/// Switch the active theme by registry id: active-global update + token
/// remap happens via the caller (needs `&mut App`) — this part persists
/// the choice into app_settings.json and reports unknown ids.
pub fn persist_theme(id: &str) -> bool {
    if !theme::set_by_name(id) {
        return false;
    }
    let mut s = workspace::app_settings();
    s.theme = Some(id.to_string());
    workspace::save_app_settings(&s);
    true
}

/// The session (chat) font: app setting or the default.
/// 族直连聊天区容器；字号是会话区所有文字的基准（`sess_size`）。
pub fn session_font() -> FontSpec {
    app_settings().session_font.unwrap_or(FontSpec {
        family: default_font_family(),
        size: 15.,
    })
}

pub fn panel_font() -> FontSpec {
    app_settings().panel_font.unwrap_or(FontSpec {
        family: default_font_family(),
        size: 15.,
    })
}

/// 文件字体（docs/UI设计/字体大小设置.md §3）：文件视图里能打开的文件
/// （markdown/txt/json/py…）源码或预览的字号基准。存储键沿用历史的
/// `markdown_font`，避免冲掉用户已存的设置。
pub fn file_font() -> FontSpec {
    app_settings().markdown_font.unwrap_or(FontSpec {
        family: default_font_family(),
        size: 15.,
    })
}

/// Persist one font slot.
pub fn save_font(slot: FontSlot, spec: FontSpec) {
    let mut s = app_settings();
    match slot {
        FontSlot::Session => s.session_font = Some(spec),
        FontSlot::Panel => s.panel_font = Some(spec),
        FontSlot::File => s.markdown_font = Some(spec),
    }
    workspace::save_app_settings(&s);
    sync_ui_scale();
}

/// 面板字号：界面 chrome 文字的基准。全界面文字经 `ui_size()` 取
/// 「面板设置值 + (base - 12)」——base 是面板 12px 时代的设计稿值，偏移量
/// 在任何设置档下不变（绝对像素差模型，字体大小设置.md §1）。默认设置值
/// 15（与设置页三档 14/15/16/17 的「中」对齐），设置里改字号，整窗界面
/// 文字（含设置页自身）立刻变化（所见即所得）。
static PANEL_PX: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(15.0f32.to_bits());

/// Startup + save_font 后重算缓存的面板字号（钳 10–17：与字号档位上限
/// 一致；历史存档超界按边界值生效）。
pub fn sync_ui_scale() {
    let v = panel_font().size.clamp(10.0, 17.0);
    std::sync::atomic::AtomicU32::store(&PANEL_PX, v.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// 界面 chrome 文字字号（base = 12px 设计稿值；生效值 = 面板设置值 +
/// (base - 12)，「设置值-1」这类文档规格即 ui_size(11.)）。
pub fn ui_size(base: f32) -> gpui::Pixels {
    let panel = f32::from_bits(PANEL_PX.load(std::sync::atomic::Ordering::Relaxed));
    gpui::px(panel + (base - 12.0))
}

/// 会话区元素字号 = 会话设置值 + delta（字体大小设置.md §2 的
/// 「设置值-2 / 设置值-1 / 设置值+2」；delta 单位 px，正文传 0）。
pub fn sess_size(delta: f32) -> gpui::Pixels {
    gpui::px(session_font().size + delta)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontSlot {
    Session,
    Panel,
    File,
}

fn app_settings() -> AppSettings {
    workspace::app_settings()
}

pub fn default_font_family() -> String {
    // system UI font; gpui resolves it per-platform
    "Segoe UI".to_string()
}

// ---------------------------------------------------------------------------
// system font catalog (字体下拉：zed parity，全量系统字体按字母序)
// ---------------------------------------------------------------------------

static FONT_CATALOG: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

/// Enumerate + sort the system font families once at startup (main.rs,
/// after the bundled fonts are registered).
pub fn init_font_catalog(cx: &App) {
    let mut names = cx.text_system().all_font_names();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    let _ = FONT_CATALOG.set(names);
}

/// The sorted system font families (empty until init_font_catalog ran).
pub fn font_catalog() -> &'static [String] {
    FONT_CATALOG.get().map(|v| v.as_slice()).unwrap_or(&[])
}

