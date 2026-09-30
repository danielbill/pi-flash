//! Themes translated 1:1 from pi-web app/globals.css `[data-theme=...]` blocks.
//!
//! v54 三层色阶: chrome (topbar/statusbar) / nav (psp dock) / bg (content),
//! plus the psp aux colors (text_soft = title-dim, text_faint = placeholder
//! gray, danger family for destructive actions).


#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub bg: u32,
    pub bg_panel: u32,
    pub bg_hover: u32,
    pub bg_selected: u32,
    pub border: u32,
    pub text: u32,
    pub text_muted: u32,
    pub text_dim: u32,
    /// psp title / git head project label (#8a9d95 in mist)
    pub text_soft: u32,
    /// placeholder / non-action info (#a3b0aa in mist)
    pub text_faint: u32,
    pub accent: u32,
    pub accent_hover: u32,
    pub accent_contrast: u32,
    pub user_bg: u32,
    pub assistant_bg: u32,
    pub tool_bg: u32,
    /// --bg-subtle (has alpha; stored as RGBA)
    pub bg_subtle: u32,
    /// topbar 两段 + statusbar (chrome layer)
    pub chrome: u32,
    /// psp dock (nav layer)
    pub nav: u32,
    /// destructive text (删除/红字菜单)
    pub danger: u32,
    /// destructive hover bg base (RGB part; paint at ~12% alpha)
    pub danger_hover: u32,
}

/// Unread green dot (--unread; works on light and dark surfaces).
pub const UNREAD: u32 = 0x2fa356;

/// accent at 10% alpha over a light surface (psp 选中 soft-tint).
pub fn accent_tint(t: &Theme) -> u32 {
    (t.accent << 8) | 0x1a
}

/// danger at ~12% alpha (menu hover / confirm row wash).
pub fn danger_wash(t: &Theme) -> u32 {
    (t.danger_hover << 8) | 0x1f
}

/// `[data-theme="mist"]` — the default pi-web look.
pub const MIST: Theme = Theme {
    bg: 0xf4f8f7,
    bg_panel: 0xe9f0ee,
    bg_hover: 0xe0eae7,
    bg_selected: 0xd7e4df,
    border: 0xafc4ba,
    text: 0x202e2b,
    text_muted: 0x455f56,
    text_dim: 0x52685f,
    text_soft: 0x8a9d95,
    text_faint: 0xa3b0aa,
    accent: 0x1e6559,
    accent_hover: 0x174f46,
    accent_contrast: 0xffffff,
    user_bg: 0xe0efeb,
    assistant_bg: 0xf4f8f7,
    tool_bg: 0xecf3f0,
    bg_subtle: 0x1841320a, // rgba(24,65,50,0.04)
    chrome: 0xdfe9e5,
    nav: 0xeef4f1,
    danger: 0xc25460,
    danger_hover: 0xd8626a,
};

/// default light theme (`:root`)
/// `[data-theme="dark"]`
/// `[data-theme="rose"]`
pub const ROSE: Theme = Theme {
    bg: 0xfcf7f8,
    bg_panel: 0xf3edef,
    bg_hover: 0xeee3e7,
    bg_selected: 0xe8dce1,
    border: 0xcdb5bf,
    text: 0x34282e,
    text_muted: 0x65505a,
    text_dim: 0x705b65,
    text_soft: 0x9a8490,
    text_faint: 0xb0a0aa,
    accent: 0x914360,
    accent_hover: 0x76324d,
    accent_contrast: 0xffffff,
    user_bg: 0xf3e4eb,
    assistant_bg: 0xfcf7f8,
    tool_bg: 0xf7eef2,
    bg_subtle: 0x0000000a,
    chrome: 0xecdfe5,
    nav: 0xf8f0f3,
    danger: 0xb5485c,
    danger_hover: 0xc95d6f,
};

pub const ALL: &[(&str, Theme)] = &[
    ("mist", MIST),
    ("rose", ROSE),
    ("one-light", ONE_LIGHT),
    ("nord-light", NORD_LIGHT),
    ("nord-dark", NORD_DARK),
    ("ayu-light", AYU_LIGHT),
    ("one-dark", ONE_DARK),
];

/// ZED "One Dark" — ported from `zed/crates/theme/src/fallback_themes.rs`
/// `zed_default_dark()` (the compile-time fallback family), HSLA -> RGB8.
/// One Light / nord / ayu port from the shipped JSONs in `assets/themes/`
/// (see gen_themes.py); translucent zed colors bake over the surface color.
pub const ONE_DARK: Theme = Theme {
    bg: 0x22252b,         // background hsla(215,12%,15%)
    bg_panel: 0x282c33,   // editor/toolbar hsla(220,12%,18%)
    bg_hover: 0x3c404c,   // element_hover hsla(225,11.8%,26.7%)
    bg_selected: 0x3b3f4a, // element_selected hsla(224,11.3%,26.1%)
    border: 0x1b1d23,     // border hsla(225,13%,12%)
    text: 0xd7dadf,       // text hsla(221,11%,86%)
    text_muted: 0x6d737e, // text_muted hsla(218,7%,46%)
    text_dim: 0x6a6f79,   // text_disabled hsla(220,6.6%,44.5%)
    text_soft: 0x757b87,
    text_faint: 0x8b919d,
    accent: 0x6189eb,     // text_accent blue hsla(222.6,77.5%,65.1%)
    accent_hover: 0x6088eb, // border_focused hsla(223,78%,65%)
    accent_contrast: 0xffffff,
    user_bg: 0x2f333d,    // element_background hsla(223,13%,21%)
    assistant_bg: 0x262931, // elevated_surface hsla(225,12%,17%)
    tool_bg: 0x2d3139,    // element_active hsla(220,11.8%,20%)
    bg_subtle: 0xffffff14,
    chrome: 0x23262c,
    nav: 0x2b2f36,
    danger: 0xdf5561,
    danger_hover: 0xe0666f,
};

/// ZED 'One Light' — ported from zed assets/themes/one/one.json (assets/themes/one/).
pub const ONE_LIGHT: Theme = Theme {
    bg: 0xfafafa,
    bg_panel: 0xebebeb,
    bg_hover: 0xdfdfe0,
    bg_selected: 0xcacaca,
    border: 0xc9c9ca,
    text: 0x242529,
    text_muted: 0x58585a,
    text_dim: 0x7e8086,
    text_soft: 0x8e9096,
    text_faint: 0xa8aab0,
    accent: 0x5c78e2,
    accent_hover: 0x7d82e8,
    accent_contrast: 0xffffff,
    user_bg: 0xebebeb,
    assistant_bg: 0xebebeb,
    tool_bg: 0xcacaca,
    bg_subtle: 0x0000000a,
    chrome: 0xe4e4e6,
    nav: 0xf2f2f3,
    danger: 0xc2545c,
    danger_hover: 0xd86570,
};

/// ZED 'Nord Light' — ported from Zed nord extension nord.json (assets/themes/nord/).
pub const NORD_LIGHT: Theme = Theme {
    bg: 0xeceff4,
    bg_panel: 0xf6f8fa,
    bg_hover: 0xebf0f4,
    bg_selected: 0xe2e5e9,
    border: 0xe1e4e8,
    text: 0x444d56,
    text_muted: 0x6a737d,
    text_dim: 0x6a737d,
    text_soft: 0x8b97a8,
    text_faint: 0xa4afbd,
    accent: 0x2188ff,
    accent_hover: 0x2188ff,
    accent_contrast: 0xffffff,
    user_bg: 0xe5e9f0,
    assistant_bg: 0xfafbfc,
    tool_bg: 0xebf0f4,
    bg_subtle: 0x0000000a,
    chrome: 0xe4e9f0,
    nav: 0xf0f3f8,
    danger: 0xb5485c,
    danger_hover: 0xc95d6f,
};

/// ZED 'Nord Dark' — ported from Zed nord extension nord.json (assets/themes/nord/).
pub const NORD_DARK: Theme = Theme {
    bg: 0x2e3440,
    bg_panel: 0x2e3440,
    bg_hover: 0x475c69,
    bg_selected: 0x6c99a6,
    border: 0x3b4252,
    text: 0xeceff4,
    text_muted: 0xd8dee9,
    text_dim: 0xd8dee9,
    text_soft: 0x8b95a6,
    text_faint: 0x9ba5b5,
    accent: 0x88c0d0,
    accent_hover: 0x6c99a6,
    accent_contrast: 0xffffff,
    user_bg: 0x3b4252,
    assistant_bg: 0x3b4252,
    tool_bg: 0x475c69,
    bg_subtle: 0xffffff14,
    chrome: 0x2a313d,
    nav: 0x343c4a,
    danger: 0xd06a75,
    danger_hover: 0xd97b85,
};

/// ZED 'Ayu Light' — ported from zed assets/themes/ayu/ayu.json (assets/themes/ayu/).
pub const AYU_LIGHT: Theme = Theme {
    bg: 0xfcfcfc,
    bg_panel: 0xececed,
    bg_hover: 0xdfe0e1,
    bg_selected: 0xcfd0d2,
    border: 0xcfd1d2,
    text: 0x5c6166,
    text_muted: 0x8b8e92,
    text_dim: 0xa9acae,
    text_soft: 0x93969c,
    text_faint: 0xabb0b5,
    accent: 0x3b9ee5,
    accent_hover: 0xc4daf6,
    accent_contrast: 0xffffff,
    user_bg: 0xececed,
    assistant_bg: 0xececed,
    tool_bg: 0xcfd0d2,
    bg_subtle: 0x0000000a,
    chrome: 0xe6e6e8,
    nav: 0xf2f2f4,
    danger: 0xc2545c,
    danger_hover: 0xd86570,
};

/// Active theme index; the UI re-reads it every render so a switch repaints
/// everything. `PI_FLASH_THEME=<name>` overrides the persisted choice for dev.
static THEME_IX: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Apply a theme by name; false when the name is unknown.
pub fn set_by_name(name: &str) -> bool {
    if let Some(ix) = ALL.iter().position(|(n, _)| *n == name) {
        THEME_IX.store(ix, std::sync::atomic::Ordering::Relaxed);
        true
    } else {
        false
    }
}

pub fn theme_name() -> &'static str {
    ALL[THEME_IX.load(std::sync::atomic::Ordering::Relaxed)].0
}

pub fn theme() -> &'static Theme {
    &ALL[THEME_IX.load(std::sync::atomic::Ordering::Relaxed)].1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch_roundtrip_and_reject() {
        assert!(set_by_name("rose"));
        assert_eq!(theme_name(), "rose");
        assert_eq!(theme().bg, ROSE.bg);
        assert!(set_by_name("mist"));
        assert_eq!(theme_name(), "mist");
        assert!(!set_by_name("nope"));
        assert_eq!(theme_name(), "mist");
        // all seven built-in themes have complete palettes (v54 layers incl.)
        for (name, t) in ALL {
            assert!(t.accent != 0, "{name} missing accent");
            assert!(t.border != 0, "{name} missing border");
            assert!(t.chrome != 0, "{name} missing chrome");
            assert!(t.nav != 0, "{name} missing nav");
            assert!(t.danger != 0, "{name} missing danger");
        }
    }
}
