//! 折叠集操作：合并、行级 reveal、doc↔vis 映射委托。
//!
//! 映射核心（`Decorations::map_doc_to_vis` / `map_vis_to_doc`）收口在
//! vendor `input/decorations.rs`（element.rs 光标、movement 方向键、点击
//! 反算都就地用它）；本模块是 app 侧入口 + **主力测试区**——所有光标/
//! 映射行为的性质测试在此，防错位 bug 散落。
//!
//! P0 reveal 粒度 = **光标所在行**（024 §6.2）：该行 folds 全部显现，
//! 其余行保持隐藏；P2 细化到光标所在行内元素。

use std::ops::Range;

use gpui_component::input::{Decorations, SectionFold};

use super::parse::HeadingLine;

/// 合并重叠/相邻的折叠区间（parse 产出的嵌套强调 fold 会重叠）。
/// 输入不要求有序。
pub fn merge(mut folds: Vec<Range<usize>>) -> Vec<Range<usize>> {
    folds.retain(|f| f.start < f.end); // 丢空段
    folds.sort_by_key(|f| f.start);
    let mut out: Vec<Range<usize>> = Vec::with_capacity(folds.len());
    for f in folds {
        match out.last_mut() {
            Some(last) if f.start <= last.end => {
                last.end = last.end.max(f.end);
            }
            _ => out.push(f),
        }
    }
    out
}

/// 容器级 reveal（024 P2）：选区/光标与容器 content **相交或接触**
/// → 该容器全部定界 folds 显现（`**粗体**` 整对标记同进退，不会
/// 左显右不显）；merge 段与任一命中容器的 folds 相交即丢。空选区
/// （光标）落在段内/两端停靠位同样触发（编辑进入态）；选区强制
/// reveal：选中的文本必须可见原文（Obsidian 行为）。
pub fn reveal_containers(
    containers: &[super::parse::Container],
    merged: &[Range<usize>],
    sel: Range<usize>,
) -> Vec<Range<usize>> {
    let touched: Vec<&super::parse::Container> = containers
        .iter()
        .filter(|c| c.content.end >= sel.start && c.content.start <= sel.end)
        .collect();
    merged
        .iter()
        .filter(|m| {
            !touched
                .iter()
                .any(|c| c.folds.iter().any(|f| f.start < m.end && m.start < f.end))
        })
        .cloned()
        .collect()
}

/// doc 偏移 → 折叠后偏移（委托 vendor 单点实现；k34j 后生产路径统一走
/// `doc_to_vis_u`，本函数保留给零宽语义性质测试）。
#[allow(dead_code)]
pub fn doc_to_vis(folds: &[Range<usize>], off: usize) -> usize {
    Decorations::map_doc_to_vis(folds, off)
}

/// doc 偏移 → 折叠后偏移（k34j 统一版：行内零宽段 + 章节占位段）。
/// 委托 vendor 单点实现（与 fold 链 Transform 数学同源）。
pub fn doc_to_vis_u(folds: &[Range<usize>], sections: &[SectionFold], off: usize) -> usize {
    Decorations::map_doc_to_vis_with(folds, sections, off)
}

/// 章节折叠占位文本（与 vendor FoldPlaceholder 默认省略号同字符）。
pub const SECTION_PLACEHOLDER: &str = "⋯";

/// 章节折叠区间（k34j，仅此一项折叠功能）：1..=5 级标题的章节段——
/// 标题行尾（crease 起点，`fold_starting_at_row` 据此命中箭头态）→ 章节
/// 最后一个非空行行尾（**不含**其换行——占位符后保留行断，对齐 zed
/// crease 语义；标题与下一标题之间的纯空行段无可折内容不产）。闭区间端
/// 取"下一同级/更高级标题（级别 ≤ 本级）前"，围栏感知由 scan_headings
/// 的级别表保证（fence 内标题不入表）；`######` 起不折叠（用户定夺）。
/// 无后续同级/更高级标题 → 文档尾。
pub fn section_ranges(src: &str, headings: &[HeadingLine]) -> Vec<SectionFold> {
    let b = src.as_bytes();
    // 行起点表（'\n' 位置 +1；首行恒 0）——line_end 据此推
    let mut line_starts = Vec::with_capacity(headings.len() + 1);
    let mut pushed = 0usize;
    for (ix, &c) in b.iter().enumerate() {
        if c == b'\n' {
            if pushed == 0 {
                line_starts.push(0);
            }
            line_starts.push(ix + 1);
            pushed += 1;
        }
    }
    if pushed == 0 {
        line_starts.push(0);
    }
    let ends_nl = src.ends_with('\n');
    let rows = if src.is_empty() {
        0
    } else {
        line_starts.len() - if ends_nl { 1 } else { 0 }
    };
    // 行内容终点（排他，不含换行；空尾行 = 行起点）
    let line_end = |row: usize| -> usize {
        match line_starts.get(row + 1) {
            Some(&n) if n > 0 && b[n - 1] == b'\n' => n - 1,
            Some(&n) => n,
            None => src.len(),
        }
    };
    let line_start = |row: usize| -> usize { line_starts.get(row).copied().unwrap_or(src.len()) };
    let is_blank = |row: usize| -> bool {
        let (s, e) = (line_start(row), line_end(row));
        s >= e
            || b[s..e]
                .iter()
                .all(|&c| c == b' ' || c == b'\t' || c == b'\r')
    };

    let mut out = Vec::new();
    for (ix, h) in headings.iter().enumerate() {
        if h.level > 5 {
            continue; // ###### 起不折叠
        }
        let start = line_end(h.row);
        // 闭合标题 = 之后首个级别 ≤ 本级的标题（表内已滤 fence）
        let closing = headings[ix + 1..].iter().find(|x| x.level <= h.level);
        // 章节内容最后一行：闭合标题前一行起向前找首个非空行
        let scan_from = closing.map_or(rows.saturating_sub(1), |c| {
            c.row.min(rows).saturating_sub(1)
        });
        let mut last_content = None;
        let mut r = scan_from;
        while r > h.row {
            if !is_blank(r) {
                last_content = Some(r);
                break;
            }
            r -= 1;
        }
        let end = last_content.map_or(start, |r| line_end(r));
        if end > start {
            out.push(SectionFold {
                range: start..end,
                row: h.row as u32,
                collapsed_text: SECTION_PLACEHOLDER.into(),
            });
        }
    }
    out
}

/// 折叠后显示文本（k34j 统一版）：行内段删除、章节段替换为占位文本——
/// 与 fold 链 Transform 空间严格一致（display 仅余横向最长行宽用途，
/// 行结构须与链行号对齐）。输入段升序互不重叠（provider prune 保证）。
pub fn display_text(src: &str, folds: &[Range<usize>], sections: &[SectionFold]) -> String {
    let mut edits: Vec<(usize, usize, &str)> = folds
        .iter()
        .map(|f| (f.start, f.end, ""))
        .chain(
            sections
                .iter()
                .map(|s| (s.range.start, s.range.end, s.collapsed_text.as_ref())),
        )
        .collect();
    edits.sort_by_key(|(s, e, _)| (*s, *e));
    let mut out = src.to_string();
    for (s, e, text) in edits.into_iter().rev() {
        out.replace_range(s..e, text);
    }
    out
}

/// 折叠后偏移 → doc 偏移。
/// （P2 点击反算入口；当前仅测试使用）
#[allow(dead_code)]
pub fn vis_to_doc(folds: &[Range<usize>], vis: usize) -> usize {
    Decorations::map_vis_to_doc(folds, vis)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_component::input::{Decorations, Rope};

    fn ranges(v: &[(usize, usize)]) -> Vec<Range<usize>> {
        v.iter().map(|&(a, b)| a..b).collect()
    }

    fn headings(src: &str) -> Vec<HeadingLine> {
        super::super::parse::parse(src).3
    }

    fn sections(src: &str) -> Vec<SectionFold> {
        section_ranges(src, &headings(src))
    }

    // ---------- 章节折叠区间（k34j） ----------

    #[test]
    fn section_basic_range_excludes_last_newline() {
        // 区间 = 标题行尾 → 章节最后非空行行尾（不含换行）——占位符后
        // 保留行断（对齐 zed crease），下一标题不黏到占位符行
        let src = "## A\npara\n## B\nz";
        let s = sections(src);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].row, 0);
        assert_eq!(s[0].range, 4..9, "\"\\npara\"");
        assert_eq!(s[1].row, 2);
        assert_eq!(s[1].range, 14..16, "\"\\nz\"");
        assert_eq!(s[0].collapsed_text.as_str(), SECTION_PLACEHOLDER);
    }

    #[test]
    fn section_levels_cap_at_five() {
        // 1..=5 可折；###### 起不再折叠（用户定夺）——自身不产段，
        // 也不截断上级章节区间
        let src = "# h1\n## h2\n### h3\n#### h4\n##### h5\n###### h6\ntext\n";
        let s = sections(src);
        let rows: Vec<u32> = s.iter().map(|x| x.row).collect();
        assert_eq!(rows, vec![0, 1, 2, 3, 4], "###### 行不折叠");
        // h5 章节（含 ###### 行与 text）到文档尾
        let last = s.last().unwrap();
        assert_eq!(
            last.range,
            34..49,
            "\"\\ntext\"（h5 到文档尾，###### 不闭合上级）"
        );
    }

    #[test]
    fn section_nested_and_higher_close() {
        // ### 被同级/更高级（##）闭合；### 章节嵌在 ## 章节内
        let src = "## A\n### B\ntext\n## C\ntail\n";
        let s = sections(src);
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].range, 4..15); // A 吞 B
        assert_eq!(s[1].range, 10..15); // B ⊂ A
        assert_eq!(s[2].range, 20..25); // C 到尾（"\\ntail"）
    }

    #[test]
    fn section_adjacent_and_blank_only_skip() {
        // 相邻标题（只有换行可折）与纯空行段无可折内容 → 不产
        let src = "## A\n## B\n\n\n## C\n";
        let s = sections(src);
        assert!(s.is_empty(), "A/B/C 皆无实质内容：{s:?}");
    }

    #[test]
    fn section_trailing_blanks_stay_visible() {
        // 章节尾随空行保留可见（zed last_non_blank_row 语义）
        let src = "## A\npara\n\n\n## B\n";
        let s = sections(src);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].range, 4..9, "\"\\npara\"，空行段不吞");
    }

    #[test]
    fn section_fence_aware() {
        // fence 内 ## 不入级别表 → 不截断章节也不自行折叠
        let src = "## A\n```md\n## not\ntext\n```\n## B\n";
        let s = sections(src);
        assert_eq!(s.len(), 1, "{s:?}");
        assert_eq!(s[0].row, 0);
        assert_eq!(s[0].range, 4..26, "吞下整个 fence（含闭合行）");
    }

    #[test]
    fn section_eof_no_trailing_newline() {
        let src = "## A\npara";
        let s = sections(src);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].range, 4..9);
    }

    #[test]
    fn section_empty_doc_and_no_headings() {
        assert!(sections("").is_empty());
        assert!(sections("正文\n无标题\n").is_empty());
    }

    // ---------- 统一映射（行内 + 章节，k34j） ----------

    fn sec(start: usize, end: usize) -> SectionFold {
        SectionFold {
            range: start..end,
            row: 0,
            collapsed_text: SECTION_PLACEHOLDER.into(),
        }
    }

    #[test]
    fn unified_mapping_basic() {
        // "## A" + fold("\npara") + "\n## B"：章节占位 3 字节
        let folds = ranges(&[]);
        let sections = vec![sec(4, 9)];
        // 段前不动
        assert_eq!(doc_to_vis_u(&folds, &sections, 0), 0);
        assert_eq!(doc_to_vis_u(&folds, &sections, 4), 4);
        // 段内收拢到占位符末
        assert_eq!(doc_to_vis_u(&folds, &sections, 5), 7);
        assert_eq!(doc_to_vis_u(&folds, &sections, 9), 7);
        // 段后 = 4 + 3 = 7 基准
        assert_eq!(doc_to_vis_u(&folds, &sections, 10), 8);
        // 逆像：占位符首 ↔ 段首（往返恒等）；占位符内 → 段后
        assert_eq!(Decorations::map_vis_to_doc_with(&folds, &sections, 4), 4);
        assert_eq!(Decorations::map_vis_to_doc_with(&folds, &sections, 6), 9);
        assert_eq!(Decorations::map_vis_to_doc_with(&folds, &sections, 7), 9);
        assert_eq!(Decorations::map_vis_to_doc_with(&folds, &sections, 8), 10);
    }

    #[test]
    fn unified_mapping_roundtrip_monotonic() {
        // 行内段与章节段交错：往返恒等（非折叠段内偏移）+ 双向单调
        let folds = ranges(&[(2, 3), (20, 22)]);
        let sections = vec![sec(6, 12), sec(30, 40)];
        let total = 50usize;
        for off in 0..total {
            let vis = doc_to_vis_u(&folds, &sections, off);
            let collapses = folds.iter().any(|f| f.start <= off && off < f.end)
                || sections
                    .iter()
                    .any(|s| s.range.start <= off && off < s.range.end);
            if !collapses {
                assert_eq!(
                    Decorations::map_vis_to_doc_with(&folds, &sections, vis),
                    off,
                    "folds={folds:?} sections={sections:?} off={off}"
                );
            }
        }
        let mut prev = 0usize;
        for off in 0..total {
            let vis = doc_to_vis_u(&folds, &sections, off);
            assert!(vis >= prev, "doc_to_vis_u 非单调 off={off}");
            prev = vis;
        }
        let mut prev = 0usize;
        for vis in 0..40 {
            let doc = Decorations::map_vis_to_doc_with(&folds, &sections, vis);
            assert!(doc >= prev, "vis_to_doc 非单调 vis={vis}");
            prev = doc;
        }
    }

    #[test]
    fn unified_zero_width_semantics_unchanged() {
        // 无章节段时统一版与零宽版逐点一致（既有语义不被 k34j 破坏）
        let folds = ranges(&[(6, 8), (20, 22)]);
        for off in 0..30 {
            assert_eq!(doc_to_vis(&folds, off), doc_to_vis_u(&folds, &[], off));
            assert_eq!(
                Decorations::map_vis_to_doc(&folds, off),
                Decorations::map_vis_to_doc_with(&folds, &[], off)
            );
        }
    }

    #[test]
    fn display_text_replaces_sections_and_inline() {
        let src = "## A\npara\n## B\n";
        let secs = sections(src);
        let out = display_text(src, &[], &secs);
        assert_eq!(out, "## A⋯\n## B\n", "章节段 → 占位符，行断保留");
        let out2 = display_text("a**b**c", &ranges(&[(1, 3), (4, 6)]), &[]);
        assert_eq!(out2, "abc", "行内段删除不变");
    }

    #[test]
    fn merge_sorts_and_merges_overlap() {
        let merged = merge(ranges(&[(10, 15), (0, 3), (12, 20), (5, 7), (4, 6)]));
        assert_eq!(merged, ranges(&[(0, 3), (4, 7), (10, 20)]));
    }

    #[test]
    fn merge_drops_empty_and_adjacent_merges() {
        // 相邻（end == next.start）合并为一段
        let merged = merge(ranges(&[(0, 2), (2, 4), (9, 9)]));
        assert_eq!(merged, ranges(&[(0, 4)]));
    }

    #[test]
    fn reveal_containers_drop_whole_container() {
        use super::super::parse::Container;
        // `**粗体**`：左标记 0..2、内容 2..6、右标记 6..8 —— 一个容器
        let containers = vec![Container {
            content: 2..6,
            folds: vec![0..2, 6..8],
        }];
        let merged = ranges(&[(0, 2), (6, 8), (20, 22)]);
        // 光标落在内容/停靠位 → 整容器（两枚标记）同显
        for sel in [3..3, 2..2, 6..6, 1..7] {
            let kept = reveal_containers(&containers, &merged, sel.clone());
            assert_eq!(kept, ranges(&[(20, 22)]), "sel {sel:?} 应整容器显现");
        }
        // 光标在段外 → 原样折叠
        assert_eq!(reveal_containers(&containers, &merged, 12..12), merged);
    }

    #[test]
    fn doc_to_vis_basic() {
        let folds = ranges(&[(6, 8), (20, 22)]); // 两段各 2 字节
        assert_eq!(doc_to_vis(&folds, 0), 0);
        assert_eq!(doc_to_vis(&folds, 5), 5);
        assert_eq!(doc_to_vis(&folds, 6), 6); // 段首不动
        assert_eq!(doc_to_vis(&folds, 7), 6); // 段内收拢到段首
        assert_eq!(doc_to_vis(&folds, 8), 6); // 段尾收拢
        assert_eq!(doc_to_vis(&folds, 9), 7); // 段后前移 2
        // fold2 前已隐藏 fold1 的 2 字节 → 段首/段尾都是 18，段后 -4
        assert_eq!(doc_to_vis(&folds, 20), 18);
        assert_eq!(doc_to_vis(&folds, 22), 18);
        assert_eq!(doc_to_vis(&folds, 24), 20);
    }

    #[test]
    fn vis_to_doc_roundtrip_on_visible_offsets() {
        let folds = ranges(&[(6, 8), (20, 22), (30, 31)]);
        let total = 40;
        for off in 0..total {
            // 段 [start, end) 收拢到段首（逆像优先段尾）→ 跳过起点侧；
            // 段尾及其后往返必须恒等
            let collapses = folds.iter().any(|f| f.start <= off && off < f.end);
            if collapses {
                continue;
            }
            let vis = doc_to_vis(&folds, off);
            assert_eq!(vis_to_doc(&folds, vis), off, "off={off} vis={vis}");
        }
    }

    #[test]
    fn mapping_monotonic() {
        let folds = ranges(&[(2, 5), (9, 13)]);
        let mut prev = 0usize;
        for off in 0..50 {
            let vis = doc_to_vis(&folds, off);
            assert!(vis >= prev, "doc_to_vis 非单调: off={off}");
            prev = vis;
        }
        let mut prev = 0usize;
        for vis in 0..50 {
            let doc = vis_to_doc(&folds, vis);
            assert!(doc >= prev, "vis_to_doc 非单调: vis={vis}");
            prev = doc;
        }
    }

    #[test]
    fn next_atomic_skips_hidden_run() {
        let dec = Decorations::new(
            Rope::from("x"),
            vec![],
            ranges(&[(6, 8), (10, 12)]),
            vec![],
            vec![],
            vec![],
        );
        // 右移：停在段后
        assert_eq!(dec.next_atomic(6, 1), 8);
        // 左移：停在段前
        assert_eq!(dec.next_atomic(8, -1), 6);
        // 不紧邻 → 原样
        assert_eq!(dec.next_atomic(0, 1), 0);
        assert_eq!(dec.next_atomic(20, -1), 20);
        // 连续两段右跳
        assert_eq!(dec.next_atomic(6, 1), 8);
    }

    /// 随机折叠集下映射性质（固定种子，确定性可复现）。
    #[test]
    fn property_roundtrip_random_folds() {
        // 简易 LCG，避免引 rand 依赖
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as usize
        };
        for _case in 0..200 {
            // 生成 0..64 内不重叠折叠段
            let mut folds = Vec::new();
            let mut cursor = 0usize;
            while cursor < 64 {
                let gap = next() % 4;
                cursor += gap;
                let len = next() % 4;
                if len == 0 || cursor + len > 64 {
                    cursor += 1.max(len);
                    continue;
                }
                folds.push(cursor..cursor + len);
                cursor += len + 1;
            }
            let folds = merge(folds);
            for off in 0..64 {
                // 段 [start, end) 收拢 → 逆像不唯一，跳过；其余恒等
                let collapses = folds.iter().any(|f| f.start <= off && off < f.end);
                if collapses {
                    continue;
                }
                let vis = doc_to_vis(&folds, off);
                assert_eq!(vis_to_doc(&folds, vis), off, "folds={folds:?} off={off}");
            }
            // 单调性
            let mut prev = 0usize;
            for off in 0..64 {
                let vis = doc_to_vis(&folds, off);
                assert!(vis >= prev);
                prev = vis;
            }
        }
    }
}
