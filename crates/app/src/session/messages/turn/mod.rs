//! 助手轮渲染：live_turn_body 流式尾窗、[`render_assistant_turn`] 轮装配
//! （组内条目计数/答案起点切分/模型标签）与 written_files 芯片。
//! 用户行件在 [`super::user_row`]（090 重构拆分）。

use super::user_row::{bar_hover_wired, render_block};
use super::*;
use std::collections::HashMap;

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb, rgba};
use pi_link::protocol::Block;

use crate::Chat;
use crate::appearance::theme;
use crate::i18n::tr;
use crate::session::actions_bar::{self};
use crate::session::fork::ForkAnchor;
use crate::ui::icon;
fn block_displayable(b: &Block) -> bool {
    match b {
        Block::Text { text, .. } | Block::Thinking { text, .. } => !text.trim().is_empty(),
        Block::ToolCall { name, .. } => !name.is_empty(),
        Block::Image { .. } => true,
    }
}

/// pi-web splitFinalAssistantBlocks：最终回答 = 尾部 text/image 连续段，
/// 返回首个 answer 块下标（无连续段时 == len）。
fn split_answer_start(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .rposition(|b| !matches!(b, Block::Text { .. } | Block::Image { .. }))
        .map_or(0, |i| i + 1)
}

/// pi-web hasFinalAssistantAnswer：存在非空 text 或 image 块。
fn msg_has_answer(m: &Msg) -> bool {
    m.blocks.iter().any(|b| match b {
        Block::Text { text, .. } => !text.trim().is_empty(),
        Block::Image { .. } => true,
        _ => false,
    })
}

/// 模型名行（pi-web MessageView model label）：模型名 + 流式期间的 ↓token
/// 估算 + t/s 徽章（四档配色同 pi-web：≥50 青 / ≥30 绿 / ≥15 黄 / 其余红）。
fn model_label_div(label: &str, est: Option<u64>, tps: Option<f32>, t: &theme::Theme) -> gpui::Div {
    // agent 名称 = 会话字号 -1（字体大小设置.md §2）
    let mut row = div()
        .text_size(crate::appearance::sess_size(-1.))
        .text_color(rgb(t.text_dim))
        .mb(px(4.))
        .flex()
        .items_center()
        .gap(px(6.))
        .child(SharedString::from(label.to_string()));
    if let Some(e) = est.filter(|e| *e > 0) {
        row = row.child(
            div()
                .flex()
                .items_center()
                .gap(px(2.))
                .text_color(rgb(t.text))
                .child(SharedString::from(format!(
                    "\u{2193} {}",
                    crate::services::format::fmt_thousand(e)
                ))),
        );
        if let Some(v) = tps {
            let bg = if v >= 50. {
                0x53b3cb
            } else if v >= 30. {
                0x9bc53d
            } else if v >= 15. {
                0xf9c22e
            } else {
                0xe01a4f
            };
            row = row.child(
                div()
                    .ml(px(6.))
                    .px(px(6.))
                    .py(px(1.))
                    .rounded(px(4.))
                    .bg(gpui::rgb(bg))
                    .text_size(crate::appearance::sess_size(-1.))
                    .text_color(rgb(0xffffff))
                    .child(SharedString::from(format!("{v:.1} t/s"))),
            );
        }
    }
    row
}

/// 流式平铺（pi-web `isLiveTail`）条数上限。长自主跑的单轮可达数百条
/// assistant 消息，而**整轮是一个 List 条目**——平铺全部意味着每帧 O(轮)
/// 地建元素、排版、命中测试（几百张工具卡还要逐帧 serde 解析 args），流式
/// 每 delta 一帧，打字/滚轮全被拖死（688 条消息会话实测明显卡顿）。视口
/// 本来只装得下尾部几条；更早的内容轮末整形后进「工作详情」组照样可展开。
const LIVE_TAIL_MAX_MSGS: usize = 12;

/// pi-web `isLiveTail` 的平铺渲染：运行中的这一轮**不分组、不折叠**——每条
/// assistant 消息一个模型名行 + 全部块（thinking/toolCall/text 就地展开），
/// 流式那条的模型名行带 ↓token 估算 + t/s 徽章。
///
/// 「工作详情」折叠行与最终回答分区只在轮末整形后生成；usage 行/复制栏也照
/// pi-web MessageView 的 `!isStreaming` 规则留给轮末（见 `render_assistant_turn`）。
#[allow(clippy::too_many_arguments)]
fn live_turn_body(
    turn: &[&Msg],
    turn_ixs: &[usize],
    weak: &gpui::WeakEntity<Chat>,
    collapsed: &HashMap<(usize, usize), bool>,
    t: &theme::Theme,
    model_label: &str,
    est: Option<u64>,
    tps: Option<f32>,
) -> gpui::Div {
    let mut body = div().flex().flex_col();
    // 巨轮限流：只平铺尾部 LIVE_TAIL_MAX_MSGS 条，前面补一行占位说明
    //（点击定位走右缘导航刻度；展开回看等轮末「工作详情」）
    let total = turn.len();
    let skip = total.saturating_sub(LIVE_TAIL_MAX_MSGS);
    if skip > 0 {
        body = body.child(
            div()
                .mb(px(8.))
                .text_size(crate::appearance::sess_size(-1.))
                .text_color(rgb(t.text_faint))
                .child(SharedString::from(crate::i18n::tf(
                    "nav.live_tail_skipped",
                    &[("n", skip.to_string())],
                ))),
        );
    }
    for (i, (m, &gix)) in turn[skip..].iter().zip(&turn_ixs[skip..]).enumerate() {
        if m.role != Role::Assistant {
            continue;
        }
        let disp: Vec<&Block> = m.blocks.iter().filter(|b| block_displayable(b)).collect();
        if disp.is_empty() {
            continue;
        }
        // 轮内最后一条 = 正在流式输出/干活的那条（徽章与 streaming 态挂它）
        let streaming = skip + i == total - 1;
        let mut item = div().mb(px(16.)).flex().flex_col().gap(px(8.));
        item = item.child(model_label_div(
            m.model.as_deref().unwrap_or(model_label),
            if streaming { est } else { None },
            if streaming { tps } else { None },
            t,
        ));
        // pi-web thinkingDurationFromFile：该消息的生成时长挂到 thinking 块
        let msg_think_dur = m.end_ts.zip(m.ts).map(|(e, s)| (e - s).max(0) / 1000);
        for b in disp {
            item = item.child(render_block(
                b,
                gix,
                weak,
                collapsed,
                t,
                streaming,
                msg_think_dur,
                m.ts,
            ));
        }
        body = body.child(item);
    }
    body
}

pub(crate) mod render;
pub(crate) use render::render_assistant_turn;
