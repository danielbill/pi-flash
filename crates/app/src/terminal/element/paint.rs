//! 网格绘制（`TerminalElement::paint` 的闭包体外移）：快照锁快照 →
//! run 分段着色 → 光标条/IME 下划线 → 鼠标/滚轮监听注册；附 run 合并
//! 与终端字体助手。

use super::*;

/// 闭包按值捕获的捕获集逐参透传（与外移前逐字节同参同语义）。
pub(super) fn paint_grid(
    focused: bool,
    term: Arc<FairMutex<Term<Proxy>>>,
    grid_rows: usize,
    selection: Option<(SelPt, SelPt)>,
    line_h: f32,
    font_size: Pixels,
    self_font_size: f32,
    preedit: Option<String>,
    blink_on: bool,
    shared: TermShared,
    bounds: gpui::Bounds<Pixels>,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    // snapshot the grid while locked, then paint styled runs;
    // caret viewport position rides along for the caret bar and
    // the preedit overlay
    let (rows, caret) = {
        let locked = term.lock();
        let rows = snapshot(&locked, grid_rows, selection);
        let offset = locked.grid().display_offset() as i32;
        let content = locked.renderable_content();
        let row = content.cursor.point.line.0 + offset;
        let col = content.cursor.point.column.0;
        // Hidden = app hid the cursor (DECTCEM); bar stays off.
        // chars_before = snapshot-text chars left of the cursor
        // (wide spacers are skipped there), resolved against the
        // shaped row below: CJK fallback glyphs don't advance
        // exactly 2 cells, so col*cell_w drifts off the painted
        // text the more wide chars sit before the caret
        let chars_before = (0..col)
            .filter(|c| {
                !locked.grid()[Line(content.cursor.point.line.0)][Column(*c)]
                    .flags
                    .contains(Flags::WIDE_CHAR_SPACER)
            })
            .count();
        let caret =
            (row >= 0 && (row as usize) < grid_rows && content.cursor.shape != CursorShape::Hidden)
                .then(|| (row as usize, chars_before));
        (rows, caret)
    };
    let line_h = px(line_h);
    let mut caret_x: Option<Pixels> = None;
    for (r, row) in rows.iter().enumerate() {
        if row.text.is_empty() {
            continue;
        }
        let runs = merge_runs(row);
        let line = window.text_system().shape_line(
            SharedString::from(row.text.clone()),
            font_size,
            &runs,
            None,
        );
        let origin = gpui::point(
            bounds.origin.x + px(PAD_L),
            bounds.origin.y + px(PAD_T) + line_h * r as f32,
        );
        // ShapedLine::paint only draws glyphs + decorations; run
        // backgrounds (cursor block / selection / ANSI bg) need
        // the explicit paint_background pass first
        let _ = line.paint_background(origin, line_h, window, cx);
        let _ = line.paint(origin, line_h, window, cx);
        if let Some((ri, chars_before)) = &caret {
            if *ri == r {
                let byte_ix = row
                    .text
                    .chars()
                    .take(*chars_before)
                    .map(|c| c.len_utf8())
                    .sum::<usize>();
                caret_x = Some(line.x_for_index(byte_ix));
            }
        }
    }

    // caret bar: 2px vertical line in the fg color at the shaped
    // pixel x of the cursor, ~1Hz blink while focused (heartbeat
    // from the term_cursor task); dim static bar when unfocused
    let _ = shared
        .weak
        .update(cx, |chat, _| chat.ws.term_cursor_focused = focused);
    if let (Some((row_ix, _)), Some(x)) = (caret, caret_x) {
        let (alpha, visible) = if focused {
            (1.0, blink_on)
        } else {
            (0.35, true)
        };
        if visible {
            let bar_origin = gpui::point(
                bounds.origin.x + px(PAD_L) + x,
                bounds.origin.y + px(PAD_T) + line_h * row_ix as f32 + px(1.),
            );
            window.paint_quad(gpui::fill(
                gpui::Bounds {
                    origin: bar_origin,
                    size: gpui::size(px(2.), line_h - px(2.)),
                },
                hsl_a(TERM_FG, alpha),
            ));
        }
    }

    // IME composition: underline the preedit string at the caret
    // pixel x (xterm draws composition inline; same affordance)
    if focused && let (Some(pre), Some((row_ix, _)), Some(x)) = (&preedit, caret, caret_x) {
        let run = gpui::TextRun {
            len: pre.len(),
            font: cell_font(&CellStyle {
                fg: hsl(TERM_FG),
                bg: None,
                bold: false,
                italic: false,
                underline: true,
                strike: false,
            }),
            color: hsl(TERM_FG),
            background_color: None,
            underline: Some(UnderlineStyle {
                thickness: px(1.),
                ..Default::default()
            }),
            strikethrough: None,
        };
        let line = window.text_system().shape_line(
            SharedString::from(pre.clone()),
            font_size,
            &[run],
            None,
        );
        let origin = gpui::point(
            bounds.origin.x + px(PAD_L) + x,
            bounds.origin.y + px(PAD_T) + line_h * row_ix as f32,
        );
        window.paint_quad(gpui::fill(
            gpui::Bounds {
                origin,
                size: gpui::size(line.width, line_h),
            },
            hsl(0x1d222b),
        ));
        let _ = line.paint(origin, line_h, window, cx);
    }

    // IME target for this frame: composition → preedit overlay,
    // commit → PTY bytes. Without a registered handler the
    // Windows IME path drops composition/commit silently, so
    // 输入法 can never type into the terminal.
    if focused {
        window.handle_input(
            &shared.focus,
            TermInput {
                tab_id: shared.tab_id,
                term: shared.term.clone(),
                pty: shared.pty.clone(),
                weak: shared.weak.clone(),
                element_bounds: bounds,
                cell_w: shared.cell_w,
                line_h: shared.line_h,
                font_size: self_font_size,
            },
            cx,
        );
    }

    // mouse: selection drag + wheel scroll (hit-tested manually)
    let this = shared.clone();
    window.on_mouse_event(move |ev: &MouseDownEvent, phase, window, cx| {
        if phase != gpui::DispatchPhase::Bubble || ev.button != MouseButton::Left {
            return;
        }
        if !bounds.contains(&ev.position) {
            // 点别处 = 上一轮拖选收场（鼠标在窗口外松开时收不到
            // MouseUp，锚点会残留）
            this.end_selection(cx);
            return;
        }
        this.start_selection(ev.position, bounds, ev.click_count, window, cx);
    });
    // drag 不要求命中 bounds：拖出终端区（顶/底）正是「越界自动
    // 滚动」的场景；「是不是在拖本次选区」由 sel_anchor 判定
    let this = shared.clone();
    window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, cx| {
        if phase != gpui::DispatchPhase::Bubble || ev.pressed_button != Some(MouseButton::Left) {
            return;
        }
        this.drag_selection(ev.position, bounds, window, cx);
    });
    // 松开左键 = 拖动结束：清锚点、保留选区。不清的话锚点会残留，
    // 之后在别处拖动会莫名改到终端选区
    let this = shared.clone();
    window.on_mouse_event(move |ev: &MouseUpEvent, phase, _window, cx| {
        if phase != gpui::DispatchPhase::Bubble || ev.button != MouseButton::Left {
            return;
        }
        this.end_selection(cx);
    });
    // right-click: terminal context menu (copy / paste / select
    // all) — 终端没有右键菜单的话，鼠标用户无从复制粘贴
    let this = shared.clone();
    window.on_mouse_event(move |ev: &MouseDownEvent, phase, window, cx| {
        if phase != gpui::DispatchPhase::Bubble
            || ev.button != MouseButton::Right
            || !bounds.contains(&ev.position)
        {
            return;
        }
        cx.stop_propagation();
        // 先聚焦终端：菜单动作作用于它，收起后键盘也继续可用
        window.focus(&this.focus);
        let (x, y) = (f32::from(ev.position.x), f32::from(ev.position.y));
        let tab_id = this.tab_id;
        let _ = this.weak.update(cx, |chat, cx| {
            chat.dlg.term_menu = Some((tab_id, x, y));
            cx.notify();
        });
    });
    let this = shared;
    window.on_mouse_event(move |ev: &ScrollWheelEvent, phase, window, cx| {
        if phase != gpui::DispatchPhase::Bubble || !bounds.contains(&ev.position) {
            return;
        }
        this.scroll(ev.delta, window, cx);
        cx.stop_propagation();
    });
}

/// Merge per-cell styles into gpui TextRuns (bytes length = utf8 len).
fn merge_runs(row: &SnapRow) -> Vec<gpui::TextRun> {
    let mut runs: Vec<gpui::TextRun> = Vec::new();
    for (ch, style) in row.text.chars().zip(&row.styles) {
        let len = ch.len_utf8();
        if let Some(last) = runs.last_mut() {
            let same = last.color == style.fg
                && last.background_color == style.bg
                && last.font.weight == font_weight(style.bold)
                && last.font.style == font_style(style.italic)
                && last.underline.is_some() == style.underline
                && last.strikethrough.is_some() == style.strike;
            if same {
                last.len += len;
                continue;
            }
        }
        runs.push(gpui::TextRun {
            len,
            font: cell_font(style),
            color: style.fg,
            background_color: style.bg,
            underline: style.underline.then(|| UnderlineStyle {
                thickness: px(1.),
                ..Default::default()
            }),
            strikethrough: style.strike.then(|| gpui::StrikethroughStyle {
                thickness: px(1.),
                ..Default::default()
            }),
        });
    }
    runs
}

fn font_weight(bold: bool) -> FontWeight {
    if bold {
        FontWeight::BOLD
    } else {
        FontWeight::NORMAL
    }
}

fn font_style(italic: bool) -> FontStyle {
    if italic {
        FontStyle::Italic
    } else {
        FontStyle::Normal
    }
}

pub(crate) fn cell_font(style: &CellStyle) -> Font {
    Font {
        family: FONT_FAMILY.into(),
        features: gpui::FontFeatures::disable_ligatures(),
        fallbacks: None,
        weight: font_weight(style.bold),
        style: font_style(style.italic),
    }
}
