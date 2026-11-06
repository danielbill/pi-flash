use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    /// 回归（图片上下留白）：宽图受 max_h 约束时，绘制框必须同比例缩放，
    /// 不能把宽定成 720、高定成 520 —— 那样 Contain 会在盒内再留白。
    #[test]
    fn fit_box_keeps_aspect_ratio_inside_max() {
        // 2:1 宽图 → 受宽度约束（720×360，高度还远没到 520）
        assert_eq!(fit_box(2880, 1440, 720., 520.), (720., 360.));
        // 16:9 宽图 → 受宽度约束，高 405 < 520
        assert_eq!(fit_box(1920, 1080, 720., 520.), (720., 405.));
        // 竖图 → 受高度约束
        assert_eq!(fit_box(1000, 4000, 720., 520.), (130., 520.));
        // 小图不放大小（保持 gpui 原生像素尺寸）
        assert_eq!(fit_box(100, 50, 720., 520.), (100., 50.));
        // 退化输入不 panic
        assert_eq!(fit_box(0, 0, 720., 520.), (720., 520.));
    }

    #[test]
    fn natural_size_parses_png_jpeg_gif_bmp_webp() {
        // PNG 8 字节签名 + IHDR
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&1920u32.to_be_bytes());
        png.extend_from_slice(&1080u32.to_be_bytes());
        png.extend_from_slice(&[0; 20]);
        assert_eq!(
            image_natural_size(&png, gpui::ImageFormat::Png),
            Some((1920, 1080))
        );

        // JPEG：SOI + APP0 段 + SOF0
        let mut jpg = vec![0xFF, 0xD8, 0xFF, 0xE0];
        jpg.extend_from_slice(&16u16.to_be_bytes());
        jpg.extend_from_slice(&[0; 14]);
        jpg.extend_from_slice(&[0xFF, 0xC0]);
        jpg.extend_from_slice(&17u16.to_be_bytes());
        jpg.push(8);
        jpg.extend_from_slice(&1080u16.to_be_bytes());
        jpg.extend_from_slice(&1920u16.to_be_bytes());
        jpg.push(3);
        jpg.extend_from_slice(&[0; 9]);
        assert_eq!(
            image_natural_size(&jpg, gpui::ImageFormat::Jpeg),
            Some((1920, 1080))
        );

        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&320u16.to_le_bytes());
        gif.extend_from_slice(&240u16.to_le_bytes());
        gif.extend_from_slice(&[0; 8]);
        assert_eq!(
            image_natural_size(&gif, gpui::ImageFormat::Gif),
            Some((320, 240))
        );

        let mut bmp = b"BM".to_vec();
        bmp.extend_from_slice(&[0; 16]);
        bmp.extend_from_slice(&(-800i32).to_le_bytes());
        bmp.extend_from_slice(&600i32.to_le_bytes());
        assert_eq!(
            image_natural_size(&bmp, gpui::ImageFormat::Bmp),
            Some((800, 600))
        );

        let mut vp8 = b"RIFF".to_vec();
        vp8.extend_from_slice(&[0; 4]);
        vp8.extend_from_slice(b"WEBP");
        vp8.extend_from_slice(b"VP8 ");
        vp8.extend_from_slice(&[0; 4]);
        vp8.extend_from_slice(&[0x30, 0x01, 0x00]); // 帧 tag
        vp8.extend_from_slice(&[0x9d, 0x01, 0x2a]); // 同步码
        vp8.extend_from_slice(&1280u16.to_le_bytes());
        vp8.extend_from_slice(&720u16.to_le_bytes());
        assert_eq!(
            image_natural_size(&vp8, gpui::ImageFormat::Webp),
            Some((1280, 720))
        );

        assert_eq!(image_natural_size(b"<svg/>", gpui::ImageFormat::Svg), None);
    }
}

#[cfg(test)]
mod skill_fold_tests {
    use super::skill_expansion_to_command;

    // pi-web lib/slash-display.test.mjs skillExpansion fixture 同款
    fn envelope(body: &str, args: Option<&str>) -> String {
        let args_s = args.map(|a| format!("\n\n{}", a)).unwrap_or_default();
        format!(
            "<skill name=\"review\" location=\"/path/to/review/SKILL.md\">\nReferences are relative to /path/to/review.\n\n{body}\n</skill>{args}",
            body = body,
            args = args_s,
        )
    }

    #[test]
    fn restores_with_args() {
        assert_eq!(
            skill_expansion_to_command(&envelope(
                "Review the supplied files.",
                Some("src/main.ts")
            ))
            .as_deref(),
            Some("/skill:review src/main.ts")
        );
    }
    #[test]
    fn restores_without_args() {
        assert_eq!(
            skill_expansion_to_command(&envelope("Review the supplied files.", None)).as_deref(),
            Some("/skill:review")
        );
    }
    #[test]
    fn multiline_args() {
        assert_eq!(
            skill_expansion_to_command(&envelope("Body.", Some("first line\nsecond line")))
                .as_deref(),
            Some("/skill:review first line\nsecond line")
        );
    }
    #[test]
    fn final_closing_tag_wins() {
        assert_eq!(
            skill_expansion_to_command(&envelope("Example:\n</skill>\nContinue.", Some("src")))
                .as_deref(),
            Some("/skill:review src")
        );
    }

    #[test]
    fn lookalike_not_collapsed() {
        assert_eq!(
            skill_expansion_to_command(
                "<skill name=\"review\" location=\"/path/to/review/SKILL.md\">\nordinary user text"
            ),
            None
        );
        assert_eq!(
            skill_expansion_to_command(
                "<skill name=\"review\" location=\"/path/to/review/SKILL.md\">\nReferences are elsewhere.\n\nbody\n</skill>"
            ),
            None
        );
        assert_eq!(skill_expansion_to_command("ordinary user text"), None);
    }
}

#[cfg(test)]
mod bar_hover_hit_test {
    //! 用户消息行操作栏悬停（v62 修复）的 gpui 几何前提锁。
    //!
    //! `bar_hover_wired` 把 on_hover 挂在**整行**上，靠的是「行内后代不遮挡」
    //! 这条 gpui 语义：`Window::hit_test` 一旦命中 `HitboxBehavior::BlockMouse`
    //! （`div().occlude()`）就 `break`，被它挡住的祖先 hitbox 连 `ids` 都进不
    //! 去，`is_hovered()` 恒 false。用户气泡曾挂 `occlude()`，于是鼠标停在气泡
    //! 上时行级 on_hover 永不触发 → 操作栏不显影；agent 轮块无遮挡后代故一切
    //! 正常（同一个行为两处表现）。
    //!
    //! 两侧都锁：不遮挡 → 行悬停必须触发（线上行为）；遮挡 → 必须被挡掉（gpui
    //! 现语义；上游若改，这条先炸，提示回来复核用户行实现与上面那段注释）。

    use std::cell::Cell;
    use std::rc::Rc;

    use gpui::{
        AppContext, Context, IntoElement, Modifiers, ParentElement, Render, Styled, TestAppContext,
        VisualTestContext, Window, div, point, prelude::*, px, size,
    };

    /// 迷你复刻用户消息行：row = `bar_hover_wired` 的挂载点，bubble = 气泡本体
    /// （`bubble_occludes` 开关模拟 v62 前后两种写法）。
    struct Row {
        row_hovered: Rc<Cell<usize>>,
        bubble_hovered: Rc<Cell<usize>>,
        bubble_occludes: bool,
    }

    impl Render for Row {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let row = self.row_hovered.clone();
            let bubble = self.bubble_hovered.clone();
            let bubble_el = div()
                .id("bubble")
                .when(self.bubble_occludes, |d| d.occlude())
                .w(px(100.))
                .h(px(40.))
                .on_hover(move |hovered, _, _| {
                    if *hovered {
                        bubble.set(bubble.get() + 1);
                    }
                });
            div()
                .id("row")
                .w(px(400.))
                .h(px(100.))
                .on_hover(move |hovered, _, _| {
                    if *hovered {
                        row.set(row.get() + 1);
                    }
                })
                .child(bubble_el)
        }
    }

    /// 画一帧后把指针移到气泡正中，返回 (行 on_hover 次数, 气泡 on_hover 次数)
    fn hover_bubble(cx: &mut TestAppContext, bubble_occludes: bool) -> (usize, usize) {
        let window: &mut VisualTestContext = cx.add_empty_window();
        let row_hovered: Rc<Cell<usize>> = Default::default();
        let bubble_hovered: Rc<Cell<usize>> = Default::default();
        let row = row_hovered.clone();
        let bubble = bubble_hovered.clone();
        let _ = window.draw(
            point(px(0.), px(0.)),
            size(px(400.), px(100.)),
            move |_, cx| {
                cx.new(|_| Row {
                    row_hovered: row.clone(),
                    bubble_hovered: bubble.clone(),
                    bubble_occludes,
                })
            },
        );
        window.simulate_mouse_move(point(px(50.), px(20.)), None, Modifiers::none());
        (row_hovered.get(), bubble_hovered.get())
    }

    /// 正例（v62 线上行为）：气泡不遮挡 → 行悬停触发 → `Chat.bar_hover` 置位
    /// → 操作栏显影，与 agent 轮块同构。
    #[gpui::test]
    fn bubble_without_occluder_lets_row_hover_fire(cx: &mut TestAppContext) {
        let (row, bubble) = hover_bubble(cx, false);
        assert_eq!(bubble, 1, "指针在气泡内，气泡自身必须 hover");
        assert_eq!(row, 1, "行级 on_hover 必须触发——缺了它操作栏就不显影");
    }

    /// 反例（v62 之前的线上行为，勿再退化）：气泡挂 `occlude()` → 行 hitbox 被
    /// 截断在 hit-test 之外 → 行级 on_hover 永不触发 → 操作栏不显影。
    #[gpui::test]
    fn bubble_occluder_swallows_row_hover(cx: &mut TestAppContext) {
        let (row, bubble) = hover_bubble(cx, true);
        assert_eq!(bubble, 1);
        assert_eq!(
            row, 0,
            "occlude 会连祖先一起截断（gpui 语义若变，此断言先炸）"
        );
    }
}

#[cfg(test)]
mod bubble_wheel {
    //! 用户气泡滚轮归属（v63-9）的回归锁。
    //!
    //! 现象：多行用户消息限高 300px 出滚动条，滚轮却「滚不动气泡，只有整屏
    //! 跟着滚」。根因不在气泡，在 gpui `List` 的滚轮注册时机——
    //! `List::paint` 原本先 paint 条目、**末尾**才 `on_mouse_event` 注册自己的
    //! 滚轮，而冒泡相位按注册**逆序**派发（后注册先收到）⇒ 列表比条目先滚，
    //! 条目内滚动容器的边界拦截（`cx.stop_propagation()`）永远晚一步，同帧
    //! 双层一起滚，气泡相对屏幕位置不变 = 看起来完全没滚。
    //!
    //! vendor 补丁把注册挪到条目循环之前（`elements/list.rs`），本测盯死语义：
    //! 内层没到边界就只滚内层、列表原地不动；内层到边界才轮到列表。
    use gpui::{
        AppContext, Context, IntoElement, ListAlignment, ListState, Modifiers, ParentElement,
        Render, ScrollDelta, ScrollHandle, ScrollWheelEvent, Styled, TestAppContext,
        VisualTestContext, Window, div, point, prelude::*, px, size,
    };

    const VP: f32 = 600.;
    /// 气泡限高（与 render_msg 的 USER_BUBBLE_MAX_HEIGHT 同值）
    const BUBBLE_H: f32 = 300.;

    struct View {
        state: ListState,
        handle: ScrollHandle,
    }

    impl Render for View {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let handle = self.handle.clone();
            div().w_full().h(px(VP)).child(
                gpui::list(self.state.clone(), move |ix, _, _| {
                    let sh = handle.clone();
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .id(("bubble", ix))
                                .h(px(BUBBLE_H))
                                .overflow_y_scroll()
                                .track_scroll(&sh)
                                // 与 turn.rs render_msg 里的边界拦截同构
                                .on_scroll_wheel({
                                    let sh = sh.clone();
                                    move |ev: &ScrollWheelEvent, window, cx| {
                                        let max = sh.max_offset().height;
                                        if max <= px(0.) {
                                            return;
                                        }
                                        let delta_y = ev.delta.pixel_delta(window.line_height()).y;
                                        if delta_y == px(0.) {
                                            return;
                                        }
                                        // 同 div 内置滚动先跑，offset 已含本次 delta
                                        let off_pre = sh.offset().y - delta_y;
                                        const EPS: f32 = 1.;
                                        let at_edge = if delta_y > px(0.) {
                                            off_pre >= px(-EPS)
                                        } else {
                                            off_pre <= px(-f32::from(max) + EPS)
                                        };
                                        if !at_edge {
                                            cx.stop_propagation();
                                        }
                                    }
                                })
                                .child(div().h(px(1000.))),
                        )
                        // 气泡下垫高，让列表本身也有滚动余量
                        .child(div().h(px(900.)))
                        .into_any_element()
                })
                .h_full(),
            )
        }
    }

    /// 画两帧（第一帧只测量条目，句柄的 max_offset 在第二帧才可信）后滚一次
    /// 滚轮，返回（气泡偏移, 列表逻辑位）。
    fn wheel(
        window: &mut VisualTestContext,
        state: &ListState,
        handle: &ScrollHandle,
        delta_y: f32,
    ) -> (f32, ListOffsetProbe) {
        window.simulate_mouse_move(point(px(100.), px(100.)), None, Modifiers::none());
        window.simulate_event(ScrollWheelEvent {
            position: point(px(100.), px(100.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(delta_y))),
            ..Default::default()
        });
        let lt = state.logical_scroll_top();
        (
            f32::from(handle.offset().y),
            ListOffsetProbe {
                item_ix: lt.item_ix,
                offset_in_item: f32::from(lt.offset_in_item),
            },
        )
    }

    /// 再画一帧：滚轮写进偏移后，超量部分由下一帧 prepaint 夹回 [-max, 0]
    fn repaint(window: &mut VisualTestContext, state: &ListState, handle: &ScrollHandle) {
        let (s, h) = (state.clone(), handle.clone());
        window.draw::<gpui::Entity<View>>(
            point(px(0.), px(0.)),
            size(px(400.), px(VP)),
            move |_, cx| {
                cx.new(|_| View {
                    state: s.clone(),
                    handle: h.clone(),
                })
            },
        );
    }

    #[derive(Debug, PartialEq)]
    struct ListOffsetProbe {
        item_ix: usize,
        offset_in_item: f32,
    }

    fn setup(cx: &mut TestAppContext) -> (&mut VisualTestContext, ListState, ScrollHandle) {
        let window: &mut VisualTestContext = cx.add_empty_window();
        let state = ListState::new(1, ListAlignment::Top, px(1000.));
        let handle = ScrollHandle::new();
        let (s, h) = (state.clone(), handle.clone());
        for _ in 0..2 {
            let (s, h) = (s.clone(), h.clone());
            window.draw::<gpui::Entity<View>>(
                point(px(0.), px(0.)),
                size(px(400.), px(VP)),
                move |_, cx| {
                    cx.new(|_| View {
                        state: s.clone(),
                        handle: h.clone(),
                    })
                },
            );
        }
        assert!(
            handle.max_offset().height > px(0.),
            "气泡必须真的溢出限高（否则本测在测空气）"
        );
        (window, state, handle)
    }

    /// 正例（v63-9 线上行为）：气泡内部还有余量时滚轮只滚气泡，会话列表不动。
    #[gpui::test]
    fn wheel_over_tall_bubble_does_not_drag_the_list(cx: &mut TestAppContext) {
        let (window, state, handle) = setup(cx);
        let (bubble, list) = wheel(window, &state, &handle, -50.);
        assert_eq!(bubble, -50., "气泡自己必须滚起来");
        assert_eq!(
            list,
            ListOffsetProbe {
                item_ix: 0,
                offset_in_item: 0.
            },
            "气泡没到边界，会话列表不得同帧跟着滚（穿透 = 用户看到的「滚不动」）"
        );
    }

    /// 气泡滚到底后，滚轮才交给列表——原生嵌套滚动语义。
    #[gpui::test]
    fn wheel_hands_over_to_list_only_at_bubble_edge(cx: &mut TestAppContext) {
        let (window, state, handle) = setup(cx);
        let max = f32::from(handle.max_offset().height);
        // 一记正好到底：气泡吃掉这一记，列表还是不动
        let (bubble, list) = wheel(window, &state, &handle, -max);
        repaint(window, &state, &handle);
        assert_eq!(f32::from(handle.offset().y), -max, "气泡停在下限");
        assert_eq!(bubble, -max);
        assert_eq!(
            list,
            ListOffsetProbe {
                item_ix: 0,
                offset_in_item: 0.
            }
        );

        // 已在底部：下一记轮到列表（气泡仍钉在下限）
        let (_, list) = wheel(window, &state, &handle, -120.);
        repaint(window, &state, &handle);
        assert_eq!(f32::from(handle.offset().y), -max, "气泡仍在底部");
        assert_eq!(list.offset_in_item, 120., "到边界后列表接手滚轮");
        assert_eq!(list.item_ix, 0);

        // 往上滚：光标仍在气泡上、气泡有上行余量 → 气泡先滚，列表不动
        let (bubble, list) = wheel(window, &state, &handle, 60.);
        assert_eq!(bubble, -max + 60.);
        assert_eq!(
            list,
            ListOffsetProbe {
                item_ix: 0,
                offset_in_item: 120.
            }
        );
    }
}
