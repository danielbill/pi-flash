use super::*;

fn q(src: &str, replacement: &str) -> SearchQuery {
    build_query(src, SearchOpts::default(), Vec::new(), Vec::new())
        .unwrap()
        .with_replacement(replacement.to_string())
}

#[test]
fn replace_all_per_line() {
    let query = q("foo", "bar");
    let (out, n) = replace_in_text(&query, "foo a\nb foo\n", false);
    assert_eq!(out, "bar a\nb bar\n");
    assert_eq!(n, 2);
}

#[test]
fn replace_preserves_crlf() {
    let query = q("foo", "bar");
    let (out, n) = replace_in_text(&query, "foo\r\nnext\r\n", false);
    assert_eq!(out, "bar\r\nnext\r\n");
    assert_eq!(n, 1);
}

#[test]
fn replace_next_only_touches_first() {
    let query = q("foo", "bar");
    let (out, n) = replace_in_text(&query, "foo\nfoo\n", true);
    assert_eq!(out, "bar\nfoo\n");
    assert_eq!(n, 1);
}

#[test]
fn path_only_row_is_dropped_once_content_hits() {
    let mut blocks = vec![
        ResultBlock {
            rel: "a/dup.md".into(),
            path_only: true,
            count: 0,
            segments: Vec::new(),
        },
        ResultBlock {
            rel: "b/only_name.md".into(),
            path_only: true,
            count: 0,
            segments: Vec::new(),
        },
    ];
    // 内容命中到达 a/dup.md → 摘掉它的文件栏，只留内容组
    assert!(drop_path_only_for(&mut blocks, "a/dup.md"));
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].rel, "b/only_name.md", "仅文件名命中的文件栏保留");
    // 再摘一次：没有可摘的，计数与内容不变
    assert!(!drop_path_only_for(&mut blocks, "a/dup.md"));
    assert_eq!(blocks.len(), 1);
}

#[test]
fn globs_split_on_comma_and_newline() {
    assert_eq!(
        split_globs("a,b\n c ,\n".to_string()),
        vec!["a".to_string(), "b".to_string(), "c".to_string()]
    );
}
