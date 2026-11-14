//! pi-flash patch（移植）：WrapMap——zed crates/editor/src/display_map/
//! wrap_map.rs 移植（2026-11-14，beads pi-flash-dqjd S2）。
//!
//! 软换行层：transforms 树（isomorphic | wrap）把 tab 空间行映射为显示行。
//! wrap 变换 output = Point(1, indent)（合成换行 + 续行缩进，zed
//! Transform::wrap 同构——软换行的"换行"不在源文本里，由本层合成；续行
//! 缩进 = gpui LineWrapper Boundary.next_indent）。
//!
//! 与 zed 的偏差（有意为之，其余逐行同构）：
//! - **同步 rewrap**：zed 把大范围 rewrap 放后台任务、期间用 interpolate
//!   过渡快照（pending_edits 队列 + is_rewrapping）；PF 布局是同步管线，
//!   旧 TextWrapper 也是同步全量重建——沿用同步口径，pending/interpolate
//!   过渡态不移植（interpolate 本身保留：wrap_width=None 的直通路径用）。
//! - **不产 WrapPatch**：zed 用 patch 供滚动条/光标在 rewrap 窗口内换算；
//!   PF 消费方每帧重读快照，无 patch 消费点。
//! - 行文本直取：zed 经 TabChunks 位图迭代收行文本；PF tab 层是懒换算，
//!   `expand_row(row)` 直取展开行文本（同一数学，见 tab_map.rs）。

use std::ops::Range;

use gpui::{App, Font, LineFragment, Pixels};
use sum_tree::{Bias, Cursor, Dimensions, SumTree};

use super::fold_map::{Point as FoldSpacePoint, TextSummary};
use super::tab_map::{TabEdit, TabPoint, TabSnapshot};

/// zed WrapPoint（wrap_map.rs:86）：显示行 + 行内**显示字节列**（续行含
/// 缩进字节——wrap 变换 output 的缩进计入列）。
#[derive(Copy, Clone, Debug, Default, Eq, Ord, PartialOrd, PartialEq)]
pub(crate) struct WrapPoint(pub FoldSpacePoint);

impl WrapPoint {
    pub fn new(row: u32, column: u32) -> Self {
        Self(FoldSpacePoint::new(row, column))
    }

    pub fn row(self) -> u32 {
        self.0.row
    }

    pub fn column(self) -> u32 {
        self.0.column
    }

    pub fn column_mut(&mut self) -> &mut u32 {
        &mut self.0.column
    }
}

/// zed WrapEdit 的 PF 形态（不产 patch，仅 sync 内部行区间推导用）。
pub(crate) type WrapRowRange = Range<u32>;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct WTransform {
    summary: WTransformSummary,
    /// wrap 变换的显示文本（"\n" + indent 空格）；None = isomorphic。
    display_text: Option<&'static str>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct WTransformSummary {
    input: TextSummary,
    output: TextSummary,
}

impl WTransformSummary {
    fn has_wraps(&self) -> bool {
        self.input.lines != self.output.lines
    }
}

impl WTransform {
    fn isomorphic(summary: TextSummary) -> Self {
        Self {
            summary: WTransformSummary {
                input: summary.clone(),
                output: summary,
            },
            display_text: None,
        }
    }

    /// zed Transform::wrap（wrap_map.rs:1300）："\n" + MAX_INDENT 空格的
    /// 静态文本按 indent 截取；output = (1, indent)。
    fn wrap(indent: u32) -> Self {
        static WRAP_TEXT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        let wrap_text = WRAP_TEXT.get_or_init(|| {
            let mut wrap_text = String::new();
            wrap_text.push('\n');
            wrap_text.extend((0..gpui::LineWrapper::MAX_INDENT as usize).map(|_| ' '));
            wrap_text
        });

        Self {
            summary: WTransformSummary {
                input: TextSummary::default(),
                output: TextSummary {
                    len: 1 + indent as usize,
                    lines: FoldSpacePoint::new(1, indent),
                    first_line_chars: 0,
                    last_line_chars: indent,
                    longest_row: 1,
                },
            },
            display_text: Some(&wrap_text[..1 + indent as usize]),
        }
    }

    fn is_isomorphic(&self) -> bool {
        self.display_text.is_none()
    }
}

impl sum_tree::Item for WTransform {
    type Summary = WTransformSummary;

    fn summary(&self, _cx: ()) -> Self::Summary {
        self.summary.clone()
    }
}

impl sum_tree::ContextLessSummary for WTransformSummary {
    fn zero() -> Self {
        Default::default()
    }

    fn add_summary(&mut self, other: &Self) {
        self.input += other.input.clone();
        self.output += other.output.clone();
    }
}

impl<'a> sum_tree::Dimension<'a, WTransformSummary> for TabPoint {
    fn zero(_cx: ()) -> Self {
        Default::default()
    }

    fn add_summary(&mut self, summary: &'a WTransformSummary, _: ()) {
        self.0 += summary.input.lines;
    }
}

impl<'a> sum_tree::Dimension<'a, WTransformSummary> for WrapPoint {
    fn zero(_cx: ()) -> Self {
        Default::default()
    }

    fn add_summary(&mut self, summary: &'a WTransformSummary, _: ()) {
        self.0 += summary.output.lines;
    }
}

/// zed push_isomorphic（Vec 版，update 构建 edit_transforms 用）。
fn push_isomorphic(transforms: &mut Vec<WTransform>, summary: TextSummary) {
    if let Some(last_transform) = transforms.last_mut() {
        if last_transform.is_isomorphic() {
            last_transform.summary.input += summary.clone();
            last_transform.summary.output += summary;
            return;
        }
    }
    transforms.push(WTransform::isomorphic(summary));
}

/// zed SumTreeExt::push_or_extend：同构相邻合并。
trait SumTreeExt {
    fn push_or_extend(&mut self, transform: WTransform);
}

impl SumTreeExt for SumTree<WTransform> {
    fn push_or_extend(&mut self, transform: WTransform) {
        let mut transform = Some(transform);
        self.update_last(
            |last_transform| {
                if last_transform.is_isomorphic() && transform.as_ref().unwrap().is_isomorphic() {
                    let transform = transform.take().unwrap();
                    last_transform.summary.input += transform.summary.input;
                    last_transform.summary.output += transform.summary.output;
                }
            },
            (),
        );

        if let Some(transform) = transform {
            self.push(transform, ());
        }
    }
}

/// zed WrapMap。PF 无 Entity/异步：直接持有快照。
pub(crate) struct WrapMap {
    snapshot: WrapSnapshot,
    wrap_width: Option<Pixels>,
    font_with_size: (Font, Pixels),
}

impl WrapMap {
    pub fn new(
        tab_snapshot: TabSnapshot,
        font: Font,
        font_size: Pixels,
        wrap_width: Option<Pixels>,
        cx: &mut App,
    ) -> Self {
        let mut this = Self {
            font_with_size: (font, font_size),
            wrap_width: None,
            snapshot: WrapSnapshot::new(tab_snapshot),
        };
        this.set_wrap_width(wrap_width, cx);
        this
    }

    pub fn snapshot(&self) -> &WrapSnapshot {
        &self.snapshot
    }

    /// PF 拓展：整文重置（set_value 打开/重载路径）——直通快照 + 按当前
    /// 宽度 rewrap（旧 TextWrapper::reset 同位）。
    pub(crate) fn reset(&mut self, tab_snapshot: TabSnapshot, cx: &mut App) {
        self.snapshot = WrapSnapshot::new(tab_snapshot);
        self.rewrap(cx);
    }

    /// zed WrapMap::sync：编辑同步。wrap_width None → interpolate 直通；
    /// Some → 同步 rewrap 受影响行。
    pub fn sync(
        &mut self,
        tab_snapshot: TabSnapshot,
        tab_edits: Vec<TabEdit>,
        cx: &mut App,
    ) -> WrapSnapshot {
        match self.wrap_width {
            None => {
                self.snapshot.interpolate(tab_snapshot, &tab_edits);
            }
            Some(wrap_width) => {
                if !tab_edits.is_empty() || self.snapshot.tab_snapshot.version != tab_snapshot.version {
                    let mut line_wrapper = cx
                        .text_system()
                        .line_wrapper(self.font_with_size.0.clone(), self.font_with_size.1);
                    let mut snapshot = self.snapshot.clone();
                    snapshot.update(
                        tab_snapshot,
                        &tab_edits,
                        wrap_width,
                        &mut |line, width| {
                            line_wrapper
                                .wrap_line(&[LineFragment::text(line)], width)
                                .collect()
                        },
                    );
                    self.snapshot = snapshot;
                }
            }
        }
        self.snapshot.clone()
    }

    /// zed set_font_with_size：字体变化 → 全量 rewrap。
    pub fn set_font_with_size(&mut self, font: Font, font_size: Pixels, cx: &mut App) -> bool {
        let font_with_size = (font, font_size);
        if font_with_size == self.font_with_size {
            false
        } else {
            self.font_with_size = font_with_size;
            self.rewrap(cx);
            true
        }
    }

    /// zed set_wrap_width：宽度变化 → 全量 rewrap。
    pub fn set_wrap_width(&mut self, wrap_width: Option<Pixels>, cx: &mut App) -> bool {
        if wrap_width == self.wrap_width {
            return false;
        }
        self.wrap_width = wrap_width;
        self.rewrap(cx);
        true
    }

    /// zed rewrap（同步版）：整文按当前宽度重建（或直通重建）。
    fn rewrap(&mut self, cx: &mut App) {
        match self.wrap_width {
            Some(wrap_width) => {
                let tab_snapshot = self.snapshot.tab_snapshot.clone();
                let range = TabPoint::new(0, 0)..tab_snapshot.max_point();
                let tab_edits = vec![TabEdit {
                    old: range.clone(),
                    new: range,
                }];
                let mut line_wrapper = cx
                    .text_system()
                    .line_wrapper(self.font_with_size.0.clone(), self.font_with_size.1);
                let mut snapshot = self.snapshot.clone();
                snapshot.update(
                    tab_snapshot,
                    &tab_edits,
                    wrap_width,
                    &mut |line, width| {
                        line_wrapper
                            .wrap_line(&[LineFragment::text(line)], width)
                            .collect()
                    },
                );
                self.snapshot = snapshot;
            }
            None => {
                let tab_snapshot = self.snapshot.tab_snapshot.clone();
                self.snapshot = WrapSnapshot::new(tab_snapshot);
            }
        }
    }
}

/// zed WrapSnapshot。
#[derive(Clone)]
pub(crate) struct WrapSnapshot {
    pub tab_snapshot: TabSnapshot,
    transforms: SumTree<WTransform>,
}

impl std::ops::Deref for WrapSnapshot {
    type Target = TabSnapshot;

    fn deref(&self) -> &Self::Target {
        &self.tab_snapshot
    }
}

impl WrapSnapshot {
    /// zed WrapSnapshot::new：直通快照（单 isomorphic 或空）。摘要直取
    /// fold 树输出摘要（O(1)——tab 不改行数，longest_row 随 fold 摘要）。
    fn new(tab_snapshot: TabSnapshot) -> Self {
        let mut transforms = SumTree::default();
        let extent = tab_snapshot.fold_snapshot.text_summary();
        if extent.lines != FoldSpacePoint::default() {
            transforms.push(WTransform::isomorphic(extent), ());
        }
        Self {
            transforms,
            tab_snapshot,
        }
    }

    /// zed interpolate（wrap_map.rs:485）：tab 快照前进时按编辑区间搬运
    /// 旧 transforms（未 rewrap 前的直通过渡）。
    fn interpolate(&mut self, new_tab_snapshot: TabSnapshot, tab_edits: &[TabEdit]) {
        let mut new_transforms;
        if tab_edits.is_empty() {
            new_transforms = self.transforms.clone();
        } else if !self.transforms.summary().has_wraps()
            && new_tab_snapshot.max_point().0 != FoldSpacePoint::default()
        {
            // Fast path: without existing wraps, interpolation is a passthrough over the new tab snapshot.
            new_transforms = SumTree::default();
            new_transforms.push(WTransform::isomorphic(tab_text_summary_rows(
                &new_tab_snapshot,
                0..new_tab_snapshot.max_point().row() + 1,
            )), ());
        } else {
            let mut old_cursor = self.transforms.cursor::<TabPoint>(());

            let mut tab_edits_iter = tab_edits.iter().peekable();
            new_transforms =
                old_cursor.slice(&tab_edits_iter.peek().unwrap().old.start, Bias::Right);

            while let Some(edit) = tab_edits_iter.next() {
                if edit.new.start.0 > new_transforms.summary().input.lines {
                    let summary = tab_text_summary_rows(
                        &new_tab_snapshot,
                        new_transforms.summary().input.lines.row..edit.new.start.row(),
                    );
                    new_transforms.push_or_extend(WTransform::isomorphic(summary));
                }

                if edit.new.start < edit.new.end {
                    let summary = tab_text_summary_rows(
                        &new_tab_snapshot,
                        edit.new.start.row()..edit.new.end.row() + 1,
                    );
                    new_transforms.push_or_extend(WTransform::isomorphic(summary));
                }

                old_cursor.seek_forward(&edit.old.end, Bias::Right);
                if let Some(next_edit) = tab_edits_iter.peek() {
                    if next_edit.old.start > old_cursor.end() {
                        if old_cursor.end().0 > edit.old.end.0 {
                            let summary = tab_text_summary_rows(
                                &self.tab_snapshot,
                                edit.old.end.row()..old_cursor.end().row(),
                            );
                            new_transforms.push_or_extend(WTransform::isomorphic(summary));
                        }

                        old_cursor.next();
                        new_transforms.append(
                            old_cursor.slice(&next_edit.old.start, Bias::Right),
                            (),
                        );
                    }
                } else {
                    if old_cursor.end().0 > edit.old.end.0 {
                        let summary = tab_text_summary_rows(
                            &self.tab_snapshot,
                            edit.old.end.row()..old_cursor.end().row(),
                        );
                        new_transforms.push_or_extend(WTransform::isomorphic(summary));
                    }
                    old_cursor.next();
                    new_transforms.append(old_cursor.suffix(), ());
                }
            }
        }

        self.transforms = new_transforms;
        self.tab_snapshot = new_tab_snapshot;
    }

    /// zed WrapSnapshot::update（wrap_map.rs:556，同步版）：按行区间编辑
    /// 重建 transforms；行文本 = tab 展开行 + 换行，wrap_line 出边界后
    /// iso/wrap 交替入树。
    pub(crate) fn update<F>(
        &mut self,
        new_tab_snapshot: TabSnapshot,
        tab_edits: &[TabEdit],
        wrap_width: Pixels,
        wrap_line: &mut F,
    ) where
        F: FnMut(&str, Pixels) -> Vec<gpui::Boundary>,
    {
        struct RowEdit {
            old_rows: Range<u32>,
            new_rows: Range<u32>,
        }

        let mut tab_edits_iter = tab_edits.iter().peekable();
        let mut row_edits = Vec::with_capacity(tab_edits.len());
        while let Some(edit) = tab_edits_iter.next() {
            let mut row_edit = RowEdit {
                old_rows: edit.old.start.row()..edit.old.end.row() + 1,
                new_rows: edit.new.start.row()..edit.new.end.row() + 1,
            };

            while let Some(next_edit) = tab_edits_iter.peek() {
                if next_edit.old.start.row() <= row_edit.old_rows.end {
                    row_edit.old_rows.end = row_edit.old_rows.end.max(next_edit.old.end.row() + 1);
                    row_edit.new_rows.end = row_edit.new_rows.end.max(next_edit.new.end.row() + 1);
                    tab_edits_iter.next();
                } else {
                    break;
                }
            }

            row_edits.push(row_edit);
        }

        let new_max_row = new_tab_snapshot.max_point().row();
        let mut new_transforms;
        if row_edits.is_empty() {
            new_transforms = self.transforms.clone();
        } else {
            let mut row_edits = row_edits.into_iter().peekable();
            let mut old_cursor = self.transforms.cursor::<TabPoint>(());

            new_transforms = old_cursor.slice(
                &TabPoint::new(row_edits.peek().unwrap().old_rows.start, 0),
                Bias::Right,
            );

            while let Some(edit) = row_edits.next() {
                if edit.new_rows.start > new_transforms.summary().input.lines.row {
                    let summary = tab_text_summary_rows(
                        &new_tab_snapshot,
                        new_transforms.summary().input.lines.row..edit.new_rows.start,
                    );
                    new_transforms.push_or_extend(WTransform::isomorphic(summary));
                }

                let mut edit_transforms = Vec::<WTransform>::new();
                for row in edit.new_rows.start..edit.new_rows.end.min(new_max_row + 1) {
                    // 行文本 = tab 展开行 + 换行（末行无换行——zed chunks
                    // 收到 EOF 同形；空末行 line 为空跳过，输入维度由位置
                    // 覆盖（空行 0 字节，Point 已达 max））
                    let expanded = new_tab_snapshot.expand_row(row).text;
                    let line = if row < new_max_row {
                        format!("{}\n", expanded)
                    } else {
                        expanded
                    };
                    if line.is_empty() {
                        break;
                    }

                    let mut prev_boundary_ix = 0;
                    for boundary in wrap_line(&line, wrap_width) {
                        let wrapped = &line[prev_boundary_ix..boundary.ix];
                        push_isomorphic(
                            &mut edit_transforms,
                            TextSummary::from_str(wrapped),
                        );
                        edit_transforms.push(WTransform::wrap(boundary.next_indent));
                        prev_boundary_ix = boundary.ix;
                    }

                    if prev_boundary_ix < line.len() {
                        push_isomorphic(
                            &mut edit_transforms,
                            TextSummary::from_str(&line[prev_boundary_ix..]),
                        );
                    }
                }

                let mut edit_transforms = edit_transforms.into_iter();
                if let Some(transform) = edit_transforms.next() {
                    new_transforms.push_or_extend(transform);
                }
                new_transforms.extend(edit_transforms, ());

                old_cursor.seek_forward(&TabPoint::new(edit.old_rows.end, 0), Bias::Right);
                if let Some(next_edit) = row_edits.peek() {
                    if next_edit.old_rows.start > old_cursor.end().row() {
                        if old_cursor.end() > TabPoint::new(edit.old_rows.end, 0) {
                            let summary = tab_text_summary_rows(
                                &self.tab_snapshot,
                                edit.old_rows.end..old_cursor.end().row(),
                            );
                            new_transforms.push_or_extend(WTransform::isomorphic(summary));
                        }
                        old_cursor.next();
                        new_transforms.append(
                            old_cursor
                                .slice(&TabPoint::new(next_edit.old_rows.start, 0), Bias::Right),
                            (),
                        );
                    }
                } else {
                    if old_cursor.end() > TabPoint::new(edit.old_rows.end, 0) {
                        let summary = tab_text_summary_rows(
                            &self.tab_snapshot,
                            edit.old_rows.end..old_cursor.end().row(),
                        );
                        new_transforms.push_or_extend(WTransform::isomorphic(summary));
                    }
                    old_cursor.next();
                    new_transforms.append(old_cursor.suffix(), ());
                }
            }
        }

        self.transforms = new_transforms;
        self.tab_snapshot = new_tab_snapshot;
    }

    // ---- 换算（zed WrapSnapshot 同名方法移植） ----

    /// zed max_point。
    pub fn max_point(&self) -> WrapPoint {
        WrapPoint(self.transforms.summary().output.lines)
    }

    /// zed to_tab_point：显示点 → tab 点（wrap 变换内 = 贴边界）。
    pub fn to_tab_point(&self, point: WrapPoint) -> TabPoint {
        let (start, _, item) = self
            .transforms
            .find::<Dimensions<WrapPoint, TabPoint>, _>((), &point, Bias::Right);
        let mut tab_point = start.1.0;
        if item.is_some_and(|t| t.is_isomorphic()) {
            tab_point += point.0 - start.0.0;
        }
        TabPoint(tab_point)
    }

    /// zed tab_point_to_wrap_point：tab 点 → 显示点（列含续行缩进）。
    pub fn tab_point_to_wrap_point(&self, point: TabPoint) -> WrapPoint {
        let (start, ..) =
            self.transforms
                .find::<Dimensions<TabPoint, WrapPoint>, _>((), &point, Bias::Right);
        WrapPoint(start.1.0 + (point.0 - start.0.0))
    }

    /// zed to_point：显示点 → buffer 点。
    pub fn to_point(&self, point: WrapPoint, bias: Bias) -> FoldSpacePoint {
        self.tab_snapshot.tab_point_to_point(self.to_tab_point(point), bias)
    }

    /// zed make_wrap_point：buffer 点 → 显示点。
    pub fn make_wrap_point(&self, point: FoldSpacePoint, bias: Bias) -> WrapPoint {
        self.tab_point_to_wrap_point(self.tab_snapshot.point_to_tab_point(point, bias))
    }

    /// zed clip_point（wrap_map.rs:969）：wrap 变换内 Left 贴段首（续行
    /// 缩进区不可停靠），再经 tab 层钳制（折叠/行尾）。
    pub fn clip_point(&self, mut point: WrapPoint, bias: Bias) -> WrapPoint {
        if bias == Bias::Left {
            let (start, _, item) = self
                .transforms
                .find::<WrapPoint, _>((), &point, Bias::Right);
            if item.is_some_and(|t| !t.is_isomorphic()) {
                point = start;
                *point.column_mut() -= 1;
            }
        }

        self.tab_point_to_wrap_point(self.tab_snapshot.clip_point(self.to_tab_point(point), bias))
    }

    /// zed line_len（wrap_map.rs:802）：显示行字节长（续行含缩进）。
    pub fn line_len(&self, row: u32) -> u32 {
        let (start, _, item) = self.transforms.find::<Dimensions<WrapPoint, TabPoint>, _>(
            (),
            &WrapPoint::new(row + 1, 0),
            Bias::Left,
        );
        if item.is_some_and(|transform| transform.is_isomorphic()) {
            let overshoot = row - start.0.row();
            let tab_row = start.1.row() + overshoot;
            let tab_line_len = self.tab_snapshot.line_len(tab_row);
            if overshoot == 0 {
                start.0.column() + (tab_line_len - start.1.column())
            } else {
                tab_line_len
            }
        } else {
            start.0.column()
        }
    }

    /// zed soft_wrap_indent：续行缩进（None = 非续行）。
    pub fn soft_wrap_indent(&self, row: u32) -> Option<u32> {
        let (.., item) = self
            .transforms
            .find::<WrapPoint, _>((), &WrapPoint::new(row + 1, 0), Bias::Right);
        item.and_then(|transform| {
            if transform.is_isomorphic() {
                None
            } else {
                Some(transform.summary.output.lines.column)
            }
        })
    }

    /// zed longest_row（summary 输出；行号索引）。
    pub fn longest_row(&self) -> u32 {
        self.transforms.summary().output.longest_row
    }

    /// zed prev_row_boundary（wrap_map.rs:986）：显示点所在/之前的最近
    /// tab 行边界（Home：软换行回退到 tab 行首——zed stop_at_soft_wraps
    /// 默认停显示行首；本方法给「跳到 tab 行首」语义用）。
    pub fn prev_row_boundary(&self, point: WrapPoint) -> u32 {
        if self.transforms.is_empty() {
            return 0;
        }

        let point = WrapPoint::new(point.row(), 0);

        let mut cursor = self
            .transforms
            .cursor::<Dimensions<WrapPoint, TabPoint>>(());

        cursor.seek(&point, Bias::Right);
        if cursor.item().is_none() {
            cursor.prev();
        }

        while let Some(transform) = cursor.item() {
            if transform.is_isomorphic() {
                let tab_summary = &transform.summary.input;
                if tab_summary.lines.row > 1 {
                    let wrap_point_at_end = cursor.end().0.row();
                    return wrap_point_at_end.saturating_sub(1).min(point.row());
                } else if cursor.start().1.column() == 0 {
                    return cursor.end().0.row().min(point.row());
                }
            }

            cursor.prev();
        }

        0
    }

    /// zed next_row_boundary：显示点之后的最近 tab 行边界。
    pub fn next_row_boundary(&self, mut point: WrapPoint) -> Option<u32> {
        point.0 += FoldSpacePoint::new(1, 0);

        let mut cursor = self
            .transforms
            .cursor::<Dimensions<WrapPoint, TabPoint>>(());
        cursor.seek(&point, Bias::Right);
        while let Some(transform) = cursor.item() {
            if transform.is_isomorphic() && cursor.start().1.column() == 0 {
                return Some(cursor.start().0.row().max(point.row()));
            } else {
                cursor.next();
            }
        }

        None
    }

    // ---- PF 渲染消费（zed 由 WrapChunks 管线提供，PF 按显示行直取） ----

    /// 显示行 → 结构行信息：tab 行号（= fold 行）、段区间（tab 展开字节）、
    /// 续行缩进、是否 tab 行首段。
    pub(crate) fn row_info(&self, row: u32) -> DisplayRowInfo {
        let mut cursor = self
            .transforms
            .cursor::<Dimensions<WrapPoint, TabPoint>>(());
        // Bias::Left（zed row_infos 同款）：行首点落在 wrap 变换输出区间内
        // （含 indent=0 的边界）判为续行；落在 iso 内才是行首段。
        cursor.seek(&WrapPoint::new(row, 0), Bias::Left);

        let (tab_row, seg_start, indent, is_row_start) = match cursor.item() {
            Some(transform) if transform.is_isomorphic() => {
                // 显示行 = 整 tab 行（同构段内无 wrap——树里 iso/wrap 交替）
                let tab_row = cursor.start().1.row() + (row - cursor.start().0.row());
                (tab_row, 0, 0, true)
            }
            Some(transform) => {
                // wrap 变换：段起 = 变换输入终点（tab 边界），缩进 = output 列
                let end = cursor.end().1;
                (
                    end.row(),
                    end.column(),
                    transform.summary.output.lines.column,
                    false,
                )
            }
            None => {
                // 越界（行号超出 max）：钳到末 tab 行行首
                let max = self.max_point();
                let tab_max = self.tab_snapshot.max_point();
                if max.0 > FoldSpacePoint::default() && row > max.row() {
                    return DisplayRowInfo {
                        wrap_row: row,
                        tab_row: tab_max.row(),
                        seg_start: 0,
                        seg_end: self.tab_snapshot.line_len(tab_max.row()),
                        indent: 0,
                        is_row_start: true,
                    };
                }
                (tab_max.row(), 0, 0, true)
            }
        };

        // 段终：同 tab 行内下一个 wrap 变换的输入起点；否则 tab 行尾
        let seg_end = if is_row_start {
            // 向前找本 tab 行内的第一个 wrap 变换
            let mut probe = cursor.clone();
            let mut end = self.tab_snapshot.line_len(tab_row);
            while let Some(t) = probe.item() {
                if !t.is_isomorphic() {
                    end = probe.start().1.column();
                    break;
                }
                if probe.end().1.row() > tab_row {
                    break;
                }
                probe.next();
            }
            end
        } else {
            // 续行段：下一变换若是 wrap（同 tab 行的下一段边界）取其起点；
            // 否则行尾。续行段后必是 iso（先走完本段）——从 cursor 前进到
            // 段 iso 的终点查。
            let mut probe = cursor;
            // 跳过当前 wrap 变换
            probe.next();
            let mut end = self.tab_snapshot.line_len(tab_row);
            while let Some(t) = probe.item() {
                if !t.is_isomorphic() {
                    end = probe.start().1.column();
                    break;
                }
                if probe.end().1.row() > tab_row {
                    break;
                }
                probe.next();
            }
            end
        };

        DisplayRowInfo {
            wrap_row: row,
            tab_row,
            seg_start,
            seg_end,
            indent,
            is_row_start,
        }
    }

    /// tab 空间总摘要（行区间直取行文本拼接；gap/全量重建用，区间通常小）。
    pub fn text_summary_for_rows(&self, rows: Range<u32>) -> TextSummary {
        tab_text_summary_rows(&self.tab_snapshot, rows)
    }
}

/// 渲染层显示行信息（PF 消费形状；zed RowInfo 的段级展开）。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DisplayRowInfo {
    /// 显示行号（wrap 行）。
    pub wrap_row: u32,
    /// 结构行号（tab 行 = fold 行）。
    pub tab_row: u32,
    /// 段起点（tab 展开字节，行内）。
    pub seg_start: u32,
    /// 段终点（tab 展开字节，行内；不含合成换行）。
    pub seg_end: u32,
    /// 续行缩进（显示字节；首段 = 0）。
    pub indent: u32,
    /// 是否 tab 行首段（行号栏显示缓冲行号的位置）。
    pub is_row_start: bool,
}

/// tab 空间行区间摘要（行文本拼接 from_str；区间通常为编辑邻域）。
fn tab_text_summary_rows(tab: &TabSnapshot, rows: Range<u32>) -> TextSummary {
    if rows.start >= rows.end {
        return TextSummary::default();
    }
    let max_row = tab.max_point().row();
    let end = rows.end.min(max_row + 1);
    if rows.start >= end {
        return TextSummary::default();
    }
    let mut text = String::new();
    for row in rows.start..end {
        text.push_str(&tab.expand_row(row).text);
        if row < max_row {
            text.push('\n');
        }
    }
    TextSummary::from_str(&text)
}

// ---------- 测试（构树直测 + 假 wrap_line 闭包，无文本系统依赖） ----------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::fold_map::{FoldMap, InlaySnapshot};
    use crate::input::tab_map::TabMap;
    use gpui::px;
    use std::num::NonZeroU32;

    fn tab_snap(text: &str) -> TabSnapshot {
        let (_, fold) = FoldMap::new(InlaySnapshot {
            buffer: ropey::Rope::from(text),
            version: 0,
        });
        TabSnapshot::new(fold, NonZeroU32::new(4).unwrap())
    }

    /// 构造带 wrap 的快照：每行在 col=2 处折、续行缩进 2（iso/wrap 交替）。
    fn wrap_at(ws: &mut WrapSnapshot, rows: Range<u32>, col: u32, indent: u32) {
        // 重建 transforms：逐行拆 iso(iso 前缀) wrap(iso 尾含 \n)
        let tab = ws.tab_snapshot.clone();
        let mut tree: SumTree<WTransform> = SumTree::default();
        let max_row = tab.max_point().row();
        for row in 0..=max_row {
            let text = tab.expand_row(row).text;
            let line = if row < max_row {
                format!("{}\n", text)
            } else {
                text
            };
            if rows.contains(&row) && line.len() as u32 > col {
                tree.push_or_extend(WTransform::isomorphic(TextSummary::from_str(
                    &line[..col as usize],
                )));
                tree.push(WTransform::wrap(indent), ());
                tree.push_or_extend(WTransform::isomorphic(TextSummary::from_str(
                    &line[col as usize..],
                )));
            } else if !line.is_empty() {
                tree.push_or_extend(WTransform::isomorphic(TextSummary::from_str(&line)));
            }
        }
        ws.transforms = tree;
    }

    /// 单行 "hello world"（11 字节），col 5 折、缩进 2：
    /// 显示两行——"hello" / "␣␣world"。
    #[test]
    fn test_wrap_conversions() {
        let mut ws = WrapSnapshot::new(tab_snap("hello world"));
        wrap_at(&mut ws, 0..1, 5, 2);

        assert_eq!(ws.max_point().row(), 1);
        // 行长：首行 5；续行 = 缩进 2 + 段 6 = 8
        assert_eq!(ws.line_len(0), 5);
        assert_eq!(ws.line_len(1), 8);
        // soft_wrap_indent（zed 语义：find(row+1)——返回 row+1 行的缩进）
        assert_eq!(ws.soft_wrap_indent(0), Some(2));
        assert_eq!(ws.soft_wrap_indent(1), None);

        // buffer 点 (0,3) → wrap (0,3)；(0,7) → 续行 (1, 2+2=4)
        assert_eq!(
            ws.make_wrap_point(FoldSpacePoint::new(0, 3), Bias::Right),
            WrapPoint::new(0, 3)
        );
        assert_eq!(
            ws.make_wrap_point(FoldSpacePoint::new(0, 7), Bias::Right),
            WrapPoint::new(1, 4)
        );
        // 反算：wrap (1,4) → tab (0,7)；wrap (0,3) → tab (0,3)
        assert_eq!(ws.to_tab_point(WrapPoint::new(1, 4)), TabPoint::new(0, 7));
        assert_eq!(ws.to_tab_point(WrapPoint::new(0, 3)), TabPoint::new(0, 3));
        // 续行缩进区内（col < indent）→ 贴段边界（tab (0,5)）
        assert_eq!(ws.to_tab_point(WrapPoint::new(1, 1)), TabPoint::new(0, 5));
        // round trip
        for col in [0u32, 3, 5, 7, 10] {
            let wp = ws.make_wrap_point(FoldSpacePoint::new(0, col), Bias::Right);
            let back = ws.to_tab_point(wp);
            let bp = ws.tab_point_to_point(back, Bias::Right);
            assert_eq!(bp, FoldSpacePoint::new(0, col), "col {col}");
        }
    }

    /// clip_point：续行缩进区 Left 贴上一显示行末字符（zed clip 同语义）。
    #[test]
    fn test_wrap_clip() {
        let mut ws = WrapSnapshot::new(tab_snap("hello world"));
        wrap_at(&mut ws, 0..1, 5, 2);
        // (1,0)（缩进区内）Left → (0, 4)（首行末字符）
        let clipped = ws.clip_point(WrapPoint::new(1, 0), Bias::Left);
        assert_eq!(clipped, WrapPoint::new(0, 4));
        // (1,1)（缩进区内）Left → (0,4)
        let clipped = ws.clip_point(WrapPoint::new(1, 1), Bias::Left);
        assert_eq!(clipped, WrapPoint::new(0, 4));
        // (1,3)（段内容内，过缩进）Left → 原位不动
        let clipped = ws.clip_point(WrapPoint::new(1, 3), Bias::Left);
        assert_eq!(clipped, WrapPoint::new(1, 3));
    }

    /// row_info：续行段（tab_row/seg_start/indent/is_row_start）。
    #[test]
    fn test_row_info() {
        let mut ws = WrapSnapshot::new(tab_snap("hello world"));
        wrap_at(&mut ws, 0..1, 5, 2);
        let first = ws.row_info(0);
        assert_eq!(
            (first.tab_row, first.seg_start, first.seg_end, first.indent, first.is_row_start),
            (0, 0, 5, 0, true)
        );
        let cont = ws.row_info(1);
        assert_eq!(
            (cont.tab_row, cont.seg_start, cont.seg_end, cont.indent, cont.is_row_start),
            (0, 5, 11, 2, false)
        );
    }

    /// update()：假 wrap_line（每行超 2 字节即在 2 处折、缩进 0）——三行文
    /// 本全量重排后 max_point/换算自洽。
    #[test]
    fn test_update_fake_wrapper() {
        let mut ws = WrapSnapshot::new(tab_snap("aaaa\nbbbb\ncccc\n"));
        let tab = ws.tab_snapshot.clone();
        let range = TabPoint::new(0, 0)..tab.max_point();
        let tab_edits = vec![TabEdit {
            old: range.clone(),
            new: range,
        }];
        ws.update(
            tab,
            &tab_edits,
            px(40.),
            &mut |line: &str, _w| {
                if line.len() > 2 {
                    vec![gpui::Boundary {
                        ix: 2,
                        next_indent: 0,
                    }]
                } else {
                    vec![]
                }
            },
        );
        // 3 行 × 2 段 = 6 显示行；max_point = 终止位置（末行尾换行后 = 行 6）
        assert_eq!(ws.max_point().row(), 6);
        // row_info：偶数行 = 行首段，奇数行 = 续行
        for row in 0..6u32 {
            let info = ws.row_info(row);
            assert_eq!(info.is_row_start, row % 2 == 0, "row {row}");
            assert_eq!(info.tab_row, row / 2);
        }
        // round trip：buffer (1,1)（"bbbb" col1）→ 显示 (2,1)（tab 行 1 的
        // 首段——每行都在 col2 折，tab 行 1 的段占显示行 2/3）
        let wp = ws.make_wrap_point(FoldSpacePoint::new(1, 1), Bias::Right);
        assert_eq!(wp, WrapPoint::new(2, 1));
        // buffer (1,3) → 续行 (3,1)
        let wp = ws.make_wrap_point(FoldSpacePoint::new(1, 3), Bias::Right);
        assert_eq!(wp, WrapPoint::new(3, 1));
        let off = ws.to_point(WrapPoint::new(3, 1), Bias::Right);
        assert_eq!(off, FoldSpacePoint::new(1, 3));
    }

    /// dqjd S2 回归：完整 sync 管线（fold writer → tab sync → wrap update）
    /// 后换算不变——折叠/tab/wrap 三层编辑同步收敛（pi-flash-dqjd）。
    #[test]
    fn test_fold_sync_pipeline() {
        let text = r#"fn main() {
    let alpha = 1;
    let beta = 2;
    if alpha < beta {
        println!("{} {}", alpha, beta);
    }
    let gamma = 3;
}
tabbed	line here
fn second() {
    println!("second");
}
"#;
        let rope = ropey::Rope::from(text);
        let nz = NonZeroU32::new(2).unwrap();
        let (mut fold_map, fold_snap0) = FoldMap::new(InlaySnapshot { buffer: rope.clone(), version: 0 });
        let mut tab_map = TabMap(TabSnapshot::new(fold_snap0.clone(), nz));
        let mut ws = WrapSnapshot::new(tab_map.0.clone());
        {
            let inlay = InlaySnapshot { buffer: rope.clone(), version: 1 };
            let (mut writer, _s, _e) = fold_map.write(inlay, Vec::new());
            let (_fs, fold_edits) = writer.fold(vec![(11..136, Default::default())]);
            let (fold_snap1, _) = fold_map.read(InlaySnapshot { buffer: rope.clone(), version: 1 }, Vec::new());
            let (tab_snap1, tab_edits) = tab_map.sync(fold_snap1.clone(), fold_edits, nz);
            // 同步管线关键不变量：tab 编辑 old 端跨被折叠的行区间
            assert_eq!(tab_edits.len(), 1);
            assert_eq!(tab_edits[0].old.start.row(), 0);
            assert_eq!(tab_edits[0].old.end.row(), 7);
            assert_eq!(tab_edits[0].new.start.row(), 0);
            assert_eq!(tab_edits[0].new.end.row(), 0);
            ws.update(
                tab_snap1,
                &tab_edits,
                gpui::px(4000.),
                &mut |line: &str, _w| vec![],
            );
        }
        assert_eq!(ws.max_point().row(), 5);
        let expect = [0usize, 138, 155, 169, 193, 195];
        for (row, &want) in expect.iter().enumerate() {
            let tp = ws.to_tab_point(WrapPoint::new(row as u32, 0));
            let (fp, _, _) = ws.tab_snapshot.tab_point_to_fold_point(tp, Bias::Right);
            let off = fp.to_offset(&ws.tab_snapshot.fold_snapshot);
            assert_eq!(
                ws.tab_snapshot.fold_snapshot.fold_offset_to_buffer(off),
                want,
                "display row {row}",
            );
        }
    }

    /// dqjd S2 复现：折叠态显示行 → buffer 换算定位（fold→tab→wrap 直通）。
    #[test]
    fn repro_fold_display_map() {
        let text = r#"fn main() {
    let alpha = 1;
    let beta = 2;
    if alpha < beta {
        println!("{} {}", alpha, beta);
    }
    let gamma = 3;
}
tabbed	line here
fn second() {
    println!("second");
}
"#;
        let rope = ropey::Rope::from(text);
        eprintln!("text len = {}", rope.len());

        let (mut fold_map, _init) = FoldMap::new(InlaySnapshot { buffer: rope.clone(), version: 0 });
        {
            let inlay = InlaySnapshot { buffer: rope.clone(), version: 1 };
            let (mut writer, _s, _e) = fold_map.write(inlay, Vec::new());
            let (_fs, edits) = writer.fold(vec![(11..136, Default::default())]);
            eprintln!("fold edits = {:?}", edits.iter().map(|e| (e.old.start.0, e.old.end.0, e.new.start.0, e.new.end.0)).collect::<Vec<_>>());
        }
        let (fold_snap, _) = fold_map.read(InlaySnapshot { buffer: rope.clone(), version: 1 }, Vec::new());
        eprintln!("fold max_point = {:?}", fold_snap.max_point());
        for row in 0..=fold_snap.max_point().row() {
            eprintln!("fold row {} -> buffer row {}", row, fold_snap.row_to_buffer_row(row));
        }

        let tab = TabSnapshot::new(fold_snap.clone(), NonZeroU32::new(2).unwrap());
        eprintln!("tab max_point = {:?}", tab.max_point());
        let ws = WrapSnapshot::new(tab.clone());
        eprintln!("wrap max_point = {:?}", ws.max_point());
        for row in 0..=6u32 {
            let tp = ws.to_tab_point(WrapPoint::new(row, 0));
            let (fp, _, _) = tab.tab_point_to_fold_point(tp, Bias::Right);
            let off = fp.to_offset(&tab.fold_snapshot);
            let buf = tab.fold_snapshot.fold_offset_to_buffer(off);
            eprintln!("display row {}: tab={:?} fold={:?} fold_off={:?} buffer={}", row, tp, fp, off, buf);
        }
    }

    /// interpolate 直通（无 wrap）：编辑后行数跟随 tab 快照。
    #[test]
    fn test_interpolate_passthrough() {
        let mut ws = WrapSnapshot::new(tab_snap("a\nb\n"));
        assert_eq!(ws.max_point().row(), 2);
        let new_tab = tab_snap("a\nb\nc\nd\n");
        // 末尾追加两行：tab 编辑 old (0,0)..(2,0) → new (0,0)..(4,0)
        ws.interpolate(
            new_tab,
            &[TabEdit {
                old: TabPoint::new(0, 0)..TabPoint::new(2, 0),
                new: TabPoint::new(0, 0)..TabPoint::new(4, 0),
            }],
        );
        assert_eq!(ws.max_point().row(), 4);
        // 直通：wrap 行 == tab 行
        for row in 0..4u32 {
            assert_eq!(ws.to_tab_point(WrapPoint::new(row, 0)), TabPoint::new(row, 0));
        }
    }
}
