//! vlist —— 等高行虚拟化列表（gpui `uniform_list` 的 app 级封装）。
//!
//! 只覆盖「等高行」这一半：uniform_list 靠行高一致才能 O(1) 求可见范围
//! （只构建视口内的行）。变高列表（聊天流、导航轮次卡）走 `ListState` +
//! `list()`，状态机各不相同（贴底跟随/锚点/splice 记账），**不进本抽象**；
//! 它们的滚动条协作另见 [`super::list_handle`]。
//!
//! 统一四件事：uniform_list 构建、高度策略（[`VListHeight`]）、列表壳
//! （圆角+边框+底色+裁剪）、空态。行构建闭包的数据捕获留在调用方（每页
//! 状态不同，本来就不可能共享）。
//! 统一四件事之外还有第五件：**滚动句柄**。uniform_list 的滚动位挂在句柄内部的
//! `RefCell` 上，句柄必须跨帧存活（见 [`scroll_handle`]），这正是「列表滚不动」
//! 的唯一根因，故封装必须自带句柄表。

use std::cell::RefCell;
use std::collections::HashMap;

use gpui::{
    AnyElement, App, UniformListDecoration, UniformListScrollHandle, Window, div, px, rgb,
    prelude::*,
};

/// 高度策略。
pub enum VListHeight {
    /// 填满父容器（父容器须自有限高，如弹层卡片内 flex_1）。
    Fill,
    /// `min(count × row_h, cap)`：行少时列表本身收缩，行多时封顶滚动
    /// （设置页列表同款）。
    Capped(f32),
}

/// 等高行虚拟化列表。
///
/// - `id`：元素 id（滚动状态挂它，同屏勿重名）；
/// - `count` / `row_h`：行数与固定行高（px，逻辑像素，行元素自己 `.h()` 一致）；
/// - `height`：高度策略；
/// - `shell`：套列表壳（圆角 6 / 1px 边框 / bg_panel / 裁剪）；
/// - `scrollbar`：右缘常显滚动条（`menu_scrollbar` 变体：可拖拽/翻页，
///   不可滚动自动不画；仅 shell 路径生效——pi-web `.enabled-models-list`
///   的 overflow-y 同位）；
/// - `empty_text`：`count == 0` 的空态文案（zh-CN 源串，内部过 tr）；
/// - `decoration`：gpui `UniformListDecoration` 装饰层（每次 paint 按当前
///   可见范围/滚动位重算，画在行之上——文件树 sticky 祖先链用；无则传 None）；
/// - `rows`：按**绝对索引**构建一行。gpui 要求 `Fn`（可能一帧多次调用），
///   数据捕获 own 进闭包、行内只读。
#[allow(clippy::too_many_arguments)]
pub fn vlist(
    id: &'static str,
    count: usize,
    row_h: f32,
    height: VListHeight,
    shell: bool,
    scrollbar: bool,
    empty_text: &'static str,
    decoration: Option<Box<dyn UniformListDecoration>>,
    rows: impl 'static + Fn(usize, &mut Window, &mut App) -> AnyElement,
) -> AnyElement {
    let t = crate::theme::theme();

    // ---- 空态：Fill = 填满父容器居中；Capped = 44px 行 + 壳 ----------
    if count == 0 {
        let mut empty = div()
            .flex()
            .items_center()
            .px(px(12.))
            .text_size(crate::appearance::ui_size(11.))
            .text_color(rgb(t.text_dim))
            .child(crate::i18n::tr(empty_text));
        match height {
            VListHeight::Fill => {
                empty = empty
                    .flex_1()
                    .min_h_0()
                    .justify_center()
                    .text_size(crate::appearance::ui_size(12.));
                return empty.into_any_element();
            }
            VListHeight::Capped(_) => {
                empty = empty.h(px(44.));
                if shell {
                    empty = empty
                        .rounded(px(6.))
                        .border_1()
                        .border_color(rgb(t.border))
                        .bg(rgb(t.bg_panel))
                        .overflow_hidden();
                }
                return empty.into_any_element();
            }
        }
    }

    // ---- 列表本体 --------------------------------------------------------
    // 句柄必须跨帧复用（[`scroll_handle`] 里有血案）；track_scroll 后 div
    // interactivity 每帧把 bounds/max_offset 同步进 base_handle，滚动条元素
    // 按它算 thumb。
    let scroll = scroll_handle(id);
    let mut list = gpui::uniform_list(id, count, move |range, window, cx| {
        range.map(|ix| rows(ix, window, cx)).collect()
    })
    .track_scroll(scroll.clone());
    if let Some(decoration) = decoration {
        list = list.with_decoration(DecorationBox(decoration));
    }
    // Fill：外层 relative + flex_1 拿定高（psp scroll-wrap 同款），列表
    // absolute 定死四角吃这个定值。gpui 0.2.2 uniform_list 的契约是
    // 「fixed (or max) height」——flex_1 直挂时 measure 拿到非 Definite
    // 的可用高会画全部内容（文件树首日翻的车：整棵树糊出来、不能滚）。
    // 滚动条兄弟节点挂 relative 包裹上（menu_scrollbar 自带右缘定位）。
    let fill_wrapper = |list: gpui::UniformList| {
        let mut wrapper = div().flex_1().min_h_0().relative().child(
            list.absolute()
                .top(px(0.))
                .left(px(0.))
                .w_full()
                .h_full(),
        );
        if scrollbar {
            wrapper = wrapper.child(super::psp_scrollbar::menu_scrollbar(
                &scroll.0.borrow().base_handle,
            ));
        }
        wrapper
    };
    let (list_el, fixed_h) = match height {
        VListHeight::Fill => (fill_wrapper(list).into_any_element(), None),
        VListHeight::Capped(cap) => {
            let h = (count as f32 * row_h).min(cap);
            (list.h(px(h)).into_any_element(), Some(h))
        }
    };

    if !shell {
        return list_el;
    }
    let mut box_ = div()
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.bg_panel))
        .overflow_hidden();
    box_ = match fixed_h {
        Some(h) => {
            if scrollbar {
                box_.h(px(h)).relative().child(list_el).child(
                    super::psp_scrollbar::menu_scrollbar(&scroll.0.borrow().base_handle),
                )
            } else {
                box_.h(px(h)).child(list_el)
            }
        }
        // Fill：壳填父容器，内层 wrapper 已带列表与滚动条
        None => box_.flex_1().min_h_0().child(list_el),
    };
    box_.into_any_element()
}

/// 取列表的滚动句柄（按 id 复用，跨帧/跨次调用同一个）。
///
/// gpui 的契约是「句柄存在视图里，每帧传给 uniform_list」
/// （`UniformListScrollHandle` 文档原话）——滚动位就在句柄内部的 `RefCell` 上，
/// 由 div interactivity 的滚轮监听器写入。**每帧 `new()` 一个 = 上一帧滚轮写进
/// 旧句柄的偏移下一帧随新句柄归零**：鼠标滚轮看着毫无反应、滚动条 thumb 永远
/// 停在顶端（文件树与设置·可用模型列表同源翻车，见 docs/buglist.md）。
///
/// 本 app 单窗口（`main.rs` 只 `open_window` 一次）、gpui 渲染单线程，故按 id 存
/// 线程局部的句柄表，等价于「句柄挂在视图上」而不必让三个调用点各穿一根句柄
/// （字体弹层的渲染闭包拿不到 Chat，穿参还得再引一层）。约束：**同一 id 同屏只
/// 能出现一次**——各调用点 id 互不相同（file-tree / mc-model-list / font-list）。
/// pub(crate)：文件树 sticky 祖先链的「点击滚动到目录」要按同一 id 拿句柄
/// 写偏移。
pub(crate) fn scroll_handle(id: &'static str) -> UniformListScrollHandle {
    SCROLL_HANDLES.with(|handles| {
        handles
            .borrow_mut()
            .entry(id)
            .or_insert_with(UniformListScrollHandle::new)
            .clone()
    })
}

thread_local! {
    /// vlist 句柄表（每 id 一个，永不回收；id 是 `&'static str`，条目数上界 = 调用点数）。
    static SCROLL_HANDLES: RefCell<HashMap<&'static str, UniformListScrollHandle>> =
        RefCell::new(HashMap::new());
}

/// `Box<dyn UniformListDecoration>` 的新类型包装：`with_decoration` 收
/// `impl UniformListDecoration`，而 trait 不能为外部类型 `Box<dyn …>` 而
/// 实现（E0117），故套一层本地新类型。vlist 的参数用 `Option<Box<…>>`
/// 让 None 调用点免写 turbofish。
struct DecorationBox(Box<dyn UniformListDecoration>);

impl UniformListDecoration for DecorationBox {
    fn compute(
        &self,
        visible_range: std::ops::Range<usize>,
        bounds: gpui::Bounds<gpui::Pixels>,
        scroll_offset: gpui::Point<gpui::Pixels>,
        item_height: gpui::Pixels,
        item_count: usize,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        self.0.compute(
            visible_range,
            bounds,
            scroll_offset,
            item_height,
            item_count,
            window,
            cx,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use gpui::{
        AppContext, Context, Entity, IntoElement, Modifiers, ParentElement, Render, ScrollDelta,
        ScrollWheelEvent, Styled, TestAppContext, VisualTestContext, Window, div, point, px, size,
    };

    use super::{VListHeight, scroll_handle, vlist};

    /// 测试专用 id（与三个真实调用点都不撞，否则会串句柄）
    const ID: &'static str = "vlist-test-list";
    const ROW_H: f32 = 20.;
    const COUNT: usize = 50;
    /// 视口 100px = 5 行；内容 1000px，滚得动
    const VIEWPORT: f32 = 100.;

    /// 记下「本帧真正构建了哪些行」——虚拟化后只有视口内那几行，故行号集合
    /// 就是滚动位的地图。
    struct TestView {
        seen: Rc<RefCell<Vec<usize>>>,
    }

    impl Render for TestView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let seen = self.seen.clone();
            // 与文件树调用点同构（定高 + flex_col 父容器 + Fill 无壳列表）——设置·
            // 可用模型列表同一份代码，只差 Capped(360) + 壳 + 滚动条三个参数
            div()
                .w(px(200.))
                .h(px(VIEWPORT))
                .flex()
                .flex_col()
                .overflow_hidden()
                .child(vlist(
                    ID,
                    COUNT,
                    ROW_H,
                    VListHeight::Fill,
                    false,
                    false,
                    "空",
                    None,
                    move |ix, _, _| {
                        seen.borrow_mut().push(ix);
                        div().w_full().h(px(ROW_H)).into_any_element()
                    },
                ))
        }
    }

    /// 画一帧，返回本帧构建的行号（去重升序）
    fn frame(cx: &mut VisualTestContext, seen: &Rc<RefCell<Vec<usize>>>) -> Vec<usize> {
        seen.borrow_mut().clear();
        cx.draw::<Entity<TestView>>(
            point(px(0.), px(0.)),
            size(px(200.), px(VIEWPORT)),
            |_, cx| {
                cx.new(|_| TestView {
                    seen: seen.clone(),
                })
            },
        );
        let mut rows = seen.borrow().clone();
        rows.sort_unstable();
        rows.dedup();
        rows
    }

    /// 句柄按 id 复用（同一 id 同一个 Rc；不同 id 不串）。
    #[test]
    fn handle_registry_reuses_one_handle_per_id() {
        let a = scroll_handle("vlist-test-a");
        let b = scroll_handle("vlist-test-a");
        let c = scroll_handle("vlist-test-b");
        assert!(Rc::ptr_eq(&a.0, &b.0), "同 id 必须复用句柄");
        assert!(!Rc::ptr_eq(&a.0, &c.0), "不同 id 不得共用句柄");
    }

    /// 用户口径的回归锁：滚轮把偏移写进句柄后，**下一帧仍按新偏移构建行**。
    ///
    /// 句柄每帧 `new()` 时此测必红（滚轮写进上一帧的句柄 → 下一帧归零 →
    /// 鼠标滚轮毫无反应，文件树/模型列表就是这么「滚不动」的）。
    #[gpui::test]
    fn wheel_scroll_keeps_offset_across_frames(cx: &mut TestAppContext) {
        let window = cx.add_empty_window();
        let seen: Rc<RefCell<Vec<usize>>> = Default::default();

        assert_eq!(frame(window, &seen), vec![0, 1, 2, 3, 4]);

        // 滚轮只派发给命中的可滚动 hitbox，故先把指针移进列表
        window.simulate_mouse_move(point(px(100.), px(50.)), None, Modifiers::none());
        window.simulate_event(ScrollWheelEvent {
            position: point(px(100.), px(50.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-3. * ROW_H))),
            ..Default::default()
        });

        // 窗口滚动条/滚轮的写入点就是它——偏移必须落在注册表里那个句柄上，
        // 且 vlist 下一帧读的也正是它
        assert_eq!(
            scroll_handle(ID).0.borrow().base_handle.offset().y,
            px(-3. * ROW_H),
            "滚轮偏移必须写进 vlist 复用的句柄"
        );
        // 视口必须落在第 3 行起（行 0 是 uniform_list 的 measure 探针行，每帧
        // 都会构建一次，与滚动位无关）
        assert_eq!(
            frame(window, &seen),
            vec![0, 3, 4, 5, 6, 7],
            "偏移没生效时这里会是 0..=4"
        );
    }
}
