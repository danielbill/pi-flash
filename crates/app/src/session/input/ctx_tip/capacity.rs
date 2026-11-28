//! 上下文容量块：容量环形/文本、token·成本明细与压缩按钮。

use super::*;

/// 上下文容量块（ZCode 风格）：标题行（已用/窗口 · 百分比，窗口未知只给
/// 已用）+ 进度条 + 分类占比行（系统态已加载且官方总量已知才画；否则退
/// 回旧两行）。标题/进度条数据源 = `actual`（provider 实际口径：最后一条
/// assistant 响应的真实 prompt，见 `SessionRuntime::context_actual`）优先，
/// 无响应回落 pi 树投影（stats）——树口径在后台折叠（插件 003 请求改写
/// 层剔除原文）后不回落，实际口径与请求真实大小对齐且不依赖插件；分类
/// 构成行恒为树口径估算；退回的两行明细（使用比例/上下文 token）与标题
/// 行同口径，避免同屏两口径互相矛盾。
pub(super) fn ctx_capacity_block(
    s: &pi_link::protocol::SessionStats,
    sys: Option<&pi_link::transcript::TranscriptSystem>,
    t: &'static crate::appearance::theme::Theme,
    actual: Option<u64>,
) -> gpui::Div {
    let (ctx_total, ctx_pct) = match actual {
        Some(tok) => (
            tok,
            s.context_window
                .filter(|w| *w > 0)
                .map(|w| tok as f64 / w as f64 * 100.),
        ),
        None => (
            s.context_tokens.unwrap_or(0),
            s.context_percent
                .or_else(|| match (s.context_tokens, s.context_window) {
                    (Some(tk), Some(w)) if w > 0 => Some(tk as f64 / w as f64 * 100.),
                    _ => None,
                }),
        ),
    };
    let bar_pct = ctx_pct; // actual 与 fallback 两分支都已算好
    // 右值：末字用 ASCII 半角字符（`%` 与 `)`），全角 `）` 自带半个字身的
    // 右空白，会让首行墨迹比下面各行的右缘内缩 ~6px（2026-10-16 用户指出的
    // 「最上面一行右侧没对齐」）；`/` 两侧同样不留空格。数字直接引用
    // （bd pi-flash-sdja 定稿：actual/内核压缩后估算/树投影，不加 ≈）。
    let head_right = match s.context_window {
        Some(w) if ctx_total > 0 && w > 0 => format!(
            "{}/{} ({})",
            fmt_k(ctx_total),
            fmt_k(w),
            bar_pct
                .map(|v| format!("{v:.1}%"))
                .unwrap_or_else(|| "\u{2014}".into()),
        ),
        _ if ctx_total > 0 => format!("{}", fmt_k(ctx_total)),
        _ => "\u{2014}".into(),
    };
    let mut ctx = div()
        .flex()
        .flex_col()
        .gap(px(8.))
        // 标题行走与下面各行同款的两列网格（右值同半宽、右缘同一列线，
        // 免得首行右缘与占比行差半格）
        .child(ctx_grid2(vec![
            div()
                .text_size(crate::appearance::ui_size(12.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text))
                .child(tr("stats.context_capacity")),
            div()
                .flex()
                .justify_end()
                .text_size(crate::appearance::ui_size(12.))
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(head_right)),
        ]));
    // 进度条：track = 边框色 ~1/3 透明度，fill = accent，宽 = 占窗口比
    if let Some(p) = bar_pct {
        ctx = ctx.child(
            div()
                .w_full()
                .h(px(6.))
                .rounded(px(3.))
                .bg(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x55)))
                .overflow_hidden()
                .child(
                    div()
                        .h_full()
                        .w(relative(p.clamp(0., 100.) as f32 / 100.))
                        .rounded(px(3.))
                        .bg(rgb(t.accent)),
                ),
        );
    }
    // 分类占比行（恒为树口径估算——构成按会话树投影统计；系统态已加载
    // 且官方总量已知才画，否则退回旧两行）
    let cats = sys
        .filter(|_| ctx_total > 0)
        .map(|sys| cat_rows(sys, ctx_total));
    match cats {
        Some(rows) if !rows.is_empty() => {
            ctx = ctx.child(ctx_grid2(
                rows.into_iter()
                    .map(|(label, color, v)| {
                        div()
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .size(px(8.))
                                    .rounded_full()
                                    .flex_shrink_0()
                                    // 色点↔文字 5px：显式点距（不走 flex gap，
                                    // 用户定夺 2026-10-13）
                                    .mr(px(5.))
                                    .bg(rgb(color)),
                            )
                            .child(
                                div()
                                    .text_size(crate::appearance::ui_size(12.))
                                    .text_color(rgb(t.text_dim))
                                    .child(SharedString::from(label)),
                            )
                            .child(
                                div()
                                    .ml_auto()
                                    .text_size(crate::appearance::ui_size(12.))
                                    .font_family(crate::editor::markdown::MONO_FAMILY)
                                    .text_color(rgb(t.text))
                                    .child(SharedString::from(cat_pct(v, ctx_total))),
                            )
                    })
                    .collect(),
            ));
        }
        _ => {
            // 明细两行跟标题行同一口径（ctx_total/ctx_pct 开头已按实际
            // 口径优先算好；两口径同屏互相矛盾——顶部 77K/160K (48.3%)
            // vs 明细 69K/1,000K (6.9%)——用户 2026-11-27 指出「状态还没
            // 对上」）。
            let tokens_text = match s.context_window {
                Some(w) if ctx_total > 0 && w > 0 => format!("{}/{}", fmt_k(ctx_total), fmt_k(w)),
                _ if ctx_total > 0 => fmt_k(ctx_total),
                _ => "\u{2014}".into(),
            };
            ctx = ctx.children(vec![
                ctx_row(
                    tr("stats.share"),
                    ctx_pct
                        .map(|p| format!("{p:.1}%"))
                        .unwrap_or_else(|| "\u{2014}".into()),
                    t,
                ),
                ctx_row(tr("stats.context_tokens"), tokens_text, t),
            ]);
        }
    }
    ctx
}

/// Token 分项二列网格，token 值统一 K 口径（用户定夺）；缓存命中率收进
/// 网格末格，正好补齐「总计」右侧的空位（用户定夺）+ 费用小节。
pub(super) fn ctx_tokens_cost_parts(
    s: &pi_link::protocol::SessionStats,
    t: &'static crate::appearance::theme::Theme,
) -> Vec<gpui::AnyElement> {
    let token_grid = ctx_grid2(vec![
        ctx_row(tr("stats.input"), fmt_k(s.input), t),
        ctx_row(tr("stats.output"), fmt_k(s.output), t),
        ctx_row(tr("stats.cache_read"), fmt_k(s.cache_read), t),
        ctx_row(tr("stats.cache_write"), fmt_k(s.cache_write), t),
        ctx_row(tr("tokens.total"), fmt_k(s.tokens_total), t),
        // 缓存命中率：用户最关心的数字，值走主题 accent 强调（用户定夺）
        div()
            .flex()
            .items_center()
            .child(
                div()
                    .text_size(crate::appearance::ui_size(12.))
                    .text_color(rgb(t.text_dim))
                    .child(tr("stats.cache_hit_rate")),
            )
            .child(
                div()
                    .ml_auto()
                    .text_size(crate::appearance::ui_size(12.))
                    .font_family(crate::editor::markdown::MONO_FAMILY)
                    .text_color(rgb(t.accent))
                    .child(SharedString::from(
                        s.cache_hit_rate()
                            .map(|r| format!("{:.1}%", r * 100.))
                            .unwrap_or_else(|| "\u{2014}".into()),
                    )),
            ),
    ]);
    let cost_rows =
        vec![ctx_row(tr("stats.total_cost"), format!("${:.4}", s.cost), t).into_any_element()];
    vec![
        ctx_section(tr("stats.tokens_total"), vec![token_grid], t),
        ctx_section(tr("stats.cost"), cost_rows, t),
    ]
}
