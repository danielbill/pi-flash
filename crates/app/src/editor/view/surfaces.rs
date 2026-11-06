//! 非文本展示面（自 view.rs 外移）：冲突横幅、图片查看器、不支持类型
//! 占位与编辑器/预览主体装配。

use super::*;

/// 冲突横幅（023 外部改动检测的对齐 Zed 裁决 UI）：仅 Changed 挂载——
/// 「文件已在磁盘上被修改」+ 重新加载 / 保留我的版本。（Deleted 不再走
/// 横幅：zed 的 DiskState::Deleted 语义 = 缓冲保留 + tab 删除线，无横幅。）
pub(super) fn conflict_banner(
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
pub(super) fn banner_btn(
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
pub(super) fn unsupported_type_name(path: &Path) -> String {
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
pub(super) fn unsupported_body(path: &Path) -> gpui::AnyElement {
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
