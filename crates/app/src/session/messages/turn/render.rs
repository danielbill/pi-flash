//! render_assistant_turn：整轮装配（工作详情组折叠/展开、最终回答、
//! hover 操作栏、written files chips）。

use super::*;

/// 一轮 agent 回复（用户消息 → 下一用户消息之间的全部 assistant 消息）。
/// pi-web ChatWindow 轮分组 parity（v56-1）：思考+工具调用全部收进
/// 「工作详情」组——有最终回答时默认折叠、流式中/无最终回答时展开；
/// 最终回答 = 末条 assistant 的尾部 text/image 连续段
/// （splitFinalAssistantBlocks parity），其前置块同样入组；每条 assistant
/// 消息自带模型名标签；usage 只取最终消息（omitUsage parity）。hover
/// 操作栏（复制整轮/用时/时间）为自定义豁免项。
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_assistant_turn(
    turn: &[&Msg],
    // 轮内各消息的全局索引（thinking/copy key 用）
    turn_ixs: &[usize],
    start_ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    collapsed: &HashMap<(usize, usize), bool>,
    t: &theme::Theme,
    model_label: &str,
    // Some(est_tokens)：此轮正在流式（最后一个 assistant 消息）
    stream_info: Option<u64>,
    // Some(tps)：流式速度（pi-web 300ms tick 估算，四档配色）
    stream_tps: Option<f32>,
    meta: MsgMeta,
    // 操作栏悬停显影（Chat.bar_hover，pi-web hovered state parity）
    bar_revealed: bool,
    // 复制反馈（032 恢复）：runtime.copy_flash 点亮中 → 栏换 ✓ 已复制
    copied: bool,
    // 「新分支」目标（None = 锚点还没回来，按钮不出；观察窗恒 None）
    fork: Option<ForkAnchor>,
    // 只读观察窗（044-0）：消息操作栏随输入面板一并隐藏
    read_only: bool,
) -> gpui::AnyElement {
    // pi-web onMouseEnter/Leave parity：悬停整轮（无遮挡后代，见 bar_hover_wired）
    let mut col = bar_hover_wired(
        div()
            .id(SharedString::from(format!("turn-{start_ix}")))
            .w_full()
            .mb(px(16.))
            .flex()
            .flex_col(),
        weak,
        start_ix,
    );
    let is_working = stream_info.is_some();

    // pi-web findFinalAssistantIndex：有 answer 连续段的末条 assistant，
    // 兜底取末条 assistant
    let final_pos = turn
        .iter()
        .rposition(|m| m.role == Role::Assistant && msg_has_answer(m))
        .or_else(|| turn.iter().rposition(|m| m.role == Role::Assistant));
    let Some(final_pos) = final_pos else {
        return col.into_any_element();
    };
    let final_msg = turn[final_pos];
    let final_gix = turn_ixs[final_pos];
    let answer_start = split_answer_start(&final_msg.blocks);
    let answer_len = final_msg.blocks.len() - answer_start;
    let final_error = final_msg.stop_reason.as_deref() == Some("error");
    let final_truncated = final_msg.stop_reason.as_deref() == Some("length");
    let has_final_answer = answer_len > 0 || final_error || final_truncated;

    // 复制整轮文本（hover 操作栏，自定义保留）。**按需构建**：巨轮（数百条
    // assistant）的全量拼接随轮内文本线性涨，若每帧都拼，打字/流式期间每帧
    // 都是 MB 级 memcpy。操作栏只在 hover/复制反馈/分支在飞时可见，隐藏帧
    // 传空串（栏 opacity 0，copy pill 也被 isEmpty 门掉，语义不变）。
    let forking = fork.as_ref().is_some_and(|f| f.forking);
    let turn_text = if bar_revealed || copied || forking {
        let mut s = String::new();
        for m in turn {
            for b in &m.blocks {
                if let Block::Text { text, .. } = b {
                    if !text.trim().is_empty() {
                        if !s.is_empty() {
                            s.push_str("\n\n");
                        }
                        s.push_str(text);
                    }
                }
            }
        }
        s
    } else {
        String::new()
    };

    // ---- 运行中：pi-web `isLiveTail` 平铺渲染（不分组、不折叠）----
    // pi-web ChatWindow：`isLiveTail = (sessionBusy || isStreaming) &&
    // endIdx === messages.length && userIdx === lastAnchorIdx` 时，把这一轮直接
    // 平铺——每条 assistant 一个模型名行、thinking/toolCall/text 就地展开，流式
    // 那条的模型名行带 ↓token 估算 + t/s 徽章。usage 行 / 复制栏（pi-web
    // MessageView `!isStreaming` 才渲染）与「工作详情」折叠行、最终回答分区一律
    // 等到**轮末整形**（下面的 !is_working 分支）才出现。
    // 旧实现轮中就套上折叠组：思考/工具被折起来（streaming 时看不见思考框），
    // 内容高度忽大忽小，还把翻页钉顶搅乱。
    let est = stream_info.filter(|_| is_working);
    let tps = stream_tps.filter(|_| is_working);
    if is_working {
        return col
            .child(live_turn_body(
                turn,
                turn_ixs,
                weak,
                collapsed,
                t,
                model_label,
                est,
                tps,
            ))
            .into_any_element();
    }

    // ---- 「工作详情」组：全部 assistant 消息的 thinking/toolCall，最终
    // 消息只贡献 answer 连续段之前的前置块（pi-web processViews parity）----
    // 折叠判定前置：**折叠帧一个块元素都不建**。此前组体无条件全量构建、
    // 只是最后不挂上树——巨轮（数百条 assistant）每帧照付几百次
    // render_block（内含 serde/markdown 实打实的解析工作），收起状态照样
    // 卡（688 条消息会话实测）。展开帧（用户点开）才付全款。
    let key = (start_ix, usize::MAX);
    let default_open = is_working || !has_final_answer;
    let open = collapsed.get(&key).copied().unwrap_or(default_open);

    // 条目/工具计数零成本扫描（标签文案用，不建元素）
    let (mut n_views, mut n_tools) = (0usize, 0usize);
    for (i, (m, _)) in turn.iter().zip(turn_ixs).enumerate().take(final_pos + 1) {
        if m.role != Role::Assistant {
            continue;
        }
        let end = if i == final_pos {
            answer_start
        } else {
            m.blocks.len()
        };
        if !m.blocks[..end].iter().any(|b| block_displayable(b)) {
            continue;
        }
        n_views += 1;
        n_tools += m.blocks[..end]
            .iter()
            .filter(|b| matches!(b, Block::ToolCall { .. }))
            .count();
    }

    if n_views > 0 {
        let label = crate::i18n::tf(
            "nav.work_details",
            &[("m", n_views.to_string()), ("t", n_tools.to_string())],
        );
        let weak_fold = weak.clone();
        let fold_row = div()
            .id(SharedString::from(format!("work-fold-{start_ix}")))
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(8.))
            .py(px(4.))
            .ml(px(-8.))
            // 工作详情行 = 会话字号（字体大小设置.md §2）
            .text_size(crate::appearance::sess_size(0.))
            .text_color(rgb(t.text_muted))
            .cursor_pointer()
            .rounded(px(6.))
            .hover(|s| s.bg(rgb(t.bg_hover)))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = weak_fold.update(cx, |c, cx| {
                    let rt = c.rt();
                    rt.update(cx, |r, _| {
                        let next = !r.collapsed.get(&key).copied().unwrap_or(false);
                        r.collapsed.insert(key, next);
                    });
                    cx.notify();
                });
            })
            .child(icon(
                if open {
                    "chevron-down"
                } else {
                    "chevron-right"
                },
                12.,
                t.text_dim,
            ))
            .child(SharedString::from(label));
        col = col.child(fold_row);
        if open {
            // 块容器 gap 8（pi-web AssistantMessageView：label 下方 flexDirection
            // column + gap 8）；label 自带 mb 4 → label→首块 12px，与 pi-web 一致
            let mut group_body = div().mt(px(8.)).flex().flex_col();
            for (i, (m, &gix)) in turn.iter().zip(turn_ixs).enumerate().take(final_pos + 1) {
                if m.role != Role::Assistant {
                    continue;
                }
                let end = if i == final_pos {
                    answer_start
                } else {
                    m.blocks.len()
                };
                let disp: Vec<&Block> = m.blocks[..end]
                    .iter()
                    .filter(|b| block_displayable(b))
                    .collect();
                if disp.is_empty() {
                    continue;
                }
                let mut item = div().mb(px(16.)).flex().flex_col().gap(px(8.));
                // 每条 assistant 消息自带模型名标签（pi-web AssistantMessageView）
                item = item.child(model_label_div(
                    m.model.as_deref().unwrap_or(model_label),
                    None,
                    None,
                    t,
                ));
                // pi-web thinkingDurationFromFile：该消息的生成时长挂到 thinking 块
                let msg_think_dur = m.end_ts.zip(m.ts).map(|(e, s)| (e - s).max(0) / 1000);
                for b in disp {
                    let streaming = is_working && i == turn.len() - 1;
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
                group_body = group_body.child(item);
            }
            col = col.child(group_body);
        }
    }

    // ---- 轮末整形：最终回答分区（pi-web finalAnswerMessage）----
    // 走到这里必然 !is_working（运行中已在上面平铺返回），所以模型名行不带
    // ↓token/t-s 徽章——pi-web 的徽章只属于 isStreaming 的那条消息。
    if answer_len > 0 || final_error || final_truncated {
        col = col.child(model_label_div(
            final_msg.model.as_deref().unwrap_or(model_label),
            None,
            None,
            t,
        ));
    }
    if answer_len > 0 {
        // pi-web 最终回答块容器 gap 8
        let mut answer = div().w_full().flex().flex_col().gap(px(8.));
        for b in &final_msg.blocks[answer_start..] {
            answer = answer.child(render_block(
                b,
                final_gix,
                weak,
                collapsed,
                t,
                false,
                None,
                final_msg.ts,
            ));
        }
        col = col.child(answer);
    }
    // c26/c27（pi-web providerError / truncated parity）：错误红框、截断黄框
    if final_error {
        let msg = final_msg
            .error_message
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or("Unknown provider error");
        col = col.child(alert_box(format!("Error: {msg}"), 0xef4444, 0xef4444, t));
    }
    if final_truncated {
        col = col.child(alert_box(
            tr("messages.truncated").to_string(),
            0xca8a04,
            0xeab308,
            t,
        ));
    }
    // c24: 轮内写文件 chips（pi-web TurnWrittenFiles parity，见 written_files_chips）
    let written = turn_written_files(turn);
    if let Some(chips) = written_files_chips(start_ix, weak, &written, t) {
        col = col.child(chips);
    }
    if read_only {
        // 观察窗：无复制/新分支/用时栏（变异操作随输入面板一并隐藏）
        return col.into_any_element();
    }
    col = col.child(actions_bar::assistant_action_bar(
        start_ix,
        weak,
        &turn_text,
        turn.last().and_then(|m| m.end_ts),
        meta.turn_user_ts,
        is_working,
        bar_revealed,
        copied,
        fork,
        t,
    ));
    col.into_any_element()
}

/// c24: 轮内写文件 chips（pi-web TurnWrittenFiles parity：由
/// 成功的 write/edit/apply_patch 工具调用推导，绝不扫描回复文本）。
fn written_files_chips(
    start_ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    written: &[String],
    t: &theme::Theme,
) -> Option<gpui::AnyElement> {
    if written.is_empty() {
        return None;
    }
    let mut chips = div().flex().flex_wrap().gap(px(6.)).mt(px(6.));
    for path in written {
        let p = path.clone();
        let weak_open = weak.clone();
        let name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone());
        chips = chips.child(
            div()
                .id(SharedString::from(format!("wf-{start_ix}-{name}")))
                .flex()
                .items_center()
                .gap(px(4.))
                .px(px(8.))
                .py(px(2.))
                .rounded(px(6.))
                .bg(rgba(t.bg_subtle))
                .border_1()
                .border_color(rgb(t.border))
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::sess_size(-2.))
                .text_color(rgb(t.text))
                .cursor_pointer()
                .hover(|s| s.text_color(rgb(t.text)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let p = p.clone();
                    let _ = weak_open.update(cx, |c, cx| {
                        c.open_file_tab(std::path::PathBuf::from(&p), cx)
                    });
                })
                .child(icon("file", 12., t.text_dim))
                .child(SharedString::from(name)),
        );
    }
    Some(chips.into_any_element())
}
