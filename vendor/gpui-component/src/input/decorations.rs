//! PF-024 补丁：Markdown 所见即所得（Live Preview）装饰缝。
//!
//! 升级 gpui-component 时需重放（同 IME/剪贴板/screenshot 补丁惯例）。
//! 架构铁律见 `docs/模块设计/024-Markdown所见即所得.md`：
//! **文档本体永远是 rope 源码，装饰是每帧纯函数的视图变换，从不反写文档**。
//!
//! - [`DecorationProvider`]：app 侧实现（`editor::markdown::wysiwyg::MdLiveProvider`），
//!   vendor 只通过 trait 调用，避免反向依赖
//! - [`Decorations`]：单帧装饰结果——折叠后显示文本 + 可见区样式（折叠坐标系）
//!   + 折叠集；doc↔vis 映射收口在此（element.rs 光标 / movement 方向键 /
//!   点击反算共用同一套换算）

use std::ops::Range;

use gpui::{HighlightStyle, SharedString};
use ropey::Rope;

/// 章节折叠（k34j，可见占位符）：app 侧声明（`Decorations::section_folds`
/// ——全部可折叠章节表，gutter 箭头渲染与点击折叠的依据）与 vendor 回读
/// （`DecorationProvider::decorate` 的 `folded_sections` 参数——fold 快照中
/// 实际生效的章节折叠，vis 映射入参）共用同一结构。
///
/// 区间语义由 app 定义（md：标题行尾 → 下一同级/更高级标题行首，围栏感知）；
/// vendor 只当不透明折叠段用——进 Transform 链时以 `collapsed_text` 为
/// 可见占位符（区别于行内语法标记的零宽 `Some("")`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionFold {
    /// 折叠区间（buffer 字节空间，可跨行）。
    pub range: Range<usize>,
    /// 折叠块首所在 buffer 行（md = 标题行；gutter 箭头/点击路由键）。
    pub row: u32,
    /// 折叠占位文本（"⋯"）。
    pub collapsed_text: SharedString,
}

/// 单帧装饰结果（每帧 prepaint 由 provider 现算，不跨帧缓存状态）。
pub struct Decorations {
    /// 折叠后的完整显示文本（行数与原文完全一致——折叠只发生在行内）。
    pub display: Rope,
    /// 可见区样式 run：**折叠坐标系**、从 0 起、按序**全覆盖**（间隙填默认样式）。
    /// 坐标基准 = 可见行切片（`display.slice_lines(visible_range)`）起始。
    pub styles: Vec<(Range<usize>, HighlightStyle)>,
    /// 行内语法标记折叠集：doc 绝对字节偏移、零宽、不含换行，升序、
    /// 互不重叠且不与 `section_folds` 相交（app 侧保证）。
    pub folds: Vec<Range<usize>>,
    /// **生效**章节折叠（k34j）：本帧实际折叠的章节（折叠态 ∩ 声明表，
    /// 区间 = 声明表当前值）——vis 映射（`doc_to_vis`/`vis_to_doc`/
    /// `next_atomic`）、display/styles 坐标系与 fold 链重建（rebuild 的
    /// sections 输入）共用此表；折叠态由 vendor fold 快照持有。
    pub section_folds: Vec<SectionFold>,
    /// 章节折叠**声明表**（k34j）：全部可折叠章节（与折叠态无关）——
    /// gutter 箭头 foldable 判定 + 点击折叠的区间/占位文本来源。
    pub section_decl: Vec<SectionFold>,
    /// 行级字号倍数（doc 行号 == vis 行号——folds 不含换行、行数不变）：
    /// 标题行按倍数上浮 shaping，行高保持 uniform（024 P1：大标题观感在
    /// 行高允许范围内上浮，行级行高缝留 P2）。
    pub line_scale: Vec<(usize, f32)>,
}

impl Decorations {
    pub fn new(
        display: Rope,
        styles: Vec<(Range<usize>, HighlightStyle)>,
        folds: Vec<Range<usize>>,
        section_folds: Vec<SectionFold>,
        section_decl: Vec<SectionFold>,
        line_scale: Vec<(usize, f32)>,
    ) -> Self {
        Self {
            display,
            styles,
            folds,
            section_folds,
            section_decl,
            line_scale,
        }
    }

    /// doc 偏移 → 折叠后偏移（光标绘制、样式基准、wrap 计算共用）。
    ///
    /// 光标落在折叠段**内部**时收拢到折叠段首（段内字符不可寻址——
    /// atomic range 的一半约定；另一半"两端可停靠"由 movement 保证）。
    pub fn doc_to_vis(&self, off: usize) -> usize {
        Self::map_doc_to_vis_with(&self.folds, &self.section_folds, off)
    }

    /// 折叠后偏移 → doc 偏移（点击反算、双击选词）。
    ///
    /// 零宽约定：行内折叠段在 vis 系**不占宽度**；章节折叠段占
    /// `collapsed_text` 宽——vis 落进占位符区间时逆像取段**后**的 doc
    /// （与既有"逆像优先取段后"约定一致）。
    pub fn vis_to_doc(&self, vis: usize) -> usize {
        Self::map_vis_to_doc_with(&self.folds, &self.section_folds, vis)
    }

    /// 方向键原子跳过：给定 doc 偏移与方向（-1/1），返回跨过紧邻折叠段后的
    /// doc 偏移。光标永不停在看不见的语法标记/折叠章节中间——停靠位
    /// （段两端）或段内（点击/插入产生）均跨到段另一侧。行内段与章节段
    /// 都参与（k34j：章节折叠整段原子）。两段表互不相交（app 侧保证），
    /// 同一偏移至多命中一段。
    pub fn next_atomic(&self, off: usize, dir: i8) -> usize {
        let mut off = off;
        loop {
            let hit_inline = Self::atomic_hit(&self.folds, off, dir);
            let hit_section = Self::atomic_hit_section(&self.section_folds, off, dir);
            let hit = hit_inline
                .map(|f| (f.start, f.end))
                .or_else(|| hit_section.map(|s| (s.range.start, s.range.end)));
            match hit {
                Some((start, end)) => {
                    let next = if dir < 0 { start } else { end };
                    if next == off {
                        return off; // 病态折叠（空段），防死循环
                    }
                    off = next;
                }
                None => return off,
            }
        }
    }

    /// 原子跳过命中判定（next_atomic 内核，行内段）：dir 方向上 off 是否
    /// 贴着/落在某折叠段。
    fn atomic_hit<'a>(folds: &'a [Range<usize>], off: usize, dir: i8) -> Option<&'a Range<usize>> {
        folds.iter().find(|f| {
            if dir < 0 {
                // 左移：段后停靠位 或 段内 → 跨到段首
                f.end == off || (f.start < off && off < f.end)
            } else {
                // 右移：段前停靠位 或 段内 → 跨到段尾
                f.start == off || (f.start < off && off < f.end)
            }
        })
    }

    /// 原子跳过命中判定（章节段，k34j）——语义同 [`Self::atomic_hit`]。
    fn atomic_hit_section(
        sections: &[SectionFold],
        off: usize,
        dir: i8,
    ) -> Option<&SectionFold> {
        sections.iter().find(|s| {
            let f = &s.range;
            if dir < 0 {
                f.end == off || (f.start < off && off < f.end)
            } else {
                f.start == off || (f.start < off && off < f.end)
            }
        })
    }

    /// 静态映射：doc → vis（纯函数，app 侧 fold.rs 直接引用做性质测试）。
    /// 零宽版（无章节折叠）。
    pub fn map_doc_to_vis(folds: &[Range<usize>], off: usize) -> usize {
        Self::map_doc_to_vis_with(folds, &[], off)
    }

    /// 静态映射：doc → vis（统一版：行内零宽段 + 章节占位段，k34j）。
    /// 两段表各自升序、互不重叠（inline 不得与 section 相交——app 侧
    /// prune 保证）；此处按 doc 序双指针归并逐段累计。
    /// 段净缩量 = len − 占位宽（行内段占位宽 0）；段内偏移收拢到段首
    /// 可见位（行内段 = 段首；章节段 = 占位符末）。
    pub fn map_doc_to_vis_with(
        folds: &[Range<usize>],
        sections: &[SectionFold],
        off: usize,
    ) -> usize {
        let mut delta = 0i64; // vis = off + delta（累计净缩量，负值）
        let mut fi = 0usize;
        let mut si = 0usize;
        loop {
            let f = folds.get(fi);
            let s = sections.get(si);
            // 取 doc 序靠前的段；同起点行内段先（行内段不含换行、更短）
            let take_inline = match (f, s) {
                (Some(f), Some(s)) => f.start <= s.range.start,
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => break,
            };
            if take_inline {
                let f = f.unwrap();
                if f.end <= off {
                    delta -= f.len() as i64;
                } else if f.start < off {
                    // off 在行内折叠段内部 → 收拢到段首可见位
                    return (off as i64 + delta - (off - f.start) as i64) as usize;
                } else {
                    break;
                }
                fi += 1;
            } else {
                let s = s.unwrap();
                let ph = s.collapsed_text.len() as i64;
                let net = (s.range.len() as i64 - ph).max(0);
                if s.range.end <= off {
                    delta -= net;
                } else if s.range.start < off {
                    // off 在章节段内部 → 收拢到占位符末（段内不可寻址）
                    return (s.range.start as i64 + delta) as usize + ph as usize;
                } else {
                    break;
                }
                si += 1;
            }
        }
        (off as i64 + delta).max(0) as usize
    }

    /// 静态映射：vis → doc（纯函数）。零宽版（无章节折叠）。
    pub fn map_vis_to_doc(folds: &[Range<usize>], vis: usize) -> usize {
        Self::map_vis_to_doc_with(folds, &[], vis)
    }

    /// 静态映射：vis → doc（统一版，k34j）。行内段零宽投影、章节段占
    /// `collapsed_text` 宽；vis 落在占位符区间内 → 逆像取段**后** doc
    /// （与零宽"逆像优先取段后"同约定）。双指针按 doc 序归并。
    pub fn map_vis_to_doc_with(
        folds: &[Range<usize>],
        sections: &[SectionFold],
        vis: usize,
    ) -> usize {
        let mut doc = vis;
        let mut hidden = 0usize; // 当前段之前累计隐藏字节数
        let mut fi = 0usize;
        let mut si = 0usize;
        loop {
            let f = folds.get(fi);
            let s = sections.get(si);
            let take_inline = match (f, s) {
                (Some(f), Some(s)) => f.start <= s.range.start,
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => break,
            };
            if take_inline {
                let f = f.unwrap();
                if f.is_empty() {
                    fi += 1;
                    continue;
                }
                // 段在 vis 系的投影位（零宽）：vis 到达即跨过整段
                let fvis = f.start.saturating_sub(hidden);
                if vis >= fvis {
                    doc += f.len();
                    hidden += f.len();
                } else {
                    break; // 段在当前 vis 之后，后续段更靠后
                }
                fi += 1;
            } else {
                let s = s.unwrap();
                let ph = s.collapsed_text.len();
                let net = s.range.len().saturating_sub(ph);
                // 段在 vis 系的投影起点（占位符首）
                let svis = s.range.start.saturating_sub(hidden);
                if vis >= svis + ph {
                    // vis 已越过占位符 → 跨过整段
                    doc += net;
                    hidden += net;
                } else if vis > svis && ph > 0 {
                    // vis 落在占位符内 → 逆像取段后（段内不可寻址）
                    doc = s.range.end;
                    break;
                } else if vis == svis && ph > 0 {
                    // vis 停在占位符首 → 段首停靠位（与 doc_to_vis(section.start)
                    // 往返恒等；零宽段不走此支——既有"投影位跨段"语义不变）
                    break;
                } else {
                    break;
                }
                si += 1;
            }
        }
        doc
    }
}

/// 装饰 provider：每帧 prepaint 调用一次，返回 `None` = 走原路径
/// （doc 文本 + tree-sitter 高亮，零开销——非 md 文件/源码态恒为 None）。
pub trait DecorationProvider {
    /// `text`：文档本体 rope；`visible_lines`：视口行区间（0-based，行号）；
    /// `selection`：当前选区 doc 字节偏移（空选区 start==end == 光标位；
    /// reveal 决策入参——段级 reveal + 选区强制 reveal）；
    /// `folded_sections`：fold 快照中**实际生效**的章节折叠（k34j——折叠态
    /// 由 vendor 快照持有，provider 据此做 vis 映射；区间为快照当前值，
    /// 编辑后由链 anchor 平移，恒为当前 buffer 坐标）。
    fn decorate(
        &self,
        text: &Rope,
        visible_lines: Range<usize>,
        selection: Range<usize>,
        folded_sections: &[SectionFold],
    ) -> Option<Decorations>;
}
