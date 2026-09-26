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

/// Light/dark family (zed Appearance parity).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Light,
    Dark,
}

/// One catalog entry of the theme registry.
#[derive(Debug, Clone, Copy)]
pub struct ThemeEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub family: Family,
    /// where the palette comes from (006: pi-web tokens vs zed theme JSON)
    pub source: &'static str,
}

/// The built-in catalog (006). One Light / nord light / ayu light /
/// nord dark land here as soon as their zed theme JSON data is reachable
/// (offline checkout; entries append to `theme::ALL`).
pub fn registry() -> &'static [ThemeEntry] {
    &[
        ThemeEntry { id: "mist", name: "雾青", family: Family::Light, source: "pi-web" },
        ThemeEntry { id: "rose", name: "蔷薇", family: Family::Light, source: "pi-web" },
        ThemeEntry { id: "one-dark", name: "One Dark", family: Family::Dark, source: "zed" },
    ]
}

/// Map the active app theme into gpui-component's global theme (extracted
/// from main(); MUST run on every theme switch).
pub fn sync_gpui_tokens(cx: &mut App) {
    gpui_component::theme::init(cx);
    let t: &Theme = theme::theme();
    let tc = gpui_component::theme::Theme::global_mut(cx);
    tc.radius = px(5.);
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
    let mut sel: gpui::Hsla = gpui::rgb(t.accent).into();
    sel.a = 0.28;
    c.selection = sel;
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

/// The session (chat) font: app setting or the mist-era default.
pub fn session_font() -> FontSpec {
    app_settings().session_font.unwrap_or(FontSpec {
        family: default_font_family(),
        size: 14.,
    })
}

pub fn panel_font() -> FontSpec {
    app_settings().panel_font.unwrap_or(FontSpec {
        family: default_font_family(),
        size: 13.,
    })
}

pub fn markdown_font() -> FontSpec {
    app_settings().markdown_font.unwrap_or(FontSpec {
        family: default_font_family(),
        size: 14.,
    })
}

/// Persist one font slot.
pub fn save_font(slot: FontSlot, spec: FontSpec) {
    let mut s = app_settings();
    match slot {
        FontSlot::Session => s.session_font = Some(spec),
        FontSlot::Panel => s.panel_font = Some(spec),
        FontSlot::Markdown => s.markdown_font = Some(spec),
    }
    workspace::save_app_settings(&s);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontSlot {
    Session,
    Panel,
    Markdown,
}

fn app_settings() -> AppSettings {
    workspace::app_settings()
}

fn default_font_family() -> String {
    // system UI font; gpui resolves it per-platform
    "Segoe UI".to_string()
}

// ---------------------------------------------------------------------------
// icon theme (006: zed architecture, pi-web icon set as the built-in)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IconTheme {
    pub id: &'static str,
    pub name: &'static str,
}

/// Built-in icon themes. The pi-web set is the embedded `assets/icons`
/// (assets.rs); file-type icon sets for the file tree land with the
/// dirTreeView port.
pub const ICON_THEMES: &[IconTheme] = &[IconTheme { id: "pi-web", name: "pi-web" }];

pub fn icon_theme(id: &str) -> Option<&'static IconTheme> {
    ICON_THEMES.iter().find(|t| t.id == id)
}
