//! 系统提示词面板（SystemPromptPanel.tsx parity；自 top_panels.rs 外移）：
//! 顶部 token 分析块 + 声明块列表。

use gpui::{SharedString, div, prelude::*, px, relative, rgb};

use super::empty_state;
use crate::Chat;
use crate::appearance::theme::Theme;
use crate::i18n::{tf, tr};

/// SystemPromptPanel.tsx + token 分析块：顶部固定分析块（高 100px，不随内容
/// 滚动，用户定稿）——总计 + 7 大类「名称 token数 比例」8 个信息块一行排布 +
/// 100% 宽比例长条；下方滚动区等宽 12px、行高 1.6、muted、pre-wrap。psp 常显
/// 滚动条只包滚动区（不延展进分析块）；`scroll` 挂在 Chat 上（跨渲染持久）。
pub(super) fn system_prompt_panel(
    sys: Option<&pi_link::transcript::TranscriptSystem>,
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    scroll: &gpui::ScrollHandle,
    t: &Theme,
) -> gpui::AnyElement {
    // 正文按分类分段渲染：每段整块铺类别色淡背景（~15% alpha，用户定稿：
    // 不要竖条），段间 10px —— 与分析块/长条的类别色一一对应。有声明的桶
    // （系统工具/插件/MCP）在其文本下方追加折叠的「调用声明」块
    let body = match sys {
        Some(s) if !s.prompt.is_empty() => {
            let agent_dir = pi_link::paths::pi_agent_dir();
            let segs = pi_link::transcript::segments(s, agent_dir.as_deref());
            let decls = pi_link::transcript::declarations(s);
            let n = segs.len();
            div()
                .flex()
                .flex_col()
                .children(
                    segs.into_iter()
                        .enumerate()
                        .map(move |(i, (bucket, text))| {
                            let color = crate::appearance::theme::bucket_color(bucket);
                            let bucket_decl =
                                decls.iter().find(|(b, _)| *b == bucket).map(|(_, v)| v);
                            let idx = pi_link::transcript::SYSTEM_BUCKETS
                                .iter()
                                .position(|x| *x == bucket);
                            let mut seg = div()
                                .w_full()
                                .rounded(px(4.))
                                .bg(gpui::rgba((color << 8) | 0x26))
                                .px(px(8.))
                                .py(px(5.))
                                .mb(if i + 1 < n { px(10.) } else { px(0.) })
                                .font_family(crate::editor::markdown::MONO_FAMILY)
                                .text_size(crate::appearance::ui_size(12.))
                                .line_height(relative(1.6))
                                .text_color(rgb(t.text_muted))
                                .child(SharedString::from(text));
                            if let (Some(decl_list), Some(idx)) = (bucket_decl, idx) {
                                seg = seg.child(decl_block(
                                    idx,
                                    decl_list,
                                    chat.aux.decl_open[idx],
                                    weak,
                                    t,
                                ));
                            }
                            seg.into_any_element()
                        }),
                )
                .into_any_element()
        }
        // prompt === "" → 空提示词（工具已禁用）；未加载 → 尚未加载
        Some(_) => empty_state(tr("system_prompt.empty_disabled"), t, false),
        None => empty_state(tr("system_prompt.loading"), t, false),
    };
    // 分析块：加载且有内容才画
    let mut col = div().flex_1().min_w_0().min_h_0().flex().flex_col();
    if let Some(s) = sys.filter(|s| !s.prompt.is_empty() || !s.tools.is_empty()) {
        col = col.child(stats_block(s, t));
    }
    // 滚动条只属于滚动区：relative 包裹 + 滚动条绝对定位贴右缘
    // （actions_menu / vlist 同款结构），不得延展进上面的统计面板
    col.child(
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .relative()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("sys-prompt-scroll")
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(scroll)
                    // 内边距对齐设置弹窗 body（mc-body：pl30 / pr30 / pb30）
                    .pt(px(14.))
                    .pb(px(30.))
                    .px(px(30.))
                    .child(body),
            )
            .child(
                div()
                    .absolute()
                    .top(px(2.))
                    .bottom(px(2.))
                    .right(px(0.))
                    .w(px(10.))
                    .child(crate::ui::psp_scrollbar::menu_scrollbar(scroll)),
            ),
    )
    .into_any_element()
}

/// token 分析块（固定 100px，不参与滚动）：一行 8 个信息块（总计 + 7 类，
/// 每格 1/8 宽，块 = 色点+名称 / token 数+比例）+ 底部 100% 宽比例长条。
fn stats_block(sys: &pi_link::transcript::TranscriptSystem, t: &Theme) -> gpui::AnyElement {
    let agent_dir = pi_link::paths::pi_agent_dir();
    let tokens = pi_link::transcript::breakdown(sys, agent_dir.as_deref());
    let total: u64 = tokens.iter().sum();
    let mut row = vec![stat_cell(
        tr("tokens.total"),
        total,
        total,
        t.accent,
        t,
        true,
    )];
    for (i, bucket) in pi_link::transcript::SYSTEM_BUCKETS.iter().enumerate() {
        row.push(stat_cell(
            bucket_label(*bucket),
            tokens[i],
            total,
            crate::appearance::theme::bucket_color(*bucket),
            t,
            false,
        ));
    }

    div()
        .h(px(100.))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .justify_between()
        .pt(px(14.))
        .pb(px(12.))
        .px(px(30.))
        .child(div().flex().children(row))
        .child(ratio_bar(&tokens, total))
        .into_any_element()
}

/// 「调用声明」折叠块：挂在所属类别的文本段下方，颜色服从类别，整体字号
/// 比正文小 2px（12→10）。默认折叠只留标题行（▸ 调用声明 · N 条 · x tokens），
/// 点击展开逐条列 name + description（声明就是随 API `tools` 字段进上下文的
/// 那部分，展开后的体量 ≈ 统计里该桶的声明 token）。
fn decl_block(
    idx: usize,
    decls: &[pi_link::transcript::ToolDecl],
    open: bool,
    weak: &gpui::WeakEntity<Chat>,
    t: &Theme,
) -> gpui::AnyElement {
    let color = crate::appearance::theme::bucket_color(pi_link::transcript::SYSTEM_BUCKETS[idx]);
    let tokens = pi_link::estimate::estimate_tokens(&pi_link::transcript::tools_wire_text(decls));
    let title = format!(
        "{} {} · {} · {} tokens",
        if open { "▾" } else { "▸" },
        tr("tool_defs.tool_declarations"),
        tf("tool_defs.items", &[("n", decls.len().to_string())]),
        crate::services::format::fmt_thousand(tokens),
    );
    let weak_toggle = weak.clone();
    let mut block = div()
        .mt(px(8.))
        .pt(px(6.))
        .border_t_1()
        .border_color(gpui::rgba((color << 8) | 0x55))
        .child(
            div()
                .id(SharedString::from(format!("decl-toggle-{idx}")))
                .flex()
                .items_center()
                .rounded(px(3.))
                .px(px(4.))
                .py(px(2.))
                .cursor_pointer()
                .text_size(crate::appearance::ui_size(10.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(color))
                .hover(|s| s.bg(gpui::rgba((color << 8) | 0x1e)))
                .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                    let _ = weak_toggle.update(cx, |c, cx| {
                        c.aux.decl_open[idx] = !c.aux.decl_open[idx];
                        cx.notify();
                    });
                })
                .child(SharedString::from(title)),
        );
    if open {
        block = block.child(div().mt(px(2.)).flex().flex_col().gap(px(6.)).children(
            decls.iter().map(|d| {
                div()
                    .flex()
                    .flex_col()
                    .text_size(crate::appearance::ui_size(10.))
                    .font_family(crate::editor::markdown::MONO_FAMILY)
                    .line_height(relative(1.5))
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(t.text))
                            .child(SharedString::from(d.name.clone())),
                    )
                    .child(
                        div()
                            .text_color(rgb(t.text_muted))
                            .child(SharedString::from(d.description.clone())),
                    )
                    .into_any_element()
            }),
        ));
    }
    block.into_any_element()
}

/// 一个信息块：色点 + 名称（上）/ token 数 · 比例（下）。一行 8 块，各占 1/8 宽。
fn stat_cell(
    label: &str,
    value: u64,
    total: u64,
    color: u32,
    t: &Theme,
    is_total: bool,
) -> gpui::AnyElement {
    div()
        .w(relative(0.125))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            div()
                .size(px(8.))
                .rounded(px(2.))
                .flex_shrink_0()
                .bg(rgb(color)),
        )
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(crate::appearance::ui_size(10.))
                        .text_color(rgb(t.text_dim))
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .child(SharedString::from(label.to_string())),
                )
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(crate::appearance::ui_size(12.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgb(if is_total { t.text } else { t.text_muted }))
                                .child(SharedString::from(crate::services::format::fmt_thousand(
                                    value,
                                ))),
                        )
                        .child(
                            div()
                                .text_size(crate::appearance::ui_size(10.))
                                .text_color(rgb(t.text_dim))
                                .whitespace_nowrap()
                                .child(SharedString::from(fmt_pct(value, total))),
                        ),
                ),
        )
        .into_any_element()
}

/// 100% 宽比例长条：段宽 = 占比，零桶跳过；h6 圆角，段间 1px 缝。
fn ratio_bar(tokens: &[u64; 7], total: u64) -> gpui::AnyElement {
    if total == 0 {
        return div().into_any_element();
    }
    div()
        .w_full()
        .h(px(6.))
        .rounded(px(3.))
        .overflow_hidden()
        .flex()
        .flex_row()
        .children(
            tokens
                .iter()
                .enumerate()
                .filter(|(_, v)| **v > 0)
                .map(|(i, v)| {
                    div()
                        .w(relative(*v as f32 / total as f32))
                        .h_full()
                        .bg(rgb(crate::appearance::theme::bucket_color(
                            pi_link::transcript::SYSTEM_BUCKETS[i],
                        )))
                        .when(i + 1 < tokens.len(), |d| d.mr(px(1.)))
                        .into_any_element()
                }),
        )
        .into_any_element()
}

/// 比例文案：>=10% 取整数，<10% 留一位小数。
fn fmt_pct(value: u64, total: u64) -> String {
    if total == 0 {
        return "0%".into();
    }
    let p = value as f64 / total as f64 * 100.0;
    if p >= 9.95 {
        format!("{:.0}%", p)
    } else {
        format!("{:.1}%", p)
    }
}

/// 分析块类别名（i18n）。ctx_tip 用量弹层的分类行同源复用。
pub(crate) fn bucket_label(b: pi_link::transcript::SystemBucket) -> &'static str {
    use pi_link::transcript::SystemBucket::*;
    match b {
        GlobalPrompt => tr("tokens.global_prompt"),
        SystemTools => tr("tokens.system_tools"),
        Skills => tr("settings.tab.skills"),
        Plugins => tr("settings.tab.extensions"),
        Mcp => tr("dock.mcp"),
        ProjectPrompt => tr("tokens.project_prompt"),
        Other => tr("dock.other"),
    }
}
