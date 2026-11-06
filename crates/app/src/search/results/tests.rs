use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled, TestAppContext,
    VisualTestContext, Window, div, point, px, size,
};

use super::{ResultBlock, results_shell};
use crate::search::ResultLine;

fn sample_blocks() -> Vec<ResultBlock> {
    vec![
        // 内容命中：两段摘录（行号是文件真值）+ 一行超长文本（验证折行）
        ResultBlock {
            rel: "docs/note.md".into(),
            path_only: false,
            count: 2,
            segments: vec![
                vec![
                    ResultLine {
                        line: 10,
                        text: "# 标题".into(),
                        spans: Vec::new(),
                        hit: false,
                        ordinal: None,
                    },
                    ResultLine {
                        line: 11,
                        text: "命中 openai 关键字".into(),
                        spans: vec![3..9],
                        hit: true,
                        ordinal: Some(0),
                    },
                    ResultLine {
                        line: 12,
                        text: "超长行 ".repeat(60),
                        spans: Vec::new(),
                        hit: false,
                        ordinal: None,
                    },
                ],
                vec![ResultLine {
                    line: 30,
                    text: "另一段 openai".into(),
                    spans: vec![3..9],
                    hit: true,
                    ordinal: Some(1),
                }],
            ],
        },
        // 仅文件名命中（文件栏）
        ResultBlock {
            rel: "src/openai_client.rs".into(),
            path_only: true,
            count: 0,
            segments: Vec::new(),
        },
    ]
}

/// 记下本帧真正构建了哪些块 + 块高（列表有高度才会构建 → 布局探针）。
struct TestView {
    seen: Rc<RefCell<Vec<usize>>>,
    heights: Rc<RefCell<Vec<(f32, f32)>>>,
    list: gpui::ListState,
    blocks: Rc<Vec<ResultBlock>>,
    collapsed: bool,
}

impl Render for TestView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let seen = self.seen.clone();
        let heights = self.heights.clone();
        let blocks = self.blocks.clone();
        let collapsed = self.collapsed;
        let on_open: super::OnOpen =
            Rc::new(|_: usize, _: u32, _: Option<usize>, _: &mut gpui::App| {});
        let on_toggle: super::OnToggle = Rc::new(|_: usize, _: &mut gpui::App| {});
        // 外层 = 搜索面板内容列（定高，模拟 content 区）
        div()
            .w(px(600.))
            .h(px(240.))
            .flex()
            .flex_col()
            .child(results_shell(
                crate::appearance::theme::theme(),
                &self.list,
                &gpui_component::scroll::ScrollbarState::default(),
                gpui::list(self.list.clone(), move |ix, _window, _cx| {
                    seen.borrow_mut().push(ix);
                    match blocks.get(ix) {
                        Some(block) => {
                            let el = super::render_block(
                                ix,
                                block,
                                None,
                                &on_open,
                                &on_toggle,
                                collapsed,
                                super::Gutter {
                                    num: 40.,
                                    left: 9.,
                                    right: 9.,
                                },
                            );
                            div()
                                .w_full()
                                .on_children_prepainted({
                                    let heights = heights.clone();
                                    move |bounds, _, _| {
                                        if let Some(b) = bounds.first() {
                                            heights.borrow_mut().push((
                                                f32::from(b.origin.y),
                                                f32::from(b.size.height),
                                            ));
                                        }
                                    }
                                })
                                .child(el)
                                .into_any_element()
                        }
                        None => div().into_any_element(),
                    }
                })
                .flex_1()
                .min_h_0(),
            ))
    }
}

fn frame(
    cx: &mut VisualTestContext,
    seen: &Rc<RefCell<Vec<usize>>>,
    heights: &Rc<RefCell<Vec<(f32, f32)>>>,
) -> Vec<usize> {
    frame_with(cx, seen, heights, sample_blocks(), false)
}

fn frame_with(
    cx: &mut VisualTestContext,
    seen: &Rc<RefCell<Vec<usize>>>,
    heights: &Rc<RefCell<Vec<(f32, f32)>>>,
    blocks: Vec<ResultBlock>,
    collapsed: bool,
) -> Vec<usize> {
    seen.borrow_mut().clear();
    heights.borrow_mut().clear();
    let blocks = Rc::new(blocks);
    cx.draw::<Entity<TestView>>(point(px(0.), px(0.)), size(px(600.), px(240.)), |_, cx| {
        cx.new(|_| TestView {
            seen: seen.clone(),
            heights: heights.clone(),
            list: gpui::ListState::new(blocks.len(), gpui::ListAlignment::Top, px(1000.)),
            blocks,
            collapsed,
        })
    });
    let mut rows = seen.borrow().clone();
    rows.sort_unstable();
    rows.dedup();
    rows
}

/// 回归锁（2026-10-21「只显示计数不显示内容」）：结果区容器必须把高度
/// 传给列表，且真实块渲染出来必须有像素高度（0 高 = 用户看到的空白）。
#[gpui::test]
fn real_blocks_render_with_height(cx: &mut TestAppContext) {
    let window = cx.add_empty_window();
    let seen: Rc<RefCell<Vec<usize>>> = Default::default();
    let heights: Rc<RefCell<Vec<(f32, f32)>>> = Default::default();
    let rows = frame(window, &seen, &heights);
    assert_eq!(rows, vec![0, 1], "两个结果块都必须被构建");
    let hs = heights.borrow().clone();
    assert!(!hs.is_empty(), "块必须完成一次 prepaint（有实际 bounds）");
    assert!(
        hs.iter().all(|(_, h)| *h > 0.),
        "块高度必须 > 0，实测 {hs:?}（0 = 结果区空白）"
    );
}

/// 收起锁：收起后该块只剩文件头（高度回落到 `header_h()`），内容行不再
/// 参与布局——「点收展钮 → 该文件查询内容收起」的行为契约。
#[gpui::test]
fn collapsed_block_keeps_only_header(cx: &mut TestAppContext) {
    let window = cx.add_empty_window();
    let seen: Rc<RefCell<Vec<usize>>> = Default::default();
    let heights: Rc<RefCell<Vec<(f32, f32)>>> = Default::default();
    let only_first = vec![sample_blocks().remove(0)];

    frame_with(&mut *window, &seen, &heights, only_first.clone(), false);
    let expanded = heights.borrow()[0].1;

    frame_with(&mut *window, &seen, &heights, only_first, true);
    let collapsed = heights.borrow()[0].1;

    assert!(
        (collapsed - super::header_h()).abs() < 2.0,
        "收起后块高应等于文件头高 {}，实测 {collapsed}",
        super::header_h()
    );
    assert!(
        collapsed < expanded - 10.0,
        "收起必须比展开矮（{collapsed} vs {expanded}）"
    );
}

/// 间隙锁（2026-10-21 用户问「间隙呢？？」）：Zed 的间隙**来自块内**
/// `BUFFER_HEADER_PADDING`（4px）而不是列表 gap——所以块在列表里必须紧贴
/// （gap 0），块高 = 2 行，条上下各留 4px ⇒ 相邻两条之间可视间隙 8px、
/// 条与自己代码行之间 4px。这里把这三条一起锁住。
#[gpui::test]
fn adjacent_headers_keep_eight_px_gap(cx: &mut TestAppContext) {
    let window = cx.add_empty_window();
    let seen: Rc<RefCell<Vec<usize>>> = Default::default();
    let heights: Rc<RefCell<Vec<(f32, f32)>>> = Default::default();
    let blocks: Vec<ResultBlock> = sample_blocks()
        .into_iter()
        .map(|mut b| {
            b.segments.clear();
            b
        })
        .collect();

    frame_with(&mut *window, &seen, &heights, blocks, true);
    let hs = heights.borrow().clone();
    assert_eq!(hs.len(), 2, "两个块都要有 bounds，实测 {hs:?}");
    let (top0, h0) = hs[0];
    let (top1, _) = hs[1];
    let item_gap = top1 - (top0 + h0);
    assert!(
        item_gap.abs() < 0.6,
        "列表项之间必须紧贴（间隙来自块内留白），实测 {item_gap}"
    );
    assert!(
        (h0 - super::header_h()).abs() < 0.6,
        "折叠块高应 = 2 行（{}），实测 {h0}",
        super::header_h()
    );
    // 块高 = 2 行、条四周 4px → 条与条的可视间隙 = 8px
    assert_eq!(super::header_h(), (super::line_h() * 2.).round());
    assert_eq!(super::HEADER_PAD, 4.);
    assert_eq!(super::HEADER_PAD * 2., 8.);
}

/// 行号列宽锁（2026-10-21 用户「line num 列不够宽，Zed 是动态调整」）：
/// 宽度 = `max(位数, 4) × ch_advance + 2 × ch_width`（Zed
/// `gutter_dimensions` + 默认 `min_line_number_digits = 4`）。
#[gpui::test]
fn gutter_width_follows_widest_line_number(cx: &mut TestAppContext) {
    fn block_with_line(line: u32) -> Vec<ResultBlock> {
        vec![ResultBlock {
            rel: "a.md".into(),
            path_only: false,
            count: 1,
            segments: vec![vec![ResultLine {
                line,
                text: "x".into(),
                spans: vec![0..1],
                hit: true,
                ordinal: Some(0),
            }]],
        }]
    }
    let (three, four, five) = cx.read(|cx| {
        (
            super::gutter_for(&block_with_line(137), cx),
            super::gutter_for(&block_with_line(2219), cx),
            super::gutter_for(&block_with_line(22198), cx),
        )
    });
    assert_eq!(
        three.num, four.num,
        "不足 min_line_number_digits(4) 位按 4 位留宽，不随位数抖动"
    );
    let step = five.num - four.num;
    assert!(
        step > 1.0,
        "5 位行号必须比 4 位更宽（实测 +{step}），否则 22198 会折行"
    );
    // 数字区够放 5 个字符：5 位时 num ≥ 5 × (step 即一个字符的推进宽)
    assert!(
        five.num >= step * 5.0 - 2.0,
        "5 位数要放得下 5 个字符（num={} step={step}）",
        five.num
    );
    assert!(three.left > 0.0 && three.right > 0.0, "左右各留 1ch");
    assert_eq!(super::vidx_digits(u32::MAX), 10);
    assert_eq!(super::vidx_digits(1), 1);
}

/// 命中区间必须进 highlight（Zed 的命中底色），且 run 不重叠、按序。
#[gpui::test]
fn match_spans_become_highlights(_cx: &mut TestAppContext) {
    let t = crate::appearance::theme::theme();
    let line = "abc openai def";
    let hs = super::match_highlights(line, &[4..10], t);
    assert!(!hs.is_empty());
    let mut last_end = 0usize;
    for (range, _) in &hs {
        assert!(range.start >= last_end, "run 必须按序不重叠: {hs:?}");
        assert!(range.start < range.end);
        last_end = range.end;
    }
    assert_eq!(hs.last().map(|(r, _)| r.end), Some(line.len()));
    let hit = hs
        .iter()
        .find(|(r, _)| r.start == 4)
        .expect("命中区间必须单独成 run");
    assert!(hit.1.background_color.is_some(), "命中 run 必须带底色");
}
