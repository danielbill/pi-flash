//! 应用内 HTML 真渲染（v54.4）：wry(WebView2) 子窗口浮于 gpui 表面之上，
//! pi-web FileViewer 的 iframe srcDoc parity。html 文件 tab 激活时覆盖
//! 内容区渲染完整网页（脚本可执行），切走即隐藏。
//!
//! 生命周期：render 尾部 `sync_html_panel` 以期望状态（哪个文件/是否可见/
//! bounds）diff 实际状态，差量驱动 create / load_html / set_bounds /
//! set_visible。bounds 由布局常量计算（statusbar 36 + topbar 36 + 文件头
//! 34 + psp 宽），每帧跟随（拖宽/resize 自动贴齐）。坐标用 wry Logical
//! （与 gpui 逻辑像素同源，DPI 由 wry 处理）。

use std::path::PathBuf;

use wry::dpi::{LogicalPosition, LogicalSize};

/// 期望的 webview 几何（逻辑像素），render 尾部算好。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct HtmlPanelGeo {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl HtmlPanelGeo {
    pub(crate) fn new(chat: &crate::Chat, window: &gpui::Window) -> Self {
        let vp = window.viewport_size();
        let f = |p: gpui::Pixels| f32::from(p);
        let x = if chat.panes_hidden { 0. } else { chat.slp_w };
        // topbar 36 + 文件头 34
        let y = 36. + 34.;
        Self {
            x,
            y,
            w: (f(vp.width) - x).max(100.),
            h: (f(vp.height) - y).max(100.),
        }
    }

    fn rect(&self) -> wry::Rect {
        wry::Rect {
            position: LogicalPosition::new(self.x, self.y).into(),
            size: LogicalSize::new(self.w, self.h).into(),
        }
    }
}

/// 应用内 HTML 渲染面板（单实例复用；切文件 load_html）。
pub(crate) struct HtmlPanel {
    webview: wry::WebView,
    /// 当前加载的文件（load_html 幂等判断）
    pub path: PathBuf,
}

impl HtmlPanel {
    /// 从缓存的 HWND 创建（pump 上下文无 Window——wry 的调用一律在
    /// render 之外的空闲点执行，否则 Win32 消息重入 render 借用会 panic）
    pub(crate) fn create_with_hwnd(
        hwnd: isize,
        path: PathBuf,
        html: &str,
        geo: HtmlPanelGeo,
    ) -> Result<Self, String> {
        struct Hwnd(isize);
        impl wry::raw_window_handle::HasWindowHandle for Hwnd {
            fn window_handle(
                &self,
            ) -> Result<wry::raw_window_handle::WindowHandle<'_>, wry::raw_window_handle::HandleError>
            {
                use wry::raw_window_handle::{RawWindowHandle, WindowHandle, Win32WindowHandle};
                let handle = Win32WindowHandle::new(
                    std::num::NonZeroIsize::new(self.0).expect("hwnd non-zero"),
                );
                Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
            }
        }
        let webview = wry::WebView::new_as_child(&Hwnd(hwnd), wry::WebViewAttributes::default())
            .map_err(|e| format!("webview create: {e}"))?;
        let _ = webview.set_bounds(geo.rect());
        let _ = webview.set_visible(false);
        webview
            .load_html(html)
            .map_err(|e| format!("load html: {e}"))?;
        Ok(Self { webview, path })
    }

    /// 切换加载的文件内容。
    pub(crate) fn load(&mut self, path: PathBuf, html: &str) -> Result<(), String> {
        self.webview
            .load_html(html)
            .map_err(|e| format!("load html: {e}"))?;
        self.path = path;
        Ok(())
    }

    pub(crate) fn set_bounds(&self, geo: &HtmlPanelGeo) {
        let _ = self.webview.set_bounds(geo.rect());
    }

    pub(crate) fn set_visible(&self, visible: bool) {
        let _ = self.webview.set_visible(visible);
    }
}
