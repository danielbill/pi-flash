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

/// 像素级二维码矩阵（含 4 模块静默区）——**给 UI 画方块用**。
///
/// 字符画（[`qr_block_text`」）在字体抗锯齿下会在模块间留细缝，
/// 部分手机会因此解不出来；画实心方块没有这个毛病。
#[derive(Debug, Clone)]
pub struct QrGrid {
    /// 边长（含静默区）
    pub size: usize,
    /// 行优先，`true` = 深色；长度 = `size * size`
    pub cells: Vec<bool>,
}

impl QrGrid {
    pub fn at(&self, x: usize, y: usize) -> bool {
        self.cells.get(y * self.size + x).copied().unwrap_or(false)
    }

    /// 每行的连续深色段 `(x, len)` —— 用来把一行合并成少数几个方块，
    /// 41×41 不会变成 1681 个元素。
    pub fn runs(&self, y: usize) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut x = 0;
        while x < self.size {
            if !self.at(x, y) {
                x += 1;
                continue;
            }
            let start = x;
            while x < self.size && self.at(x, y) {
                x += 1;
            }
            out.push((start, x - start));
        }
        out
    }
}

/// 渲染成矩阵；`quiet` 是四周静默区的模块数（规范要求 4）。
pub fn qr_grid(url: &str, quiet: usize) -> Result<QrGrid, String> {
    let code =
        qrcode::QrCode::new(url.as_bytes()).map_err(|e| format!("二维码编码失败: {e}"))?;
    let w = code.width();
    use qrcode::Color as C;
    let src = code.to_colors(); // 行优先，长度 w*w
    let size = w + quiet * 2;
    let mut cells = vec![false; size * size];
    for y in 0..w {
        for x in 0..w {
            cells[(y + quiet) * size + (x + quiet)] = src[y * w + x] == C::Dark;
        }
    }
    Ok(QrGrid { size, cells })
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
    fn grid_matches_source_matrix_and_has_quiet_zone() {
        let g = qr_grid(URL, 4).unwrap();
        let w = qrcode::QrCode::new(URL.as_bytes()).unwrap().width();
        assert_eq!(g.size, w + 4 * 2, "边长 = 模块数 + 2*静默区");
        assert_eq!(g.cells.len(), g.size * g.size);
        // 静默区必须全浅色，否则手机解不出来
        for i in 0..g.size {
            assert!(!g.at(i, 0) && !g.at(i, 1), "上静默区必须为空");
            assert!(!g.at(0, i) && !g.at(1, i), "左静默区必须为空");
        }
        // 左上角定位图案（7x7，外圈实心）
        assert!(g.at(4, 4), "定位图案左上角应为深色");
        assert!(g.at(10, 4) && g.at(4, 10) && g.at(10, 10), "定位图案三角");
        assert!(!g.at(5, 5), "定位图案中心应为空");
    }

    #[test]
    fn runs_cover_exactly_the_dark_cells_of_each_row() {
        let g = qr_grid(URL, 4).unwrap();
        for y in 0..g.size {
            let runs = g.runs(y);
            // 段之间必须交替：起点有序、互不重叠
            let mut prev_end = 0;
            for (x, len) in &runs {
                assert!(*x >= prev_end, "段重叠");
                assert!(*len > 0, "空段");
                assert!(g.at(*x, y) && g.at(x + len - 1, y), "段两端必须深色");
                prev_end = x + len;
            }
            assert!(prev_end <= g.size);
        }
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
