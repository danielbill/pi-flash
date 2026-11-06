//! Run → gpui 元素层（自 mod.rs 外移）：行内样式映射（highlight）、
//! styled_text/link_ranges/runs_element 组装、标题字号、块边距合并与
//! render_blocks 整树渲染。

use super::*;

pub(crate) fn highlight(style: Style, t: &Theme) -> Option<HighlightStyle> {
    let h = match style {
        Style::Normal => return None,
        // pi-web strong: 700 字重 + color-mix(text 88%, accent)
        Style::Bold => HighlightStyle {
            font_weight: Some(FontWeight::BOLD),
            color: Some(rgb(crate::appearance::theme::mix_rgb(t.text, t.accent, 0.88)).into()),
            ..Default::default()
        },
        // pi-web em: var(--text-muted)
        Style::Italic => HighlightStyle {
            font_style: Some(FontStyle::Italic),
            color: Some(rgb(t.text_muted).into()),
            ..Default::default()
        },
        Style::BoldItalic => HighlightStyle {
            font_weight: Some(FontWeight::BOLD),
            font_style: Some(FontStyle::Italic),
            color: Some(rgb(crate::appearance::theme::mix_rgb(t.text, t.accent, 0.88)).into()),
            ..Default::default()
        },
        // note: gpui 0.2.2 highlights cannot change font family; 正文的行内 code
        // 走 paragraph_element 拆段成等宽盒，此高亮只是表格/标题内的兜底
        Style::Code => HighlightStyle {
            background_color: Some(rgb(t.tool_bg).into()),
            ..Default::default()
        },
        // pi-web a: 下划线 45% 透明（offset gpui 无对应）
        Style::Link => HighlightStyle {
            color: Some(rgb(t.accent).into()),
            underline: Some(gpui::UnderlineStyle {
                thickness: px(1.),
                color: Some(gpui::rgba((t.accent << 8) | 0x73).into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        // 024 P1 标题：正文色 + 字重（字号上浮由 Decorations.line_scale
        // 行级 shaping 承担，行高 uniform 内 cap）
        Style::Heading(_) => HighlightStyle {
            font_weight: Some(FontWeight::BOLD),
            color: Some(rgb(t.text).into()),
            ..Default::default()
        },
        Style::Strike => HighlightStyle {
            strikethrough: Some(gpui::StrikethroughStyle {
                thickness: px(1.),
                ..Default::default()
            }),
            ..Default::default()
        },
        // 公式在渲染前已拆段为图片；防御性落到代码风
        Style::Math | Style::DisplayMath => HighlightStyle {
            background_color: Some(rgb(t.tool_bg).into()),
            ..Default::default()
        },
    };
    Some(h)
}

pub(super) fn styled_text(
    runs: &[Run],
    t: &Theme,
    size: f32,
    line_h: f32,
    color: u32,
    weight: FontWeight,
) -> StyledText {
    let mut s = String::new();
    let mut highlights = Vec::new();
    for r in runs {
        let start = s.len();
        s.push_str(&r.text);
        let end = s.len();
        if let Some(h) = highlight(r.style, t) {
            highlights.push((start..end, h));
        }
    }
    StyledText::new(s).with_default_highlights(&base_style(size, line_h, color, weight), highlights)
}

/// 链接 run 的（拼接后字节区间, URL）平行列表，供 InteractiveText 命中。
pub(super) fn link_ranges(runs: &[Run]) -> (Vec<Range<usize>>, Vec<String>) {
    let mut ranges = Vec::new();
    let mut urls = Vec::new();
    let mut pos = 0usize;
    for r in runs {
        let end = pos + r.text.len();
        if let Some(url) = &r.url {
            ranges.push(pos..end);
            urls.push(url.clone());
        }
        pos = end;
    }
    (ranges, urls)
}

/// runs → 文本元素：含链接 run 时包 InteractiveText（点击 = cx.open_url 调
/// 系统默认浏览器；悬停手型光标由 InteractiveText 自动处理），否则纯
/// StyledText（多数块零开销）。element id 用拼接文本哈希——渲染无状态，
/// 靠内容稳定性保住 InteractiveText 跨帧的 mouse down/up 状态（流式中
/// 正在增长的段落点击可能失效，定稿即恢复）。
pub(super) fn runs_element(
    runs: &[Run],
    t: &Theme,
    size: f32,
    line_h: f32,
    color: u32,
    weight: FontWeight,
) -> AnyElement {
    let text = styled_text(runs, t, size, line_h, color, weight);
    let (ranges, urls) = link_ranges(runs);
    if ranges.is_empty() {
        return text.into_any_element();
    }
    let mut hasher = DefaultHasher::new();
    hasher.write(runs_text(runs).as_bytes());
    let id = gpui::ElementId::named_usize("md-link", hasher.finish() as usize);
    let target = md_target();
    InteractiveText::new(id, text)
        .on_click(ranges, move |ix, _window, cx| {
            let Some(url) = urls.get(ix) else { return };
            // 点击归链接自己消费（本地文件开 tab，其余开浏览器），冒泡到
            // md 预览容器的「点击进编辑」不得再触发
            cx.stop_propagation();
            // href 指向本地文件 → 内置文件标签页（pi-web 拦截 file 链接同型）
            if let Some(path) = as_file_path(url) {
                open_path_with(target.clone(), path, cx);
            } else {
                cx.open_url(url);
            }
        })
        .into_any_element()
}

/// 正文块（段落/表格/代码等）：字号必须挂在容器 div 上——gpui 0.2.2 的
/// StyledText 排版字号取 `window.text_style()`（容器继承链），runs 里的
/// font_size 只决定字族/字重/颜色，对字形尺寸无效。此前 base_style 里算好
/// 的字号对正文/表格/代码块从未生效（一直画容器继承的默认值），只有像
/// 标题、列表标号那样把 text_size 挂在容器上的元素才真正随设置变。
pub(super) fn sized_text(
    runs: &[Run],
    t: &Theme,
    size: f32,
    line_h: f32,
    color: u32,
    weight: FontWeight,
) -> AnyElement {
    let spec = active_md_spec();
    div()
        .w_full()
        .text_size(px(spec.size + (size - BASE)))
        .line_height(relative(line_h))
        .font_weight(weight)
        .child(runs_element(runs, t, size, line_h, color, weight))
        .into_any_element()
}

/// pi-web 标题字号（em 相对 14px 正文）。
pub(super) fn size_for_level(level: u8) -> f32 {
    match level {
        1 => BASE * 1.16,
        2 => BASE * 1.08,
        3 => BASE * 0.98,
        _ => BASE,
    }
}

pub(super) fn runs_text(runs: &[Run]) -> String {
    runs.iter().map(|r| r.text.as_str()).collect()
}

/// 各块的 CSS margin (mt, mb)，pi-web globals.css .markdown-body 规格。
/// flex 不做外边距折叠——render_blocks 用 max(prev.mb, cur.mt) 复现 CSS
/// 兄弟折叠，末块自然无 mb（= p:last-child { margin-bottom: 0 } 同效，
/// 此前逐块挂 margin 导致块间距系统性偏大：8+6=14 而 CSS 取 8）。
pub(super) fn block_margins(b: &MdBlock) -> (f32, f32) {
    match b {
        MdBlock::Heading { .. } => (10., 5.),
        MdBlock::Paragraph { .. } => (0., 8.),
        MdBlock::Code { .. } | MdBlock::Mermaid { .. } => (6., 6.),
        MdBlock::Quote { .. } => (6., 6.),
        // 列表容器 ul/ol { margin: 5px 0 8px }
        MdBlock::ListItem { .. } => (5., 8.),
        MdBlock::Table { .. } => (8., 8.),
        // pi-web img { margin: 8px 0 }
        MdBlock::Image { .. } => (8., 8.),
        // katex-display { margin: 0.6em }
        MdBlock::Math { .. } => (8., 8.),
        MdBlock::Rule => (12., 12.),
    }
}

pub(super) fn render_blocks(
    blocks: &[MdBlock],
    depth: usize,
    t: &Theme,
    streaming: bool,
    color: u32,
) -> gpui::Div {
    let mut col = div().flex().flex_col();
    // 间距 = max(前块 mb, 本块 mt)；首块保留自身 mt（引用块内有 padding，
    // 首元素 margin 不折叠出去，与 CSS 一致）
    let mut prev_mb: Option<f32> = None;
    let mut push = |col: gpui::Div, el: gpui::AnyElement, mt: f32, mb: f32| -> gpui::Div {
        let space = prev_mb.map_or(mt, |p| p.max(mt));
        prev_mb = Some(mb);
        col.child(div().mt(px(space)).child(el))
    };
    // 连续 ListItem 收进列表容器（li 间距 3px 用 gap）
    let mut i = 0;
    while i < blocks.len() {
        if matches!(blocks[i], MdBlock::ListItem { .. }) {
            let mut list = div().flex().flex_col().gap(px(3.));
            while i < blocks.len() && matches!(blocks[i], MdBlock::ListItem { .. }) {
                list = list.child(render_block(&blocks[i], depth, t, streaming, color));
                i += 1;
            }
            col = push(col, list.into_any_element(), 5., 8.);
        } else {
            let (mt, mb) = block_margins(&blocks[i]);
            let el = render_block(&blocks[i], depth, t, streaming, color);
            col = push(col, el, mt, mb);
            i += 1;
        }
    }
    col
}
