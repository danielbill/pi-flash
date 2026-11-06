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

mod bar;
mod surfaces;

use self::bar::file_nav_bar;
#[cfg(test)]
use self::surfaces::unsupported_type_name;
use self::surfaces::{conflict_banner, unsupported_body};
pub(crate) use self::surfaces::{render_editor_surface, render_image_surface};

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
