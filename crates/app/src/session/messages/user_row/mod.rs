//! 用户消息行（032）：[`render_block`] 块分发（text/thinking/image/工具
//! 简卡）、skill 折叠命令还原、操作栏悬停接线、[`render_msg`] 用户行、
//! [`render_bash_msg`]。自 turn.rs 拆出（090 重构）；三个 tests mod 随迁。

use super::*;
use std::collections::HashMap;

use gpui::{MouseButton, SharedString, div, prelude::*, px, relative, rgb};
use pi_link::protocol::Block;

use crate::Chat;
use crate::appearance::theme;
use crate::editor::markdown;
use crate::i18n::tr;
use crate::session::actions_bar::user_action_bar;
use crate::ui::icon;

// itlq.5 拆分（只搬不改）：rows = Block 渲染与技能展开/hover 接线；
// tests = 内嵌测试外移。
pub(crate) mod rows;
pub(crate) use rows::{bar_hover_wired, render_block, skill_expansion_to_command};

#[cfg(test)]
mod tests;

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
    // 只读观察窗（044-0）：消息操作栏随输入面板一并隐藏
    read_only: bool,
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

        let bottom = if read_only {
            None
        } else {
            Some(user_action_bar(msg_ix, weak, m.ts, bar_revealed, copied, t))
        };
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
        if let Some(bottom) = bottom {
            row = row.child(bottom);
        }
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
        None,
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
                    // 共用 ui::spinner（loader.svg + repeat() + with_fps(24)）：
                    // 旧实现把同一个弧线 spinner 内联手写，漏了 `.repeat()` →
                    // 转一圈就停（观感 bug），也没上帧率限速阀（repeat() 动画
                    // 不限帧 = 整窗满帧率重绘，见 docs/agents/gpui.md
                    // 「repeat() 动画不限帧」）。走 helper 后一处定义一处改。
                    crate::ui::spinner(11., t.accent),
                )
                .child(SharedString::from(tr("composer.running_hint"))),
        );
    }
    col
}
