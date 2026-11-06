//! 文件视图导航栏族（自 view.rs 外移）：面包屑导航条 + 预览/源码切换
//! 眼钮 + 搜索钮 + 同级 siblings 弹出菜单。

use super::*;

/// 导航操作栏：左 = 面包屑（项目根相对路径段，目录段点开兄弟文件菜单，
/// 对齐 Zed 可点击面包屑）；右 = kind handler 声明的工具栏按钮序列
/// （默认仅 search；md = eye/eye-off 源码渲染切换 + search）+ search（聚焦
/// 编辑器并派发组件 Search，即 Ctrl+F 内置搜索替换弹层）。
pub(super) fn file_nav_bar(
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
                // 左距 23：与 topbar 会话 tab icon 墨迹对齐（同 session_fn_bar，
                // 两栏视觉同步）
                .pl(px(23.))
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
pub(super) fn eye_button(weak: &gpui::WeakEntity<Chat>, source_mode: bool) -> gpui::AnyElement {
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
pub(super) fn search_button(
    ed: Option<gpui::Entity<gpui_component::input::InputState>>,
) -> gpui::AnyElement {
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
pub(super) fn crumb_siblings_menu(weak: &gpui::WeakEntity<Chat>, dir: &Path) -> gpui::AnyElement {
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
