//! 焦点仲裁辅助（自 chat.rs 外移）：render 帧级焦点回收的白名单判据
//! （文件编辑器 / md 预览 / 编辑器搜索面板）+ Focusable 落点。

use super::Chat;
use crate::*;

impl Chat {
    /// 当前文件 tab 编辑器的焦点句柄（持焦时 Some）——render 帧级焦点回收
    /// 的白名单，与终端/对话框同级（023）。
    pub(crate) fn file_editor_focus(
        &self,
        window: &gpui::Window,
        cx: &App,
    ) -> Option<gpui::FocusHandle> {
        if self.layout.content_view != ContentView::File {
            return None;
        }
        let path = self.active_file_path()?;
        let ed = self.ws.file_cache.get(&path)?.editor.clone()?;
        let st = ed.read(cx);
        let handle = st.focus_handle(cx);
        // 搜索面板（Ctrl+F）输入框持焦同样不回收——它不是编辑器本体的
        // focus_handle，漏判会被下方仲裁抢回 composer，面板打不了字
        (handle.is_focused(window) || st.search_panel_focused(window, cx)).then_some(handle)
    }

    /// 082 三期：当前文件 tab 是否处于 **markdown 渲染预览态**（File 视图
    /// + md kind + md_source=false）——逐帧仲裁据此把焦点交给预览容器。
    pub(crate) fn md_preview_active(&self) -> bool {
        if self.layout.content_view != ContentView::File {
            return false;
        }
        let Some(path) = self.active_file_path() else {
            return false;
        };
        crate::editor::kind::is_markdown(&path)
            && self
                .ws
                .file_cache
                .get(&path)
                .is_some_and(|f| !f.md_source && !f.binary)
    }

    /// 082：编辑器搜索面板（Ctrl+F）输入框持焦判定——ViewLeft/ViewRight
    /// 在此退回组件词移动（查词打字时 Ctrl+←/→ 仍是跳词，不能切走 tab
    /// 把面板焦点弄丢）。编辑器本体持焦不算（那里切 tab 正是需求）。
    pub(crate) fn editor_search_panel_focused(&self, window: &gpui::Window, cx: &App) -> bool {
        if self.layout.content_view != ContentView::File {
            return false;
        }
        let Some(path) = self.active_file_path() else {
            return false;
        };
        let Some(ed) = self.ws.file_cache.get(&path).and_then(|f| f.editor.clone()) else {
            return false;
        };
        ed.read(cx).search_panel_focused(window, cx)
    }
}

impl Focusable for Chat {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        // composer 就绪后，"聚焦聊天"即落到输入组件（真输入框：光标/
        // 选区/IME 全可用），chat.layout.focus 仅作组件创建前的回退
        self.composer
            .composer
            .as_ref()
            .map(|c| c.read(cx).focus_handle_in(cx))
            .unwrap_or_else(|| self.layout.focus.clone())
    }
}
