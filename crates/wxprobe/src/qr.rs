//! 二维码渲染（P0 的 `render_qr` 抽成 lib，供 CLI 与 GPUI 设置页共用）。
//!
//! 两条通路：
//! * [`qr_block_text`] —— 半块字符（`▀▄█`，一字符 = 2 个竖向模块），
//!   **零依赖图片解码**就能显示，是设置页扫码面板用的那条
//! * [`qr_svg`] —— 320×320 SVG，写盘后浏览器打开扫码

use qrcode::render::unicode::Dense1x2;

/// 半块字符二维码。`module_w` 是每个模块的横向字符宽（1 = 最紧凑）。
///
/// 显示端用等宽字体即可扫；GPUI 里字号给足 14px 以上。
pub fn qr_block_text(url: &str, module_w: usize) -> Result<String, String> {
    let code = qrcode::QrCode::new(url.as_bytes())
        .map_err(|e| format!("二维码编码失败: {e}"))?;
    // Dense1x2 自带竖向半块配对：module_dimensions(1, N) 只调横向密度
    Ok(code
        .render::<Dense1x2>()
        .min_dimensions((module_w * 8) as u32, 0)
        .quiet_zone(true)
        .build())
}

/// 320×320 的 SVG 文本（落盘后用浏览器打开 / 交给有 SVG 能力的渲染端）。
pub fn qr_svg(url: &str) -> Result<String, String> {
    let code = qrcode::QrCode::new(url.as_bytes())
        .map_err(|e| format!("二维码编码失败: {e}"))?;
    Ok(code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(320, 320)
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://ilinkai.weixin.qq.com/ilink/bot/qr?x=1234567890";

    #[test]
    fn block_text_is_rectangular_and_only_half_block_glyphs() {
        let art = qr_block_text(URL, 2).unwrap();
        let lines: Vec<&str> = art.lines().collect();
        assert!(lines.len() > 10, "太短，不像二维码：{}", lines.len());
        let w = lines[0].chars().count();
        assert!(w > 10, "太窄：{w}");
        for l in &lines {
            assert_eq!(l.chars().count(), w, "必须是等宽矩形（GPUI 文本渲染按等宽假设）");
            assert!(
                l.chars().all(|c| c == '█' || c == '▀' || c == '▄' || c == ' '),
                "出现半块字形之外的字符：{l:?}"
            );
        }
    }

    #[test]
    fn block_text_has_quiet_zone_and_finder_pattern() {
        let art = qr_block_text(URL, 2).unwrap();
        let lines: Vec<&str> = art.lines().collect();
        // 前几行必须是静默区（全空），否则扫不出来
        let head = lines
            .iter()
            .take_while(|l| l.trim().is_empty())
            .count();
        assert!(head >= 2, "缺静默区：只有 {head} 行空白");
        // 左上角定位图案第一行 = 实心-实心对 → 连续 █ 起头
        // 跳过静默区的前导空白，第一行有效内容就是左上角定位图案
        let finder = lines[head].trim_start();
        assert!(
            finder.starts_with("█▀▀▀▀▀█"),
            "左上角定位图案丢失：{finder:?}"
        );
    }

    #[test]
    fn same_input_is_deterministic() {
        assert_eq!(qr_block_text(URL, 2).unwrap(), qr_block_text(URL, 2).unwrap());
    }

    #[test]
    fn svg_output_looks_like_svg() {
        let svg = qr_svg(URL).unwrap();
        assert!(svg.starts_with("<?xml") || svg.contains("<svg"), "不是 SVG: {}", &svg[..40.min(svg.len())]);
        assert!(svg.contains("path") || svg.contains("rect"));
    }

    #[test]
    fn absurdly_long_input_is_rejected_not_panicking() {
        // 超出 QR 容量 → 返回 Err，不能 panic（设置页要能显示错误）
        let long = "x".repeat(6000);
        assert!(qr_block_text(&long, 2).is_err());
        assert!(qr_svg(&long).is_err());
    }
}
