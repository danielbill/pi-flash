//! pi-flash patch（对齐 zed）：块级折叠（code folding）。
//!
//! 全量对齐 zed 无 LSP 路（PF 无 LSP，zed 无 LSP 时全语言统一走缩进折叠，
//! fold.rs:56 `is_foldable |= !use_lsp_folding_ranges && starts_indent(..)`）：
//!
//! - 折叠资格判定 = `starts_indent`（zed display_map.rs:2241 原样移植）：
//!   当前行非空白、其下第一个非空行缩进更深 → 可折叠
//! - 块范围 = 从该行缩进向下的连续更深缩进块（zed indent crease 同语义）
//! - gutter Disclosure 箭头（zed render_crease_toggle）：folded 恒显、
//!   可折叠行 hover/光标行显
//! - 折叠集 = FoldMap（zed FoldMap 对应物；PF 024 FoldSet 是行内零宽
//!   折叠，块级折叠另建，行选择层与行内字符层正交可共存）
//!
//! 显示行映射：display_row = buffer_row − 之前隐藏行数；隐藏行 =
//! range.start+1..range.end（range.end 独占 = 块后第一行）。
//! 复杂度全部 O(折叠数)（folds 有序 Vec，数量小），禁 O(行数) 每帧扫。

use gpui::Context;

use super::state::InputState;
use super::RopeExt as _;
use std::ops::Range;

/// 块级折叠集（buffer 行空间；ranges 按 start 升序、互不重叠）。
/// range = start..end：start 行可见（块首），end 独占（块后第一行），
/// 隐藏行 = start+1..end。
#[derive(Default)]
pub(crate) struct FoldMap {
    ranges: Vec<Range<usize>>,
}

impl FoldMap {
    pub(crate) fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    pub(crate) fn clear(&mut self) {
        self.ranges.clear();
    }

    /// 折叠一个行区间（end 独占；需 end > start+1 才有隐藏行）。
    /// 插入后与相邻区间合并（zed FoldMap unfold_intersecting 反向操作）。
    pub(crate) fn fold(&mut self, range: Range<usize>) {
        if range.end <= range.start + 1 {
            return;
        }
        let (mut start, mut end) = (range.start, range.end);
        // 吸收与之相交/相触的已有区间（start ≤ old.end 且 end ≥ old.start，
        // 行区间相交判定）
        let mut merged: Vec<Range<usize>> = Vec::with_capacity(self.ranges.len());
        for r in self.ranges.drain(..) {
            if r.start <= end && r.end >= start {
                start = start.min(r.start);
                end = end.max(r.end);
            } else {
                merged.push(r);
            }
        }
        merged.push(start..end);
        merged.sort_by_key(|r| r.start);
        self.ranges = merged;
    }

    /// 展开与行区间相交的所有折叠（zed unfold_ranges 的行空间对应物）。
    pub(crate) fn unfold_intersecting(&mut self, rows: Range<usize>) {
        self.ranges.retain(|r| r.end <= rows.start || r.start >= rows.end);
    }

    /// 展开以 row 为块首的折叠（gutter 箭头点击 unfold_at）。
    pub(crate) fn unfold_at(&mut self, row: usize) -> bool {
        let before = self.ranges.len();
        self.ranges.retain(|r| r.start != row);
        before != self.ranges.len()
    }

    /// row 是否为某折叠的块首。
    pub(crate) fn is_fold_start(&self, row: usize) -> bool {
        self.ranges.iter().any(|r| r.start == row)
    }

    /// row 是否被隐藏（严格处于某折叠内部，块首不算）。
    pub(crate) fn is_line_folded(&self, row: usize) -> bool {
        self.ranges.iter().any(|r| row > r.start && row < r.end)
    }

    /// 隐藏 row 的折叠（row 在内部，非块首）。
    pub(crate) fn hiding(&self, row: usize) -> Option<Range<usize>> {
        self.ranges.iter().find(|r| row > r.start && row < r.end).cloned()
    }

    /// buffer 行 → 显示行（folded 行也映射，落在其块首显示行）。
    pub(crate) fn display_row(&self, row: usize) -> usize {
        let mut hidden = 0usize;
        for r in &self.ranges {
            if r.start >= row {
                break;
            }
            hidden += (r.end.min(row) - r.start - 1).max(0);
        }
        row - hidden
    }

    /// 显示行 → buffer 行。
    pub(crate) fn buffer_row(&self, display_row: usize) -> usize {
        let mut row = display_row;
        for r in &self.ranges {
            if row <= r.start {
                break;
            }
            let hidden = r.end - r.start - 1;
            if row <= r.start + hidden {
                // 落在隐藏区（越界钳制用），收敛到块首
                return r.start;
            }
            row += hidden;
        }
        row
    }

    /// 总显示行数。
    pub(crate) fn total_display_rows(&self, total_rows: usize) -> usize {
        total_rows - self.hidden_count()
    }

    /// 隐藏行总数。
    pub(crate) fn hidden_count(&self) -> usize {
        self.ranges.iter().map(|r| r.end - r.start - 1).sum()
    }

    /// 全部隐藏行（高度统计用）。
    pub(crate) fn hidden_rows(&self) -> impl Iterator<Item = usize> + '_ {
        self.ranges
            .iter()
            .flat_map(|r| (r.start + 1)..r.end)
    }

    /// 折叠区间表（只读；宿主快照/观测用）。
    pub(crate) fn ranges(&self) -> &[Range<usize>] {
        &self.ranges
    }

    /// 可见行的 buffer 行表（显示区间 → buffer 行列表；区间须在显示
    /// 行空间内）。
    pub(crate) fn buffer_rows(&self, display_range: Range<usize>) -> Vec<usize> {
        if display_range.is_empty() {
            return Vec::new();
        }
        let mut rows = Vec::with_capacity(display_range.len());
        let mut row = self.buffer_row(display_range.start);
        for _ in display_range {
            rows.push(row);
            row = self.next_visible_row(row);
        }
        rows
    }

    /// row 之后下一个可见行（隐藏区整段跳过）。
    fn next_visible_row(&self, row: usize) -> usize {
        let mut row = row + 1;
        while let Some(r) = self.hiding(row) {
            row = r.end;
        }
        row
    }

    /// 文本编辑后的折叠集维护：与编辑行区间相交的折叠展开（zed 编辑
    /// 入折叠区即 reveal），其下折叠按行数差平移。
    pub(crate) fn edit_rows_adjust(&mut self, old_rows: Range<usize>, new_end_row: usize) {
        if self.ranges.is_empty() {
            return;
        }
        let delta = new_end_row as isize - old_rows.end as isize;
        let mut out = Vec::with_capacity(self.ranges.len());
        for r in self.ranges.drain(..) {
            if r.end > old_rows.start && r.start < old_rows.end {
                continue; // 与编辑相交 → 展开
            }
            if r.start >= old_rows.end {
                // 编辑之下 → 平移
                let shift = |v: usize| (v as isize + delta).max(0) as usize;
                let nr = shift(r.start)..shift(r.end);
                if nr.end > nr.start + 1 {
                    out.push(nr);
                }
            } else {
                // 编辑之上 → 原样（区间可能因删行变退化，钳制）
                let nr = r.start..r.end.min(old_rows.start);
                if nr.end > nr.start + 1 {
                    out.push(nr);
                } else {
                    // end 被钳回块内 → 展开该块剩余部分（start..old_start
                    // 若仍有隐藏行则保留）
                    let nr = r.start..old_rows.start;
                    if nr.end > nr.start + 1 {
                        out.push(nr);
                    }
                }
            }
        }
        out.sort_by_key(|r| r.start);
        self.ranges = out;
    }
}

impl InputState {
    fn folds_active(&self) -> bool {
        self.mode.is_code_editor()
    }

    /// 折叠区间表（buffer 行空间，start=块首可见行，end 独占）。
    /// pi-flash patch：宿主快照/自动化观测只读视图。
    pub fn fold_ranges(&self) -> Vec<Range<usize>> {
        if self.folds_active() {
            self.folds.ranges().to_vec()
        } else {
            Vec::new()
        }
    }

    /// 隐藏行总数（同上，宿主观测用）。
    pub fn fold_hidden_rows(&self) -> usize {
        if self.folds_active() {
            self.folds.hidden_count()
        } else {
            0
        }
    }

    /// 编辑器布局度量（自动化测试坐标换算用）：(bounds_origin_x,
    /// bounds_origin_y, line_number_width, line_height, visible_top)，
    /// 逻辑像素。bounds.origin 含滚动偏移。
    pub fn layout_metrics(&self) -> Option<(f32, f32, f32, f32, f32)> {
        let layout = self.last_layout.as_ref()?;
        let bounds = self.last_bounds.as_ref()?;
        Some((
            f32::from(bounds.origin.x),
            f32::from(bounds.origin.y),
            f32::from(layout.line_number_width),
            f32::from(layout.line_height),
            f32::from(layout.visible_top),
        ))
    }

    /// 行缩进宽度（zed line_indent_for_buffer_row：行首空白宽度；空白字符
    /// 按 1 计——zed IndentSize raw_len 同为字符数）。
    fn fold_indent_len(&self, row: usize) -> usize {
        self.text
            .slice_line(row)
            .bytes()
            .take_while(|b| *b == b' ' || *b == b'\t')
            .count()
    }

    fn fold_line_is_blank(&self, row: usize) -> bool {
        self.text.slice_line(row).chars().all(|c| c.is_whitespace())
    }

    /// 折叠资格判定（zed display_map.rs:2241 starts_indent 原样移植）：
    /// 当前行非空白、其下第一个非空行缩进更深 → true。
    pub(crate) fn fold_starts_indent(&self, row: usize) -> bool {
        let max_row = self.text.lines_len().saturating_sub(1);
        if !self.folds_active() || row >= max_row {
            return false;
        }
        let line_indent = self.fold_indent_len(row);
        if self.fold_line_is_blank(row) {
            return false;
        }
        for next_row in row + 1..=max_row {
            let next_indent = self.fold_indent_len(next_row);
            if next_indent > line_indent {
                return true;
            } else if !self.fold_line_is_blank(next_row) {
                return false;
            }
        }
        false
    }

    /// row 的可折叠块范围（zed indent crease 同语义：从 row 缩进向下的
    /// 连续更深缩进块，尾部空行不收）。end 独占。
    pub(crate) fn foldable_range_for_row(&self, row: usize) -> Option<Range<usize>> {
        if !self.fold_starts_indent(row) {
            return None;
        }
        let max_row = self.text.lines_len().saturating_sub(1);
        let indent = self.fold_indent_len(row);
        let mut end = row + 1;
        while end <= max_row {
            if self.fold_line_is_blank(end) {
                end += 1;
                continue;
            }
            if self.fold_indent_len(end) > indent {
                end += 1;
            } else {
                break;
            }
        }
        // 尾部空行不收（zed 块边界在最后内容行）
        while end > row + 1 && self.fold_line_is_blank(end - 1) {
            end -= 1;
        }
        Some(row..end)
    }

    /// 折叠 row 所在块（gutter 箭头 folded 态点击）。
    pub(crate) fn fold_at(&mut self, row: usize, cx: &mut Context<Self>) {
        if let Some(range) = self.foldable_range_for_row(row) {
            // 折叠块内若含光标/选区，块首保留光标（zed fold_at autoscroll
            // 对应物：光标落块首行）
            let cursor_row = self.text.offset_to_point(self.cursor()).row;
            if range.contains(&cursor_row) && cursor_row != range.start {
                self.selected_range = {
                    let start = self.text.line_start_offset(range.start);
                    (start..start).into()
                };
            }
            self.folds.fold(range);
            cx.notify();
        }
    }

    /// 展开 row 的折叠（块首箭头点击）。点击隐藏行所在块 → 展开该块
    /// （zed unfold_at：与 row 相交的折叠全展开）。
    pub(crate) fn unfold_at(&mut self, row: usize, cx: &mut Context<Self>) {
        let had = self.folds.unfold_at(row);
        if !had {
            if let Some(r) = self.folds.hiding(row) {
                self.folds.unfold_intersecting(r.start + 1..r.end);
            }
        }
        cx.notify();
    }

    /// gutter 箭头点击：folded → 展开，可折叠 → 折叠（zed Disclosure
    /// toggle_callback 语义）。
    pub(crate) fn toggle_fold_at(&mut self, row: usize, cx: &mut Context<Self>) {
        if self.folds.is_fold_start(row) || self.folds.is_line_folded(row) {
            self.unfold_at(row, cx);
        } else {
            self.fold_at(row, cx);
        }
    }

    /// 防御性 reveal：光标落在隐藏行（undo/程序化选区）→ 展开包含它的
    /// 折叠（zed selection reveal 语义）。move_to 主口调用。
    pub(crate) fn reveal_folded_cursor(&mut self, cx: &mut Context<Self>) {
        if self.folds.is_empty() || !self.folds_active() {
            return;
        }
        let row = self.text.offset_to_point(self.cursor()).row;
        if let Some(r) = self.folds.hiding(row) {
            self.folds.unfold_intersecting(r.start + 1..r.end);
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(ranges: Vec<Range<usize>>) -> FoldMap {
        FoldMap { ranges }
    }

    #[test]
    fn fold_display_mapping() {
        let m = map(vec![1..5]); // 行 1 可见，行 2-4 隐藏
        assert_eq!(m.display_row(0), 0);
        assert_eq!(m.display_row(1), 1); // 块首
        assert_eq!(m.display_row(5), 2); // 块后第一行
        assert_eq!(m.total_display_rows(10), 8);
        assert!(m.is_line_folded(3));
        assert!(!m.is_line_folded(1));
        assert!(m.is_fold_start(1));
        assert_eq!(m.buffer_row(2), 5);
        assert_eq!(m.buffer_row(0), 0);
    }

    #[test]
    fn fold_merge_and_unfold() {
        let mut m = FoldMap::default();
        m.fold(1..4);
        m.fold(2..6); // 相交 → 合并 1..6
        assert_eq!(m.total_display_rows(10), 6);
        m.unfold_at(1);
        assert!(m.is_empty());
    }

    #[test]
    fn fold_edit_adjust() {
        let mut m = map(vec![1..5, 8..12]);
        // 在行 2-3 间删 1 行（old_rows 2..3 → new_end 2）
        m.edit_rows_adjust(2..3, 2);
        // 1..5 与编辑相交（end=5 > 2 && start=1 < 3）→ 展开；
        // 8..12 平移 -1 → 7..11
        assert!(
            m.ranges.len() == 1 && m.ranges[0] == (7usize..11),
            "{:?}",
            m.ranges
        );
    }

    #[test]
    fn fold_edit_above_keeps_range() {
        let mut m = map(vec![5..9]);
        m.edit_rows_adjust(0..2, 1); // 顶部删 1 行
        assert!(m.ranges.len() == 1 && m.ranges[0] == (4usize..8));
    }
}
