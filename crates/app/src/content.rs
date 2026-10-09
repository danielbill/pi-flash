//! 内容区 (v54): topbar-r 之下的一切。状态机 chat / term / md —— 终端与
//! markdown 预览以 topbar tab 打开（Obsidian 式），聊天为默认视图。内容
//! 区直通窗口底（statusbar 只在面板段）。

use gpui::{Context, Entity, Focusable, KeyDownEvent, MouseButton, SharedString, div, img, prelude::*, px, rgb};

use crate::Chat;
use crate::ContentView;
use crate::i18n::tr;
use crate::theme::theme as T;
use crate::ui::icon;

/// 内容主视图：按 content_view 切换（flex-1，占满 topbar-r 以下）。
pub(crate) fn content_main(
    chat: &mut Chat,
    entity: Entity<Chat>,
    weak: &gpui::WeakEntity<Chat>,
    window: &mut gpui::Window,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let view: gpui::AnyElement = match chat.content_view {
        ContentView::Chat => crate::session::main_column(chat, entity, weak, window, cx)
            .into_any_element(),
        ContentView::Term => term_view(chat, weak, window, cx)
            .map(|d| d.into_any_element())
            .unwrap_or_else(|| empty_hint(tr("暂无终端会话"), t)),
        ContentView::File => file_view(chat, weak, window, cx).into_any_element(),
    };
    div()
        .id("content-main")
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        .child(view)
        .into_any_element()
}

fn empty_hint(text: &str, t: &'static crate::theme::Theme) -> gpui::AnyElement {
    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .text_color(rgb(t.text_dim))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

// ---------------------------------------------------------------------------
// 终端视图：纯暗面（tab 在 topbar-r；失败/退出横幅保留；无 38px 头）
// ---------------------------------------------------------------------------

pub(crate) fn term_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    _window: &mut gpui::Window,
    cx: &mut Context<Chat>,
) -> Option<gpui::AnyElement> {
    let _t = T();
    let body: Option<gpui::AnyElement> = chat
        .active_panel_tab
        .and_then(|ix| chat.panel_tabs.get(ix).cloned())
        .map(|tab| match tab {
            crate::PanelTab::File(_) => div().into_any_element(),
            crate::PanelTab::Term(id) => {
                let tix = chat.terminals.iter().position(|t| t.id == id);
                let Some(tix) = tix else {
                    return div().into_any_element();
                };
                let tab = &chat.terminals[tix];
                let mut col = div().flex_1().min_h_0().flex().flex_col();
                match &tab.status {
                    crate::terminal::TermStatus::Exited(code) => {
                        let code_text = code
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| tr("unknown").to_string());
                        col = col.child(
                            div()
                                .py(px(7.))
                                .px(px(12.))
                                .text_size(crate::appearance::ui_size(11.))
                                .font_family(crate::terminal::FONT_FAMILY)
                                .text_color(rgb(0x6e8a7d))
                                .child(SharedString::from(crate::i18n::tf(
                                    "Process exited with code {code_text}",
                                    &[("code_text", code_text)],
                                ))),
                        );
                    }
                    crate::terminal::TermStatus::Ready => {}
                }
                col.child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .bg(rgb(0x141a17))
                        .child(
                            crate::terminal::TerminalElement::new(tab, weak.clone())
                                .blink_on(chat.term_cursor_on)
                                .track_focus(&tab.focus)
                                .flex_1()
                                .h_full()
                                .on_mouse_down(MouseButton::Left, {
                                    let f = tab.focus.clone();
                                    move |_, window, _cx| {
                                        window.focus(&f);
                                    }
                                })
                                .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _w, cx| {
                                    this.terminal_key(ev, cx);
                                })),
                        ),
                )
                .into_any_element()
            }
        });

    body.map(|b| {
        div()
            .id("term-view")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(rgb(0x141a17))
            .child(b)
            .into_any_element()
    })
}

// ---------------------------------------------------------------------------
// 023 文件编辑展示页（fileView）：Zed 式标签栏 + 面包屑导航栏 + 编辑区。
// 底座 = gpui-component Input 的 CodeEditor 模式（tree-sitter 高亮/行号/
// 内置搜索替换弹层），「不语义自研，简化对齐 zed」——见 023 设计文档。
// ---------------------------------------------------------------------------

use std::path::Path;

/// 扩展名 → gpui-component tree-sitter 语言名（未收录回退 "text" 纯色；
/// 已剔除 ruby/sql——sequel 的 cc 钉版与 gpui 冲突、ruby 的 parser.c 在
/// MSVC 下编不过，均从 vendored feature 里移除）。
fn ts_language(ext: &str) -> &'static str {
    match ext {
        "rs" => "rust",
        "py" | "pyw" => "python",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "go" => "go",
        "json" | "jsonc" => "json",
        "html" | "htm" | "xml" | "vue" | "svg" => "html",
        "css" | "scss" => "css",
        "md" | "markdown" => "markdown",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" => "cpp",
        "cs" => "csharp",
        "java" => "java",
        "swift" => "swift",
        "zig" => "zig",
        "sh" | "bash" | "zsh" => "bash",
        "cmake" => "cmake",
        "proto" => "proto",
        "diff" | "patch" => "diff",
        "ex" => "elixir",
        "graphql" | "gql" => "graphql",
        "scala" => "scala",
        "mk" => "make",
        _ => "text",
    }
}

/// 编辑器实体懒创建：InputState::new 要 `&mut Window`，而 open_file_tab
/// 的调用链没有——渲染帧是唯一同时持有 window 与 Chat 可变的点。创建时
/// 订阅 Change 事件；dirty 用「编辑器值 != 磁盘真值缓存」比较（set_value
/// 也发 Change，盲标会假脏）。
fn ensure_file_editor(
    chat: &mut Chat,
    path: &Path,
    window: &mut gpui::Window,
    cx: &mut Context<Chat>,
) {
    if chat
        .file_cache
        .get(path)
        .map(|f| f.editor.is_some())
        .unwrap_or(true)
    {
        return;
    }
    let content = chat
        .file_cache
        .get(path)
        .map(|f| f.content.clone())
        .unwrap_or_default();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let ed = cx.new(|scx| {
        gpui_component::input::InputState::new(window, scx)
            .code_editor(ts_language(&ext))
            .soft_wrap(false)
    });
    ed.update(cx, |st, scx| st.set_value(content, window, scx));
    cx.subscribe(&ed, |this, ed, ev: &gpui_component::input::InputEvent, cx| {
        if matches!(ev, gpui_component::input::InputEvent::Change) {
            let src = ed.entity_id();
            if let Some((path, ft)) = this.file_cache.iter_mut().find(|(_, f)| {
                f.editor.as_ref().map(|e| e.entity_id()) == Some(src)
            }) {
                // Rope 与 String 直接比较（ropey PartialEq，零分配 memcmp）；
                // 原先 value().to_string() 每次编辑都全文物化一遍（4.6MB 文件
                // 每次按键 ~8ms + 一次全文分配）。
                let dirty = *ed.read(cx).text() != ft.content;
                if dirty {
                    ft.last_edit = Some(std::time::Instant::now());
                }
                if ft.dirty != dirty {
                    ft.dirty = dirty;
                    cx.notify();
                }
                if dirty {
                    let p = path.clone();
                    this.autosave_later(p, cx);
                }
            }
        }
    })
    .detach();
    if let Some(ft) = chat.file_cache.get_mut(path) {
        ft.editor = Some(ed);
    }
}

/// 消费 reload_pending（自动重载路径）：磁盘新内容灌进编辑器。set_value
/// 绕过 undo 历史、复位滚动——正合「磁盘为准」的重载语义。
fn consume_file_reload(
    chat: &mut Chat,
    path: &Path,
    window: &mut gpui::Window,
    cx: &mut Context<Chat>,
) {
    let pending = chat
        .file_cache
        .get(path)
        .map(|f| f.reload_pending)
        .unwrap_or(false);
    if !pending {
        return;
    }
    let content = chat
        .file_cache
        .get(path)
        .map(|f| f.content.clone())
        .unwrap_or_default();
    if let Some(ed) = chat.file_cache.get(path).and_then(|f| f.editor.clone()) {
        ed.update(cx, |st, scx| st.set_value(content, window, scx));
    }
    if let Some(ft) = chat.file_cache.get_mut(path) {
        ft.reload_pending = false;
    }
}

fn file_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    window: &mut gpui::Window,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let Some(path) = chat.active_file_path() else {
        return empty_hint(tr("在左侧文件树中选择一个文件"), t);
    };
    let (md_source, conflict) = chat
        .file_cache
        .get(&path)
        .map(|f| (f.md_source, f.conflict.clone()))
        .unwrap_or((false, None));

    // 渲染帧副作用区：编辑器懒创建 + 自动重载灌入（见两 fn 文档）
    ensure_file_editor(chat, &path, window, cx);
    consume_file_reload(chat, &path, window, cx);
    // 开文件即聚焦（Zed 行为；编辑器实体就绪后消费；md 渲染态无实体则丢弃）
    if chat.pending_focus_file.as_deref() == Some(path.as_path()) {
        chat.pending_focus_file = None;
        if let Some(ed) = chat.file_cache.get(&path).and_then(|f| f.editor.clone()) {
            if !ed.read(cx).focus_handle(cx).is_focused(window) {
                ed.update(cx, |st, scx| st.focus(window, scx));
            }
        }
    }

    let mut host = div()
        .id("file-view")
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        .child(file_nav_bar(chat, weak, &path, md_source))
        // Ctrl+S 保存：action 绑定在 "Input" 上下文（编辑器聚焦时命中），
        // 由本容器 on_action 接住
        .on_action(cx.listener(|this, _: &crate::FileSave, _w, cx| {
            this.save_active_file(cx);
        }));

    if let Some(c) = conflict.as_ref() {
        host = host.child(conflict_banner(weak, &path, c));
    }
    host.child(file_editor_body(chat, &path, md_source))
        .into_any_element()
}

/// 导航操作栏：左 = 面包屑（项目根相对路径段，目录段点开兄弟文件菜单，
/// 对齐 Zed 可点击面包屑）；右 = eye/eye-off（md 源码/渲染切换）+ search（聚焦
/// 编辑器并派发组件 Search，即 Ctrl+F 内置搜索替换弹层）。
fn file_nav_bar(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    path: &Path,
    md_source: bool,
) -> gpui::AnyElement {
    let t = T();
    let is_md = md_file(path);

    // 面包屑段：cwd 相对路径
    let rel = path.strip_prefix(&chat.cwd).unwrap_or(path);
    let segments: Vec<std::ffi::OsString> = rel
        .components()
        .map(|c| c.as_os_str().to_os_string())
        .collect();
    let mut crumbs = div().min_w_0().flex().items_center().overflow_hidden();
    let mut acc = chat.cwd.clone();
    for (i, seg) in segments.iter().enumerate() {
        acc = acc.join(seg);
        let last = i + 1 == segments.len();
        let seg_text: SharedString = seg.to_string_lossy().to_string().into();
        if last {
            crumbs = crumbs.child(
                div()
                    .text_size(crate::appearance::ui_size(11.5))
                    .text_color(rgb(t.text))
                    .whitespace_nowrap()
                    .child(seg_text),
            );
        } else {
            let dir = acc.clone();
            let weak_crumb = weak.clone();
            let weak_off = weak.clone();
            let dd = chat.crumb_dd.clone();
            let open = chat.crumb_menu_dir.as_ref() == Some(&dir);
            let seg_dir = dir.clone();
            crumbs = crumbs.child(
                crate::ui::dropdown(
                    SharedString::from(format!("fv-crumb-{i}")),
                    &dd,
                    open,
                    move |_w, cx| {
                        let _ = weak_crumb.update(cx, |c, cx| {
                            // 再点同段 = 收起；点别段 = 换目标
                            c.crumb_menu_dir = if c.crumb_menu_dir.as_ref() == Some(&dir) {
                                None
                            } else {
                                Some(dir.clone())
                            };
                            cx.notify();
                        });
                    },
                    move |_w, cx| {
                        let _ = weak_off.update(cx, |c, cx| {
                            c.crumb_menu_dir = None;
                            cx.notify();
                        });
                    },
                    div()
                        .text_size(crate::appearance::ui_size(11.5))
                        .text_color(rgb(t.text_muted))
                        .cursor_pointer()
                        .hover(|s| s.text_color(rgb(t.text)))
                        .whitespace_nowrap()
                        .child(seg_text)
                        .into_any_element(),
                    move || crumb_siblings_menu(weak, &seg_dir),
                ),
            );
            crumbs = crumbs.child(
                div()
                    .px(px(3.))
                    .text_size(crate::appearance::ui_size(10.))
                    .child(icon("chevron-right", 9., t.text_faint)),
            );
        }
    }

    // 右侧操作区：eye（仅 md）+ search
    let ed_for_focus = chat.file_cache.get(path).and_then(|f| f.editor.clone());
    let mut right = div().flex_shrink_0().flex().items_center().gap(px(2.));
    if is_md {
        let weak_eye = weak.clone();
        let src = md_source;
        right = right.child(
            div()
                .id("fv-eye")
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .cursor_pointer()
                .when(md_source, |d| d.bg(rgb(t.bg_selected)))
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_eye.update(cx, |c, cx| {
                        if let Some(p) = c.active_file_path() {
                            if let Some(ft) = c.file_cache.get_mut(&p) {
                                ft.md_source = !src;
                            }
                            // 切到源码态即聚焦编辑器（渲染帧消费）
                            if !src {
                                c.pending_focus_file = Some(p);
                            }
                        }
                        cx.notify();
                    });
                })
                .child(icon(
                    // 图标 = 点击后的动作：渲染态 → eye-off（关闭预览进源码），
                    // 源码态 → eye（回到预览）
                    if md_source { "eye" } else { "eye-off" },
                    14.,
                    if md_source { t.accent } else { t.text_muted },
                )),
        );
    }
    right = right.child(
        div()
            .id("fv-search")
            .size(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(5.))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(t.bg_hover)))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                if let Some(ed) = &ed_for_focus {
                    ed.update(cx, |s, scx| s.focus(window, scx));
                    window.dispatch_action(Box::new(gpui_component::input::Search), cx);
                }
            })
            .child(icon("search", 13., t.text_muted)),
    );

    div()
        .h(px(32.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(8.))
        .pl(px(12.))
        .pr(px(8.))
        .border_b_1()
        .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x66)))
        .child(crumbs)
        .child(div().flex_1())
        .child(right)
        .into_any_element()
}

/// 面包屑目录段的兄弟文件菜单（对齐 Zed 点击面包屑列同级文件）。
fn crumb_siblings_menu(weak: &gpui::WeakEntity<Chat>, dir: &Path) -> gpui::AnyElement {
    let t = T();
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names.truncate(200);
    let mut col = div()
        .id("fv-crumb-menu")
        .min_w(px(220.))
        .max_h(px(360.))
        .overflow_y_scroll()
        .p(px(4.))
        .bg(rgb(t.bg))
        .border_1()
        .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x8c)))
        .rounded(px(9.))
        .shadow_lg()
        .flex()
        .flex_col();
    if names.is_empty() {
        col = col.child(
            div()
                .px(px(10.))
                .py(px(7.))
                .text_size(crate::appearance::ui_size(11.5))
                .text_color(rgb(t.text_faint))
                .child(SharedString::from(tr("（无文件）").to_string())),
        );
    }
    for n in names {
        let weak_row = weak.clone();
        let target = dir.join(&n);
        let label: SharedString = n.into();
        col = col.child(
            div()
                .id(SharedString::from(format!("fv-crumb-file-{label}")))
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(10.))
                .py(px(6.))
                .rounded(px(6.))
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let p = target.clone();
                    let _ = weak_row.update(cx, |c, cx| {
                        c.crumb_menu_dir = None;
                        c.open_file_tab(p, cx);
                    });
                })
                .child(crate::ui::icon("file", 12., t.text_faint))
                .child(label),
        );
    }
    col.into_any_element()
}

/// 冲突横幅（023 外部改动检测的对齐 Zed 裁决 UI）：
/// Changed = 「文件已在磁盘上被修改」+ 重新加载 / 保留我的版本；
/// Deleted = 「文件已从磁盘消失」+ 保留缓冲。
fn conflict_banner(
    weak: &gpui::WeakEntity<Chat>,
    path: &Path,
    conflict: &crate::FileConflict,
) -> gpui::AnyElement {
    let t = T();
    let weak_reload = weak.clone();
    let weak_keep = weak.clone();
    let p_reload = path.to_path_buf();
    let p_keep = path.to_path_buf();
    let (msg, reload_label, keep_label) = match conflict {
        crate::FileConflict::Changed => (
            tr("文件已在磁盘上被修改（本地有未保存修改）"),
            tr("重新加载"),
            tr("保留我的版本"),
        ),
        crate::FileConflict::Deleted => (tr("文件已从磁盘消失"), "", tr("保留缓冲")),
    };
    let mut row = div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(10.))
        .px(px(12.))
        .py(px(6.))
        .bg(rgb(crate::theme::mix_rgb(0xe0a562, t.bg, 0.82)))
        .border_b_1()
        .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x66)))
        .text_size(crate::appearance::ui_size(11.5))
        .text_color(rgb(t.text))
        .child(SharedString::from(msg.to_string()))
        .child(div().flex_1());
    if !reload_label.is_empty() {
        row = row.child(banner_btn("fv-banner-reload", reload_label, move |_, _, cx| {
            let p = p_reload.clone();
            let _ = weak_reload.update(cx, |c, cx| c.file_reload_from_disk(&p, cx));
        }));
    }
    row = row.child(banner_btn("fv-banner-keep", keep_label, move |_, _, cx| {
        let p = p_keep.clone();
        let _ = weak_keep.update(cx, |c, cx| c.file_ignore_conflict(&p, cx));
    }));
    row.into_any_element()
}

/// 冲突横幅按钮（轻边框款）。
fn banner_btn(
    id: &'static str,
    label: &str,
    handler: impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> gpui::AnyElement {
    let t = T();
    div()
        .id(id)
        .flex_shrink_0()
        .px(px(9.))
        .py(px(3.))
        .rounded(px(6.))
        .border_1()
        .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x8c)))
        .text_size(crate::appearance::ui_size(11.))
        .text_color(rgb(t.text))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .on_mouse_down(MouseButton::Left, handler)
        .child(SharedString::from(label.to_string()))
        .into_any_element()
}

/// 编辑区主体：md 渲染预览（默认）或 CodeEditor（所有文件都可编辑，Zed parity）。
fn file_editor_body(
    chat: &mut Chat,
    path: &Path,
    md_source: bool,
) -> gpui::AnyElement {    let t = T();
    let is_md = md_file(path);
    let Some(ft) = chat.file_cache.get(path) else {
        return empty_hint(tr("文件已关闭"), t);
    };
    let content = ft.content.clone();

    // md 渲染预览（默认态）：复用 agent 正文的 markdown 渲染器
    if is_md && !md_source {
        // 图片相对路径解析基准：tab 路径可能来自消息文本（相对形态），
        // 统一按工作区 cwd 绝对化再取父目录，不依赖进程 cwd
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            chat.cwd.join(path)
        };
        // 块级虚拟化（抄 zed thread_view 的 list 架构，v60）：ListState 每
        // 帧只建可视块条目（progress.md 784 块全量构建 ≈ 30ms/帧，滚动必
        // 卡）。滚动条 = v63-6 配方：ListStateHandle 适配 gpui-component
        // Scrollbar，仅实际溢出时渲染，且必须是 list 容器的**兄弟**（同
        // 旧滚动层——作为子元素会被连带位移，滚动了滑块跟内容漂出视口）。
        let blocks = crate::markdown::doc_blocks(&content);
        if chat.file_view_list_path.as_deref() != Some(path) {
            chat.file_view_list.reset(blocks.len());
            chat.file_view_list_path = Some(path.to_path_buf());
        } else if chat.file_view_list.item_count() != blocks.len() {
            chat.file_view_list.reset(blocks.len());
        }
        let base = abs.parent().map(|p| p.to_path_buf());
        let blocks_for_list = blocks.clone();
        return div()
            .relative()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(rgb(t.bg))
            .child(
                gpui::list(chat.file_view_list.clone(), move |ix, _window, _cx| {
                    crate::markdown::render_doc_item(&blocks_for_list, ix, base.as_deref())
                })
                // list 元素自身要 flex_1 从 flex 列父容器拿高度——Auto 尺寸
                // 下无内容贡献、无 grow 会被 taffy 布局成 0 高（条目建了
                // 也全画在 0 高视口外，预览全空）；session_list 同款
                .flex_1()
                .min_h_0(),
            )
            .when(
                chat.file_view_list.max_offset_for_scrollbar().height > px(0.),
                |d| {
                    let handle =
                        crate::ui::list_handle::ListStateHandle(chat.file_view_list.clone());
                    d.child(
                        div()
                            .absolute()
                            .top(px(4.))
                            .bottom(px(4.))
                            .right(px(3.))
                            .w(px(8.))
                            .child(gpui_component::scroll::Scrollbar::vertical(
                                &chat.file_scrollbar,
                                &handle,
                            )),
                    )
                },
            )
            .into_any_element();
    }

    // 图片：gpui img() 真渲染（沿旧版查看路径，最佳查看方式）
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let img_format = match ext.as_str() {
        "png" => Some(gpui::ImageFormat::Png),
        "jpg" | "jpeg" => Some(gpui::ImageFormat::Jpeg),
        "gif" => Some(gpui::ImageFormat::Gif),
        "webp" => Some(gpui::ImageFormat::Webp),
        "bmp" => Some(gpui::ImageFormat::Bmp),
        "svg" => Some(gpui::ImageFormat::Svg),
        _ => None,
    };
    if let Some(format) = img_format {
        return match std::fs::read(path)
            .ok()
            .map(|bytes| std::sync::Arc::new(gpui::Image::from_bytes(format, bytes)))
        {
            Some(image) => div()
                .id("fv-img")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .bg(rgb(t.bg))
                .p(px(20.))
                .flex()
                .justify_center()
                .items_start()
                .child(img(image).max_w_full())
                .into_any_element(),
            None => empty_hint(tr("图片读取失败"), t),
        };
    }

    // CodeEditor（gpui-component）：tree-sitter 高亮 + 行号 + Ctrl+F 搜索替换
    let Some(ed) = ft.editor.clone() else {
        return empty_hint(tr("编辑器初始化中…"), t);
    };
    div()
        .id("fv-editor")
        .relative()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        .font_family(crate::markdown::MONO_FAMILY)
        .text_size(px(crate::appearance::file_font().size))
        .child(
            gpui_component::input::TextInput::new(&ed)
                .h_full()
                .appearance(false)
                .bordered(false)
                .focus_bordered(false),
        )
        .into_any_element()
}

/// markdown 源文件判定（eye 预览切换只对它出现）。
fn md_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some(e) if e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown")
    )
}

#[cfg(test)]
mod tests {
    use super::ts_language;

    /// 扩展名 → tree-sitter 语言名映射（023 编辑器高亮）；未收录回退 text。
    #[test]
    fn ts_language_mapping() {
        assert_eq!(ts_language("rs"), "rust");
        assert_eq!(ts_language("py"), "python");
        assert_eq!(ts_language("tsx"), "tsx");
        assert_eq!(ts_language("md"), "markdown");
        assert_eq!(ts_language("toml"), "toml");
        assert_eq!(ts_language("cpp"), "cpp");
        // 大小写由调用方 lower-case 后再进本表（ensure_file_editor）
        assert_eq!(ts_language("CPP"), "text");
        // 已从 vendored feature 剔除的语言回退纯色（不 panic、不高亮）
        assert_eq!(ts_language("rb"), "text");
        assert_eq!(ts_language("sql"), "text");
        assert_eq!(ts_language("unknownext"), "text");
    }
}
