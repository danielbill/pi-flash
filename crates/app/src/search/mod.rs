//! 083 项目搜索——Zed `crates/search/src/project_search.rs` 面板的 PF 复刻。
//!
//! 三层职责：
//! - [`query`]：**Zed 原码**（`crates/project/src/search.rs` + `util/paths.rs`
//!   的 `PathMatcher`），逐字抄，只做 `RelPath`→`PathBuf` 的机械替换；
//! - [`engine`]：执行层（候选路径 → `match_path` 过滤 → 读盘扫 → 流式回传），
//!   函数逐条对照 Zed `project_search.rs`；
//! - [`panel`] / [`results`]：搜索条（Zed `ProjectSearchBar::render` 版式）与
//!   结果面板（Zed 多缓冲结果编辑器的视觉等价物）；
//! - [`sessions`]：**会话查询**（2026-10-21 把 013 的弹窗版改成同版式面板，
//!   只查询、不替换/过滤）——组头复用 [`results::card_header`]，正文同一条
//!   单色 + 命中底色口径。见 `docs/模块设计/084-会话查询.md`。
//!
//! 产品级唯一差异：Zed 的「找文件」（Ctrl+P 文件查找器）与「找内容」
//! （Ctrl+Shift+F 项目搜索）是两个面板，PF 合并进同一结果列表——因为它替换
//! 掉的正是原来那个「搜索文件」弹窗（Ctrl+L），用户的心智是「一次输入、文件
//! 与内容一起搜」。文件名命中排最前的独立分组（[`engine::MAX_PATH_HITS`] 封顶）。
//!
//! 全部状态住 [`SearchUi`]（挂在 `WsUi.search`）：搜索是工作区级状态，切会话
//! 不丢、切项目重搜。

pub(crate) mod engine;
mod panel;
mod query;
mod results;
mod sessions;

pub(crate) use panel::{opts_labels, view};
pub(crate) use query::PathMatcher;
pub(crate) use sessions::view as session_view;

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use gpui::Task;

use crate::*;
use engine::SearchOpts;
use query::SearchQuery;

/// 结果列表行高（与 Zed 结果编辑器一致：命中等高行）。
pub(crate) const LINE_H: f32 = 22.;
/// 文件组头**块**行高 = Zed `FILE_HEADER_HEIGHT`(2 行)；条 = 块 − 2×4px 留白
pub(crate) const HEADER_H: f32 = LINE_H * 2.;
/// 搜索联动的防抖（Zed `SEARCH_ON_TYPE_DEBOUNCE` = 250ms）
const DEBOUNCE_MS: u64 = 250;

/// 搜索阶段（Zed `SearchState` 的简化版：PF 没有 Remote/OpenBuffersOnly）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Phase {
    Idle,
    Running,
    Done,
}

/// 一个文件的结果块（Zed 一个 Multibuffer excerpt 组的等价物）。
#[derive(Clone, Debug)]
pub(crate) struct ResultBlock {
    /// 相对 cwd 的 `/` 分隔路径
    pub(crate) rel: String,
    /// 文件名命中（该文件内容里没有命中，只有路径匹配）
    pub(crate) path_only: bool,
    /// 命中行数（path_only 时为 0）
    pub(crate) count: usize,
    /// 摘录段（每段若干连续行；多段 = 一段代码里的多个命中窗口）
    pub(crate) segments: Vec<Vec<ResultLine>>,
}

/// 摘录里的一行。
#[derive(Clone, Debug)]
pub(crate) struct ResultLine {
    /// 1-based 行号
    pub(crate) line: u32,
    pub(crate) text: String,
    /// 行内命中区间（byte）
    pub(crate) spans: Vec<Range<usize>>,
    /// 命中行 = true；上下文行 = false
    pub(crate) hit: bool,
    /// 全场命中序号（0-based；上下文行为 None）
    pub(crate) ordinal: Option<usize>,
}

/// 项目搜索的界面状态。
pub(crate) struct SearchUi {
    /// 四个输入框（首次进入搜索面板时懒建，跨开关复用）
    pub(crate) query: Option<gpui::Entity<TextInput>>,
    pub(crate) replace: Option<gpui::Entity<TextInput>>,
    pub(crate) include: Option<gpui::Entity<TextInput>>,
    pub(crate) exclude: Option<gpui::Entity<TextInput>>,
    pub(crate) opts: SearchOpts,
    /// 查询词镜像（`on_change` 同步；渲染帧不读输入框实体）
    pub(crate) query_text: String,
    /// 替换词镜像（同上）
    pub(crate) replace_text: String,
    pub(crate) show_replace: bool,
    pub(crate) show_filters: bool,
    pub(crate) phase: Phase,
    /// 结果块模型（跨帧只克隆 Arc；追加走 Arc::make_mut）
    pub(crate) blocks: Arc<Vec<ResultBlock>>,
    /// 被收起（只留文件头）的块：**按相对路径记**——块下标会随查询重排，
    /// 路径不会（Zed 的收起也是挂在 excerpt 头上，不跟序号走）
    pub(crate) collapsed: Arc<std::collections::HashSet<String>>,
    /// 结果列表状态（变高块列表：每块 = 文件头 + 若干行）
    pub(crate) list: gpui::ListState,
    /// 结果区竖向滚动条状态（gpui-component `Scrollbar`；与 md 预览同款）
    pub(crate) scrollbar: gpui_component::scroll::ScrollbarState,
    /// 命中的内容文件数 / 文件名命中数 / 命中行总数 / 已读盘文件数
    pub(crate) files: usize,
    pub(crate) path_files: usize,
    pub(crate) matches: usize,
    pub(crate) scanned: usize,
    /// 当前选中的命中序号（对应 `ResultLine.ordinal`）
    pub(crate) active: Option<usize>,
    /// 查询/过滤器的编译错误
    pub(crate) error: Option<String>,
    /// 触到结果上限（计数后缀 `+`）
    pub(crate) limit: bool,
    /// 替换结果提示（文本, 是否失败）
    pub(crate) notice: Option<(String, bool)>,
    /// 代际：每次发起搜索 +1，陈旧批次按代际丢弃
    pub(crate) generation: u64,
    pub(crate) cancel: Arc<AtomicBool>,
    pub(crate) pending: Option<Task<()>>,
    /// 防抖任务（新输入 drop 旧任务 = 取消旧定时器）
    pub(crate) debounce: Option<Task<()>>,
}

impl Default for SearchUi {
    fn default() -> Self {
        Self {
            query: None,
            replace: None,
            include: None,
            exclude: None,
            opts: SearchOpts::default(),
            query_text: String::new(),
            replace_text: String::new(),
            show_replace: false,
            show_filters: false,
            phase: Phase::Idle,
            blocks: Arc::new(Vec::new()),
            collapsed: Arc::new(std::collections::HashSet::new()),
            list: gpui::ListState::new(0, gpui::ListAlignment::Top, px(1000.)),
            scrollbar: gpui_component::scroll::ScrollbarState::default(),
            files: 0,
            path_files: 0,
            matches: 0,
            scanned: 0,
            active: None,
            error: None,
            limit: false,
            notice: None,
            generation: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            pending: None,
            debounce: None,
        }
    }
}

mod actions;
mod nav;
mod replace;

#[cfg(test)]
mod tests;

/// 显示规则（纯函数便于单测）：内容命中到达时，摘掉同名的「仅文件名命中」
/// 文件栏——**内容与标题都命中的文件只展示内容组**（2026-10-21 用户定案：
/// 「只有标题命中肯定显示文件栏，内容和标题都命中了就不重复展示」）。
/// 返回是否真的摘掉了东西。
pub(crate) fn drop_path_only_for(blocks: &mut Vec<ResultBlock>, rel: &str) -> bool {
    let before = blocks.len();
    blocks.retain(|b| !(b.path_only && b.rel == rel));
    blocks.len() != before
}

/// include/exclude 输入框 → glob 列表（Zed 用逗号分隔；PF 额外支持换行）。
pub(crate) fn split_globs(text: String) -> Vec<String> {
    text.split([',', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// 建查询（Zed `SearchQuery::text` / `regex` 的分流 + `PathMatcher` 构造）。
pub(crate) fn build_query(
    text: &str,
    opts: SearchOpts,
    include: Vec<String>,
    exclude: Vec<String>,
) -> Result<SearchQuery, String> {
    let is_posix = !cfg!(windows);
    let inc = PathMatcher::new(include, is_posix).map_err(|e| e.to_string())?;
    let exc = PathMatcher::new(exclude, is_posix).map_err(|e| e.to_string())?;
    let built = if opts.regex {
        SearchQuery::regex(
            text,
            opts.whole_word,
            opts.case_sensitive,
            false,
            inc,
            exc,
            false,
        )
    } else {
        SearchQuery::text(
            text,
            opts.whole_word,
            opts.case_sensitive,
            false,
            inc,
            exc,
            false,
        )
    };
    built.map_err(|e| e.to_string())
}

/// 对整段文本做替换，返回 (新文本, 替换处数)。
///
/// 逐行调用 Zed 的 `replacement_for`（它按命中区间精确重排捕获组，等价于
/// Zed 结果编辑器里的 `ReplaceAll` 施加在每个 range 上），再拼回文本——这样
/// `$1`/`\n` 等语义与 Zed 完全一致。
fn replace_in_text(q: &SearchQuery, src: &str, next_only: bool) -> (String, usize) {
    let mut out = String::with_capacity(src.len());
    let mut count = 0usize;
    let mut done = false;
    // split_inclusive 让每行（含行尾换行）原样拼回：CRLF 与「末尾无换行」
    // 都不会被改写
    for chunk in src.split_inclusive('\n') {
        let (raw, had_lf) = match chunk.strip_suffix('\n') {
            Some(rest) => (rest, true),
            None => (chunk, false),
        };
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let crlf = raw.len() != line.len();
        if done {
            out.push_str(chunk);
            continue;
        }
        let spans = q.search_str(line);
        if spans.is_empty() {
            out.push_str(chunk);
            continue;
        }
        let mut cursor = 0usize;
        for span in spans {
            out.push_str(&line[cursor..span.start]);
            match q.replacement_for(line, span.clone()) {
                Some(rep) => out.push_str(&rep),
                None => out.push_str(&line[span.clone()]),
            }
            cursor = span.end;
            count += 1;
            if next_only {
                done = true;
                break;
            }
        }
        out.push_str(&line[cursor..]);
        if crlf {
            out.push('\r');
        }
        if had_lf {
            out.push('\n');
        }
    }
    (out, count)
}

/// 搜索面板上的六个开关。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchToggle {
    CaseSensitive,
    WholeWord,
    Regex,
    IncludeIgnored,
    Filters,
    Replace,
}
