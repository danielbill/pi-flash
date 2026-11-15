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

use gpui::rgb;
use gpui_component::input::{DecorationProvider, Decorations, LineType, Rope, SectionFold};

/// md Live Preview 装饰 provider：每帧 prepaint 现算（纯函数，无状态、
/// 不落缓存——reveal 因此免费：selection 变了下帧自然拿到新 cursor）。
///
/// 挂在 `InputState::set_decorations`；源码态（eye 切换）摘掉即回原路径。
///
/// k34j 章节折叠：provider 只**声明**（`section_ranges`，全部 1..=5 可折
/// 章节），折叠态由 vendor fold 快照持有（`decorate` 的 `folded_sections`
/// 回读）——箭头 toggle/编辑 anchor 平移全在 vendor 侧，本模块每帧纯函数
/// 据快照折叠行取声明区间为生效集（与 rebuild 输入同帧同源）。
pub struct MdLiveProvider;

impl DecorationProvider for MdLiveProvider {
    fn decorate(
        &self,
        text: &Rope,
        visible_lines: Range<usize>,
        selection: Range<usize>,
        folded_sections: &[SectionFold],
    ) -> Option<Decorations> {
        if text.len() == 0 {
            return None;
        }
        let line_count = text.len_lines(LineType::LF);
        if visible_lines.start >= line_count {
            return None;
        }
        let src = text.to_string();
        let (spans, containers, line_scale, headings) = parse::parse(&src);
        let merged = fold::merge(parse::flat_folds(&containers));

        // P2 容器级 reveal（024）：折叠段与选区相交/接触即整容器显现
        //（`**粗体**` 整对标记同进退），其余容器保持折叠。章节段不参与
        // reveal——只随箭头/占位符点击 toggle（选区跨折叠段由快照原子性
        // 保证端点可见）。
        let selection = selection.start.min(text.len())..selection.end.min(text.len());
        let revealed = fold::reveal_containers(&containers, &merged, selection);

        // k34j 章节折叠：声明表（全部 1..=5 可折章节）+ 生效集（快照已折
        // 行 ∩ 声明表，区间取声明当前值——标题编辑后与 rebuild 输入同帧
        // 同源，无一帧漂移）
        let declared = fold::section_ranges(&src, &headings);
        let section_folds: Vec<SectionFold> = declared
            .iter()
            .filter(|d| folded_sections.iter().any(|f| f.row == d.row))
            .cloned()
            .collect();

        // 被生效章节吞掉的行内 fold 不产出（链上已并入章节段占位符；
        // 保留会让 element 行内对账每帧失配重建）
        let folds: Vec<Range<usize>> = revealed
            .into_iter()
            .filter(|f| {
                !section_folds
                    .iter()
                    .any(|s| s.range.start <= f.start && f.end <= s.range.end)
            })
            .collect();

        // 折叠后显示文本：行内段删除、章节段 → 占位符（与 fold 链空间
        // 严格一致——display 仅余横向最长行宽用途，行结构须对齐链行号）
        let display = fold::display_text(&src, &folds, &section_folds);

        // dqjd S3 契约 v2：styles = **绝对 vis 坐标**（从 doc 0 起）——
        // vendor 链按行 vis 区间直取切片，不再依赖视口切片基准（旧 base0
        // 相对坐标在滚动 + 折叠叠加时错位）。visible_lines 参数保留（签名
        // 兼容），样式计算不再用。
        let total = fold::doc_to_vis_u(&folds, &section_folds, text.len());

        // span → 绝对 vis 坐标 → 全覆盖分区 run
        let t = crate::appearance::theme::theme();
        let mut overlaps = Vec::with_capacity(spans.len());
        for s in &spans {
            let Some(hl) = crate::editor::markdown::highlight(s.style, t) else {
                continue;
            };
            let a = fold::doc_to_vis_u(&folds, &section_folds, s.range.start);
            let b = fold::doc_to_vis_u(&folds, &section_folds, s.range.end);
            if a < b && a < total {
                overlaps.push((a..b.min(total), hl));
            }
        }
        // 章节占位符灰显（k34j）：占位符 vis 区间 = 段首映射位 → +占位宽
        for s in &section_folds {
            let a = fold::doc_to_vis_u(&folds, &section_folds, s.range.start);
            let b = (a + s.collapsed_text.len()).min(total);
            if a < b {
                overlaps.push((
                    a..b,
                    gpui::HighlightStyle {
                        color: Some(rgb(t.text_muted).into()),
                        ..Default::default()
                    },
                ));
            }
        }
        let styles = style::partition(overlaps, total);

        Some(Decorations::new(
            Rope::from(display),
            styles,
            folds,
            section_folds,
            declared,
            line_scale,
        ))
    }
}
