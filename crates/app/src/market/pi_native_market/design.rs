//! Pi 原生扩展市场 —— 设计稿 `docs/UI设计/仿-pi.dev-Packages-页面设计/pi-packages.html`
//! 的**字面量移植层**（040-2）。
//!
//! 本文件是这一页唯一的「外部真值」落点：设计稿里 `:root` 的 CSS 变量、`@layer
//! pages` 里 packages 段的尺寸，全部在这里翻译成常量。**页面代码不许再写真值**
//! ——要改观感，改这里，并且改完回写设计稿对照表（`docs/agents/market.md`）。
//!
//! 三条换算约定（设计稿 → 本文件）：
//!
//! 1. **rem → px**：设计稿 `html{font-size:18px}`，所以 `0.86rem = 15.48px`。
//!    这里直接写 18 进制算好的 px 值，等号后面的注释保留 rem 原式。
//! 2. **vw → 实测宽度**：设计稿用 `clamp()` / 媒体查询按**视口宽**取值；桌面应用
//!    里「视口」= 内容区宽度（窗口 − 侧栏），由 [`Metrics::new`] 传入。同一个
//!    clamp 公式照抄，保证与浏览器同宽时数值一致。
//! 3. **没有的东西不假装有**：gpui 0.2.2 无 `letter-spacing`、无 `aspect-ratio`、
//!    无 `box-shadow`——字距用 [`spaced`] 插细空格模拟、预览框按设计公式**自己算
//!    宽高**（[`Metrics::frame`]）、投影省略（20px 半径的黑 25%，深底上几乎不可见）。
//!    这三处是唯一有意偏离，改设计稿时若要动字距/预览比例，改这里。
//!
//! **共享约定（090-4）**：本文件的**几何部分**（[`Metrics`] / `sp` / `px18` /
//! [`spaced`] / `MONO`）同时被 PF 插件市场页（`crate::market::pf_market`）复用
//! ——那边只要求 layout 对齐本页，因此几何真值只落这一处；**色板不共享**（PF 页
//! 走主题语义 token，见该文件头的映射表）。改几何时两页同时生效，属预期。

use gpui::Rgba;

use crate::appearance::theme::rgba_a;

// ---------------------------------------------------------------------------
// 色板（设计稿 :root 原值；面板类都是 rgba，括号里是 hex8 原样）
// ---------------------------------------------------------------------------

/// 页面底色 `--bg-deep`
pub const BG_DEEP: u32 = 0x0d1116;
/// 面板底 `--panel: #212730e6`
const PANEL_RGB: u32 = 0x212730;
const PANEL_A: f32 = 0.902;
/// 面板软底 `--panel-soft: #252f3dd1`（选择控件 / 预览框）
const PANEL_SOFT_RGB: u32 = 0x252f3d;
const PANEL_SOFT_A: f32 = 0.820;
/// 描边 `--line: #49505980`
const LINE_RGB: u32 = 0x495059;
const LINE_A: f32 = 0.502;
/// 强描边 `--line-strong: #757d8975`（卡片外框 / 命令条）
const LINE_STRONG_RGB: u32 = 0x757d89;
const LINE_STRONG_A: f32 = 0.459;
/// 正文。**注意**：token 表写的是 `--text: #ebe7e4`，但浏览器实测 body/标题/
/// 包名的实际取色是 `#d5d8db @0.96`（后置层覆盖了 `--text`）。设计稿为准 =
/// 实测为准，这里按实测值入册。
pub const TEXT: u32 = 0xd5d8db;
pub const TEXT_A: f32 = 0.960;
/// 段落正文（实测 `#d5d8db @0.84`，对应 `--copy`）
pub const COPY: u32 = 0xd5d8db;
pub const COPY_A: f32 = 0.840;
/// 弱化文字 `--muted: #9fa4abad`（占比符/页内灰；深肤页的占位符也用这支）
pub const MUTED: u32 = 0x9fa4ab;
const MUTED_A: f32 = 0.678;
/// 次级文字 `--muted-strong: #d5d8dbcc`
const MUTED_STRONG_RGB: u32 = 0xd5d8db;
const MUTED_STRONG_A: f32 = 0.800;
/// 主色 `--accent: #6a9fcc`
pub const ACCENT: u32 = 0x6a9fcc;
/// 控件底 `--control-background: #ffffff04`
const CONTROL_RGB: u32 = 0xffffff;
const CONTROL_A: f32 = 0.016;
/// 控件 hover 底 `--control-background-hover: #ffffff09`
const CONTROL_HOVER_A: f32 = 0.035;
/// 悬浮洗色 `--surface-tint: #ffffff05`
const TINT_A: f32 = 0.020;
/// 类型徽标四色（extension/skill/theme/prompt）
const SUCCESS: u32 = 0x5db87a;
const WARNING: u32 = 0xe8993a;
const ERROR: u32 = 0xe8704f;

/// npm / github / report 三个链接图标色 = `--muted`
pub fn muted() -> Rgba {
    gpui::rgba(rgba_a(MUTED, MUTED_A))
}

/// 正文色
pub fn text() -> Rgba {
    gpui::rgba(rgba_a(TEXT, TEXT_A))
}

/// 段落色（`--copy`）
pub fn copy() -> Rgba {
    gpui::rgba(rgba_a(COPY, COPY_A))
}

/// 次级文字（`--muted-strong`）
pub fn muted_strong() -> Rgba {
    gpui::rgba(rgba_a(MUTED_STRONG_RGB, MUTED_STRONG_A))
}

/// 描边
pub fn line() -> Rgba {
    gpui::rgba(rgba_a(LINE_RGB, LINE_A))
}

/// 强描边
pub fn line_strong() -> Rgba {
    gpui::rgba(rgba_a(LINE_STRONG_RGB, LINE_STRONG_A))
}

/// 面板底
pub fn panel() -> Rgba {
    gpui::rgba(rgba_a(PANEL_RGB, PANEL_A))
}

/// 软面板底
pub fn panel_soft() -> Rgba {
    gpui::rgba(rgba_a(PANEL_SOFT_RGB, PANEL_SOFT_A))
}

/// 控件底
pub fn control_bg() -> Rgba {
    gpui::rgba(rgba_a(CONTROL_RGB, CONTROL_A))
}

/// 控件 hover 底
pub fn control_bg_hover() -> Rgba {
    gpui::rgba(rgba_a(CONTROL_RGB, CONTROL_HOVER_A))
}

/// 悬浮洗色
pub fn tint() -> Rgba {
    gpui::rgba(rgba_a(CONTROL_RGB, TINT_A))
}

/// accent 半透明（hover 描边用；设计 `--accent-border-hover: #6a9fcc6b`）
pub fn accent_alpha(a: f32) -> Rgba {
    gpui::rgba(rgba_a(ACCENT, a))
}

/// 类型徽标的（描边, 底, 字）三色。`kind` = pi 清单资源类型；未知 = package。
pub fn badge_colors(kind: Option<&str>) -> (Rgba, Rgba, Rgba) {
    let (rgb, border_a, bg_a) = match kind {
        Some("extension") => (SUCCESS, 0.52, 0.14),
        Some("skill") => (ACCENT, 0.52, 0.14),
        Some("theme") => (ERROR, 0.50, 0.12),
        Some("prompt") => (WARNING, 0.50, 0.12),
        Some("subagent") => (SUCCESS, 0.52, 0.14),
        // package（无 pi 清单）/ 未知
        _ => return (line(), gpui::rgba(rgba_a(0x000000, 0.0)), muted_strong()),
    };
    (
        gpui::rgba(rgba_a(rgb, border_a)),
        gpui::rgba(rgba_a(rgb, bg_a)),
        gpui::rgb(rgb),
    )
}

// ---------------------------------------------------------------------------
// 字体槽
// ---------------------------------------------------------------------------

/// 等宽（设计 `--mono` / `--accent-mono`）。设计稿首选 Departure Mono / Commit
/// Mono 都没装，落到 `JetBrains Mono`——正是 PF 内置那一份，浏览器与本应用
/// 实测同一字形。
pub const MONO: &str = "JetBrains Mono";

/// 衬线（设计 `--serif`：Plantin 系 → Georgia）。Windows 无 Plantin，
/// 浏览器里实际渲染的就是 Georgia，故取同名族精确对齐。
pub const SERIF: &str = "Georgia";

// ---------------------------------------------------------------------------
// 尺寸（18px rem 进制 / 设计稿 clamp 原式）
// ---------------------------------------------------------------------------

pub mod px18 {
    /// 0.62rem —— 最近发布条目的时间戳
    pub const T12SMALL: f32 = 11.16;
    /// 0.622222rem —— 类型徽标
    pub const BADGE_FS: f32 = 11.2;
    /// 0.68rem —— meta 行 / 链接
    pub const META_FS: f32 = 12.24;
    /// 0.7rem —— 翻页条
    pub const PAGER_FS: f32 = 12.6;
    /// 0.72rem —— 最近发布条目名 / 按钮字
    pub const BTN_FS: f32 = 12.96;
    /// 0.76rem —— 结果计数
    pub const COUNT_FS: f32 = 13.68;
    /// 0.82rem —— 卡片标题
    pub const TITLE_FS: f32 = 14.76;
    /// 0.84rem —— 包描述
    pub const DESC_FS: f32 = 15.12;
    /// 0.86rem —— 包名
    pub const NAME_FS: f32 = 15.48;
    /// 1rem —— 正文 / 表单标签（body 继承）
    pub const BODY_FS: f32 = 18.;
    /// 15rem/18 —— 输入控件字（设计 `--form-control-font-size`）
    pub const FORM_FS: f32 = 15.;
}

/// 卡片外壳（radius 0 / 1px 描边）与公共间距
pub mod sp {
    /// 卡片头/体内距三档（clamp(0.85rem, 1.4vw, 1.15rem) / (…, 1.05rem)）
    pub const CARD_PAD_MIN: f32 = 15.3;
    pub const CARD_PAD_HEAD_MAX: f32 = 20.7;
    pub const CARD_PAD_BODY_MAX: f32 = 18.9;
    /// 外壳内距（clamp(1rem, 3vw, 2rem)）
    pub const SHELL_PAD_MIN: f32 = 18.;
    pub const SHELL_PAD_MAX: f32 = 36.;
    /// 外壳底内距 4rem
    pub const SHELL_PAD_BOTTOM: f32 = 72.;
    /// 内容外壳宽度上限 82.5rem
    pub const SHELL_MAX_W: f32 = 1485.;
    /// 卡片之间 / shell grid gap 1rem
    pub const BLOCK_GAP: f32 = 18.;
    /// 最近发布网格 gap 0.75rem
    pub const RECENT_GAP: f32 = 13.5;
    /// 最近发布条目内距 0.72rem
    pub const RECENT_PAD: f32 = 12.96;
    /// 最近发布条目行间 0.24rem
    pub const RECENT_ROW_GAP: f32 = 4.32;
    /// 卡片头内距 0.78rem（纵向）
    pub const HEADER_PAD_Y: f32 = 14.04;
    /// index 卡体内 grid gap 0.9rem
    pub const BODY_GAP: f32 = 16.2;
    /// 工具栏 gap 0.7rem
    pub const BAR_GAP: f32 = 12.6;
    /// 控件高 44px（设计 `--action-bar-control-height`）
    pub const CONTROL_H: f32 = 44.;
    /// 表单控件内距 8 / 13
    pub const CONTROL_PAD_X: f32 = 13.;
    /// 下拉箭头视觉尺寸 1.5rem，右侧偏移 = 13 − 0.4375rem(7.875)
    pub const CARET: f32 = 27.;
    pub const CARET_INSET: f32 = 5.125;
    /// 工具栏按钮内距 0.85rem
    pub const BTN_PAD_X: f32 = 15.3;
    /// 包卡预览列 / 预览内距
    pub const PREVIEW_PAD: f32 = 12.96;
    pub const FRAME_PAD: f32 = 9.9;
    pub const FRAME_GAP: f32 = 5.76;
    pub const FRAME_MAX_W: f32 = 158.4;
    /// 包卡体内距 0.85rem→1.05rem 与元素间距 0.55rem
    pub const PKG_PAD: f32 = 18.9;
    pub const PKG_GAP: f32 = 9.9;
    /// 包描述行高 1.45 × 15.12
    pub const DESC_LINE_H: f32 = 21.92;
    /// 徽标内距 / 行高 0.08em
    pub const BADGE_PAD_Y: f32 = 2.52;
    pub const BADGE_PAD_X: f32 = 9.;
    pub const BADGE_LINE_H: f32 = 14.;
    /// 链接图标 0.78rem + 文字 0.32rem 间距 + 项间 0.8rem
    pub const LINK_ICON: f32 = 14.;
    pub const LINK_GAP: f32 = 5.76;
    pub const LINK_ITEM_GAP: f32 = 14.4;
    /// 命令条内距 0.55 / 0.6 / 0.72rem、行内 gap 0.55rem、code 行高 1.35
    pub const INSTALL_PAD_Y: f32 = 9.9;
    pub const INSTALL_PAD_R: f32 = 10.8;
    pub const INSTALL_PAD_L: f32 = 12.96;
    pub const INSTALL_GAP: f32 = 9.9;
    pub const CODE_FS: f32 = 12.96;
    pub const CODE_LINE_H: f32 = 17.5;
    /// 复制钮高 1.85rem / 内距 0.28·0.52rem / 字 0.611rem
    pub const COPY_H: f32 = 33.3;
    pub const COPY_PAD_X: f32 = 9.36;
    pub const COPY_FS: f32 = 11.;
    /// 翻页条上内距 0.9rem、gap 0.8rem、格子 2.15rem、内距 0.5·0.62rem
    pub const PAGER_PT: f32 = 16.2;
    pub const PAGER_GAP: f32 = 14.4;
    pub const PAGER_CELL: f32 = 38.7;
    pub const PAGER_CELL_MIN_W: f32 = 122.4;
    pub const PAGER_PAD_X: f32 = 11.16;
    /// 窄屏（<768）预览条高 5.8rem
    pub const NARROW_PREVIEW_H: f32 = 104.4;
}

/// 字距模拟：设计稿用 `letter-spacing`（uppercase 小标题 0.16em、按钮 0.12em、
/// 徽标 0.08em）。gpui 0.2.2 无字距支持，这里在字符间插 U+2009(THIN SPACE,
/// ≈0.2em) / U+200A(HAIR SPACE, ≈0.1em) 近似。
///
/// 只用于**全大写短标签**（标题 / 按钮 / 徽标 / 时间戳）——正文套它会破坏断行
/// 与查词，绝不外传。
pub fn spaced(s: &str, wide: bool) -> String {
    let sep = if wide { '\u{2009}' } else { '\u{200a}' };
    let mut out = String::with_capacity(s.len() * 2);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 {
            out.push(sep);
        }
        out.push(ch);
    }
    out
}

/// 一次渲染的几何（全部由**内容区宽度**推出；设计稿里这些量都挂 vw/clamp）。
#[derive(Debug, Clone, Copy)]
pub struct Metrics {
    /// 内容区宽度（窗口 − 侧栏；= 设计稿里的 `100vw`）
    #[allow(dead_code)]
    pub content_w: f32,
    /// 外壳水平内距 clamp(1rem, 3vw, 2rem)
    pub shell_pad: f32,
    /// 卡片头内距（水平）clamp(0.85rem, 1.4vw, 1.15rem)
    pub card_pad_head: f32,
    /// 卡片体内距 clamp(0.85rem, 1.4vw, 1.05rem)
    pub card_pad_body: f32,
    /// 卡片外框宽（不含外边）
    #[allow(dead_code)]
    pub card_w: f32,
    /// 预览列宽 = 28% 卡宽（下限 7.2rem）
    pub preview_col_w: f32,
    /// 预览框（16:10）
    pub frame_w: f32,
    pub frame_h: f32,
    /// 窄屏（<768）= 单列卡、预览转顶部
    pub narrow: bool,
    /// 工具栏是否折成一列（<1024）
    pub stacked_bar: bool,
}

impl Metrics {
    /// 外壳内容列宽（= min(内容宽, 82.5rem) − 两侧内距）：分段各自定宽居中
    pub fn shell_inner_w(&self) -> f32 {
        (self.content_w - self.shell_pad * 2.)
            .min(sp::SHELL_MAX_W - self.shell_pad * 2.)
            .max(1.)
    }

    /// 内容列两侧留白（= 外壳内距，超宽窗口时承担整体居中）：
    /// 列表项里**不用**首尾 `justify_center` 居中——列表项宽度不定，
    /// 居中会静默失效（实测最近发布段贴左、index 段居中，「最上面歪了」）。
    pub fn side_pad(&self) -> f32 {
        ((self.content_w - self.shell_inner_w()) / 2.).max(0.)
    }

    /// 包卡总高（设计稿 `article[data-package-card]` 的 `height:100%` 实测
    /// 285.59@1440：内距 18.9×2 + 名称 1.35em + 描述 3×1.45em + meta +
    /// 徽标 + 链接 + 命令条 + 边框 2，行间 0.55rem×5）。
    ///
    /// **必须定高**：GPUI 的 `line_clamp` 只截视觉行，盒子的布局高仍按整段
    /// 文本算——不定高的话卡片会被长描述撑高（这就是「卡片比设计稿高 46px」
    /// 的根因，2026-10-22 实测）。
    pub fn card_h(&self) -> f32 {
        let body = sp::PKG_PAD * 2.
            + px18::NAME_FS * 1.35
            + sp::PKG_GAP
            + sp::DESC_LINE_H * 3.
            + sp::PKG_GAP
            + px18::META_FS * 1.6
            + sp::PKG_GAP
            + (sp::BADGE_LINE_H + sp::BADGE_PAD_Y * 2. + 2.)
            + sp::PKG_GAP
            + sp::LINK_ICON
            + sp::PKG_GAP
            + (sp::COPY_H + sp::INSTALL_PAD_Y * 2. + 2.)
            + 2.;
        if self.narrow {
            sp::NARROW_PREVIEW_H + body
        } else {
            body
        }
    }
}

impl Metrics {
    pub fn new(content_w: f32) -> Self {
        let w = content_w.max(1.);
        let vw = w / 100.;
        let shell_pad = (3. * vw).clamp(sp::SHELL_PAD_MIN, sp::SHELL_PAD_MAX);
        let card_pad_head = (1.4 * vw).clamp(sp::CARD_PAD_MIN, sp::CARD_PAD_HEAD_MAX);
        let card_pad_body = (1.4 * vw).clamp(sp::CARD_PAD_MIN, sp::CARD_PAD_BODY_MAX);
        let inner_w = w.min(sp::SHELL_MAX_W) - shell_pad * 2.;
        let narrow = w < 768.;
        let cols = if narrow { 1. } else { 2. };
        // 网格所在的「index 卡」自己带 1px 边框 + 体内距，两侧各吃一份
        let grid_w = inner_w - 2. - card_pad_body * 2.;
        let card_w = ((grid_w - sp::BLOCK_GAP * (cols - 1.)) / cols).max(1.);
        // 预览列 = minmax(7.2rem, 28%)
        let preview_col_w = (card_w * 0.28).max(129.6).min(card_w);
        // 预览框宽 = 列宽 − 内距×2 − 右边框，封顶 8.8rem；16:10
        let frame_w = (preview_col_w - sp::PREVIEW_PAD * 2. - 1.).clamp(24., sp::FRAME_MAX_W);
        let frame_h = frame_w / 1.6;
        Self {
            content_w: w,
            shell_pad,
            card_pad_head,
            card_pad_body,
            card_w,
            preview_col_w,
            frame_w,
            frame_h,
            narrow,
            stacked_bar: w < 1024.,
        }
    }
}

/// 最近发布条目高：内距 + 名字行（1.6 行高）+ 摘要行 + 时间行 + 间隙 + 边框
pub fn recent_item_h() -> f32 {
    sp::RECENT_PAD * 2.
        + 1.
        + px18::BTN_FS * 1.6
        + sp::RECENT_ROW_GAP
        + 13.5 * 1.35
        + sp::RECENT_ROW_GAP
        + px18::T12SMALL * 1.6
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_match_design_at_1440() {
        // 设计稿实测（1440 视口 / 内容 1403）：外壳内距 36、卡片 635.66、
        // 预览列 177.42、预览框 150.52×94.06。
        let m = Metrics::new(1403.);
        assert_eq!(m.shell_pad, 36.);
        assert!((m.card_w - 635.66).abs() < 1.0, "card_w={}", m.card_w);
        assert!(
            (m.preview_col_w - 177.42).abs() < 1.0,
            "preview={}",
            m.preview_col_w
        );
        assert!((m.frame_w - 150.52).abs() < 1.0, "frame_w={}", m.frame_w);
        assert!((m.frame_h - 94.06).abs() < 1.0, "frame_h={}", m.frame_h);
    }

    #[test]
    fn card_height_matches_design() {
        // 设计实测 285.59（1440 视口）
        let m = Metrics::new(1403.);
        assert!((m.card_h() - 285.59).abs() < 2.0, "card_h={}", m.card_h());
    }

    #[test]
    fn recent_item_height_is_stable() {
        // 最近发布条目自然高 ≈93.3（设计实测 93.31）；骨架屏用它占位
        assert!(
            (recent_item_h() - 93.3).abs() < 2.0,
            "h={}",
            recent_item_h()
        );
    }

    #[test]
    fn narrow_switches_single_column() {
        assert!(Metrics::new(700.).narrow);
        assert!(!Metrics::new(900.).narrow);
        assert!(Metrics::new(900.).stacked_bar);
        assert!(!Metrics::new(1100.).stacked_bar);
    }

    #[test]
    fn spaced_inserts_between_chars_only() {
        assert_eq!(spaced("AB", true), "A\u{2009}B");
        assert_eq!(spaced("A", true), "A");
        assert_eq!(spaced("", true), "");
    }
}

// ---------------------------------------------------------------------------
// 时间戳文案（设计稿 "10m ago" / "2h ago" / "1d ago"）
// ---------------------------------------------------------------------------

/// ISO 8601 → unix 秒（只吃 `YYYY-MM-DDTHH:MM:SS` 前缀；解析失败 None）。
fn parse_unix(iso: &str) -> Option<i64> {
    let date = iso.get(0..10)?;
    let time = iso.get(11..19).unwrap_or("00:00:00");
    let y = date.get(0..4)?.parse::<i64>().ok()?;
    let m = date.get(5..7)?.parse::<i64>().ok()?;
    let d = date.get(8..10)?.parse::<i64>().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let hh = time.get(0..2)?.parse::<i64>().ok()?;
    let mm = time.get(3..5)?.parse::<i64>().ok()?;
    let ss = time.get(6..8)?.parse::<i64>().ok()?;
    Some(days_from_civil(y, m, d) * 86_400 + hh * 3600 + mm * 60 + ss)
}

/// 公历 → 自 1970-01-01 起的天数（Howard Hinnant；与 pi-link 同款，避免为一个
/// 展示函数把纯函数提到协议层）。
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 「10m ago / 3h ago / 2d ago」（设计稿时间戳口径：分钟→小时→天）。
pub fn ago_label(iso: &str) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let Some(t) = parse_unix(iso) else {
        return String::new();
    };
    let secs = (now - t).max(0);
    if secs < 3_600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3_600)
    } else {
        format!("{}d ago", secs / 86_400)
    }
}
