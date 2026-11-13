//! pi-flash patch（移植）：hard tab 渲染层——zed display_map/tab_map.rs 的
//! 行内等价实现。PF 无 TabMap transforms 树（zed 因增量编辑同步建树），PF
//! 行级即时换算（渲染/点击/光标每行一两次，O(行字节) 重建足够便宜）。
//!
//! zed 语义（tab_map.rs expand_tabs/collapse_tabs）：
//! - `\t` 展开为到下一制表位（tab_size 对齐、按**字符列**计）的空格；
//! - 列 ≥ MAX_EXPANSION_COLUMN 的 tab 不再展开（渲染为 1 空格）；
//! - 双向换算：折叠列（源字节）↔ 展开列（展开文本字节）。
//!
//! shape 层（DirectWrite）对 `\t` 渲染 0 宽——含 tab 的行必须先展开再
//! shape，所有行内索引换算（position_for_index / closest_index_for_
//! position）统一走展开坐标，出入经本层映射表。

use gpui::TextRun;

/// zed MAX_EXPANSION_COLUMN（tab_map.rs:17）：超过此列的 tab 不展开。
pub(super) const MAX_EXPANSION_COLUMN: u32 = 256;

/// 一行的 tab 展开结果：展开文本 + 展开字节 → 源字节映射表。
///
/// `map[i]` = 展开文本第 **i 字节**对应的源行内字节（多字节字符与 tab 段
/// 均为多对一）；`is_tab[i]` = 该展开字节是否为 tab 展开的空格。
/// `map[text.len()]` = 源行长度（哨兵）。
pub(super) struct TabExpansion {
    pub(super) text: String,
    pub(super) map: Vec<usize>,
    pub(super) is_tab: Vec<bool>,
}

impl TabExpansion {
    /// 展开一行（zed expand_tabs 语义：制表位 = tab_size 对齐，按字符列）。
    pub(super) fn build(line: &str, tab_size: u32) -> Self {
        Self::build_with_col(line, tab_size, 0)
    }

    /// 同 build，`init_col` = 段首的行内字符列（软换行段的制表位按整行对齐）。
    pub(super) fn build_with_col(line: &str, tab_size: u32, init_col: u32) -> Self {
        let tab_size = tab_size.max(1);
        let mut text = String::with_capacity(line.len());
        let mut map = Vec::with_capacity(line.len() + 1);
        let mut is_tab = Vec::with_capacity(line.len() + 1);
        let mut col = init_col;
        let mut src_ix = 0usize;
        for c in line.chars() {
            let src_len = c.len_utf8();
            if c == '\t' && col < MAX_EXPANSION_COLUMN {
                let tab_len = tab_size - (col % tab_size);
                for _ in 0..tab_len {
                    map.push(src_ix);
                    is_tab.push(true);
                    text.push(' ');
                }
                col += tab_len;
            } else {
                for _ in 0..src_len {
                    map.push(src_ix);
                    is_tab.push(false);
                }
                text.push(c);
                col += 1;
            }
            src_ix += src_len;
        }
        map.push(src_ix);
        is_tab.push(false);
        Self { text, map, is_tab }
    }

    /// 源行内字节 → 展开文本字节（源字节落在其对应展开段起点）。
    pub(super) fn source_to_expanded(&self, src: usize) -> usize {
        // 线性扫映射表首个 ≥ src 的项（src 是源字节边界）。行长度有限，
        // 每帧每行仅一两次调用。
        self.map
            .iter()
            .position(|&s| s >= src)
            .unwrap_or(self.map.len().saturating_sub(1))
    }

    /// 展开文本字节 → 源行内字节（点击反算）。tab 段内按位置二分：前半落
    /// tab 字节（Left），后半落 tab 之后（Right）——zed collapse_tabs 的
    /// bias 语义近似。多字节字符内恒落字符起点。
    pub(super) fn expanded_to_source(&self, exp: usize) -> usize {
        let exp = exp.min(self.map.len().saturating_sub(1));
        let src = self.map[exp];
        if self.is_tab[exp] {
            // tab 段内相对位置过半 → 落 tab 之后（源 tab 字节 +1）
            let seg_start = exp
                - self.is_tab[..=exp]
                    .iter()
                    .rev()
                    .take_while(|&&t| t)
                    .count()
                + 1;
            let seg_len = self.is_tab[exp..]
                .iter()
                .take_while(|&&t| t)
                .count()
                + (exp - seg_start)
                + 1;
            if exp - seg_start >= seg_len / 2 {
                return (src + 1).min(self.src_len());
            }
        }
        src
    }

    fn src_len(&self) -> usize {
        *self.map.last().unwrap_or(&0)
    }
}

/// 把相对源行字节坐标的 runs 重排到展开坐标（shape 输入用）。
///
/// tab 段继承源 tab 字节的样式；普通段逐字节取源字节样式后合并相邻同样式
/// （O(展开字节)，每行 shape 一次）。
pub(super) fn rerun_expanded(
    line_runs: &[TextRun],
    exp: &TabExpansion,
    base_font: gpui::Font,
    base_color: gpui::Hsla,
) -> Vec<TextRun> {
    // 原 runs → 源字节样式查询（游标）
    let run_at = |src: usize| -> Option<&TextRun> {
        let mut cursor = 0usize;
        for run in line_runs {
            if src < cursor + run.len {
                return Some(run);
            }
            cursor += run.len;
        }
        None
    };

    let mut out: Vec<TextRun> = Vec::new();
    for exp_ix in 0..exp.text.len() {
        let src = exp.map[exp_ix];
        let run = run_at(src);
        match (out.last_mut(), run) {
            (Some(last), Some(r)) if last.font == r.font && last.color == r.color
                && last.background_color == r.background_color
                && last.underline == r.underline
                && last.strikethrough == r.strikethrough =>
            {
                last.len += 1;
            }
            (_, Some(r)) => out.push(TextRun {
                len: 1,
                font: r.font.clone(),
                color: r.color,
                background_color: r.background_color,
                underline: r.underline,
                strikethrough: r.strikethrough,
            }),
            (Some(last), None)
                if last.font == base_font && last.color == base_color
                    && last.background_color.is_none()
                    && last.underline.is_none()
                    && last.strikethrough.is_none() =>
            {
                last.len += 1;
            }
            (_, None) => out.push(TextRun {
                len: 1,
                font: base_font.clone(),
                color: base_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            }),
        }
    }
    out
}
