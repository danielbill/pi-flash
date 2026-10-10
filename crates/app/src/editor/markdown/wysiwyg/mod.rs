//! Markdown 所见即所得（Live Preview）：Obsidian 式单视图编辑——正文即渲染、
//! 语法字符隐藏（零宽折叠）、光标进入哪段哪段语法显现。
//!
//! 铁律：**rope 源码是文档本体，唯一真相；本模块只产视图装饰，从不改写
//! 文档**。复制/撤销/持久化天然正确。
//!
//! 数据流（每帧 prepaint，O(可见行)，纯函数无状态）：
//!
//! ```text
//! rope → parse.rs (pulldown-cmark) → LineModel[]（行样式 + span + folds）
//!   → fold.rs FoldSet（doc↔vis 映射 + atomic + reveal）
//!   → vendor 三缝：layout_lines（折叠后 shaping）/ highlight_lines（样式 run）
//!     / layout_cursor + movement（光标经 FoldSet 换算）
//! ```
//!
//! 设计文档：`docs/模块设计/024-Markdown所见即所得.md`（分期 P0-P3、
//! vendor 补丁点、编辑语义决策表、风险全在彼处）。
//!
//! vendor 缝：`gpui_component::input::{DecorationProvider, Decorations}`
//! （trait/结果类型定义在 vendor，app 实现——app 依赖 vendor，反向不可）。

pub mod fold;
pub mod parse;
pub mod style;
pub mod widget;

use std::ops::Range;

use gpui_component::input::{Decorations, DecorationProvider, LineType, Rope, RopeExt as _};

/// md Live Preview 装饰 provider：每帧 prepaint 现算（纯函数，无状态、
/// 不落缓存——reveal 因此免费：selection 变了下帧自然拿到新 cursor）。
///
/// 挂在 `InputState::set_decorations`；源码态（eye 切换）摘掉即回原路径。
pub struct MdLiveProvider;

impl DecorationProvider for MdLiveProvider {
    fn decorate(
        &self,
        text: &Rope,
        visible_lines: Range<usize>,
        cursor: usize,
    ) -> Option<Decorations> {
        if text.len() == 0 {
            return None;
        }
        let line_count = text.len_lines(LineType::LF);
        if visible_lines.start >= line_count {
            return None;
        }
        let src = text.to_string();
        let (spans, raw_folds) = parse::parse(&src);
        let merged = fold::merge(raw_folds);

        // P0 行级 reveal（024 §6.2）：光标所在行 folds 全部显现，其余行隐藏
        let cursor = cursor.min(text.len());
        let row = text.offset_to_point(cursor).row;
        let line = text.line_start_offset(row)..text.line_end_offset(row);
        let folds = fold::reveal_line(&merged, line);

        // 折叠后显示文本：folds 不含 \n（parse 保证），行数严格一致
        let mut display = src;
        for f in folds.iter().rev() {
            display.replace_range(f.clone(), "");
        }

        // 可见切片基准——与 element.rs 的 slice_lines(vr.start..vr.end) 同式
        let slice_start = text.line_start_offset(visible_lines.start);
        let end_row = visible_lines.end.saturating_sub(1).min(line_count - 1);
        let slice_end = text.line_end_offset(end_row);
        let base0 = fold::doc_to_vis(&folds, slice_start);
        let total = fold::doc_to_vis(&folds, slice_end).saturating_sub(base0);

        // span → 折叠坐标系（可见切片为原点）→ 全覆盖分区 run
        let t = crate::theme::theme();
        let mut overlaps = Vec::with_capacity(spans.len());
        for s in &spans {
            let Some(hl) = crate::editor::markdown::highlight(s.style, t) else {
                continue;
            };
            let a = fold::doc_to_vis(&folds, s.range.start).saturating_sub(base0);
            let b = fold::doc_to_vis(&folds, s.range.end).saturating_sub(base0);
            if a < b && a < total {
                overlaps.push((a..b.min(total), hl));
            }
        }
        let styles = style::partition(overlaps, total);

        Some(Decorations::new(Rope::from(display), styles, folds))
    }
}
