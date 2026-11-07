//! 面板行渲染：panel_row、统计段、状态标记与时长/token 文案。

use super::*;

/// 第三行状态点色（会话运行结果青红点）：运行 accent、完成 NOTICE、
/// 异常/中止/轮次用尽 danger。
pub(super) fn row_dot_color(row: &CatalogRow) -> u32 {
    let t = T();
    if row.in_flight() {
        t.accent
    } else if row.status_kind() == StatusKind::Done {
        crate::appearance::theme::NOTICE
    } else {
        t.danger
    }
}

pub(super) fn panel_row(weak: &gpui::WeakEntity<Chat>, row: &CatalogRow) -> gpui::AnyElement {
    let t = T();
    let kind = row.status_kind();
    let mark_c = mark_color(row);
    let name = row.profile.clone().unwrap_or_else(|| row.provider.clone());
    let task = row
        .label
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| row.child_id.clone());
    let open = row.session_file.clone().map(PathBuf::from);

    let mut el = div()
        .id(SharedString::from(format!("subagent-row-{}", row.child_id)))
        .w_full()
        .h(px(CARD_H))
        // 水平 padding 挂 list（px(10)，用户定稿两侧对称）——行内不再带
        // 水平 padding，hover 高亮随之内缩，菜单边缘干净
        .mb(px(1.))
        .rounded(px(crate::ui::tokens::radius::INLINE))
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(6.))
        .text_size(crate::appearance::ui_size(12.))
        .hover(|s| s.bg(rgb(t.bg_hover)));
    if open.is_some() {
        let weak_row = weak.clone();
        el = el
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = weak_row.update(cx, |c, cx| {
                    if let Some(p) = open.clone() {
                        c.open_subagent_child(p, cx);
                    }
                });
            });
    }
    el.child(
        // 行一：状态标记 + 名称 + 统计（044-0 验收④：时长/轮/言/工具/token
        // icon 化上移到名字后）+ 失败分码（B3④：被拒/失败行没有子会话可
        // 点，理由内联行尾可见）。分栏定稿（2026-11-07）：名称限宽 150、
        // 统计整体右对齐（flex_1 + justify_end 撑满推右）
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            // 标记列固定 15px 宽（spinner 满宽、终态色点居中）——名称左缘
            // 恒在 35px 处，行二任务名的 px(23) 对齐才有基准（原先点只有
            // 7px 宽，名称随标记缩宽，与任务名错开 8px）
            .child(
                div()
                    .w(px(15.))
                    .flex_shrink_0()
                    .flex()
                    .justify_center()
                    .child(status_mark(kind, mark_c)),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .max_w(px(150.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(rgb(t.text))
                    .child(SharedString::from(name)),
            )
            .child(
                // stats 容器：**必须显式 .flex()**——无它子元素纵向堆叠
                //（gpui div 缺省非 row flex；验收④返工实录）；右对齐 =
                // justify_end，极窄时左缘裁切、右侧 token 恒可见
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(10.))
                    .text_size(crate::appearance::ui_size(11.))
                    .text_color(rgb(t.text_dim))
                    .children(stats_segments(row)),
            )
            .children(
                row.error_code
                    .as_deref()
                    .filter(|_| matches!(kind, StatusKind::Error))
                    .map(|code| {
                        div()
                            .flex_shrink_0()
                            .text_size(crate::appearance::ui_size(10.))
                            .text_color(rgb(C_ERROR))
                            .child(SharedString::from(code.to_string()))
                    }),
            ),
    )
    .child(
        // 行二：任务名（label 退 childId，超长截断）。px(23) = list 10 +
        // 标记列 15 + 行一 gap 8 − list 10 —— 与行一子代理名称左缘对齐
        div()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .px(px(23.))
            .text_size(crate::appearance::ui_size(11.))
            .text_color(rgb(t.text_dim))
            .child(SharedString::from(task)),
    )
    .into_any_element()
}

/// 行一统计段（044-0 验收④）：时长恒有；stats/usage 缺项（老 descriptor）
/// 整段隐藏，不摆 "—" 占位。轮/言/工具/时长/token 全 icon+数字（右侧导航
/// 条同款 + clock-4/coins，用户供图 lucide）。
fn stats_segments(row: &CatalogRow) -> Vec<gpui::AnyElement> {
    let t = T();
    let mut segs: Vec<gpui::AnyElement> = Vec::new();
    segs.push(
        div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(crate::ui::icon("clock-4", 11., t.text_dim))
            .child(SharedString::from(live_duration_text(row)))
            .into_any_element(),
    );
    let mut push_stat = |icon_name: &'static str, n: u64| {
        segs.push(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(crate::ui::icon(icon_name, 11., t.text_dim))
                .child(SharedString::from(n.to_string()))
                .into_any_element(),
        );
    };
    if let Some(n) = row.turns {
        push_stat("message-square-text", n);
    }
    if let Some(n) = row.agent_messages {
        push_stat("bot", n);
    }
    if let Some(n) = row.tool_calls {
        push_stat("wrench", n);
    }
    if let Some(n) = row.usage_total {
        // token 走 tokens_text 的 k 格式化（306k；push_stat 是裸计数专用）
        segs.push(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(crate::ui::icon("coins", 11., t.text_dim))
                .child(SharedString::from(tokens_text(n)))
                .into_any_element(),
        );
    }
    segs
}

/// 行的运行耗时秒数——**头部合计与行时远唯一口径**（044-0 B4：原先头部
/// 用 stale 的 updatedAt−startedAt、行用 live now−startedAt，出现合计 3s
/// vs 行 13s 的矛盾）：运行中 = 实时 now−startedAt（轮间隔内补齐实时性），
/// 终态 = updatedAt−startedAt。
fn row_secs(row: &CatalogRow) -> Option<i64> {
    if row.in_flight() {
        let a = parse_iso_secs(&row.started_at)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(a);
        return Some((now - a).max(0));
    }
    row_duration_secs(row)
}

/// 行一时长（044-0 运行时长）。
fn live_duration_text(row: &CatalogRow) -> String {
    row_secs(row)
        .map(duration_text)
        .unwrap_or_else(|| "—".into())
}

/// 行一状态标记（044-0：同会话列表）——运行中 = 8 针荆棘圈 spinner；
/// 终态 = 色点（绿完成 / 橙中止与轮次用尽 / 红失败）。
pub(super) fn status_mark(kind: StatusKind, color: u32) -> gpui::AnyElement {
    match kind {
        StatusKind::Running => crate::ui::spark_spinner(15., color),
        _ => div()
            .size(px(7.))
            .rounded_full()
            .bg(rgb(color))
            .into_any_element(),
    }
}

/// 状态标记用色（与行文案 status_of 同源，字体色换标记色）。
fn mark_color(row: &CatalogRow) -> u32 {
    match row.status_kind() {
        StatusKind::Running => C_RUNNING,
        StatusKind::Done => C_DONE,
        StatusKind::Aborted | StatusKind::MaxTurns => C_ABORTED,
        StatusKind::Error => C_ERROR,
    }
}

/// 秒 → 紧凑时长（pi-web 口径）：22s / 1m42s。
pub(crate) fn duration_text(secs: i64) -> String {
    if secs < 0 {
        return "—".into();
    }
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m{}s", secs / 60, secs % 60)
    }
}

/// tokens 紧凑格式（pi-web 口径）：870 / 23k / 100k（044-0 行二：k为单位）。
fn tokens_text(n: u64) -> String {
    if n < 1000 {
        n.to_string()
    } else {
        format!("{}k", (n + 500) / 1000)
    }
}
