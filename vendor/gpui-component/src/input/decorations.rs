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

use gpui::HighlightStyle;
use ropey::Rope;

/// 单帧装饰结果（每帧 prepaint 由 provider 现算，不跨帧缓存状态）。
pub struct Decorations {
    /// 折叠后的完整显示文本（行数与原文完全一致——折叠只发生在行内）。
    pub display: Rope,
    /// 可见区样式 run：**折叠坐标系**、从 0 起、按序**全覆盖**（间隙填默认样式）。
    /// 坐标基准 = 可见行切片（`display.slice_lines(visible_range)`）起始。
    pub styles: Vec<(Range<usize>, HighlightStyle)>,
    /// 折叠集：doc 绝对字节偏移，升序、互不重叠（app 侧保证）。
    pub folds: Vec<Range<usize>>,
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
        line_scale: Vec<(usize, f32)>,
    ) -> Self {
        Self {
            display,
            styles,
            folds,
            line_scale,
        }
    }

    /// doc 偏移 → 折叠后偏移（光标绘制、样式基准、wrap 计算共用）。
    ///
    /// 光标落在折叠段**内部**时收拢到折叠段首（段内字符不可寻址——
    /// atomic range 的一半约定；另一半"两端可停靠"由 movement 保证）。
    pub fn doc_to_vis(&self, off: usize) -> usize {
        Self::map_doc_to_vis(&self.folds, off)
    }

    /// 折叠后偏移 → doc 偏移（点击反算、双击选词）。
    ///
    /// 零宽约定：折叠段在 vis 系**不占宽度**——vis 走到段的投影位即
    /// 跨过整段（逆像优先取段**后**的 doc：`vis_to_doc(doc_to_vis(f.end))`
    /// 往返恒等，段首往返收拢到段尾）。
    pub fn vis_to_doc(&self, vis: usize) -> usize {
        Self::map_vis_to_doc(&self.folds, vis)
    }

    /// 方向键原子跳过：给定 doc 偏移与方向（-1/1），返回跨过紧邻折叠段后的
    /// doc 偏移。光标永不停在看不见的语法标记中间——停靠位（段两端）或
    /// 段内（点击/插入产生）均跨到段另一侧。
    pub fn next_atomic(&self, off: usize, dir: i8) -> usize {
        let mut off = off;
        loop {
            let hit = self.folds.iter().find(|f| {
                if dir < 0 {
                    // 左移：段后停靠位 或 段内 → 跨到段首
                    f.end == off || (f.start < off && off < f.end)
                } else {
                    // 右移：段前停靠位 或 段内 → 跨到段尾
                    f.start == off || (f.start < off && off < f.end)
                }
            });
            match hit {
                Some(f) => {
                    let next = if dir < 0 { f.start } else { f.end };
                    if next == off {
                        return off; // 病态折叠（空段），防死循环
                    }
                    off = next;
                }
                None => return off,
            }
        }
    }

    /// 静态映射：doc → vis（纯函数，app 侧 fold.rs 直接引用做性质测试）。
    pub fn map_doc_to_vis(folds: &[Range<usize>], off: usize) -> usize {
        let mut hidden_before = 0usize;
        for f in folds {
            if f.end <= off {
                hidden_before += f.len();
            } else if f.start < off {
                // off 在折叠段内部 → 收拢到段首可见位
                hidden_before += off - f.start;
                break;
            } else {
                break;
            }
        }
        off - hidden_before
    }

    /// 静态映射：vis → doc（纯函数）。
    pub fn map_vis_to_doc(folds: &[Range<usize>], vis: usize) -> usize {
        let mut doc = vis;
        let mut hidden = 0usize; // 当前段之前累计隐藏字节数
        for f in folds {
            if f.is_empty() {
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
        }
        doc
    }
}

/// 装饰 provider：每帧 prepaint 调用一次，返回 `None` = 走原路径
/// （doc 文本 + tree-sitter 高亮，零开销——非 md 文件/源码态恒为 None）。
pub trait DecorationProvider {
    /// `text`：文档本体 rope；`visible_lines`：视口行区间（0-based，行号）；
    /// `selection`：当前选区 doc 字节偏移（空选区 start==end == 光标位；
    /// reveal 决策入参——段级 reveal + 选区强制 reveal）。
    fn decorate(
        &self,
        text: &Rope,
        visible_lines: Range<usize>,
        selection: Range<usize>,
    ) -> Option<Decorations>;
}
