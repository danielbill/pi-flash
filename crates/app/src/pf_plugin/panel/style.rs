//! 组件树 gpui 渲染层·样式助手：分区/分组标题、发丝线、主题色名与 tone
//! 词表 → 色值映射、`#hex` 解析（canvas fill）。

use super::*;

/// 分区标题：图标 + 文字，**左对齐到内容左边**（外壳 `PANEL_PAD` 由
/// `panel-frame` 统一给，这里不再叠一层），上方留 `SECTION_HEAD_PT`。
pub(super) fn section_header(
    icon_name: Option<&str>,
    label: &str,
    align: &str,
    pad: f32,
    t: &'static Theme,
    plugin_id: &str,
) -> gpui::AnyElement {
    let mut row = div()
        .flex_none()
        .flex()
        .items_center()
        // 对齐与留白由插件给意图（titleAlign / titlePad），像素在这里算：
        // 外壳已经有 PANEL_PAD，插件要的距离比它大就补差额，小则不再外扩
        .gap(px(6.))
        .pt(px(SECTION_HEAD_PT))
        .pb(px(2.));
    let inset = (pad - PANEL_PAD).max(0.);
    row = match align {
        "end" => row.justify_end().pr(px(inset)),
        "center" => row.justify_center(),
        _ => row.justify_start(),
    };
    if let Some(name) = icon_name {
        row = row.child(
            gpui::svg()
                .path(SharedString::from(icon_asset(name, plugin_id)))
                .text_color(rgb(t.text_faint))
                .size(px(crate::ui::tokens::icon::MD)),
        );
    }
    row.child(
        div()
            // 标题字号 = 面板正文（UI 规范 §2 body = ui(12)），不再用 9.5 小字
            .text_size(ui_size(12.))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(rgb(t.text_faint))
            .child(SharedString::from(label.to_string())),
    )
    .into_any_element()
}

/// `#rrggbb` / `#rgb` → 0xRRGGBB；非法 fill 返回 None（跳过上色，不留黑块）。
pub(super) fn hex_color(s: &str) -> Option<u32> {
    let h = s.strip_prefix('#')?;
    let (r, g, b) = match h.len() {
        6 => (
            u8::from_str_radix(&h[0..2], 16).ok()?,
            u8::from_str_radix(&h[2..4], 16).ok()?,
            u8::from_str_radix(&h[4..6], 16).ok()?,
        ),
        3 => {
            let c =
                |i: usize| -> Option<u8> { Some(u8::from_str_radix(&h[i..i + 1], 16).ok()? * 17) };
            (c(0)?, c(1)?, c(2)?)
        }
        _ => return None,
    };
    Some(((r as u32) << 16) | ((g as u32) << 8) | b as u32)
}

pub(super) fn group_header(label: &str, t: &'static Theme) -> gpui::AnyElement {
    div()
        .h(px(24.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .text_size(ui_size(12.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(label.to_string())),
        )
        .into_any_element()
}

/// 发丝线：主题 border 在暗色下与底色太近（看不清），统一用 text 的低透明度。
pub(super) fn hairline(t: &'static Theme) -> gpui::Rgba {
    gpui::rgba(crate::appearance::theme::rgba_a(t.text, 0.18))
}

/// 主题色名 → 色值（column.bg / column.border 用；与 tone 词表同一套 + 补充）。
pub(super) fn theme_color(name: &str, t: &'static Theme) -> u32 {
    match name {
        "bg" => t.bg,
        "bg_panel" => t.bg_panel,
        "chrome" => t.chrome, // topbar/statusbar 同层
        "border" => crate::appearance::theme::border_alpha(t, 0xC0),
        "white" => 0xffffff,
        _ => tone_color(name, t),
    }
}

pub(super) fn tone_color(tone: &str, t: &'static Theme) -> u32 {
    match tone {
        "text" => t.text,
        "text.muted" => t.text_muted,
        "text.faint" => t.text_faint,
        "accent" => t.accent,
        "danger" => t.danger,
        _ => t.text_dim, // text.dim 默认
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_color_parses_and_rejects() {
        assert_eq!(hex_color("#111827"), Some(0x111827));
        assert_eq!(hex_color("#abc"), Some(0xaabbcc));
        assert_eq!(hex_color("red"), None);
        assert_eq!(hex_color("#12345"), None);
        assert_eq!(hex_color(""), None);
    }
}
