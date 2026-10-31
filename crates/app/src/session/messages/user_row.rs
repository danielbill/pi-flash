//! 用户消息行（032）：[`render_block`] 块分发（text/thinking/image/工具
//! 简卡）、skill 折叠命令还原、操作栏悬停接线、[`render_msg`] 用户行、
//! [`render_bash_msg`]。自 turn.rs 拆出（090 重构）；三个 tests mod 随迁。

use super::*;
use std::collections::HashMap;

use gpui::{
    Animation, AnimationExt, MouseButton, SharedString, div, prelude::*, px, relative, rgb,
};
use pi_link::protocol::Block;

use crate::Chat;
use crate::appearance::theme;
use crate::editor::markdown;
use crate::i18n::tr;
use crate::session::actions_bar::user_action_bar;
use crate::ui::icon;
pub(crate) fn render_block(
    b: &Block,
    msg_ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    collapsed: &HashMap<(usize, usize), bool>,
    t: &theme::Theme,
    // 流式中的消息：markdown 代码块跳过 syntect 高亮（v56-3 c15）
    streaming: bool,
    // thinking 时长（pi-web ThinkingBlock 右侧 Ns；快照按消息首尾时间差）
    thinking_dur: Option<i64>,
) -> gpui::Div {
    match b {
        Block::Text {
            content_index,
            text,
            ..
        } if !text.trim().is_empty() => {
            // 定稿块走 TextView（划选 + Ctrl+C 复制，textview.rs）；流式块
            // 仍走自研渲染器——TextView 文本变更是 200ms 防抖后台重解析，
            // 会拖慢流式显示（消息划选一期定夺）
            if streaming {
                div().w_full().child(markdown::render(text, t, streaming))
            } else {
                let hash = {
                    use std::hash::{Hash, Hasher};
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    text.hash(&mut h);
                    h.finish()
                };
                div().w_full().child(AgentMarkdown::new(
                    SharedString::from(format!("tv-{msg_ix}-{content_index}-{hash:x}")),
                    text.clone(),
                ))
            }
        }
        Block::Thinking {
            text,
            content_index,
        } if !text.trim().is_empty() => {
            let key = (msg_ix, *content_index);
            // 「展示思考」开关 = 新思考块的默认展开态：开=默认展开全文，
            // 关（默认）=收成一行（灯泡+单行预览，点击可展开）；用户手动
            // 展开过的块以 collapsed 里的显式值为准
            let expanded = collapsed
                .get(&key)
                .copied()
                .unwrap_or(crate::services::workspace::show_thinking());
            let weak = weak.clone();
            // pi-web getThinkingPreview: first line, up to 240 chars, trimmed
            let preview: String = text
                .trim_start()
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(240)
                .collect::<String>()
                .trim_end()
                .to_string();
            // pi-web ThinkingBlock: flat var(--bg) strip with 1px border —
            // [lightbulb] [one-line preview] collapsed, [lightbulb] [pre-wrap
            // muted body] expanded; amber bulb while expanded, no chevron, no
            // "thinking" label
            let toggle = div()
                .id(SharedString::from(format!("th-{msg_ix}-{content_index}")))
                .cursor_pointer()
                .flex()
                .items_center()
                .gap_1p5()
                .min_w_0()
                .text_color(rgb(t.text_muted))
                .when(expanded, |d| d.flex_shrink_0())
                .when(!expanded, |d| d.flex_1())
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let k = key;
                    let _ = weak.update(cx, |c, cx| {
                        let rt = c.rt();
                        rt.update(cx, |r, _| {
                            let next = !r.collapsed.get(&k).copied().unwrap_or(true);
                            r.collapsed.insert(k, next);
                        });
                        cx.notify();
                    });
                })
                .child(if expanded {
                    icon("lightbulb", 14., 0xd4a017)
                } else {
                    icon("lightbulb", 14., t.text_muted)
                })
                .children((!expanded).then(|| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(if preview.is_empty() {
                            SharedString::from(tr("loading.ellipsis")).into_any_element()
                        } else {
                            SharedString::from(preview).into_any_element()
                        })
                }));
            let mut block = div()
                .w_full()
                // 块间距由上层 gap 8 容器统一（pi-web 块容器 gap:8），
                // 块自身不挂 margin（此前 my_1=4px 与工具卡 0px 不均）
                .flex()
                .items_start()
                .gap_1p5()
                .min_w_0()
                .px(px(10.))
                .py(px(6.))
                .rounded(px(7.))
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.bg))
                .font_family(crate::editor::markdown::MONO_FAMILY)
                // 思考块字号 = 会话字号 -2（字体大小设置.md §2；族保持等宽）
                .text_size(crate::appearance::sess_size(-2.))
                .line_height(relative(1.5))
                .child(toggle);
            if expanded {
                block = block.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(rgb(t.text_muted))
                        .child(SharedString::from(text.clone())),
                );
            }
            // pi-web {duration}s：右侧 text-dim
            if let Some(d) = thinking_dur {
                block = block.child(
                    div()
                        .flex_shrink_0()
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(format!("{d}s"))),
                );
            }
            block
        }
        Block::ToolCall {
            content_index,
            name,
            args,
            result,
            is_error,
            images,
            duration_s,
            details,
            args_partial,
            result_arrived,
            ..
        } => {
            // v56-2 c8/c9: pi-web ToolCallBlock parity —— 状态色卡片 +
            // 参数摘要 + 耗时 + 旋转箭头 + 展开体（参数 pre / 结果 pre）；
            // diff（c10）与图片（c11）见后续提交。收进「工作详情」组内。
            render_tool_card(
                msg_ix,
                *content_index,
                name,
                args,
                result,
                *is_error,
                images,
                details.as_ref(),
                *duration_s,
                *args_partial,
                *result_arrived,
                weak,
                collapsed,
                t,
            )
        }
        _ => div().w_full(),
    }
}

/// pi-web lib/slash-display.ts skillExpansionToCommand 的 Rust 移植：识别
/// pi _expandSkillCommand 输出的信封，还原紧凑命令（仅显示用，存储文本
/// 不变）。贪婪正文（正文可能含示例 </skill>，取最后一个），可选双换行
/// 后缀为用户参数。
pub(crate) fn skill_expansion_to_command(text: &str) -> Option<String> {
    let rest = text.strip_prefix("<skill name=\"")?;
    let (name, rest) = rest.split_once('"')?;
    let rest = rest.strip_prefix(" location=\"")?;
    let (_loc, rest) = rest.split_once('"')?;
    let rest = rest.strip_prefix(">\n")?;
    let rest = rest.strip_prefix("References are relative to ")?;
    let rest = rest.split_once('\n')?.1; // 跳过 base 目录行
    let rest = rest.strip_prefix('\n')?; // 空行
    let close = rest.rfind("\n</skill>")?;
    let tail = &rest[close + 9..];
    let args = match tail.strip_prefix("\n\n") {
        Some(a) => a.to_string(),
        None if tail.is_empty() => String::new(),
        None => return None,
    };
    Some(if args.is_empty() {
        format!("/skill:{}", name)
    } else {
        format!("/skill:{} {}", name, args)
    })
}

/// 操作栏悬停显影的**唯一**接线：用户消息行与 agent 轮块都走它，不许各写
/// 一段内联 `on_hover`（两套写法必然分叉，见下）。
///
/// 状态 = [`Chat::bar_hover`]：单个 `Option<usize>`，存该行在列表里的索引
/// （用户行 = 消息索引，agent 轮 = 轮首消息索引；两者角色不同，索引永不撞车）。
///
/// **为什么必须统一且小心后代遮挡**：gpui `Window::hit_test` 命中
/// `HitboxBehavior::BlockMouse`（即 `div().occlude()`）就 `break`，插入更早
/// 的 hitbox（**祖先全部**）连 `ids` 都进不去，`hitbox.is_hovered()` 恒
/// false。用户气泡曾挂 `occlude()`（本意「禁止鼠标透传到下层消息」），于是
/// 鼠标停在气泡上时行级 on_hover 永不触发、操作栏不显影；agent 轮块没有遮挡
/// 后代，一切正常——这就是「同一种行为两处表现」的根因。
///
/// 挂在**有 id 的行**上（用户行 `msgrow-*` / 轮块 `turn-*`），故要
/// `StatefulInteractiveElement`：`on_hover` 在它上面，不在裸 `InteractiveElement`。
/// 回归锁见本文件测试模块 `bar_hover_hit_test`（正反两侧）。
pub(crate) fn bar_hover_wired<E: StatefulInteractiveElement>(
    el: E,
    weak: &gpui::WeakEntity<Chat>,
    ix: usize,
) -> E {
    let weak = weak.clone();
    el.on_hover(move |hovered, _, cx| {
        let _ = weak.update(cx, |c, cx| {
            let next = if *hovered { Some(ix) } else { None };
            if c.composer.bar_hover != next {
                c.composer.bar_hover = next;
                cx.notify();
            }
        });
    })
}

pub(crate) fn render_msg(
    m: &Msg,
    msg_ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    expanded_skills: &std::collections::HashSet<String>,
    bubble_scrolls: &std::rc::Rc<
        std::cell::RefCell<std::collections::HashMap<String, gpui::ScrollHandle>>,
    >,
    _collapsed: &HashMap<(usize, usize), bool>,
    t: &theme::Theme,
    // Some(est_tokens) 仅当此消息是流式中的最后一条（工作中回复）
    _stream_info: Option<u64>,
    _meta: MsgMeta,
    // 操作栏悬停显影（Chat.bar_hover，pi-web hovered state parity）
    bar_revealed: bool,
    // 复制反馈（032 恢复）：runtime.copy_flash 点亮中 → 栏换 ✓ 已复制
    copied: bool,
) -> gpui::Div {
    // pi-web 消息间距 marginBottom 16（v 此前 22 偏大）
    let mut col = div().w_full().mb(px(16.)).flex().flex_col();
    // 消息内文件路径/本地文件链接点击的目标（markdown 渲染构建期捕获进
    // 事件闭包；文件 md 预览走 content.rs 自设 base）
    markdown::set_link_target(weak.clone(), None);
    if m.role == Role::User {
        // v56-4 c18-c22（pi-web UserMessageView parity）：右对齐 85% 宽、
        // user_bg 底 + 1px 蓝边框 rgba(59,130,246,.2)、圆角 12、pad 8/12、
        // 内容走 markdown、内嵌图片 240 上限、超高 300px 内部滚动；
        // 操作行（复制/编辑/新分支）hover 淡入 = 自定义豁免项。
        let text = m.plain_text();

        // v58: 用户消息图片 = 气泡上方独立缩略图行（参考截图 parity，不再
        // 内嵌气泡）：67×67（composer 56 的 +20%）、间隔 5px、右对齐（随
        // 行 items_end）、点击开 ImagePreview 大图弹窗
        let image_row: Option<gpui::AnyElement> = {
            let mut thumbs: Vec<gpui::AnyElement> = Vec::new();
            for (bi, b) in m.blocks.iter().enumerate() {
                if let Block::Image { mime, data, .. } = b {
                    if let Some(format) = mime_to_image_format(mime) {
                        if let Ok(image) = decode_image_cached(data, format) {
                            let image_for_open = image.clone();
                            let weak_open = weak.clone();
                            thumbs.push(
                                div()
                                    .id(SharedString::from(format!("uimg-{msg_ix}-{bi}")))
                                    .size(px(67.))
                                    .flex_shrink_0()
                                    .rounded(px(6.))
                                    .border_1()
                                    .border_color(rgb(t.border))
                                    .bg(rgb(t.bg_panel))
                                    .overflow_hidden()
                                    .cursor_pointer()
                                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                        let image = image_for_open.clone();
                                        let _ = weak_open.update(cx, |c, cx| {
                                            c.dlg.dialog =
                                                Some(crate::Dialog::ImagePreview { image });
                                            cx.notify();
                                        });
                                    })
                                    .child(
                                        gpui::img(image)
                                            .size_full()
                                            .object_fit(gpui::ObjectFit::Cover),
                                    )
                                    .into_any_element(),
                            );
                        }
                    }
                }
            }
            (!thumbs.is_empty()).then(|| {
                div()
                    .w(relative(0.85))
                    .flex()
                    .flex_wrap()
                    .justify_end()
                    .gap(px(5.))
                    .children(thumbs)
                    .into_any_element()
            })
        };
        // c19: 用户内容走 markdown；但 HTML 不渲染、标签原样显示
        // （render_user）——用户消息是发出内容的凭证，气泡吞标签会让
        // 用户无法核对 agent 实际收到的文本（v57 用户反馈）
        //
        // v57: 技能展开消息（CLI 把 /skill:xxx 展开成多行全文）默认折叠
        // 成 mono 命令行 + 展开箭头（pi-web parity），展开后内容区限高
        // 带滚动条；单行 /skill:xxx（未展开回显）按普通渲染
        // v57: pi-web UserMessageView parity——skill 展开消息（信封文本）
        // 默认折叠：mono 技能名 + 展开箭头 + 参数原文；展开显示全文
        // markdown；复制/编辑目标都是紧凑命令（copyTarget/editTarget parity）
        let command_text = skill_expansion_to_command(&text);
        let skill_key = m
            .entry_id
            .clone()
            .unwrap_or_else(|| format!("skill-{}", msg_ix));
        let skill_open = expanded_skills.contains(&skill_key);
        let md: gpui::AnyElement = if text.trim().is_empty() {
            div().into_any_element()
        } else if let Some(cmd) = &command_text {
            let (cmd_name, cmd_args) = match cmd.split_once(' ') {
                Some((n, a)) => (n.to_string(), a.to_string()),
                None => (cmd.clone(), String::new()),
            };
            let weak_skill = weak.clone();
            let chevron = if skill_open {
                "chevron-up"
            } else {
                "chevron-down"
            };
            let mut stack = div().flex().flex_col().w_full().gap(px(6.));
            stack = stack.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .flex_wrap()
                    .child(
                        div()
                            .id(SharedString::from(format!("skill-toggle-{}", msg_ix)))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .flex_shrink_0()
                            .font_family(crate::editor::markdown::MONO_FAMILY)
                            .text_size(crate::appearance::ui_size(13.))
                            .text_color(rgb(t.accent))
                            .cursor_pointer()
                            .hover(|s| s.opacity(0.85))
                            .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                                let key = skill_key.clone();
                                let _ = weak_skill.update(cx, |c, cx| {
                                    if !c.composer.expanded_skills.remove(&key) {
                                        c.composer.expanded_skills.insert(key);
                                    }
                                    cx.notify();
                                });
                            })
                            .child(SharedString::from(cmd_name))
                            .child(icon(chevron, 11., t.accent)),
                    )
                    .when(!cmd_args.is_empty(), |d| {
                        d.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(crate::appearance::ui_size(14.))
                                .text_color(rgb(t.text))
                                .child(SharedString::from(cmd_args)),
                        )
                    }),
            );
            if skill_open {
                stack = stack.child(markdown::render_user(&text, t));
            }
            stack.into_any_element()
        } else {
            markdown::render_user(&text, t)
        };

        let scroll_key = m
            .entry_id
            .clone()
            .unwrap_or_else(|| format!("ububble-{}", msg_ix));
        let (scroll_handle, scroll_state) = {
            let mut map = bubble_scrolls.borrow_mut();
            let h = map
                .entry(scroll_key)
                .or_insert_with(gpui::ScrollHandle::new)
                .clone();
            let st = gpui_component::scroll::ScrollbarState::default();
            (h, st)
        };

        let bottom = user_action_bar(msg_ix, weak, m.ts, bar_revealed, copied, t);
        // 纯图片消息（空文本）不渲染空泡；缩略图行在气泡上方
        let has_text = !text.trim().is_empty();
        let mut row = bar_hover_wired(
            div()
                .id(SharedString::from(format!("msgrow-{msg_ix}")))
                .w_full()
                .flex()
                .flex_col()
                .items_end()
                .gap(px(3.)),
            weak,
            msg_ix,
        );
        if let Some(imgs) = image_row {
            row = row.child(imgs);
        }
        if has_text {
            row = row.child(
                div().max_w(relative(0.85)).flex().min_w_0().child(
                    div()
                        .relative()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .id(SharedString::from(format!("ububble-{msg_ix}")))
                                // c21: 超高气泡 300px 内部滚动（USER_BUBBLE_MAX_HEIGHT）
                                .max_h(px(300.))
                                .overflow_y_scroll()
                                .track_scroll(&scroll_handle)
                                .flex_1()
                                .min_w_0()
                                // v63-8: 气泡内滚拦截（穿透修复）。gpui 滚轮派发给
                                // 所有命中 hitbox（含祖先，v63-3 删 occlude 后无
                                // 人拦），长气泡上滚轮会**同帧**连动外层会话列表
                                // ——「用户消息鼠标穿透」。在此判断：滚动前已在
                                // 对应边界才放行（原生嵌套滚动语义：内滚到底，
                                // 下一滚才带动外层）；未溢出（max=0）恒放行。
                                // 注意 gpui 滚动 offset 存储域为 [-max, 0]，且
                                // 同 div 内置滚动 handler 先于本 listener 执行，
                                // offset() 已含本次 delta，须减回去取滚动前值。
                                // 本拦截能生效的前提：会话列表（gpui `List`）的
                                // 滚轮注册晚于条目，否则列表同帧先滚、stop 已晚
                                // （vendor `elements/list.rs` v63-9 补丁修正了注册
                                // 时机，回归锁 `mod bubble_wheel`）。
                                .on_scroll_wheel({
                                    let sh = scroll_handle.clone();
                                    move |ev: &gpui::ScrollWheelEvent, window, cx| {
                                        let max = sh.max_offset().height;
                                        if max <= px(0.) {
                                            return; // 未溢出：无内滚空间，恒放行
                                        }
                                        let delta_y = ev.delta.pixel_delta(window.line_height()).y;
                                        if delta_y == px(0.) {
                                            return;
                                        }
                                        let off_pre = sh.offset().y - delta_y;
                                        const EPS: f32 = 1.; // 浮点容差，防边界误拦
                                        let at_edge = if delta_y > px(0.) {
                                            off_pre >= px(-EPS) // 已在顶
                                        } else {
                                            off_pre <= px(-f32::from(max) + EPS) // 已在底
                                        };
                                        if !at_edge {
                                            cx.stop_propagation();
                                        }
                                    }
                                })
                                // 此处**不得** occlude()：gpui hit_test 命中
                                // BlockMouse 即 break，会把祖先（本行的
                                // bar_hover_wired）一起挡掉 → 鼠标停在气泡上
                                // 操作栏永不显影（v62 修复）。滚轮也一并受益：
                                // 气泡内滚到底后能继续滚会话列表。
                                .px(px(12.))
                                .pr(px(14.))
                                .py(px(8.))
                                .rounded(px(12.))
                                .bg(rgb(t.user_bg))
                                .border_1()
                                .border_color(gpui::rgba(theme::rgba_a(0x3b82f6, 0.2)))
                                .text_color(rgb(t.text))
                                .child(div().flex().flex_col().child(md)),
                        )
                        // 滚动条仅在实际溢出限高时渲染（max_offset>0
                        // = 内容超高；未溢出无条）
                        .children((scroll_handle.max_offset().height > px(0.)).then(|| {
                            div()
                                .absolute()
                                .top(px(8.))
                                .bottom(px(8.))
                                .right(px(3.))
                                .w(px(8.))
                                .child(
                                    gpui_component::scroll::Scrollbar::vertical(
                                        &scroll_state,
                                        &scroll_handle,
                                    )
                                    .scroll_size(gpui::size(
                                        px(0.),
                                        px(300.) + scroll_handle.max_offset().height,
                                    )),
                                )
                        })),
                ),
            );
        }
        row = row.child(bottom);
        col = col.child(row);
    }
    // 非 User 臂已删：mod.rs 分发入口只在 m.role == Role::User 时调
    // render_msg（assistant 轮走 render_assistant_turn、bash 走 render_bash_msg），
    // 曾有的「单条 assistant」防御分支不可达，见 pzy 拆分前清理。
    col
}

/// Role::Bash 渲染（mod.rs 分发入口；pi-web BashExecutionView parity）：
/// 合成 toolCall 卡片复用 render_tool_card（bash 卡 = 命令预览 + 结果体），
/// 执行中加一行「执行中」状态。独立消息行，不进轮分组。
pub(crate) fn render_bash_msg(
    m: &Msg,
    msg_ix: usize,
    t: &theme::Theme,
    collapsed: &HashMap<(usize, usize), bool>,
    weak: &gpui::WeakEntity<Chat>,
) -> gpui::Div {
    let mut col = div().w_full().mb(px(16.)).flex().flex_col();
    let Some(info) = &m.bash else {
        return col;
    };
    // pi-web：excluded 的命令显示 "bash (local)"（输出仅本地，不进模型）
    let name = if info.excluded {
        "bash (local)"
    } else {
        "bash"
    };
    let args = serde_json::json!({ "command": info.command }).to_string();
    // is_error 对齐 agent 的 bash 工具卡：非零退出码 = 红（取消不算）
    let is_error = !info.cancelled && info.exit_code.is_some_and(|c| c != 0);
    let done = !info.running;
    // pi-web bashExecutionToText 的后缀行（展示即模型所见）
    let mut result = info.output.trim_end().to_string();
    if result.is_empty() {
        result = "(no output)".into();
    }
    if info.cancelled {
        result.push_str("\n(command cancelled)");
    } else if let Some(code) = info.exit_code {
        if code != 0 {
            result.push_str(&format!("\nCommand exited with code {code}"));
        }
    }
    if info.truncated {
        result.push_str(&format!(
            "\n[Output truncated. Full output: {}]",
            info.full_output_path.as_deref().unwrap_or("")
        ));
    }
    let duration_s = m.end_ts.zip(m.ts).map(|(e, s)| (e - s).max(0) / 1000);
    col = col.child(render_tool_card(
        msg_ix,
        0,
        name,
        &args,
        &result,
        is_error,
        &[],
        None,
        duration_s,
        false,
        done,
        weak,
        collapsed,
        t,
    ));
    if info.running {
        col = col.child(
            div()
                .mt(px(4.))
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(crate::appearance::sess_size(-3.))
                .text_color(rgb(t.text_dim))
                .child(
                    gpui::svg()
                        .path(SharedString::from("icons/loader.svg"))
                        .text_color(rgb(t.accent))
                        .size(px(11.))
                        .with_animation(
                            SharedString::from(format!("bash-spin-{msg_ix}")),
                            Animation::new(std::time::Duration::from_millis(900)),
                            |el, delta| {
                                el.with_transformation(gpui::Transformation::rotate(gpui::radians(
                                    delta * 2. * std::f32::consts::PI,
                                )))
                            },
                        )
                        .into_any_element(),
                )
                .child(SharedString::from(tr("composer.running_hint"))),
        );
    }
    col
}

/// Block 渲染可见性（组内条目计数/过滤用；空 thinking/空 text 跳过）。
/// 思考块永远可见（收起=一行），不随「展示思考」开关消失。

#[cfg(test)]
mod tests {
    use super::*;

    /// 回归（图片上下留白）：宽图受 max_h 约束时，绘制框必须同比例缩放，
    /// 不能把宽定成 720、高定成 520 —— 那样 Contain 会在盒内再留白。
    #[test]
    fn fit_box_keeps_aspect_ratio_inside_max() {
        // 2:1 宽图 → 受宽度约束（720×360，高度还远没到 520）
        assert_eq!(fit_box(2880, 1440, 720., 520.), (720., 360.));
        // 16:9 宽图 → 受宽度约束，高 405 < 520
        assert_eq!(fit_box(1920, 1080, 720., 520.), (720., 405.));
        // 竖图 → 受高度约束
        assert_eq!(fit_box(1000, 4000, 720., 520.), (130., 520.));
        // 小图不放大小（保持 gpui 原生像素尺寸）
        assert_eq!(fit_box(100, 50, 720., 520.), (100., 50.));
        // 退化输入不 panic
        assert_eq!(fit_box(0, 0, 720., 520.), (720., 520.));
    }

    #[test]
    fn natural_size_parses_png_jpeg_gif_bmp_webp() {
        // PNG 8 字节签名 + IHDR
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&1920u32.to_be_bytes());
        png.extend_from_slice(&1080u32.to_be_bytes());
        png.extend_from_slice(&[0; 20]);
        assert_eq!(
            image_natural_size(&png, gpui::ImageFormat::Png),
            Some((1920, 1080))
        );

        // JPEG：SOI + APP0 段 + SOF0
        let mut jpg = vec![0xFF, 0xD8, 0xFF, 0xE0];
        jpg.extend_from_slice(&16u16.to_be_bytes());
        jpg.extend_from_slice(&[0; 14]);
        jpg.extend_from_slice(&[0xFF, 0xC0]);
        jpg.extend_from_slice(&17u16.to_be_bytes());
        jpg.push(8);
        jpg.extend_from_slice(&1080u16.to_be_bytes());
        jpg.extend_from_slice(&1920u16.to_be_bytes());
        jpg.push(3);
        jpg.extend_from_slice(&[0; 9]);
        assert_eq!(
            image_natural_size(&jpg, gpui::ImageFormat::Jpeg),
            Some((1920, 1080))
        );

        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&320u16.to_le_bytes());
        gif.extend_from_slice(&240u16.to_le_bytes());
        gif.extend_from_slice(&[0; 8]);
        assert_eq!(
            image_natural_size(&gif, gpui::ImageFormat::Gif),
            Some((320, 240))
        );

        let mut bmp = b"BM".to_vec();
        bmp.extend_from_slice(&[0; 16]);
        bmp.extend_from_slice(&(-800i32).to_le_bytes());
        bmp.extend_from_slice(&600i32.to_le_bytes());
        assert_eq!(
            image_natural_size(&bmp, gpui::ImageFormat::Bmp),
            Some((800, 600))
        );

        let mut vp8 = b"RIFF".to_vec();
        vp8.extend_from_slice(&[0; 4]);
        vp8.extend_from_slice(b"WEBP");
        vp8.extend_from_slice(b"VP8 ");
        vp8.extend_from_slice(&[0; 4]);
        vp8.extend_from_slice(&[0x30, 0x01, 0x00]); // 帧 tag
        vp8.extend_from_slice(&[0x9d, 0x01, 0x2a]); // 同步码
        vp8.extend_from_slice(&1280u16.to_le_bytes());
        vp8.extend_from_slice(&720u16.to_le_bytes());
        assert_eq!(
            image_natural_size(&vp8, gpui::ImageFormat::Webp),
            Some((1280, 720))
        );

        assert_eq!(image_natural_size(b"<svg/>", gpui::ImageFormat::Svg), None);
    }
}

#[cfg(test)]
mod skill_fold_tests {
    use super::skill_expansion_to_command;

    // pi-web lib/slash-display.test.mjs skillExpansion fixture 同款
    fn envelope(body: &str, args: Option<&str>) -> String {
        let args_s = args.map(|a| format!("\n\n{}", a)).unwrap_or_default();
        format!(
            "<skill name=\"review\" location=\"/path/to/review/SKILL.md\">\nReferences are relative to /path/to/review.\n\n{body}\n</skill>{args}",
            body = body,
            args = args_s,
        )
    }

    #[test]
    fn restores_with_args() {
        assert_eq!(
            skill_expansion_to_command(&envelope(
                "Review the supplied files.",
                Some("src/main.ts")
            ))
            .as_deref(),
            Some("/skill:review src/main.ts")
        );
    }
    #[test]
    fn restores_without_args() {
        assert_eq!(
            skill_expansion_to_command(&envelope("Review the supplied files.", None)).as_deref(),
            Some("/skill:review")
        );
    }
    #[test]
    fn multiline_args() {
        assert_eq!(
            skill_expansion_to_command(&envelope("Body.", Some("first line\nsecond line")))
                .as_deref(),
            Some("/skill:review first line\nsecond line")
        );
    }
    #[test]
    fn final_closing_tag_wins() {
        assert_eq!(
            skill_expansion_to_command(&envelope("Example:\n</skill>\nContinue.", Some("src")))
                .as_deref(),
            Some("/skill:review src")
        );
    }

    #[test]
    fn lookalike_not_collapsed() {
        assert_eq!(
            skill_expansion_to_command(
                "<skill name=\"review\" location=\"/path/to/review/SKILL.md\">\nordinary user text"
            ),
            None
        );
        assert_eq!(
            skill_expansion_to_command(
                "<skill name=\"review\" location=\"/path/to/review/SKILL.md\">\nReferences are elsewhere.\n\nbody\n</skill>"
            ),
            None
        );
        assert_eq!(skill_expansion_to_command("ordinary user text"), None);
    }
}

#[cfg(test)]
mod bar_hover_hit_test {
    //! 用户消息行操作栏悬停（v62 修复）的 gpui 几何前提锁。
    //!
    //! `bar_hover_wired` 把 on_hover 挂在**整行**上，靠的是「行内后代不遮挡」
    //! 这条 gpui 语义：`Window::hit_test` 一旦命中 `HitboxBehavior::BlockMouse`
    //! （`div().occlude()`）就 `break`，被它挡住的祖先 hitbox 连 `ids` 都进不
    //! 去，`is_hovered()` 恒 false。用户气泡曾挂 `occlude()`，于是鼠标停在气泡
    //! 上时行级 on_hover 永不触发 → 操作栏不显影；agent 轮块无遮挡后代故一切
    //! 正常（同一个行为两处表现）。
    //!
    //! 两侧都锁：不遮挡 → 行悬停必须触发（线上行为）；遮挡 → 必须被挡掉（gpui
    //! 现语义；上游若改，这条先炸，提示回来复核用户行实现与上面那段注释）。

    use std::cell::Cell;
    use std::rc::Rc;

    use gpui::{
        AppContext, Context, IntoElement, Modifiers, ParentElement, Render, Styled, TestAppContext,
        VisualTestContext, Window, div, point, prelude::*, px, size,
    };

    /// 迷你复刻用户消息行：row = `bar_hover_wired` 的挂载点，bubble = 气泡本体
    /// （`bubble_occludes` 开关模拟 v62 前后两种写法）。
    struct Row {
        row_hovered: Rc<Cell<usize>>,
        bubble_hovered: Rc<Cell<usize>>,
        bubble_occludes: bool,
    }

    impl Render for Row {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let row = self.row_hovered.clone();
            let bubble = self.bubble_hovered.clone();
            let bubble_el = div()
                .id("bubble")
                .when(self.bubble_occludes, |d| d.occlude())
                .w(px(100.))
                .h(px(40.))
                .on_hover(move |hovered, _, _| {
                    if *hovered {
                        bubble.set(bubble.get() + 1);
                    }
                });
            div()
                .id("row")
                .w(px(400.))
                .h(px(100.))
                .on_hover(move |hovered, _, _| {
                    if *hovered {
                        row.set(row.get() + 1);
                    }
                })
                .child(bubble_el)
        }
    }

    /// 画一帧后把指针移到气泡正中，返回 (行 on_hover 次数, 气泡 on_hover 次数)
    fn hover_bubble(cx: &mut TestAppContext, bubble_occludes: bool) -> (usize, usize) {
        let window: &mut VisualTestContext = cx.add_empty_window();
        let row_hovered: Rc<Cell<usize>> = Default::default();
        let bubble_hovered: Rc<Cell<usize>> = Default::default();
        let row = row_hovered.clone();
        let bubble = bubble_hovered.clone();
        let _ = window.draw(
            point(px(0.), px(0.)),
            size(px(400.), px(100.)),
            move |_, cx| {
                cx.new(|_| Row {
                    row_hovered: row.clone(),
                    bubble_hovered: bubble.clone(),
                    bubble_occludes,
                })
            },
        );
        window.simulate_mouse_move(point(px(50.), px(20.)), None, Modifiers::none());
        (row_hovered.get(), bubble_hovered.get())
    }

    /// 正例（v62 线上行为）：气泡不遮挡 → 行悬停触发 → `Chat.bar_hover` 置位
    /// → 操作栏显影，与 agent 轮块同构。
    #[gpui::test]
    fn bubble_without_occluder_lets_row_hover_fire(cx: &mut TestAppContext) {
        let (row, bubble) = hover_bubble(cx, false);
        assert_eq!(bubble, 1, "指针在气泡内，气泡自身必须 hover");
        assert_eq!(row, 1, "行级 on_hover 必须触发——缺了它操作栏就不显影");
    }

    /// 反例（v62 之前的线上行为，勿再退化）：气泡挂 `occlude()` → 行 hitbox 被
    /// 截断在 hit-test 之外 → 行级 on_hover 永不触发 → 操作栏不显影。
    #[gpui::test]
    fn bubble_occluder_swallows_row_hover(cx: &mut TestAppContext) {
        let (row, bubble) = hover_bubble(cx, true);
        assert_eq!(bubble, 1);
        assert_eq!(
            row, 0,
            "occlude 会连祖先一起截断（gpui 语义若变，此断言先炸）"
        );
    }
}

#[cfg(test)]
mod bubble_wheel {
    //! 用户气泡滚轮归属（v63-9）的回归锁。
    //!
    //! 现象：多行用户消息限高 300px 出滚动条，滚轮却「滚不动气泡，只有整屏
    //! 跟着滚」。根因不在气泡，在 gpui `List` 的滚轮注册时机——
    //! `List::paint` 原本先 paint 条目、**末尾**才 `on_mouse_event` 注册自己的
    //! 滚轮，而冒泡相位按注册**逆序**派发（后注册先收到）⇒ 列表比条目先滚，
    //! 条目内滚动容器的边界拦截（`cx.stop_propagation()`）永远晚一步，同帧
    //! 双层一起滚，气泡相对屏幕位置不变 = 看起来完全没滚。
    //!
    //! vendor 补丁把注册挪到条目循环之前（`elements/list.rs`），本测盯死语义：
    //! 内层没到边界就只滚内层、列表原地不动；内层到边界才轮到列表。
    use gpui::{
        AppContext, Context, IntoElement, ListAlignment, ListState, Modifiers, ParentElement,
        Render, ScrollDelta, ScrollHandle, ScrollWheelEvent, Styled, TestAppContext,
        VisualTestContext, Window, div, point, prelude::*, px, size,
    };

    const VP: f32 = 600.;
    /// 气泡限高（与 render_msg 的 USER_BUBBLE_MAX_HEIGHT 同值）
    const BUBBLE_H: f32 = 300.;

    struct View {
        state: ListState,
        handle: ScrollHandle,
    }

    impl Render for View {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let handle = self.handle.clone();
            div().w_full().h(px(VP)).child(
                gpui::list(self.state.clone(), move |ix, _, _| {
                    let sh = handle.clone();
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .id(("bubble", ix))
                                .h(px(BUBBLE_H))
                                .overflow_y_scroll()
                                .track_scroll(&sh)
                                // 与 turn.rs render_msg 里的边界拦截同构
                                .on_scroll_wheel({
                                    let sh = sh.clone();
                                    move |ev: &ScrollWheelEvent, window, cx| {
                                        let max = sh.max_offset().height;
                                        if max <= px(0.) {
                                            return;
                                        }
                                        let delta_y = ev.delta.pixel_delta(window.line_height()).y;
                                        if delta_y == px(0.) {
                                            return;
                                        }
                                        // 同 div 内置滚动先跑，offset 已含本次 delta
                                        let off_pre = sh.offset().y - delta_y;
                                        const EPS: f32 = 1.;
                                        let at_edge = if delta_y > px(0.) {
                                            off_pre >= px(-EPS)
                                        } else {
                                            off_pre <= px(-f32::from(max) + EPS)
                                        };
                                        if !at_edge {
                                            cx.stop_propagation();
                                        }
                                    }
                                })
                                .child(div().h(px(1000.))),
                        )
                        // 气泡下垫高，让列表本身也有滚动余量
                        .child(div().h(px(900.)))
                        .into_any_element()
                })
                .h_full(),
            )
        }
    }

    /// 画两帧（第一帧只测量条目，句柄的 max_offset 在第二帧才可信）后滚一次
    /// 滚轮，返回（气泡偏移, 列表逻辑位）。
    fn wheel(
        window: &mut VisualTestContext,
        state: &ListState,
        handle: &ScrollHandle,
        delta_y: f32,
    ) -> (f32, ListOffsetProbe) {
        window.simulate_mouse_move(point(px(100.), px(100.)), None, Modifiers::none());
        window.simulate_event(ScrollWheelEvent {
            position: point(px(100.), px(100.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(delta_y))),
            ..Default::default()
        });
        let lt = state.logical_scroll_top();
        (
            f32::from(handle.offset().y),
            ListOffsetProbe {
                item_ix: lt.item_ix,
                offset_in_item: f32::from(lt.offset_in_item),
            },
        )
    }

    /// 再画一帧：滚轮写进偏移后，超量部分由下一帧 prepaint 夹回 [-max, 0]
    fn repaint(window: &mut VisualTestContext, state: &ListState, handle: &ScrollHandle) {
        let (s, h) = (state.clone(), handle.clone());
        window.draw::<gpui::Entity<View>>(
            point(px(0.), px(0.)),
            size(px(400.), px(VP)),
            move |_, cx| {
                cx.new(|_| View {
                    state: s.clone(),
                    handle: h.clone(),
                })
            },
        );
    }

    #[derive(Debug, PartialEq)]
    struct ListOffsetProbe {
        item_ix: usize,
        offset_in_item: f32,
    }

    fn setup(cx: &mut TestAppContext) -> (&mut VisualTestContext, ListState, ScrollHandle) {
        let window: &mut VisualTestContext = cx.add_empty_window();
        let state = ListState::new(1, ListAlignment::Top, px(1000.));
        let handle = ScrollHandle::new();
        let (s, h) = (state.clone(), handle.clone());
        for _ in 0..2 {
            let (s, h) = (s.clone(), h.clone());
            window.draw::<gpui::Entity<View>>(
                point(px(0.), px(0.)),
                size(px(400.), px(VP)),
                move |_, cx| {
                    cx.new(|_| View {
                        state: s.clone(),
                        handle: h.clone(),
                    })
                },
            );
        }
        assert!(
            handle.max_offset().height > px(0.),
            "气泡必须真的溢出限高（否则本测在测空气）"
        );
        (window, state, handle)
    }

    /// 正例（v63-9 线上行为）：气泡内部还有余量时滚轮只滚气泡，会话列表不动。
    #[gpui::test]
    fn wheel_over_tall_bubble_does_not_drag_the_list(cx: &mut TestAppContext) {
        let (window, state, handle) = setup(cx);
        let (bubble, list) = wheel(window, &state, &handle, -50.);
        assert_eq!(bubble, -50., "气泡自己必须滚起来");
        assert_eq!(
            list,
            ListOffsetProbe {
                item_ix: 0,
                offset_in_item: 0.
            },
            "气泡没到边界，会话列表不得同帧跟着滚（穿透 = 用户看到的「滚不动」）"
        );
    }

    /// 气泡滚到底后，滚轮才交给列表——原生嵌套滚动语义。
    #[gpui::test]
    fn wheel_hands_over_to_list_only_at_bubble_edge(cx: &mut TestAppContext) {
        let (window, state, handle) = setup(cx);
        let max = f32::from(handle.max_offset().height);
        // 一记正好到底：气泡吃掉这一记，列表还是不动
        let (bubble, list) = wheel(window, &state, &handle, -max);
        repaint(window, &state, &handle);
        assert_eq!(f32::from(handle.offset().y), -max, "气泡停在下限");
        assert_eq!(bubble, -max);
        assert_eq!(
            list,
            ListOffsetProbe {
                item_ix: 0,
                offset_in_item: 0.
            }
        );

        // 已在底部：下一记轮到列表（气泡仍钉在下限）
        let (_, list) = wheel(window, &state, &handle, -120.);
        repaint(window, &state, &handle);
        assert_eq!(f32::from(handle.offset().y), -max, "气泡仍在底部");
        assert_eq!(list.offset_in_item, 120., "到边界后列表接手滚轮");
        assert_eq!(list.item_ix, 0);

        // 往上滚：光标仍在气泡上、气泡有上行余量 → 气泡先滚，列表不动
        let (bubble, list) = wheel(window, &state, &handle, 60.);
        assert_eq!(bubble, -max + 60.);
        assert_eq!(
            list,
            ListOffsetProbe {
                item_ix: 0,
                offset_in_item: 120.
            }
        );
    }
}
