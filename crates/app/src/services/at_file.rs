//! @ 文件检索的纯逻辑（031 输入面板；pi-web `lib/file-fuzzy.ts` 逐条移植）。
//!
//! 行为对齐 pi TUI：@ 触发于行首/空白之后（`foo@bar` 不触发），候选用 TUI
//! scoreEntry 阶梯打分，确认后插入 `@path `。文件/目录形态见
//! [`build_at_insert_text`]。

/// 一次 @ token 提取：`start` = `@` 的字节下标；`query` = `@` 之后、光标
/// 之前的查询词（引号形态已剥引号）；`quoted` = `@"..."` 引号形态。
#[derive(Debug, Clone, PartialEq)]
pub struct AtQueryMatch {
    pub start: usize,
    pub query: String,
    pub quoted: bool,
}

/// 检测光标前的 @ token（pi-web extractAtQuery 的正则语义：左最匹配）。
/// @ 必须在文本开头或前面是空白字符（`foo@bar` 不触发），@ 到末尾之间无
/// 空白；引号形态 `@"查询` 允许查询词含空格，但内部出现 `"` 或换行即失
/// 效（闭合引号 = 菜单收起）。返回 None = 无激活 token。
pub fn extract_at_query(before_cursor: &str) -> Option<AtQueryMatch> {
    let bytes = before_cursor.as_bytes();
    // 正则按左最匹配：同形 token 多个候选时取最左。引号形态整体优先于
    // 普通形态（pi-web 先试 quoted 正则）。
    let boundary = |i: usize| -> bool {
        i == 0 || before_cursor[..i].chars().next_back().is_some_and(|c| c.is_whitespace())
    };
    for i in 0..bytes.len() {
        if bytes[i] != b'@' || !boundary(i) || bytes.get(i + 1) != Some(&b'"') {
            continue;
        }
        if bytes[i + 2..].iter().all(|b| *b != b'"' && *b != b'\n') {
            return Some(AtQueryMatch {
                start: i,
                query: before_cursor[i + 2..].to_string(),
                quoted: true,
            });
        }
    }
    for i in 0..bytes.len() {
        if bytes[i] != b'@' || !boundary(i) {
            continue;
        }
        if bytes[i + 1..]
            .iter()
            .all(|b| *b != b'"' && !(*b as char).is_whitespace())
        {
            return Some(AtQueryMatch {
                start: i,
                query: before_cursor[i + 1..].to_string(),
                quoted: false,
            });
        }
    }
    None
}

fn path_depth(p: &str) -> usize {
    p.bytes().filter(|b| *b == b'/').count()
}

/// 从扁平文件列表构建条目（文件 + 派生目录），浅层优先、同层字母序——
/// 空 @ 查询展示的默认顺序（pi-web buildEntriesFromFiles）。
pub fn build_entries_from_files(files: &[String]) -> Vec<FileEntry> {
    let mut dirs: Vec<String> = Vec::new();
    for f in files {
        let bytes = f.as_bytes();
        let mut i = 0;
        while let Some(rel) = bytes[i..].iter().position(|b| *b == b'/') {
            i += rel + 1;
            let dir = &f[..i - 1];
            if !dirs.iter().any(|d| d == dir) {
                dirs.push(dir.to_string());
            }
        }
    }
    let mut entries: Vec<FileEntry> = dirs
        .into_iter()
        .map(|path| FileEntry { path, is_dir: true })
        .chain(files.iter().filter(|f| !f.is_empty()).map(|f| FileEntry {
            path: f.clone(),
            is_dir: false,
        }))
        .collect();
    entries.sort_by(|a, b| path_depth(&a.path).cmp(&path_depth(&b.path)).then(a.path.cmp(&b.path)));
    entries
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileEntry {
    /// 相对 cwd 的 `/` 分隔路径，无尾随斜杠
    pub path: String,
    pub is_dir: bool,
}

fn is_subsequence(needle: &str, haystack: &str) -> bool {
    let mut hay = haystack.chars();
    needle.chars().all(|n| hay.any(|h| h == n))
}

/// TUI scoreEntry 阶梯（精确 100 / 前缀 80 / basename 子串 50 / 全路径子串
/// 30 / 子序列 10，目录 +10）。查询含 `/` 时对全相对路径打分——这是目录
/// 钻取的机制：`@src/` 的查询 `src/` 前缀匹配 src 下全部条目，而 src 目录
/// 自身（`src`）不以 `src/` 开头被排除（pi-web scoreEntry）。
fn score_entry(entry: &FileEntry, lower_query: &str) -> i32 {
    let lower_path = entry.path.to_lowercase();
    let mut score = 0;
    if lower_query.contains('/') {
        if lower_path == lower_query {
            score = 100;
        } else if lower_path.starts_with(lower_query) {
            score = 80;
        } else if lower_path.contains(lower_query) {
            score = 50;
        } else if is_subsequence(lower_query, &lower_path) {
            score = 10;
        }
    } else {
        let lower_name = match lower_path.rfind('/') {
            Some(ix) => &lower_path[ix + 1..],
            None => lower_path.as_str(),
        };
        if lower_name == lower_query {
            score = 100;
        } else if lower_name.starts_with(lower_query) {
            score = 80;
        } else if lower_name.contains(lower_query) {
            score = 50;
        } else if lower_path.contains(lower_query) {
            score = 30;
        } else if is_subsequence(lower_query, &lower_path) {
            score = 10;
        }
    }
    if entry.is_dir && score > 0 {
        score += 10;
    }
    score
}

pub const AT_RESULT_LIMIT: usize = 20;

/// 打分过滤 + 排序（分数 → 深度浅优先 → 字母序），截前 limit 条
/// （pi-web filterFileEntries）。
pub fn filter_file_entries(entries: &[FileEntry], query: &str) -> Vec<FileEntry> {
    filter_file_entries_limit(entries, query, AT_RESULT_LIMIT)
}

pub fn filter_file_entries_limit(entries: &[FileEntry], query: &str, limit: usize) -> Vec<FileEntry> {
    let lower_query = query.to_lowercase();
    if lower_query.is_empty() {
        return entries.iter().take(limit).cloned().collect();
    }
    let mut scored: Vec<(i32, &FileEntry)> = entries
        .iter()
        .map(|e| (score_entry(e, &lower_query), e))
        .filter(|(s, _)| *s > 0)
        .collect();
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| path_depth(&a.1.path).cmp(&path_depth(&b.1.path)))
            .then_with(|| a.1.path.cmp(&b.1.path))
    });
    scored.truncate(limit);
    scored.into_iter().map(|(_, e)| e.clone()).collect()
}

/// 候选确认后的替换文本（pi-web buildAtInsertText）：
/// - 文件闭合 token：`@path `（路径含空格加引号 `@"path" `），光标在末尾；
/// - 目录不闭合：`@dir/`，菜单保持打开供钻取；引号目录插成闭合的
///   `@"dir/"`，光标在闭引号前。
/// 返回 (替换文本, 光标相对替换文本开头的字节偏移)。
pub fn build_at_insert_text(path: &str, is_dir: bool) -> (String, usize) {
    let p = if is_dir { format!("{path}/") } else { path.to_string() };
    if is_dir {
        if p.contains(' ') {
            let text = format!("@\"{p}\"");
            (text.clone(), text.len() - 1)
        } else {
            let text = format!("@{p}");
            (text.clone(), text.len())
        }
    } else if p.contains(' ') {
        (format!("@\"{p}\" "), format!("@\"{p}\" ").len())
    } else {
        (format!("@{p} "), format!("@{p} ").len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_requires_line_start_or_whitespace() {
        // 行首 / 空白后触发
        assert_eq!(extract_at_query("@").unwrap().query, "");
        assert_eq!(extract_at_query("看 @src").unwrap().query, "src");
        assert_eq!(extract_at_query("a\n@read").unwrap().query, "read");
        // 邮箱不触发
        assert_eq!(extract_at_query("foo@bar"), None);
        // 查询中断（空白）不触发
        assert_eq!(extract_at_query("@src main"), None);
    }

    #[test]
    fn quoted_form_and_closed_quote() {
        let m = extract_at_query("@\"my dir/fi").unwrap();
        assert!(m.quoted);
        assert_eq!(m.query, "my dir/fi");
        // 闭合引号 = token 失效
        assert_eq!(extract_at_query("@\"my dir\""), None);
        // 无空白前的 @" 不触发
        assert_eq!(extract_at_query("x@\"y"), None);
        // 左最匹配：@b@c 的查询是 "b@c"（pi-web 正则语义）
        let m = extract_at_query("a @b@c").unwrap();
        assert_eq!((m.start, m.query.as_str()), (2, "b@c"));
    }

    #[test]
    fn extract_reports_byte_start() {
        let m = extract_at_query("看看 @crates").unwrap();
        assert_eq!(m.start, "看看 ".len());
        assert_eq!(&"看看 @crates"[m.start..], "@crates");
    }

    fn files() -> Vec<String> {
        ["components/ChatInput.tsx", "src/app/page.tsx", "src/lib/util.ts", "README.md"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn entries_derive_dirs_and_sort_shallow_first() {
        let es = build_entries_from_files(&files());
        let dirs: Vec<&str> = es.iter().filter(|e| e.is_dir).map(|e| e.path.as_str()).collect();
        assert_eq!(dirs, vec!["components", "src", "src/app", "src/lib"]);
        // README.md（深度 0）排在 src/lib/util.ts（深度 2）前
        let pos = |p: &str| es.iter().position(|e| e.path == p).unwrap();
        assert!(pos("README.md") < pos("src/app/page.tsx"));
        assert!(pos("src/app/page.tsx") < pos("src/lib/util.ts"));
    }

    #[test]
    fn score_ladder_matches_tui() {
        let es = build_entries_from_files(&files());
        // 子序列 fallback："chinp" 命中 components/ChatInput.tsx
        let hits = filter_file_entries(&es, "chinp");
        assert_eq!(hits[0].path, "components/ChatInput.tsx");
        // basename 精确 > 全路径子串
        let hits = filter_file_entries(&es, "page");
        assert_eq!(hits[0].path, "src/app/page.tsx");
        // 目录钻取：`src/` 前缀命中 src 下条目，src 目录自身被排除
        let hits = filter_file_entries(&es, "src/");
        assert!(hits.iter().all(|e| e.path.starts_with("src/")));
        assert!(!hits.iter().any(|e| e.path == "src"));
        // 目录 +10：@src 与 @src/ 前缀同级时目录在前
        let hits = filter_file_entries(&es, "src");
        assert_eq!(hits[0].path, "src");
        assert!(hits[0].is_dir);
    }

    #[test]
    fn insert_text_forms() {
        // 文件闭合 + 尾空格
        assert_eq!(build_at_insert_text("src/a.ts", false), ("@src/a.ts ".into(), 10));
        // 含空格加引号
        assert_eq!(build_at_insert_text("my file.ts", false), ("@\"my file.ts\" ".into(), 14));
        // 目录不闭合（钻取继续）
        assert_eq!(build_at_insert_text("src", true), ("@src/".into(), 5));
        // 引号目录闭合、光标在闭引号前
        let (text, cur) = build_at_insert_text("my dir", true);
        assert_eq!(text, "@\"my dir/\"");
        assert_eq!(cur, text.len() - 1);
    }
}
