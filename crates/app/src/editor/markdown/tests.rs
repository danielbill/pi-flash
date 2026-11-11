/// 025 P1：wiki 图片展开——fence 外展开、fence 内不碰、尺寸语法取文件名。
#[test]
fn wiki_images_expanded_outside_fence_only() {
    let src = "看图\n\n![[file-a.png]]\n\n```\n![[code.png]]\n```\n\n![[b.png|300]]\n";
    let blocks = super::doc_blocks(src);
    let imgs: Vec<&str> = blocks
        .iter()
        .filter_map(|b| match b {
            MdBlock::Image { url, .. } => Some(url.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(imgs, vec!["file-a.png", "b.png"]);
}

/// 025 P1：含空格路径用 <> 包裹后 pulldown 能正常解析为图片。
#[test]
fn spaced_path_in_angle_brackets_parses_as_image() {
    let blocks = super::doc_blocks("![](<assets/文章 一/file-1.png>)");
    assert!(matches!(
        blocks.first(),
        Some(MdBlock::Image { url, .. }) if url == "assets/文章 一/file-1.png"
    ));
}

/// 裸空格路径（不包 <>）解析失败作为对照——证明包裹必要性。
#[test]
fn bare_spaced_path_fails_to_parse_as_image() {
    let blocks = super::doc_blocks("![](assets/文章 一/file-1.png)");
    assert!(!matches!(blocks.first(), Some(MdBlock::Image { .. })));
}
use super::*;

fn text_of(runs: &[Run]) -> String {
    runs.iter().map(|r| r.text.as_str()).collect()
}

fn parse(src: &str) -> Vec<MdBlock> {
    parse_with(src, true)
}

fn parse_with(src: &str, html: bool) -> Vec<MdBlock> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_GFM);
    opts.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    // v57-2: $…$ / $$…$$ 数学（pi-web remark-math parity）
    opts.insert(Options::ENABLE_MATH);
    parse_blocks(&parse_events(src, opts), html)
}

#[test]
fn headings_and_paragraphs() {
    let blocks = parse("# Title\n\nhello world");
    assert_eq!(blocks.len(), 2);
    match &blocks[0] {
        MdBlock::Heading { level, runs } => {
            assert_eq!(*level, 1);
            assert_eq!(text_of(runs), "Title");
        }
        other => panic!("{other:?}"),
    }
    match &blocks[1] {
        MdBlock::Paragraph { runs } => assert_eq!(text_of(runs), "hello world"),
        other => panic!("{other:?}"),
    }
}

/// README 头部形（`<p><img></p>` HTML 块）：doc 模式兜底必须提取出
/// 图片块（src/width/alt），纯文本剥标签后保留（回归锁 pi-flash 图片
/// 不显示——html.rs 行内路径对块内 img 只产 🖼 占位文本）。
#[test]
fn html_block_doc_extracts_img_with_width() {
    let blocks = html_block_doc(
        "<p align=\"left\"><img src=\"crates/app/assets/icon/pi-flash-256.png\" width=\"88\" alt=\"pi-flash logo\"></p>",
    );
    assert_eq!(blocks.len(), 1);
    match &blocks[0] {
        MdBlock::Image { url, alt, width } => {
            assert_eq!(url, "crates/app/assets/icon/pi-flash-256.png");
            assert_eq!(*width, Some(88.));
            assert_eq!(text_of(alt), "");
        }
        other => panic!("{other:?}"),
    }
}

/// 同上兜底的剥标签支路：img 之外的文本内容不能丢。
#[test]
fn html_block_doc_keeps_text_after_img() {
    let blocks = html_block_doc("<p><img src=\"a.png\">尾随文本</p>");
    assert_eq!(blocks.len(), 2);
    assert!(matches!(&blocks[0], MdBlock::Image { url, .. } if url == "a.png"));
    match &blocks[1] {
        MdBlock::Paragraph { runs } => assert_eq!(text_of(runs), "尾随文本"),
        other => panic!("{other:?}"),
    }
}

/// 链接可点击的前提：`[text](url)` 的 dest_url 必须随 run 保留
/// （此前 Tag::Link{..} 直接丢弃，链接只有样式没有目标）。
#[test]
fn link_keeps_dest_url() {
    let blocks = parse("see [网易转载原文](https://example.com/a?b=1) here");
    match &blocks[0] {
        MdBlock::Paragraph { runs } => {
            assert_eq!(runs.len(), 3);
            assert_eq!(runs[0].url, None);
            assert_eq!(runs[1].style, Style::Link);
            assert_eq!(runs[1].text, "网易转载原文");
            assert_eq!(runs[1].url.as_deref(), Some("https://example.com/a?b=1"));
            assert_eq!(runs[2].url, None);
        }
        other => panic!("{other:?}"),
    }
}

/// GFM autolink：裸 URL 提为 Link run，目标 = 文本本身。
#[test]
fn autolink_url_is_text() {
    let blocks = parse("visit https://example.com/x. ok");
    match &blocks[0] {
        MdBlock::Paragraph { runs } => {
            assert_eq!(runs.len(), 3);
            assert_eq!(runs[1].style, Style::Link);
            assert_eq!(runs[1].text, "https://example.com/x");
            assert_eq!(runs[1].url.as_deref(), Some("https://example.com/x"));
        }
        other => panic!("{other:?}"),
    }
}

/// 链接内嵌粗体：样式合成 Bold，但 URL 随样式栈保留（点击不丢）。
#[test]
fn nested_bold_in_link_keeps_url() {
    let blocks = parse("[**bold link**](https://example.com)");
    match &blocks[0] {
        MdBlock::Paragraph { runs } => {
            assert_eq!(runs.len(), 1);
            assert_eq!(runs[0].style, Style::Bold);
            assert_eq!(runs[0].url.as_deref(), Some("https://example.com"));
        }
        other => panic!("{other:?}"),
    }
}

/// 行内 `<a href>`：href 经 InlineHtml 分支随 run 保留。
#[test]
fn inline_html_link_keeps_href() {
    let blocks = parse_with("<a href=\"https://x.example\">y</a>", true);
    match &blocks[0] {
        MdBlock::Paragraph { runs } => {
            assert_eq!(runs.len(), 1);
            assert_eq!(runs[0].style, Style::Link);
            assert_eq!(runs[0].url.as_deref(), Some("https://x.example"));
        }
        other => panic!("{other:?}"),
    }
}

// ---- as_file_path：消息内文件路径点击的识别层 ----

#[test]
fn file_path_accepts_absolute_and_relative() {
    // Windows 盘符（含空格 + CJK——截图场景）
    assert_eq!(
        as_file_path(r"D:\my_obsidian\文章\1 每日博文\x.md").as_deref(),
        Some(r"D:\my_obsidian\文章\1 每日博文\x.md")
    );
    assert_eq!(as_file_path("D:/a/b.md").as_deref(), Some("D:/a/b.md"));
    // UNC / POSIX 根
    assert_eq!(
        as_file_path(r"\\server\share\x.txt").as_deref(),
        Some(r"\\server\share\x.txt")
    );
    assert_eq!(
        as_file_path("/usr/local/bin/zsh").as_deref(),
        Some("/usr/local/bin/zsh")
    );
    // 含分隔符的相对路径
    assert_eq!(as_file_path("src/foo.rs").as_deref(), Some("src/foo.rs"));
    assert_eq!(as_file_path(r"..\x.py").as_deref(), Some(r"..\x.py"));
}

#[test]
fn file_path_strips_line_col_suffix() {
    assert_eq!(
        as_file_path("src/app.rs:120").as_deref(),
        Some("src/app.rs")
    );
    assert_eq!(
        as_file_path("src/app.rs:120:5").as_deref(),
        Some("src/app.rs")
    );
    // 盘符冒号不受影响
    assert_eq!(as_file_path(r"C:\x.md").as_deref(), Some(r"C:\x.md"));
    // 剥完失去分隔符 = 纯文件名，不认
    assert_eq!(as_file_path("foo.md:12"), None);
}

#[test]
fn file_path_rejects_non_paths() {
    // 纯文件名/普通词：写过的文件已有轮末 chips，防误判
    assert_eq!(as_file_path("README.md"), None);
    assert_eq!(as_file_path("TODO"), None);
    assert_eq!(as_file_path("npm install"), None);
    // 其他 scheme
    assert_eq!(as_file_path("https://example.com/a"), None);
    assert_eq!(as_file_path("mailto:a@b.c"), None);
    assert_eq!(as_file_path("#anchor"), None);
    assert_eq!(as_file_path(""), None);
}

#[test]
fn file_path_quotes_and_file_url() {
    // 成对引号包裹
    assert_eq!(as_file_path("'D:\\x y.md'").as_deref(), Some("D:\\x y.md"));
    assert_eq!(
        as_file_path("\"src/a b.rs\"").as_deref(),
        Some("src/a b.rs")
    );
    // file:// URL：盘符 / POSIX 根 / UNC
    assert_eq!(
        as_file_path("file:///C:/Users/x.md").as_deref(),
        Some("C:/Users/x.md")
    );
    assert_eq!(
        as_file_path("file:///home/u/x").as_deref(),
        Some("/home/u/x")
    );
    assert_eq!(
        as_file_path("file://server/share/x.txt").as_deref(),
        Some(r"\\server\share\x.txt")
    );
    // 百分号解码
    assert_eq!(
        as_file_path("file:///D:/%E6%96%87/x.md").as_deref(),
        Some("D:/文/x.md")
    );
}

/// DOC_MODE 恢复 guard：断言失败也要复位，免得毒化同进程其他测试。
struct DocModeGuard;
impl Drop for DocModeGuard {
    fn drop(&mut self) {
        set_doc_mode(false);
    }
}

/// pulldown 0.13 包裹式 HTML 块回归锁（README 头部形态）：块级 HTML 被
/// Start(HtmlBlock) 包裹后，此前落进 parse_block 兜底整段被吞——图片
/// 和 tagline 一起消失。此测锁住「包裹内容必须走到 doc 分发」；其余
/// 测试均不含块级 HTML，不受并行下 DOC_MODE 翻转影响。
#[test]
fn html_block_wrapped_reaches_doc_dispatch() {
    let _g = DocModeGuard;
    set_doc_mode(true);
    let blocks = parse_with(
        "# t\n\n<p align=\"left\"><img src=\"a.png\" width=\"88\" alt=\"l\"></p>\ntagline text\n\n## h\n",
        true,
    );
    assert_eq!(
        blocks.len(),
        4,
        "heading + Image + tagline Paragraph + heading"
    );
    match &blocks[1] {
        MdBlock::Image { url, width, .. } => {
            assert_eq!(url, "a.png");
            assert_eq!(*width, Some(88.));
        }
        other => panic!("{other:?}"),
    }
    match &blocks[2] {
        // CommonMark type-6 HTML 块到空行为止：下一行 tagline 同属块内，
        // 剥标签后保留为段落（Zed 截图同款：链接语法按原文显示）
        MdBlock::Paragraph { runs } => assert_eq!(text_of(runs), "tagline text"),
        other => panic!("{other:?}"),
    }
}

/// 非 doc 模式（聊天 html=true 不存在，此测直打 html::blocks 分支）：
/// 包裹式 HTML 块不能再整段消失。
#[test]
fn html_block_wrapped_reaches_html_subset_dispatch() {
    let blocks = parse_with("<div>hello</div>\n\nafter", true);
    assert!(!blocks.is_empty(), "块级 HTML 不得整段丢失");
    assert!(
        blocks
            .iter()
            .any(|b| matches!(b, MdBlock::Paragraph { runs } if text_of(runs).contains("hello")))
    );
    match &blocks[blocks.len() - 1] {
        MdBlock::Paragraph { runs } => assert_eq!(text_of(runs), "after"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn bold_and_code_runs() {
    let blocks = parse("a **b** `c` d");
    match &blocks[0] {
        MdBlock::Paragraph { runs } => {
            let styles: Vec<Style> = runs.iter().map(|r| r.style).collect();
            assert_eq!(
                styles,
                vec![
                    Style::Normal,
                    Style::Bold,
                    Style::Normal,
                    Style::Code,
                    Style::Normal
                ]
            );
            assert_eq!(text_of(runs), "a b c d");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn code_block_keeps_newlines_and_lang() {
    let blocks = parse("```rust\nfn a() {}\n```");
    match &blocks[0] {
        MdBlock::Code { lang, code } => {
            assert_eq!(lang, "rust");
            assert_eq!(code, "fn a() {}\n");
        }
        other => panic!("{other:?}"),
    }
}

/// 回归（2026-10-12 用户实测）：紧凑列表里的**行内元素整段丢失**——
/// 解析器在列表项分支只认 `Event::Text`，`Event::Code`/强调/链接全被
/// `_ => *i += 1` 静默吞掉，一条 `- \`a\`：\`b\`` 的列表渲染出来只剩「：、」。
#[test]
fn list_items_keep_inline_runs() {
    let blocks = parse("- `a`：`b`\n- 普通 **粗体** 与 `code`\n- [链接](https://x.y)");
    let items: Vec<&MdBlock> = blocks
        .iter()
        .filter(|b| matches!(b, MdBlock::ListItem { .. }))
        .collect();
    assert_eq!(items.len(), 3);
    let styles = |ix: usize| -> Vec<Style> {
        match items[ix] {
            MdBlock::ListItem { runs, .. } => runs.iter().map(|r| r.style).collect(),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(
        text_of(match items[0] {
            MdBlock::ListItem { runs, .. } => runs,
            _ => unreachable!(),
        }),
        "a：b",
        "行内 code 必须保留"
    );
    assert!(
        styles(0).contains(&Style::Code),
        "code run 保留：{:?}",
        styles(0)
    );
    assert!(
        styles(1).contains(&Style::Bold),
        "粗体保留：{:?}",
        styles(1)
    );
    assert!(
        styles(1).contains(&Style::Code),
        "code run 保留：{:?}",
        styles(1)
    );
    assert!(
        styles(2).contains(&Style::Link),
        "链接保留：{:?}",
        styles(2)
    );
}

#[test]
fn list_items_with_markers() {
    let blocks = parse("- one\n- two\n\n1. x");
    let items: Vec<&MdBlock> = blocks
        .iter()
        .filter(|b| matches!(b, MdBlock::ListItem { .. }))
        .collect();
    assert_eq!(items.len(), 3);
    match items[0] {
        MdBlock::ListItem {
            marker,
            runs,
            depth,
            ..
        } => {
            assert_eq!(marker, "\u{2022}");
            assert_eq!(*depth, 0);
            assert_eq!(text_of(runs), "one");
        }
        other => panic!("{other:?}"),
    }
    match items[2] {
        MdBlock::ListItem { marker, .. } => assert_eq!(marker, "1."),
        other => panic!("{other:?}"),
    }
}

#[test]
fn code_highlight_produces_colored_runs() {
    for dark in [false, true] {
        let segs = highlight_segments("fn main() {}\n", "rust", dark);
        assert!(!segs.is_empty(), "dark={dark}");
        // keyword "fn" should be styled differently from plain text
        assert!(segs.iter().any(|(t, _)| t.contains("fn")));
        assert!(
            segs.len() > 1,
            "expected multiple colored segments, dark={dark}"
        );
    }
}

#[test]
fn highlight_cache_hits_return_identical_segments() {
    let code = "fn main() {\n    let x = 1;\n}\nlet y = 2;\n";
    let direct = highlight_segments(code, "rust", true);
    let first = cached_highlight_segments(code, "rust", true);
    let second = cached_highlight_segments(code, "rust", true); // 应命中
    assert_eq!(&direct, &first[..]);
    assert_eq!(&direct, &second[..]);
    // 明暗/语言不串缓存
    let light = cached_highlight_segments(code, "rust", false);
    assert_eq!(&highlight_segments(code, "rust", false), &light[..]);
    let py = cached_highlight_segments(code, "python", true);
    assert_eq!(&highlight_segments(code, "python", true), &py[..]);
}

/// 开销留档（--nocapture 看）：大代码块 syntect 冷跑 vs 缓存命中。
/// Chat 每帧重建可见元素树，此差值就是原先编辑/拖选时每帧白打的成本。
#[test]
fn highlight_cache_cost_probe() {
    let unit = "fn generated_item(n: usize) -> usize {\n    let mut acc = n ^ 0x9E37_79B9;\n    for i in 0..8 {\n        acc = acc.rotate_left(7).wrapping_mul(0x100_0000_001B3);\n    }\n    match acc % 5 { 0 => acc, 1 => acc + 1, 2 => !acc, 3 => acc >> 2, _ => acc << 2 }\n}\n";
    let code = unit.repeat(40); // ~320 行，AI 回复常见体量
    warm_up(); // syn() 一次性语法集装载不计入
    let t = std::time::Instant::now();
    let cold = highlight_segments(&code, "rust", true);
    let d_cold = t.elapsed();
    let _ = cached_highlight_segments(&code, "rust", true);
    let t = std::time::Instant::now();
    for _ in 0..100 {
        let _ = cached_highlight_segments(&code, "rust", true);
    }
    let d_hot = t.elapsed();
    eprintln!(
        "[perf] highlight {}B/{}lines: syntect {:.2} ms, cached hit {:.4} ms",
        code.len(),
        cold.iter().filter(|(s, _)| s.contains('\n')).count(),
        d_cold.as_secs_f64() * 1e3,
        d_hot.as_secs_f64() * 1e3 / 100.
    );
    assert!(d_cold > d_hot / 100, "缓存命中应显著快于 syntect 冷跑");
}

#[test]
fn table_parses_head_and_rows() {
    let blocks = parse("| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |");
    match &blocks[0] {
        MdBlock::Table { head, rows } => {
            assert_eq!(head.len(), 2);
            assert_eq!(text_of(&head[0]), "a");
            assert_eq!(rows.len(), 2);
            assert_eq!(text_of(&rows[1][0]), "3");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn image_parses_url_and_alt() {
    let blocks = parse("![alt text](img.png)");
    match &blocks[0] {
        MdBlock::Image { url, alt, width } => {
            assert_eq!(url, "img.png");
            assert_eq!(text_of(alt), "alt text");
            assert_eq!(*width, None);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn strikethrough_run() {
    let blocks = parse("~~gone~~ kept");
    match &blocks[0] {
        MdBlock::Paragraph { runs } => {
            assert!(
                runs.iter()
                    .any(|r| r.style == Style::Strike && r.text == "gone")
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn task_list_marker_parsed() {
    let blocks = parse(
        "- [x] done
- [ ] todo",
    );
    let items: Vec<&MdBlock> = blocks
        .iter()
        .filter(|b| matches!(b, MdBlock::ListItem { .. }))
        .collect();
    assert_eq!(items.len(), 2);
    match items[0] {
        MdBlock::ListItem { task, .. } => assert_eq!(*task, Some(true)),
        other => panic!("{other:?}"),
    }
    match items[1] {
        MdBlock::ListItem { task, .. } => assert_eq!(*task, Some(false)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn gfm_autolink_becomes_link_run() {
    let blocks = parse("see https://example.com/x now");
    match &blocks[0] {
        MdBlock::Paragraph { runs } => {
            assert!(
                runs.iter()
                    .any(|r| r.style == Style::Link && r.text.contains("example.com"))
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn yaml_frontmatter_swallowed() {
    let blocks = parse(
        "---
title: x
---

hello",
    );
    assert_eq!(blocks.len(), 1);
    match &blocks[0] {
        MdBlock::Paragraph { runs } => assert_eq!(text_of(runs), "hello"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn single_tilde_is_not_strikethrough() {
    // pi-web remark-gfm {singleTilde:false}：单 `~` 是约等/范围号（“约
    // ~4.5%”、“5~10 倍”），同段内两个单 `~` 配对也不得误标删除线
    // （2026-11-11 调研 AMD 会话实测：两整句被中划线）；`~~双波浪~~` 才是。
    // 案例一取自真实会话第二段原文（无 `$`，不掺数学行为）：
    let src = "9 月 MI455X 还传提价 ~10%，说明短期供不应求，但长期定价天花板明显低于 NVIDIA（75% 毛利率 vs AMD 55~56%）";
    match &parse(src)[0] {
        MdBlock::Paragraph { runs } => {
            assert!(!runs.iter().any(|r| r.style == Style::Strike));
            // 字面波浪线一字不丢
            assert_eq!(text_of(runs), src);
        }
        other => panic!("{other:?}"),
    }
    // 案例二：真实会话第一段（含 `$`，数学行为不掺判，只锁删除线）：
    let blocks = parse(
        "（AI 加速器份额 ~4.5%）；NVIDIA 单季 $89B、YoY +117% 仍在加速，订单簿 $500B~$1T 是 AMD 的 5~10 倍。",
    );
    match &blocks[0] {
        MdBlock::Paragraph { runs } => {
            assert!(!runs.iter().any(|r| r.style == Style::Strike));
        }
        other => panic!("{other:?}"),
    }
    // `~~双波浪~~` 仍是删除线：见 strikethrough_run，不再重复。
}

#[test]
fn inline_math_becomes_marker_run() {
    let blocks = parse("能量公式 $E=mc^2$ 很有名");
    match &blocks[0] {
        MdBlock::Paragraph { runs } => {
            assert!(
                runs.iter()
                    .any(|r| r.style == Style::Math && r.text == "E=mc^2")
            );
            assert!(
                runs.iter()
                    .any(|r| r.style == Style::Normal && r.text.contains("能量公式"))
            );
        }
        other => panic!("{other:?}"),
    }
}

/// 渲染冒烟：vs Dark+ 高亮 / 行内 code 盒 / 引用 muted / 任务框 / hr
/// 渐变各路径在明暗两主题下都不 panic（vs_dark_plus 是代码构建主题）。
#[test]
fn render_smoke_all_paths_both_modes() {
    let src = "# 标题\n\n正文 **bold** `code` [link](https://x.y) $E=mc^2$\n\n```rust\nfn a() {}\n```\n\n> 引用 **内粗**\n\n- [x] done\n- [ ] todo\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n---\n";
    for t in crate::appearance::theme::ALL {
        let el = render(src, &t.1, false);
        let _ = el.into_any_element();
    }
}

#[test]
fn display_math_is_block() {
    let blocks = parse(
        r"前文

$$
\frac{1}{2}
$$",
    );
    let has_display = blocks.iter().any(|b| {
        matches!(b, MdBlock::Paragraph { runs }
            if runs.iter().any(|r| r.style == Style::DisplayMath && r.text.contains("frac")))
    });
    assert!(
        has_display,
        "multiline $$ must surface as DisplayMath marker run"
    );
}

#[test]
fn user_mode_shows_html_literals() {
    // render_user：标签不渲染、原文可见（用户消息凭证原则）
    let blocks = parse_with("<b>粗体</b> 保持", false);
    match &blocks[0] {
        MdBlock::Paragraph { runs } => {
            let all = runs.iter().map(|r| r.text.as_str()).collect::<String>();
            assert!(all.contains("<b>粗体</b>"), "tags must stay literal: {all}");
            assert!(runs.iter().all(|r| r.style == Style::Normal));
        }
        other => panic!("{other:?}"),
    }
    // html=true 路径依旧渲染（assistant）
    let blocks2 = parse_with("<b>粗体</b> 保持", true);
    match &blocks2[0] {
        MdBlock::Paragraph { runs } => {
            assert!(
                !runs.iter().any(|r| r.text.contains("<b>")),
                "tags consumed"
            );
            assert!(runs.iter().any(|r| r.style == Style::Bold));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn chinese_text_roundtrip() {
    let blocks = parse("\u{4e2d}\u{6587}**\u{52a0}\u{7c97}**\u{6d4b}\u{8bd5}");
    match &blocks[0] {
        MdBlock::Paragraph { runs } => {
            assert_eq!(
                text_of(runs),
                "\u{4e2d}\u{6587}\u{52a0}\u{7c97}\u{6d4b}\u{8bd5}"
            );
            assert_eq!(runs[1].style, Style::Bold);
        }
        other => panic!("{other:?}"),
    }
}
