//! 解析缓存（自 mod.rs 外移）：MD_PARSE_CACHE LRU + STREAM_PARSE_GATE
//! 流式解析门 + FNV-1a 哈希 + 探针。

use super::*;

// 解析结果缓存：gpui List 每帧对可见条目重建元素树，pulldown 解析是其中
// 最贵的纯 CPU 段（大 markdown 消息数百 µs～ms 级），滚动/流式时每帧白打。
// key = 源文哈希 + 长度 + html 标志，命中后全等校验防碰撞；线程局部
// VecDeque 当 LRU（元素构建只在主线程）。流式期间末条消息源文每 delta
// 一变，最多占满队头被挤出，不影响命中态。
thread_local! {
    static MD_PARSE_CACHE: std::cell::RefCell<
        std::collections::VecDeque<((u64, usize, bool), (String, std::rc::Rc<Vec<MdBlock>>))>,
    > = const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}
const MD_PARSE_CACHE_CAP: usize = 128;
/// 字节预算（源文 32MB）：大粘贴场景 128 条 × MB 级源文＋解析树的组合曾把
/// 稳态推高上百 MB；超预算从头驱逐（LRU），解析树随条目一起释放
const MD_PARSE_CACHE_BUDGET_BYTES: usize = 32 * 1024 * 1024;

// 流式尾条目解析门（A3，2026-11-06 流式卡死修复）：流式块源文每 delta
// 一变 → 上面的 LRU 恒 miss → 每帧全量 pulldown 重解析 O(已流式字数)，
// 快流模型（116 t/s）下与全窗重建叠加把帧成本推爆、输入饿死。门内
// （150ms）源文是上一棵树的前缀增长（append-only 流式）时直接复用上一
// 棵解析树——元素树每帧照常重建，只有显示滞后 ≤150ms（与 spinner/t-s
// 徽章的刷新粒度一致，无感知）。非前缀（换块/截断）立即重解析并换门内
// 容：交替流式块场景退化为原行为，只慢不错。常驻一条（≤MAX_MARKDOWN
// _CHARS 量级），被下一次流式渲染覆盖，不显式清理。
thread_local! {
    static STREAM_PARSE_GATE: std::cell::RefCell<
        Option<(std::time::Instant, String, std::rc::Rc<Vec<MdBlock>>, bool)>,
    > = const { std::cell::RefCell::new(None) };
}
const STREAM_GATE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(150);

pub(super) fn stream_gate_blocks(src: &str, html: bool) -> std::rc::Rc<Vec<MdBlock>> {
    STREAM_PARSE_GATE.with(|cell| {
        let mut gate = cell.borrow_mut();
        if let Some((at, prev, tree, prev_html)) = gate.as_ref() {
            if *prev_html == html
                && at.elapsed() < STREAM_GATE_INTERVAL
                && src.starts_with(prev.as_str())
            {
                return std::rc::Rc::clone(tree);
            }
        }
        let parsed = cached_blocks(src, html);
        *gate = Some((
            std::time::Instant::now(),
            src.to_string(),
            std::rc::Rc::clone(&parsed),
            html,
        ));
        parsed
    })
}

/// FNV-1a 按 8 字节块处理（进程内去重键，非加密；命中另有全等校验兜底）
pub(crate) fn hash_str(s: &str) -> u64 {
    hash_bytes(s.as_bytes())
}

/// 字节版 FNV-1a（同 [`hash_str`]；无 str 场景如本地图片缓存用）
pub(crate) fn hash_bytes(b: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325;
    let mut chunks = b.chunks_exact(8);
    for chunk in &mut chunks {
        let mut word = u64::from_le_bytes(chunk.try_into().unwrap());
        for _ in 0..8 {
            h = (h ^ (word & 0xff)).wrapping_mul(0x100000001b3);
            word >>= 8;
        }
    }
    for &byte in chunks.remainder() {
        h = (h ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    h
}

/// 解析事件流（含单波浪线纠正）：pulldown-cmark 0.13 的
/// ENABLE_STRIKETHROUGH 会把同段内配对的**单** `~` 也当删除线（Tag 层面
/// 单/双不可分、亦无开关），而 pi-web remark-gfm 是 `{singleTilde:false}`
/// ——CJK 文本里 `约 ~4.5%`、`5~10 倍` 这类约等/范围号极常见（2026-11-11
/// 调研 AMD 会话两整句被误标中划线）。借 offset_iter 的 End 区间（恰为
/// 闭合定界符）数 `~`：单条则把 Start/End 改写成字面 `~`，内容照常。
pub(in crate::editor::markdown) fn parse_events<'a>(src: &'a str, opts: Options) -> Vec<Event<'a>> {
    use pulldown_cmark::{Tag, TagEnd};
    let mut events: Vec<Event> = Vec::new();
    let mut strike_starts: Vec<usize> = Vec::new();
    for (ev, range) in Parser::new_ext(src, opts).into_offset_iter() {
        let ev = match ev {
            Event::Start(Tag::Strikethrough) => {
                strike_starts.push(events.len());
                Event::Start(Tag::Strikethrough)
            }
            Event::End(TagEnd::Strikethrough) => match strike_starts.pop() {
                Some(start_ix) if src[range].matches('~').count() == 1 => {
                    events[start_ix] = Event::Text("~".into());
                    Event::Text("~".into())
                }
                _ => Event::End(TagEnd::Strikethrough),
            },
            other => other,
        };
        events.push(ev);
    }
    events
}

pub(super) fn cached_blocks(src: &str, html: bool) -> std::rc::Rc<Vec<MdBlock>> {
    let key = (hash_str(src), src.len(), html);
    MD_PARSE_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        if let Some(pos) = cache.iter().rposition(|(k, _)| *k == key) {
            let entry = cache.remove(pos).expect("pos 来自刚才的迭代");
            if entry.1.0.as_str() == src {
                let blocks = std::rc::Rc::clone(&entry.1.1);
                cache.push_back(entry); // 刷新到队尾（LRU）
                return blocks;
            }
        }
        let mut opts = Options::empty();
        opts.insert(Options::ENABLE_STRIKETHROUGH);
        opts.insert(Options::ENABLE_TABLES);
        // c12: 任务列表 + GFM 自动链接；c14: YAML frontmatter 吞掉
        opts.insert(Options::ENABLE_TASKLISTS);
        opts.insert(Options::ENABLE_GFM);
        opts.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
        // v57-2: $…$ / $$…$$ 数学（pi-web remark-math parity）
        opts.insert(Options::ENABLE_MATH);
        let events = parse_events(src, opts);
        let parsed = std::rc::Rc::new(parse_blocks(&events, html));
        cache.push_back((key, (src.to_string(), std::rc::Rc::clone(&parsed))));
        while cache.len() > MD_PARSE_CACHE_CAP
            || cache.iter().map(|(k, _)| k.1).sum::<usize>() > MD_PARSE_CACHE_BUDGET_BYTES
        {
            if cache.pop_front().is_none() {
                break;
            }
        }
        parsed
    })
}

/// 探针（automation snapshot）：（条数，源文字节）
pub(crate) fn md_parse_probe() -> (usize, usize) {
    MD_PARSE_CACHE.with(|cell| {
        let cache = cell.borrow();
        (cache.len(), cache.iter().map(|(k, _)| k.1).sum::<usize>())
    })
}

/// Html/InlineHtml 的字面显示（html=false 路径）：标签原文可见。
pub(super) fn literal_runs(html_src: &str) -> Vec<Run> {
    vec![Run {
        text: html_src.to_string(),
        style: Style::Normal,
        url: None,
    }]
}

/// 超长消息（pi-web ⚠ Message content is very large）：提示行 + 纯文本
/// 滚动视图（pi-web 展开后的 pre maxHeight 420；省去一次点击）。
pub(super) fn render_oversize(src: &str, t: &Theme) -> AnyElement {
    let kb = src.len() / 1024;
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .text_color(rgb(0xca8a04))
                .text_size(px(crate::ui::tokens::fixed::MD_WARN))
                .child(SharedString::from(format!(
                    "⚠ {} ({}KB)",
                    crate::i18n::tr("messages.too_large"),
                    kb
                ))),
        )
        .child(
            div()
                .id("md-oversize")
                .max_h(px(420.))
                .overflow_y_scroll()
                .restrict_scroll_to_axis()
                .font_family(MONO_FAMILY)
                .text_size(px(crate::ui::tokens::fixed::MD_OVERSIZE))
                .line_height(relative(1.5))
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(src.to_string())),
        )
        .into_any_element()
}

/// 渲染列钳制的 flex 段（gpui 0.2.2 无带参 flex_grow，直写 style 精修）：
/// basis 0 + 指定 grow 占比 + shrink 0；min_w 用作窄面板下的保底钳制。
pub(crate) fn flex_span(grow: f32, min_w: Option<f32>) -> gpui::Div {
    let mut d = div().flex_basis(px(0.));
    d.style().flex_grow = Some(grow);
    d.style().flex_shrink = Some(0.);
    if let Some(w) = min_w {
        d = d.min_w(px(w));
    }
    d
}
