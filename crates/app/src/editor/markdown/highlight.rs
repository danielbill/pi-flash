//! syntect 代码高亮集成：语法集OnceLock、双主题、逐段高亮与 thread_local
//! LRU 缓存（cap 128）。render_code_block 消费。

use super::*;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color, ThemeSet};
use syntect::parsing::SyntaxSet;

/// Syntax highlighting state (loaded once; ~100ms cold, cached for process life).
struct Syn {
    ps: SyntaxSet,
    ts: ThemeSet,
    /// 深色主题（VS Code Dark+ 近似，代码构建——syntect 默认主题集无
    /// vscDarkPlus，base16-ocean.dark 色系偏蓝灰差距大）
    dark: syntect::highlighting::Theme,
}

/// 启动期预热（startup::spawn_boot_tasks 的后台线程调用）：syntect 语法集
/// + 主题集冷加载 ~百 ms，此前落在首次 md 渲染帧里（第一次渲染卡顿）。
/// OnceLock 线程安全——预热未完成时 UI 线程首渲仍会等待，但启动后用户
/// 手速开文件必然晚于后台装载完成。
pub fn warm_up() {
    let syn = syn();
    let _ = syn.ps.syntaxes().len();
}

fn syn() -> &'static Syn {
    static SYN: std::sync::OnceLock<Syn> = std::sync::OnceLock::new();
    SYN.get_or_init(|| {
        // start from the built-in defaults (includes plain text + common langs),
        // then allow extra syntaxes from an optional assets folder
        let mut builder = SyntaxSet::load_defaults_newlines().into_builder();
        builder.add_from_folder("assets/syntaxes", false).ok();
        let ps = builder.build();
        let ts = ThemeSet::load_defaults();
        let dark = vs_dark_plus();
        Syn { ps, ts, dark }
    })
}

/// 语法高亮主题按 UI 明暗切换（pi-web: isDark ? vscDarkPlus : vs）。
const LIGHT_THEME: &str = "InspiredGitHub";

/// VS Code Dark+ 调色板（dark_plus.json 的 token 色），按选择器前缀匹配。
/// 空选择器 = 全局默认前景 #D4D4D4。
fn vs_dark_plus() -> syntect::highlighting::Theme {
    use syntect::highlighting::{Color, StyleModifier, ThemeItem};
    let fg = |rgb24: u32| Color {
        r: (rgb24 >> 16) as u8,
        g: (rgb24 >> 8) as u8,
        b: rgb24 as u8,
        a: 0xff,
    };
    let item = |selector: &str, rgb24: u32| ThemeItem {
        scope: selector.parse().expect("valid scope selector"),
        style: StyleModifier {
            foreground: Some(fg(rgb24)),
            background: None,
            font_style: None,
        },
    };
    let table: &[(&str, u32)] = &[
        ("", 0xd4d4d4),
        ("comment", 0x6a9955),
        ("string", 0xce9178),
        ("string.regexp", 0xd16969),
        ("constant.numeric", 0xb5cea8),
        ("constant.character.escape", 0xd7ba7d),
        ("constant.language", 0x569cd6),
        ("keyword.control", 0xc586c0),
        ("keyword", 0x569cd6),
        ("storage", 0x569cd6),
        ("entity.name.function", 0xdcdcaa),
        ("support.function", 0xdcdcaa),
        ("entity.name.type", 0x4ec9b0),
        ("entity.name.class", 0x4ec9b0),
        ("entity.name.struct", 0x4ec9b0),
        ("entity.name.enum", 0x4ec9b0),
        ("support.type", 0x4ec9b0),
        ("support.class", 0x4ec9b0),
        ("entity.name.tag", 0x569cd6),
        ("entity.other.attribute-name", 0x9cdcfe),
        ("variable", 0x9cdcfe),
        ("support.variable", 0x9cdcfe),
    ];
    syntect::highlighting::Theme {
        name: Some("vs-dark-plus".into()),
        author: Some("pi-flash".into()),
        settings: Default::default(),
        scopes: table.iter().map(|(s, c)| item(s, *c)).collect(),
    }
}

/// Highlight `code` and return colored text segments (never spans across
/// lines — callers rely on per-line boundaries for the gutter).
pub(super) fn highlight_segments(code: &str, lang: &str, dark: bool) -> Vec<(String, [u8; 3])> {
    let syn = syn();
    let syntax = lang
        .split(',')
        .next()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .and_then(|l| syn.ps.find_syntax_by_token(l))
        .unwrap_or_else(|| syn.ps.find_syntax_plain_text());
    let theme = if dark {
        &syn.dark
    } else {
        syn.ts
            .themes
            .get(LIGHT_THEME)
            .unwrap_or_else(|| syn.ts.themes.values().next().unwrap())
    };
    let mut hl = HighlightLines::new(syntax, theme);
    let mut out: Vec<(String, [u8; 3])> = Vec::new();
    for line in syntect::util::LinesWithEndings::from(code) {
        let Ok(ranges) = hl.highlight_line(line, &syn.ps) else {
            continue;
        };
        // 每行起一段新 run（行号 gutter 需要按行插入）
        let mut line_open = false;
        for (style, text) in ranges {
            let Color { r, g, b, a: _ } = style.foreground;
            if let Some(last) = out.last_mut() {
                if line_open && last.1 == [r, g, b] {
                    last.0.push_str(text);
                    continue;
                }
            }
            out.push((text.to_string(), [r, g, b]));
            line_open = true;
        }
    }
    out
}

// 高亮结果缓存：gpui List 每帧对可见条目重建元素树，syntect 正则状态机
// 对大代码块是 ms 级纯 CPU，输入不变时每帧白打（打字/拖选/流式动画/光标
// 闪烁任一原因的重绘都会连坐）。key = 代码哈希 + 长度 + 语言哈希 + 明暗，
// 命中后源文全等校验防碰撞；线程局部 VecDeque LRU（与 MD_PARSE_CACHE
// 同款，元素构建只在主线程）。流式期间代码块逐 delta 变化，最多占满队头
// 被挤出，不影响命中态。
thread_local! {
    static HL_CACHE: std::cell::RefCell<
        std::collections::VecDeque<
            ((u64, usize, u64, bool), (String, String, std::rc::Rc<Vec<(String, [u8; 3])>>)),
        >,
    > = const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}
const HL_CACHE_CAP: usize = 128;
/// 字节预算（代码源 32MB）：大代码块场景 128 条只按条数封顶时稳态可到几十
/// MB；超预算从头驱逐（LRU）。按 key.1（代码字节数）计，语言串占比极小忽略
const HL_CACHE_BUDGET_BYTES: usize = 32 * 1024 * 1024;

/// [`highlight_segments`] 的 LRU 包装（渲染热路径一律走这里）。
pub(crate) fn cached_highlight_segments(
    code: &str,
    lang: &str,
    dark: bool,
) -> std::rc::Rc<Vec<(String, [u8; 3])>> {
    let key = (hash_str(code), code.len(), hash_str(lang), dark);
    HL_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        if let Some(pos) = cache.iter().rposition(|(k, _)| *k == key) {
            let entry = cache.remove(pos).expect("pos 来自刚才的迭代");
            if entry.1.0.as_str() == code && entry.1.1.as_str() == lang {
                let segs = std::rc::Rc::clone(&entry.1.2);
                cache.push_back(entry); // 刷新到队尾（LRU）
                return segs;
            }
        }
        let segs = std::rc::Rc::new(highlight_segments(code, lang, dark));
        cache.push_back((
            key,
            (
                code.to_string(),
                lang.to_string(),
                std::rc::Rc::clone(&segs),
            ),
        ));
        while cache.len() > HL_CACHE_CAP
            || cache.iter().map(|(k, _)| k.1).sum::<usize>() > HL_CACHE_BUDGET_BYTES
        {
            if cache.pop_front().is_none() {
                break;
            }
        }
        segs
    })
}

/// 探针（automation snapshot）：（条数，代码源字节）
pub(crate) fn hl_cache_probe() -> (usize, usize) {
    HL_CACHE.with(|cell| {
        let cache = cell.borrow();
        (cache.len(), cache.iter().map(|(k, _)| k.1).sum::<usize>())
    })
}

/// gpui-component 组件盘语法色（issue#4 续）：chat 的 TextView 代码块与
/// 023 CodeEditor 都吃 `sync_gpui_tokens` 注入的这一份。与上面 syntect
/// 两盘**同源同值**——暗盘 = vs_dark_plus（=pi-web 的 vscDarkPlus），浅盘
/// = InspiredGitHub（syntect 默认主题集，色值实测 dump）。全应用代码一个
/// 色语言，与 pi-web 同思路（isDark ? vscDarkPlus : vs），无每主题配色。
///
/// SyntaxColors/ThemeStyle 字段私有，走 serde 直构造（与组件
/// default-theme.json 同构，未知字段忽略，组件加槽位不破此处）。
pub(crate) fn component_syntax_colors(dark: bool) -> gpui_component::highlighter::SyntaxColors {
    const DARK: &str = r##"{
        "primary":            {"color": "#d4d4d4"},
        "keyword":            {"color": "#569cd6"},
        "function":           {"color": "#dcdcaa"},
        "string":             {"color": "#ce9178"},
        "string.escape":      {"color": "#d7ba7d"},
        "string.regex":       {"color": "#d16969"},
        "string.special":     {"color": "#d7ba7d"},
        "string.special.symbol": {"color": "#d7ba7d"},
        "comment":            {"color": "#6a9955"},
        "comment.doc":        {"color": "#6a9955"},
        "number":             {"color": "#b5cea8"},
        "constant":           {"color": "#b5cea8"},
        "boolean":            {"color": "#569cd6"},
        "type":               {"color": "#4ec9b0"},
        "enum":               {"color": "#4ec9b0"},
        "constructor":        {"color": "#4ec9b0"},
        "variable":           {"color": "#9cdcfe"},
        "variable.special":   {"color": "#569cd6"},
        "property":           {"color": "#9cdcfe"},
        "attribute":          {"color": "#9cdcfe"},
        "tag":                {"color": "#569cd6"},
        "title":              {"color": "#dcdcaa"},
        "text.literal":       {"color": "#ce9178"},
        "operator":           {"color": "#d4d4d4"},
        "punctuation":        {"color": "#d4d4d4"},
        "punctuation.bracket": {"color": "#d4d4d4"},
        "punctuation.delimiter": {"color": "#d4d4d4"},
        "punctuation.list_marker": {"color": "#d4d4d4"},
        "punctuation.special": {"color": "#d4d4d4"},
        "link_text":          {"color": "#569cd6"},
        "link_uri":           {"color": "#9cdcfe"},
        "label":              {"color": "#c8c8c8"},
        "preproc":            {"color": "#c586c0"}
    }"##;
    const LIGHT: &str = r##"{
        "primary":            {"color": "#323232"},
        "keyword":            {"color": "#a71d5d"},
        "function":           {"color": "#795da3"},
        "string":             {"color": "#183691"},
        "string.escape":      {"color": "#a71d5d"},
        "string.regex":       {"color": "#a71d5d"},
        "string.special":     {"color": "#a71d5d"},
        "string.special.symbol": {"color": "#a71d5d"},
        "comment":            {"color": "#969896"},
        "comment.doc":        {"color": "#969896"},
        "number":             {"color": "#0086b3"},
        "constant":           {"color": "#0086b3"},
        "boolean":            {"color": "#0086b3"},
        "type":               {"color": "#0086b3"},
        "enum":               {"color": "#0086b3"},
        "constructor":        {"color": "#0086b3"},
        "variable":           {"color": "#323232"},
        "variable.special":   {"color": "#323232"},
        "property":           {"color": "#323232"},
        "attribute":          {"color": "#795da3"},
        "tag":                {"color": "#63a35c"},
        "title":              {"color": "#795da3"},
        "text.literal":       {"color": "#183691"},
        "operator":           {"color": "#323232"},
        "punctuation":        {"color": "#323232"},
        "punctuation.bracket": {"color": "#323232"},
        "punctuation.delimiter": {"color": "#323232"},
        "punctuation.list_marker": {"color": "#323232"},
        "punctuation.special": {"color": "#323232"},
        "link_text":          {"color": "#795da3"},
        "link_uri":           {"color": "#183691"},
        "label":              {"color": "#323232"},
        "preproc":            {"color": "#a71d5d"}
    }"##;
    serde_json::from_str(if dark { DARK } else { LIGHT })
        .expect("component_syntax_colors: 内置 JSON 必须合法")
}
