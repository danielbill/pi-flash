//! 样式 run 分区器：把可能重叠的样式区间变成**按序、全覆盖、互不重叠**
//! 的 run 序列——`element.rs` 的 runs 是从可见区 0 起**累计消费**的连续
//! 序列（`runs_for_range` 按长度游走），间隙必须填默认样式，否则后续
//! run 全部错位（024 §2 缝1 的坐标约定）。
//!
//! 样式规格不在此复制：`markdown::highlight(Style, &Theme)` 是唯一来源
//! （pi-web globals.css 规格：strong = 700 + 88% accent 混色 …），调用方
//! 先把 `Style` 翻译成 `HighlightStyle` 再交给分区器。

use std::ops::Range;

use gpui::HighlightStyle;

/// 把 `overlaps`（可乱序、可重叠）分区为 `[0, total)` 的全覆盖序列。
///
/// - 覆盖多层的区间逐字段合并（后出现者覆盖先前的 Some 字段）
/// - 未覆盖间隙填 `HighlightStyle::default()`（全 None = 不改基础样式）
/// - `total == 0` → 空序列
pub fn partition(
    overlaps: Vec<(Range<usize>, HighlightStyle)>,
    total: usize,
) -> Vec<(Range<usize>, HighlightStyle)> {
    if total == 0 {
        return Vec::new();
    }
    let mut cuts: Vec<usize> = Vec::with_capacity(overlaps.len() * 2 + 2);
    cuts.push(0);
    cuts.push(total);
    for (r, _) in &overlaps {
        if r.start > 0 && r.start < total {
            cuts.push(r.start);
        }
        if r.end > 0 && r.end < total {
            cuts.push(r.end);
        }
    }
    cuts.sort_unstable();
    cuts.dedup();

    let mut out = Vec::with_capacity(cuts.len());
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let mut style = HighlightStyle::default();
        for (r, s) in &overlaps {
            if r.start <= a && r.end >= b {
                merge_style(&mut style, s);
            }
        }
        out.push((a..b, style));
    }
    out
}

/// 后者 Some 字段覆盖前者（字段级合并，gpui HighlightStyle 无 builder）。
pub fn merge_style(base: &mut HighlightStyle, b: &HighlightStyle) {
    if b.color.is_some() {
        base.color = b.color;
    }
    if b.font_weight.is_some() {
        base.font_weight = b.font_weight;
    }
    if b.font_style.is_some() {
        base.font_style = b.font_style;
    }
    if b.background_color.is_some() {
        base.background_color = b.background_color;
    }
    if b.underline.is_some() {
        base.underline = b.underline.clone();
    }
    if b.strikethrough.is_some() {
        base.strikethrough = b.strikethrough.clone();
    }
    if b.fade_out.is_some() {
        base.fade_out = b.fade_out;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{FontWeight, HighlightStyle};

    fn bold() -> HighlightStyle {
        HighlightStyle {
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        }
    }

    /// 校验分区不变式：有序、互不重叠、全覆盖 [0,total)。
    fn assert_partition(parts: &[(Range<usize>, HighlightStyle)], total: usize) {
        let mut expect = 0usize;
        for (r, _) in parts {
            assert_eq!(r.start, expect, "缝隙/乱序: {parts:?}");
            expect = r.end;
        }
        assert_eq!(expect, total, "未全覆盖: {parts:?}");
    }

    #[test]
    fn empty_input_yields_single_default_run() {
        let parts = partition(vec![], 10);
        assert_partition(&parts, 10);
        assert_eq!(parts.len(), 1);
        assert!(parts[0].1.font_weight.is_none());
    }

    #[test]
    fn simple_split_with_gaps() {
        // [0..2] base, [2..5] bold, [5..7] base, [7..9] bold, [9..10] base
        let parts = partition(vec![(2..5, bold()), (7..9, bold())], 10);
        assert_partition(&parts, 10);
        assert_eq!(parts.len(), 5);
        assert!(parts[1].0 == (2..5) && parts[1].1.font_weight.is_some());
        assert!(parts[3].0 == (7..9) && parts[3].1.font_weight.is_some());
        assert!(parts[0].1.font_weight.is_none());
    }

    #[test]
    fn overlapping_styles_merged_fieldwise() {
        let italic = HighlightStyle {
            font_style: Some(gpui::FontStyle::Italic),
            ..Default::default()
        };
        // 重叠区 [3..6)：bold + italic
        let parts = partition(vec![(1..6, bold()), (3..8, italic)], 10);
        assert_partition(&parts, 10);
        let mid = parts.iter().find(|(r, _)| *r == (3..6)).unwrap();
        assert!(mid.1.font_weight.is_some() && mid.1.font_style.is_some());
        // 只有 bold 的 [1..3)
        let b = parts.iter().find(|(r, _)| *r == (1..3)).unwrap();
        assert!(b.1.font_weight.is_some() && b.1.font_style.is_none());
        // 只有 italic 的 [6..8)
        let i = parts.iter().find(|(r, _)| *r == (6..8)).unwrap();
        assert!(i.1.font_weight.is_none() && i.1.font_style.is_some());
    }

    #[test]
    fn out_of_range_clipped() {
        // 超出 total 的部分不产生段、不破坏全覆盖
        let parts = partition(vec![(8..20, bold())], 10);
        assert_partition(&parts, 10);
    }

    #[test]
    fn zero_total_empty() {
        assert!(partition(vec![(0..5, bold())], 0).is_empty());
    }

    #[test]
    fn unsorted_input_handled() {
        let parts = partition(vec![(6..8, bold()), (1..3, bold())], 10);
        assert_partition(&parts, 10);
    }
}
