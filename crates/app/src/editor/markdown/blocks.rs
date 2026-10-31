//! 块级渲染件（v54）：表格（token 度量限宽）、图片（本地图片 LRU 缓存）、
//! 段落（行内富段拆盒）与 [`render_block`] 分发器（MdBlock → 元素）。
//! 自 mod.rs 拆出（090 重构）。

use super::*;

/// 表格：外框圆角 7px、th bg_panel 650 字重、行分隔线、偶数行斑马纹
/// （pi-web .markdown-table-wrap）。
/// 单元格显示宽度权重：ASCII 记 1、CJK/全角记 2（HTML 表 auto layout 的
/// 近似度量）。
fn cell_weight(runs: &[Run]) -> usize {
    runs.iter()
        .map(|r| {
            r.text
                .chars()
                .map(|c| if (c as u32) > 0x2e80 { 2 } else { 1 })
                .sum::<usize>()
        })
        .sum()
}

/// 最长**不可断行 token** 的显示宽度（>0x2E80 计 2）：换行只落在空白/
/// CJK 字符边界，ASCII 连续段（002068、59.71）是单一 token，其像素宽
/// 就是该列的 min-content（HTML auto layout 语义）
fn token_units(runs: &[Run]) -> usize {
    let text: String = runs.iter().map(|r| r.text.as_str()).collect();
    let (mut best, mut cur) = (0usize, 0usize);
    for c in text.chars() {
        if c.is_whitespace() || (c as u32) > 0x2e80 {
            best = best.max(cur);
            cur = 0;
        } else {
            cur += 1;
        }
    }
    best.max(cur)
}

fn render_table(head: &[Vec<Run>], rows: &[Vec<Vec<Run>>], t: &Theme) -> gpui::Div {
    // 列宽 = 列内最长单元格的显示宽度（钳制 [6,44]），归一化后全表共用
    // 同一组相对宽度——跨行对齐成网格，长单元格换行（pi-web HTML 表
    // auto layout 的近似；flex_1 按内容分配导致每行列位漂移，已废）
    let n_cols = head
        .len()
        .max(rows.iter().map(|r| r.len()).max().unwrap_or(0))
        .max(1);
    const MIN_W: usize = 6;
    const MAX_W: usize = 44;
    let mut weights = vec![MIN_W; n_cols];
    for (ci, cell) in head.iter().enumerate() {
        weights[ci] = weights[ci].max(cell_weight(cell).clamp(MIN_W, MAX_W));
    }
    for row in rows {
        for (ci, cell) in row.iter().enumerate().take(n_cols) {
            weights[ci] = weights[ci].max(cell_weight(cell).clamp(MIN_W, MAX_W));
        }
    }
    let total: f32 = weights.iter().sum::<usize>() as f32;
    let fracs: Vec<f32> = weights.iter().map(|w| *w as f32 / total).collect();

    // pi-flash patch: 每列像素地板——最长不可断 token × 0.7em（ASCII 实测
    // advance 0.5~0.63em，0.7 取保守上限；CJK 一字 1em < 2×0.7em 亦安全）
    // + px(10)×2 内边距 + 2 圆整。纯比例分配在大列（锁到 44）撑大分母时
    // 会把窄列压到内容宽以下折行（"002068" 断成 00206/8），与 TextView
    // 表格同坑同修；此处无 window 不做实测，按字号估算（表头 BASE/正文 13）
    let mut token_w = vec![0usize; n_cols];
    for (ci, cell) in head.iter().enumerate() {
        token_w[ci] = token_w[ci].max(token_units(cell));
    }
    for row in rows {
        for (ci, cell) in row.iter().enumerate().take(n_cols) {
            token_w[ci] = token_w[ci].max(token_units(cell));
        }
    }
    let min_ws: Vec<f32> = token_w
        .iter()
        .map(|&u| u as f32 * BASE * 0.7 + 22.)
        .collect();

    // 地板只抬不压：其余列按 flex_shrink 让位，各自地板仍受保护；
    // 格内超长仍受 relative(frac) 约束换行
    let cell_div = |frac: f32, min_w: f32| {
        div()
            .w(relative(frac))
            .min_w(px(min_w))
            .px(px(10.))
            .py(px(6.))
    };

    // 字体大小设置.md：表格头 = 槽位字号，表格内容 = 槽位字号 -1，行高 1.7
    // （字号经 sized_text 挂容器，见其注释）
    let head_cells: Vec<gpui::AnyElement> = head
        .iter()
        .enumerate()
        .map(|(ci, cell)| {
            cell_div(fracs[ci], min_ws[ci])
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(crate::appearance::theme::mix_rgb(
                    t.text,
                    t.text_muted,
                    0.88,
                )))
                .child(sized_text(
                    cell,
                    t,
                    BASE,
                    1.7,
                    crate::appearance::theme::mix_rgb(t.text, t.text_muted, 0.88),
                    FontWeight::SEMIBOLD,
                ))
                .into_any_element()
        })
        .collect();
    let head_row = div()
        .flex()
        .bg(rgb(t.bg_panel))
        .border_b_1()
        .border_color(rgb(t.border))
        .children(head_cells);

    let body_rows: Vec<gpui::AnyElement> = rows
        .iter()
        .enumerate()
        .map(|(ri, row)| {
            let mut line = div().flex().border_b_1().border_color(rgb(t.border));
            if ri % 2 == 1 {
                line = line.bg(rgba(t.bg_subtle));
            }
            for (ci, cell) in row.iter().enumerate().take(n_cols) {
                line = line.child(cell_div(fracs[ci], min_ws[ci]).child(sized_text(
                    cell,
                    t,
                    13.,
                    1.7,
                    t.text,
                    FontWeight::NORMAL,
                )));
            }
            line.into_any_element()
        })
        .collect();

    div()
        .w_full()
        .mt(px(8.))
        .mb(px(8.))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(7.))
        .overflow_hidden()
        .child(head_row)
        .children(body_rows)
}

/// 图片：本地文件 gpui img() 直渲染；http/不存在 → alt 文本占位。
/// 相对路径按 md 文件目录解析（IMG_BASE，render_themed 设置）。
fn render_image(url: &str, width: Option<f32>, alt: &[Run], t: &Theme) -> gpui::AnyElement {
    let placeholder = || {
        div()
            .w_full()
            .text_color(rgb(t.text_dim))
            .italic()
            .child(SharedString::from(format!("🖼 {}", runs_text(alt))))
            .into_any_element()
    };
    if url.starts_with("http://") || url.starts_with("https://") || url.is_empty() {
        return placeholder();
    }
    let ext = std::path::Path::new(url)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let format = match ext.as_str() {
        "png" => Some(gpui::ImageFormat::Png),
        "jpg" | "jpeg" => Some(gpui::ImageFormat::Jpeg),
        "gif" => Some(gpui::ImageFormat::Gif),
        "bmp" => Some(gpui::ImageFormat::Bmp),
        "svg" => Some(gpui::ImageFormat::Svg),
        "webp" => Some(gpui::ImageFormat::Webp),
        _ => None,
    };
    let path = std::path::Path::new(url);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        resolve_relative_image(path)
    };
    let Some((bytes, format)) = std::fs::read(&path).ok().zip(format) else {
        return placeholder();
    };
    div()
        .w_full()
        .child(
            gpui::img(local_image_cached(format, bytes))
                .max_w_full()
                .when_some(width, |d, w| d.w(px(w.max(24.))))
                .rounded(px(6.)),
        )
        .into_any_element()
}

// 本地图片缓存：md 源里的 ![](本地路径) 每帧重建元素树，原先每帧
// std::fs::read + 新建 Arc<Image>；gpui 按内容哈希去重 loading_assets，
// 但解码位图只进不出。这里线程局部 VecDeque LRU（同 MD_PARSE_CACHE 的
// 理由，元素构建只在主线程），驱逐时经 asset_evict 同步放掉 gpui 侧位图。
// 字节预算按源文件字节计（解码位图与源同量级）。
thread_local! {
    static LOCAL_IMG_CACHE: std::cell::RefCell<
        std::collections::VecDeque<
            ((u64, usize, u8), (Vec<u8>, std::sync::Arc<gpui::Image>)),
        >,
    > = const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}
const LOCAL_IMG_CACHE_CAP: usize = 32;
const LOCAL_IMG_CACHE_BUDGET_BYTES: usize = 64 * 1024 * 1024;

fn local_image_cached(format: gpui::ImageFormat, bytes: Vec<u8>) -> std::sync::Arc<gpui::Image> {
    let key = (hash_bytes(&bytes), bytes.len(), format as u8);
    LOCAL_IMG_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        if let Some(pos) = cache.iter().rposition(|(k, _)| *k == key) {
            let entry = cache.remove(pos).expect("pos 来自刚才的迭代");
            if entry.1.0 == bytes {
                let image = entry.1.1.clone();
                cache.push_back(entry); // 刷新到队尾（LRU）
                return image;
            }
        }
        let image = std::sync::Arc::new(gpui::Image::from_bytes(format, bytes.clone()));
        cache.push_back((key, (bytes, image.clone())));
        while cache.len() > LOCAL_IMG_CACHE_CAP
            || cache.iter().map(|(_, v)| v.0.len()).sum::<usize>() > LOCAL_IMG_CACHE_BUDGET_BYTES
        {
            match cache.pop_front() {
                Some((_, (_, evicted))) => {
                    // gpui 侧解码位图同步放掉（见 services::asset_evict）
                    crate::services::asset_evict::queue_evicted(evicted);
                }
                None => break,
            }
        }
        image
    })
}

/// 探针（automation snapshot）：（条数，源文件字节）
pub(crate) fn local_img_probe() -> (usize, usize) {
    LOCAL_IMG_CACHE.with(|cell| {
        let cache = cell.borrow();
        (
            cache.len(),
            cache.iter().map(|(_, v)| v.0.len()).sum::<usize>(),
        )
    })
}

/// 行内 code 盒（pi-web .markdown-inline-code）：mono（槽位字号 -1.12 =
/// 0.92em 的绝对差换算）+ bg-subtle + 圆角 5 + padding 1px 5px + 70% 边框
/// 描边；颜色恒 --text（引用块内不继承）。
/// gpui 行内 highlight 换不了字体家族，拆盒才能落 mono。

/// 行内富段：含 Code/Math 的段落拆 flex-wrap 段（文本段内部自然换行，
/// code = 等宽盒、公式 = 图片）；纯文本段落保持单一 StyledText。
fn paragraph_element(runs: &[Run], t: &Theme, color: u32) -> AnyElement {
    let rich = runs
        .iter()
        .any(|r| matches!(r.style, Style::Code | Style::Math | Style::DisplayMath));
    if !rich {
        return sized_text(runs, t, BASE, 1.7, color, FontWeight::NORMAL);
    }
    let flush = |row: gpui::Div, tr: &mut Vec<Run>, t: &Theme, color: u32| -> gpui::Div {
        if !tr.is_empty() {
            let taken = std::mem::take(tr);
            return row.child(div().max_w_full().child(sized_text(
                &taken,
                t,
                BASE,
                1.7,
                color,
                FontWeight::NORMAL,
            )));
        }
        row
    };
    let mut row = div().w_full().flex().flex_wrap().items_end();
    let mut text_run: Vec<Run> = Vec::new();
    for r in runs {
        match r.style {
            Style::Code => {
                row = flush(row, &mut text_run, t, color);
                row = row.child(inline_code_box(&r.text, t));
            }
            Style::Math => {
                row = flush(row, &mut text_run, t, color);
                row = row.child(crate::editor::markdown::render::math::inline_element(
                    &r.text, t,
                ));
            }
            Style::DisplayMath => {
                row = flush(row, &mut text_run, t, color);
                // 块级公式：flex_wrap 下 w_full 独占一行
                row = row.child(div().w_full().child(
                    crate::editor::markdown::render::math::block_element(&r.text, t),
                ));
            }
            _ => text_run.push(r.clone()),
        }
    }
    row = flush(row, &mut text_run, t, color);
    row.into_any_element()
}

pub(crate) fn render_block(
    b: &MdBlock,
    depth: usize,
    t: &Theme,
    streaming: bool,
    color: u32,
) -> AnyElement {
    match b {
        MdBlock::Heading { level, runs } => {
            let doc = doc_mode();
            let spec = active_md_spec();
            // 文档模式 = Zed/GitHub 尺度（h1/h2 放大 + 底部分隔线）；聊天区
            // 维持 pi-web 标题规则（h1 1.16em …）
            let size = if doc {
                doc_size_for_level(*level, spec.size)
            } else {
                size_for_level(*level)
            };
            // pi-web 标题规则：h1/h2/h4-h6 = var(--text)（引用块内也是），
            // h3 = color-mix(text 88%, muted)
            let color = if *level == 3 {
                crate::appearance::theme::mix_rgb(t.text, t.text_muted, 0.88)
            } else {
                t.text
            };
            let text_size = if doc { size } else { spec.size + (size - BASE) };
            let underlined = doc && (*level <= 2);
            div()
                .w_full()
                .font_weight(if doc && *level == 1 {
                    FontWeight::BOLD
                } else {
                    FontWeight::SEMIBOLD
                })
                .text_size(px(text_size))
                .line_height(relative(1.35))
                .when(underlined, |d| {
                    d.pb(px(if *level == 1 { 7. } else { 5. }))
                        .border_b_1()
                        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x55)))
                })
                .child(runs_element(
                    runs,
                    t,
                    size,
                    1.35,
                    color,
                    FontWeight::SEMIBOLD,
                ))
                .into_any_element()
        }
        MdBlock::Paragraph { runs } => paragraph_element(runs, t, color),
        MdBlock::Code { code, lang, .. } => {
            if doc_mode() {
                render_code_block_doc(lang, code, t)
            } else {
                render_code_block(lang, code, t, streaming).into_any_element()
            }
        }
        MdBlock::Quote { blocks } => div()
            .w_full()
            .border_l_3()
            .border_color(rgb(crate::appearance::theme::mix_rgb(
                t.border,
                t.text_muted,
                0.75,
            )))
            .rounded_r(px(6.))
            .bg(rgba(t.bg_subtle))
            .px(px(11.))
            .py(px(6.))
            // pi-web blockquote color: var(--text-muted)——经 color 参数落进
            // 内部段落的 StyledText（此前容器色被子元素硬编码 text 覆盖）
            .child(render_blocks(blocks, depth + 1, t, streaming, t.text_muted))
            .into_any_element(),
        MdBlock::ListItem {
            depth: d,
            marker,
            runs,
            task,
        } => {
            let task = *task;
            // 悬挂缩进（pi-web ul{padding-left:22px} + li{padding-left:2px}
            // + list-style-position:outside）：marker 落在文字左侧的固定
            // 槽里，文字统一从 24px 起排，嵌套每层 +22px。
            // items_start（非 center）：pi-web outside marker 与内容第一行
            // 对齐——多行 item 时圆点/勾选框必须钉在首行，垂直居中会漂到
            // 整条 item 中间；行盒 = 1.7×字号，文本 marker（"1."）行盒同高
            // 自然落首行
            let slot = div()
                .w(px(24.))
                .flex_shrink_0()
                .flex()
                .items_start()
                .justify_end() // marker 靠槽右缘 = 悬挂在文字左侧
                .pr(px(4.)) // marker 右边到文字的视觉间隙（浏览器 outside marker）
                // 字体大小设置.md「list mark = 设置值」；绝对差换算下
                // 不随外层标题字号复利放大
                .text_size(px(active_md_spec().size))
                .line_height(relative(1.7))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(crate::appearance::theme::mix_rgb(
                    t.accent,
                    t.text_muted,
                    0.72,
                )));
            let marker_el: AnyElement = match task {
                // c12: 任务项 marker = 14px 复选框（选中 = accent 10% 淡底 +
                // 55% 边框 + accent 对勾，pi-web :checked 规则）；top:0.35em
                // 钉在首行（pi-web input 绝对定位 top 0.35em parity）
                Some(checked) => slot
                    .child(
                        div()
                            .mt(px(5.))
                            .w(px(14.))
                            .h(px(14.))
                            .flex_shrink_0()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(rgb(if checked {
                                crate::appearance::theme::mix_rgb(t.accent, t.border, 0.55)
                            } else {
                                t.border
                            }))
                            .bg(rgb(if checked {
                                crate::appearance::theme::mix_rgb(t.accent, t.bg, 0.10)
                            } else {
                                t.bg
                            }))
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(checked, |b| {
                                b.child(
                                    div()
                                        .text_size(px(crate::ui::tokens::fixed::MD_CHECK_GLYPH))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(rgb(t.accent))
                                        .child(SharedString::from(tr("action.confirm_mark"))),
                                )
                            }),
                    )
                    .into_any_element(),
                // 无序列表：画 0.45em 实心圆（Chrome list-style disc 尺寸），
                // 首行行盒内垂直居中——"•" 字形在 14px 下只有 ~4px 且偏细
                None if marker == "•" => {
                    let spec = active_md_spec();
                    let dot = (0.45 * spec.size).round();
                    let first_line = 1.7 * spec.size;
                    slot.child(
                        div()
                            .mt(px(((first_line - dot) / 2.).max(0.)))
                            .size(px(dot))
                            .rounded(px(dot / 2.))
                            .bg(rgb(crate::appearance::theme::mix_rgb(
                                t.accent,
                                t.text_muted,
                                0.72,
                            ))),
                    )
                    .into_any_element()
                }
                None => slot
                    .child(SharedString::from(marker.clone()))
                    .into_any_element(),
            };
            div()
                .flex()
                .pl(px((d * 22) as f32))
                .child(marker_el)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(paragraph_element(runs, t, t.text)),
                )
                .into_any_element()
        }
        MdBlock::Table { head, rows } => render_table(head, rows, t).into_any_element(),
        MdBlock::Image { url, alt, width } => render_image(url, *width, alt, t),
        MdBlock::Math { latex } => crate::editor::markdown::render::math::block_element(latex, t),
        MdBlock::Mermaid { source } => {
            // pi-web MermaidBlock parity：流式期间只显源码；失败回退源码块
            if !streaming {
                if let Some(el) =
                    crate::editor::markdown::render::mermaid::diagram_element(source, t.dark)
                {
                    return el;
                }
            }
            render_code_block("mermaid", source, t, true).into_any_element()
        }
        MdBlock::Rule => {
            // pi-web hr: 渐变淡出线（linear-gradient 90deg transparent-border-
            // transparent）。gpui 渐变仅两停靠点，用底线 + 两端 18% 遮罩复现
            let base: gpui::Hsla = rgb(t.border).into();
            let bg: gpui::Hsla = rgb(t.bg).into();
            let bg_a0 = gpui::Hsla { a: 0., ..bg };
            div()
                .w_full()
                .h(px(1.))
                .relative()
                .child(div().size_full().bg(base))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .w(relative(0.18))
                        .h_full()
                        .bg(gpui::linear_gradient(
                            90.,
                            gpui::linear_color_stop(bg, 0.),
                            gpui::linear_color_stop(bg_a0, 1.),
                        )),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .right_0()
                        .w(relative(0.18))
                        .h_full()
                        .bg(gpui::linear_gradient(
                            90.,
                            gpui::linear_color_stop(bg_a0, 0.),
                            gpui::linear_color_stop(bg, 1.),
                        )),
                )
                .into_any_element()
        }
    }
}
