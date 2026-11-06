use super::*;

fn empty_matchers() -> (PathMatcher, PathMatcher) {
    (
        PathMatcher::new(std::iter::empty::<&str>(), true).unwrap(),
        PathMatcher::new(std::iter::empty::<&str>(), true).unwrap(),
    )
}

fn text(q: &str, whole_word: bool, case_sensitive: bool) -> SearchQuery {
    let (inc, exc) = empty_matchers();
    SearchQuery::text(q, whole_word, case_sensitive, false, inc, exc, false).unwrap()
}

fn regex(q: &str, whole_word: bool, case_sensitive: bool) -> SearchQuery {
    let (inc, exc) = empty_matchers();
    SearchQuery::regex(q, whole_word, case_sensitive, false, inc, exc, false).unwrap()
}

#[test]
fn text_query_is_literal() {
    let q = text("fn(", false, true);
    assert_eq!(q.search_str("fn(a: u8)"), vec![0..3]);
    assert!(q.search_str("other").is_empty());
}

#[test]
fn ascii_case_insensitive_uses_aho_corasick() {
    let q = text("Foo", false, false);
    assert!(matches!(q, SearchQuery::Text { .. }));
    assert_eq!(q.search_str("foo Foo"), vec![0..3, 4..7]);
    let q = text("Foo", false, true);
    assert_eq!(q.search_str("foo Foo"), vec![4..7]);
}

#[test]
fn non_ascii_case_insensitive_falls_back_to_regex() {
    // Zed 原逻辑：AhoCorasick 不支持 unicode 忽略大小写 → 回退 regex
    let q = text("Ä", false, false);
    assert!(matches!(q, SearchQuery::Regex { escaped: true, .. }));
    assert_eq!(q.search_str("ä Ä"), vec![0..2, 3..5]);
}

#[test]
fn case_sensitive_pattern_modifier() {
    // \C = 强制区分大小写
    let q = regex(r"\CFoo", false, false);
    assert!(q.case_sensitive());
    assert_eq!(q.as_str(), r"\CFoo");
    assert_eq!(q.search_str("foo Foo"), vec![4..7]);
    // \c = 强制忽略大小写
    let q = regex(r"\cFoo", false, true);
    assert!(!q.case_sensitive());
    assert_eq!(q.search_str("foo Foo"), vec![0..3, 4..7]);
}

#[test]
fn whole_word_uses_boundary_on_word_edges_only() {
    let q = text("foo", true, true);
    assert_eq!(q.search_str("a foo b"), vec![2..5]);
    assert!(q.search_str("foobar").is_empty());
    // 含空格的多词查询不能被两侧 \b 夹死
    let q = text("foo bar", true, true);
    assert_eq!(q.search_str("x foo bar y"), vec![2..9]);
}

#[test]
fn regex_multiline_and_crlf() {
    let q = regex(r"^hit$", false, true);
    // crlf(true)：`$` 在 \r 之前也成立
    assert_eq!(q.search_str("hit"), vec![0..3]);
    assert!(q.search_str("x hit").is_empty());
}

#[test]
fn match_path_walks_up_and_exclude_wins() {
    let (_, exc) = empty_matchers();
    let inc = PathMatcher::new(["src"], true).unwrap();
    let q = SearchQuery::text("x", false, true, false, inc, exc, false).unwrap();
    assert!(q.match_path(Path::new("src/a.rs")));
    assert!(q.match_path(Path::new("src/deep/b.rs")));
    assert!(!q.match_path(Path::new("docs/x.md")));

    let inc = PathMatcher::new(std::iter::empty::<&str>(), true).unwrap();
    let exc = PathMatcher::new(["src/vendor"], true).unwrap();
    let q = SearchQuery::text("x", false, true, false, inc, exc, false).unwrap();
    assert!(q.match_path(Path::new("src/a.rs")));
    assert!(!q.match_path(Path::new("src/vendor/c.rs")));
}

/// glob 语义与 Zed `PathMatcher` **完全一致**：Zed 的 `GlobBuilder` 不设
/// `literal_separator`，所以 `*` 会跨分隔符（`src/*.rs` 命中
/// `src/deep/a.rs`）。这不是 bug，是上游行为——PF 抄它就保持一样。
#[test]
fn glob_matches_like_zed_path_matcher() {
    let (_, exc) = empty_matchers();
    let inc = PathMatcher::new(["src/*.rs"], true).unwrap();
    let q = SearchQuery::text("x", false, true, false, inc, exc, false).unwrap();
    assert!(q.match_path(Path::new("src/a.rs")));
    assert!(
        q.match_path(Path::new("src/deep/a.rs")),
        "`*` 跨分隔符（Zed 同）"
    );

    let (_, exc) = empty_matchers();
    let inc = PathMatcher::new(["src/**/*.rs"], true).unwrap();
    let q = SearchQuery::text("x", false, true, false, inc, exc, false).unwrap();
    assert!(q.match_path(Path::new("src/deep/a.rs")));
    assert!(!q.match_path(Path::new("docs/a.rs")));
}

#[test]
fn replacement_expands_captures() {
    let q = regex(r"(\w+)@(\w+)", false, true).with_replacement("$2:$1".into());
    assert_eq!(q.replacement_for("a@b", 0..3).as_deref(), Some("b:a"));
    // escaped 查询直接字面量替换
    let q = text("a@b", false, true).with_replacement("X".into());
    assert_eq!(q.replacement_for("a@b", 0..3).as_deref(), Some("X"));
}
