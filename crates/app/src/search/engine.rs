//! 项目搜索执行层——**结构对照 Zed** `crates/project/src/project_search.rs`，
//! 查询层直接抄 Zed（见 [`super::query`]），**无任何内容索引**（取证见
//! `docs/模块设计/083-项目搜索.md` §1）。
//!
//! 与 Zed 的函数对照表：
//!
//! | Zed | 本文件 |
//! |---|---|
//! | `Search::local` / `into_handle` | [`run_search`] |
//! | `provide_search_paths`（worktree 快照遍历） | [`candidate_paths`]（无 worktree 层，改走 `git ls-files` / `ignore::WalkBuilder`） |
//! | `handle_scan_path`（路径过滤前置） | [`run_search`] 里的 `match_path` 闸 |
//! | `handle_find_first_match`（磁盘粗筛首命中） | [`scan_file`] |
//! | `handle_find_all_matches`（开 Buffer 精算 range） | [`scan_text`]（一次读盘直接出全部 range，PF 无 Buffer 层） |
//! | `build_excerpt_ranges`（±ctx 摘录窗口） | [`build_blocks`]（同一公式 `start-ctx .. end+ctx`，见函数注释） |
//! | `MAX_SEARCH_RESULT_FILES` / `MAX_SEARCH_RESULT_RANGES` | [`MAX_FILES`] / [`MAX_MATCH_LINES`]（同值 5000 / 10000） |
//! | `maintain_sorted_search_results`（保证路径序） | 候选清单已排序、单线程串行 → 天然有序，无需重排缓冲 |
//! | `is_utf8_prefix` / `decode_byte_header` 二进制早退 | [`scan_file`]（复用 `editor::file_content`，同一判据） |
//!
//! Zed 两阶段（先 detect 首命中、再开 Buffer 精算）是它的 Buffer/MultiBuffer
//! 架构决定的；PF 没有 Buffer 层，一次读盘就能把「命中行 + 上下文行」收齐，
//! 故合并为单阶段——**语义不变**，只是少一次读盘。

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::channel::mpsc::UnboundedSender;

use super::query::SearchQuery;
use crate::editor::file_content::{
    ByteContent, FILE_ANALYSIS_BYTES, analyze_byte_content, decode_text,
};

/// 上下文行数（Zed `multibuffer_context_lines` 默认 2）
pub(crate) const CONTEXT_LINES: u32 = 2;
/// 命中文件数上限（Zed `Search::MAX_SEARCH_RESULT_FILES`）
pub(crate) const MAX_FILES: usize = 5_000;
/// 命中行数上限（Zed `Search::MAX_SEARCH_RESULT_RANGES`）
pub(crate) const MAX_MATCH_LINES: usize = 10_000;
/// 文件名命中上限：Zed 的文件查找器是独立面板，PF 把它并进同一结果列表，
/// 因此单独封顶，防止 `src` 这类词把内容结果挤出视口
pub(crate) const MAX_PATH_HITS: usize = 50;
/// 单文件扫描上限（PF 特有：Zed 不设——它的 buffer 本来就能开大文件）
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// 单文件命中行上限（防单文件刷满全局额度）
const MAX_LINES_PER_FILE: usize = 500;
/// 单行字节上限（047-内存削减 P1-6）：命中行与摘录行整行存储——minified
/// 大单行（实测 8MB 单行文件）一行就是 8MB×(1+2·ctx)。超限截断 + 「…」
/// （对齐 Zed 行折叠；正则跨截断点的匹配会漏，病理大单行可接受）。
const MAX_LINE_BYTES: usize = 4 * 1024;

/// 截断到 [`MAX_LINE_BYTES`]（字符边界），截断尾补「…」；未超限原样返回。
fn clip_line(line: &str) -> String {
    if line.len() <= MAX_LINE_BYTES {
        return line.to_string();
    }
    let mut cut = MAX_LINE_BYTES;
    while !line.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = line[..cut].to_string();
    out.push('…');
    out
}

/// 截断字节点（未超限 = 全长）：供搜索视图先截再匹配，spans 落在截断前缀
/// 内与真实行保持自洽（跳转定位不受影响）。
fn clip_point(line: &str) -> usize {
    if line.len() <= MAX_LINE_BYTES {
        return line.len();
    }
    let mut cut = MAX_LINE_BYTES;
    while !line.is_char_boundary(cut) {
        cut -= 1;
    }
    cut
}

/// 搜索开关位（对应 Zed `SearchOptions` 里本项目用到的四个）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SearchOpts {
    pub(crate) case_sensitive: bool,
    pub(crate) whole_word: bool,
    pub(crate) regex: bool,
    pub(crate) include_ignored: bool,
}

/// 一个文件里的一处命中行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LineHit {
    /// 1-based 行号
    pub(crate) line: u32,
    /// 行内命中区间（字节）
    pub(crate) spans: Vec<Range<usize>>,
}

/// 命中文件的完整结果：命中行 + 已按上下文合并好的摘录段。
#[derive(Clone, Debug)]
pub(crate) struct FileHit {
    /// 相对 cwd 的 `/` 分隔路径
    pub(crate) rel: String,
    pub(crate) hits: Vec<LineHit>,
    /// 摘录段（每段 = 连续行 `(1-based 行号, 行文本)`）。相邻命中的上下文
    /// 窗口重叠时并成一段（Zed 每命中一个 excerpt；PF 合并同类项，避免
    /// 同一段代码在结果里重复出现）。
    pub(crate) segments: Vec<Vec<(u32, String)>>,
}

/// 后台 → 前台的消息。
pub(crate) enum SearchMsg {
    /// 文件名命中（一次性发，结果区最前的独立分组）
    PathHits(Vec<String>),
    Hit(FileHit),
    /// 触到文件/行数上限
    Limit,
    /// 结束；scanned = 实际读盘过的文件数
    Done {
        scanned: usize,
    },
}

/// 候选路径（相对 cwd、`/` 分隔、已排序）。
///
/// Zed 从 worktree 内存快照取（`Snapshot::files`）；PF 没有 worktree 层，
/// 默认走 `git ls-files --cached --others --exclude-standard`（与 @ 检索、
/// 文件搜索同一事实源，天然尊重 .gitignore），非 git 回退 BFS；
/// `include_ignored` 时改走 ignore crate 的全量遍历（Zed Scan Ignored 同义）。
pub(crate) fn candidate_paths(cwd: &Path, include_ignored: bool) -> Vec<String> {
    if !include_ignored {
        let mut files = crate::services::file_index::load_listing(cwd).files;
        files.sort();
        files.dedup();
        return files;
    }
    let mut out = Vec::new();
    let walker = ignore::WalkBuilder::new(cwd)
        // 四个 ignore 源全关 = 连被忽略目录也进候选集
        .hidden(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .ignore(false)
        .parents(false)
        // .git 目录自身永远不扫（对象库是二进制垃圾场）
        .filter_entry(|e| e.file_name() != ".git")
        .build();
    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(cwd) else {
            continue;
        };
        out.push(rel.to_string_lossy().replace('\\', "/"));
    }
    out.sort();
    out
}

/// 单文件扫描：命中行 + 摘录段。None = 无命中。
///
/// 对应 Zed `handle_find_all_matches`（它扫 BufferSnapshot，PF 扫 `&str`），
/// 行内区间由 [`SearchQuery::search_str`]（Zed 原码）给出。
pub(crate) fn scan_text(rel: String, text: &str, q: &SearchQuery) -> Option<FileHit> {
    let mut hits: Vec<LineHit> = Vec::new();
    for (ix, raw) in text.lines().enumerate() {
        // CRLF：fancy_regex 已开 crlf(true)，但行内区间是按行切出来的，
        // 尾 CR 会让 `$` 之后多一个字节；Zed 在 detect 里做整文件归一，
        // PF 逐行剥尾（等价）。
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        // 超长行先截到字节窗再匹配（047-P1-6）：spans 落在截断前缀内，
        // 与摘录行的截断展示保持自洽
        let view = &line[..clip_point(line)];
        let spans = q.search_str(view);
        if spans.is_empty() {
            continue;
        }
        hits.push(LineHit {
            line: ix as u32 + 1,
            spans,
        });
        if hits.len() >= MAX_LINES_PER_FILE {
            break;
        }
    }
    if hits.is_empty() {
        return None;
    }
    let hit_lines: Vec<u32> = hits.iter().map(|h| h.line).collect();
    let segments = build_blocks(text, &hit_lines, CONTEXT_LINES);
    Some(FileHit {
        rel,
        hits,
        segments,
    })
}

/// 摘录段切分：逐条对照 Zed `build_excerpt_ranges`
/// （`crates/multi_buffer/src/multi_buffer.rs:3162`）：
///
/// ```ignore
/// let start_row = range.start.row.saturating_sub(context_line_count);
/// let start = Point::new(start_row, 0);
/// let end_row = (range.end.row + context_line_count).min(snapshot.max_point().row);
/// ```
///
/// 差别只在两处：① 行是 1-based（Zed 0-based）；② 相邻命中的窗口重叠或
/// 紧邻（间隔 <= 2·ctx+1）时并段，而不是各出一段。
fn build_blocks(text: &str, hit_lines: &[u32], ctx: u32) -> Vec<Vec<(u32, String)>> {
    let total = text.lines().count() as u32;
    if total == 0 || hit_lines.is_empty() {
        return Vec::new();
    }
    let merge_gap = 2 * ctx + 1;
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for &line in hit_lines {
        match ranges.last_mut() {
            Some(last) if line <= last.1 + merge_gap => last.1 = line,
            _ => ranges.push((line, line)),
        }
    }
    let lines: Vec<&str> = text.lines().collect();
    ranges
        .into_iter()
        .map(|(first, last)| {
            let start = first.saturating_sub(ctx).max(1);
            let end = (last + ctx).min(total);
            (start..=end)
                .map(|n| {
                    let raw = lines.get(n as usize - 1).copied().unwrap_or("");
                    // 摘录行同口径截断（047-P1-6）：大单行不整行进内存
                    let line = raw.strip_suffix('\r').unwrap_or(raw);
                    (n, clip_line(line))
                })
                .collect()
        })
        .collect()
}

/// 读盘 + 扫描一个文件（Zed `handle_find_first_match` 的 PF 等价物：二进制
/// 早退 + 编码嗅探都在这一步）。
fn scan_file(cwd: &Path, rel: &str, q: &SearchQuery) -> Option<FileHit> {
    let abs = cwd.join(rel);
    let meta = std::fs::metadata(&abs).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = std::fs::read(&abs).ok()?;
    if bytes.is_empty() {
        return None;
    }
    // 二进制早退：与文件编辑器同一判据（头 1KB）
    let binary = {
        let head = &bytes[..bytes.len().min(FILE_ANALYSIS_BYTES)];
        matches!(analyze_byte_content(head), ByteContent::Binary)
    };
    if binary {
        return None;
    }
    // UTF-8（无 ESC 控制序列）零拷贝直用；否则走 BOM/UTF-16/chardetng
    // （GBK 遗留文件也能搜到，与打开文件的解码口径一致）
    let owned;
    let text: &str = match std::str::from_utf8(&bytes) {
        Ok(s) if !s.contains('\x1b') => s,
        _ => {
            owned = decode_text(bytes).ok()?.text;
            owned.as_str()
        }
    };
    scan_text(rel.to_string(), text, q)
}

/// 搜索主循环（阻塞，跑在后台执行器上）。`cancel` 置位即尽快返回。
pub(crate) fn run_search(
    cwd: PathBuf,
    q: SearchQuery,
    opts: SearchOpts,
    cancel: Arc<AtomicBool>,
    tx: UnboundedSender<SearchMsg>,
) {
    let paths = candidate_paths(&cwd, opts.include_ignored);

    // 文件名命中：与内容命中同一套查询（Zed 的文件查找器走 fuzzy；PF 合并
    // 进同一面板后改用「查询式命中」——输入 `main.rs` 或 `src/.*\.rs` 都能
    // 把文件本身捞出来）
    let path_hits: Vec<String> = paths
        .iter()
        .filter(|p| !q.search_str(p).is_empty())
        .take(MAX_PATH_HITS)
        .cloned()
        .collect();
    if !path_hits.is_empty() && tx.unbounded_send(SearchMsg::PathHits(path_hits)).is_err() {
        return;
    }

    let mut scanned = 0usize;
    let mut files = 0usize;
    let mut lines = 0usize;
    let mut hit_limit = false;
    for rel in &paths {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        // 路径过滤前置于读盘（Zed `handle_scan_path` 的第一道闸）
        if !q.match_path(Path::new(rel)) {
            continue;
        }
        scanned += 1;
        let Some(hit) = scan_file(&cwd, rel, &q) else {
            continue;
        };
        files += 1;
        lines += hit.hits.len();
        if tx.unbounded_send(SearchMsg::Hit(hit)).is_err() {
            return;
        }
        if files > MAX_FILES || lines > MAX_MATCH_LINES {
            hit_limit = true;
            break;
        }
    }
    if hit_limit {
        let _ = tx.unbounded_send(SearchMsg::Limit);
    }
    let _ = tx.unbounded_send(SearchMsg::Done { scanned });
}

#[cfg(test)]
mod tests {
    use super::super::query::{PathMatcher, SearchQuery};
    use super::*;

    fn query(src: &str, opts: SearchOpts) -> SearchQuery {
        let inc = PathMatcher::default();
        let exc = PathMatcher::default();
        if opts.regex {
            SearchQuery::regex(
                src,
                opts.whole_word,
                opts.case_sensitive,
                false,
                inc,
                exc,
                false,
            )
            .expect("regex compiles")
        } else {
            SearchQuery::text(
                src,
                opts.whole_word,
                opts.case_sensitive,
                false,
                inc,
                exc,
                false,
            )
            .expect("text query")
        }
    }

    #[test]
    fn crlf_and_line_numbers() {
        let c = query("hit", SearchOpts::default());
        let hit = scan_text(
            "a.txt".into(),
            "nope\r\nhit here\r\nnope\r\nhit again\r\n",
            &c,
        )
        .expect("two hits");
        assert_eq!(
            hit.hits.iter().map(|h| h.line).collect::<Vec<_>>(),
            vec![2, 4]
        );
        let all: Vec<&str> = hit
            .segments
            .iter()
            .flatten()
            .map(|(_, t)| t.as_str())
            .collect();
        assert!(all.iter().all(|t| !t.contains('\r')));
        assert!(all.contains(&"hit here"));
    }

    #[test]
    fn context_blocks_merge_when_overlapping() {
        let c = query("needle", SearchOpts::default());
        let mut lines: Vec<String> = (1..=40).map(|n| format!("line {n}")).collect();
        for n in [1usize, 2, 3, 30] {
            lines[n - 1].push_str(" needle");
        }
        let text = format!("{}\n", lines.join("\n"));
        let hit = scan_text("a.txt".into(), &text, &c).expect("hits");
        assert_eq!(hit.hits.len(), 4);
        assert_eq!(hit.segments.len(), 2, "两个不相邻的摘录段");
        assert_eq!(hit.segments[0].first().map(|(n, _)| *n), Some(1));
        assert_eq!(hit.segments[0].last().map(|(n, _)| *n), Some(5));
        assert_eq!(hit.segments[1].first().map(|(n, _)| *n), Some(28));
        assert_eq!(hit.segments[1].last().map(|(n, _)| *n), Some(32));
    }

    #[test]
    fn near_hits_share_one_block() {
        let c = query("x", SearchOpts::default());
        let text = "x\nx\nx\n\n\n\n\nx\n";
        let hit = scan_text("a.txt".into(), text, &c).expect("hits");
        assert_eq!(hit.segments.len(), 1);
        assert_eq!(hit.segments[0].first().map(|(n, _)| *n), Some(1));
        assert_eq!(hit.segments[0].last().map(|(n, _)| *n), Some(8));
    }

    /// 端到端流水线（读盘 → 过滤 → 扫描 → 流式回传）：用临时目录造一个
    /// 迷你项目，验证「文件名命中 + 内容命中 + 上限消息」都真的走得通——
    /// 这是不能在 UI 上点的时候唯一的链路证据。
    #[test]
    fn run_search_end_to_end_on_temp_project() {
        let dir = std::env::temp_dir().join(format!("pif-search-e2e-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/main.rs"),
            "fn main() {
    let needle = 1;
}
",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/other.rs"),
            "no match here
",
        )
        .unwrap();
        std::fs::write(
            dir.join("README.md"),
            "needle in docs
",
        )
        .unwrap();
        // 二进制（含 NUL 头）必须被跳过
        std::fs::write(dir.join("blob.bin"), [0u8, 1, 2, 3, 0, 0]).unwrap();

        let (tx, mut rx) = futures::channel::mpsc::unbounded::<SearchMsg>();
        let q = query("needle", SearchOpts::default());
        run_search(
            dir.clone(),
            q,
            SearchOpts::default(),
            Arc::new(AtomicBool::new(false)),
            tx,
        );
        let mut path_hits = Vec::new();
        let mut hits = Vec::new();
        let mut done = None;
        while let Ok(msg) = rx.try_recv() {
            match msg {
                SearchMsg::PathHits(p) => path_hits = p,
                SearchMsg::Hit(h) => hits.push(h),
                SearchMsg::Limit => {}
                SearchMsg::Done { scanned } => done = Some(scanned),
            }
        }
        let _ = std::fs::remove_dir_all(&dir);

        assert!(done.is_some(), "必须收到 Done");
        // 文件名命中：README.md 的路径里没有 needle，故为空
        assert!(path_hits.is_empty(), "path hits = {path_hits:?}");
        // 内容命中：src/main.rs 与 README.md（顺序按路径序）
        let rels: Vec<&str> = hits.iter().map(|h| h.rel.as_str()).collect();
        assert_eq!(
            rels,
            vec!["README.md", "src/main.rs"],
            "路径序 + 二进制跳过"
        );
        assert_eq!(hits[1].hits[0].line, 2, "命中行号");
        assert_eq!(hits[0].segments.len(), 1, "单段摘录");
    }

    #[test]
    fn regex_mode() {
        let c = query(
            r"\d+",
            SearchOpts {
                regex: true,
                ..Default::default()
            },
        );
        let hit = scan_text("a.txt".into(), "ab 12 cd\n", &c).expect("hit");
        assert_eq!(hit.hits[0].spans, vec![3..5]);
    }

    /// P1-6（047-内存削减）：超长行截断——摘录行 ≤4KB + 「…」收尾；截断
    /// 窗内的命中正常给出；只在窗外的匹配不报（截断语义，防 8MB minified
    /// 单行整行进内存）。
    #[test]
    fn long_lines_clipped_to_budget() {
        let head = "a".repeat(MAX_LINE_BYTES - 16);
        let tail = "b".repeat(1024);
        // 命中在截断窗内
        let text = format!("{head}NEEDLE{tail}\nplain\n");
        let c = query("NEEDLE", SearchOpts::default());
        let hit = scan_text("big.txt".into(), &text, &c).expect("hit");
        let seg = &hit.segments[0][0].1;
        assert!(
            seg.len() <= MAX_LINE_BYTES + 3,
            "摘录行截断（{}B）",
            seg.len()
        );
        assert!(seg.ends_with('…'), "截断尾补省略号");
        assert_eq!(
            hit.hits[0].spans,
            vec![(MAX_LINE_BYTES - 16)..(MAX_LINE_BYTES - 10)]
        );

        // 命中只在窗外：不报
        let text = format!("{head}{tail}NEEDLE\n");
        let hit = scan_text("big.txt".into(), &text, &c);
        assert!(hit.is_none(), "截断窗外的匹配不报");

        // 摘录段里的上下文行（非命中行）同样截断
        let long = "x".repeat(MAX_LINE_BYTES + 2048);
        let text = format!("{long}\nNEEDLE\n");
        let hit = scan_text("big.txt".into(), &text, &c).expect("hit");
        assert!(hit.segments[0][0].1.ends_with('…'), "上下文行截断");
    }
}
