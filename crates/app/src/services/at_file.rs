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
/// [`extract_at_query`] 的触发符参数化版（031-2 统一输入菜单）：`&` 会话
/// token 与 `@` 同构（同一套行首/空白边界与引号语法），`trigger` 传对应
/// ASCII 字节即可。
pub fn extract_trigger_query(before_cursor: &str, trigger: u8) -> Option<AtQueryMatch> {
    let bytes = before_cursor.as_bytes();
    // 正则按左最匹配：同形 token 多个候选时取最左。引号形态整体优先于
    // 普通形态（pi-web 先试 quoted 正则）。
    let boundary = |i: usize| -> bool {
        i == 0
            || before_cursor[..i]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_whitespace())
    };
    for i in 0..bytes.len() {
        if bytes[i] != trigger || !boundary(i) || bytes.get(i + 1) != Some(&b'"') {
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
        if bytes[i] != trigger || !boundary(i) {
            continue;
        }
        // token 体按**字符**判空白/引号（pi-web 正则 [^\s"] 的语义）：字节
        // 直转 char 会把 UTF-8 续字节当 Latin-1 判——0x85→U+0085(NEL)、
        // 0xA0→U+00A0(NBSP) 都是空白，「内存」(E5 86 85)这类字会掐断
        // token，@ 菜单对这些目录名直接失灵（2026-11-12 用户实测）
        if before_cursor[i + 1..]
            .chars()
            .all(|c| c != '"' && !c.is_whitespace())
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

/// 从扁平文件列表构建条目（文件 + 派生目录），浅层优先、同层字母序——
/// 空 @ 查询展示的默认顺序（pi-web buildEntriesFromFiles）。目录去重走
/// HashSet：原线性 `iter().any` 在大仓库（git 硬上限 20 万条）下是 O(n²)。
/// 最终顺序完全由末尾 sort 决定，去重容器不影响结果。
pub fn build_entries_from_files(files: &[String]) -> Vec<FileEntry> {
    let mut dirs: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for f in files {
        let bytes = f.as_bytes();
        let mut i = 0;
        while let Some(rel) = bytes[i..].iter().position(|b| *b == b'/') {
            i += rel + 1;
            dirs.insert(&f[..i - 1]);
        }
    }
    let mut entries: Vec<FileEntry> = dirs
        .into_iter()
        .map(|d| FileEntry::new(d.to_string(), true))
        .chain(
            files
                .iter()
                .filter(|f| !f.is_empty())
                .map(|f| FileEntry::new(f.clone(), false)),
        )
        .collect();
    entries.sort_by(|a, b| a.depth.cmp(&b.depth).then(a.path.cmp(&b.path)));
    entries
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileEntry {
    /// 相对 cwd 的 `/` 分隔路径，无尾随斜杠
    pub path: String,
    pub is_dir: bool,
    /// 预计算小写路径：score_entry 常驻热路径（@ 菜单打开期间每次渲染
    /// 全量打分），现场 to_lowercase 是逐条目逐帧的 String 分配
    pub lower_path: String,
    /// 预计算路径深度（`/` 计数）：构建排序与结果排序的比较器都用
    pub depth: usize,
}

impl FileEntry {
    pub fn new(path: String, is_dir: bool) -> Self {
        let lower_path = path.to_lowercase();
        let depth = path.bytes().filter(|b| *b == b'/').count();
        Self {
            path,
            is_dir,
            lower_path,
            depth,
        }
    }
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
    // lower_path 构建期已算好——打分路径零分配
    let lower_path = entry.lower_path.as_str();
    let mut score = 0;
    if lower_query.contains('/') {
        if lower_path == lower_query {
            score = 100;
        } else if lower_path.starts_with(lower_query) {
            score = 80;
        } else if lower_path.contains(lower_query) {
            score = 50;
        } else if is_subsequence(lower_query, lower_path) {
            score = 10;
        }
    } else {
        let lower_name = match lower_path.rfind('/') {
            Some(ix) => &lower_path[ix + 1..],
            None => lower_path,
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

pub fn filter_file_entries_limit(
    entries: &[FileEntry],
    query: &str,
    limit: usize,
) -> Vec<FileEntry> {
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
            .then_with(|| a.1.depth.cmp(&b.1.depth))
            .then_with(|| a.1.path.cmp(&b.1.path))
    });
    scored.truncate(limit);
    scored.into_iter().map(|(_, e)| e.clone()).collect()
}

/// 万能搜索的文件名过滤（031-2）：仅 basename 子串命中（`query` 须已
/// 小写），顺序沿用构建序（浅层优先 → 字母序），截前 limit 条。
/// 全量扫描有 `menu_search_cache` 记忆化兜底（menu.rs），热路径只在
/// 输入变化时发生。
pub fn filter_file_names(entries: &[FileEntry], lower_query: &str, limit: usize) -> Vec<FileEntry> {
    if lower_query.is_empty() {
        return entries.iter().take(limit).cloned().collect();
    }
    entries
        .iter()
        .filter(|e| {
            let n = match e.lower_path.rfind('/') {
                Some(ix) => &e.lower_path[ix + 1..],
                None => e.lower_path.as_str(),
            };
            n.contains(lower_query)
        })
        .take(limit)
        .cloned()
        .collect()
}

/// 候选确认后的替换文本（pi-web buildAtInsertText）：
/// - 文件闭合 token：`@path `（路径含空格加引号 `@"path" `），光标在末尾；
/// - 目录不闭合：`@dir/`，菜单保持打开供钻取；
/// - `force_quotes`：引号态触发（当前 token 是 `@"…`）时强制引号形态——
///   无空格路径也插 `@"dir/"`，否则引号态钻取会退化成裸形态、与残留的
///   闭引号错乱（pi-web forceQuotes parity，2026-11-12 修引号钻取挂引号）。
/// 返回 (替换文本, 光标相对替换文本开头的字节偏移)。
pub fn build_at_insert_text(path: &str, is_dir: bool, force_quotes: bool) -> (String, usize) {
    let p = if is_dir {
        format!("{path}/")
    } else {
        path.to_string()
    };
    let needs_quotes = force_quotes || p.contains(' ');
    if is_dir {
        if needs_quotes {
            let text = format!("@\"{p}\"");
            (text.clone(), text.len() - 1)
        } else {
            let text = format!("@{p}");
            (text.clone(), text.len())
        }
    } else if needs_quotes {
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
        assert_eq!(extract_trigger_query("@", b'@').unwrap().query, "");
        assert_eq!(extract_trigger_query("看 @src", b'@').unwrap().query, "src");
        assert_eq!(
            extract_trigger_query("a\n@read", b'@').unwrap().query,
            "read"
        );
        // 邮箱不触发
        assert_eq!(extract_trigger_query("foo@bar", b'@'), None);
        // 查询中断（空白）不触发
        assert_eq!(extract_trigger_query("@src main", b'@'), None);
    }

    #[test]
    fn quoted_form_and_closed_quote() {
        let m = extract_trigger_query("@\"my dir/fi", b'@').unwrap();
        assert!(m.quoted);
        assert_eq!(m.query, "my dir/fi");
        // 闭合引号 = token 失效
        assert_eq!(extract_trigger_query("@\"my dir\"", b'@'), None);
        // 无空白前的 @" 不触发
        assert_eq!(extract_trigger_query("x@\"y", b'@'), None);
        // 左最匹配：@b@c 的查询是 "b@c"（pi-web 正则语义）
        let m = extract_trigger_query("a @b@c", b'@').unwrap();
        assert_eq!((m.start, m.query.as_str()), (2, "b@c"));
    }

    #[test]
    fn extract_reports_byte_start() {
        let m = extract_trigger_query("看看 @crates", b'@').unwrap();
        assert_eq!(m.start, "看看 ".len());
        assert_eq!(&"看看 @crates"[m.start..], "@crates");
    }

    #[test]
    fn amp_trigger_is_isomorphic_to_at() {
        // 031-2 统一菜单：`&` 会话 token 与 `@` 同一语法
        let m = extract_trigger_query("看 &会话名", b'&').unwrap();
        assert_eq!((m.start, m.query.as_str()), ("看 ".len(), "会话名"));
        assert_eq!(extract_trigger_query("a&&b", b'&'), None);
        // 边界同规：`foo&bar` 不触发（非行首/空白后）
        assert_eq!(extract_trigger_query("foo&bar", b'&'), None);
        // `@` 语义不受参数化影响（回归）
        assert_eq!(extract_trigger_query("foo@bar", b'@'), None);
    }

    #[test]
    fn name_filter_matches_basename_only() {
        let es = build_entries_from_files(&files());
        // 仅 basename 子串命中：派生目录 src/app 按名入选；src/lib/util.ts
        // 不因全路径含 "app" 入选
        let hits = filter_file_names(&es, "app", 20);
        assert_eq!(
            hits.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
            vec!["src/app"]
        );
        // 文件按 basename 命中（basename "page.tsx" 含 "page"）
        let hits = filter_file_names(&es, "page", 20);
        assert_eq!(
            hits.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
            vec!["src/app/page.tsx"]
        );
        // 空查询 = 构建序截断
        let hits = filter_file_names(&es, "", 2);
        assert_eq!(hits.len(), 2);
        // 大小写不敏感（query 已小写，条目侧预计算）
        let hits = filter_file_names(&es, "readme", 20);
        assert_eq!(hits[0].path, "README.md");
    }

    #[test]
    fn cjk_token_body_not_cut_by_utf8_bytes() {
        // 「内」(E5 86 85)「研」(E7 A0 94)「光」的 UTF-8 字节里有 0x85/0xA0，
        // 直转 char 是 U+0085(NEL)/U+00A0(NBSP) 空白——曾把 token 掐断，
        // @内存/个股调研/太空光伏 菜单整体失灵（2026-11-12 用户实测）
        assert_eq!(extract_trigger_query("@内存", b'@').unwrap().query, "内存");
        assert_eq!(
            extract_trigger_query("@个股调研/", b'@').unwrap().query,
            "个股调研/"
        );
        assert_eq!(
            extract_trigger_query("@专题/内存/", b'@').unwrap().query,
            "专题/内存/"
        );
        // 真·空白字符仍终止 token（NBSP 是 JS \s，语义保持）
        assert_eq!(extract_trigger_query("@a\u{a0}b", b'@'), None);
        // token 后跟空白仍不触发
        assert_eq!(extract_trigger_query("@内存 存", b'@'), None);
    }

    fn files() -> Vec<String> {
        [
            "components/ChatInput.tsx",
            "src/app/page.tsx",
            "src/lib/util.ts",
            "README.md",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    #[test]
    fn entries_derive_dirs_and_sort_shallow_first() {
        let es = build_entries_from_files(&files());
        let dirs: Vec<&str> = es
            .iter()
            .filter(|e| e.is_dir)
            .map(|e| e.path.as_str())
            .collect();
        assert_eq!(dirs, vec!["components", "src", "src/app", "src/lib"]);
        // README.md（深度 0）排在 src/lib/util.ts（深度 2）前
        let pos = |p: &str| es.iter().position(|e| e.path == p).unwrap();
        assert!(pos("README.md") < pos("src/app/page.tsx"));
        assert!(pos("src/app/page.tsx") < pos("src/lib/util.ts"));
    }

    #[test]
    fn entries_dedupe_shared_dirs_once() {
        // 多文件共享目录：每级目录只出现一次（HashSet 去重 parity）
        let files: Vec<String> = ["a/b/c.txt", "a/b/d.txt", "a/e.txt", "a/b/f/g.txt"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let es = build_entries_from_files(&files);
        let dirs: Vec<&str> = es
            .iter()
            .filter(|e| e.is_dir)
            .map(|e| e.path.as_str())
            .collect();
        assert_eq!(dirs, vec!["a", "a/b", "a/b/f"]);
        assert_eq!(es.iter().filter(|e| !e.is_dir).count(), 4);
        // 预计算字段与 path 一致
        for e in &es {
            assert_eq!(e.lower_path, e.path.to_lowercase());
            assert_eq!(e.depth, e.path.bytes().filter(|b| *b == b'/').count());
        }
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
        assert_eq!(
            build_at_insert_text("src/a.ts", false, false),
            ("@src/a.ts ".into(), 10)
        );
        // 含空格加引号
        assert_eq!(
            build_at_insert_text("my file.ts", false, false),
            ("@\"my file.ts\" ".into(), 14)
        );
        // 目录不闭合（钻取继续）
        assert_eq!(
            build_at_insert_text("src", true, false),
            ("@src/".into(), 5)
        );
        // 引号目录闭合、光标在闭引号前
        let (text, cur) = build_at_insert_text("my dir", true, false);
        assert_eq!(text, "@\"my dir/\"");
        assert_eq!(cur, text.len() - 1);
        // 引号态触发强制引号形态：无空格路径也插引号（pi-web forceQuotes
        // parity——否则裸形态与残留闭引号错乱，2026-11-12 修引号钻取挂引号）
        assert_eq!(
            build_at_insert_text("sub", true, true),
            ("@\"sub/\"".into(), 6)
        );
        assert_eq!(
            build_at_insert_text("sub/a.ts", false, true),
            ("@\"sub/a.ts\" ".into(), 12)
        );
    }
}
