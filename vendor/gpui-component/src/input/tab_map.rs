//! pi-flash patch（移植）：TabMap——zed crates/editor/src/display_map/
//! tab_map.rs 移植（2026-11-14，beads pi-flash-dqjd）。
//!
//! **zed 现行 TabMap 不是 SumTree**：是架在 FoldSnapshot 行文本上的懒换算
//! 层——快照只持有 fold 快照 + tab_size + max_expansion_column，点换算按行
//! 即时展开（expand_tabs/collapse_tabs），SumTree Transform 链由 fold/wrap
//! 两层承担（tab 不改行数、无独立树可建）。本文件逐语义移植该设计：
//!
//! - [`TabSnapshot`]：fold 快照 + tab_size；`sync` 把 fold 编辑透传给下游
//!   （TabEdit = FoldEdit，fold 空间字节——tab 层无树，编辑不变形）。
//! - 点换算 [`TabSnapshot::fold_point_to_tab_point`] /
//!   [`TabSnapshot::tab_point_to_fold_point`]：zed 参考实现语义（tab_map.rs
//!   tests `expected_expand_tabs`/`expected_collapse_tabs`——展开位按
//!   **expanded char col** 对制表位、守卫按 **collapsed byte < 列上限**、
//!   tab→fold 带 bias：Left 贴 tab 字节、Right 越过 tab）。随机对拍测试
//!   随文件底部一并移植。
//! - [`TabExpansion`]：渲染层行展开（展开文本 + 展开字节→fold 字节映射 +
//!   is_tab 位图），shape 前展开、runs 重排、光标/命中行内换算共用；展开
//!   数学与点换算同一套（守卫 collapsed byte < MAX_EXPANSION_COLUMN）。
//!
//! zed 语义要点（expand/collapse 参考实现逐行对照）：
//! - tab 展开宽 = `tab_size - (展开字符列 % tab_size)`（制表位按字符列）；
//! - 行内 collapsed 字节 ≥ max_expansion_column(256) 的 tab 不展开
//!   （渲染 1 空格、换算按 1 原始字节）；
//! - fold 点落在 tab 字节上 → 展开坐标取该 tab 展开段**起点**（守卫在
//!   处理该字符前 break）。

use std::num::NonZeroU32;
use std::ops::Range;

use gpui::TextRun;
use sum_tree::Bias;

use super::fold_map::{
    FoldEdit, FoldOffset, FoldPoint, FoldSnapshot, InlayPoint, Point as FoldSpacePoint,
};

/// zed MAX_EXPANSION_COLUMN（tab_map.rs:11）：行内 collapsed 字节超过此
/// 限的 tab 不再展开（渲染 1 空格）。
pub(crate) const MAX_EXPANSION_COLUMN: u32 = 256;

/// zed MAX_TABS（tab_map.rs:15）：tab 宽上限（SPACES 位图容量，本实现按
/// 字符迭代无位图，保留同值语义防 tab_size 溢出）。
const MAX_TAB_SIZE: u32 = 128;

/// zed TabEdit（tab_map.rs:551）：tab 空间点编辑——fold 编辑经本层映射
/// （old 端点在旧 fold 快照、new 端点在新 fold 快照上换算，zed
/// tab_map.rs:179-188 同构），下游 wrap 层按**行区间**消费。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TabEdit {
    pub old: Range<TabPoint>,
    pub new: Range<TabPoint>,
}

/// zed TabPoint（tab_map.rs:524）：行号 + 行内**展开字节列**。
#[derive(Copy, Clone, Debug, Default, Eq, Ord, PartialOrd, PartialEq)]
pub(crate) struct TabPoint(pub FoldSpacePoint);

impl TabPoint {
    pub fn new(row: u32, column: u32) -> Self {
        Self(FoldSpacePoint::new(row, column))
    }

    pub fn row(self) -> u32 {
        self.0.row
    }

    pub fn column(self) -> u32 {
        self.0.column
    }
}

/// zed TabMap(TabSnapshot)。
#[derive(Clone)]
pub(crate) struct TabMap(pub TabSnapshot);

impl TabMap {
    pub fn new(fold_snapshot: FoldSnapshot, tab_size: NonZeroU32) -> Self {
        Self(TabSnapshot::new(fold_snapshot, tab_size))
    }

    /// zed TabMap::sync（tab_map.rs:41）：fold 快照前进 + 编辑映射。
    /// tab_size 变化 → 整文编辑（zed 同款，下游全量 rewrap）；否则把
    /// fold 字节编辑的端点换算成 tab 点（zed 原文还有「编辑扩到行内后续
    /// tab」的区间加宽——那是给下游 SumTree 增量划界用的优化，PF wrap 层
    /// 按行消费编辑，行粒度已足够）。
    pub fn sync(
        &mut self,
        fold_snapshot: FoldSnapshot,
        fold_edits: Vec<FoldEdit>,
        tab_size: NonZeroU32,
    ) -> (TabSnapshot, Vec<TabEdit>) {
        let tab_size = tab_size.get().min(MAX_TAB_SIZE);
        let changed = self.0.fold_snapshot.version != fold_snapshot.version
            || self.0.tab_size.get() != tab_size;

        if changed && self.0.tab_size.get() != tab_size {
            // tab_size 变化：整文编辑（zed tab_map.rs:49-61）
            let old_max = self.0.max_point();
            let new_snapshot = TabSnapshot {
                fold_snapshot,
                tab_size: NonZeroU32::new(tab_size).expect("tab size > 0"),
                max_expansion_column: self.0.max_expansion_column,
                version: self.0.version + 1,
            };
            let new_max = new_snapshot.max_point();
            self.0 = new_snapshot.clone();
            return (
                new_snapshot,
                vec![TabEdit {
                    old: TabPoint::new(0, 0)..old_max,
                    new: TabPoint::new(0, 0)..new_max,
                }],
            );
        }

        let old_max_expansion_column = self.0.max_expansion_column;
        let old_version = self.0.version;
        let old_snapshot = std::mem::replace(
            &mut self.0,
            TabSnapshot {
                fold_snapshot: fold_snapshot.clone(),
                tab_size: NonZeroU32::new(tab_size).expect("tab size > 0"),
                max_expansion_column: old_max_expansion_column,
                version: if changed {
                    old_version + 1
                } else {
                    old_version
                },
            },
        );
        if fold_edits.is_empty() {
            return (self.0.clone(), Vec::new());
        }

        // fold 字节编辑 → tab 点编辑（zed tab_map.rs:176-190）
        let tab_edits = fold_edits
            .into_iter()
            .map(|fold_edit| {
                let old_start = fold_edit.old.start.to_point(&old_snapshot.fold_snapshot);
                let old_end = fold_edit.old.end.to_point(&old_snapshot.fold_snapshot);
                let new_start = fold_edit.new.start.to_point(&self.0.fold_snapshot);
                let new_end = fold_edit.new.end.to_point(&self.0.fold_snapshot);
                TabEdit {
                    old: old_snapshot.fold_point_to_tab_point(old_start)
                        ..old_snapshot.fold_point_to_tab_point(old_end),
                    new: self.0.fold_point_to_tab_point(new_start)
                        ..self.0.fold_point_to_tab_point(new_end),
                }
            })
            .collect();
        (self.0.clone(), tab_edits)
    }
}

/// zed TabSnapshot：fold 快照 + tab 展开参数。
#[derive(Clone)]
pub(crate) struct TabSnapshot {
    pub fold_snapshot: FoldSnapshot,
    pub tab_size: NonZeroU32,
    /// The maximum column up to which a tab can expand.
    /// Any tab after this column will not expand.
    pub max_expansion_column: u32,
    pub version: usize,
}

impl std::ops::Deref for TabSnapshot {
    type Target = FoldSnapshot;

    fn deref(&self) -> &Self::Target {
        &self.fold_snapshot
    }
}

impl TabSnapshot {
    pub fn new(fold_snapshot: FoldSnapshot, tab_size: NonZeroU32) -> Self {
        Self {
            fold_snapshot,
            tab_size: NonZeroU32::new(tab_size.get().min(MAX_TAB_SIZE))
                .expect("tab size > 0"),
            max_expansion_column: MAX_EXPANSION_COLUMN,
            version: 0,
        }
    }

    /// 行展开参数（渲染层与点换算共用同一套数学的入口）。
    fn params(&self) -> (u32, u32) {
        (self.tab_size.get(), self.max_expansion_column)
    }

    /// zed TabSnapshot::line_len：该行展开后字节长（不含换行）。末行走
    /// max_point 列（zed 同款分支）。
    pub fn line_len(&self, row: u32) -> u32 {
        let max_point = self.max_point();
        if row < max_point.row() {
            let (tab_size, max_exp) = self.params();
            expand_tabs(
                &self.fold_snapshot.row_text(row),
                self.fold_snapshot.line_len(row),
                tab_size,
                max_exp,
            )
        } else {
            max_point.column()
        }
    }

    /// zed TabSnapshot::max_point：行数 = fold 行数（tab 不增行），末行列
    /// = 末行展开长。
    pub fn max_point(&self) -> TabPoint {
        let fold_max = self.fold_snapshot.max_point();
        let (tab_size, max_exp) = self.params();
        let expanded = expand_tabs(
            &self.fold_snapshot.row_text(fold_max.0.row),
            fold_max.0.column,
            tab_size,
            max_exp,
        );
        TabPoint::new(fold_max.0.row, expanded)
    }

    /// zed TabSnapshot::clip_point：先塌回 fold 空间钳制（光标不入折叠/
    /// 行尾钳制），再展开回 tab 空间。
    pub fn clip_point(&self, point: TabPoint, bias: Bias) -> TabPoint {
        let (fold_point, _, _) = self.tab_point_to_fold_point(point, bias);
        let clipped = self.fold_snapshot.clip_point(fold_point, bias);
        self.fold_point_to_tab_point(clipped)
    }

    /// zed TabSnapshot::fold_point_to_tab_point：fold（字节列）→ tab（展开
    /// 字节列）。无 bias——fold 点落在 tab 字节上取展开段起点（参考实现
    /// 守卫在处理该字符前 break 的自然结果）。
    pub fn fold_point_to_tab_point(&self, input: FoldPoint) -> TabPoint {
        let (tab_size, max_exp) = self.params();
        let expanded = expand_tabs(
            &self.fold_snapshot.row_text(input.row()),
            input.column(),
            tab_size,
            max_exp,
        );
        TabPoint::new(input.row(), expanded)
    }

    /// zed TabSnapshot::tab_point_to_fold_point：tab（展开字节列）→
    /// fold（字节列），返回 (fold 点, 展开字符列, 距下一制表位余量)。
    /// 后两项供 zed TabChunks 对齐（PF 渲染层经 TabExpansion 消费，保留
    /// 返回形状对齐 zed 签名）。
    pub fn tab_point_to_fold_point(&self, output: TabPoint, bias: Bias) -> (FoldPoint, u32, u32) {
        let (tab_size, max_exp) = self.params();
        let (collapsed, expanded_chars, to_next_stop) = collapse_tabs(
            &self.fold_snapshot.row_text(output.row()),
            output.column(),
            bias,
            tab_size,
            max_exp,
        );
        (FoldPoint::new(output.row(), collapsed), expanded_chars, to_next_stop)
    }

    /// zed TabSnapshot::point_to_tab_point：buffer 点 → fold 点 → tab 点。
    pub fn point_to_tab_point(&self, point: FoldSpacePoint, bias: Bias) -> TabPoint {
        let fold_point = self.fold_snapshot.to_fold_point(InlayPoint(point), bias);
        self.fold_point_to_tab_point(fold_point)
    }

    /// zed TabSnapshot::tab_point_to_point：tab 点 → fold 点 → buffer 点。
    pub fn tab_point_to_point(&self, point: TabPoint, bias: Bias) -> FoldSpacePoint {
        let (fold_point, _, _) = self.tab_point_to_fold_point(point, bias);
        self.fold_snapshot.fold_point_to_buffer_point(fold_point)
    }

    // ---- 渲染层（PF 消费辅助：zed 由 TabChunks 管线提供，PF 按行直取） ----

    /// 展开一行（fold 空间行文本 → 展开文本 + 字节映射）。渲染/行内换算
    /// 统一入口；数学与点换算同源（见模块注释）。
    pub fn expand_row(&self, row: u32) -> TabExpansion {
        let (tab_size, max_exp) = self.params();
        TabExpansion::build(&self.fold_snapshot.row_text(row), tab_size, max_exp)
    }
}

// ---------- 展开/塌缩数学（zed 参考实现 expected_expand_tabs/expected_collapse_tabs 移植） ----------

/// zed expand_tabs：fold 字节列 → 展开字节列。守卫 = collapsed 字节
/// `< min(列, 上限)`——fold 点落在 tab 字节上取展开段起点。
pub(crate) fn expand_tabs(line: &str, column: u32, tab_size: u32, max_exp: u32) -> u32 {
    let end_column = column.min(max_exp);
    let mut expanded_chars = 0u32;
    let mut expanded_bytes = 0u32;
    let mut collapsed_bytes = 0u32;
    for c in line.chars() {
        if collapsed_bytes >= end_column {
            break;
        }
        if c == '\t' {
            let tab_len = tab_size - expanded_chars % tab_size;
            expanded_bytes += tab_len;
            expanded_chars += tab_len;
        } else {
            expanded_bytes += c.len_utf8() as u32;
            expanded_chars += 1;
        }
        collapsed_bytes += c.len_utf8() as u32;
    }
    expanded_bytes + column.saturating_sub(collapsed_bytes)
}

/// zed collapse_tabs：展开字节列 → (fold 字节列, 展开字符列, 距下一制表位)。
/// tab 段内 Left 贴 tab 字节、Right 越过 tab；collapsed 字节 ≥ 上限后
/// 按原始字节直算（tab 不展开，1 字节）。
pub(crate) fn collapse_tabs(
    line: &str,
    column: u32,
    bias: Bias,
    tab_size: u32,
    max_exp: u32,
) -> (u32, u32, u32) {
    let mut expanded_bytes = 0u32;
    let mut expanded_chars = 0u32;
    let mut collapsed_bytes = 0u32;
    for c in line.chars() {
        if expanded_bytes >= column {
            break;
        }
        if collapsed_bytes >= max_exp {
            break;
        }

        if c == '\t' {
            let tab_len = tab_size - expanded_chars % tab_size;
            expanded_chars += tab_len;
            expanded_bytes += tab_len;
            if expanded_bytes > column {
                expanded_chars -= expanded_bytes - column;
                return match bias {
                    Bias::Left => (collapsed_bytes, expanded_chars, expanded_bytes - column),
                    Bias::Right => (collapsed_bytes + 1, expanded_chars, 0),
                };
            }
        } else {
            expanded_chars += 1;
            expanded_bytes += c.len_utf8() as u32;
        }

        if expanded_bytes > column && matches!(bias, Bias::Left) {
            expanded_chars -= 1;
            break;
        }

        collapsed_bytes += c.len_utf8() as u32;
    }

    (
        collapsed_bytes + column.saturating_sub(expanded_bytes),
        expanded_chars,
        0,
    )
}

// ---------- 渲染层行展开（PF：shape 前展开 + 行内换算映射表） ----------

/// 一行的 tab 展开结果：展开文本 + 展开字节 → fold 字节映射表。
///
/// `map[i]` = 展开文本第 **i 字节**对应的行内源字节（多字节字符与 tab 段
/// 均为多对一）；`is_tab[i]` = 该展开字节是否为 tab 展开的空格。
/// `map[text.len()]` = 源行长度（哨兵）。
///
/// 展开守卫与点换算同源：tab 的 **collapsed 字节 < max_exp** 才展开到
/// 制表位，否则渲染 1 空格（zed TabChunks `input_column < max_expansion_
/// column` 分支同语义）。
pub(crate) struct TabExpansion {
    pub(crate) text: String,
    pub(crate) map: Vec<usize>,
    pub(crate) is_tab: Vec<bool>,
}

impl TabExpansion {
    /// 展开一行（制表位按展开字符列对齐）。
    pub(crate) fn build(line: &str, tab_size: u32, max_exp: u32) -> Self {
        let tab_size = tab_size.max(1);
        let mut text = String::with_capacity(line.len());
        let mut map = Vec::with_capacity(line.len() + 1);
        let mut is_tab = Vec::with_capacity(line.len() + 1);
        let mut col = 0u32; // 展开字符列
        let mut collapsed = 0usize; // 行内源字节
        for c in line.chars() {
            let src_len = c.len_utf8();
            if c == '\t' {
                // 守卫按 collapsed 字节（zed TabChunks input_column 同式）
                let tab_len = if (collapsed as u32) < max_exp {
                    tab_size - col % tab_size
                } else {
                    1
                };
                for _ in 0..tab_len {
                    map.push(collapsed);
                    is_tab.push(true);
                    text.push(' ');
                }
                col += tab_len;
            } else {
                for _ in 0..src_len {
                    map.push(collapsed);
                    is_tab.push(false);
                }
                text.push(c);
                col += 1;
            }
            collapsed += src_len;
        }
        map.push(collapsed);
        is_tab.push(false);
        Self { text, map, is_tab }
    }

    /// 同 build，`init_col` = 段首的行内字符列（软换行段的制表位按整行
    /// 对齐）。**legacy**：S2 显示行收口后由 wrap 链的缩进变换取代（守卫
    /// 仍是字符列——段首前序字节数在切片上不可得，与 zed byte 守卫的偏差
    /// 仅影响 256 列外的 CJK 行，S2 一并消灭）。
    pub(crate) fn build_with_col(line: &str, tab_size: u32, init_col: u32) -> Self {
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

    /// 行内源字节 → 展开文本字节（源字节落在其对应展开段起点）。
    pub(crate) fn source_to_expanded(&self, src: usize) -> usize {
        // 线性扫映射表首个 ≥ src 的项（src 是源字节边界）。行长度有限，
        // 每帧每行仅一两次调用。
        self.map
            .iter()
            .position(|&s| s >= src)
            .unwrap_or(self.map.len().saturating_sub(1))
    }

    /// 展开文本字节 → 行内源字节（点击反算）。tab 段内按位置二分：前半落
    /// tab 字节（Left），后半落 tab 之后（Right）——zed collapse_tabs 的
    /// bias 语义近似。多字节字符内恒落字符起点。
    pub(crate) fn expanded_to_source(&self, exp: usize) -> usize {
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

/// 把相对行源字节坐标的 runs 重排到展开坐标（shape 输入用）。
///
/// tab 段继承源 tab 字节的样式；普通段逐字节取源字节样式后合并相邻同样式
/// （O(展开字节)，每行 shape 一次）。
pub(crate) fn rerun_expanded(
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

// ---------- 测试（zed tab_map.rs tests 参考实现对照移植） ----------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::fold_map::{FoldMap, InlaySnapshot};

    fn fold_snapshot(text: &str) -> FoldSnapshot {
        let (_, snapshot) = FoldMap::new(InlaySnapshot {
            buffer: ropey::Rope::from(text),
            version: 0,
        });
        snapshot
    }

    /// zed TabSnapshot 参考实现（tests expected_to_tab_point）原样对照。
    fn expected_expand(
        line: &str,
        column: u32,
        tab_size: u32,
        max_exp: u32,
    ) -> u32 {
        let mut expanded_chars = 0u32;
        let mut expanded_bytes = 0u32;
        let mut collapsed_bytes = 0u32;
        let end_column = column.min(max_exp);
        for c in line.chars() {
            if collapsed_bytes >= end_column {
                break;
            }
            if c == '\t' {
                let tab_len = tab_size - expanded_chars % tab_size;
                expanded_bytes += tab_len;
                expanded_chars += tab_len;
            } else {
                expanded_bytes += c.len_utf8() as u32;
                expanded_chars += 1;
            }
            collapsed_bytes += c.len_utf8() as u32;
        }
        expanded_bytes + column.saturating_sub(collapsed_bytes)
    }

    /// zed expected_collapse_tabs 原样对照（含 bias 分支）。
    fn expected_collapse(
        line: &str,
        column: u32,
        bias: Bias,
        tab_size: u32,
        max_exp: u32,
    ) -> (u32, u32, u32) {
        let mut expanded_bytes = 0u32;
        let mut expanded_chars = 0u32;
        let mut collapsed_bytes = 0u32;
        for c in line.chars() {
            if expanded_bytes >= column {
                break;
            }
            if collapsed_bytes >= max_exp {
                break;
            }

            if c == '\t' {
                let tab_len = tab_size - expanded_chars % tab_size;
                expanded_chars += tab_len;
                expanded_bytes += tab_len;
                if expanded_bytes > column {
                    expanded_chars -= expanded_bytes - column;
                    return match bias {
                        Bias::Left => (collapsed_bytes, expanded_chars, expanded_bytes - column),
                        Bias::Right => (collapsed_bytes + 1, expanded_chars, 0),
                    };
                }
            } else {
                expanded_chars += 1;
                expanded_bytes += c.len_utf8() as u32;
            }

            if expanded_bytes > column && matches!(bias, Bias::Left) {
                expanded_chars -= 1;
                break;
            }

            collapsed_bytes += c.len_utf8() as u32;
        }

        (
            collapsed_bytes + column.saturating_sub(expanded_bytes),
            expanded_chars,
            0,
        )
    }

    /// zed test_expand_tabs：单行逐字符位置 expand 对拍参考实现。
    #[test]
    fn test_expand_tabs_matches_reference() {
        let input = "A\tBC\tDEF\tG\tHI\tJ\tK\tL\tM";
        for tab_size in [1u32, 2, 4, 8] {
            let snapshot = TabSnapshot::new(
                fold_snapshot(input),
                NonZeroU32::new(tab_size).unwrap(),
            );
            for (ix, _) in input.char_indices() {
                let fold_point = FoldPoint::new(0, ix as u32);
                let actual = snapshot.fold_point_to_tab_point(fold_point).column();
                let expected = expected_expand(input, ix as u32, tab_size, MAX_EXPANSION_COLUMN);
                assert_eq!(actual, expected, "tab_size {tab_size} col {ix}");
            }
        }
    }

    /// zed test_collapse_tabs：单行逐字符位置 collapse 对拍（双 bias）。
    #[test]
    fn test_collapse_tabs_matches_reference() {
        let input = "A\tBC\tDEF\tG\tHI\tJ\tK\tL\tM";
        for tab_size in [1u32, 2, 4, 8] {
            let snapshot = TabSnapshot::new(
                fold_snapshot(input),
                NonZeroU32::new(tab_size).unwrap(),
            );
            for (ix, _) in input.char_indices() {
                for bias in [Bias::Left, Bias::Right] {
                    let actual =
                        snapshot.tab_point_to_fold_point(TabPoint::new(0, ix as u32), bias);
                    let expected = expected_collapse(
                        input,
                        ix as u32,
                        bias,
                        tab_size,
                        MAX_EXPANSION_COLUMN,
                    );
                    assert_eq!(actual.0 .0.column, expected.0, "tab_size {tab_size} col {ix} bias {bias:?}");
                    assert_eq!(actual.1, expected.1, "expanded chars tab_size {tab_size} col {ix} bias {bias:?}");
                    assert_eq!(actual.2, expected.2, "to_next_stop tab_size {tab_size} col {ix} bias {bias:?}");
                }
            }
        }
    }

    /// zed test_collapse_tabs_random（收窄迭代数）：随机串 expand/collapse
    /// 对拍参考实现。
    #[test]
    fn test_tabs_random_matches_reference() {
        // 固定种子伪随机（无 rand 依赖：简单 LCG）
        let mut seed = 0x2545F4914F6CDD1Du64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _case in 0..40 {
            let len = (next() % 300) as usize;
            let tab_size = (next() % 4 + 1) as u32;
            let mut input = String::with_capacity(len);
            for _ in 0..len {
                if next() % 100 < 12 {
                    input.push('\t');
                } else {
                    // 可打印 ASCII（排除 tab/\n/\r）
                    let mut b = (next() % 95 + 32) as u8;
                    while b == b'\t' || b == b'\n' || b == b'\r' {
                        b = (next() % 95 + 32) as u8;
                    }
                    input.push(b as char);
                }
            }
            // 混入多字节字符（zed RandomCharIter 覆盖面）
            if len > 10 {
                let at = (next() as usize) % (len - 5).max(1);
                if input.is_char_boundary(at) && input[at..].chars().next().is_some() {
                    input.replace_range(at..at + 1, "中");
                }
            }

            let max_exp = (next() % 400) as u32;
            for (ix, _) in input.char_indices() {
                // 自由函数对拍参考实现（随机 max_exp 覆盖守卫分支）
                assert_eq!(expand_tabs(&input, ix as u32, tab_size, max_exp), expected_expand(&input, ix as u32, tab_size, max_exp), "expand input {:?} ix {ix}", input);
                for bias in [Bias::Left, Bias::Right] {
                    let (got_col, got_chars, got_stop) =
                        collapse_tabs(&input, ix as u32, bias, tab_size, max_exp);
                    let (exp_col, exp_chars, exp_stop) =
                        expected_collapse(&input, ix as u32, bias, tab_size, max_exp);
                    assert_eq!(got_col, exp_col, "collapse input {:?} ix {ix} bias {bias:?}", input);
                    assert_eq!(got_chars, exp_chars);
                    assert_eq!(got_stop, exp_stop);
                }
            }
            // 展开文本与映射表自洽：map 反查覆盖每个源字节边界
            let exp = TabExpansion::build(&input, tab_size, max_exp);
            for (ix, _) in input.char_indices() {
                let e = exp.source_to_expanded(ix);
                assert!(e <= exp.text.len());
            }
        }
    }

    /// zed test_long_lines：超过 max_expansion_column 的 tab 渲染 1 空格，
    /// 展开文本与 zed 期望一致。
    #[test]
    fn test_long_lines() {
        let max_expansion_column = 12;
        let input = "A\tBC\tDEF\tG\tHI\tJ\tK\tL\tM";
        let mut output = String::new();
        // 参考展开：collapsed 字节 < 12 的 tab 展开制表位，之后 1 空格
        let tab_size = 4u32;
        let mut expanded_chars = 0u32;
        let mut collapsed = 0u32;
        for c in input.chars() {
            if c == '\t' {
                let tab_len = if collapsed < max_expansion_column {
                    tab_size - expanded_chars % tab_size
                } else {
                    1
                };
                for _ in 0..tab_len {
                    output.push(' ');
                }
                expanded_chars += tab_len;
            } else {
                output.push(c);
                expanded_chars += 1;
            }
            collapsed += 1;
        }
        let exp = TabExpansion::build(input, tab_size, max_expansion_column);
        assert_eq!(exp.text, output);
    }

    /// line_len/max_point 与展开文本自洽（多行 + 折叠占位符行）。
    #[test]
    fn test_line_len_and_max_point() {
        let text = "fn main() {\n\tlet a\tx = 1;\n\tprintln!(\"{}\", a);\n}\n";
        let snapshot = TabSnapshot::new(fold_snapshot(text), NonZeroU32::new(4).unwrap());
        for (row, line) in text.split('\n').enumerate() {
            if row == 4 {
                break; // 末尾空行（text 以 \n 结尾 → 第 4 行为空行）
            }
            let exp = snapshot.expand_row(row as u32);
            assert_eq!(snapshot.line_len(row as u32) as usize, exp.text.len());
            assert_eq!(exp.text, TabExpansion::build(line, 4, MAX_EXPANSION_COLUMN).text);
        }
        let max = snapshot.max_point();
        assert_eq!(max.row(), 4); // 文本以 \n 结尾 → 5 行（末行空）
        assert_eq!(max.column(), 0);
    }
}
