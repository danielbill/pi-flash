//! 子代理工具卡渲染（委派/回收卡）。

use super::*;

// args 投影缓存（profile + 任务名）：卡片每帧只需要两个短字段，而 args
// 里含整段 prompt——每帧整串 `serde_json::from_str` 是巨轮会话的重绘
// 大头。key = (源文哈希, 长度)，命中后还要全等校验防碰撞（同
// `tool_card::tool_preview` 的 PREVIEW_CACHE 范式）。
thread_local! {
    static ARGS_FIELDS_CACHE: std::cell::RefCell<
        std::collections::VecDeque<((u64, usize), (String, (String, String)))>,
    > = const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}
/// 条数上限：卡片数量级远小于工具预览，128 条足够覆盖一屏。
const ARGS_FIELDS_CACHE_CAP: usize = 128;
/// 字节预算（存 args 原文用于全等校验）：子代理 args 以 prompt 为主，
/// 2MB 能装上千条普通任务。
const ARGS_FIELDS_CACHE_BUDGET_BYTES: usize = 2 * 1024 * 1024;

/// 卡片需要的两个 args 字段：`profile`（缺省 general-purpose）与任务名
/// （`label` 优先，退回 `prompt` 前 25 字）。带线程局部 LRU。
fn subagent_args_fields(args: &str) -> (String, String) {
    if args.is_empty() {
        return ("general-purpose".to_string(), String::new());
    }
    let key = (crate::editor::markdown::hash_str(args), args.len());
    ARGS_FIELDS_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        if let Some(pos) = cache.iter().rposition(|(k, _)| *k == key) {
            if cache[pos].1.0.as_str() == args {
                let entry = cache.remove(pos).expect("pos 来自刚才的迭代");
                let out = entry.1.1.clone();
                cache.push_back(entry); // 刷新到队尾（LRU）
                return out;
            }
        }
        let out = subagent_args_fields_uncached(args);
        cache.push_back((key, (args.to_string(), out.clone())));
        while cache.len() > ARGS_FIELDS_CACHE_CAP
            || cache.iter().map(|(k, _)| k.1).sum::<usize>() > ARGS_FIELDS_CACHE_BUDGET_BYTES
        {
            if cache.pop_front().is_none() {
                break;
            }
        }
        out
    })
}

fn subagent_args_fields_uncached(args: &str) -> (String, String) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(args) else {
        return ("general-purpose".to_string(), String::new());
    };
    let profile = v
        .get("profile")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("general-purpose")
        .to_string();
    let task = v
        .get("label")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            v.get("prompt")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .chars()
                .take(25)
                .collect()
        });
    (profile, task)
}

/// 044-0：subagent 专属卡——向 zcode 渲染对齐的**紧凑行**（验收⑤定稿）：
/// `[状态标记] 🤖子代理 <name(profile)> <时长> <任务名(25字截断)> <完成提示>`
/// 状态标记同会话列表（运行 spark_spinner(accent) / 完成 NOTICE 青蓝点 /
/// 异常 danger 红点）；完成提示 faint（任务完成 / 派发失败 / 执行异常），异常态
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
    // 借引用读 details 的四个字段：原实现 `details.cloned()` 每帧深拷贝整份
    // JSON，只为随后 `.get(..)`——纯浪费。
    let stop_reason = details
        .and_then(|d| d.get("stop_reason"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let session_file = details
        .and_then(|d| d.get("session_file"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from);
    let error_code = details
        .and_then(|d| d.get("error_code"))
        .and_then(|v| v.as_str());

    // args 解析结果走缓存：整串（含 prompt）每帧解析是重绘大头。
    let (profile, task) = subagent_args_fields(args);

    // 完成提示分类（验收⑤注 2/3）：
    //   派发失败 = 子代理根本没跑起来——PF 容量闸（CAPACITY_REACHED）、
    //              provider 侧拒绝/并发（START_REJECTED 归一）、深度/资料
    //              校验（DEPTH_LIMIT / PROFILE_INVALID / PROVIDER_UNKNOWN /
    //              BACKEND_UNSUPPORTED / DUPLICATE_CHILD）
    //   执行异常 = 跑起来后的失败——轮次用尽（max_looped/MAX_TURNS）、
    //              意外中断（aborted）、provider 服务侧异常（PROVIDER_ERROR）
    //              及其他一切非派发分码。**result 已到但 stopReason ≠
    //              completed 一律算异常**（2026-11-07 修正：live abort 的
    //              settle 不带 error_code，此前落「运行中」分支转圈到永远）
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
            .text_color(rgb(t.text_faint))
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
            .text_color(rgb(t.text_faint))
            .underline()
            .with_tip_detail(tip)
            .child(label)
            .into_any_element()
    } else {
        // 运行中不再占位（2026-11-07 用户定夺：删掉 `.....`），只留 spinner
        div().into_any_element()
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
        .child(
            // 标记槽定宽：spinner(15px) 与状态点(7px) 同槽居中，状态切换
            // 不再让后面的文字列错位（2026-11-07 用户：最后一行对齐失败）
            div()
                .w(px(15.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .child(mark_el),
        )
        // subagent 统一 bot icon（044-0 验收③）：图标档位 ICON_MD = 16，
        // 颜色随主题 accent（2026-11-07 用户定夺：不许自造尺寸/颜色）
        .child(crate::ui::icon("bot", 16., t.accent))
        .child(
            // 子代理名称栏（注 5）：150px 定宽，左对齐，超长截断；名字加粗
            div()
                .w(px(150.))
                .flex_shrink_0()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::sess_size(-3.))
                .font_weight(FontWeight::SEMIBOLD)
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
    // 完成提示紧跟任务描述栏左对齐（不贴右）；颜色走 text_faint，
    // 与任务描述的 text_dim 拉开一级（2026-11-07 用户：两行颜色不该一样）
    row = row.child(hint);
    // 整行点击 = 打开子会话（044-0 验收④，只读观察窗）。details 里没有
    // session_file 时（background 派发尚未落 completion）用 child_id 在主
    // 会话消息里反查，避免「点不动」。
    let child_id = details
        .and_then(|d| d.get("child_id"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if session_file.is_some() || child_id.is_some() {
        let weak_open = weak.clone();
        row = row
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = weak_open.update(cx, |c, cx| {
                    let sf = session_file.clone().or_else(|| {
                        let cid = child_id.as_deref()?;
                        c.session_file_for_child(cx, cid)
                    });
                    if let Some(sf) = sf {
                        c.open_session(sf, false, cx);
                    }
                });
            });
    }
    div().flex().flex_col().px(px(2.)).py(px(2.)).child(row)
}

#[cfg(test)]
mod args_fields_tests {
    use super::*;

    #[test]
    fn label_and_profile_come_from_args() {
        let (p, t) = subagent_args_fields(
            r#"{"profile":"explorer","label":"查日志","prompt":"很长的任务描述"}"#,
        );
        assert_eq!(p, "explorer");
        assert_eq!(t, "查日志");
    }

    #[test]
    fn prompt_is_the_task_fallback_and_is_truncated() {
        let long = "一二三四五六七八九十一二三四五六七八九十一二三四五六七八九十";
        let args = format!(r#"{{"prompt":"{long}"}}"#);
        let (p, t) = subagent_args_fields(&args);
        assert_eq!(p, "general-purpose");
        assert_eq!(t.chars().count(), 25);
        assert!(long.starts_with(&t));
    }

    #[test]
    fn partial_args_fall_back_without_panicking() {
        let (p, t) = subagent_args_fields(r#"{"profile":"expl"#);
        assert_eq!(p, "general-purpose");
        assert!(t.is_empty());
        assert_eq!(
            subagent_args_fields(""),
            ("general-purpose".to_string(), String::new())
        );
    }

    #[test]
    fn repeated_renders_hit_the_cache() {
        let args = r#"{"profile":"reviewer","label":"审代码","prompt":"x"}"#;
        let first = subagent_args_fields(args);
        let second = subagent_args_fields(args);
        assert_eq!(first, second);
        // 同键不同文（碰撞防护）：结果必须按原文重新算
        let other = r#"{"profile":"reviewer","label":"审代码 ","prompt":"x"}"#;
        assert_eq!(subagent_args_fields(other).1, "审代码 ");
    }
}
