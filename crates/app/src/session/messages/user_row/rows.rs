//! 用户消息 Block 渲染（render_block）、技能展开命令推导与
//! bar_hover_wired 接线构件。

use super::*;

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
    // 消息起始时间戳（epoch ms）：子代理卡运行中 live 时长的锚
    //（044-0 验收⑤）；其余块不消费
    msg_ts: Option<i64>,
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
                msg_ts,
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
