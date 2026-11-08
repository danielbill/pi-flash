//! 设置「界面」页 (v60)：主题（淡色5 + 深色2 各一排）/ 字体（三槽位：族
//! 下拉 + 字号三档下拉）/ 内容显示（展示思考、文件树 Git 标识两开关，
//! 无小节标题——用户定夺多余）/ 语言（三个一排，置底——用户定夺）。
//! 主题持久化只写 pi-flash 自己的 app_settings.json——pi 的 settings.json
//! 有同名的 theme 键（值域 dark/light），写入 pi-flash 主题 id 会让 pi
//! 每次启动报错。
//!
//! itlq.2 拆分（只搬不改）：本文件 = 页骨架 + 主题/语言行；fonts = 字体
//! 行与三档下拉弹卡。

use super::*;
use crate::appearance::theme;
use crate::appearance::{FontSlot, ThemeEntry};

pub(crate) mod fonts;
use fonts::font_row;

/// 槽位序号 → FontSlot（族/字号下拉共用）。
fn slot_of(ix: usize) -> FontSlot {
    match ix {
        0 => FontSlot::Session,
        1 => FontSlot::Panel,
        _ => FontSlot::File,
    }
}

fn slot_font(ix: usize) -> crate::services::workspace::FontSpec {
    match ix {
        0 => crate::appearance::session_font(),
        1 => crate::appearance::panel_font(),
        _ => crate::appearance::file_font(),
    }
}

pub(crate) fn mc_general_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    font_popup: Option<usize>,
    font_dd: &gpui::Entity<DropdownState>,
    font_filter: &gpui::Entity<TextInput>,
    font_filter_value: &str,
    size_popup: Option<usize>,
    size_dd: &gpui::Entity<DropdownState>,
) -> gpui::AnyElement {
    let t = T();
    let _ = chat;
    let mut col = div().flex().flex_col().pr(px(6.));

    // ---- 主题：淡色一排 / 深色一排 --------------------------------------
    col = col.child(section_label(tr("settings.theme")));
    col = col.child(theme_row(false, weak));
    col = col.child(theme_row(true, weak));

    // ---- 字体：三槽位（族下拉 250px + 字号步进 + 预览行） ----------------
    col = col.child(section_label(tr("settings.fonts")));
    for (ix, label) in [
        (0usize, tr("settings.chat_font")),
        (1, tr("settings.panel_font")),
        (2, tr("settings.file_font")),
    ] {
        col = col.child(font_row(
            ix,
            label,
            weak,
            font_dd.clone(),
            font_filter.clone(),
            font_filter_value,
            font_popup == Some(ix),
            size_popup == Some(ix),
            size_dd,
            t,
        ));
    }

    // ---- 内容显示：会话/文件树的显示开关（自「其他」页挪入；不加小节
    // 标题——用户定夺多余，行内标题+描述已自明） ----------------------------
    // 首行外包 mt(7)：补齐与 section_label mt(20) 同款的小节间距（set_row
    // 自带 py(13)）
    // 会话内容宽度（消息列 + composer 胶囊同源；pi-web chatContentWidth
    // parity）：拖动即实时预览（消息列与胶囊同步变宽/变窄），松手写盘。
    // 无复位钮（用户定夺：冗余——再拖回去即可，不存在不可逆后果）
    let chat_w = crate::services::workspace::chat_content_width();
    let w_min = crate::services::workspace::CHAT_WIDTH_MIN as f32;
    let w_max = crate::services::workspace::CHAT_WIDTH_MAX as f32;
    let w_step = crate::services::workspace::CHAT_WIDTH_STEP as f32;
    col = col.child(
        div().mt(px(7.)).child(set_row(
            tr("settings.chat_width"),
            tr("settings.chat_width_hint"),
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .min_w(px(56.))
                        .flex()
                        .justify_end()
                        .font_family(crate::editor::markdown::MONO_FAMILY)
                        .text_size(crate::appearance::ui_size(12.))
                        .text_color(rgb(t.text_muted))
                        .child(SharedString::from(format!("{} px", chat_w as u32))),
                )
                .child(width_slider(
                    "general-chat-width",
                    chat_w,
                    w_min,
                    w_max,
                    w_step,
                    weak,
                ))
                // 复位钮已拨（用户定夺：冗余——再拖回来即可，无不可逆后果）
                .into_any_element(),
            t,
        )),
    );
    // 展示思考 switch（思考块始终渲染；关（默认）=新思考块收成一行，
    // 开=默认展开全文。用户手动开合过的块以显式状态为准）
    let show_thinking = crate::services::workspace::show_thinking();
    col = col.child(set_row(
        tr("settings.show_thinking"),
        tr("settings.show_thinking_hint"),
        switch_base("general-show-thinking", show_thinking, false, {
            let weak = weak.clone();
            move |on, cx| {
                let _ = weak.update(cx, |_c, cx| {
                    let mut s = crate::services::workspace::app_settings();
                    s.show_thinking = Some(on);
                    crate::services::workspace::save_app_settings(&s);
                    cx.notify();
                });
            }
        }),
        t,
    ));

    // 部分操作隐藏 switch（dock 两操作栏不常用 icon 自动隐藏总开关；默认
    // on = 隐藏，hover 各自操作栏才显影。切开关即生效：notify 重渲 Chat，
    // 下帧 dock 操作栏按新值决定是否挂 opacity 0）
    let autohide = crate::services::workspace::autohide_icons();
    col = col.child(set_row(
        tr("settings.autohide_icons"),
        tr("settings.autohide_icons_hint"),
        switch_base("general-autohide-icons", autohide, false, {
            let weak = weak.clone();
            move |on, cx| {
                let _ = weak.update(cx, |_c, cx| {
                    let mut s = crate::services::workspace::app_settings();
                    s.autohide_icons = Some(on);
                    crate::services::workspace::save_app_settings(&s);
                    cx.notify();
                });
            }
        }),
        t,
    ));

    // 显示状态栏 switch（034 自输入面板 MCP 右侧挪入：输入下方状态栏信息
    // 显隐，默认 on）。落盘走 ui_state（`session_status_bar`），切开关即
    // notify 重渲——状态带整条按新值决定是否渲染。
    let status_bar = crate::services::workspace::session_status_bar_on();
    col = col.child(set_row(
        tr("settings.show_status_bar"),
        tr("settings.show_status_bar_hint"),
        switch_base("general-show-status-bar", status_bar, false, {
            let weak = weak.clone();
            move |on, cx| {
                let _ = weak.update(cx, |_c, cx| {
                    crate::services::workspace::set_session_status_bar(on);
                    cx.notify();
                });
            }
        }),
        t,
    ));

    // ---- 语言：三个一排，左对齐（置底——用户定夺） ------------------------
    col = col.child(section_label(tr("settings.language")));
    col = col.child(lang_row(weak, t));

    col.into_any_element()
}

fn section_label(text: &str) -> gpui::AnyElement {
    let t = T();
    div()
        .mt(px(20.))
        .mb(px(8.))
        .text_size(crate::appearance::ui_size(13.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(t.text_muted))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

/// 一个明暗档的主题卡片组。每排固定 5 个等宽槽位（跨排同宽），超过 5 个
/// 自动折行兑底（当前淡色 5 = 1 排、深色 2 + 3 个空槽）。
/// 卡片即配色横条本身（无外框壳）：主题底色 + accent / muted 圆点，主题名
/// 居中放进横条（文字用主题自身前景色保证可读）。
fn theme_row(dark: bool, weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    let current = theme::theme_name();
    let entries: Vec<&ThemeEntry> = crate::appearance::registry()
        .iter()
        .filter(|e| {
            theme::ALL
                .iter()
                .find(|(id, _)| *id == e.id)
                .map(|(_, th)| th.dark == dark)
                .unwrap_or(false)
        })
        .collect();
    // 折行：每 5 个一行，末行用空槽补齐 5 槽，保证所有行槽位等宽
    let rows: Vec<Vec<Option<&ThemeEntry>>> = entries
        .chunks(5)
        .map(|chunk| (0..5).map(|ix| chunk.get(ix).copied()).collect())
        .collect();
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .mt(px(8.))
        .children(rows.into_iter().map(|cells| {
            div()
                .flex()
                .gap(px(8.))
                .children(cells.into_iter().map(|cell| {
                    let Some(entry) = cell else {
                        // 空槽：只占位，保持与上一排等宽
                        return div().flex_1().min_w_0().into_any_element();
                    };
                    let th = theme::ALL
                        .iter()
                        .find(|(id, _)| *id == entry.id)
                        .map(|(_, t)| *t)
                        .expect("registry id missing from theme table");
                    let active = entry.id == current;
                    let weak_card = weak.clone();
                    let theme_id = entry.id.to_string();
                    // 卡片外包一层无 padding/边框的等宽槽位：卡片自带的 padding+边框
                    // 会计入 taffy 的假设主尺寸（flex-basis 0 压不掉），直接 flex_1 会
                    // 比空槽宽出一个 padding+border，导致上下两排不等宽。
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .id(SharedString::from(format!("theme-{}", entry.id)))
                                .w_full()
                                .h(px(36.))
                                .rounded(px(5.))
                                .border_1()
                                // 选中描边加粗到 2px（后写覆盖 border_1 的四边宽度）
                                .when(active, |d| d.border_2())
                                .border_color(rgb(if active { th.accent } else { th.border }))
                                .bg(rgb(th.bg))
                                .overflow_hidden()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .px(px(8.))
                                .cursor_pointer()
                                .hover(|s| s.border_color(rgb(th.text_muted)))
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    let _ = weak_card.update(cx, |_c, cx| {
                                        if crate::appearance::persist_theme(&theme_id) {
                                            // 006: re-map tokens so widget-library surfaces
                                            // (inputs/modals) follow the switch
                                            crate::appearance::sync_gpui_tokens(cx);
                                            cx.notify();
                                        }
                                    });
                                })
                                // 左圆点区 / 右空位等宽（各 flex_1），名字真正落在横条中央
                                .child(
                                    div()
                                        .flex_1()
                                        .flex()
                                        .items_center()
                                        .gap(px(4.))
                                        .child(div().size(px(7.)).rounded_full().bg(rgb(th.accent)))
                                        .child(
                                            div()
                                                .size(px(7.))
                                                .rounded_full()
                                                .bg(rgb(th.text_muted)),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_size(crate::appearance::ui_size(11.5))
                                        .font_weight(if active {
                                            gpui::FontWeight::SEMIBOLD
                                        } else {
                                            gpui::FontWeight::NORMAL
                                        })
                                        .text_color(rgb(if active { th.accent } else { th.text }))
                                        .whitespace_nowrap()
                                        .child(SharedString::from(entry.name)),
                                )
                                .child(div().flex_1()),
                        )
                        .into_any_element()
                }))
        }))
        .into_any_element()
}

/// 一个字体槽位行：标签 + 250px 族下拉 + 字号三档下拉。所见即所得分工：
/// 面板字号 = 全局 UI 缩放（ui_size）；会话字体 = 用户/agent 输出正文的
/// 族+字号（markdown.rs MD_SPEC，不含 meta 等 chrome）；Markdown 字体 =
/// 文件预览 markdown。
/// 语言：三个 radio（14px 圆圈，无边框无底色）。文字统一 text_muted，
/// 选中只加粗（SEMIBOLD）不变色；圆圈 accent 描边 + 内点。
fn lang_row(
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    let lang_current = i18n::lang_ix();
    div()
        .flex()
        .gap(px(24.))
        .children(i18n::LANG_LABELS.iter().enumerate().map(|(ix, label)| {
            let active = lang_current == ix;
            let weak_lang = weak.clone();
            div()
                .id(SharedString::from(format!("lang-{ix}")))
                .flex()
                .items_center()
                .gap(px(7.))
                .cursor_pointer()
                .font_weight(if active {
                    gpui::FontWeight::SEMIBOLD
                } else {
                    gpui::FontWeight::NORMAL
                })
                .text_color(rgb(t.text_muted))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_lang.update(cx, |_c, cx| {
                        i18n::set_lang(ix);
                        save_lang_pref(ix);
                        cx.notify();
                    });
                })
                .child(
                    div()
                        .size(px(14.))
                        .rounded_full()
                        .border_1()
                        .border_color(rgb(if active { t.accent } else { t.text_muted }))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(active, |d| {
                            d.child(div().size(px(6.)).rounded_full().bg(rgb(t.accent)))
                        }),
                )
                // 文字颜色继承父容器（radio 文字色统一在容器上设置）
                .child(
                    div()
                        .text_size(crate::appearance::ui_size(12.))
                        .child(SharedString::from(label.to_string())),
                )
                .into_any_element()
        }))
        .into_any_element()
}
