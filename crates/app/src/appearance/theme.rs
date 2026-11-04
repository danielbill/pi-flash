//! Themes translated 1:1 from pi-web app/globals.css `[data-theme=...]` blocks.
//!
//! v54 三层色阶: chrome (topbar/statusbar) / nav (psp dock) / bg (content),
//! plus the psp aux colors (text_soft = title-dim, text_faint = placeholder
//! gray, danger family for destructive actions).
//!
//! 主题一致性规则（校准基准 = mist 的相对亮度关系，所有主题必须维持）：
//! - border 与 bg 有明显对比（浅色系 border 深于 bg；深色系 border 亮于 bg）
//! - tool_bg / bg_subtle 只比 bg 深或亮 ~3%（工具卡、代码块底）
//! - bg_hover < bg_selected 亮度递进（同向）
//! - text 阶梯单调：text > text_muted > text_dim > text_soft > text_faint
//!   （相对背景的对比度递减）
//! - accent_hover 与 accent 同向加深/提亮

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    /// 深色主题标记（语法高亮主题、覆盖层阴影等按明暗分支）
    pub dark: bool,
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

/// 通知点三色（2026-10-10 定夺）：完成通知青蓝 / 内部中断黄 / 外部错误红
/// （红 = `t.danger`，不设常量）。绿色只表达正确性（终端就绪点等），
/// 不作通知点——青蓝语义是「跑完了回来看」，不承载对错判断。
pub const NOTICE: u32 = 0x3aa6d0;
pub const WARN: u32 = 0xfacc15;
/// 后台会话阻塞请求等待应答（#问题卡不弹：agent 其实挂着，转圈是骗人的）
pub const ATTENTION: u32 = 0xfb923c;

/// Linear RGB channel mix: `wa` = weight of a (0..1). pi-web 的
/// `color-mix(in srgb, a X%, b)` 等价物（代码块底、strong/marker 混色等）。
pub fn mix_rgb(a: u32, b: u32, wa: f32) -> u32 {
    let wa = wa.clamp(0., 1.);
    let mix_ch = |sa: u32, sb: u32| -> u32 {
        let v = (sa as f32 * wa + sb as f32 * (1. - wa)).round();
        v.clamp(0., 255.) as u32
    };
    (mix_ch((a >> 16) & 0xff, (b >> 16) & 0xff) << 16)
        | (mix_ch((a >> 8) & 0xff, (b >> 8) & 0xff) << 8)
        | mix_ch(a & 0xff, b & 0xff)
}

/// border at alpha `a` (0x00..0xff)：替代散落各处的 mist border alpha 硬编码。
pub fn border_alpha(t: &Theme, a: u32) -> u32 {
    (t.border << 8) | (a & 0xff)
}

/// 0xRRGGBB + alpha → gpui::rgba 的 0xRRGGBBAA 布局（alpha 在低字节）。
pub fn rgba_a(rgb24: u32, alpha: f32) -> u32 {
    let a = (alpha * 255.0).round().clamp(0.0, 255.0) as u32;
    ((rgb24 & 0xffffff) << 8) | a
}

/// token 分析块 7 大类的固定类别色（系统提示词面板比例长条 + 信息块色点）。
/// 中饱和中亮度：mist/default 浅底与 dark 深底下都可读；与主题 accent 无关
/// （类别色要跨主题稳定，同一类永远同一颜色）。
pub fn bucket_color(b: pi_link::transcript::SystemBucket) -> u32 {
    use pi_link::transcript::SystemBucket::*;
    match b {
        // 蓝：全局提示词（最大头）
        GlobalPrompt => 0x4c7ed9,
        // 青：系统工具
        SystemTools => 0x38a3c4,
        // 紫：技能
        Skills => 0x8e6bc7,
        // 橙：插件
        Plugins => 0xd08b3c,
        // 玫红：MCP
        Mcp => 0xc75b63,
        // 绿：项目提示词
        ProjectPrompt => 0x3d9e6e,
        // 灰：其他
        Other => 0x8a9099,
    }
}

/// 「消息」类别色（composer 用量弹层的分类行专用）：消息 token 不属于系统
/// 提示词 7 大桶（由官方上下文总量反推），但要与桶色同板展示；品红与 7 桶
/// 色相均不相邻。固定值，跨主题不变（同 bucket_color 口径）。
pub const CTX_MESSAGES_COLOR: u32 = 0xc75bc7;

/// danger_hover at alpha `a`（删除确认行 wash 等）。
pub fn danger_alpha(t: &Theme, a: u32) -> u32 {
    (t.danger_hover << 8) | (a & 0xff)
}

/// danger at ~12% alpha (menu hover / confirm row wash).
pub fn danger_wash(t: &Theme) -> u32 {
    (t.danger_hover << 8) | 0x1f
}

/// `[data-theme="mist"]` — the default pi-web look.
pub const MIST: Theme = Theme {
    dark: false,
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

/// MiMo-V2.6-Pro 014「Kitchen Notebook」——奶油纸凸版食谱夹：纸底/栏线/
/// 校对红直接取 014 页面的 CSS 变量（--paper/--rule/--red/--mute），
/// 中间阶按 mist 的语义梯度插值（校准基准见文件头）。
pub const KITCHEN: Theme = Theme {
    dark: false,
    bg: 0xfbf5e9,       // --paper 奶油纸底
    bg_panel: 0xf4ead8, // --paper2 次级纸面
    bg_hover: 0xf0e4cc,
    bg_selected: 0xe8dab9,
    border: 0xd8cbb4, // --rule 栏线
    text: 0x241c14,   // --ink 墨黑棕
    text_muted: 0x4f4232,
    text_dim: 0x6b5c48,
    text_soft: 0x7a6a57, // --mute
    text_faint: 0x9b8d78,
    accent: 0xb8402d, // --red 校对红（014 唯一强调色）
    accent_hover: 0x9c3425,
    accent_contrast: 0xffffff,
    user_bg: 0xf4ead8,
    assistant_bg: 0xfbf5e9,
    tool_bg: 0xf2e7d3,
    bg_subtle: 0x241c140a, // rgba(36,28,20,0.04)
    chrome: 0xf0e5d0,
    nav: 0xfdf8ef,
    danger: 0xa5342a,
    danger_hover: 0xc85a44,
};

/// Catppuccin Latte（紫罗兰，移植自 Obsidian 主题 AnuPpuccin 的浅色底子）。
/// 中性灰阶原样取 Catppuccin Latte 官方色板——灰白底带 220° 蓝紫相（不是
/// 纯灰），12 档灰阶均匀，正文对底 7.06:1（WCAG AAA）。强调色取用户
/// Obsidian 自选 accent #5F5FAB（雾紫罗兰，链接对底 5.00:1、白字压底 5.66:1）。
/// 语义校准：user_bg = base 掺 10% accent；accent_hover 浅色系惯例加深
/// （hsl L52→42）；nav 取 base/mantle 之间（比 bg 略深 ~2%，对齐 mist 口径）。
pub const LATTE: Theme = Theme {
    dark: false,
    bg: 0xeff1f5,          // base
    bg_panel: 0xe6e9ef,    // mantle
    bg_hover: 0xdce0e8,    // crust
    bg_selected: 0xccd0da, // surface0
    border: 0xbcc0cc,      // surface1
    text: 0x4c4f69,        // text（subtext 系原色）
    text_muted: 0x5c5f77,  // subtext1
    text_dim: 0x6c6f85,    // subtext0
    text_soft: 0x8c8fa1,   // overlay1（跳过 overlay2，拉开与 dim 的档距）
    text_faint: 0x9ca0b0,  // overlay0
    accent: 0x5f5fab,      // 用户 Obsidian 自选 accent（雾紫罗兰）
    accent_hover: 0x4a4a8c,
    accent_contrast: 0xffffff,
    user_bg: 0xe1e2ee, // base + 10% accent（用户消息块淡紫）
    assistant_bg: 0xeff1f5,
    tool_bg: 0xe6e9ef, // mantle（bg 深 ~3%）
    bg_subtle: 0x0000000a,
    chrome: 0xdce0e8,       // crust（topbar/statusbar 比 panel 深）
    nav: 0xe9ecf2,          // base↔mantle 插值
    danger: 0xd20f39,       // Latte red
    danger_hover: 0xe64553, // Latte maroon（同向提亮）
};

/// `[data-theme="rose"]`
pub const ROSE: Theme = Theme {
    dark: false,
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
    ("kitchen", KITCHEN),
    ("mist", MIST),
    ("rose", ROSE),
    ("latte", LATTE),
    ("one-light", ONE_LIGHT),
    ("nord-dark", NORD_DARK),
    ("one-dark", ONE_DARK),
    ("pine", PINE_FOREST),
];

/// ZED "One Dark" — ported from `zed/crates/theme/src/fallback_themes.rs`
/// `zed_default_dark()` (the compile-time fallback family), HSLA -> RGB8.
/// One Light / nord port from the shipped JSONs in `assets/themes/`
/// (see gen_themes.py); translucent zed colors bake over the surface color.
/// 语义校准：border 提亮到 bg 之上（pi-web 分隔线语义）、text 阶梯拉开。
pub const ONE_DARK: Theme = Theme {
    dark: true,
    bg: 0x22252b,          // background hsla(215,12%,15%)
    bg_panel: 0x282c33,    // editor/toolbar hsla(220,12%,18%)
    bg_hover: 0x343843,    // element_hover 提亮收敛（原 3c404c 过亮）
    bg_selected: 0x3b3f4a, // element_selected hsla(224,11.3%,26.1%)
    border: 0x353a45,      // 亮于 bg（原 1b1d23 更暗 → 分隔线不可见）
    text: 0xd7dadf,        // text hsla(221,11%,86%)
    text_muted: 0x9ea4af,  // 相对 bg 对比 ~65%（原 6d737e 与 text 差距过大）
    text_dim: 0x828996,
    text_soft: 0x767d8a,
    text_faint: 0x6b7280,
    accent: 0x6189eb,       // text_accent blue hsla(222.6,77.5%,65.1%)
    accent_hover: 0x7ba0f0, // 深色系 hover 提亮
    accent_contrast: 0xffffff,
    user_bg: 0x2f333d,      // element_background hsla(223,13%,21%)
    assistant_bg: 0x262931, // elevated_surface hsla(225,12%,17%)
    tool_bg: 0x2a2e36,      // bg +~3% 亮（工具卡/代码块底）
    bg_subtle: 0xffffff14,
    chrome: 0x23262c,
    nav: 0x2b2f36,
    danger: 0xdf5561,
    danger_hover: 0xe0666f,
};

/// ZED 'One Light' — ported from zed assets/themes/one/one.json.
/// 语义校准：tool_bg 回到 bg+3% 深（原 = bg_selected 过深）。
pub const ONE_LIGHT: Theme = Theme {
    dark: false,
    bg: 0xfafafa,
    bg_panel: 0xf0f0f1,
    bg_hover: 0xdfdfe0,
    bg_selected: 0xd8d8da,
    border: 0xcfcfd1,
    text: 0x242529,
    text_muted: 0x58585a,
    text_dim: 0x7e8086,
    text_soft: 0x8e9096,
    text_faint: 0xa8aab0,
    accent: 0x5c78e2,
    accent_hover: 0x4a63c4, // 浅色系 hover 加深（原 c4daf6 反向变浅）
    accent_contrast: 0xffffff,
    user_bg: 0xf0f0f1,
    assistant_bg: 0xfafafa,
    tool_bg: 0xf0f0f1,
    bg_subtle: 0x0000000a,
    chrome: 0xe9e9eb,
    nav: 0xf4f4f5,
    danger: 0xc2545c,
    danger_hover: 0xd86570,
};

/// ZED 'Nord Dark' — ported from Zed nord extension nord.json.
/// 语义校准：bg_selected 降饱和（原 6c99a6 过艳）、text 阶梯拉开。
pub const NORD_DARK: Theme = Theme {
    dark: true,
    bg: 0x2e3440,
    bg_panel: 0x333a48,
    bg_hover: 0x3f4a5c,
    bg_selected: 0x4d5a72,
    border: 0x3f4859,
    text: 0xeceff4,
    text_muted: 0xbac5d5,
    text_dim: 0x9ba7bb,
    text_soft: 0x8b95a6,
    text_faint: 0x7e8aa0,
    accent: 0x88c0d0,
    accent_hover: 0xa3d4e2, // 深色系 hover 提亮
    accent_contrast: 0x2e3440,
    user_bg: 0x3b4252,
    assistant_bg: 0x2e3440,
    tool_bg: 0x353d4d,
    bg_subtle: 0xffffff14,
    chrome: 0x2a313d,
    nav: 0x343c4a,
    danger: 0xd06a75,
    danger_hover: 0xd97b85,
};

/// Pine Forest Green (Dark) — ported from VS Code 扩展 felix-tjernberg.
/// pine-forest-green（MIT，源 JSON 归档 assets/themes/pine-forest-green/）。
/// 单色系墨绿：底色 08190a 近黑松林绿，正文/灰阶全是白色/绿色半透明层
/// 按底色合成的实色（ffffffa1→a4aaa4 等）；唯一强调色 = 松针绿，原版
/// 2ecc40 微调降亮度/饱和到 29a83c（近黑底上防刺眼，用户实测反馈）。
/// 语义校准：原主题 frameless 无实体描边 → border 取 focusBorder 合成色
/// 124718；danger 系单色主题里没有 → 新增珊瑚红（语义必需）；accent_hover
/// 按深色系惯例提亮。
pub const PINE_FOREST: Theme = Theme {
    dark: true,
    bg: 0x08190a,           // editor.background 墨绿近黑
    bg_panel: 0x0b1f0d,     // 校准: bg 微亮（原主题全平面）
    bg_hover: 0x0b280e,     // list.hoverBackground #0e38137a 合成
    bg_selected: 0x165c1e,  // list.activeSelection #2ecc4060 合成（标志绿块）
    border: 0x124718,       // focusBorder #2ecc4042 合成（描边校准）
    text: 0xa4aaa4,         // editor.foreground #ffffffa1 合成
    text_muted: 0x929a93,   // sideBar.foreground #ffffff90 合成
    text_dim: 0x878888,     // foreground #bebebeb3 合成
    text_soft: 0x5c675d,    // statusBar.foreground #ffffff58 合成
    text_faint: 0x465247,   // 校准: ~25% 白合成
    accent: 0x29a83c,       // 松针绿（原 2ecc40，微调防刺眼）
    accent_hover: 0x3ec251, // 同步微调后的 hover 提亮
    accent_contrast: 0x08190a,
    user_bg: 0x103d15, // 校准: 20% 松针绿合成（用户消息块）
    assistant_bg: 0x08190a,
    tool_bg: 0x0a1e0c, // 校准: bg +~3% 亮
    bg_subtle: 0xffffff14,
    chrome: 0x061308, // 校准: 比 bg 略深（原主题 status/title = bg）
    nav: 0x0c220f,
    danger: 0xcf6a5f, // 校准新增: 单色主题无红，语义必需
    danger_hover: 0xe08378,
};

/// Active theme index; the UI re-reads it every render so a switch repaints
/// everything. `PI_FLASH_THEME=<name>` overrides the persisted choice for dev.
static THEME_IX: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Apply a theme by name; false when the name is unknown —— 未知名一律
/// 回退到注册表第一个主题（持久化配置里被删除/拼错的主题名不留半悬状态）。
pub fn set_by_name(name: &str) -> bool {
    if let Some(ix) = ALL.iter().position(|(n, _)| *n == name) {
        THEME_IX.store(ix, std::sync::atomic::Ordering::Relaxed);
        true
    } else {
        THEME_IX.store(0, std::sync::atomic::Ordering::Relaxed);
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

    /// gpui::rgba 是 0xRRGGBBAA（alpha 低字节）——v56 曾写反成 AARRGGBB
    /// 导致通道错位（工具卡藏青/参数区纯黄，用户实测发现）。
    #[test]
    fn rgba_a_layout_is_rrggbbaa() {
        assert_eq!(rgba_a(0x22c55e, 0.25), 0x22c55e40);
        assert_eq!(rgba_a(0xf87171, 0.45), 0xf8717173);
        assert_eq!(rgba_a(0x3b82f6, 0.2), 0x3b82f633);
        // bg_subtle 是全仓库唯一 8 位 RRGGBBAA 主题色，必须走 rgba() 不能
        // 走 rgb()（rgb 跳首字节，0xffffff14 会读成 #ffff14 纯黄）
        // 深色主题 bg_subtle=0xffffff14 → 白 8% alpha（而非 rgb() 误读的 #ffff14）
        let subtle = gpui::rgba(0xffffff14);
        assert!((subtle.r - 1.0).abs() < 1e-3);
        assert!((subtle.g - 1.0).abs() < 1e-3);
        assert!((subtle.b - 1.0).abs() < 1e-3);
        assert!((subtle.a - 0.0784).abs() < 1e-3);
    }

    #[test]
    fn switch_roundtrip_and_reject() {
        assert!(set_by_name("rose"));
        assert_eq!(theme_name(), "rose");
        assert_eq!(theme().bg, ROSE.bg);
        assert!(set_by_name("mist"));
        assert_eq!(theme_name(), "mist");
        // 未知/已删除主题名 → 回退到注册表第一个（不留半悬状态）
        assert!(!set_by_name("nope"));
        assert_eq!(theme_name(), ALL[0].0);
        assert_eq!(theme().bg, ALL[0].1.bg);
        assert!(set_by_name("mist"));
        assert_eq!(theme_name(), "mist");
        // all built-in themes have complete palettes (v54 layers incl.)
        for (name, t) in ALL {
            assert!(t.accent != 0, "{name} missing accent");
            assert!(t.border != 0, "{name} missing border");
            assert!(t.chrome != 0, "{name} missing chrome");
            assert!(t.nav != 0, "{name} missing nav");
            assert!(t.danger != 0, "{name} missing danger");
        }
    }

    /// 主题一致性不变量（所有主题相对 bg 的方向必须一致）：
    /// 浅色系 border/tool_bg/hover/selected 深于 bg；深色系亮于 bg；
    /// text 阶梯对比度相对 bg 单调递减。
    #[test]
    fn semantic_ladder_holds_for_all_themes() {
        fn lum(c: u32) -> f32 {
            let ch = |s: u32| -> f32 {
                let v = (s & 0xff) as f32 / 255.;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * ch(c >> 16) + 0.7152 * ch(c >> 8) + 0.0722 * ch(c)
        }
        for (name, t) in ALL {
            let dark = t.dark;
            let deeper = |x: u32| {
                if dark {
                    lum(x) > lum(t.bg)
                } else {
                    lum(x) < lum(t.bg)
                }
            };
            assert!(deeper(t.border), "{name}: border must contrast against bg");
            assert!(deeper(t.tool_bg), "{name}: tool_bg off-baseline vs bg");
            assert!(deeper(t.bg_hover), "{name}: bg_hover off-baseline vs bg");
            assert!(
                deeper(t.bg_selected),
                "{name}: bg_selected off-baseline vs bg"
            );
            // hover → selected 亮度递进（选中更显）
            let dh = (lum(t.bg) - lum(t.bg_hover)).abs();
            let ds = (lum(t.bg) - lum(t.bg_selected)).abs();
            assert!(
                ds > dh,
                "{name}: selected must differ from bg more than hover"
            );
            // text 阶梯：对比度单调递减
            let base = lum(t.bg);
            let dist = |c: u32| (lum(c) - base).abs();
            let (lt, lm, ld, lsoft, lfaint) = (
                dist(t.text),
                dist(t.text_muted),
                dist(t.text_dim),
                dist(t.text_soft),
                dist(t.text_faint),
            );
            assert!(lt > lm, "{name}: text vs muted");
            assert!(lm > ld, "{name}: muted vs dim");
            assert!(ld >= lsoft, "{name}: dim vs soft");
            assert!(lsoft > lfaint, "{name}: soft vs faint");
        }
    }
}
