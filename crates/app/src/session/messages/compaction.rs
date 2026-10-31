//! 压缩卡与自定义消息卡（052）：compaction summary 解析、
//! [`render_compaction_card`]、[`render_custom_msg`]。自 cards.rs 拆出（090 重构）。

use super::*;

use std::collections::HashMap;

use gpui::{FontWeight, MouseButton, SharedString, div, prelude::*, px, relative, rgb};

use crate::Chat;
use crate::appearance::theme;
use crate::editor::markdown;
use crate::i18n::{tf, tr};
use crate::ui::icon;

/// pi-web parseCompactionSummary parity：剥离尾部的
/// <read-files>/<modified-files> 段，返回 (body, read, modified)。
fn parse_compaction_summary(summary: &str) -> (String, Vec<String>, Vec<String>) {
    let lines: Vec<&str> = summary.lines().collect();
    let mut spans: Vec<(usize, usize, usize)> = Vec::new(); // (kind, start, end)
    let mut k = 0usize;
    while k < lines.len() {
        let t = lines[k].trim();
        let kind = if t == "<read-files>" {
            0
        } else if t == "<modified-files>" {
            1
        } else {
            k += 1;
            continue;
        };
        let close = if kind == 0 {
            "</read-files>"
        } else {
            "</modified-files>"
        };
        let mut j = k + 1;
        while j < lines.len() && lines[j].trim() != close {
            j += 1;
        }
        if j < lines.len() {
            spans.push((kind, k, j));
            k = j + 1;
        } else {
            k += 1;
        }
    }
    // 尾段：从文档末尾反向取连续（只隔空行）的 section 段
    let mut read = Vec::new();
    let mut modified = Vec::new();
    let mut body_end = lines.len();
    let mut cursor = lines.len();
    for (kind, start, end) in spans.iter().rev() {
        if *end != cursor - 1 {
            break;
        }
        // 段与段之间（或首段之前）只允许空行
        if cursor < lines.len() {
            if !lines[*end + 1..cursor].iter().all(|l| l.trim().is_empty()) {
                break;
            }
        }
        if *start > 0 && !lines[*start - 1].trim().is_empty() {
            break;
        }
        let files: Vec<String> = lines[*start + 1..*end]
            .iter()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        if *kind == 0 {
            read.extend(files);
        } else {
            modified.extend(files);
        }
        body_end = *start;
        cursor = *start;
    }
    let body = lines[..body_end].join("\n").trim().to_string();
    (body, read, modified)
}

/// Role::Custom 渲染（v56-6 c28；mod.rs 分发入口）。
pub(crate) fn render_custom_msg(
    m: &Msg,
    msg_ix: usize,
    t: &theme::Theme,
    collapsed: &HashMap<(usize, usize), bool>,
    weak: &gpui::WeakEntity<Chat>,
) -> gpui::Div {
    let mut col = div().w_full().mb(px(16.)).flex().flex_col();
    let custom_type = m.custom_type.as_deref().unwrap_or("");
    match custom_type {
        "compaction" => col = col.child(render_compaction_card(m, msg_ix, t, collapsed, weak)),
        "branch_summary" => {
            // pi-web 将 branch_summary 渲为 user 气泡（斜体引言+摘要）；
            // 这里为保持 fork 锚点对齐保留 Custom 角色，渲染为斜体引言 +
            // 摘要 markdown（已知偏差）
            let summary = m.plain_text();
            col = col.child(
                div()
                    .italic()
                    .text_color(rgb(t.text_muted))
                    .text_size(crate::appearance::ui_size(13.))
                    .mb(px(4.))
                    .child(SharedString::from(tr("branch.detour_note"))),
            );
            if !summary.trim().is_empty() {
                col = col.child(markdown::render(&summary, t, false));
            }
        }
        _ if !m.custom_display => {
            // pi-web display:false：折叠暗卡 + 140 字符预览
            let preview: String = m
                .plain_text()
                .replace('\n', " ")
                .chars()
                .take(140)
                .collect();
            let preview = if preview.is_empty() {
                tr("messages.extended_hidden").to_string()
            } else {
                format!("{}...", preview)
            };
            col = col.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(10.))
                    .py(px(6.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(t.border))
                    .bg(rgb(t.bg))
                    .opacity(0.82)
                    .text_size(crate::appearance::ui_size(12.))
                    .text_color(rgb(t.text_muted))
                    .child(
                        div()
                            .font_family(crate::editor::markdown::MONO_FAMILY)
                            .text_size(crate::appearance::ui_size(11.))
                            .child(SharedString::from(if custom_type.is_empty() {
                                "extension".to_string()
                            } else {
                                custom_type.to_string()
                            })),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(SharedString::from(preview)),
                    ),
            );
        }
        _ => {
            // pi-web CustomMessageView 简化 parity：customType 头 + markdown 正文
            let body = m.plain_text();
            let card = div()
                .border_1()
                .border_color(rgb(t.border))
                .rounded(px(8.))
                .overflow_hidden()
                .bg(rgb(t.bg))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(10.))
                        .py(px(7.))
                        .border_b_1()
                        .border_color(rgb(t.border))
                        .bg(rgb(t.bg_panel))
                        .text_color(rgb(t.text_muted))
                        .child(
                            div()
                                .font_family(crate::editor::markdown::MONO_FAMILY)
                                .text_size(crate::appearance::ui_size(11.))
                                .child(SharedString::from(if custom_type.is_empty() {
                                    "extension".to_string()
                                } else {
                                    custom_type.to_string()
                                })),
                        ),
                )
                .child(
                    div()
                        .px(px(13.))
                        .py(px(11.))
                        .child(markdown::render(&body, t, false)),
                );
            col = col.child(card);
        }
    }
    col
}

/// pi-web CompactionMessageView parity（v56-6 c28）。
fn render_compaction_card(
    m: &Msg,
    msg_ix: usize,
    t: &theme::Theme,
    collapsed: &HashMap<(usize, usize), bool>,
    weak: &gpui::WeakEntity<Chat>,
) -> gpui::Div {
    let (body, read_files, modified_files) = parse_compaction_summary(&m.plain_text());
    let card = div()
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(8.))
        .overflow_hidden()
        .bg(rgb(t.bg))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(10.))
                .py(px(7.))
                .border_b_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.bg_panel))
                .text_color(rgb(t.text_muted))
                .child(
                    div()
                        .font_family(crate::editor::markdown::MONO_FAMILY)
                        .text_size(crate::appearance::ui_size(11.))
                        .child(SharedString::from("compaction")),
                )
                .child(
                    div()
                        .ml_auto()
                        .text_size(crate::appearance::ui_size(10.))
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(
                            m.ts.map(crate::services::format::fmt_msg_time)
                                .unwrap_or_default(),
                        )),
                ),
        );
    // 正文容器（pi-web <div style={{padding:"11px 13px 12px"}}>）：
    // 文件元数据必须挂在这层里面；挂到卡片根上会让分隔线和清单框
    // 顶掉左右 13px 内边距，直接贴住卡片边框
    let mut body = div()
        .px(px(13.))
        .pt(px(11.))
        .pb(px(12.))
        .child(
            div()
                .text_size(crate::appearance::ui_size(15.))
                .font_weight(FontWeight::BOLD)
                .line_height(relative(1.35))
                .text_color(rgb(t.text))
                .child(SharedString::from(tr("compaction.done"))),
        )
        .child(
            div()
                .mt(px(3.))
                .mb(px(10.))
                .text_size(crate::appearance::ui_size(14.))
                .line_height(relative(1.5))
                .text_color(rgb(t.text))
                .child(SharedString::from(tr("compaction.summary_intro"))),
        )
        .child(if body.is_empty() {
            div()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(tr("compaction.no_summary")))
                .into_any_element()
        } else {
            markdown::render(&body, t, false)
        });

    // 文件元数据（pi-web CompactionFileMetadata parity）：
    // <details> 默认收起，summary 行 =「文件上下文：N 读取，M 修改」，
    // 顶线分隔；展开后按"修改/读取"分节，清单是带边框底色的等宽框
    let total = read_files.len() + modified_files.len();
    if total > 0 {
        let mut parts: Vec<String> = Vec::new();
        if !read_files.is_empty() {
            parts.push(tf(
                "compaction.reads",
                &[("n", read_files.len().to_string())],
            ));
        }
        if !modified_files.is_empty() {
            parts.push(tf(
                "compaction.writes",
                &[("n", modified_files.len().to_string())],
            ));
        }
        // 折叠位复用 runtime.collapsed；usize::MAX 作content_index，
        // 避开同一消息里工具调用用的 (msg_ix, content_index) 键
        let key = (msg_ix, usize::MAX);
        let open = collapsed.get(&key).copied().unwrap_or(false);
        let weak_toggle = weak.clone();
        let label = tf("compaction.file_context", &[("details", parts.join(", "))]);
        let mut meta = div()
            .mt(px(10.))
            .pt(px(8.))
            .border_t_1()
            .border_color(rgb(t.border))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let _ = weak_toggle.update(cx, |c, cx| {
                            let rt = c.rt();
                            rt.update(cx, |r, _| {
                                let next = !r.collapsed.get(&key).copied().unwrap_or(false);
                                r.collapsed.insert(key, next);
                            });
                            cx.notify();
                        });
                    })
                    .child(
                        // pi-web 浏览器 <summary> 原生三角（summary 12px muted
                        // 同款尺寸）——原 10px text_faint 字形小到看不清
                        icon(
                            if open {
                                "chevron-down"
                            } else {
                                "chevron-right"
                            },
                            12.,
                            t.text_muted,
                        ),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .text_size(crate::appearance::ui_size(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .line_height(relative(1.4))
                            .text_color(rgb(t.text_muted))
                            .child(SharedString::from(label.clone())),
                    ),
            );

        if open {
            for (title, files) in [
                (tr("compaction.modified_files"), &modified_files),
                (tr("compaction.read_files"), &read_files),
            ] {
                if files.is_empty() {
                    continue;
                }
                let list = div()
                    .id(SharedString::from(format!("cfiles-{msg_ix}-{title}")))
                    .max_h(px(180.))
                    .overflow_y_scroll()
                    .py(px(7.))
                    .px(px(8.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(t.border))
                    .bg(rgb(t.bg_panel))
                    .flex()
                    .flex_col()
                    .gap(px(3.)) // li + li { margin-top: 3px }
                    .children(files.iter().map(|f| {
                        div()
                            .font_family(crate::editor::markdown::MONO_FAMILY)
                            .text_size(crate::appearance::ui_size(11.))
                            .line_height(relative(1.45))
                            .text_color(rgb(t.text_muted))
                            .child(SharedString::from(f.clone()))
                    }));
                meta = meta.child(
                    div()
                        .mt(px(8.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .mb(px(4.))
                                .text_size(crate::appearance::ui_size(11.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(t.text))
                                .child(SharedString::from(tr(title))),
                        )
                        .child(list),
                );
            }
        }
        body = body.child(meta);
    }
    card.child(body)
}
