//! 023 文件编辑展示页（fileView）视图：Zed 式标签栏 + 面包屑导航栏 + 编辑区
//! （自 content.rs 拆出，归 editor 模块域）。底座 = gpui-component Input 的
//! CodeEditor 模式（tree-sitter 高亮/行号/内置搜索替换弹层），「不语义自研，
//! 简化对齐 zed」——见 023 设计文档。
//!
//! 编排边界：文件打开/缓冲区/file_cache 状态仍在 Chat（main.rs），本文件
//! 只管视图渲染与交互；md 所见即所得见 `super::markdown::wysiwyg`。

use gpui::{
    Context, Focusable, MouseButton, SharedString, div, img, prelude::*, px, relative, rgb,
};

use super::kind::{BodyCtx, FileKind, ToolButton, kind_of};
use crate::Chat;
use crate::appearance::theme::theme as T;
use crate::content::empty_hint;
use crate::i18n::tr;
use crate::services::workspace::same_path;
use crate::ui::icon;
use std::path::Path;

/// 编辑器实体懒创建：InputState::new 要 `&mut Window`，而 open_file_tab
/// 的调用链没有——渲染帧是唯一同时持有 window 与 Chat 可变的点。创建时
/// 订阅 Change 事件；dirty 用「编辑器值 != 磁盘真值缓存」比较（set_value
/// 也发 Change，盲标会假脏）。编辑器配置（高亮语言/软换行/图片粘贴钩子）
/// 由 kind handler 产出；`editor_config() = None` 的类型（图片查看器）不建实体。
fn ensure_file_editor(
    chat: &mut Chat,
    path: &Path,
    window: &mut gpui::Window,
    cx: &mut Context<Chat>,
) {
    let Some(cfg) = kind_of(path).editor_config() else {
        return;
    };
    if chat
        .ws
        .file_cache
        .get(path)
        .map(|f| f.editor.is_some())
        .unwrap_or(true)
    {
        return;
    }
    let content = chat
        .ws
        .file_cache
        .get(path)
        .map(|f| f.content.clone())
        .unwrap_or_default();
    let ed = cx.new(|scx| {
        gpui_component::input::InputState::new(window, scx)
            .code_editor(cfg.ts_lang)
            .soft_wrap(cfg.soft_wrap)
    });
    ed.update(cx, |st, scx| {
        st.set_value(content, window, scx);
        // 025 插图（P0）：handler 配了粘贴钩子才挂（目前仅 md）——非 md
        // 自动不做（文本粘贴照旧）。落盘/引用串见 markdown::attachments。
        if cfg.image_paste {
            st.on_image_paste = Some(crate::editor::markdown::attachments::image_paste_hook(
                path.to_path_buf(),
            ));
        }
    });
    cx.subscribe(
        &ed,
        |this, ed, ev: &gpui_component::input::InputEvent, cx| {
            if matches!(ev, gpui_component::input::InputEvent::Change) {
                let src = ed.entity_id();
                if let Some((path, ft)) = this
                    .ws
                    .file_cache
                    .iter_mut()
                    .find(|(_, f)| f.editor.as_ref().map(|e| e.entity_id()) == Some(src))
                {
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
        },
    )
    .detach();
    if let Some(ft) = chat.ws.file_cache.get_mut(path) {
        ft.editor = Some(ed);
    }
}

/// 消费搜索结果的「跳到第 N 行」请求（083）：把编辑器光标挪到目标行首。
/// `set_cursor_offset` 内部 `move_to` → `scroll_to` 会保证光标可见，因此
/// 不需要额外的滚动 API（与 Zed 点击搜索结果后 reveal 到范围同效）。
fn consume_pending_jump(chat: &mut Chat, path: &Path, cx: &mut Context<Chat>) {
    let Some((target, line)) = chat.ws.pending_jump.clone() else {
        return;
    };
    if !same_path(&target, path) {
        return;
    }
    chat.ws.pending_jump = None;
    let Some(ed) = chat.ws.file_cache.get(path).and_then(|f| f.editor.clone()) else {
        return;
    };
    // 行号 → 字节偏移：内容来自磁盘真值缓存（编辑器未建时 open_file_tab
    // 已灌入内容；有未保存修改时取编辑器值，与正文一致）
    let content = chat
        .ws
        .file_cache
        .get(path)
        .map(|f| match (&f.editor, f.dirty) {
            (Some(_), true) => ed.read(cx).text().to_string(),
            _ => f.content.clone(),
        })
        .unwrap_or_default();
    let mut offset = 0usize;
    for (ix, raw) in content.split_inclusive('\n').enumerate() {
        if ix + 1 >= line.max(1) as usize {
            break;
        }
        offset += raw.len();
    }
    ed.update(cx, |st, cx| st.set_cursor_offset(offset, cx));
}

/// 消费 reload_pending（自动重载路径）：磁盘新内容灌进编辑器。用
/// `reload_value` 而非 `set_value`——外部改动要保留阅读位置/光标（Zed 的
/// 外部 reload 保留 anchor）；set_value 是程序化赋值语义（光标到末尾、
/// 滚动归零），拿它灌外部改动会每存一次跳一次屏。
fn consume_file_reload(
    chat: &mut Chat,
    path: &Path,
    window: &mut gpui::Window,
    cx: &mut Context<Chat>,
) {
    let pending = chat
        .ws
        .file_cache
        .get(path)
        .map(|f| f.reload_pending)
        .unwrap_or(false);
    if !pending {
        return;
    }
    let content = chat
        .ws
        .file_cache
        .get(path)
        .map(|f| f.content.clone())
        .unwrap_or_default();
    if let Some(ed) = chat.ws.file_cache.get(path).and_then(|f| f.editor.clone()) {
        ed.update(cx, |st, scx| st.reload_value(content, window, scx));
    }
    if let Some(ft) = chat.ws.file_cache.get_mut(path) {
        ft.reload_pending = false;
    }
}

pub(crate) fn file_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    window: &mut gpui::Window,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let Some(path) = chat.active_file_path() else {
        return empty_hint(tr("file_view.pick_in_tree"), t);
    };
    let conflict = chat
        .ws
        .file_cache
        .get(&path)
        .map(|f| f.conflict.clone())
        .unwrap_or(None);
    // 非文本 tab（open_file_tab 开的：图片查看 / 「无法打开」灰屏）：无缓冲
    // 无编辑器——懒创建/重载灌入/聚焦三件套全部跳过
    let binary = chat.ws.file_cache.get(&path).is_some_and(|f| f.binary);
    let tab_image = chat.ws.file_cache.get(&path).and_then(|f| f.image);

    // 渲染帧副作用区：编辑器懒创建 + 自动重载灌入（见两 fn 文档）
    if !binary {
        ensure_file_editor(chat, &path, window, cx);
        consume_file_reload(chat, &path, window, cx);
        consume_pending_jump(chat, &path, cx);
        // 开文件即聚焦（Zed 行为；编辑器实体就绪后消费）。082 三期：仅
        // 源码态消费——预览态（md_source=false）树里没有编辑器元素，聚焦
        // 实体句柄必悬空（docs/agents/gpui.md 悬空句柄陷阱：旧焦点 blur、
        // 新焦点收不到、键落根），预览焦点改由逐帧仲裁交给预览容器；
        // 意图在此丢弃，等真正切回源码态时的显式手势再置
        if chat.ws.pending_focus_file.as_deref() == Some(path.as_path()) {
            chat.ws.pending_focus_file = None;
            let source_mode = chat.ws.file_cache.get(&path).is_some_and(|f| f.md_source);
            if source_mode
                && let Some(ed) = chat.ws.file_cache.get(&path).and_then(|f| f.editor.clone())
                && !ed.read(cx).focus_handle(cx).is_focused(window)
            {
                ed.update(cx, |st, scx| st.focus(window, scx));
            }
        }
    }

    let kind = kind_of(&path);
    let mut host = div()
        .id("file-view")
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        // 023：导航操作栏由壳统一渲染（面包屑 + handler 声明的工具栏按钮）
        .child(file_nav_bar(chat, weak, kind, &path, cx))
        // Ctrl+S 保存：action 绑定在 "Input" 上下文（编辑器聚焦时命中），
        // 由本容器 on_action 接住
        .on_action(cx.listener(|this, _: &crate::FileSave, _w, cx| {
            this.save_active_file(cx);
        }));

    // zed 同款：只有「磁盘被改 + 本地有未保存修改」（Changed）才横幅裁决；
    // Deleted（文件从磁盘消失）不弹横幅——缓冲保留可继续编辑，tab 删除线
    // 是唯一标记，保存即重建（DiskState::Deleted 语义）。二进制占位 tab
    // 无缓冲，不参与冲突裁决（check_external_file_changes 同步跳过）
    if !binary {
        if let Some(c @ crate::FileConflict::Changed) = conflict.as_ref() {
            host = host.child(conflict_banner(weak, &path, c));
        }
    }
    // 主体渲染算法由 kind handler 决定（md 双态/图片查看器/代码编辑器）。
    // 非文本 tab 分流（zed 同款两类视图）：image=Some → 图片查看面板
    // （ImageView）；None → 「无法打开 {ext} 类型文件」灰屏（InvalidItemView）
    host.child(if binary {
        match tab_image {
            Some(format) => render_image_surface(&path, format),
            None => unsupported_body(&path),
        }
    } else {
        file_editor_body(chat, kind, &path, weak, cx)
    })
    .into_any_element()
}

/// 导航操作栏面包屑的路径分段：`(显示文本, 该段绝对路径)`。
///
/// - 在 `cwd` 下 = 相对分段（累积起点 cwd）；工作区外 = 绝对分段。
/// - 目录段路径供点开「兄弟文件菜单」，所以分段必须累积出真实绝对路径
///   （此前盘符段从 cwd 拼起 → `D:` 后面全是错目录）。
/// - Windows 盘符前缀与根分隔符合成一段：`D:` + `\` = `D:\`（UNC 共享名
///   `\\server\share` 同理），Zed 面包屑里它是一个 crumb 而不是两个。
/// - POSIX 根 `/` 无前缀可合，自成一段。
///
/// 自动化快照（files 面 crumbs）与本文件渲染共用，防止两边漂移。
pub(crate) fn breadcrumb_segments(
    cwd: &Path,
    path: &Path,
) -> Vec<(SharedString, std::path::PathBuf)> {
    let under_cwd = path.strip_prefix(cwd).is_ok();
    let rel = path.strip_prefix(cwd).unwrap_or(path);
    let mut segments: Vec<(SharedString, std::path::PathBuf)> = Vec::new();
    let mut acc = if under_cwd {
        cwd.to_path_buf()
    } else {
        std::path::PathBuf::new()
    };
    for c in rel.components() {
        if c == std::path::Component::RootDir
            && let Some((text, p)) = segments.last_mut()
        {
            // 前缀段（盘符 / UNC 共享名）+ 根："D:\" / "\\\\server\\share\\"
            let mut joined = text.to_string();
            joined.push(std::path::MAIN_SEPARATOR);
            *text = joined.into();
            p.push(std::path::MAIN_SEPARATOR_STR);
            // 累积路径跟着走（后续 Normal 段从本段续拼）
            acc.push(std::path::MAIN_SEPARATOR_STR);
            continue;
        }
        acc.push(c.as_os_str());
        segments.push((
            c.as_os_str().to_string_lossy().to_string().into(),
            acc.clone(),
        ));
    }
    segments
}

/// 导航操作栏：左 = 面包屑（项目根相对路径段，目录段点开兄弟文件菜单，
/// 对齐 Zed 可点击面包屑）；右 = kind handler 声明的工具栏按钮序列
/// （默认仅 search；md = eye/eye-off 源码渲染切换 + search）+ search（聚焦
/// 编辑器并派发组件 Search，即 Ctrl+F 内置搜索替换弹层）。
fn file_nav_bar(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    kind: &'static dyn FileKind,
    path: &Path,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let source_mode = chat.ws.file_cache.get(path).is_some_and(|f| f.md_source);

    // 面包屑段：cwd 相对路径；不在 cwd 下（工作区外/别的盘打开的文件）退化
    // 为绝对路径。Windows 盘符前缀与根分隔符合成一段——Zed 面包屑同款：
    // `D:\` 是一个 crumb，此前拆成 `D:` 与 `\` 两段（且目录累积 acc 从 cwd
    // 拼起，盘符段后的兄弟菜单指向错误目录）。
    // 无项目态：无 cwd 相对化基准，退化为绝对路径段
    let segments = breadcrumb_segments(
        chat.ws.cwd.as_deref().unwrap_or_else(|| Path::new("")),
        path,
    );
    let mut crumbs = div().min_w_0().flex().items_center().overflow_hidden();
    for (i, (seg_text, dir)) in segments.iter().enumerate() {
        let last = i + 1 == segments.len();
        let seg_text = seg_text.clone();
        if last {
            crumbs = crumbs.child(
                div()
                    .text_size(crate::appearance::ui_size(11.5))
                    .text_color(rgb(t.text))
                    .whitespace_nowrap()
                    .child(seg_text),
            );
        } else {
            let dir = dir.clone();
            let weak_crumb = weak.clone();
            let weak_off = weak.clone();
            let dd = chat.ws.crumb_dd.clone();
            let open = chat.ws.crumb_menu_dir.as_ref() == Some(&dir);
            let seg_dir = dir.clone();
            crumbs = crumbs.child(crate::ui::dropdown(
                SharedString::from(format!("fv-crumb-{i}")),
                &dd,
                open,
                move |_w, cx| {
                    let _ = weak_crumb.update(cx, |c, cx| {
                        // 再点同段 = 收起；点别段 = 换目标
                        c.ws.crumb_menu_dir = if c.ws.crumb_menu_dir.as_ref() == Some(&dir) {
                            None
                        } else {
                            Some(dir.clone())
                        };
                        cx.notify();
                    });
                },
                move |_w, cx| {
                    let _ = weak_off.update(cx, |c, cx| {
                        c.ws.crumb_menu_dir = None;
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
            ));
            crumbs = crumbs.child(
                div()
                    .px(px(3.))
                    .text_size(crate::appearance::ui_size(10.))
                    .child(icon("chevron-right", 9., t.text_faint)),
            );
        }
    }

    // 右侧操作区：handler 声明的按钮序列（023/024：默认仅搜索，md 加预览）。
    // 对齐 caption 格网（2026-10-14）：容器无 gap 无右距，每钮自占 42px 格
    // （与 titlebar 窗口控制同宽同贴缘），search/eye 中心线 = 21/63px，与
    // close/max 上下垂直对齐
    let ed_for_focus = chat.ws.file_cache.get(path).and_then(|f| f.editor.clone());
    let mut right = div().flex_shrink_0().flex().items_center();
    // 二进制占位 tab 无编辑器：search/eye 都是死钮，整列不渲染
    if !chat.ws.file_cache.get(path).is_some_and(|f| f.binary) {
        for btn in kind.toolbar() {
            right = match btn {
                ToolButton::Eye => right.child(eye_button(weak, source_mode)),
                ToolButton::Search => right.child(search_button(ed_for_focus.clone())),
            };
        }
    }

    // 操作栏整体（zed 同构）：第一行 = 面包屑 + eye/search 图标；第二行 =
    // 搜索框（Ctrl+F 展开、Esc 收起，不用时整行收起）——搜索框是操作栏的
    // 一部分，面板元素直接渲染在本栏内（vendor 摘掉了编辑器侧挂载），
    // × 与右侧图标列同缘
    let search_open = ed_for_focus
        .as_ref()
        .is_some_and(|ed| ed.read(cx).search_panel_open(cx));
    let mut bar = div()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .border_b_1()
        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x66)))
        .child(
            div()
                .h(px(32.))
                .flex()
                .items_center()
                .gap(px(8.))
                // 左距 20：与 topbar 会话 tab 的 icon 墨迹对齐
                //（tab = strip pl 10 + tab pl 12，view.rs 面包屑 20 ≈ 22 - 字形留白）
                .pl(px(20.))
                .child(crumbs)
                .child(div().flex_1())
                .child(right),
        );
    if search_open
        && let Some(panel) = ed_for_focus
            .as_ref()
            .and_then(|ed| ed.read(cx).search_panel_element())
    {
        bar = bar.child(panel);
    }
    bar.into_any_element()
}

/// 预览切换按钮（ToolButton::Eye，双态类型工具栏；024）：图标 = 点击后的
/// 动作——源码态显示 eye（回预览），渲染态显示 eye-off（进源码）。
/// 2026-10-14 用户定夺：源码态不再画 bg_selected 选中底色（md 默认源码
/// 态，进去即高亮块观感突兀），hover 底色保留。
/// 2026-10-13 用户定夺：eye/eye-off 字形本身已表意，不再用 accent 上色
/// 区分状态，恒用 text_muted。
fn eye_button(weak: &gpui::WeakEntity<Chat>, source_mode: bool) -> gpui::AnyElement {
    let t = T();
    let weak_eye = weak.clone();
    // 42px 定位格（与 caption_button 同宽）：内部保留 24px 圆角命中区做
    // hover 底色，视觉语言不变，仅列位对齐窗口控制
    div()
        .w(px(42.))
        .h(px(32.))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .id("fv-eye")
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_eye.update(cx, |c, cx| {
                        if let Some(p) = c.active_file_path() {
                            if let Some(ft) = c.ws.file_cache.get_mut(&p) {
                                ft.md_source = !source_mode;
                            }
                            // 切到源码态即聚焦编辑器（渲染帧消费）
                            if !source_mode {
                                c.ws.pending_focus_file = Some(p);
                            }
                        }
                        cx.notify();
                    });
                })
                .child(icon(
                    if source_mode { "eye" } else { "eye-off" },
                    crate::ui::tokens::icon::LG,
                    t.text_muted,
                )),
        )
        .into_any_element()
}

/// 搜索按钮（ToolButton::Search）：聚焦编辑器并派发内置搜索（Ctrl+F 弹层）。
fn search_button(ed: Option<gpui::Entity<gpui_component::input::InputState>>) -> gpui::AnyElement {
    let t = T();
    // 42px 定位格同 eye_button：search 中心线贴 close（距右缘 21px）
    div()
        .w(px(42.))
        .h(px(32.))
        .flex()
        .items_center()
        .justify_center()
        .child(
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
                    if let Some(ed) = &ed {
                        ed.update(cx, |s, scx| s.focus(window, scx));
                        window.dispatch_action(Box::new(gpui_component::input::Search), cx);
                    }
                })
                .child(icon("search", crate::ui::tokens::icon::LG, t.text_muted)),
        )
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
        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x8c)))
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
                .child(SharedString::from(tr("conflict.no_files").to_string())),
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
                        c.ws.crumb_menu_dir = None;
                        c.open_file_tab(p, cx);
                    });
                })
                .child(crate::ui::icon("file", 12., t.text_faint))
                .child(label),
        );
    }
    col.into_any_element()
}

/// 冲突横幅（023 外部改动检测的对齐 Zed 裁决 UI）：仅 Changed 挂载——
/// 「文件已在磁盘上被修改」+ 重新加载 / 保留我的版本。（Deleted 不再走
/// 横幅：zed 的 DiskState::Deleted 语义 = 缓冲保留 + tab 删除线，无横幅。）
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
            tr("conflict.modified_on_disk"),
            tr("conflict.reload"),
            tr("conflict.keep_my_version"),
        ),
        crate::FileConflict::Deleted => unreachable!("Deleted 不再渲染横幅（zed parity）"),
    };
    let mut row = div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(10.))
        .px(px(12.))
        .py(px(6.))
        .bg(rgb(crate::appearance::theme::mix_rgb(0xe0a562, t.bg, 0.82)))
        .border_b_1()
        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x66)))
        .text_size(crate::appearance::ui_size(11.5))
        .text_color(rgb(t.text))
        .child(SharedString::from(msg.to_string()))
        .child(div().flex_1());
    if !reload_label.is_empty() {
        row = row.child(banner_btn(
            "fv-banner-reload",
            reload_label,
            move |_, _, cx| {
                let p = p_reload.clone();
                let _ = weak_reload.update(cx, |c, cx| c.file_reload_from_disk(&p, cx));
            },
        ));
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
        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x8c)))
        .text_size(crate::appearance::ui_size(11.))
        .text_color(rgb(t.text))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .on_mouse_down(MouseButton::Left, handler)
        .child(SharedString::from(label.to_string()))
        .into_any_element()
}

/// 编辑区主体分发壳：主体渲染算法由 kind handler 决定（md 双态/图片查看
/// 器/代码与文本编辑器），本函数零类型特判——类型决策全在 kind.rs。
fn file_editor_body(
    chat: &mut Chat,
    kind: &'static dyn FileKind,
    path: &Path,
    weak: &gpui::WeakEntity<Chat>,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let source_mode = chat.ws.file_cache.get(path).is_some_and(|f| f.md_source);
    kind.render_body(
        cx,
        BodyCtx {
            chat,
            weak,
            path,
            source_mode,
        },
    )
}

/// 图片查看面板（open_file_tab 图片认领 / ImageKind 主体共用）：gpui img()
/// 真渲染（最佳查看方式），无编辑器、只读。
pub(crate) fn render_image_surface(path: &Path, format: gpui::ImageFormat) -> gpui::AnyElement {
    let t = T();
    match std::fs::read(path)
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
        None => empty_hint(tr("file_view.image_read_failed"), t),
    }
}

/// 「无法打开 {ext} 类型文件」的类型名：后缀优先（如 ttf）；无后缀的
/// 二进制（裸名可执行等）退文件名兜底。
fn unsupported_type_name(path: &Path) -> String {
    if let Some(e) = path.extension().and_then(|e| e.to_str()) {
        return e.to_string();
    }
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "?".into())
}

/// 无法打开灰屏（binary tab 主体，仿 Zed Could not open file）：tab 照开
/// 让用户看见是哪个文件，主体居中「无法打开 {ext} 类型文件」+「二进制文件
/// 不受支持」+ 默认应用兜底按钮。纯展示无编辑器——工具栏按钮整列不渲染
/// （file_nav_bar）。
fn unsupported_body(path: &Path) -> gpui::AnyElement {
    let t = T();
    let type_name = unsupported_type_name(path);
    let p = path.to_path_buf();
    div()
        .id("fv-unsupported")
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(8.))
        .bg(rgb(t.bg))
        .child(
            div()
                .text_size(crate::appearance::ui_size(13.))
                .text_color(rgb(t.text))
                .child(SharedString::from(crate::i18n::tf(
                    "file_view.unsupported_type",
                    &[("ext", type_name)],
                ))),
        )
        .child(
            div()
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(
                    tr("file_view.binary_unsupported").to_string(),
                )),
        )
        .child(crate::ui::button(
            "fv-unsupported-open",
            tr("file_view.open_in_default_app"),
            crate::ui::BtnSize::Md,
            crate::ui::BtnVariant::Secondary,
            false,
            move |_, _, _| crate::services::shell_open::open_in_default_app(&p),
        ))
        .into_any_element()
}

/// 编辑器面板（TextKind/CodeKind render_body 与 MarkdownKind 源码态共用）：
/// gpui-component CodeEditor——tree-sitter 高亮 + 行号 + Ctrl+F 搜索替换，
/// 右键菜单（zed）：编辑块 + 文件动作块，渲染挂 psp_overlays 同层。
pub(crate) fn render_editor_surface(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    path: &Path,
    _cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let Some(ft) = chat.ws.file_cache.get(path) else {
        return empty_hint(tr("conflict.file_closed"), t);
    };
    let Some(ed) = ft.editor.clone() else {
        return empty_hint(tr("conflict.initializing_editor"), t);
    };
    let weak_menu = weak.clone();
    let menu_path = path.to_path_buf();
    // 族+字号都随「文档字体」槽位（zed buffer font 同语义，md 预览侧
    // base_style 已同源）；此前族硬编码 JetBrains Mono，设置里选族不生效
    let file_font = crate::appearance::file_font();
    div()
        .id("fv-editor")
        .relative()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        .font_family(file_font.family)
        .text_size(px(file_font.size))
        // zed BufferLineHeight::Comfortable（默认档，1.618；Standard=1.3）：
        // 组件原恒 1.25rem 行距过密（vendor 补丁让 code editor 模式吃继承值）
        .line_height(relative(1.618))
        .on_mouse_down(
            gpui::MouseButton::Right,
            move |ev: &gpui::MouseDownEvent, _, cx| {
                cx.stop_propagation();
                let _ = weak_menu.update(cx, |c, cx| {
                    c.dlg.editor_menu = Some((
                        menu_path.clone(),
                        f32::from(ev.position.x),
                        f32::from(ev.position.y),
                    ));
                    cx.notify();
                });
            },
        )
        .child(
            gpui_component::input::TextInput::new(&ed)
                .h_full()
                .appearance(false)
                .bordered(false)
                .focus_bordered(false),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::breadcrumb_segments;
    use std::path::{Path, PathBuf};

    /// 在 cwd 下的相对分段：每段路径累积正确，最后一段 = 全路径。
    #[test]
    fn crumbs_relative_under_cwd() {
        let cwd = PathBuf::from("C:\\proj");
        let segs = breadcrumb_segments(&cwd, Path::new("C:\\proj\\src\\a.rs"));
        let texts: Vec<String> = segs.iter().map(|(t, _)| t.to_string()).collect();
        assert_eq!(texts, vec!["src", "a.rs"]);
        assert_eq!(segs[0].1, PathBuf::from("C:\\proj\\src"));
        assert_eq!(segs[1].1, PathBuf::from("C:\\proj\\src\\a.rs"));
    }

    /// 工作区外绝对路径：Windows 盘符 + 根合成 "D:\\" 一段（此前拆成
    /// "D:" 与 "\\" 两段，且后续段的累积路径从 cwd 拼起是错的）。
    #[cfg(windows)]
    #[test]
    fn crumbs_absolute_merges_drive_root() {
        let cwd = PathBuf::from("C:\\proj");
        let segs = breadcrumb_segments(&cwd, Path::new("D:\\my_obsidian\\gitee_vault\\x.md"));
        let texts: Vec<String> = segs.iter().map(|(t, _)| t.to_string()).collect();
        assert_eq!(texts, vec!["D:\\", "my_obsidian", "gitee_vault", "x.md"]);
        // 兄弟菜单目标 = 真实绝对目录（不是 "C:\\proj\\D:"）
        assert_eq!(segs[0].1, PathBuf::from("D:\\"));
        assert_eq!(segs[1].1, PathBuf::from("D:\\my_obsidian"));
        assert_eq!(segs[2].1, PathBuf::from("D:\\my_obsidian\\gitee_vault"));
    }

    /// POSIX 根："" 无前缀可合，根自成一段，累积路径仍绝对。
    #[cfg(not(windows))]
    #[test]
    fn crumbs_absolute_keeps_posix_root() {
        let cwd = PathBuf::from("/home/u/proj");
        let segs = breadcrumb_segments(&cwd, Path::new("/etc/pi/x.conf"));
        let texts: Vec<String> = segs.iter().map(|(t, _)| t.to_string()).collect();
        assert_eq!(texts, vec!["/", "etc", "pi", "x.conf"]);
        assert_eq!(segs[0].1, PathBuf::from("/"));
        assert_eq!(segs[1].1, PathBuf::from("/etc"));
    }

    // 扩展名 → kind 路由断言迁至 kind.rs 注册表测试（ext_routing 等）。

    /// 「无法打开 {ext} 类型文件」的类型名：后缀优先（ttf 灰屏正例）；
    /// 无后缀二进制退文件名。
    #[test]
    fn unsupported_type_name_prefers_ext() {
        use super::unsupported_type_name;
        assert_eq!(
            unsupported_type_name(Path::new("fonts/JetBrainsMono-Bold.ttf")),
            "ttf"
        );
        assert_eq!(unsupported_type_name(Path::new("bin\\app")), "app");
        assert_eq!(unsupported_type_name(Path::new("Makefile")), "Makefile");
    }
}
