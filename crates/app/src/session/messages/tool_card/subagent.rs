//! 子代理工具卡渲染（委派/回收卡）。

use super::*;

/// 044-0：subagent 专属卡——向 zcode 渲染对齐的**紧凑行**（验收⑤定稿）：
/// `[状态标记] 🤖子代理 <name(profile)> <时长> <任务名(25字截断)> <完成提示>`
/// 状态标记同会话列表（运行 spark_spinner(accent) / 完成 NOTICE 青蓝点 /
/// 异常 danger 红点）；完成提示 faint（运行 `.....` / 任务完成），异常态
/// faint+下划线 + hover tooltip（错误详情）。
/// 数据源（扩展 pf-subagents 的 details，snake_case）：`child_id` / `status`
/// / `stop_reason` / `session_file` / `error_code` / `duration_ms`；args 取
/// `profile` / `label` / `prompt`。整行点击 = 打开子会话（只读观察窗）。
#[allow(clippy::too_many_arguments)]
pub(super) fn render_subagent_card(
    msg_ix: usize,
    content_index: usize,
    args: &str,
    result: &str,
    is_error: bool,
    details: Option<&serde_json::Value>,
    // 时长已从规格行删除（2026-11-07 简化），形参保留对齐派发签名
    _duration_s: Option<i64>,
    // 行不区分「参数是否流式部分到达」（args 只取 name/task），保留形参
    // 以对齐派发签名
    _args_partial: bool,
    result_arrived: bool,
    _msg_ts: Option<i64>,
    weak: &gpui::WeakEntity<Chat>,
    // 旧版展开态残留槽位：紧凑行无正文，保留形参对齐签名
    _collapsed: &HashMap<(usize, usize), bool>,
    t: &theme::Theme,
) -> gpui::Div {
    let d = details.cloned().unwrap_or_default();
    let stop_reason = d.get("stop_reason").and_then(|v| v.as_str()).unwrap_or("");
    let session_file = d
        .get("session_file")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from);
    let error_code = d.get("error_code").and_then(|v| v.as_str());

    let args_v = serde_json::from_str::<serde_json::Value>(args).ok();
    let profile = args_v
        .as_ref()
        .and_then(|v| v.get("profile"))
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("general-purpose")
        .to_string();
    let task = args_v
        .as_ref()
        .and_then(|v| {
            v.get("label")
                .and_then(|x| x.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| {
            args_v
                .as_ref()
                .and_then(|v| v.get("prompt"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .chars()
                .take(25)
                .collect::<String>()
        });

    // 完成提示分类（验收⑤注 2/3）：
    //   派发失败 = 子代理根本没跑起来——PF 容量闸（CAPACITY_REACHED）、
    //              provider 侧拒绝/并发（START_REJECTED 归一）、深度/资料
    //              校验（DEPTH_LIMIT / PROFILE_INVALID / PROVIDER_UNKNOWN /
    //              BACKEND_UNSUPPORTED / DUPLICATE_CHILD）
    //   执行异常 = 跑起来后的失败——轮次用尽（max_looped/MAX_TURNS）、
    //              意外中断（aborted）、provider 服务侧异常（PROVIDER_ERROR）
    //              及其他一切非派发分码。**result 已到但 stopReason ≠
    //              completed 一律算异常**（2026-11-07 修正：live abort 的
    //              settle descriptor 不带 error_code，此前落「运行中」
    //              分支转圈到永远）
    //   任务完成 = result 到且 stopReason==completed 且无错
    //   运行中   = 其余（`.....`）
    const DISPATCH_CODES: [&str; 7] = [
        "CAPACITY_REACHED",
        "START_REJECTED",
        "DEPTH_LIMIT",
        "PROVIDER_UNKNOWN",
        "BACKEND_UNSUPPORTED",
        "PROFILE_INVALID",
        "DUPLICATE_CHILD",
    ];
    let errored = is_error || error_code.is_some();
    let dispatch_failed = error_code.is_some_and(|c| DISPATCH_CODES.contains(&c));
    let done = result_arrived && stop_reason == "completed" && !errored;
    let failed = !done && (errored || result_arrived);

    // 状态标记（验收⑤注 4：同会话列表 psp_row_slot 统一样式，15px 转圈）
    let mark_el: gpui::AnyElement = if done {
        div()
            .size(px(7.))
            .rounded_full()
            .bg(rgb(crate::appearance::theme::NOTICE))
            .into_any_element()
    } else if failed {
        div()
            .size(px(7.))
            .rounded_full()
            .bg(rgb(t.danger))
            .into_any_element()
    } else {
        crate::ui::spark_spinner(15., t.accent)
    };

    // tooltip 详情：result 全文（多行错误详情，350×100-300 滚动浮层）→
    // 分码兜底
    let err_detail: SharedString = {
        let body = result.trim();
        if !body.is_empty() {
            body.to_string()
        } else {
            error_code.unwrap_or("").to_string()
        }
    }
    .into();

    // 完成提示段（faint=text_dim；异常态 faint+下划线+详情浮层）
    let hint: gpui::AnyElement = if done {
        div()
            .flex_shrink_0()
            .text_color(rgb(t.text_dim))
            .child(tr("subagent.card.done"))
            .into_any_element()
    } else if failed {
        let (label, tip) = if dispatch_failed {
            (tr("subagent.card.dispatch_error"), err_detail)
        } else {
            (tr("subagent.card.run_error"), err_detail)
        };
        div()
            .id(SharedString::from(format!(
                "sa-hint-{msg_ix}-{content_index}"
            )))
            .flex_shrink_0()
            .text_color(rgb(t.text_dim))
            .underline()
            .with_tip_detail(tip)
            .child(label)
            .into_any_element()
    } else {
        div()
            .flex_shrink_0()
            .text_color(rgb(t.text_dim))
            .child(".....")
            .into_any_element()
    };

    let mut row = div()
        .id(SharedString::from(format!("sa-{msg_ix}-{content_index}")))
        .flex()
        .items_center()
        .gap(px(7.))
        .px(px(10.))
        .py(px(5.))
        .min_w_0()
        .rounded(px(crate::ui::tokens::radius::CTRL))
        .text_size(crate::appearance::sess_size(-2.))
        .hover(|s| s.bg(gpui::rgba(theme::rgba_a(t.accent, 0.06))))
        .child(mark_el)
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(4.))
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::sess_size(-3.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(0x60a5fa))
                // subagent 统一 bot icon（044-0 验收③）
                .child(crate::ui::icon("bot", 11., 0x60a5fa))
                .child("子代理"),
        )
        .child(
            // 子代理名称栏（注 5）：150px 定宽，左对齐，超长截断
            div()
                .w(px(150.))
                .flex_shrink_0()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::sess_size(-3.))
                .text_color(rgb(t.text))
                .child(SharedString::from(profile)),
        )
        .child(
            // 任务描述栏（注 5）：300px 定宽，左对齐，超长截断
            div()
                .w(px(300.))
                .flex_shrink_0()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(task)),
        );
    row = row.child(hint);
    if let Some(sf) = session_file {
        let weak_open = weak.clone();
        row = row
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = weak_open.update(cx, |c, cx| {
                    c.open_session(sf.clone(), false, cx);
                });
            });
    }
    div().flex().flex_col().px(px(2.)).py(px(2.)).child(row)
}
