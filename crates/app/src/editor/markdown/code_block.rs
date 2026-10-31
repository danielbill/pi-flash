//! 代码块渲染（v54）：CodeScroll 滚动句柄线程局部表（按内容哈希键控）+
//! 文档/气泡两版代码块（syntect 高亮、行号、复制钮）+ 行内 code 盒。
//! 自 mod.rs 拆出（090 重构）；AST 类型与文本件仍在 mod.rs。

use super::*;

fn push_color(
    highlights: &mut Vec<(std::ops::Range<usize>, HighlightStyle)>,
    start: usize,
    end: usize,
    c: [u8; 3],
) {
    let color = rgb(((c[0] as u32) << 16) | ((c[1] as u32) << 8) | c[2] as u32);
    highlights.push((
        start..end,
        HighlightStyle {
            color: Some(color.into()),
            ..Default::default()
        },
    ));
}

// ---------------------------------------------------------------------------
// 代码块横向滚动句柄表——Zed Markdown::code_block_scroll_handles 的无状态
// 等价物：渲染函数没有实体可挂句柄，仿 ui::vlist SCROLL_HANDLES 建线程
// 局部表，按代码内容哈希键控（同内容块共享句柄=镜像滚动，罕见且无害）。
// Scrollbar 拖拽写句柄偏移，track_scroll 的滚动容器每帧读同一偏移。
// 哈希键无法枚举回收，超容量整表清空。
// ---------------------------------------------------------------------------
struct CodeScroll {
    handle: gpui::ScrollHandle,
    bar_state: gpui_component::scroll::ScrollbarState,
}

thread_local! {
    static CODE_SCROLLS: std::cell::RefCell<
        std::collections::HashMap<u64, std::rc::Rc<CodeScroll>>,
    > = std::cell::RefCell::new(std::collections::HashMap::new());
}

fn code_scroll(hash: u64) -> std::rc::Rc<CodeScroll> {
    CODE_SCROLLS.with(|cell| {
        let mut map = cell.borrow_mut();
        if map.len() > 256 {
            map.clear();
        }
        map.entry(hash)
            .or_insert_with(|| {
                std::rc::Rc::new(CodeScroll {
                    handle: gpui::ScrollHandle::new(),
                    bar_state: gpui_component::scroll::ScrollbarState::default(),
                })
            })
            .clone()
    })
}

/// 代码块：外框圆角 7px + 头部（语言名 / 复制）+ 行号 + 高亮体（字号 =
/// 槽位字号 -1，字体大小设置.md「代码块内容」；pi-web 行高 1.62）。
/// 文档模式代码块（Zed/GitHub 预览形制）：无语言标签/复制/行号 chrome，
/// 圆角框 + 语法高亮 + 横向滚动。
pub(crate) fn render_code_block_doc(lang: &str, code: &str, t: &Theme) -> gpui::AnyElement {
    let code = code.trim_end_matches('\n');
    let body_bg = crate::appearance::theme::mix_rgb(t.bg, t.bg_panel, 0.92);
    let mut text = String::new();
    let mut highlights: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();
    for (seg, c) in cached_highlight_segments(code, lang, t.dark).iter() {
        if seg.is_empty() {
            continue;
        }
        let start = text.len();
        text.push_str(seg);
        push_color(&mut highlights, start, text.len(), *c);
    }
    let spec = active_md_spec();
    let base = TextStyle {
        color: rgb(t.text).into(),
        font_family: MONO_FAMILY.into(),
        font_size: px(spec.size - 1.).into(),
        line_height: relative(1.6),
        ..Default::default()
    };
    let hash = hash_str(&format!("doc\0{lang}\0{code}"));
    let scroll = code_scroll(hash);
    div()
        .relative()
        .w_full()
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(8.))
        .bg(rgb(body_bg))
        .overflow_hidden()
        .child(
            div()
                .id(SharedString::from(format!("md-code-doc-{hash:016x}")))
                // Zed markdown 代码块同构：滚动容器必须 display:flex——gpui
                // block 子元素被拉伸到容器宽，永远无横向溢出可滚；flex_none
                // 内层让 nowrap 文本按自然宽度溢出成可滚内容
                .flex()
                .w_full()
                .px(px(14.))
                .py(px(12.))
                .text_size(px(spec.size - 1.))
                .line_height(relative(1.6))
                .whitespace_nowrap()
                .overflow_x_scroll()
                // 锁轴：纵向滚轮穿透给外层页面滚动，横向滚轮只滚本块
                .restrict_scroll_to_axis()
                .track_scroll(&scroll.handle)
                .child(
                    div()
                        .flex_none()
                        .child(StyledText::new(text).with_default_highlights(&base, highlights)),
                ),
        )
        // 横向滑块：Scrollbar 必须套显式 absolute 细条（用户气泡/导航浮层
        // 的既有配方）——直接挂容器用其自带 Absolute+100% 布局在 gpui 0.2.2
        // 下测不出可见 thumb。内容不超宽时 Scrollbar 自隐藏，无需手动 gate。
        .child(
            div()
                .absolute()
                .left(px(3.))
                .right(px(3.))
                .bottom(px(3.))
                .h(px(8.))
                .child(gpui_component::scroll::Scrollbar::horizontal(
                    &scroll.bar_state,
                    &scroll.handle,
                )),
        )
        .into_any_element()
}

pub(crate) fn render_code_block(lang: &str, code: &str, t: &Theme, streaming: bool) -> gpui::Div {
    let code = code.trim_end_matches('\n');
    let body_bg = crate::appearance::theme::mix_rgb(t.bg, t.bg_panel, 0.92);

    // header
    let copy_code = code.to_string();
    let header = div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .px(px(10.))
        .py(px(5.))
        .bg(rgb(t.bg_panel))
        .border_b_1()
        .border_color(rgb(t.border))
        .text_size(px(crate::ui::tokens::fixed::MD_LANG_BAR))
        .child(
            div()
                .font_family(MONO_FAMILY)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(if lang.is_empty() {
                    "text".to_string()
                } else {
                    lang.to_string()
                })),
        )
        .child(
            div()
                .id("md-copy")
                .cursor_pointer()
                .rounded(px(5.))
                .border_1()
                .border_color(rgb(t.border))
                .px(px(7.))
                .py(px(2.))
                .text_color(rgb(t.text_muted))
                .hover(|s| s.text_color(rgb(t.text)))
                .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy_code.clone()));
                })
                .child(SharedString::from(crate::i18n::tr("action.copy"))),
        );

    // 代码体：行号 gutter 前缀 + 逐行高亮段
    let lines: Vec<&str> = if code.is_empty() {
        vec![""]
    } else {
        code.split('\n').collect()
    };
    // c15: 流式期间纯文本（pi-web 渲染裸 <pre>，不做 Prism 高亮）
    let mut text = String::new();
    let mut highlights: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();
    if streaming {
        // 纯文本，无行号无高亮
        text.push_str(code);
    } else {
        let n_digits = lines.len().to_string().len();
        let dim = rgb(t.text_dim);
        // LRU 命中返回 Rc 借用，不再每帧 clone 全部高亮段
        let segs = cached_highlight_segments(code, lang, t.dark);
        let mut seg_ix = 0usize;
        for (i, _) in lines.iter().enumerate() {
            // 行号 gutter
            let start = text.len();
            text.push_str(&format!("{:>w$}  ", i + 1, w = n_digits));
            highlights.push((
                start..text.len(),
                HighlightStyle {
                    color: Some(dim.into()),
                    ..Default::default()
                },
            ));
            // 该行的高亮段
            while seg_ix < segs.len() {
                let (seg, c) = &segs[seg_ix];
                match seg.find('\n') {
                    Some(pos) => {
                        let (head, _) = seg.split_at(pos);
                        if !head.is_empty() {
                            let start = text.len();
                            text.push_str(head);
                            push_color(&mut highlights, start, text.len(), *c);
                        }
                        seg_ix += 1;
                        break; // 该行结束
                    }
                    None => {
                        if !seg.is_empty() {
                            let start = text.len();
                            text.push_str(seg);
                            push_color(&mut highlights, start, text.len(), *c);
                        }
                        seg_ix += 1;
                    }
                }
            }
            if i + 1 < lines.len() {
                text.push('\n');
            }
        }
    }

    let spec = active_md_spec();
    let base = TextStyle {
        color: rgb(t.text).into(),
        font_family: MONO_FAMILY.into(),
        font_size: px(spec.size - 1.).into(),
        line_height: relative(1.62),
        ..Default::default()
    };
    let hash = hash_str(&format!("chat\0{lang}\0{code}"));
    let scroll = code_scroll(hash);

    div()
        .w_full()
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(7.))
        .overflow_hidden()
        .bg(rgb(body_bg))
        // pi-web box-shadow: 0 1px 0 border 42%（块底一条更深的细线）
        .shadow(vec![gpui::BoxShadow {
            color: gpui::rgba(crate::appearance::theme::border_alpha(t, 0x6b)).into(),
            offset: gpui::point(px(0.), px(1.)),
            blur_radius: px(0.),
            spread_radius: px(0.),
        }])
        .child(header)
        .child(
            div()
                .relative()
                .w_full()
                .child(
                    div()
                        .id(SharedString::from(format!("md-code-body-{hash:016x}")))
                        // Zed 同构：flex 滚动容器 + flex_none 内层（见
                        // render_code_block_doc 注释），块级横向滚动才有
                        // 溢出内容可滚
                        .flex()
                        .w_full()
                        .px(px(13.))
                        .py(px(11.))
                        // 字号挂容器（见 sized_text 注释）：代码块 = 槽位字号 -1
                        .text_size(px(spec.size - 1.))
                        .line_height(relative(1.62))
                        // c15：长行不再裁剪——nowrap + 横向滚动（pi-web <pre> 语义）
                        .whitespace_nowrap()
                        .overflow_x_scroll()
                        // 锁轴：纵向滚轮穿透给外层页面滚动，横向滚轮只滚本块
                        .restrict_scroll_to_axis()
                        .track_scroll(&scroll.handle)
                        .child(div().flex_none().child(
                            StyledText::new(text).with_default_highlights(&base, highlights),
                        )),
                )
                // 横向滑块：显式 absolute 细条（同 render_code_block_doc 注释）
                .child(
                    div()
                        .absolute()
                        .left(px(3.))
                        .right(px(3.))
                        .bottom(px(3.))
                        .h(px(8.))
                        .child(gpui_component::scroll::Scrollbar::horizontal(
                            &scroll.bar_state,
                            &scroll.handle,
                        )),
                ),
        )
}

pub(crate) fn inline_code_box(text: &str, t: &Theme) -> AnyElement {
    let spec = active_md_spec();
    let style = TextStyle {
        color: rgb(t.text).into(),
        font_family: MONO_FAMILY.into(),
        font_size: px(spec.size - 1.12).into(),
        line_height: relative(1.5),
        ..Default::default()
    };
    let mut box_div = div()
        .max_w_full()
        .rounded(px(5.))
        .bg(rgba(t.bg_subtle))
        .border_1()
        .border_color(gpui::rgba((t.border << 8) | 0xb3))
        .px(px(5.))
        .py(px(1.))
        // 字号挂容器（见 sized_text 注释）：行内 code = 槽位 -1.12（0.92em）
        .text_size(px(spec.size - 1.12))
        .line_height(relative(1.5));
    // 路径形 code 可点开文件标签页（pi-web 消息链接的行内 code 变体）：
    // on_mouse_down 无需 stateful id，流式增长中也即时可点；非路径 code 盒零改动
    if let Some(path) = as_file_path(text) {
        let target = md_target();
        let accent = t.accent;
        box_div = box_div
            .cursor_pointer()
            .hover(move |s| s.text_color(rgb(accent)))
            .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                // 点击归 code 自己消费（开文件 tab），不冒泡给 md 预览容器的
                // 「点击进编辑」（on_mouse_down 在容器 on_click 记录按下态
                // 之前发，这里停了容器整条 click 链都不触发）
                cx.stop_propagation();
                open_path_with(target.clone(), path.clone(), cx);
            });
    }
    box_div
        .child(StyledText::new(text.to_string()).with_default_highlights(&style, Vec::new()))
        .into_any_element()
}
