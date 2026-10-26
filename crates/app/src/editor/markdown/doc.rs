//! 文档预览模式（doc mode）：DOC_MODE/IMG_BASE 开关、wiki 图片展开、
//! 文档渲染入口（render_doc_item/render_preview_body）与活动装饰同步。

use super::*;
use crate::appearance::theme::Theme;
use gpui::{AnyElement, ClickEvent, div, px, rgb};
use std::path::PathBuf;

/// 预览区 click（进源码）与拖拽手势的分界：down→up 位移不超过
/// [`CLICK_MAX_DRAG`] 算 genuine click，超过算拖拽。与 gpui div.rs
/// DRAG_THRESHOLD(2.) 同量级，取 4 宽松一点防手抖误判。
const CLICK_MAX_DRAG: f32 = 4.0;

fn click_within_drag(down: gpui::Point<gpui::Pixels>, up: gpui::Point<gpui::Pixels>) -> bool {
    let dx = f32::from(up.x) - f32::from(down.x);
    let dy = f32::from(up.y) - f32::from(down.y);
    dx * dx + dy * dy <= CLICK_MAX_DRAG * CLICK_MAX_DRAG
}

static DOC_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(super) fn doc_mode() -> bool {
    DOC_MODE.load(std::sync::atomic::Ordering::Relaxed)
}

pub(super) fn set_doc_mode(on: bool) {
    DOC_MODE.store(on, std::sync::atomic::Ordering::Relaxed);
}

static IMG_BASE: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);

fn img_base() -> Option<std::path::PathBuf> {
    IMG_BASE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// 相对图片路径解析（025 P1）：md 目录直查 → md 同级 `assets/` 直层 →
/// `assets/<子目录>/`同名匹配（Obsidian 归档结构 assets/文章名/file.png，
/// wiki 引用多为裸文件名）→ 都找不到回退原路径（占位）。
pub(super) fn resolve_relative_image(url: &std::path::Path) -> std::path::PathBuf {
    let Some(base) = img_base() else {
        return url.to_path_buf();
    };
    let direct = base.join(url);
    if direct.exists() {
        return direct;
    }
    let assets = base.join("assets");
    if let Ok(rd) = std::fs::read_dir(&assets) {
        let flat = assets.join(url);
        if flat.exists() {
            return flat;
        }
        let Some(name) = url.file_name() else {
            return url.to_path_buf();
        };
        for e in rd.flatten() {
            let cand = e.path().join(name);
            if cand.is_file() {
                return cand;
            }
        }
    }
    url.to_path_buf()
}

/// 文档模式标题字号（Zed/GitHub 尺度，相对预览正文字号）。
pub(super) fn doc_size_for_level(level: u8, base: f32) -> f32 {
    match level {
        1 => base * 2.05,
        2 => base * 1.5,
        3 => base * 1.26,
        4 => base * 1.1,
        _ => base,
    }
}

/// HTML 块的文档模式兜底（html.rs 块级解析对「<p><img></p>+裸文本」这类
/// 连排块会整段丢失——README 头部即此形态）：提取全部 <img> 为图片块，
/// 剩余标签剥掉、内文保留为段落。
pub(super) fn html_block_doc(h: &str) -> Vec<MdBlock> {
    fn attr(tag: &str, name: &str) -> Option<String> {
        let lower = tag.to_ascii_lowercase();
        let key = format!("{name}=");
        let i = lower.find(&key)?;
        let rest = &tag[i + key.len()..];
        let quote = rest.chars().next()?;
        let val = if quote == '"' || quote == '\'' {
            let end = rest[1..].find(quote)? + 1;
            &rest[1..end]
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            &rest[..end]
        };
        Some(val.to_string())
    }
    let mut out = Vec::new();
    let mut rest = h;
    while let Some(pos) = rest.find("<img") {
        let Some(end) = rest[pos..].find('>') else {
            break;
        };
        let tag = &rest[pos..=pos + end];
        if let Some(url) = attr(tag, "src") {
            let width = attr(tag, "width")
                .map(|w| w.trim().trim_end_matches("px").trim().to_string())
                .and_then(|w| w.parse::<f32>().ok());
            out.push(MdBlock::Image {
                url,
                width,
                alt: Vec::new(),
            });
        }
        rest = &rest[pos + end + 1..];
    }
    // 剥标签留内文
    let mut text = String::new();
    let mut in_tag = false;
    for ch in h.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    let text = text
        .split('\n')
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if !text.is_empty() {
        out.push(MdBlock::Paragraph {
            runs: vec![Run {
                text,
                style: Style::Normal,
                url: None,
            }],
        });
    }
    out
}

/// 文档预览虚拟化（抄 zed thread_view 的 list 架构，v60）：文件预览不再
/// 一次性构建整棵元素树（progress.md 784 块全量构建 ≈ 30ms/帧，滚动必
/// 卡），改由 ListState 每帧只建可视条目。条目 = 单个 md 块（zed 的
/// MarkdownElement 同样按块出元素、块自带 margins）。两步交给调用方接
/// gpui::list：
/// - [`doc_blocks`]：解析（LRU 缓存）+ 取块数当 item_count；
/// - [`render_doc_item`]：list 条目闭包，懒渲染第 ix 块。
pub fn doc_blocks(src: &str) -> std::rc::Rc<Vec<MdBlock>> {
    // 025 P1：Obsidian wiki 图片 `![[file.png]]` → 标准 `![](file.png)`。
    // pulldown-cmark 不认 wiki 语法（Obsidian 迁移库大量存在）；在解析
    // 前统一展开（fence 代码块内不碰，纯 `[[wiki链接]]` 不碰）。
    let expanded = expand_wiki_images(src);
    set_doc_mode(true);
    let blocks = cached_blocks(&expanded, true);
    set_doc_mode(false);
    blocks
}

/// 把源码里的 wiki 图片引用展开为标准 markdown（逐行，fence 感知）。
/// `![[a.png]]` → `![](a.png)`；`![[a.png|300]]` 取尺寸前的文件名。
/// 另：独占一行的图片引用（wiki 或标准式）前后补空行——Obsidian 习惯
/// 回车即图（图与文字间常无空行），GFM 同段落不产图片块，不补分割
/// 预览就不出图（025 P1 实翻车）。
fn expand_wiki_images(src: &str) -> String {
    let mut out = String::with_capacity(src.len() + 32);
    let mut in_fence = false;
    for line in src.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            out.push_str(line);
            continue;
        }
        if in_fence {
            out.push_str(line);
            continue;
        }
        let expanded = expand_wiki_line(line);
        let trimmed = expanded.trim();
        if is_standalone_image_ref(trimmed) {
            let blank_before = out.is_empty() || out.ends_with("\n\n");
            if !blank_before {
                out.push('\n');
            }
            out.push_str(&expanded);
            if !expanded.ends_with('\n') {
                out.push('\n');
            }
            out.push('\n');
        } else {
            out.push_str(&expanded);
        }
    }
    out
}

/// 整行恰为一条图片引用：`![[…]]` 或 `![](…)`（闭合到行尾）。
fn is_standalone_image_ref(line: &str) -> bool {
    if let Some(rest) = line.strip_prefix("![[") {
        return rest.ends_with("]]") && !rest.contains('\n');
    }
    if let Some(rest) = line.strip_prefix("![](") {
        return rest.ends_with(')') && !rest.contains("![](") && !rest.contains('\n');
    }
    false
}

fn expand_wiki_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    loop {
        let Some(s) = rest.find("![[") else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..s]);
        let after = &rest[s + 3..];
        let Some(e) = after.find("]]") else {
            // 无闭合：原样保留剩余部分
            out.push_str(&rest[s..]);
            break;
        };
        let name = &after[..e];
        if name.is_empty() || name.contains('[') || name.contains(']') || name.contains('\n') {
            out.push_str(&rest[s..s + 3 + e + 2]);
        } else {
            // Obsidian 尺寸语法 `![[a.png|300]]`：取管道前文件名
            let file = name.split('|').next().unwrap_or(name).trim();
            out.push_str("![](");
            out.push_str(file);
            out.push(')');
        }
        rest = &after[e + 2..];
    }
    out
}

/// list 条目渲染。doc_mode/字体规格/img 基准在条目内自设——list 条目
/// 懒渲染发生在调用方 render 函数返回之后，不能依赖外层的 set/reset 窗口。
pub fn render_doc_item(
    blocks: &std::rc::Rc<Vec<MdBlock>>,
    ix: usize,
    base_dir: Option<&std::path::Path>,
) -> AnyElement {
    let mut spec = crate::appearance::file_font();
    spec.size = (spec.size * 1.15).round();
    set_md_spec(spec);
    set_doc_mode(true);
    *IMG_BASE.lock().unwrap_or_else(|e| e.into_inner()) = base_dir.map(|p| p.to_path_buf());
    let el = render_doc_item_inner(blocks, ix, crate::appearance::theme::theme());
    set_doc_mode(false);
    el
}

fn render_doc_item_inner(blocks: &[MdBlock], ix: usize, t: &Theme) -> AnyElement {
    let Some(b) = blocks.get(ix) else {
        return div().into_any_element();
    };
    // 本块的 mb 不在这里落：非末块由下一条目的 top 取用，末块弃用
    // （文档 pb(40) 兜底）——与整树版"末块 mb 无人消费"一致
    let mt = block_margins(b).0;
    let first = ix == 0;
    let last = ix + 1 == blocks.len();
    // 块间距 = max(前块 mb, 本块 mt)——整树版 render_blocks 的合并规则，
    // 但虚拟化后没有"上一条目的已渲染 margin"可折算，改按块数据自洽：
    // 每条目自己算 pt（前块 margins 可查），mb 只喂给下一条目、自身不落。
    // li→li 之间是列表容器 gap 3px；首条目额外吃文档 pt(26)；末条目弃
    // mb、文档 pb(40) 兜底（全部与整树版逐像素同规）
    let top = if first {
        26. + mt
    } else if matches!(blocks[ix - 1], MdBlock::ListItem { .. })
        && matches!(b, MdBlock::ListItem { .. })
    {
        3.
    } else {
        block_margins(&blocks[ix - 1]).1.max(mt)
    };
    div()
        .w_full()
        .flex()
        .pt(px(top))
        .when(last, |d| d.pb(px(40.)))
        .child(flex_span(4., Some(20.)))
        // 内容段必须显式 min_w(0)：flex 项的自动最小尺寸 = min-content，而
        // gpui 的文本在 MinContent 下不折行（wrap_width = None，单行宽度），
        // 于是 92% 段被撑到整段单行宽、正文永远不回行（右缘溢出）。min_w(0)
        // 后段宽 = 字面 92/100 容器宽，StyledText 拿到 definite 宽自然折行
        .child(
            flex_span(92., None)
                .min_w(px(0.))
                .child(render_block(b, 1, t, false, t.text)),
        )
        .child(flex_span(4., Some(20.)))
        .into_any_element()
}

// ---------------------------------------------------------------------------
// 文件 md 预览（MarkdownKind::render_body 的 preview 态，自 view.rs 归位）
// ---------------------------------------------------------------------------

/// md 渲染预览面板：内容真值 = **缓冲区** + 块级虚拟化 + 滚动锚点恢复 +
/// 溢出滚动条（v60/v63-6 配方，注释保留原始依据）。
pub(crate) fn render_preview_body(
    chat: &mut crate::Chat,
    weak: &gpui::WeakEntity<crate::Chat>,
    path: &std::path::Path,
    cx: &mut gpui::Context<crate::Chat>,
) -> gpui::AnyElement {
    let t = crate::appearance::theme::theme();
    // 预览内容真值 = **缓冲区**（Zed 的 markdown preview 渲染 buffer 文本，
    // 不是磁盘文件）：有未保存修改（源码态改完切预览 / 冲突挂着 / 自动保存
    // 还没落地）时取编辑器值，否则用磁盘真值缓存（省一次 Rope 物化）。
    let content = match chat.ws.file_cache.get(path) {
        Some(ft) if ft.dirty => ft
            .editor
            .as_ref()
            .map(|e| e.read(cx).text().to_string())
            .unwrap_or_else(|| ft.content.clone()),
        Some(ft) => ft.content.clone(),
        None => String::new(),
    };
    // 图片相对路径解析基准：tab 路径可能来自消息文本（相对形态），
    // 统一按工作区 cwd 绝对化再取父目录，不依赖进程 cwd
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        match chat.ws.cwd.clone() {
            // 相对路径按工作区绝对化（不依赖进程 cwd）
            Some(cwd) => cwd.join(path),
            // 无项目态没有基准：给必然读不到的路径 → 按图片缺失渲染
            None => PathBuf::from(std::path::MAIN_SEPARATOR.to_string()),
        }
    };
    // 块级虚拟化（抄 zed thread_view 的 list 架构，v60）：ListState 每
    // 帧只建可视块条目（progress.md 784 块全量构建 ≈ 30ms/帧，滚动必
    // 卡）。滚动条 = v63-6 配方：ListStateHandle 适配 gpui-component
    // Scrollbar，仅实际溢出时渲染，且必须是 list 容器的**兄弟**（同
    // 旧滚动层——作为子元素会被连带位移，滚动了滑块跟内容漂出视口）。
    let blocks = doc_blocks(&content);
    if chat.ws.md_preview_list_path.as_deref() != Some(path) {
        chat.ws.md_preview_list.reset(blocks.len());
        chat.ws.md_preview_list_path = Some(path.to_path_buf());
    } else if chat.ws.md_preview_list.item_count() != blocks.len() {
        // 块数变了（外部改动/编辑动了段落结构）也不能跳回顶部：记下
        // 当前阅读锚点（条目 + 条目内偏移），重建后原样恢复（Zed 的
        // preview 由 buffer anchor 驱动，滚动位跨 reload 不掉）。
        let anchor = chat.ws.md_preview_list.logical_scroll_top();
        chat.ws.md_preview_list.reset(blocks.len());
        chat.ws.md_preview_list.scroll_to(gpui::ListOffset {
            item_ix: anchor.item_ix.min(blocks.len().saturating_sub(1)),
            offset_in_item: anchor.offset_in_item,
        });
    }
    let base = abs.parent().map(|p| p.to_path_buf());
    let blocks_for_list = blocks.clone();
    let weak_preview = weak.clone();
    let weak_click = weak.clone();
    let click_path = path.to_path_buf();
    div()
        .id("md-preview")
        .relative()
        // 082 三期：预览容器可聚焦（被动只读视图，持焦只为了键盘滚动，
        // 无输入副作用）——逐帧仲裁在 File+预览态把焦点交给它，↑/↓/翻页
        // 键不再死角；Ctrl+←/→ 是无上下文壳层绑定，仍优先切 tab
        .track_focus(&chat.ws.md_preview_focus)
        .key_context("MdPreview")
        .flex_1()
        .min_h_0()
        .min_w_0()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        // 点预览区 = 进源码编辑（2026-10-16 用户拍板，Obsidian 同型）。流程
        // 与 eye 按钮同款：只改 md_source + pending_focus_file（渲染帧消费
        // 聚焦编辑器 + sync_live_decorations 对账）。挂 on_click（mouse-up）
        // 而非 on_mouse_down：预览内链接在 mouse-up 才发，按下即切会把整棵
        // 元素树换掉、链接点击丢失。链接与路径 code 块在自己 handler 里
        // stop_propagation 不落此处（渲染器与聊天区共用，聊天无祖先点击
        // 监听，行为不变）。
        // **拖拽不切**：gpui 的 on_click 只要 down/up 都落在 hitbox 就发
        // （div.rs 的 DRAG_THRESHOLD 只管 drag_listener 拖放，不拦
        // click），按住拖动的手势 mouse-up 也被当成「点击」把整棵树换成
        // 源码——预览正文是自绘 StyledText（无选区机制），用户在预览里
        // 唯一的划选尝试落空后还被甩进源码态，观感即「选中文字 ctrl-c
        // 复制不了」。所以 down→up 位移超过 CLICK_MAX_DRAG 视为拖拽，留
        // 在预览态；genuine click（位移 ≤ 阈值）才进源码，Obsidian 阅读
        // 视图同型（点一下才进编辑）。预览正文可选可复制是独立缺口
        // （自绘渲染器无选区机制），不在本处
        .on_click(move |ev, _, cx| {
            let ClickEvent::Mouse(m) = ev else { return };
            if !click_within_drag(m.down.position, m.up.position) {
                return;
            }
            let _ = weak_click.update(cx, |c, cx| {
                if let Some(ft) = c.ws.file_cache.get_mut(&click_path) {
                    ft.md_source = true;
                }
                c.ws.pending_focus_file = Some(click_path.clone());
                cx.notify();
            });
        })
        // 082 三期：预览键盘滚动（绑定见 main.rs MdPreview 上下文）。
        // 方向键 = 3 行步进，翻页 = 85% 视口；Home/End 到首/尾
        .on_action(cx.listener(|this, _: &crate::PreviewUp, window, cx| {
            this.ws
                .md_preview_list
                .scroll_by(-window.line_height() * 3.);
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &crate::PreviewDown, window, cx| {
            this.ws.md_preview_list.scroll_by(window.line_height() * 3.);
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &crate::PreviewPageUp, window, cx| {
            this.ws
                .md_preview_list
                .scroll_by(-window.viewport_size().height * 0.85);
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &crate::PreviewPageDown, window, cx| {
            this.ws
                .md_preview_list
                .scroll_by(window.viewport_size().height * 0.85);
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &crate::PreviewHome, _window, cx| {
            this.ws.md_preview_list.scroll_to(gpui::ListOffset {
                item_ix: 0,
                offset_in_item: px(0.),
            });
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &crate::PreviewEnd, _window, cx| {
            this.ws.md_preview_list.scroll_by(px(1_000_000.));
            cx.notify();
        }))
        .child(
            gpui::list(chat.ws.md_preview_list.clone(), move |ix, _window, _cx| {
                // 预览内路径点击基准 = 预览文件所在目录（pi-web baseDir 同型）
                set_link_target(weak_preview.clone(), base.clone());
                render_doc_item(&blocks_for_list, ix, base.as_deref())
            })
            // list 元素自身要 flex_1 从 flex 列父容器拿高度——Auto 尺寸
            // 下无内容贡献、无 grow 会被 taffy 布局成 0 高（条目建了
            // 也全画在 0 高视口外，预览全空）；session_list 同款
            .flex_1()
            .min_h_0(),
        )
        .when(
            chat.ws.md_preview_list.max_offset_for_scrollbar().height > px(0.),
            |d| {
                let handle =
                    crate::ui::list_handle::ListStateHandle(chat.ws.md_preview_list.clone());
                d.child(
                    div()
                        .absolute()
                        .top(px(4.))
                        .bottom(px(4.))
                        .right(px(3.))
                        .w(px(8.))
                        .child(gpui_component::scroll::Scrollbar::vertical(
                            &chat.ws.md_preview_scrollbar,
                            &handle,
                        )),
                )
            },
        )
        .into_any_element()
}

/// 024: md 源码态 WYSIWYG 装饰幂等同步——`source_mode=true` 且「界面页
/// Markdown 所见即所得」开关开启时挂装饰 provider（非光标行折叠语法字符 +
/// 渲染器同款着色；preview 渲染页不渲染编辑器，无需装饰）。开关默认关
/// （2026-10-13 用户定夺：回车崩溃 + 标题行距两缺陷未修，默认退普通编辑
/// 路径），关 = 摘 provider 即 stock 编辑器。行号保持默认（024 新设计：
/// 源码视图既有能力不丢）。鼠标 eye 与 `file.view_mode` op 都只改
/// ft.md_source，状态差异在此收口防漂移。
///
/// **本函数是软换行 ⇄ WYSIWYG 的唯一耦合点**（2026-10-14 用户定夺解耦）：
/// md 的折行由 kind.rs `EditorConfig` 声明恒开（stock 路径回行，6bc1f2f
/// 口径）；但折叠与折行结构互斥（vendor element.rs 折叠分支 debug_assert
/// 「折叠态必须关软换行」——折叠改写每行字节长，text_wrapper 的换行段是按
/// doc 文本算的），所以挂装饰必须同时关折行、摘装饰恢复折行。早退分支
/// 安全：装饰与折行总在本 closure 里一起写，`has_decorations()==live` 蕴含
/// 折行已同步。
pub(crate) fn sync_live_decorations(
    chat: &mut crate::Chat,
    path: &std::path::Path,
    source_mode: bool,
    cx: &mut gpui::Context<crate::Chat>,
) {
    let live = source_mode && crate::services::workspace::md_live_preview();
    let Some(ed) = chat.ws.file_cache.get(path).and_then(|f| f.editor.clone()) else {
        return;
    };
    if ed.read(cx).has_decorations() == live {
        return;
    }
    ed.update(cx, |st, scx| {
        st.set_soft_wrap_cx(!live, scx);
        st.set_decorations(if live {
            Some(std::rc::Rc::new(wysiwyg::MdLiveProvider))
        } else {
            None
        });
    });
}

#[cfg(test)]
mod doc_tests {
    use super::{click_within_drag, CLICK_MAX_DRAG};
    use gpui::{point, px};

    /// genuine click（位移 ≤ 阈值，含轻微手抖）→ 进源码编辑。
    #[test]
    fn genuine_click_passes() {
        let o = point(px(100.), px(200.));
        assert!(click_within_drag(o, o)); // 原地
        assert!(click_within_drag(
            o,
            point(px(100. + CLICK_MAX_DRAG), px(200.))
        )); // 恰好阈值
        assert!(click_within_drag(o, point(px(103.), px(201.)))); // 手抖
    }

    /// 拖拽（位移 > 阈值）不进源码——预览态划选手势留在预览（024 预览
    /// 正文无选区机制，拖一下就被甩进源码 = 「预览选中复制不了」的元凶）。
    #[test]
    fn drag_motion_rejected() {
        let o = point(px(100.), px(200.));
        let eps = 0.01;
        assert!(!click_within_drag(
            o,
            point(px(100. + CLICK_MAX_DRAG + eps), px(200.))
        ));
        assert!(!click_within_drag(
            o,
            point(px(100.), px(200. + CLICK_MAX_DRAG + eps))
        ));
        // 斜向小分量叠加超阈值同样拒
        assert!(!click_within_drag(
            o,
            point(px(100. + CLICK_MAX_DRAG), px(200. + CLICK_MAX_DRAG))
        ));
    }
}
