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
}

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

static RUST: LangAssist = LangAssist { pairs: code_brackets!(), lists: None };
static PYTHON: LangAssist = LangAssist {
    pairs: code_brackets!(p('\'', '\'', true, true, false)),
    lists: None,
};
static JS: LangAssist = LangAssist {
    pairs: code_brackets!(p('\'', '\'', true, true, false), p('`', '`', true, true, false)),
    lists: None,
};
static GO: LangAssist = LangAssist {
    pairs: code_brackets!(p('`', '`', true, true, false)),
    lists: None,
};
static JSON: LangAssist = LangAssist { pairs: code_brackets!(), lists: None };
static C_LIKE: LangAssist = LangAssist {
    pairs: code_brackets!(p('\'', '\'', true, true, false)),
    lists: None,
};
static HTML: LangAssist = LangAssist {
    pairs: code_brackets!(p('\'', '\'', true, true, false), p('<', '>', true, true, true)),
    lists: None,
};
static CSS: LangAssist = LangAssist { pairs: code_brackets!(), lists: None };
static TOML: LangAssist = LangAssist {
    pairs: &[
        p('(', ')', true, true, true),
        p('[', ']', true, true, true),
        p('"', '"', true, true, false),
    ],
    lists: None,
};
static YAML: LangAssist = LangAssist {
    pairs: code_brackets!(p('\'', '\'', true, true, false)),
    lists: None,
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
};
/// 未知语言兜底：基础括号 + 引号（zed 无语言配置时不启用任何辅助；PF 的
/// code_editor 只挂在已注册 kind 上，兜底仅为未知 ts_lang 留基础体验）。
static FALLBACK: LangAssist = LangAssist { pairs: code_brackets!(), lists: None };

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

        // P1b：autoclose（空选区补闭合对）/ overtype（越过闭合符）在此接入
        false
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

        let lists = self.lang_assist().and_then(|l| l.lists);
        let (tab_size, hard_tabs) = self
            .mode
            .tab_size()
            .map(|t| (t.tab_size, t.hard_tabs))
            .unwrap_or((2, false));

        // 列表续行族（zed extend_list_on_newline；仅空选区）
        if sel.is_empty() {
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
        }

        // P1b：注释续行（//、///、块注释 * 前缀）在此接入

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
