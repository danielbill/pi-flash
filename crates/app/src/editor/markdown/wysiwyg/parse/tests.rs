//! wysiwyg parse 内嵌测试外移。

use super::*;

/// 折叠集渲染：应用 folds 后的文本（模拟隐藏语法）。
fn folded(src: &str) -> String {
    let (_, cs, _) = parse(src);
    let mut folds = flat_folds(&cs);
    folds.sort_by_key(|f| f.start);
    let mut out = src.to_string();
    for f in folds.iter().rev() {
        out.replace_range(f.clone(), "");
    }
    out
}

fn has_fold_covering(src: &str, needle: Range<usize>) -> bool {
    let (_, cs, _) = parse(src);
    // parse 产出单字符 fold（由 fold::merge 归一后再判断覆盖）
    let merged = crate::editor::markdown::wysiwyg::fold::merge(flat_folds(&cs));
    merged
        .iter()
        .any(|f| f.start <= needle.start && f.end >= needle.end)
}

#[test]
fn strong_markers_folded_text_intact() {
    let src = "hello **world** bye";
    assert_eq!(folded(src), "hello world bye");
    // 样式 span 覆盖 world
    let (spans, _, _) = parse(src);
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].range, 8..13);
    assert!(matches!(spans[0].style, Style::Bold));
}

#[test]
fn underscore_strong() {
    assert_eq!(folded("__bold__"), "bold");
}

#[test]
fn emphasis_single() {
    assert_eq!(folded("a *b* c"), "a b c");
    let (spans, _, _) = parse("a *b* c");
    assert!(matches!(spans[0].style, Style::Italic));
}

#[test]
fn nested_bold_italic_triple_marker() {
    // `***a***` → 三枚星号全折（em 拿 1、strong 拿满 2，dedupe 后并集）
    assert_eq!(folded("***a***"), "a");
    let (spans, _, _) = parse("***a***");
    assert!(matches!(spans[0].style, Style::BoldItalic));
}

#[test]
fn unclosed_strong_conservative() {
    // 未闭合 → pulldown 不产生 Strong 容器 → 不折不样式
    let (spans, cs, _) = parse("**abc");
    let folds = flat_folds(&cs);
    assert!(spans.is_empty());
    assert!(folds.is_empty());
    assert_eq!(folded("**abc"), "**abc");
}

#[test]
fn adjacent_after_closing() {
    // `**a**b` 类：strong 边界验证仍正确
    assert_eq!(folded("**a**b"), "ab");
}

#[test]
fn inline_code() {
    assert_eq!(folded("run `cargo test` now"), "run cargo test now");
    let (spans, _, _) = parse("run `cargo test` now");
    assert_eq!(spans.len(), 1);
    assert!(matches!(spans[0].style, Style::Code));
}

#[test]
fn heading_prefix_folded() {
    assert_eq!(folded("# Title"), "Title");
    assert_eq!(folded("###### Six"), "Six");
    assert_eq!(folded("###"), "");
    // 无空格的 `#tag` 不是标题
    assert_eq!(folded("#tag"), "#tag");
    // 7 个 # 不是标题
    assert_eq!(folded("####### seven"), "####### seven");
}

#[test]
fn heading_inside_fence_not_folded() {
    // P1：围栏标记行整段折叠（``` 不可见），围栏内 heading 不折
    let src = "```\n# not heading\n```\n# real";
    assert_eq!(folded(src), "\n# not heading\n\nreal");
}

#[test]
fn heading_indented_code_not_folded() {
    // 4 空格缩进 = 代码块
    assert_eq!(folded("    # code"), "    # code");
}

#[test]
fn cjk_boundaries() {
    assert_eq!(folded("中文**粗体**文字"), "中文粗体文字");
    assert_eq!(folded("**中文**"), "中文");
}

#[test]
fn no_fold_crosses_newline() {
    // 强调跨行：定界符各自贴着本行内容，fold 不得含 \n
    let src = "**line1\nline2**";
    let (_, cs, _) = parse(src);
    let folds = flat_folds(&cs);
    for f in &folds {
        assert!(!src[f.clone()].contains('\n'), "fold {f:?} 跨行了");
    }
    assert_eq!(folded(src), "line1\nline2");
}

#[test]
fn plain_text_untouched() {
    for src in ["", "hello", "a_b_c", "1 * 2 * 3", "snake_case_name"] {
        let (spans, cs, _) = parse(src);
        let folds = flat_folds(&cs);
        assert!(spans.is_empty(), "{src} 不该有 span");
        assert!(folds.is_empty(), "{src} 不该有 fold");
    }
}

#[test]
fn has_fold_covering_helper_works() {
    let src = "x **y** z";
    assert!(has_fold_covering(src, 2..4)); // 左 `**`
    assert!(has_fold_covering(src, 5..7)); // 右 `**`
}

/// P1：标题整行样式 + 行级字号倍数 + 前缀折叠。
#[test]
fn heading_spans_scale_and_folds() {
    let src = "# 一级\n正文\n## 二级\n";
    let (spans, _folds, scale) = parse(src);
    let heads: Vec<(Range<usize>, u8)> = spans
        .iter()
        .filter_map(|s| match s.style {
            Style::Heading(l) => Some((s.range.clone(), l)),
            _ => None,
        })
        .collect();
    assert_eq!(heads.len(), 2);
    assert_eq!(heads[0].1, 1);
    assert_eq!(heads[1].1, 2);
    assert_eq!(&src[heads[0].0.clone()], "一级");
    assert_eq!(&src[heads[1].0.clone()], "二级");
    // 行号 → 倍数（folds 不含换行，行号对齐）
    assert!(scale.contains(&(0, 1.62)), "{scale:?}");
    assert!(scale.contains(&(2, 1.42)), "{scale:?}");
    // 前缀折叠覆盖 `# ` 与 `## `
    assert!(has_fold_covering(src, 0..2));
    let h2 = src.find("## ").unwrap();
    assert!(has_fold_covering(src, h2..h2 + 3));
}

/// P1：链接折叠 `[` 与 `](url)`，文本保留 + Link 样式。
#[test]
fn link_folds_brackets_and_url() {
    let src = "见 [pi-web](https://x.com) 主页";
    let (spans, cs, _) = parse(src);
    let folds = flat_folds(&cs);
    let merged = crate::editor::markdown::wysiwyg::fold::merge(folds);
    let lb = src.find('[').unwrap();
    let rb = src.find(']').unwrap();
    let end = src.find(')').unwrap() + 1;
    let mut out = src.to_string();
    for f in merged.iter().rev() {
        out.replace_range(f.clone(), "");
    }
    assert_eq!(out, "见 pi-web 主页", "折叠后应只剩可见文本");
    assert!(merged.iter().any(|f| f.start == lb && f.end == lb + 1));
    assert!(merged.iter().any(|f| f.start == rb && f.end == end));
    assert!(
        spans
            .iter()
            .any(|s| matches!(s.style, Style::Link) && &src[s.range.clone()] == "pi-web")
    );
}

/// P1：围栏标记行整段折叠（含语言名），代码内容保留。
#[test]
fn fence_marker_lines_folded() {
    let src = "```bash\nnpm i\n```\n";
    let (_, cs, _) = parse(src);
    let folds = flat_folds(&cs);
    let merged = crate::editor::markdown::wysiwyg::fold::merge(folds);
    let mut out = src.to_string();
    for f in merged.iter().rev() {
        out.replace_range(f.clone(), "");
    }
    assert_eq!(out, "\nnpm i\n\n");
}
