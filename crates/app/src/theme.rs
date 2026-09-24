//! Themes translated 1:1 from pi-web app/globals.css `[data-theme=...]` blocks.

use gpui::rgb;

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
    pub accent: u32,
    pub accent_hover: u32,
    pub accent_contrast: u32,
    pub user_bg: u32,
    pub assistant_bg: u32,
    pub tool_bg: u32,
    /// --bg-subtle (has alpha; stored as RGBA)
    pub bg_subtle: u32,
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
    accent: 0x1e6559,
    accent_hover: 0x174f46,
    accent_contrast: 0xffffff,
    user_bg: 0xe0efeb,
    assistant_bg: 0xf4f8f7,
    tool_bg: 0xecf3f0,
    bg_subtle: 0x1841320a, // rgba(24,65,50,0.04)
};

/// default light theme (`:root`)
pub const DEFAULT: Theme = Theme {
    bg: 0xffffff,
    bg_panel: 0xf5f5f5,
    bg_hover: 0xeeeeee,
    bg_selected: 0xe8e8e8,
    border: 0xe0e0e0,
    text: 0x1a1a1a,
    text_muted: 0x515c6b,
    text_dim: 0x5e6673,
    accent: 0x245bce,
    accent_hover: 0x1d4ed8,
    accent_contrast: 0xffffff,
    user_bg: 0xeff6ff,
    assistant_bg: 0xffffff,
    tool_bg: 0xf9fafb,
    bg_subtle: 0x0000000a, // rgba(0,0,0,0.03)
};

/// `[data-theme="dark"]`
pub const DARK: Theme = Theme {
    bg: 0x1a1a1a,
    bg_panel: 0x242424,
    bg_hover: 0x2e2e2e,
    bg_selected: 0x383838,
    border: 0x454545,
    text: 0xe8e8e8,
    text_muted: 0xb7b7b7,
    text_dim: 0xa4a4a4,
    accent: 0xa4c2f4,
    accent_hover: 0xc3d8fa,
    accent_contrast: 0x182234,
    user_bg: 0x292929,
    assistant_bg: 0x1a1a1a,
    tool_bg: 0x222222,
    bg_subtle: 0xffffff0a, // rgba(255,255,255,0.04)
};

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
    accent: 0x914360,
    accent_hover: 0x76324d,
    accent_contrast: 0xffffff,
    user_bg: 0xf3e4eb,
    assistant_bg: 0xfcf7f8,
    tool_bg: 0xf7eef2,
    bg_subtle: 0x0000000a,
};

pub const ALL: &[(&str, Theme)] = &[
    ("mist", MIST),
    ("default", DEFAULT),
    ("dark", DARK),
    ("rose", ROSE),
];

/// Active theme. Runtime switching arrives with the M6 theme panel;
/// `PI_FLASH_THEME=<name>` overrides for development.
pub fn theme() -> &'static Theme {
    if let Ok(name) = std::env::var("PI_FLASH_THEME") {
        if let Some((_, t)) = ALL.iter().find(|(n, _)| *n == name) {
            return t;
        }
    }
    &MIST
}

/// gpui color helper
pub fn c(v: u32) -> gpui::Hsla {
    rgb(v).into()
}
