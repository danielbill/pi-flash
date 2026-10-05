//! Dropdown — 通用下拉组件：触发按钮 + 贴附弹层。
//!
//! 锚定机制与 vendored gpui-component 的 dropdown 相同：弹层经 `deferred`
//! 保留块流内布局（absolute 子元素静态位置 = 触发按钮正下方），`anchored`
//! 负责窗口内收口与溢出翻转——**不需要手工捕获/计算任何坐标**，`deferred`
//! 同时让弹层逃出滚动容器的裁剪。
//!
//! 外点收起用 `on_mouse_down_out`（点弹层外任意处触发，含触发按钮本身）；
//! 守卫状态挡掉"外点收起后同一次点击又把菜单展开"的双触发（down 顺序不
//! 保证，用 300ms 时间窗判定）。

use gpui::{
    anchored, deferred, div, prelude::*, px, AnyElement, App, ElementId, Entity, MouseButton,
    Window,
};

/// 弹层收起的防抖守卫（同一组件的多次开合共享一个实例即可）。
pub struct DropdownState {
    last_dismiss: Option<std::time::Instant>,
}

impl DropdownState {
    pub fn new() -> Self {
        Self { last_dismiss: None }
    }

    fn note_dismiss(&mut self) {
        self.last_dismiss = Some(std::time::Instant::now());
    }

    fn dismissed_recently(&self) -> bool {
        self.last_dismiss
            .map(|t| t.elapsed().as_millis() < 300)
            .unwrap_or(false)
    }
}

/// `open` 由调用方状态决定（快照值）；`on_toggle` = 触发按钮被点；
/// `on_dismiss` = 弹层外被点。`popup()` 仅在 open 时构建（惰性）。
pub fn dropdown(
    id: impl Into<ElementId>,
    guard: &Entity<DropdownState>,
    open: bool,
    on_toggle: impl Fn(&mut Window, &mut App) + 'static,
    on_dismiss: impl Fn(&mut Window, &mut App) + 'static,
    trigger: AnyElement,
    popup: impl FnOnce() -> AnyElement,
) -> AnyElement {
    let guard_toggle = guard.clone();
    let guard_out = guard.clone();

    div()
        .id(id.into())
        // 触发按钮（block 流第一个盒子）
        .child(
            div()
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    // 刚被外点收起（同一次点击的 down 已把菜单关掉）→ 忽略
                    if !guard_toggle.read(cx).dismissed_recently() {
                        on_toggle(window, cx);
                    }
                })
                .child(trigger),
        )
        // 弹层：deferred 保持块流静态位置（按钮正下方）+ anchored 窗口收口
        .when(open, |d| {
            d.child(
                deferred(
                    anchored()
                        .snap_to_window_with_margin(px(8.))
                        .child(
                            div()
                                .occlude()
                                .mt(px(4.))
                                .on_mouse_down_out(move |_, window, cx| {
                                    guard_out.update(cx, |s, _| s.note_dismiss());
                                    on_dismiss(window, cx);
                                    // down_out 是 capture 阶段监听器，stop 后 bubble
                                    // 整体跳过：外点只关本弹层，不会穿透到下层表面
                                    // （如设置弹窗的点外关闭），避免一次点击关两层
                                    cx.stop_propagation();
                                })
                                .child(popup()),
                        ),
                )
                .with_priority(1),
            )
        })
        .into_any_element()
}
