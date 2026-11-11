//! pi-flash patch（对齐 zed）：输入辅助（typing assists）。
//!
//! zed 的输入辅助由语言配置驱动（grammars config.toml：pairs/注释符/列表
//! 模式，见 zed crates/grammars/src/markdown/config.toml 等）；PF 没有语言
//! 配置层，这里以 ts_lang 名为键建小型静态表（PF 的 ts_lang 集合见 app 侧
//! editor/kind.rs），逐项对齐 zed 默认语义：
//!
//! - auto-surround：有选区时输入开括号/引号包裹选区（zed use_auto_surround）
//! - 列表续行族：无序 `- * +` / 有序 `1.` 递增 / 任务 `- [ ]`；空列表项
//!   按 Enter 清标记退出列表（zed extend_list_on_newline）
//! - 整行复制/剪切：空选区 Ctrl+C/X 取整行（含行尾换行），剪贴板带
//!   entire-line 标记，空选区粘贴时插到当前行上方（zed clipboard.rs）
//!
//! 性能铁律：只做 O(当前行/选区) 级逻辑，禁每键全文扫描。

use gpui::{ClipboardItem, Context, Window};

use super::mode::InputMode;
use super::state::InputState;
use super::RopeExt as _;
use std::ops::Range;

/// 括号对（zed PairConfig 对应物）。单字符对（zed surround 要求
/// `start.chars().count() == 1`）。close/newline 自 P1b（autoclose /
/// 花括号三行结构）批次接入，先随表登记。
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(super) struct Pair {
    pub start: char,
    pub end: char,
    /// 输入 start 自动补 end（zed pair close；P1b autoclose 用）
    pub close: bool,
    /// 有选区时输入 start 包裹选区（zed pair surround）
    pub surround: bool,
    /// 空对内 Enter 生成三行结构（zed pair newline；P1b 用）
    pub newline: bool,
}

const fn p(start: char, end: char, close: bool, surround: bool, newline: bool) -> Pair {
    Pair {
        start,
        end,
        close,
        surround,
        newline,
    }
}

/// 列表续行配置（zed unordered_list / ordered_list / task_list 对应物）。
/// 标记/前缀均含尾随空格——空项判定（光标 == 标记末尾）依赖它。
pub(super) struct ListConfig {
    pub task_prefixes: &'static [&'static str],
    pub task_continuation: &'static str,
    pub unordered: &'static [&'static str],
    /// 有序列表分隔点（zed pattern `(\d+)\. ` 的 `.`；非正则实现）
    pub ordered_dots: &'static [char],
}

pub(super) struct LangAssist {
    pub pairs: &'static [Pair],
    pub lists: Option<&'static ListConfig>,
    /// 行注释标记（含尾随空格，zed line_comment；长的在前使 `/// ` 压过
    /// `// ` 最长匹配）
    pub line_comments: &'static [&'static str],
    /// 块注释（zed block_comment：续行前缀 + start tag 行加档空格数）
    pub block_comment: Option<&'static BlockComment>,
}

/// 块注释续行配置（zed block_comment config 对应物）。
pub(super) struct BlockComment {
    pub start: &'static str,
    pub end: &'static str,
    /// 续行前缀（rustdoc/jsdoc 风格 "* "；HTML 注释为空）
    pub prefix: &'static str,
    /// start tag 行续行前缀前的额外空格数（zed tab_size；1 使 `*` 对齐
    /// `/*` 的星号位）
    pub tab_size: usize,
}

/// 自动补入括号对的追踪（zed AutocloseRegion 对应物，PF 单选区单实例）：
/// overtype（键入闭合符越过）与 backspace 成对删除用。
#[derive(Clone, Copy)]
pub(super) struct AutoclosedPair {
    pub open: usize,
    pub close: usize,
    pub open_ch: char,
    pub close_ch: char,
}

/// autoclose 前文允许集（zed 默认 `autoclose_before = ";:.,=}])>"`）
const AUTOCLOSE_BEFORE: &str = ";:.,=}])>";

/// zed markdown 配置（crates/grammars/src/markdown/config.toml）：任务前缀
/// 续行统一回 `- [ ] `（勾选续行变未勾选）；有序 `(\d+)\. `。
static MD_LISTS: ListConfig = ListConfig {
    task_prefixes: &["- [ ] ", "- [x] ", "- [X] "],
    task_continuation: "- [ ] ",
    unordered: &["- ", "* ", "+ "],
    ordered_dots: &['.'],
};

macro_rules! code_brackets {
    ($($extra:expr),* $(,)?) => {
        &[
            p('(', ')', true, true, true),
            p('[', ']', true, true, true),
            p('{', '}', true, true, true),
            p('"', '"', true, true, false),
            $($extra),*
        ]
    };
}

/// rustdoc/jsdoc 风格块注释：`*` 对齐 `/*` 的星号位（tab_size=1 加档）
static BLOCK_C: BlockComment = BlockComment {
    start: "/*",
    end: "*/",
    prefix: "* ",
    tab_size: 1,
};
/// HTML/Markdown 注释块（zed markdown config 同款：prefix 空、tab_size 1）
static BLOCK_XML: BlockComment = BlockComment {
    start: "<!--",
    end: "-->",
    prefix: "",
    tab_size: 1,
};

static RUST: LangAssist = LangAssist {
    pairs: code_brackets!(),
    lists: None,
    line_comments: &["/// ", "//! ", "// "],
    block_comment: Some(&BLOCK_C),
};
static PYTHON: LangAssist = LangAssist {
    pairs: code_brackets!(p('\'', '\'', true, true, false)),
    lists: None,
    line_comments: &["# "],
    block_comment: None,
};
static JS: LangAssist = LangAssist {
    pairs: code_brackets!(p('\'', '\'', true, true, false), p('`', '`', true, true, false)),
    lists: None,
    line_comments: &["// "],
    block_comment: Some(&BLOCK_C),
};
static GO: LangAssist = LangAssist {
    pairs: code_brackets!(p('`', '`', true, true, false)),
    lists: None,
    line_comments: &["// "],
    block_comment: Some(&BLOCK_C),
};
static JSON: LangAssist = LangAssist {
    pairs: code_brackets!(),
    lists: None,
    line_comments: &[],
    block_comment: None,
};
static C_LIKE: LangAssist = LangAssist {
    pairs: code_brackets!(p('\'', '\'', true, true, false)),
    lists: None,
    line_comments: &["// "],
    block_comment: Some(&BLOCK_C),
};
static HTML: LangAssist = LangAssist {
    pairs: code_brackets!(p('\'', '\'', true, true, false), p('<', '>', true, true, true)),
    lists: None,
    line_comments: &[],
    block_comment: Some(&BLOCK_XML),
};
static CSS: LangAssist = LangAssist {
    pairs: code_brackets!(),
    lists: None,
    line_comments: &[],
    block_comment: Some(&BLOCK_C),
};
static TOML: LangAssist = LangAssist {
    pairs: &[
        p('(', ')', true, true, true),
        p('[', ']', true, true, true),
        p('"', '"', true, true, false),
    ],
    lists: None,
    line_comments: &["# "],
    block_comment: None,
};
static YAML: LangAssist = LangAssist {
    pairs: code_brackets!(p('\'', '\'', true, true, false)),
    lists: None,
    line_comments: &["# "],
    block_comment: None,
};
/// zed markdown：括号 close+newline；引号/反引号 close=false（只插入不补
/// 对）；`*` `~` 仅 surround（粗体/删除线包裹）。
static MARKDOWN: LangAssist = LangAssist {
    pairs: &[
        p('(', ')', true, true, true),
        p('[', ']', true, true, true),
        p('{', '}', true, true, true),
        p('<', '>', true, true, true),
        p('"', '"', false, false, false),
        p('\'', '\'', false, false, false),
        p('`', '`', false, false, false),
        p('*', '*', false, true, false),
        p('~', '~', false, true, false),
    ],
    lists: Some(&MD_LISTS),
    line_comments: &[],
    block_comment: Some(&BLOCK_XML),
};
/// 未知语言兜底：基础括号 + 引号（zed 无语言配置时不启用任何辅助；PF 的
/// code_editor 只挂在已注册 kind 上，兜底仅为未知 ts_lang 留基础体验）。
static FALLBACK: LangAssist = LangAssist {
    pairs: code_brackets!(),
    lists: None,
    line_comments: &[],
    block_comment: None,
};

static TABLE: &[(&str, &LangAssist)] = &[
    ("rust", &RUST),
    ("python", &PYTHON),
    ("javascript", &JS),
    ("typescript", &JS),
    ("tsx", &JS),
    ("go", &GO),
    ("json", &JSON),
    ("c", &C_LIKE),
    ("html", &HTML),
    ("css", &CSS),
    ("toml", &TOML),
    ("yaml", &YAML),
    ("markdown", &MARKDOWN),
];

pub(super) fn lang_assist_for(name: &str) -> &'static LangAssist {
    TABLE
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| *v)
        .unwrap_or(&FALLBACK)
}

impl InputState {
    /// 当前语言辅助配置；仅 code_editor 模式有（composer 等多行输入不受
    /// 输入辅助影响，行为不变）。
    pub(super) fn lang_assist(&self) -> Option<&'static LangAssist> {
        match &self.mode {
            InputMode::CodeEditor { language, .. } => Some(lang_assist_for(language.as_ref())),
            _ => None,
        }
    }

    /// 纯键入路径的单字符辅助（replace_text_in_range 钩子调用）。命中返回
    /// true 表示已处理，调用方直接返回不再走通用替换。
    pub(super) fn try_type_assist(
        &mut self,
        typed: char,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(lang) = self.lang_assist() else {
            return false;
        };

        // auto-surround（zed input.rs handle_input surround 分支）：有选区
        // + 输入单字符开括号/引号 → 包裹选区，选区保留在内部
        let pair = if self.selected_range.is_empty() {
            None
        } else {
            lang.pairs
                .iter()
                .find(|pair| pair.start == typed && pair.surround)
                .copied()
        };
        if let Some(pair) = pair {
            let range = self.selected_range;
            let inner = self.text.slice(range.start..range.end).to_string();
            let new_text = format!("{}{}{}", pair.start, inner, pair.end);
            self.replace_text_in_range_silent(
                Some(self.range_to_utf16(&(range.start..range.end))),
                &new_text,
                window,
                cx,
            );
            let head = range.start + pair.start.len_utf8();
            self.selected_range = (head..head + inner.len()).into();
            self.update_preferred_column();
            cx.notify();
            return true;
        }

        // autoclose / overtype（zed input.rs handle_input，仅空选区）
        if self.selected_range.is_empty() {
            let cursor = self.cursor();

            // autoclose（zed 条件：following/preceding_text_allows_autoclose
            // + 非 closing quote）→ 补闭合对，光标留在对内并登记追踪
            let open_pair = lang.pairs.iter().copied().find(|p| p.start == typed);
            if let Some(pair) = open_pair {
                if pair.close && self.autoclose_allowed(lang, cursor, pair) {
                    let new_text = format!("{}{}", typed, pair.end);
                    self.replace_text_in_range_silent(
                        Some(self.range_to_utf16(&(cursor..cursor))),
                        &new_text,
                        window,
                        cx,
                    );
                    self.autoclosed_pair = Some(AutoclosedPair {
                        open: cursor,
                        close: cursor + typed.len_utf8(),
                        open_ch: typed,
                        close_ch: pair.end,
                    });
                    return true;
                }
            }

            // overtype（zed should_skip）：光标恰在自动补入闭合符前且键入
            // 同符 → 越过不插入（追踪即废）
            let stale = self.autoclosed_pair;
            if let Some(p) = stale {
                if cursor == p.close
                    && p.close_ch == typed
                    && self.text.slice(cursor..self.text.len()).chars().next() == Some(typed)
                {
                    self.autoclosed_pair = None;
                    self.move_to((cursor + typed.len_utf8()).min(self.text.len()), cx);
                    return true;
                }
            }
        }

        false
    }

    /// autoclose 前后文校验（zed following_text_allows_autoclose +
    /// preceding_text_allows_autoclose + is_closing_quote 三条件）。
    fn autoclose_allowed(&self, lang: &LangAssist, cursor: usize, pair: Pair) -> bool {
        // 后文：行尾/空白/autoclose_before 表/任一闭合括号才补对
        // （`x(`、`foo"` 压词字符场景不补）
        let next_ok = match self.text.slice(cursor.min(self.text.len())..).chars().next() {
            None => true,
            Some(c) => {
                c.is_whitespace()
                    || AUTOCLOSE_BEFORE.contains(c)
                    || lang.pairs.iter().any(|p| p.end == c)
            }
        };
        if !next_ok {
            return false;
        }
        if pair.start != pair.end {
            // 括号对（start≠end）：zed 前文无条件放行
            return true;
        }
        // 引号对：前一字符不是词字符（`it's` 中打 ' 不补对）
        let row = self.text.offset_to_point(cursor).row;
        let line_start = self.text.line_start_offset(row);
        let before = self.text.slice(line_start..cursor).to_string();
        if let Some(prev) = before.chars().next_back() {
            if prev.is_alphanumeric() || prev == '_' {
                return false;
            }
        }
        // 引号对：本行光标前已有奇数个同符 → 本次键入是闭合引号，不补对
        before.matches(pair.start).count() % 2 == 0
    }

    /// 光标落在自动补入括号对之外时失效追踪（move_to / set_selected_range
    /// 共用；zed region 仅在选区处于对内时参与输入辅助）。
    pub(super) fn autoclose_invalidate_outside(&mut self, offset: usize) {
        if let Some(p) = self.autoclosed_pair {
            if offset <= p.open || offset > p.close {
                self.autoclosed_pair = None;
            }
        }
    }

    /// backspace 成对删除（zed：仅自动补入的括号对）：空选区、光标恰在
    /// 闭合符前、左侧即开括号 → 一次删一对（单步 undo）。
    pub(super) fn backspace_delete_pair(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.mode.is_code_editor() {
            return false;
        }
        let cursor = self.cursor();
        if cursor == 0 {
            return false;
        }
        let p = match self.autoclosed_pair {
            Some(p) => p,
            None => return false,
        };
        if cursor != p.close || cursor - 1 != p.open {
            return false;
        }
        // 结构校验（平移异常兜底）：范围合法且两端字符仍与登记一致
        if p.close + p.close_ch.len_utf8() > self.text.len() {
            self.autoclosed_pair = None;
            return false;
        }
        let mut chars = self.text.slice(p.open..self.text.len()).chars();
        if chars.next() != Some(p.open_ch) || chars.next() != Some(p.close_ch) {
            self.autoclosed_pair = None;
            return false;
        }
        let range = p.open..p.close + p.close_ch.len_utf8();
        self.replace_text_in_range_silent(Some(self.range_to_utf16(&range)), "", window, cx);
        self.autoclosed_pair = None;
        true
    }

    /// 块注释续行（zed documentation_delimiter_for_newline）：光标在
    /// start tag 或 prefix 之后且确在未闭合块内 → 换行 + 前缀；光标仍在本
    /// 行 end tag 之前时再补一行对齐（`/* x| */` Enter 出三行结构）。
    /// 返回 true 表示已处理。
    #[allow(clippy::too_many_arguments)]
    fn enter_block_comment_continuation(
        &mut self,
        bc: &BlockComment,
        line: &str,
        indent_str: &str,
        full_ws: usize,
        col: usize,
        cursor: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let after_ws = &line[full_ws.min(line.len())..];

        // 光标在 start tag 之后（zed cursor_is_after_start_tag）
        let after_start = after_ws.starts_with(bc.start) && full_ws + bc.start.len() <= col;
        // 光标在 prefix 本体之后（zed cursor_is_after_delimiter）
        let prefix_trim = bc.prefix.trim_end();
        let after_prefix = !prefix_trim.is_empty()
            && after_ws.starts_with(prefix_trim)
            && full_ws + prefix_trim.len() <= col;
        if !(after_start || after_prefix) {
            return false;
        }

        // 确在未闭合块注释内（PF 无 tree-sitter scope 查询，向上有限行
        // 扫描找未配对 start；仅 Enter 触发，非每键热路径）
        if !self.within_block_comment(bc) {
            return false;
        }

        // start tag 行续行带一档额外缩进（zed additional_indent =
        // spaces(tab_size)，让 `*` 对齐 `/*` 星号位）；prefix 行不加档
        let add = if after_start {
            " ".repeat(bc.tab_size.max(1))
        } else {
            String::new()
        };
        let mut new_text = format!("\n{}{}{}", indent_str, add, bc.prefix);

        // 光标后同行还有 end tag？（zed cursor_is_before_end_tag）→ 补尾行；
        // 光标恰在 end tag 起点时尾行加一档（zed 同款）
        let row = self.text.offset_to_point(cursor).row;
        let line_end = self.text.line_end_offset(row).min(self.text.len());
        let tail = self.text.slice(cursor.min(line_end)..line_end).to_string();
        if let Some(idx) = tail.find(bc.end) {
            new_text.push('\n');
            new_text.push_str(indent_str);
            if idx == 0 {
                new_text.push_str(&" ".repeat(bc.tab_size.max(1)));
            }
        }

        self.replace_text_in_range_silent(
            Some(self.range_to_utf16(&(cursor..cursor))),
            &new_text,
            window,
            cx,
        );
        true
    }

    /// 光标行是否位于未闭合的块注释内：光标行按「光标之前」的文本判定
    /// （`/* ab|c */` 的 end 在光标后，不算闭合），其上各行按整行判定，
    /// 有限行扫描（≤300 行）——start 更近即块内；平衡行（同行闭合）视为
    /// 中性继续向上。PF 无 tree-sitter scope 查询的近似（嵌套注释不计）；
    /// 仅 Enter 触发，非每键热路径。
    fn within_block_comment(&self, bc: &BlockComment) -> bool {
        let cursor = self.cursor();
        let cursor_row = self.text.offset_to_point(cursor).row;
        let line_start = self.text.line_start_offset(cursor_row);
        let head = self.text.slice(line_start..cursor).to_string();
        let mut row = cursor_row;
        let mut scanned = 0usize;
        loop {
            let text = if row == cursor_row {
                head.clone()
            } else {
                self.text.slice_line(row).to_string()
            };
            match (text.rfind(bc.start), text.rfind(bc.end)) {
                (Some(s), Some(e)) if s > e => return true, // 最近的 start 未闭合
                (Some(_), None) => return true,             // 仅 start → 块内
                (None, Some(_)) => return false,            // 仅 end → 块外
                _ => {}                                     // 平衡/无关行 → 继续向上
            }
            if row == 0 || scanned >= 300 {
                return false;
            }
            row -= 1;
            scanned += 1;
        }
    }

    /// 光标两侧最近非空白字符恰构成 newline=true 括号对（限当前行内，zed
    /// insert_extra_newline_brackets 的空隙校验：中间只许空格/制表）。
    fn bracket_pair_around(&self, lang: Option<&LangAssist>, cursor: usize) -> Option<Pair> {
        let lang = lang?;
        let row = self.text.offset_to_point(cursor).row;
        let line_start = self.text.line_start_offset(row);
        let line_end = self.text.line_end_offset(row).min(self.text.len());
        let before = self.text.slice(line_start..cursor.min(cursor)).to_string();
        let after = self.text.slice(cursor.max(line_start)..line_end.max(cursor)).to_string();
        let prev = before.chars().rev().find(|c| !c.is_whitespace())?;
        let next = after.chars().find(|c| !c.is_whitespace())?;
        lang.pairs
            .iter()
            .copied()
            .find(|p| p.start == prev && p.end == next && p.newline)
    }

    /// code_editor 的 Enter 换行辅助（动作版，必然处理）。优先级对齐 zed
    /// newline()（input.rs:535）：注释续行 > 块注释续行 > 列表续行 > 花括号
    /// 对内三行结构 > 默认缩进；P1a 先落列表族与默认缩进（zed 语义：当前
    /// 行缩进截到光标列；光标贴住缩进末尾时清掉行首残留缩进）。
    pub(super) fn enter_newline_assist(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let sel = self.selected_range;
        let cursor = sel.start;
        let row = self.text.offset_to_point(cursor).row;
        let line_start = self.text.line_start_offset(row);
        let line = self.text.slice_line(row).to_string();
        let col = cursor.saturating_sub(line_start);
        let full_ws = line
            .bytes()
            .take_while(|b| *b == b' ' || *b == b'\t')
            .count();
        let indent_len = full_ws.min(col);
        let indent_str = line[..indent_len.min(line.len())].to_string();

        let lang = self.lang_assist();
        let lists = lang.and_then(|l| l.lists);
        let (tab_size, hard_tabs) = self
            .mode
            .tab_size()
            .map(|t| (t.tab_size, t.hard_tabs))
            .unwrap_or((2, false));

        // 以下辅助均要求空选区（zed newline 各 delimiter 分支同守卫）
        if sel.is_empty() {
            // 行注释续行（zed extend_comment_on_newline，delimiter 优先级
            // 最高：comment > documentation > list）
            let line_delim = match lang {
                Some(l) => line_comment_delimiter(l, &line, col, full_ws),
                None => None,
            };
            if let Some(delim) = line_delim {
                let new_text = format!("\n{}{}", indent_str, delim);
                self.replace_text_in_range_silent(
                    Some(self.range_to_utf16(&(cursor..cursor))),
                    &new_text,
                    window,
                    cx,
                );
                return;
            }

            // 块注释续行（zed documentation_delimiter_for_newline：
            // `/*` 行三行结构 + `* ` 前缀行续行）
            let bc = lang.and_then(|l| l.block_comment);
            if let Some(bc) = bc {
                if self.enter_block_comment_continuation(
                    bc,
                    &line,
                    &indent_str,
                    full_ws,
                    col,
                    cursor,
                    window,
                    cx,
                ) {
                    return;
                }
            }

            // 列表续行族（zed extend_list_on_newline）
            if let Some(list) = lists {
                if self.enter_list_continuation(
                    list,
                    &line,
                    &indent_str,
                    full_ws,
                    col,
                    line_start,
                    tab_size,
                    hard_tabs,
                    window,
                    cx,
                ) {
                    return;
                }
            }

            // 花括号/圆括号对内 Enter → 三行结构（zed
            // insert_extra_newline_brackets；无 tree-sitter indent query，
            // 中行缩进按当前制表位一档，末行对齐缩进）
            if self.bracket_pair_around(lang, cursor).is_some() {
                let unit = self
                    .mode
                    .tab_size()
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "    ".into());
                let new_text = format!("\n{}{}\n{}", indent_str, unit, indent_str);
                self.replace_text_in_range_silent(
                    Some(self.range_to_utf16(&(cursor..cursor))),
                    &new_text,
                    window,
                    cx,
                );
                return;
            }
        }

        // 默认：换行 + 缩进（zed：当前行缩进截到光标列）。空选区且光标贴
        // 住缩进末尾（空行/纯缩进行）时编辑扩展到行首，清掉残留缩进空白，
        // 避免上一行留下尾随空格（zed 同款）。
        let mut edit_start = sel.start;
        if sel.is_empty() && full_ws > 0 && col == full_ws {
            edit_start = line_start;
        }
        let new_text = format!("\n{}", indent_str);
        self.replace_text_in_range_silent(
            Some(self.range_to_utf16(&(edit_start..sel.end))),
            &new_text,
            window,
            cx,
        );
    }

    /// 列表续行（zed list_delimiter_for_newline 语义）：
    /// - 标记后有内容且光标在标记之后 → 换行 + 同缩进 + 同/递增标记；
    /// - 光标恰在标记末尾且无内容（空列表项）→ 退出列表：顶格清标记
    ///   （ClearCurrentLine），带缩进则退一个制表位保留标记
    ///   （UnindentCurrentLine）——连按 Enter 逐级退出嵌套。
    /// 返回 true 表示已处理。
    #[allow(clippy::too_many_arguments)]
    fn enter_list_continuation(
        &mut self,
        list: &ListConfig,
        line: &str,
        indent_str: &str,
        full_ws: usize,
        col: usize,
        line_start: usize,
        tab_size: usize,
        hard_tabs: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let after_ws = &line[full_ws.min(line.len())..];

        // 最长匹配：任务前缀（`- [ ] `）优先于无序标记（`- `）
        let mut best: Option<(usize, String, String)> = None; // (marker_len, 有内容续行, 空项续行)
        for pfx in list.task_prefixes {
            if after_ws.starts_with(pfx)
                && best.as_ref().map_or(true, |(l, _, _)| pfx.len() > *l)
            {
                best = Some((
                    pfx.len(),
                    list.task_continuation.to_string(),
                    list.task_continuation.to_string(),
                ));
            }
        }
        for m in list.unordered {
            if after_ws.starts_with(m) && best.as_ref().map_or(true, |(l, _, _)| m.len() > *l) {
                best = Some((m.len(), m.to_string(), m.to_string()));
            }
        }
        if best.is_none() {
            if let Some((marker_len, n, dot)) = parse_ordered(after_ws, list.ordered_dots) {
                // zed：有内容续行数字 +1；空项续行复位为 1
                best = Some((
                    marker_len,
                    format!("{}{} ", n + 1, dot),
                    format!("1{} ", dot),
                ));
            }
        }
        let Some((marker_len, cont_next, cont_exit)) = best else {
            return false;
        };

        let end_of_prefix = full_ws + marker_len;
        let has_content = line[end_of_prefix.min(line.len())..]
            .chars()
            .any(|c| !c.is_whitespace());
        let cursor = self.cursor();

        if has_content && col >= end_of_prefix {
            let new_text = format!("\n{}{}", indent_str, cont_next);
            self.replace_text_in_range_silent(
                Some(self.range_to_utf16(&(cursor..cursor))),
                &new_text,
                window,
                cx,
            );
            return true;
        }

        if col == end_of_prefix {
            if full_ws == 0 {
                // 顶格空项：清掉标记本身，不插换行（zed ClearCurrentLine）
                self.replace_text_in_range_silent(
                    Some(self.range_to_utf16(&(line_start..cursor))),
                    "",
                    window,
                    cx,
                );
            } else {
                // 缩进空项：退一个制表位、保留标记（zed UnindentCurrentLine）
                let new_text = format!("{}{}", outdent_one_stop(indent_str, tab_size, hard_tabs), cont_exit);
                self.replace_text_in_range_silent(
                    Some(self.range_to_utf16(&(line_start..cursor))),
                    &new_text,
                    window,
                    cx,
                );
            }
            return true;
        }

        false
    }

    /// 空选区整行复制内容（zed do_copy is_entire_line 分支）：含行尾换行；
    /// 末行无换行时补 '\n'。剪贴板带 ENTIRE_LINE_META 标记，供空选区粘贴
    /// 时插到当前行上方。
    pub(crate) fn whole_line_clipboard_item(&self) -> ClipboardItem {
        let row = self.text.offset_to_point(self.cursor()).row;
        let line_start = self.text.line_start_offset(row);
        let line_end = self.text.line_end_offset(row).min(self.text.len());
        let mut copied = self.text.slice(line_start..line_end).to_string();
        if line_end >= self.text.len() {
            copied.push('\n');
        }
        ClipboardItem::new_string_with_metadata(copied, ENTIRE_LINE_META.to_string())
    }

    /// 空选区整行删除范围（cut 用）：非末行含行尾换行，末行删到行尾。
    pub(crate) fn whole_line_range(&self) -> Range<usize> {
        let row = self.text.offset_to_point(self.cursor()).row;
        let line_start = self.text.line_start_offset(row);
        line_start..self.text.line_end_offset(row).min(self.text.len())
    }
}

/// 整行复制的剪贴板元数据标记（gpui ClipboardString metadata 原样字符串，
/// Windows 后端经注册格式随 OS 剪贴板往返持久）。
pub(super) const ENTIRE_LINE_META: &str = "pf:entire-line";

/// 行注释续行标记（zed comment_delimiter_for_newline）：光标在行注释标记
/// 之后 → Some(标记含尾随空格)；块注释起始行不算（zed 同款排除）。
fn line_comment_delimiter(
    lang: &LangAssist,
    line: &str,
    col: usize,
    full_ws: usize,
) -> Option<&'static str> {
    let after_ws = &line[full_ws.min(line.len())..];
    let mut best: Option<&'static str> = None;
    for delim in lang.line_comments {
        if after_ws.starts_with(delim.trim_end()) && best.map_or(true, |d| delim.len() > d.len()) {
            best = Some(delim);
        }
    }
    let delim = best?;
    // 块注释起始行不算行注释（zed：block start 与行注释前缀同形时排除，
    // 如 lua `--`/`--[[`）
    if let Some(bc) = lang.block_comment {
        let bs = bc.start.trim_end();
        if bs.starts_with(delim.trim_end()) && after_ws.starts_with(bs) {
            return None;
        }
    }
    // 光标须越过标记本体（zed cursor_is_placed_after_comment_marker）
    if full_ws + delim.trim_end().len() <= col {
        Some(delim)
    } else {
        None
    }
}

/// 解析有序列表标记（zed ordered_list pattern `(\d+)\. ` 的非正则版）：
/// 返回 (标记字节数, 数字, 分隔点)。
fn parse_ordered(after_ws: &str, dots: &[char]) -> Option<(usize, u32, char)> {
    let mut n: u32 = 0;
    let mut digits: usize = 0;
    let mut bytes: usize = 0;
    for c in after_ws.chars() {
        match c.to_digit(10) {
            Some(d) if digits < 9 => {
                n = n.checked_mul(10)?.checked_add(d)?;
                digits += 1;
                bytes += c.len_utf8();
            }
            _ => break,
        }
    }
    if digits == 0 {
        return None;
    }
    let dot = after_ws[bytes..].chars().next()?;
    if !dots.contains(&dot) || !after_ws[bytes + dot.len_utf8()..].starts_with(' ') {
        return None;
    }
    Some((bytes + dot.len_utf8() + 1, n, dot))
}

/// 从行首缩进退一个制表位（zed `IndentSize::outdent_len` 语义：当前缩进
/// 列退到上一档；恰在整档则退一整档）。
fn outdent_one_stop(indent: &str, tab_size: usize, hard_tabs: bool) -> String {
    if indent.is_empty() {
        return String::new();
    }
    if hard_tabs || indent.ends_with('\t') {
        let mut s = indent.to_string();
        if s.ends_with('\t') {
            s.pop();
        }
        return s;
    }
    let unit = tab_size.max(1);
    let n = indent.len();
    let remove = if n % unit == 0 { unit } else { n % unit };
    let remove = remove.min(n);
    indent[..n - remove].to_string()
}
