//! gpui ListState → gpui-component Scrollbar 适配（v63-6）。
//!
//! gpui-component 的 Scrollbar 只认 `ScrollHandleOffsetable`（div 滚动的
//! ScrollHandle / uniform_list 句柄），不认变高 `ListState`。gpui 的
//! ListState 自带一组滚动条协作接口（list.rs：scroll_px_offset_for_
//! scrollbar / set_offset_from_scrollbar / max_offset_for_scrollbar），
//! 符号约定与 ScrollHandle 一致（负 y），本适配器原样转接——拖拽期间
//! 高度测量稳定（scrollbar_drag_* 由 Scrollbar 不经此调用，暂不需要）。

use gpui::{px, size, ListState, Pixels, Point, Size};
use gpui_component::scroll::ScrollHandleOffsetable;

/// ListState 的 Scrollbar 句柄视图（Clone 便宜：ListState 本就是 Rc）。
#[derive(Clone)]
pub(crate) struct ListStateHandle(pub ListState);

impl ScrollHandleOffsetable for ListStateHandle {
    fn offset(&self) -> Point<Pixels> {
        // list.rs 返回 (0, -当前偏移)，与 ScrollHandle 的负 y 约定一致
        self.0.scroll_px_offset_for_scrollbar()
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        self.0.set_offset_from_scrollbar(offset);
    }

    fn content_size(&self) -> Size<Pixels> {
        // 与 ScrollHandle::content_size（max_offset + bounds.size）同构
        size(
            px(0.),
            self.0.max_offset_for_scrollbar().height
                + self.0.viewport_bounds().size.height,
        )
    }
}
