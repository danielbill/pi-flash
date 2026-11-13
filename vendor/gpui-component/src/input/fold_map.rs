//! pi-flash patch（移植）：块级折叠映射——**zed crates/editor/src/display_map/
//! fold_map.rs 全量移植**（2026-11-12，beads pi-flash-dv53；算法/结构/命名
//! 逐行保留）。
//!
//! 类型替身（PF 单 buffer 对 zed 多缓冲基建的最小映射，其余逻辑不变）：
//! - `MultiBufferSnapshot` → `Rope`（gpui-component input 的 ropey Rope）
//! - `Anchor` → 字节 offset（PF 编辑时经 `shift_fold_ranges_for_edit` 显式
//!   平移，等价 zed anchor 随编辑移动的语义）
//! - `InlaySnapshot/InlayPoint/InlayOffset` → 恒等包装层（PF 无 inlay 层，
//!   inlay 坐标 == buffer 坐标）
//! - `MBTextSummary` → `TextSummary`（本文件实现，从 Rope 切片测量）
//! - `language::Point` → 本文件 `Point`（zed point.rs 算术 impl 移植）
//! - `language::Edit<T>` → `BufEdit`
//!
//! 去除项（PF 不消费）：Chunk/ChunkRenderer/宽度测量（update_fold_widths）、
//! FoldChunks 管线（element 用 `text_for_range`/`row_text` 直取）、
//! FoldPlaceholder.render 元素（PF 占位符恒为 "⋯" 文本）、zed tests
//! （依赖 MultiBuffer/InlayMap 基建）。
//!
//! 消费入口（InputState）：`FoldMap::new` / `read`（buffer 编辑 sync）/
//! writer `fold`/`unfold_intersecting` / snapshot `is_line_folded`/
//! `to_fold_point`/`FoldPoint::to_inlay_point`（fold↔buffer 双向）/
//! `clip_point`（光标不入折叠）/ `intersects_fold` / `fold_count`。
//! crease 构建（zed display_map.rs:2241/2290 移植）：`starts_indent`/
//! `closing_bracket_indent_len`/`fold_crease_for_row`。

use std::{
    cmp::{self, Ordering},
    iter,
    ops::{Add, AddAssign, Deref, DerefMut, Range, Sub, SubAssign},
    usize,
};

use gpui::SharedString;
use ropey::Rope;
use sum_tree::{Bias, Cursor, Dimensions, FilterCursor, SumTree, Summary};

use super::rope_ext::RopeExt as _;
use super::state::InputState;
use super::mode::InputMode;

// ---------- Point（zed crates/language/src/point.rs 移植） ----------

/// zed language::Point 对应物（row/column 均 u32，column 按字节计）。
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct Point {
    pub row: u32,
    pub column: u32,
}

impl Point {
    pub fn new(row: u32, column: u32) -> Self {
        Self { row, column }
    }
}

impl Add for Point {
    type Output = Point;

    fn add(self, other: Point) -> Point {
        if other.row == 0 {
            Point::new(self.row, self.column + other.column)
        } else {
            Point::new(self.row + other.row, other.column)
        }
    }
}

impl AddAssign for Point {
    fn add_assign(&mut self, other: Point) {
        *self = *self + other;
    }
}

impl Sub for Point {
    type Output = Point;

    fn sub(self, other: Point) -> Point {
        if self.row == other.row {
            Point::new(0, self.column - other.column)
        } else {
            Point::new(self.row - other.row, self.column)
        }
    }
}

impl SubAssign for Point {
    fn sub_assign(&mut self, other: Point) {
        *self = *self - other;
    }
}

// ---------- TextSummary（zed MBTextSummary 对应物） ----------

/// zed MBTextSummary 对应物：字节长度 + 行摘要（lines = 区间终点 Point）。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct TextSummary {
    pub len: usize,
    pub lines: Point,
    pub first_line_chars: u32,
    pub last_line_chars: u32,
    pub longest_row: u32,
}

impl TextSummary {
    pub fn from_str(text: &str) -> Self {
        let mut summary = Self {
            len: text.len(),
            ..Default::default()
        };
        // 行摘要：split('\n') 逐行；末段（无尾随换行时）为末行
        let mut longest_row_len = 0u32;
        let mut longest_row = 0u32;
        let mut row = 0u32;
        let mut first = true;
        let mut last_line = "";
        for line in text.split('\n') {
            let line_chars = line.chars().count() as u32;
            if first {
                summary.first_line_chars = line_chars;
                first = false;
            }
            if line_chars > longest_row_len {
                longest_row_len = line_chars;
                longest_row = row;
            }
            last_line = line;
            row += 1;
        }
        summary.longest_row = longest_row;
        if text.is_empty() {
            summary.lines = Point::new(0, 0);
            summary.last_line_chars = 0;
            summary.first_line_chars = 0;
        } else if text.ends_with('\n') {
            // 末段是空串（换行后的空行）
            summary.lines = Point::new(row - 1, 0);
            summary.last_line_chars = 0;
        } else {
            summary.lines = Point::new(row - 1, last_line.len() as u32);
            summary.last_line_chars = last_line.chars().count() as u32;
        }
        summary
    }

    /// 从 Rope 字节区间测量（sync 只对被编辑/被折叠小区间调用）。
    fn measure(buffer: &Rope, range: Range<usize>) -> Self {
        let text = text_slice(buffer, range);
        Self::from_str(&text)
    }
}

impl Add for TextSummary {
    type Output = TextSummary;

    /// zed MBTextSummary Add 语义（行拼接：other 首行并入 self 末行）。
    fn add(self, other: TextSummary) -> TextSummary {
        if other.lines.row == 0 {
            TextSummary {
                len: self.len + other.len,
                lines: self.lines + other.lines,
                first_line_chars: self.first_line_chars,
                last_line_chars: if self.lines.row == 0 {
                    self.last_line_chars + other.last_line_chars
                } else {
                    self.last_line_chars
                },
                longest_row: cmp::max(self.longest_row, other.longest_row),
            }
        } else {
            TextSummary {
                len: self.len + other.len,
                lines: self.lines + other.lines,
                first_line_chars: self.first_line_chars,
                last_line_chars: other.last_line_chars,
                longest_row: cmp::max(self.longest_row, other.longest_row + self.lines.row),
            }
        }
    }
}

impl AddAssign for TextSummary {
    fn add_assign(&mut self, other: TextSummary) {
        *self = self.clone() + other;
    }
}

// ---------- Rope 字节区间辅助 ----------

fn clamp_range(buffer: &Rope, mut range: Range<usize>) -> Range<usize> {
    let max = buffer.len();
    range.start = range.start.min(max);
    range.end = range.end.min(max).max(range.start);
    // 钳到字符边界（ropey 切片要求）
    range.start = buffer.clip_offset(range.start, Bias::Right);
    range.end = buffer.clip_offset(range.end, Bias::Left);
    range
}

/// Rope 字节区间文本（ropey 无直接字节切片，按 clip 后区间走字符迭代；
/// 仅 sync 被编辑/被折叠小区间与 FoldMap::new 整文一次性调用）。
fn text_slice(buffer: &Rope, range: Range<usize>) -> String {
    let range = clamp_range(buffer, range);
    if range.start >= range.end {
        return String::new();
    }
    buffer.slice(range.start..range.end).to_string()
}

// ---------- 恒等 inlay 层（PF 无 inlay，坐标 == buffer 坐标） ----------

/// zed InlaySnapshot 对应物：恒等变换的 buffer 视图 + 版本号。
#[derive(Clone)]
pub struct InlaySnapshot {
    pub buffer: Rope,
    pub version: usize,
}

#[derive(Copy, Clone, Debug, Default, Eq, Ord, PartialOrd, PartialEq)]
pub(crate) struct InlayPoint(pub Point);

impl InlayPoint {
    pub fn new(row: u32, column: u32) -> Self {
        Self(Point::new(row, column))
    }

    pub fn row(self) -> u32 {
        self.0.row
    }

    pub fn column(self) -> u32 {
        self.0.column
    }
}

// zed inlay_map.rs:172/180 原文
impl Add for InlayPoint {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl Sub for InlayPoint {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, Ord, PartialOrd, PartialEq)]
pub(crate) struct InlayOffset(pub usize);

impl Add for InlayOffset {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl Sub for InlayOffset {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

impl SubAssign for InlayOffset {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl AddAssign for InlayOffset {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl InlaySnapshot {
    fn len(&self) -> InlayOffset {
        InlayOffset(self.buffer.len())
    }

    fn max_point(&self) -> InlayPoint {
        let row = self.buffer.lines_len().saturating_sub(1) as u32;
        InlayPoint::new(row, self.buffer.line_len(row as usize) as u32)
    }

    fn to_point(&self, offset: InlayOffset) -> InlayPoint {
        let p = self.buffer.offset_to_point(offset.0);
        InlayPoint(Point::new(p.row as u32, p.column as u32))
    }

    fn to_offset(&self, point: InlayPoint) -> InlayOffset {
        InlayOffset(
            self.buffer
                .point_to_offset(tree_sitter::Point::new(
                point.0.row as usize,
                point.0.column as usize,
            )),
        )
    }

    fn clip_point(&self, point: InlayPoint) -> InlayPoint {
        let max_row = self.buffer.lines_len().saturating_sub(1) as u32;
        let row = point.0.row.min(max_row);
        let col = point.0.column.min(self.buffer.line_len(row as usize) as u32);
        InlayPoint::new(row, col)
    }

    fn text_summary(&self) -> TextSummary {
        TextSummary::measure(&self.buffer, 0..self.buffer.len())
    }

    fn text_summary_for_range(&self, range: Range<InlayOffset>) -> TextSummary {
        TextSummary::measure(&self.buffer, range.start.0..range.end.0)
    }

    // 恒等：inlay ↔ buffer 无变换
    fn to_inlay_point(&self, point: Point) -> InlayPoint {
        InlayPoint(point)
    }
    fn to_buffer_point(&self, point: InlayPoint) -> Point {
        point.0
    }
    fn to_inlay_offset(&self, offset: usize) -> InlayOffset {
        InlayOffset(offset)
    }
    fn to_buffer_offset(&self, offset: InlayOffset) -> usize {
        offset.0
    }
}

// ---------- BufEdit（zed language::Edit<T> 对应物） ----------

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BufEdit<T> {
    pub old: Range<T>,
    pub new: Range<T>,
}

impl<T: Copy + Sub<Output = T> + Into<f64>> BufEdit<T> {
    fn old_len(&self) -> T {
        self.old.end - self.old.start
    }
    fn new_len(&self) -> T {
        self.new.end - self.new.start
    }
}

impl BufEdit<InlayOffset> {
    fn old_len(&self) -> usize {
        self.old.end.0 - self.old.start.0
    }
    fn new_len(&self) -> usize {
        self.new.end.0 - self.new.start.0
    }
}

pub type FoldEdit = BufEdit<FoldOffset>;
type InlayEdit = BufEdit<InlayOffset>;

// ---------- FoldPlaceholder（PF 简化：占位符恒为 "⋯" 文本） ----------

/// zed FoldPlaceholder 对应物（去 render 元素/宽度约束——PF 占位符恒为
/// collapsed_text 或默认 "⋯" 的纯文本 run）。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FoldPlaceholder {
    /// If true, merges the fold with an adjacent one.
    pub merge_adjacent: bool,
    /// Category of the fold. Useful for carefully removing from overlapping folds.
    pub type_tag: Option<std::any::TypeId>,
    /// Text provided to display in place of the folded range.
    /// When set, this is used instead of the default "⋯" ellipsis.
    pub collapsed_text: Option<SharedString>,
}

impl Default for FoldPlaceholder {
    fn default() -> Self {
        Self {
            merge_adjacent: true,
            type_tag: None,
            collapsed_text: None,
        }
    }
}

// ---------- FoldPoint / FoldOffset ----------

#[derive(Copy, Clone, Debug, Default, Eq, Ord, PartialOrd, PartialEq)]
pub(crate) struct FoldPoint(pub Point);

impl FoldPoint {
    pub fn new(row: u32, column: u32) -> Self {
        Self(Point::new(row, column))
    }

    pub fn row(self) -> u32 {
        self.0.row
    }

    pub fn column(self) -> u32 {
        self.0.column
    }

    pub fn row_mut(&mut self) -> &mut u32 {
        &mut self.0.row
    }

    /// zed FoldPoint::to_inlay_point（transforms 查找 + overshoot）。
    pub fn to_inlay_point(self, snapshot: &FoldSnapshot) -> InlayPoint {
        let (start, _, _) = snapshot
            .transforms
            .find::<Dimensions<FoldPoint, InlayPoint>, _>((), &self, Bias::Right);
        let overshoot = self.0 - start.0.0;
        InlayPoint(start.1.0 + overshoot)
    }

    /// zed FoldPoint::to_offset（fold 空间 offset）。
    pub fn to_offset(self, snapshot: &FoldSnapshot) -> FoldOffset {
        let (start, _, item) = snapshot
            .transforms
            .find::<Dimensions<FoldPoint, TransformSummary>, _>((), &self, Bias::Right);
        let overshoot = self.0 - start.1.output.lines;
        let mut offset = start.1.output.len;
        if !overshoot_eq_zero(overshoot) {
            let transform = item.expect("display point out of range");
            assert!(transform.placeholder.is_none());
            let end_inlay_offset = snapshot
                .inlay_snapshot
                .to_offset(InlayPoint(start.1.input.lines + overshoot));
            offset += end_inlay_offset.0 - start.1.input.len;
        }
        FoldOffset(offset)
    }
}

fn overshoot_eq_zero(p: Point) -> bool {
    p.row == 0 && p.column == 0
}

impl<'a> sum_tree::Dimension<'a, TransformSummary> for FoldPoint {
    fn zero(_cx: ()) -> Self {
        Default::default()
    }

    fn add_summary(&mut self, summary: &'a TransformSummary, _: ()) {
        self.0 += summary.output.lines;
    }
}

// ---------- FoldMapWriter ----------

pub(crate) struct FoldMapWriter<'a>(&'a mut FoldMap);

impl FoldMapWriter<'_> {
    /// zed fold()：插入折叠（offset 区间；忽略空区间/重叠归并交给树）。
    pub(crate) fn fold(
        &mut self,
        ranges: impl IntoIterator<Item = (Range<usize>, FoldPlaceholder)>,
    ) -> (FoldSnapshot, Vec<FoldEdit>) {
        let mut edits = Vec::new();
        let mut folds = Vec::new();
        let snapshot = self.0.snapshot.inlay_snapshot.clone();
        for (range, fold_text) in ranges.into_iter() {
            // Ignore any empty ranges.
            if range.start == range.end {
                continue;
            }

            let fold_range = range.clone();

            folds.push(Fold {
                id: FoldId(post_inc(&mut self.0.next_fold_id.0)),
                range: FoldRange(fold_range),
                placeholder: fold_text,
            });

            let inlay_range = snapshot.to_inlay_offset(range.start)
                ..snapshot.to_inlay_offset(range.end);
            edits.push(InlayEdit {
                old: inlay_range.clone(),
                new: inlay_range,
            });
        }

        folds.sort_unstable_by(|a, b| {
            a.range
                .0
                .start
                .cmp(&b.range.0.start)
                .then(a.range.0.end.cmp(&b.range.0.end))
        });

        self.0.snapshot.folds = {
            let mut new_tree = SumTree::new(&snapshot.buffer);
            let mut cursor = self.0.snapshot.folds.cursor::<FoldRange>(&snapshot.buffer);
            for fold in folds {
                new_tree.append(cursor.slice(&fold.range, Bias::Right), &snapshot.buffer);
                new_tree.push(fold, &snapshot.buffer);
            }
            new_tree.append(cursor.suffix(), &snapshot.buffer);
            new_tree
        };

        let edits = consolidate_inlay_edits(edits);
        let edits = self.0.sync(snapshot.clone(), edits);
        (self.0.snapshot.clone(), edits)
    }

    /// zed unfold_intersecting：展开与区间相交的所有折叠。
    pub(crate) fn unfold_intersecting(
        &mut self,
        ranges: impl IntoIterator<Item = Range<usize>>,
        inclusive: bool,
    ) -> (FoldSnapshot, Vec<FoldEdit>) {
        let mut edits = Vec::new();
        let mut fold_ixs_to_delete = Vec::new();
        let snapshot = self.0.snapshot.inlay_snapshot.clone();
        let buffer = &snapshot.buffer;
        for range in ranges.into_iter() {
            let range = range;
            let mut folds_cursor =
                intersecting_folds(&snapshot, &self.0.snapshot.folds, range.clone(), inclusive);
            while let Some(fold) = folds_cursor.item() {
                let offset_range = fold.range.0.clone();
                if offset_range.end > offset_range.start {
                    let inlay_range = snapshot.to_inlay_offset(offset_range.start)
                        ..snapshot.to_inlay_offset(offset_range.end);
                    edits.push(InlayEdit {
                        old: inlay_range.clone(),
                        new: inlay_range,
                    });
                }
                fold_ixs_to_delete.push(*folds_cursor.start());
                folds_cursor.next();
            }
        }

        fold_ixs_to_delete.sort_unstable();
        fold_ixs_to_delete.dedup();

        self.0.snapshot.folds = {
            let mut cursor = self.0.snapshot.folds.cursor::<usize>(buffer);
            let mut folds = SumTree::new(buffer);
            for fold_ix in fold_ixs_to_delete {
                folds.append(cursor.slice(&fold_ix, Bias::Right), buffer);
                cursor.next();
            }
            folds.append(cursor.suffix(), buffer);
            folds
        };

        let edits = consolidate_inlay_edits(edits);
        let edits = self.0.sync(snapshot.clone(), edits);
        (self.0.snapshot.clone(), edits)
    }
}

fn post_inc(value: &mut usize) -> usize {
    let v = *value;
    *value += 1;
    v
}

// ---------- FoldMap ----------

/// zed FoldMap 对应物（transforms 树：fold = 把区间替换成 placeholder 的
/// 一等编辑变换；显示/缓冲映射由树查询给出）。
pub struct FoldMap {
    snapshot: FoldSnapshot,
    next_fold_id: FoldId,
}

impl FoldMap {
    pub fn new(inlay_snapshot: InlaySnapshot) -> (Self, FoldSnapshot) {
        let this = Self {
            snapshot: FoldSnapshot {
                folds: SumTree::new(&inlay_snapshot.buffer),
                transforms: SumTree::from_item(
                    Transform {
                        summary: TransformSummary {
                            input: inlay_snapshot.text_summary(),
                            output: inlay_snapshot.text_summary(),
                        },
                        placeholder: None,
                    },
                    (),
                ),
                inlay_snapshot: inlay_snapshot.clone(),
                version: 0,
            },
            next_fold_id: FoldId::default(),
        };
        let snapshot = this.snapshot.clone();
        (this, snapshot)
    }

    /// zed read()：buffer 编辑后 sync（edits：old 在旧 buffer 坐标、
    /// new 在新 buffer 坐标）。
    pub fn read(
        &mut self,
        inlay_snapshot: InlaySnapshot,
        edits: Vec<InlayEdit>,
    ) -> (FoldSnapshot, Vec<FoldEdit>) {
        let edits = self.sync(inlay_snapshot, edits);
        (self.snapshot.clone(), edits)
    }

    pub(crate) fn write(
        &mut self,
        inlay_snapshot: InlaySnapshot,
        edits: Vec<InlayEdit>,
    ) -> (FoldMapWriter<'_>, FoldSnapshot, Vec<FoldEdit>) {
        let (snapshot, edits) = self.read(inlay_snapshot, edits);
        (FoldMapWriter(self), snapshot, edits)
    }

    /// zed sync() 全量移植：transforms 树随 buffer 编辑重建（折叠区间经
    /// pull/extension 收口——编辑触到折叠即展开该折叠的变换）。
    fn sync(
        &mut self,
        inlay_snapshot: InlaySnapshot,
        inlay_edits: Vec<InlayEdit>,
    ) -> Vec<FoldEdit> {
        if inlay_edits.is_empty() {
            if self.snapshot.inlay_snapshot.version != inlay_snapshot.version {
                self.snapshot.version += 1;
            }
            self.snapshot.inlay_snapshot = inlay_snapshot;
            Vec::new()
        } else {
            let mut inlay_edits_iter = inlay_edits.iter().cloned().peekable();

            let mut new_transforms = SumTree::<Transform>::default();
            let mut cursor = self.snapshot.transforms.cursor::<InlayOffset>(());
            cursor.seek(&InlayOffset(0), Bias::Right);

            while let Some(mut edit) = inlay_edits_iter.next() {
                let mut item_is_non_fold = false;
                let mut item_summary = None;
                if let Some(item) = cursor.item() {
                    if !item.is_fold() {
                        item_is_non_fold = true;
                        item_summary = Some(item.summary.clone());
                    }
                }
                if item_is_non_fold {
                    new_transforms.update_last(
                        |transform| {
                            if !transform.is_fold() {
                                if let Some(summary) = &item_summary {
                                    transform.summary.add_summary(summary, ());
                                }
                                cursor.next();
                            }
                        },
                        (),
                    );
                }
                new_transforms.append(cursor.slice(&edit.old.start, Bias::Left), ());
                edit.new.start -= InlayOffset(edit.old.start.0 - cursor.start().0);
                edit.old.start = *cursor.start();

                cursor.seek(&edit.old.end, Bias::Right);
                cursor.next();

                let mut delta = edit.new_len() as isize - edit.old_len() as isize;
                loop {
                    edit.old.end = *cursor.start();

                    if let Some(next_edit) = inlay_edits_iter.peek() {
                        if next_edit.old.start > edit.old.end {
                            break;
                        }

                        let next_edit = inlay_edits_iter.next().unwrap();
                        delta += next_edit.new_len() as isize - next_edit.old_len() as isize;

                        if next_edit.old.end >= edit.old.end {
                            edit.old.end = next_edit.old.end;
                            cursor.seek(&edit.old.end, Bias::Right);
                            cursor.next();
                        }
                    } else {
                        break;
                    }
                }

                edit.new.end = InlayOffset(
                    ((edit.new.start.0 + edit.old_len()) as isize + delta) as usize,
                );

                let mut folds_cursor = self
                    .snapshot
                    .folds
                    .cursor::<FoldRange>(&inlay_snapshot.buffer);
                folds_cursor.seek(&inlay_snapshot.to_buffer_offset(edit.new.start), Bias::Left);

                let mut folds = iter::from_fn({
                    let inlay_snapshot = &inlay_snapshot;
                    move || {
                        let item = folds_cursor.item().map(|fold| {
                            let buffer_start = fold.range.start;
                            let buffer_end = fold.range.end;
                            (
                                fold.clone(),
                                inlay_snapshot.to_inlay_offset(buffer_start)
                                    ..inlay_snapshot.to_inlay_offset(buffer_end),
                            )
                        });
                        folds_cursor.next();
                        item
                    }
                })
                .peekable();

                while folds
                    .peek()
                    .is_some_and(|(_, fold_range)| fold_range.start < edit.new.end)
                {
                    let (fold, mut fold_range) = folds.next().unwrap();
                    let sum = new_transforms.summary();

                    assert!(fold_range.start.0 >= sum.input.len);

                    while folds.peek().is_some_and(|(next_fold, next_fold_range)| {
                        next_fold_range.start < fold_range.end
                            || (next_fold_range.start == fold_range.end
                                && fold.placeholder.merge_adjacent
                                && next_fold.placeholder.merge_adjacent)
                    }) {
                        let (_, next_fold_range) = folds.next().unwrap();
                        if next_fold_range.end > fold_range.end {
                            fold_range.end = next_fold_range.end;
                        }
                    }

                    if fold_range.start.0 > sum.input.len {
                        let text_summary = inlay_snapshot
                            .text_summary_for_range(InlayOffset(sum.input.len)..fold_range.start);
                        push_isomorphic(&mut new_transforms, text_summary);
                    }

                    if fold_range.end > fold_range.start {
                        const ELLIPSIS: &str = "⋯";

                        let placeholder_text: SharedString = fold
                            .placeholder
                            .collapsed_text
                            .clone()
                            .unwrap_or_else(|| ELLIPSIS.into());
                        let chars_bitmap = placeholder_text
                            .char_indices()
                            .fold(0u128, |bitmap, (idx, _)| {
                                bitmap | 1u128.unbounded_shl(idx as u32)
                            });

                        new_transforms.push(
                            Transform {
                                summary: TransformSummary {
                                    output: TextSummary::from_str(&placeholder_text),
                                    input: inlay_snapshot
                                        .text_summary_for_range(fold_range.start..fold_range.end),
                                },
                                placeholder: Some(TransformPlaceholder {
                                    text: placeholder_text,
                                    chars: chars_bitmap,
                                }),
                            },
                            (),
                        );
                    }
                }

                let sum = new_transforms.summary();
                if sum.input.len < edit.new.end.0 {
                    let text_summary = inlay_snapshot
                        .text_summary_for_range(InlayOffset(sum.input.len)..edit.new.end);
                    push_isomorphic(&mut new_transforms, text_summary);
                }
            }

            new_transforms.append(cursor.suffix(), ());
            if new_transforms.is_empty() {
                let text_summary = inlay_snapshot.text_summary();
                push_isomorphic(&mut new_transforms, text_summary);
            }

            drop(cursor);

            let mut fold_edits = Vec::with_capacity(inlay_edits.len());
            {
                let old_len = self.snapshot.inlay_snapshot.len();
                let new_len = inlay_snapshot.len();
                let mut old_transforms = self
                    .snapshot
                    .transforms
                    .cursor::<Dimensions<InlayOffset, FoldOffset>>(());
                let mut new_transforms =
                    new_transforms.cursor::<Dimensions<InlayOffset, FoldOffset>>(());

                let mut widened_edits = Vec::<InlayEdit>::with_capacity(inlay_edits.len());
                let mut inlay_edits_iter = inlay_edits.into_iter().peekable();
                while let Some(mut edit) = inlay_edits_iter.next() {
                    let previous_edit = widened_edits.last();
                    let mut merge_with_previous =
                        previous_edit.is_some_and(|previous| previous.old.end >= edit.old.start);
                    if !merge_with_previous {
                        let mut limit = match previous_edit {
                            Some(previous) => (edit.old.start - previous.old.end)
                                .min(edit.new.start - previous.new.end),
                            None => InlayOffset(edit.old.start.0.min(edit.new.start.0)),
                        };
                        loop {
                            old_transforms.seek(&edit.old.start, Bias::Left);
                            new_transforms.seek(&edit.new.start, Bias::Left);
                            let old_pull = if old_transforms.item().is_some_and(|t| t.is_fold()) {
                                edit.old.start - old_transforms.start().0
                            } else {
                                InlayOffset(0)
                            };
                            let new_pull = if new_transforms.item().is_some_and(|t| t.is_fold()) {
                                edit.new.start - new_transforms.start().0
                            } else {
                                InlayOffset(0)
                            };
                            let _ = old_pull;
                            let pull = old_pull.max(new_pull).min(limit);
                            if pull == InlayOffset(0) {
                                break;
                            }
                            edit.old.start -= pull;
                            edit.new.start -= pull;
                            limit -= pull;
                        }
                        merge_with_previous = previous_edit.is_some() && limit == InlayOffset(0);
                    }

                    let mut limit = match inlay_edits_iter.peek() {
                        Some(next) => {
                            (next.old.start - edit.old.end).min(next.new.start - edit.new.end)
                        }
                        None => (old_len - edit.old.end).min(new_len - edit.new.end),
                    };
                    loop {
                        old_transforms.seek_forward(&edit.old.end, Bias::Right);
                        new_transforms.seek_forward(&edit.new.end, Bias::Right);
                        let old_extension = if old_transforms.item().is_some_and(|t| t.is_fold()) {
                            old_transforms.end().0 - edit.old.end
                        } else {
                            InlayOffset(0)
                        };
                        let new_extension = if new_transforms.item().is_some_and(|t| t.is_fold()) {
                            new_transforms.end().0 - edit.new.end
                        } else {
                            InlayOffset(0)
                        };
                        let extension = old_extension.max(new_extension).min(limit);
                        if extension == InlayOffset(0) {
                            break;
                        }
                        edit.old.end += extension;
                        edit.new.end += extension;
                        limit -= extension;
                    }

                    if merge_with_previous {
                        if let Some(previous) = widened_edits.last_mut() {
                            previous.old.end = edit.old.end;
                            previous.new.end = edit.new.end;
                        }
                    } else {
                        widened_edits.push(edit);
                    }
                }

                for edit in widened_edits {
                    old_transforms.seek(&edit.old.start, Bias::Left);
                    let old_start = old_transforms.start().1.0
                        + (edit.old.start - old_transforms.start().0).0;
                    old_transforms.seek_forward(&edit.old.end, Bias::Right);
                    let old_end = old_transforms.start().1.0
                        + (edit.old.end - old_transforms.start().0).0;

                    new_transforms.seek(&edit.new.start, Bias::Left);
                    let new_start = new_transforms.start().1.0
                        + (edit.new.start - new_transforms.start().0).0;
                    new_transforms.seek_forward(&edit.new.end, Bias::Right);
                    let new_end = new_transforms.start().1.0
                        + (edit.new.end - new_transforms.start().0).0;

                    fold_edits.push(FoldEdit {
                        old: FoldOffset(old_start)..FoldOffset(old_end),
                        new: FoldOffset(new_start)..FoldOffset(new_end),
                    });
                }

                fold_edits = consolidate_fold_edits(fold_edits);
            }

            self.snapshot.transforms = new_transforms;
            self.snapshot.inlay_snapshot = inlay_snapshot;
            self.snapshot.version += 1;
            fold_edits
        }
    }
}

// ---------- FoldSnapshot ----------

#[derive(Clone)]
pub struct FoldSnapshot {
    pub inlay_snapshot: InlaySnapshot,
    transforms: SumTree<Transform>,
    folds: SumTree<Fold>,
    pub version: usize,
}

impl Deref for FoldSnapshot {
    type Target = InlaySnapshot;

    fn deref(&self) -> &Self::Target {
        &self.inlay_snapshot
    }
}

impl FoldSnapshot {
    pub fn buffer(&self) -> &Rope {
        &self.inlay_snapshot.buffer
    }

    pub fn fold_count(&self) -> usize {
        self.folds.items(&self.inlay_snapshot.buffer).len()
    }

    /// 以 offset 为块首的折叠区间（gutter 箭头 folded 判定/占位符展开）。
    pub fn fold_starting_at(&self, offset: usize) -> Option<Range<usize>> {
        self.folds
            .items(&self.inlay_snapshot.buffer)
            .iter()
            .find(|f| f.range.start == offset)
            .map(|f| f.range.0.clone())
    }

    /// 以缓冲行号为块首的折叠（crease 起点 = 行尾偏移）。
    pub fn fold_starting_at_row(&self, buffer_row: u32) -> Option<Range<usize>> {
        let row_start = self.buffer.point_to_offset(tree_sitter::Point::new(
            buffer_row as usize,
            self.buffer.line_len(buffer_row as usize),
        ));
        self.fold_starting_at(row_start)
    }

    /// 全部折叠区间（buffer 字节空间；宿主快照观测用）。
    pub fn fold_ranges(&self) -> Vec<Range<usize>> {
        self.folds
            .items(&self.inlay_snapshot.buffer)
            .iter()
            .map(|f| f.range.0.clone())
            .collect()
    }

    /// 隐藏行总数（缓冲行数 − 显示行数）。
    pub fn hidden_rows(&self) -> usize {
        self.buffer
            .lines_len()
            .saturating_sub(self.max_point().row() as usize + 1)
    }

    #[inline(always)]
    pub fn has_folds(&self) -> bool {
        !self.folds.is_empty()
    }

    /// zed text_summary_for_range（fold 空间 → 摘要）。
    pub fn text_summary_for_range(&self, range: Range<FoldPoint>) -> TextSummary {
        let mut summary = TextSummary::default();

        let mut cursor = self
            .transforms
            .cursor::<Dimensions<FoldPoint, InlayPoint>>(());
        cursor.seek(&range.start, Bias::Right);
        if let Some(transform) = cursor.item() {
            let start_in_transform = range.start.0 - cursor.start().0.0;
            let end_in_transform = cmp::min(range.end, cursor.end().0).0 - cursor.start().0.0;
            if let Some(placeholder) = transform.placeholder.as_ref() {
                summary = TextSummary::from_str(
                    &placeholder.text.as_ref()
                        [start_in_transform.column as usize..end_in_transform.column as usize],
                );
            } else {
                let inlay_start = self
                    .inlay_snapshot
                    .to_offset(InlayPoint(cursor.start().1.0 + start_in_transform));
                let inlay_end = self
                    .inlay_snapshot
                    .to_offset(InlayPoint(cursor.start().1.0 + end_in_transform));
                summary = self
                    .inlay_snapshot
                    .text_summary_for_range(inlay_start..inlay_end);
            }
        }

        if range.end > cursor.end().0 {
            cursor.next();
            summary += cursor
                .summary::<_, TransformSummary>(&range.end, Bias::Right)
                .output;
            if let Some(transform) = cursor.item() {
                let end_in_transform = range.end.0 - cursor.start().0.0;
                if let Some(placeholder) = transform.placeholder.as_ref() {
                    summary += TextSummary::from_str(
                        &placeholder.text.as_ref()[..end_in_transform.column as usize],
                    );
                } else {
                    let inlay_start = self.inlay_snapshot.to_offset(cursor.start().1);
                    let inlay_end = self
                        .inlay_snapshot
                        .to_offset(InlayPoint(cursor.start().1.0 + end_in_transform));
                    summary += self
                        .inlay_snapshot
                        .text_summary_for_range(inlay_start..inlay_end);
                }
            }
        }

        summary
    }

    /// zed to_fold_point（buffer 点 → fold 点；折叠内按 bias 贴边）。
    pub fn to_fold_point(&self, point: InlayPoint, bias: Bias) -> FoldPoint {
        let (start, end, item) = self
            .transforms
            .find::<Dimensions<InlayPoint, FoldPoint>, _>((), &point, Bias::Right);
        if item.is_some_and(|t| t.is_fold()) {
            if bias == Bias::Left || point == start.0 {
                start.1
            } else {
                end.1
            }
        } else {
            let overshoot = point.0 - start.0.0;
            FoldPoint(cmp::min(start.1.0 + overshoot, end.1.0))
        }
    }


    pub fn len(&self) -> FoldOffset {
        FoldOffset(self.transforms.summary().output.len)
    }

    pub fn line_len(&self, row: u32) -> u32 {
        let line_start = FoldPoint::new(row, 0).to_offset(self).0;
        let line_end = if row >= self.max_point().row() {
            self.len().0
        } else {
            FoldPoint::new(row + 1, 0).to_offset(self).0 - 1
        };
        (line_end - line_start) as u32
    }

    /// zed row_infos：显示行 → 缓冲行迭代器（PF 恒等 inlay：缓冲行 = 输入行）。
    pub fn row_infos(&self, start_row: u32) -> FoldRows<'_> {
        if start_row > self.transforms.summary().output.lines.row {
            panic!("invalid display row {}", start_row);
        }

        let fold_point = FoldPoint::new(start_row, 0);
        let mut cursor = self
            .transforms
            .cursor::<Dimensions<FoldPoint, InlayPoint>>(());
        cursor.seek(&fold_point, Bias::Left);

        let overshoot = fold_point.0 - cursor.start().0.0;
        let inlay_point = InlayPoint(cursor.start().1.0 + overshoot);
        let input_rows = BufferRows {
            next: inlay_point.0.row,
            max: self.inlay_snapshot.max_point().0.row,
        };

        FoldRows {
            fold_point,
            input_rows,
            cursor,
        }
    }

    pub fn max_point(&self) -> FoldPoint {
        FoldPoint(self.transforms.summary().output.lines)
    }

    /// zed FoldSnapshot::longest_row（fold_map.rs:925 原文）。
    pub fn longest_row(&self) -> u32 {
        self.transforms.summary().output.longest_row
    }

    /// zed folds_in_range（区间内折叠迭代）。
    pub fn folds_in_range(&self, range: Range<usize>) -> impl Iterator<Item = &Fold> {
        let mut folds = intersecting_folds(&self.inlay_snapshot, &self.folds, range, false);
        iter::from_fn(move || {
            let item = folds.item();
            folds.next();
            item
        })
    }

    /// zed intersects_fold（offset 是否落在折叠变换内）。
    pub fn intersects_fold(&self, offset: usize) -> bool {
        let inlay_offset = self.inlay_snapshot.to_inlay_offset(offset);
        let (_, _, item) = self
            .transforms
            .find::<InlayOffset, _>((), &inlay_offset, Bias::Right);
        item.is_some_and(|t| t.placeholder.is_some())
    }

    /// zed is_line_folded（缓冲行是否被折叠隐藏）。
    pub fn is_line_folded(&self, buffer_row: u32) -> bool {
        let mut inlay_point = self
            .inlay_snapshot
            .to_inlay_point(Point::new(buffer_row, 0));
        let mut cursor = self.transforms.cursor::<InlayPoint>(());
        cursor.seek(&inlay_point, Bias::Right);
        loop {
            match cursor.item() {
                Some(transform) => {
                    let buffer_point = self.inlay_snapshot.to_buffer_point(inlay_point);
                    if buffer_point.row != buffer_row {
                        return false;
                    } else if transform.placeholder.is_some() {
                        return true;
                    }
                }
                None => return false,
            }

            if cursor.end().row() == inlay_point.row() {
                cursor.next();
            } else {
                inlay_point.0 += Point::new(1, 0);
                cursor.seek(&inlay_point, Bias::Right);
            }
        }
    }

    /// zed placeholder_range_at（给定 fold 点所在占位符区间）。
    pub(crate) fn placeholder_range_at(&self, point: FoldPoint) -> Option<Range<FoldPoint>> {
        let (start, end, item) = self
            .transforms
            .find::<FoldPoint, _>((), &point, Bias::Right);
        item.filter(|transform| transform.placeholder.is_some())
            .map(|_| start..end)
    }

    /// zed clip_point（把点钳出折叠占位符——光标不入折叠）。
    pub fn clip_point(&self, point: FoldPoint, bias: Bias) -> FoldPoint {
        let (start, end, item) = self
            .transforms
            .find::<Dimensions<FoldPoint, InlayPoint>, _>((), &point, Bias::Right);
        if let Some(transform) = item {
            let transform_start = start.0.0;
            if transform.placeholder.is_some() {
                if point.0 == transform_start || matches!(bias, Bias::Left) {
                    FoldPoint(transform_start)
                } else {
                    FoldPoint(end.0.0)
                }
            } else {
                let overshoot = InlayPoint(point.0 - transform_start);
                let inlay_point = start.1 + overshoot;
                let clipped_inlay_point = self.inlay_snapshot.clip_point(inlay_point);
                FoldPoint(start.0.0 + (clipped_inlay_point - start.1).0)
            }
        } else {
            FoldPoint(self.transforms.summary().output.lines)
        }
    }

    // ---- PF 消费辅助（zed 由 chunk 管线/编辑器直接提供，PF 简化为直取） ----

    /// fold offset → buffer offset（transforms 反查）。
    pub fn fold_offset_to_buffer(&self, offset: FoldOffset) -> usize {
        let (start, _, item) = self
            .transforms
            .find::<Dimensions<FoldOffset, TransformSummary>, _>((), &offset, Bias::Right);
        let overshoot = offset.0 - start.0.0;
        if item.is_some_and(|t| t.is_fold()) {
            // 占位符内：贴折叠起点
            start.1.input.len
        } else {
            start.1.input.len + overshoot
        }
    }

    /// buffer offset → fold offset（经 fold point 换算，折叠内按 bias 贴边）。
    pub fn buffer_offset_to_fold(&self, offset: usize, bias: Bias) -> FoldOffset {
        let point = self.inlay_snapshot.to_point(InlayOffset(offset));
        let fold_point = self.to_fold_point(point, bias);
        fold_point.to_offset(self)
    }

    /// fold 行文本（懒取：transforms 游标 seek + 收集到换行；PF element
    /// 每帧只取可见行）。
    pub fn row_text(&self, row: u32) -> String {
        let start = FoldPoint::new(row, 0).to_offset(self).0;
        let end = if row >= self.max_point().row() {
            self.len().0
        } else {
            FoldPoint::new(row + 1, 0).to_offset(self).0.saturating_sub(1)
        };
        let mut out = String::new();
        let mut cursor = self
            .transforms
            .cursor::<Dimensions<FoldOffset, InlayOffset>>(());
        cursor.seek(&FoldOffset(start), Bias::Right);
        let mut produced = 0usize;
        let want = end - start;
        while produced < want {
            let Some(transform) = cursor.item() else { break };
            let seg_start = cursor.start().0.0.max(start);
            let seg_end = (cursor.end().0.0).min(end);
            if seg_end > seg_start {
                if let Some(placeholder) = transform.placeholder.as_ref() {
                    let s = seg_start - cursor.start().0.0;
                    let e = seg_end - cursor.start().0.0;
                    out.push_str(&placeholder.text.as_ref()[s..e]);
                } else {
                    let inlay_start = cursor.start().1 + InlayOffset(seg_start - cursor.start().0.0);
                    let inlay_end = cursor.start().1 + InlayOffset(seg_end - cursor.start().0.0);
                    out.push_str(&text_slice(
                        &self.inlay_snapshot.buffer,
                        inlay_start.0..inlay_end.0,
                    ));
                }
                produced += seg_end - seg_start;
            }
            if produced < want {
                cursor.next();
            } else {
                break;
            }
        }
        out
    }

    /// fold 行 → 缓冲行（行号栏映射）。
    pub fn row_to_buffer_row(&self, row: u32) -> u32 {
        self.to_inlay_point_of_row(row).0.row
    }

    fn to_inlay_point_of_row(&self, row: u32) -> InlayPoint {
        let fold_point = FoldPoint::new(row, 0);
        let (start, _, _) = self
            .transforms
            .find::<Dimensions<FoldPoint, InlayPoint>, _>((), &fold_point, Bias::Left);
        let overshoot = fold_point.0 - start.0.0;
        InlayPoint(start.1.0 + overshoot)
    }

    /// 显示行分段：(行内字节区间, buffer 字节区间)——buffer 段走语法
    /// 高亮，None = 折叠占位符（zed chunk 管线的 placeholder chunk 对应物）。
    pub fn row_segments(&self, row: u32) -> Vec<(Range<usize>, Option<Range<usize>>)> {
        let mut out = vec![];
        let row_start = FoldPoint::new(row, 0).to_offset(self).0;
        let row_end = if row >= self.max_point().row() {
            self.len().0
        } else {
            FoldPoint::new(row + 1, 0).to_offset(self).0.saturating_sub(1)
        };
        let mut cursor = self
            .transforms
            .cursor::<Dimensions<FoldOffset, InlayOffset>>(());
        cursor.seek(&FoldOffset(row_start), Bias::Right);
        while let Some(transform) = cursor.item() {
            let seg = cursor.start().0.0.max(row_start)..(cursor.end().0.0).min(row_end);
            if seg.end > seg.start {
                if transform.placeholder.is_some() {
                    out.push((seg.start - row_start..seg.end - row_start, None));
                } else {
                    let inlay_start =
                        cursor.start().1 + InlayOffset(seg.start - cursor.start().0.0);
                    let inlay_end = cursor.start().1 + InlayOffset(seg.end - cursor.start().0.0);
                    out.push((
                        seg.start - row_start..seg.end - row_start,
                        Some(inlay_start.0..inlay_end.0),
                    ));
                }
            }
            if cursor.end().0.0 >= row_end {
                break;
            }
            cursor.next();
        }
        out
    }

    /// 行内占位符字节区间（fold 行内局部坐标；PF run 着色用）。
    pub fn placeholder_ranges_in_row(&self, row: u32) -> Vec<Range<usize>> {
        let mut out = vec![];
        let row_start = FoldPoint::new(row, 0).to_offset(self).0;
        let row_end = if row >= self.max_point().row() {
            self.len().0
        } else {
            FoldPoint::new(row + 1, 0).to_offset(self).0.saturating_sub(1)
        };
        let mut cursor = self
            .transforms
            .cursor::<Dimensions<FoldOffset, InlayOffset>>(());
        cursor.seek(&FoldOffset(row_start), Bias::Right);
        while let Some(transform) = cursor.item() {
            let seg = cursor.start().0.0.max(row_start)..(cursor.end().0.0).min(row_end);
            if seg.end > seg.start {
                if transform.placeholder.is_some() {
                    out.push(seg.start - row_start..seg.end - row_start);
                }
            }
            if cursor.end().0.0 >= row_end {
                break;
            }
            cursor.next();
        }
        out
    }
}

fn push_isomorphic(transforms: &mut SumTree<Transform>, summary: TextSummary) {
    let mut did_merge = false;
    transforms.update_last(
        |last| {
            if !last.is_fold() {
                last.summary.input += summary.clone();
                last.summary.output += summary.clone();
                did_merge = true;
            }
        },
        (),
    );
    if !did_merge {
        transforms.push(
            Transform {
                summary: TransformSummary {
                    input: summary.clone(),
                    output: summary,
                },
                placeholder: None,
            },
            (),
        )
    }
}

fn intersecting_folds<'a>(
    inlay_snapshot: &'a InlaySnapshot,
    folds: &'a SumTree<Fold>,
    range: Range<usize>,
    inclusive: bool,
) -> FilterCursor<'a, 'a, impl 'a + FnMut(&FoldSummary) -> bool, Fold, usize> {
    let mut cursor = folds.filter::<_, usize>(&inlay_snapshot.buffer, move |summary| {
        let start_cmp = range.start.cmp(&summary.max_end);
        let end_cmp = range.end.cmp(&summary.min_start);

        if inclusive {
            start_cmp <= Ordering::Equal && end_cmp >= Ordering::Equal
        } else {
            start_cmp == Ordering::Less && end_cmp == Ordering::Greater
        }
    });
    cursor.next();
    cursor
}

fn consolidate_inlay_edits(mut edits: Vec<InlayEdit>) -> Vec<InlayEdit> {
    edits.sort_unstable_by(|a, b| {
        a.old
            .start
            .cmp(&b.old.start)
            .then_with(|| b.old.end.cmp(&a.old.end))
    });

    let mut inlay_edits = edits.into_iter();

    if let Some(mut first_edit) = inlay_edits.next() {
        let mut v: Vec<_> = inlay_edits
            .scan(&mut first_edit, |prev_edit, edit| {
                if prev_edit.old.end >= edit.old.start {
                    prev_edit.old.end = prev_edit.old.end.max(edit.old.end);
                    prev_edit.new.start = prev_edit.new.start.min(edit.new.start);
                    prev_edit.new.end = prev_edit.new.end.max(edit.new.end);
                    Some(None) // Skip this edit, it's merged
                } else {
                    let prev = std::mem::replace(*prev_edit, edit);
                    Some(Some(prev)) // Yield the previous edit
                }
            })
            .filter_map(|x| x)
            .collect();
        v.push(first_edit.clone());
        v
    } else {
        vec![]
    }
}

fn consolidate_fold_edits(mut edits: Vec<FoldEdit>) -> Vec<FoldEdit> {
    edits.sort_unstable_by(|a, b| {
        a.old
            .start
            .cmp(&b.old.start)
            .then_with(|| b.old.end.cmp(&a.old.end))
    });
    let mut fold_edits = edits.into_iter();

    if let Some(mut first_edit) = fold_edits.next() {
        let mut v: Vec<_> = fold_edits
            .scan(&mut first_edit, |prev_edit, edit| {
                if prev_edit.old.end >= edit.old.start {
                    prev_edit.old.end = prev_edit.old.end.max(edit.old.end);
                    prev_edit.new.start = prev_edit.new.start.min(edit.new.start);
                    prev_edit.new.end = prev_edit.new.end.max(edit.new.end);
                    Some(None) // Skip this edit, it's merged
                } else {
                    let prev = std::mem::replace(*prev_edit, edit);
                    Some(Some(prev)) // Yield the previous edit
                }
            })
            .filter_map(|x| x)
            .collect();
        v.push(first_edit.clone());
        v
    } else {
        vec![]
    }
}

#[derive(Clone, Debug, Default)]
struct Transform {
    summary: TransformSummary,
    placeholder: Option<TransformPlaceholder>,
}

#[derive(Clone, Debug)]
struct TransformPlaceholder {
    text: SharedString,
    chars: u128,
}

impl Transform {
    fn is_fold(&self) -> bool {
        self.placeholder.is_some()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct TransformSummary {
    output: TextSummary,
    input: TextSummary,
}

impl sum_tree::Item for Transform {
    type Summary = TransformSummary;

    fn summary(&self, _cx: ()) -> Self::Summary {
        self.summary.clone()
    }
}

impl sum_tree::ContextLessSummary for TransformSummary {
    fn zero() -> Self {
        Default::default()
    }

    fn add_summary(&mut self, other: &Self) {
        self.input += other.input.clone();
        self.output += other.output.clone();
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Default, Ord, PartialOrd, Hash)]
pub struct FoldId(pub usize);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Fold {
    pub id: FoldId,
    pub range: FoldRange,
    pub placeholder: FoldPlaceholder,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FoldRange(pub(crate) Range<usize>);

impl Deref for FoldRange {
    type Target = Range<usize>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for FoldRange {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Default for FoldRange {
    fn default() -> Self {
        Self(0..usize::MAX)
    }
}

impl sum_tree::Item for Fold {
    type Summary = FoldSummary;

    fn summary(&self, _cx: &Rope) -> Self::Summary {
        FoldSummary {
            start: self.range.start,
            end: self.range.end,
            min_start: self.range.start,
            max_end: self.range.end,
            count: 1,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FoldSummary {
    start: usize,
    end: usize,
    min_start: usize,
    max_end: usize,
    count: usize,
}

impl Default for FoldSummary {
    fn default() -> Self {
        Self {
            start: 0,
            end: usize::MAX,
            min_start: usize::MAX,
            max_end: 0,
            count: 0,
        }
    }
}

impl sum_tree::Summary for FoldSummary {
    type Context<'a> = &'a Rope;

    fn zero(_cx: &Rope) -> Self {
        Default::default()
    }

    fn add_summary(&mut self, other: &Self, _buffer: Self::Context<'_>) {
        if other.min_start.cmp(&self.min_start) == Ordering::Less {
            self.min_start = other.min_start;
        }
        if other.max_end.cmp(&self.max_end) == Ordering::Greater {
            self.max_end = other.max_end;
        }

        #[cfg(debug_assertions)]
        {
            let start_comparison = self.start.cmp(&other.start);
            assert!(start_comparison <= Ordering::Equal);
            if start_comparison == Ordering::Equal {
                assert!(self.end.cmp(&other.end) >= Ordering::Equal);
            }
        }

        self.start = other.start;
        self.end = other.end;
        self.count += other.count;
    }
}

impl<'a> sum_tree::Dimension<'a, FoldSummary> for FoldRange {
    fn zero(_cx: &Rope) -> Self {
        Default::default()
    }

    fn add_summary(&mut self, summary: &'a FoldSummary, _: &Rope) {
        self.0.start = summary.start;
        self.0.end = summary.end;
    }
}

impl sum_tree::SeekTarget<'_, FoldSummary, FoldRange> for FoldRange {
    fn cmp(&self, other: &Self, _buffer: &Rope) -> Ordering {
        Ord::cmp(&self.0.start, &other.0.start).then(Ord::cmp(&self.0.end, &other.0.end))
    }
}

impl sum_tree::SeekTarget<'_, FoldSummary, FoldRange> for usize {
    fn cmp(&self, cursor_location: &FoldRange, _buffer: &Rope) -> Ordering {
        Ord::cmp(self, &cursor_location.start)
    }
}

impl<'a> sum_tree::Dimension<'a, FoldSummary> for usize {
    fn zero(_cx: &Rope) -> Self {
        Default::default()
    }

    fn add_summary(&mut self, summary: &'a FoldSummary, _: &Rope) {
        *self += summary.count;
    }
}

// ---------- FoldRows（显示行 → 缓冲行迭代；zed RowInfo 对应物 = 缓冲行号） ----------

/// zed InlayBufferRows 对应物（PF 恒等 inlay：缓冲行计数器）。
struct BufferRows {
    next: u32,
    max: u32,
}

impl BufferRows {
    fn seek(&mut self, row: u32) {
        self.next = row;
    }

    fn next(&mut self) -> Option<u32> {
        if self.next > self.max {
            None
        } else {
            let row = self.next;
            self.next += 1;
            Some(row)
        }
    }
}

pub struct FoldRows<'a> {
    cursor: Cursor<'a, 'static, Transform, Dimensions<FoldPoint, InlayPoint>>,
    input_rows: BufferRows,
    fold_point: FoldPoint,
}

impl FoldRows<'_> {
    pub(crate) fn seek(&mut self, row: u32) {
        let fold_point = FoldPoint::new(row, 0);
        self.cursor.seek(&fold_point, Bias::Left);
        let overshoot = fold_point.0 - self.cursor.start().0.0;
        let inlay_point = InlayPoint(self.cursor.start().1.0 + overshoot);
        self.input_rows.seek(inlay_point.0.row);
        self.fold_point = fold_point;
    }
}

impl Iterator for FoldRows<'_> {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        let mut traversed_fold = false;
        while self.fold_point > self.cursor.end().0 {
            self.cursor.next();
            traversed_fold = true;
            if self.cursor.item().is_none() {
                break;
            }
        }

        if self.cursor.item().is_some() {
            if traversed_fold {
                self.input_rows.seek(self.cursor.start().1.0.row);
                self.input_rows.next();
            }
            *self.fold_point.row_mut() += 1;
            self.input_rows.next()
        } else {
            None
        }
    }
}

// ---------- fold 偏移算术（zed FoldOffset ops 移植） ----------

#[derive(Copy, Clone, Debug, Default, Eq, Ord, PartialOrd, PartialEq)]
pub struct FoldOffset(pub usize);

impl FoldOffset {
    pub fn to_point(self, snapshot: &FoldSnapshot) -> FoldPoint {
        let (start, _, item) = snapshot
            .transforms
            .find::<Dimensions<FoldOffset, TransformSummary>, _>((), &self, Bias::Right);
        let overshoot = self.0 - start.0.0;
        let overshoot_point = if item.is_some_and(|t| t.is_fold()) {
            Point::new(0, overshoot as u32)
        } else {
            let inlay_offset = InlayOffset(start.1.input.len) + InlayOffset(overshoot);
            let inlay_point = snapshot.inlay_snapshot.to_point(inlay_offset);
            inlay_point.0 - start.1.input.lines
        };
        FoldPoint(start.1.output.lines + overshoot_point)
    }
}

impl Add for FoldOffset {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl Sub for FoldOffset {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl SubAssign<usize> for FoldOffset {
    fn sub_assign(&mut self, rhs: usize) {
        self.0 -= rhs;
    }
}

impl Add<usize> for FoldOffset {
    type Output = Self;

    fn add(self, rhs: usize) -> Self::Output {
        Self(self.0 + rhs)
    }
}

impl AddAssign<usize> for FoldOffset {
    fn add_assign(&mut self, rhs: usize) {
        self.0 += rhs;
    }
}

impl<'a> sum_tree::Dimension<'a, TransformSummary> for FoldOffset {
    fn zero(_cx: ()) -> Self {
        Default::default()
    }

    fn add_summary(&mut self, summary: &'a TransformSummary, _: ()) {
        self.0 += summary.output.len;
    }
}

impl<'a> sum_tree::Dimension<'a, TransformSummary> for InlayPoint {
    fn zero(_cx: ()) -> Self {
        Default::default()
    }

    fn add_summary(&mut self, summary: &'a TransformSummary, _: ()) {
        self.0 += summary.input.lines;
    }
}

impl<'a> sum_tree::Dimension<'a, TransformSummary> for InlayOffset {
    fn zero(_cx: ()) -> Self {
        Default::default()
    }

    fn add_summary(&mut self, summary: &'a TransformSummary, _: ()) {
        self.0 += summary.input.len;
    }
}

// ---------- InlayPoint/FoldPoint 维度（Dimensions 用） ----------

impl<'a> sum_tree::Dimension<'a, TransformSummary> for Point {
    fn zero(_cx: ()) -> Self {
        Default::default()
    }

    fn add_summary(&mut self, summary: &'a TransformSummary, _: ()) {
        *self += summary.output.lines;
    }
}

// ---------- crease 构建（zed display_map.rs:2241/2290 移植） ----------

pub(super) trait FoldBufferExt {
    /// zed starts_indent（display_map.rs:2241 原样）：当前行非空白、其下
    /// 第一个非空行缩进更深 → 可折叠。
    fn starts_indent(&self, buffer_row: u32) -> bool;
    /// zed closing_bracket_indent_len（display_map.rs:2267 移植）：行以该
    /// 语言的闭合括号/闭合关键词开头 → Some(缩进位)。`language` =
    /// InputMode::CodeEditor 的 language 名（zed language_scope_at(row,0)
    /// 的括号表在 PF 的对应物）。
    fn closing_bracket_indent_len(&self, row: u32, language: Option<&str>) -> Option<u32>;
    /// zed crease_for_buffer_row 的无 LSP 路（display_map.rs:2290 移植）：
    /// crease = (行尾, 闭合括号缩进位 | 最后非空行尾)，闭合括号随折叠行
    /// 显示在占位符后。
    fn fold_crease_for_row(
        &self,
        buffer_row: u32,
        language: Option<&str>,
    ) -> Option<(Range<usize>, FoldPlaceholder)>;
}

/// 各语言的闭合括号/闭合关键词 **end** 串表——zed 各语言 config.toml 的
/// `brackets` 数组原样数据移植（zed closing_bracket_indent_len 用
/// `scope.brackets().any(|(pair,_)| line_text.starts_with(&pair.end))` 全量
/// 匹配，不做上下文过滤——language.rs:1373 brackets() 的 bool 只供补全）。
/// bash 的 then→else/fi/done/elif、in→esac 关键词对即用户截图
/// `then⋯else`（else 保留在占位符后）的语义来源。
/// 不在 zed 参考仓的语言（java/csharp/swift/zig/cmake/elixir/graphql/
/// scala/make/toml 等外部扩展）沿用 PF 通用字符集兜底。
const FOLD_BRACKET_ENDS: &[(&str, &[&str])] = &[
    ("rust", &["}", "\"#", "\"##", "\"###", "]", ")", ">", "\"", "`", " */"]),
    ("python", &["\"", "'", "}", "]", ")"]),
    ("javascript", &["}", "]", ")", ">", "\"", "'", "`", " */"]),
    ("typescript", &["}", "]", ")", ">", "\"", "'", "`", " */"]),
    ("tsx", &["}", "]", ")", ">", "\"", "'", "`", " */"]),
    ("go", &["}", "]", ")", "\"", "'", "`", " */"]),
    ("json", &["}", "]", ")", "\""]),
    ("jsonc", &["}", "]", ")", "\""]),
    ("css", &["}", "]", ")", "\"", "'"]),
    ("yaml", &["}", "]", "\"", "'"]),
    ("c", &["}", "]", ")", "\"", "`", "'", " */"]),
    ("cpp", &["}", "]", ")", "\"", "`", "'", " */"]),
    ("bash", &["]", ")", "}", "\"", "'", "done", "fi", "else", "elif", "esac"]),
    ("proto", &["}", "]", ")", "\"", "'", " */"]),
    ("html", &["}", "]", ")", "\"", ">", " --"]),
    ("markdown", &["}", "]", ")", ">", "\"", "'", "`", "*", "~"]),
    ("diff", &[]),
];
/// 未收录语言的兜底闭合集（PF 原 FOLD_CLOSING_BRACKETS 保留）。
const FOLD_CLOSING_BRACKETS: &[char] = &['}', ')', ']', '>'];

impl FoldBufferExt for Rope {
    fn starts_indent(&self, buffer_row: u32) -> bool {
        let max_row = self.lines_len().saturating_sub(1) as u32;
        if buffer_row >= max_row {
            return false;
        }

        let line_indent = indent_len_for_line(self, buffer_row);
        if is_line_blank(self, buffer_row) {
            return false;
        }

        (buffer_row + 1..=max_row)
            .find_map(|next_row| {
                let next_line_indent = indent_len_for_line(self, next_row);
                if next_line_indent > line_indent {
                    Some(true)
                } else if !is_line_blank(self, next_row) {
                    Some(false)
                } else {
                    None
                }
            })
            .unwrap_or(false)
    }

    fn closing_bracket_indent_len(&self, row: u32, language: Option<&str>) -> Option<u32> {
        let indent_len = indent_len_for_line(self, row);
        let line_text = text_slice(
            self,
            self.point_to_offset(tree_sitter::Point::new(row as usize, indent_len as usize))..self.len(),
        );
        let line_head = line_text.lines().next().unwrap_or("");
        // zed：全量括号对表 starts_with(pair.end)；先查语言表，未收录语言
        // 落通用字符集兜底
        let lang_hit = language
            .and_then(|lang| {
                FOLD_BRACKET_ENDS
                    .iter()
                    .find(|(name, _)| *name == lang)
                    .map(|(_, ends)| *ends)
            })
            .is_some_and(|ends| ends.iter().any(|e| line_head.starts_with(*e)));
        if lang_hit || FOLD_CLOSING_BRACKETS.iter().any(|b| line_head.starts_with(*b)) {
            Some(indent_len)
        } else {
            None
        }
    }

    fn fold_crease_for_row(
        &self,
        buffer_row: u32,
        language: Option<&str>,
    ) -> Option<(Range<usize>, FoldPlaceholder)> {
        let start = self.point_to_offset(tree_sitter::Point::new(
            buffer_row as usize,
            self.line_len(buffer_row as usize),
        ));
        if !self.starts_indent(buffer_row) {
            return None;
        }

        let start_line_indent = indent_len_for_line(self, buffer_row);
        let max_point = self.lines_len().saturating_sub(1) as u32;
        let mut closing_row = None;

        for row in (buffer_row + 1)..=max_point {
            let line_indent = indent_len_for_line(self, row);
            if !is_line_blank(self, row) && line_indent <= start_line_indent {
                closing_row = Some(row);
                break;
            }
        }

        let last_non_blank_row = |from_row: u32| -> usize {
            let mut row = from_row;
            while row > buffer_row && is_line_blank(self, row) {
                row -= 1;
            }
            self.point_to_offset(tree_sitter::Point::new(
                row as usize,
                self.line_len(row as usize),
            ))
        };

        let end = if let Some(row) = closing_row {
            if let Some(indent_len) = self.closing_bracket_indent_len(row, language) {
                // Include newline and whitespace before closing delimiter,
                // so it appears on the same display line as the fold placeholder
                self.point_to_offset(tree_sitter::Point::new(row as usize, indent_len as usize))
            } else {
                last_non_blank_row(row - 1)
            }
        } else {
            last_non_blank_row(max_point)
        };

        Some((
            start..end,
            FoldPlaceholder::default(),
        ))
    }
}

fn indent_len_for_line(buffer: &Rope, row: u32) -> u32 {
    let line = text_slice(
        buffer,
        buffer.point_to_offset(tree_sitter::Point::new(row as usize, 0))
            ..buffer.line_end_offset(row as usize),
    );
    line.bytes()
        .take_while(|b| *b == b' ' || *b == b'\t')
        .count() as u32
}

fn is_line_blank(buffer: &Rope, row: u32) -> bool {
    text_slice(
        buffer,
        buffer.point_to_offset(tree_sitter::Point::new(row as usize, 0))
            ..buffer.line_end_offset(row as usize),
    )
    .chars()
    .all(|c| c.is_whitespace())
}

// ---------- InputState 折叠入口（zed editor fold_at/unfold_at 移植） ----------

impl InputState {
    /// zed fold_at（fold.rs:424 移植）：折叠 buffer_row 所在 crease。
    pub(super) fn fold_map_fold_at(&mut self, buffer_row: u32) {
        // closing_bracket_indent_len 的语言表入口（zed language_scope_at
        // 的 PF 对应物 = CodeEditor mode 的 language 名）
        let language = match &self.mode {
            InputMode::CodeEditor { language, .. } => Some(language.as_ref().to_string()),
            _ => None,
        };
        let Some((range, placeholder)) =
            self.text.fold_crease_for_row(buffer_row, language.as_deref())
        else {
            return;
        };
        let version = self.fold_snapshot.version;
        let inlay_snapshot = InlaySnapshot {
            buffer: self.text.clone(),
            version,
        };
        let (mut writer, _snapshot, _edits) = self.fold_map.write(inlay_snapshot, Vec::new());
        writer.fold([(range, placeholder)]);
        self.sync_fold_snapshot();
        self.fold_clip_selections();
    }

    /// gutter 判定：该缓冲行是否为折叠块首。
    pub(super) fn fold_starts_at_row(&self, buffer_row: u32) -> bool {
        self.fold_snapshot.fold_starting_at_row(buffer_row).is_some()
    }

    /// zed unfold_at（fold.rs 移植）：展开与 buffer_row 相交的折叠。
    pub(super) fn fold_map_unfold_at(&mut self, buffer_row: u32) {
        let row_start = self
            .text
            .point_to_offset(tree_sitter::Point::new(buffer_row as usize, 0));
        let row_end = self.text.point_to_offset(tree_sitter::Point::new(
            buffer_row as usize,
            self.text.line_len(buffer_row as usize),
        ));
        let version = self.fold_snapshot.version;
        let inlay_snapshot = InlaySnapshot {
            buffer: self.text.clone(),
            version,
        };
        let (mut writer, _snapshot, _edits) = self.fold_map.write(inlay_snapshot, Vec::new());
        // zed unfold_at（fold.rs:497）→ unfold_ranges(..., inclusive=true,
        // ...)：行区间与 crease 相切也算相交（crease 起点在行尾）。
        writer.unfold_intersecting([row_start..row_end], true);
        self.sync_fold_snapshot();
        self.fold_clip_selections();
    }

    /// zed unfold_intersecting（字节区间入口；编辑 reveal 用）。
    pub(super) fn fold_map_unfold_ranges(&mut self, ranges: impl IntoIterator<Item = Range<usize>>) {
        let version = self.fold_snapshot.version;
        let inlay_snapshot = InlaySnapshot {
            buffer: self.text.clone(),
            version,
        };
        let (mut writer, _snapshot, _edits) = self.fold_map.write(inlay_snapshot, Vec::new());
        writer.unfold_intersecting(ranges, true);
        self.sync_fold_snapshot();
    }

    /// buffer 编辑后：fold 区间按 zed anchor 语义平移 + read/sync。
    pub(super) fn fold_map_sync_edit(&mut self, old_range: Range<usize>, new_text_len: usize) {
        shift_fold_ranges_for_edit(&mut self.fold_snapshot, old_range.clone(), new_text_len);
        let version = self.fold_snapshot.version;
        let inlay_snapshot = InlaySnapshot {
            buffer: self.text.clone(),
            version: version + 1,
        };
        let edits = vec![InlayEdit {
            old: InlayOffset(old_range.start)..InlayOffset(old_range.end),
            new: InlayOffset(old_range.start)..InlayOffset(old_range.start + new_text_len),
        }];
        let (snapshot, _) = self.fold_map.read(inlay_snapshot, edits);
        self.fold_snapshot = snapshot;
    }

    /// writer 产物回填（fold/unfold 后 folds 树在 writer 内已重建，这里
    /// 只取回 snapshot；buffer 未变，inlay 版本不变）。
    fn sync_fold_snapshot(&mut self) {
        self.fold_snapshot = self.fold_map.snapshot.clone();
    }

    /// zed 折叠后选区收口（fold.rs fold_creases → 显示换算经 clip_point；
    /// PF 光标/选区是裸 offset，必须落锤后显式钳出隐藏区，否则插入落在
    /// 隐藏文本里不可见）。走全变换链 buffer→to_fold_point→clip_point→
    /// fold_offset_to_buffer——不得把 buffer 点当 fold 点直接钳（fold 后
    /// 行号错位，见 fold_clip_offset 的教训）。
    fn fold_clip_selections(&mut self) {
        if !self.fold_snapshot.has_folds() {
            return;
        }
        let mut clip = |off: usize, bias: Bias| -> usize {
            let p = self.text.offset_to_point(off);
            let fp = self.fold_snapshot.to_fold_point(
                InlayPoint(Point::new(p.row as u32, p.column as u32)),
                bias,
            );
            let clipped = self.fold_snapshot.clip_point(fp, bias);
            self.fold_snapshot
                .fold_offset_to_buffer(clipped.to_offset(&self.fold_snapshot))
        };
        let start = clip(self.selected_range.start, Bias::Left);
        let end = clip(self.selected_range.end, Bias::Right);
        if start != self.selected_range.start || end != self.selected_range.end {
            self.selected_range = (start..end).into();
        }
    }

    /// 光标钳出折叠占位符（zed clip_point 经 change_selections 应用）。
    pub(super) fn fold_clip_offset(&mut self, offset: usize, bias: Bias) -> usize {
        if !self.fold_snapshot.has_folds() {
            return offset;
        }
        let p = self.text.offset_to_point(offset);
        let point = Point::new(p.row as u32, p.column as u32);
        let clipped = self.fold_snapshot.clip_point(FoldPoint(point), bias);
        if clipped.0 == point {
            return offset;
        }
        self.text.point_to_offset(tree_sitter::Point::new(
            clipped.0.row as usize,
            clipped.0.column as usize,
        ))
    }

    /// 折叠区间表（buffer 字节空间）。宿主快照/自动化观测只读视图；
    /// 非 code_editor 或 md 装饰态恒空。
    pub fn fold_ranges(&self) -> Vec<Range<usize>> {
        if self.fold_allowed() {
            self.fold_snapshot.fold_ranges()
        } else {
            Vec::new()
        }
    }

    /// 隐藏行总数（宿主观测用）。
    pub fn fold_hidden_rows(&self) -> usize {
        if self.fold_allowed() {
            self.fold_snapshot.hidden_rows()
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

    /// 折叠是否启用（code_editor 且非 024 装饰态——md wysiwyg 行内折叠
    /// 系统与块级折叠互斥，同 024 折叠/折行互斥先例）。
    pub(super) fn fold_allowed(&self) -> bool {
        self.mode.is_code_editor() && self.decorations.is_none()
    }

    /// zed indent_guides.rs 的 fold 过滤（indent_guides_in_range 的 filter
    /// 原样语义）：完全被折叠块包含的 guide 不画——fold 起点行号严格小于
    /// guide 起点行号、fold 终点行号 ≥ guide 终点行号（Point 行主序比较，
    /// fold 起点 = 块首行尾）。
    pub(super) fn indent_guides_filter_folds(&self, guides: Vec<IndentGuide>) -> Vec<IndentGuide> {
        if !self.fold_allowed() || !self.fold_snapshot.has_folds() {
            return guides;
        }
        let fold_rows: Vec<(u32, u32)> = self
            .fold_snapshot
            .fold_ranges()
            .iter()
            .map(|r| {
                (
                    self.text.offset_to_point(r.start).row as u32,
                    self.text.offset_to_point(r.end).row as u32,
                )
            })
            .collect();
        guides
            .into_iter()
            .filter(|g| {
                !fold_rows
                    .iter()
                    .any(|&(fold_start_row, fold_end_row)| {
                        fold_start_row < g.start_row && fold_end_row >= g.end_row
                    })
            })
            .collect()
    }
}

/// zed anchor_after/anchor_before 随编辑移动的 offset 版：编辑前的折叠
/// 区间按 anchor 贴合规则移动（PF 无 anchor 的显式平移）。
fn shift_fold_ranges_for_edit(snapshot: &mut FoldSnapshot, edit: Range<usize>, new_len: usize) {
    if !snapshot.has_folds() {
        return;
    }
    let buffer = snapshot.inlay_snapshot.buffer.clone();
    let delta = new_len as isize - (edit.end - edit.start) as isize;
    let mut new_tree = SumTree::new(&buffer);
    for fold in snapshot.folds.items(&buffer) {
        let mut range = fold.range.0.clone();
        // anchor_after(start)：start 在编辑前不动；在编辑内塌到 edit.start；
        // 在编辑后平移
        range.start = if range.start < edit.start {
            range.start
        } else if range.start < edit.end {
            edit.start
        } else {
            (range.start as isize + delta).max(0) as usize
        };
        // anchor_before(end)：end 在编辑前不动；在编辑内塌到 edit.start；
        // 在编辑后平移
        range.end = if range.end <= edit.start {
            range.end
        } else if range.end <= edit.end {
            edit.start
        } else {
            (range.end as isize + delta).max(0) as usize
        };
        let fold = Fold {
            range: FoldRange(range),
            ..fold.clone()
        };
        new_tree.push(fold, &buffer);
    }
    snapshot.folds = new_tree;
}

// ---------- indent guides（zed multi_buffer.rs indent_guides_in_range 移植） ----------

/// zed text::LineIndent（text.rs:641 替身）：行首 tab/space 计数 + 空行标记。
pub(super) struct LineIndent {
    tabs: u32,
    spaces: u32,
    line_blank: bool,
}

impl LineIndent {
    fn is_line_blank(&self) -> bool {
        self.line_blank
    }

    /// zed LineIndent::len：缩进列数（tab 折算 tab_size 列）。
    fn len(&self, tab_size: u32) -> u32 {
        self.tabs * tab_size + self.spaces
    }
}

/// zed line_indents 的单 buffer 等价：行首缩进（LineIndent::from_chunks
/// 原样逻辑——遇首个非空白字符止）。
fn line_indent_for_row(buffer: &Rope, row: u32) -> LineIndent {
    let line = text_slice(
        buffer,
        buffer.point_to_offset(tree_sitter::Point::new(row as usize, 0))
            ..buffer.line_end_offset(row as usize),
    );
    let mut tabs = 0u32;
    let mut spaces = 0u32;
    let mut line_blank = true;
    for ch in line.chars() {
        if ch == '\t' {
            tabs += 1;
        } else if ch == ' ' {
            spaces += 1;
        } else {
            if ch != '\n' {
                line_blank = false;
            }
            break;
        }
    }
    LineIndent {
        tabs,
        spaces,
        line_blank,
    }
}

/// zed IndentGuide（multi_buffer.rs:1222 替身——单 buffer 无 buffer_id；
/// 线宽/色取主题默认，见 element 绘制）。
pub(super) struct IndentGuide {
    pub(super) start_row: u32,
    pub(super) end_row: u32,
    pub(super) depth: u32,
    pub(super) tab_size: u32,
}

/// zed MultiBufferSnapshot::indent_guides_in_range（multi_buffer.rs:6182）
/// 单 buffer 版，算法原样：逐行走栈；空白行向后搜非空行
/// （TRAILING_ROW_SEARCH_LIMIT = 25）；depth = 缩进列数 / tab_size；
/// 深度回落弹栈收口（空白行后搜到的结束行回退到 first_row-1）、加深压
/// 栈、栈内 guide 的 end_row 随行走推进。
pub(super) fn indent_guides_in_range(
    buffer: &Rope,
    range: Range<u32>,
    tab_size: u32,
) -> Vec<IndentGuide> {
    let max_row = buffer.lines_len().saturating_sub(1) as u32;
    let start_row = range.start;
    let end_row = range.end;

    let mut result = Vec::new();
    let mut indent_stack: Vec<IndentGuide> = Vec::new();

    let mut row = start_row;
    loop {
        if row > end_row || row > max_row {
            break;
        }
        let first_row = row;
        let mut line_indent = line_indent_for_row(buffer, row);
        let current_depth = indent_stack.len() as u32;

        // When encountering empty, continue until found useful line indent
        // then add to the indent stack with the depth found
        let mut found_indent = false;
        let mut last_row = first_row;
        if line_indent.is_line_blank() {
            while !found_indent {
                row += 1;
                if row > max_row {
                    break;
                }
                const TRAILING_ROW_SEARCH_LIMIT: u32 = 25;
                if row > end_row + TRAILING_ROW_SEARCH_LIMIT {
                    break;
                }

                let new_line_indent = line_indent_for_row(buffer, row);
                if new_line_indent.is_line_blank() {
                    continue;
                }
                last_row = row.min(end_row);
                line_indent = new_line_indent;
                found_indent = true;
                break;
            }
        } else {
            found_indent = true;
        }

        let depth = if found_indent {
            line_indent.len(tab_size) / tab_size
        } else {
            0
        };

        match depth.cmp(&current_depth) {
            std::cmp::Ordering::Less => {
                for _ in 0..(current_depth - depth) {
                    let mut indent = indent_stack.pop().expect("indent stack underflow");
                    if last_row != first_row {
                        // In this case, we landed on an empty row, had to seek forward,
                        // and discovered that the indent we where on is ending.
                        indent.end_row = first_row.saturating_sub(1);
                    }
                    result.push(indent);
                }
            }
            std::cmp::Ordering::Greater => {
                for next_depth in current_depth..depth {
                    indent_stack.push(IndentGuide {
                        start_row: first_row,
                        end_row: last_row,
                        depth: next_depth,
                        tab_size,
                    });
                }
            }
            _ => {}
        }

        for indent in indent_stack.iter_mut() {
            indent.end_row = last_row;
        }

        row += 1;
    }

    result.extend(indent_stack);
    result
}
