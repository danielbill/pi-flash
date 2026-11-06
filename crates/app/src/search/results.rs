//! 搜索结果面板（083）——**对齐 Zed 的排版**，复用 editor 的渲染轮子。
//!
//! 2026-10-21 用户两轮返工后的定稿口径：
//! 1. 搜索框不加 border（`TextInput::flat()`）；
//! 2. 组头不能是黑色（`bg_subtle` 是 RRGGBBAA，`rgb()` 读会成近黑 → 用 `bg_panel`）；
//! 3. 文件块 100% 宽度（无外边距/圆角/边框/块间空隙）；
//! 4. 内容区与 editor 内容区对齐：字体族/字号取 `appearance::file_font()`；
//! 5. **排版照 Zed**：长行折行、行高 1.5、组头圆角+投影+加高、行号列随字号
//!    加宽、两侧留白（右侧让出滚动条宽度）、命中底色用中性 selection 色；
//! 6. **不做语法着色**：结果区是单色正文（syntect 在 markdown 上会给出红/橙/
//!    橄榄一堆颜色，用户评「五花八门」）；也**无 hover 变色**、活跃命中
//!    **只有左缘竖条**不刷整行底（2026-10-21 用户定案，逐条对齐 Zed）。
//!
//! 显示规则（在 `search/mod.rs` 落实）：**标题命中且内容也命中的文件只显示
//! 内容组；只有标题命中才显示文件栏**——所以这里 `path_only` 的块是纯文件栏。
//!
//! 行号仍是按文件真值画的：Zed 的行号来自 MultiBuffer（每命中一个 excerpt），
//! 而 PF 的编辑器底座是 gpui-component `InputState`，没有多缓冲概念。

use gpui::prelude::*;
use gpui::{AnyElement, SharedString, WeakEntity, div, px, rgb};
use std::rc::Rc;

use super::ResultBlock;
mod landing;
mod lines;

use self::landing::landing;
pub(super) use self::lines::match_highlights;
use self::lines::render_line;

use crate::Chat;
use crate::appearance::theme::{Theme, theme as T};
use crate::i18n::tr;

/// 打开结果的动作：`(块下标, 目标行, 命中序号, &mut App)`。抽成闭包是为了让
/// 渲染层不依赖 `Chat`（无头布局测试能直接跑生产渲染代码，见 tests）。
type OnOpen = Rc<dyn Fn(usize, u32, Option<usize>, &mut gpui::App)>;

/// 收展一个文件块：`(块下标, &mut App)`。
type OnToggle = Rc<dyn Fn(usize, &mut gpui::App)>;

/// 行高倍数：Zed 结果编辑器偏紧凑（`editor/view.rs` 的 1.618 是文档阅读态，
/// 搜索结果按 1.5 收，长行折行后不显松散）
const LINE_HEIGHT: f32 = 1.5;

/// 一行的高度（随「文档字号」设置缩放，与 editor 内容区同源）
fn line_h() -> f32 {
    (crate::appearance::file_font().size * LINE_HEIGHT).round()
}

/// 行号列几何——**照 Zed `Editor::gutter_dimensions`（editor.rs:12178-12270）**
/// 逐项算，不再拍死一个宽度：
/// - `digit_count = widest_line_number.ilog10() + 1`；
/// - `max_line_number_width = column_pixels(digit_count)`（该字体下 digit_count
///   个空格的推进宽）；
/// - `line_gutter_width = max(上面那个, ch_advance × min_line_number_digits)`，
///   默认设置 `min_line_number_digits = 4`（`assets/settings/default.json:753`）
///   ——不足 4 位也按 4 位留宽，行号位数增长时列宽才不会跳；
/// - `left_padding = ch_width`、`right_padding = ch_width`（只开行号时各 1 字符）；
/// - 列宽 = `line_gutter_width + left + right`。
///
/// 号码取**整个结果集的**最宽行号（Zed 的 `widest_line_number()` 也是整份
/// multibuffer 一个宽），否则各块行号左右错位。
#[derive(Clone, Copy, Debug)]
struct Gutter {
    /// 行号数字区宽（右对齐）
    num: f32,
    left: f32,
    right: f32,
}

impl Gutter {
    fn total(&self) -> f32 {
        self.num + self.left + self.right
    }
}

/// `min_line_number_digits`（Zed 默认 4）
const MIN_LINE_NUMBER_DIGITS: usize = 4;

fn gutter_for(blocks: &[ResultBlock], cx: &gpui::App) -> Gutter {
    let widest = blocks
        .iter()
        .flat_map(|b| b.segments.iter().flatten())
        .map(|l| l.line)
        .max()
        .unwrap_or(1);
    let digits = (vidx_digits(widest)).max(1);
    let font = crate::appearance::file_font();
    let font_id = cx
        .text_system()
        .resolve_font(&gpui::font(font.family.clone()));
    let size = px(font.size);
    let ch_adv = f32::from(
        cx.text_system()
            .ch_advance(font_id, size)
            .unwrap_or(px(font.size * 0.6)),
    );
    let ch_w = f32::from(
        cx.text_system()
            .ch_width(font_id, size)
            .unwrap_or(px(ch_adv)),
    );
    Gutter {
        num: (ch_adv * digits.max(MIN_LINE_NUMBER_DIGITS) as f32).round(),
        left: ch_w.round(),
        right: ch_w.round(),
    }
}

/// 十进制位数（`ilog10` 在 1 上返回 0，得补 1）
fn vidx_digits(mut n: u32) -> usize {
    let mut d = 1;
    while n >= 10 {
        n /= 10;
        d += 1;
    }
    d
}

// ---------------------------------------------------------------------------
// 文件头几何——**逐值照抄 Zed**，不再自己拍：
//   `crates/editor/src/editor.rs:298`  FILE_HEADER_HEIGHT = 2（行）
//   `crates/editor/src/editor.rs:299`  BUFFER_HEADER_PADDING = rems(0.25) = 4px
//   `crates/editor/src/element/header.rs:733`  .p(BUFFER_HEADER_PADDING)
//        .w_full().h(FILE_HEADER_HEIGHT * line_height)   ← 块高 = 2 行
//   `header.rs:718-760`  内条: .pl_1(4) .pr_2(8) .rounded_sm(4px) .gap_1p5(6px)
//        .border_1(border) .bg(editor_subheader_background) .hover(element_hover)
//   收展钮: `header.rs:747`  size 28×28（`rems_from_px(28)`）、rounded_xs(2px)、
//        悬停底 `element_selected`；图标 `IconSize::Small` = 14px
//   → 块高 2 行、条四周留 4px：**块与块之间自然有 8px 间隙**，条与自己的代码
//     行之间有 4px 间隙。这就是用户问的「间隙」。
// ---------------------------------------------------------------------------

/// 文件头**块**高 = 2 行（Zed `FILE_HEADER_HEIGHT`）
fn header_h() -> f32 {
    (line_h() * 2.).round()
}

/// 块内留白 = Zed `BUFFER_HEADER_PADDING`
pub(super) const HEADER_PAD: f32 = 4.;

/// 结果区：空态（Zed 的 landing page）或块列表。
pub(super) fn view(
    chat: &mut Chat,
    weak: &WeakEntity<Chat>,
    cx: &gpui::App,
    t: &'static Theme,
) -> AnyElement {
    if chat.ws.search.blocks.is_empty() {
        return landing(chat, weak, t);
    }
    let ui = &chat.ws.search;
    let blocks = ui.blocks.clone();
    let collapsed = ui.collapsed.clone();
    let active = ui.active;
    let list = ui.list.clone();
    let scrollbar = ui.scrollbar.clone();
    let gutter = gutter_for(&blocks, cx);
    let weak_rows = weak.clone();
    let weak_toggle = weak.clone();
    let on_toggle: OnToggle = Rc::new(move |block_ix, cx| {
        let _ = weak_toggle.update(cx, |chat, cx| chat.search_toggle_block(block_ix, cx));
    });
    let on_open: OnOpen = Rc::new(move |block_ix, line, ordinal, cx| {
        let _ = weak_rows.update(cx, |chat, cx| {
            chat.search_open_block(block_ix, line, ordinal, cx)
        });
    });
    results_shell(
        t,
        &list,
        &scrollbar,
        gpui::list(list.clone(), move |ix, _window, _cx| match blocks.get(ix) {
            Some(block) => render_block(
                ix,
                block,
                active,
                &on_open,
                &on_toggle,
                collapsed.contains(&block.rel),
                gutter,
            ),
            None => div().into_any_element(),
        })
        // list 自身要 flex_1 从父列拿高度（Auto 尺寸下会被布局成 0 高）
        .flex_1()
        .min_h_0(),
    )
    .into_any_element()
}

/// 结果区外壳。**必须是 flex 列**：gpui 的 `div` 默认不是 flex 容器，缺
/// `flex().flex_col()` 时内部 list 拿不到定高 → 高度 0，结果全丢（2026-10-21
/// 用户实测「只显示计数 1/222 不显示内容」的根因）。与 md 预览
/// （`editor/markdown/doc.rs`）的列表容器配方逐字一致；抽成函数是为了让
/// [`tests::real_blocks_render_with_height`] 能直接测生产代码的布局。
fn results_shell(
    t: &'static Theme,
    list_state: &gpui::ListState,
    scrollbar: &gpui_component::scroll::ScrollbarState,
    list: impl gpui::IntoElement,
) -> gpui::Stateful<gpui::Div> {
    let mut shell = div()
        .id("search-results")
        .flex_1()
        .min_h_0()
        .min_w_0()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        // 第一条文件头与搜索框留 5px 间隙：文件头块自带 Zed 的 4px 内留白，
        // 这里补 1px = 5px（用户 2026-10-21 定案的那个 5px）
        .pt(px(1.))
        // 两侧留白 + 右侧让出滚动条宽度：Zed 的文件头与代码行都从这条左线起，
        // 右边留出滚动条（2026-10-21 用户实测「两侧要有明显 padding，右侧还要
        // 留滚动条的宽度」）
        .pl(px(8.))
        .pr(px(18.))
        .relative()
        .child(list);
    // 竖向滚动条（与 md 预览同一挂法：绝对定位叠在右缘，内容已留 18px）
    if list_state.max_offset_for_scrollbar().height > px(0.) {
        let handle = crate::ui::list_handle::ListStateHandle(list_state.clone());
        shell = shell.child(
            div()
                .absolute()
                .top(px(4.))
                .bottom(px(4.))
                .right(px(4.))
                .w(px(8.))
                .child(gpui_component::scroll::Scrollbar::vertical(
                    scrollbar, &handle,
                )),
        );
    }
    shell
}

/// 一个文件结果块：组头 + 若干摘录段（整宽、无外边距、无圆角、块间无空隙）。
fn render_block(
    block_ix: usize,
    block: &ResultBlock,
    active: Option<usize>,
    on_open: &OnOpen,
    on_toggle: &OnToggle,
    collapsed: bool,
    gutter: Gutter,
) -> AnyElement {
    let t = T();
    let mut card = div()
        .id(SharedString::from(format!("psb-{block_ix}")))
        .w_full()
        .flex()
        .flex_col()
        .child(header_row(
            block_ix, block, on_open, on_toggle, collapsed, t,
        ));
    // 收起 = 只留文件头；纯文件栏本来就没有内容行
    if block.path_only || collapsed {
        return card.into_any_element();
    }
    // 不做语法着色：结果区是单色正文（用户 2026-10-21：syntect 在 markdown 上
    // 给出红/橙/柠等一堆颜色，「五花八门」）
    for (seg_ix, segment) in block.segments.iter().enumerate() {
        if seg_ix > 0 {
            // 段间分隔：1px 细线（Zed 的 excerpt 之间是头尾控制条，PF 用细线
            // 表达同一件事，不占高度、不刷底色）
            card = card.child(div().w_full().h(px(1.)).bg(rgb(t.border)));
        }
        for (line_ix, line) in segment.iter().enumerate() {
            card = card.child(render_line(
                block_ix, line, active, on_open, seg_ix, line_ix, gutter,
            ));
        }
    }
    card.into_any_element()
}

/// 组头（文件操作栏）——版式逐项照 `crates/editor/src/element/header.rs`
/// 的 `render_buffer_header`：块 `p 4` + 内条 `rounded_sm(4) / border_1 /
/// pl 4 pr 8 / gap 6`，左起「收展钮（28×28）→ 标题 → 副标题」。
///
/// 会话查询（`search/sessions.rs`）复用同一个 [`card_header`]，保证两种结果
/// 面板的组头一模一样（用户 2026-10-21：会话查询「根据文件查询的界面改造」）。
pub(super) struct HeaderCallbacks {
    pub(super) open: Rc<dyn Fn(&mut gpui::App)>,
    pub(super) toggle: Rc<dyn Fn(&mut gpui::App)>,
}

pub(super) fn card_header(
    id: String,
    title: SharedString,
    sub: SharedString,
    tail: Option<&'static str>,
    collapsed: bool,
    cb: HeaderCallbacks,
    t: &'static Theme,
) -> AnyElement {
    let font = crate::appearance::file_font();
    let HeaderCallbacks { open, toggle } = cb;
    // 块：p(4) + 高 2 行 → 条四周留 4px（块间因此有 8px 间隙）
    div()
        .id(SharedString::from(format!("psb-h-{id}")))
        // ui-raw: ok Zed render_buffer_header 逐值复刻（块p4/内条 pl4 pr8 gap6 rounded4）
        .w_full()
        .h(px(header_h()))
        .p(px(HEADER_PAD))
        .child(
            // 内条
            div()
                .id(SharedString::from(format!("psb-b-{id}")))
                .w_full()
                .h_full()
                .flex()
                .items_center()
                .pl(px(4.))
                .pr(px(8.))
                .gap(px(6.))
                .rounded(px(4.))
                .border_1()
                .border_color(rgb(t.border))
                // 组头底：bg_panel（`bg_subtle` 是 RRGGBBAA，rgb() 读会变近黑）
                .bg(rgb(t.bg_panel))
                .shadow_sm()
                .cursor_pointer()
                // Zed 内条本身就有 hover 底（`element_hover`）——源码行为，不是发明
                .hover(move |s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                    open(cx);
                })
                // 收展钮：Zed 的 28×28 方钮（**不带 tooltip**——用户定案）。
                // 点它只收展，不打开。
                .child(
                    div()
                        .id(SharedString::from(format!("psb-t-{id}")))
                        .flex_shrink_0()
                        // ui-raw: ok Zed 收展钜 28×28（`rems_from_px(28)`），
                        // 不套 PF 的 `ui_size`（那套基准是 12，会把 28 放大成 31）
                        .size(px(28.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(2.))
                        .hover(move |s| s.bg(rgb(t.bg_selected)))
                        .cursor_pointer()
                        .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                            // 别让点击冒泡到内条（那会直接打开）
                            cx.stop_propagation();
                            toggle(cx);
                        })
                        .child(crate::ui::icon(
                            if collapsed {
                                "chevron-right"
                            } else {
                                "chevron-down"
                            },
                            14.,
                            t.text_dim,
                        )),
                )
                // 标题 + 副标题（Zed 的 `gap_0p5` = 2px，整块 flex_1）
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(2.))
                        .child(
                            div()
                                .flex_shrink_0()
                                .font_family(font.family.clone())
                                .text_size(px(font.size))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgb(t.text))
                                .child(title),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .font_family(font.family.clone())
                                .text_size(px(font.size))
                                .text_color(rgb(t.text_muted))
                                .child(sub),
                        ),
                )
                .when_some(tail, |d, label| {
                    d.child(
                        div()
                            .flex_shrink_0()
                            .font_family(crate::appearance::panel_font().family)
                            .text_size(crate::appearance::ui_size(11.))
                            .text_color(rgb(t.text_dim))
                            .child(SharedString::from(label)),
                    )
                }),
        )
        .into_any_element()
}

fn header_row(
    block_ix: usize,
    block: &ResultBlock,
    on_open: &OnOpen,
    on_toggle: &OnToggle,
    collapsed: bool,
    t: &'static Theme,
) -> AnyElement {
    let name: SharedString = block
        .rel
        .rsplit('/')
        .next()
        .unwrap_or(&block.rel)
        .to_string()
        .into();
    let dir: SharedString = {
        let d = &block.rel[..block.rel.len().saturating_sub(name.len())];
        d.trim_end_matches('/').to_string().into()
    };
    let open = on_open.clone();
    let toggle = on_toggle.clone();
    card_header(
        block_ix.to_string(),
        name,
        dir,
        block.path_only.then(|| tr("search.path_match")),
        collapsed,
        HeaderCallbacks {
            open: Rc::new(move |cx| open(block_ix, 1, None, cx)),
            toggle: Rc::new(move |cx| toggle(block_ix, cx)),
        },
        t,
    )
}

#[cfg(test)]
mod tests;
