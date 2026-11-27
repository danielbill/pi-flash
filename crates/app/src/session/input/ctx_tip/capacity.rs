//! 上下文容量块：容量环形/文本、token·成本明细与压缩按钮。

use super::*;

/// 上下文容量块（ZCode 风格）：标题行（已用/窗口 · 百分比，窗口未知只给
/// 已用）+ 进度条 + 分类占比行（系统态已加载且官方总量已知才画；否则退
/// 回旧两行）。`bac_ctx` = 插件 003 的请求口径 override（tokens, window）：
/// 后台折叠剔除发生在请求改写层而原文留在树里，pi 的树口径永不回落——
/// 插件在场时标题/进度条换用请求口径（分母 = 插件判定窗口，与插件折叠
/// 判定同源）；分类构成行按树口径统计，与 override 总量不同基，退回
/// 旧两行；退回的两行明细（使用比例/上下文 token）也跟容量环同口径，
/// 避免两口径同屏互相矛盾（2026-11-27 用户实测指出）。
pub(super) fn ctx_capacity_block(
    s: &pi_link::protocol::SessionStats,
    sys: Option<&pi_link::transcript::TranscriptSystem>,
    t: &'static crate::appearance::theme::Theme,
    bac_ctx: Option<(u64, u64)>,
) -> gpui::Div {
    let (ctx_total, ctx_pct) = match bac_ctx {
        Some((tok, win)) if win > 0 => (tok, Some(tok as f64 / win as f64 * 100.)),
        _ => (
            s.context_tokens.unwrap_or(0),
            s.context_percent
                .or_else(|| match (s.context_tokens, s.context_window) {
                    (Some(tk), Some(w)) if w > 0 => Some(tk as f64 / w as f64 * 100.),
                    _ => None,
                }),
        ),
    };
    let bar_pct = ctx_pct; // override 与 fallback 两分支都已算好
    // 右值：末字用 ASCII 半角字符（`%` 与 `)`），全角 `）` 自带半个字身的
    // 右空白，会让首行墨迹比下面各行的右缘内缩 ~6px（2026-10-16 用户指出的
    // 「最上面一行右侧没对齐」）；`/` 两侧同样不留空格。
    let head_right = match bac_ctx {
        // override：窗口由解析保证 > 0，标题恒为 已用/窗口 (pct)
        Some((tk, w)) => format!(
            "{}/{} ({})",
            fmt_k(tk),
            fmt_k(w),
            bar_pct
                .map(|v| format!("{v:.1}%"))
                .unwrap_or_else(|| "\u{2014}".into()),
        ),
        None => match (s.context_tokens, s.context_window) {
            (Some(tk), Some(w)) if w > 0 => format!(
                "{}/{} ({})",
                fmt_k(tk),
                fmt_k(w),
                bar_pct
                    .map(|v| format!("{v:.1}%"))
                    .unwrap_or_else(|| "\u{2014}".into()),
            ),
            (Some(tk), None) => fmt_k(tk),
            _ => "\u{2014}".into(),
        },
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
    // 分类占比行（bac override 时退回——构成按树口径，与请求口径不同基；
    // 系统态已加载且官方总量已知才画，否则退回旧两行）
    let cats = sys
        .filter(|_| bac_ctx.is_none() && ctx_total > 0)
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
            // 明细两行跟容量环同一口径：override 在场时用请求口径
            // （ctx_total/ctx_pct 开头已算好；实测两口径同屏互相矛盾——
            // 顶部 77K/160K (48.3%) vs 明细 69K/1,000K (6.9%)，用户
            // 2026-11-27 指出「状态还没对上」），树口径明细只在无
            // override 时展示。
            let tokens_text = match bac_ctx {
                Some((tok, win)) if win > 0 => format!("{}/{}", fmt_k(tok), fmt_k(win)),
                _ => match (s.context_tokens, s.context_window) {
                    (Some(tk), Some(w)) => format!("{}/{}", fmt_k(tk), fmt_k(w)),
                    (Some(tk), None) => fmt_k(tk),
                    _ => "\u{2014}".into(),
                },
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
