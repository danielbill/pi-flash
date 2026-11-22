//! 046 任务管理页（2026-11-16 用户五条拍板，不再参考设计 HTML）：
//!
//! ① 列表/详情 = 40%/60% 分栏（列表最小宽 400）；② 页头二级 tab = PF 插件
//! 市场简易 tab（MONO 大字 + `|` 分隔，弃融底卡避免与一级页签栏撞样式）；
//! ③ 详情 label/控件左右排（subagent 表单同款 w96/w250）；④ 任务列表默认
//! 分两组：上【脚本任务】下【Agent 任务】（subagent list 同款结构样式）；
//! ⑤ 空列表提示「暂无任务」。数据面读 `chat.tasks`（泵/CRUD 刷新，不自建
//! 轮询）。

use std::path::PathBuf;

use chrono::TimeZone as _;
use gpui::{AnyElement, MouseButton, SharedString, div, prelude::*, px, rgb, rgba};

use pf_context::domain::tasks::{Outcome, TaskKind, TaskRun};

use crate::Chat;
use crate::appearance::theme::{self, Theme, theme};
use crate::appearance::ui_size;
use crate::editor::markdown::MONO_FAMILY;
use crate::i18n::{tf, tr};
use crate::market::pf_market::MONO;
use crate::market::pi_native_market::design::px18;
use crate::services::format::fmt_msg_time;
use crate::tasks::form::{field, form_head, kind_block, sched_row, type_seg};
use crate::tasks::{DefRow, PAGE_TAB_LOG, PAGE_TAB_TASKS};
use crate::ui::icon;
use crate::ui::kit::{Btn, config_button, group_header, sidebar_item, status_dot};
use crate::ui::overlay;
use crate::ui::spark_spinner;
use crate::ui::tooltip::WithTip;

// ---------------------------------------------------------------------------
// 页装配
// ---------------------------------------------------------------------------

/// 任务管理页主体（content.rs `ContentView::Tasks` 分支）。
pub(crate) fn page(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    window: &gpui::Window,
    cx: &mut gpui::Context<Chat>,
) -> AnyElement {
    let t = theme();
    let page = &chat.tasks.page;

    // 页头：左【任务 | 日志】简易 tab + 右【新建】（子代理页同款 Primary）
    let weak_tab = weak.clone();
    let head = div()
        .flex_none()
        .h(px(40.))
        .bg(rgb(t.nav))
        .border_b_1()
        .border_color(rgba(theme::border_alpha(t, 0x45)))
        .flex()
        .items_center()
        .pl(px(16.))
        .pr(px(15.))
        .child(pipe_tabs(page.tab, &weak_tab, t))
        .child(
            div()
                .ml_auto()
                .h_full()
                .flex()
                .items_center()
                .child(config_button(
                    // 子代理页「新建」同款主按钮（2026-11-17 拍板：按钮样式
                    // 对齐 settings/agents——同 Btn::Primary 纯文字钮）
                    "task-page-new",
                    weak,
                    &tr("task.new"),
                    Btn::Primary,
                    true,
                    false,
                    |c, cx| c.task_new_form(cx),
                )),
        );

    let body = match page.tab {
        PAGE_TAB_LOG => log_body(chat, weak, t),
        _ => tasks_body(chat, weak, t, window, cx),
    };

    div()
        .id("task-page")
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        .child(head)
        .child(body)
        .into_any_element()
}

/// 页头二级 tab（PF 插件市场 `tab_row` 简易形制，2026-11-16 用户拍板）：
/// MONO 大字基线排 + `|` 分隔；活动全亮、非活动 text_dim hover 回亮。
fn pipe_tabs(cur: u8, weak: &gpui::WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    let tabs: [(u8, &'static str); 2] = [
        (PAGE_TAB_TASKS, "task.tab_main"),
        (PAGE_TAB_LOG, "task.tab_log"),
    ];
    let mut row = div().flex().items_baseline().gap(px(10.));
    for &(ix, key) in &tabs {
        if ix != PAGE_TAB_TASKS {
            row = row.child(
                div()
                    .font_family(MONO)
                    .text_size(px(px18::TITLE_FS))
                    .text_color(rgb(t.text_faint))
                    .child("|"),
            );
        }
        let active = cur == ix;
        let weak = weak.clone();
        row = row.child(
            div()
                .id(SharedString::from(format!("task-page-tab-{ix}")))
                .font_family(MONO)
                .text_size(px(px18::TITLE_FS))
                .cursor_pointer()
                .text_color(rgb(if active { t.text } else { t.text_dim }))
                .when(!active, |d| d.hover(|h| h.text_color(rgb(t.text))))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak.update(cx, |c, cx| {
                        c.tasks.page.tab = ix;
                        c.tasks.page.popup = None;
                        cx.notify();
                    });
                })
                .child(tr(key).to_string()),
        );
    }
    row.into_any_element()
}

// ---------------------------------------------------------------------------
// 共用行视觉（任务列表行 / 日志行同语言，截图拍板版）
// ---------------------------------------------------------------------------

/// 行结果槽：running = spinner / ok = 绿点 / failed = 红点 / timeout = 黄点 /
/// skipped = 空槽（截图拍板：灰名无点）。
pub(crate) fn run_slot(r: &TaskRun, t: &'static Theme) -> Option<AnyElement> {
    match r.outcome {
        Outcome::Running => Some(spark_spinner(12., t.accent)),
        Outcome::Ok => Some(status_dot(theme::UNREAD)),
        Outcome::Failed => Some(status_dot(t.danger)),
        Outcome::Timeout => Some(status_dot(theme::WARN)),
        Outcome::Skipped => None,
    }
}

/// mono 时间戳 `MM-DD HH:MM:SS`（截图拍板格式）。
pub(crate) fn fmt_stamp(ms: i64) -> String {
    match chrono::Local.timestamp_millis_opt(ms) {
        chrono::LocalResult::Single(t) => t.format("%m-%d %H:%M:%S").to_string(),
        _ => String::new(),
    }
}

/// 行 hover 摘要（时间 + 结果 + 原因）。
fn run_tip(r: &TaskRun) -> String {
    let outcome = match r.outcome {
        Outcome::Running => tr("task.running"),
        Outcome::Ok => tr("task.outcome_ok"),
        Outcome::Failed => tr("task.outcome_failed"),
        Outcome::Timeout => tr("task.outcome_timeout"),
        Outcome::Skipped => tr("task.outcome_skipped"),
    };
    let reason = r
        .result_json
        .as_deref()
        .and_then(|j| serde_json::from_str::<serde_json::Value>(j).ok())
        .and_then(|v| {
            v.get("reason")
                .or_else(|| v.get("error"))
                .and_then(|x| x.as_str().map(str::to_string))
        });
    let mut tip = format!("{} · {}", fmt_msg_time(r.started_at), outcome);
    if let Some(reason) = reason {
        tip.push_str(" · ");
        tip.push_str(&reason);
    }
    tip
}

/// 行装配（两 tab 共用，subagent list 同款 30px sidebar_item）：类型 icon
/// （2026-11-17 拍板：icon 用设计 HTML 的 tico——脚本=file-code、
/// Agent=bot，None = 不出）+ 名（可点下划线开话；停用/跳过灰名）+
/// 结果槽 + 右缘 mono 时间。`select` = Some(def_id) 时行点击 = 详情回填
/// （任务列表行）。
#[allow(clippy::too_many_arguments)]
fn task_row(
    row_id: SharedString,
    title: String,
    gray: bool,
    link: Option<PathBuf>,
    tico: Option<&'static str>,
    slot: Option<AnyElement>,
    tip: Option<String>,
    stamp: String,
    selected: bool,
    weak: &gpui::WeakEntity<Chat>,
    select: Option<String>,
    t: &'static Theme,
) -> AnyElement {
    let dot_id = SharedString::from(format!("{row_id}-dot"));
    // 颜色继承 sidebar_item（active text / 非活动 text_muted，subagent 行同款）；
    // 仅停用/跳过灰名覆盖
    let mut name_el = div()
        .id(SharedString::from(format!("{row_id}-name")))
        .flex_1()
        .min_w_0()
        .truncate()
        .text_size(ui_size(12.))
        .when(gray, |d| d.text_color(rgb(t.text_faint)))
        .child(SharedString::from(title));
    if let Some(sp) = link.clone() {
        let weak_link = weak.clone();
        name_el = name_el
            .cursor_pointer()
            .underline()
            .hover(|h| h.text_color(rgb(t.accent)))
            .with_tip("task.open_session_tip")
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                let _ = weak_link.update(cx, |c, cx| {
                    c.open_session(sp.clone(), false, cx);
                });
            });
    }
    let weak_row = weak.clone();
    sidebar_item(row_id, selected)
        // subagent 列表同款：列表区与详情区同底，只留分组头与右分隔线
        .bg(rgb(t.bg))
        .when_some(select, |d, def_id| {
            d.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = weak_row.update(cx, |c, cx| {
                    c.task_select(&def_id, cx);
                });
            })
        })
        // 设计 HTML sp-li 行序：tico → 名 → 结果槽 → 时间
        .children(tico.map(|name| icon(name, crate::ui::tokens::icon::SM, t.text_dim)))
        .child(
            div()
                .id(dot_id)
                .w(px(16.))
                .flex_none()
                .flex()
                .items_center()
                .when_some(slot, |d, s| d.child(s))
                .when_some(tip, |d, tip| d.with_tip_text(tip)),
        )
        .child(name_el)
        .child(
            div()
                .flex_none()
                .font_family(MONO_FAMILY)
                // 活动行 SEMIBOLD 不下渗进时间戳
                .font_weight(gpui::FontWeight::NORMAL)
                .text_size(ui_size(11.))
                .text_color(rgb(t.text_faint))
                .child(SharedString::from(stamp)),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// ① 任务 tab：set-split 两栏（左列表 + 右表单）
// ---------------------------------------------------------------------------

fn tasks_body(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    window: &gpui::Window,
    cx: &mut gpui::Context<Chat>,
) -> AnyElement {
    let sel = chat.tasks.page.sel.clone();

    // 左列表（40% 最小宽 400，2026-11-16 拍板）：上【脚本任务】下
    // 【Agent 任务】两组（subagent list 同款：group_header + 30px 行）；
    // 空库整列「暂无任务」。
    let mut list = div()
        .id("task-sp-list")
        .w(gpui::relative(0.4))
        .min_w(px(400.))
        .flex_none()
        .border_r_1()
        .border_color(rgba(theme::border_alpha(t, 0x45)))
        .overflow_y_scroll()
        .px(px(6.))
        .pt(px(8.))
        .flex()
        .flex_col();
    if chat.tasks.defs.is_empty() {
        list = list.child(
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_size(ui_size(12.))
                .text_color(rgb(t.text_faint))
                .child(tr("task.empty").to_string()),
        );
    } else {
        for (agent, group_key) in [(false, "task.group_script"), (true, "task.group_agent")] {
            let defs: Vec<&DefRow> = chat
                .tasks
                .defs
                .iter()
                .filter(|d| matches!(d.spec.kind, TaskKind::Agent) == agent)
                .collect();
            list = list.child(group_header(
                &format!("{} {}", tr(group_key), defs.len()),
                None,
            ));
            for DefRow { def, .. } in defs {
                let last = chat.tasks.runs.iter().find(|r| r.def_id == def.id);
                let running = chat.tasks.running.iter().any(|k| *k == def.id);
                let (slot, tip) = if running {
                    (Some(spark_spinner(12., t.accent)), None)
                } else {
                    match last {
                        Some(r) => (run_slot(r, t), Some(run_tip(r))),
                        None => (None, None),
                    }
                };
                let stamp = last.map(|r| fmt_stamp(r.started_at)).unwrap_or_default();
                let link = last.and_then(|r| r.session_path.clone()).map(PathBuf::from);
                let id = def.id.clone();
                // 设计 HTML tico：脚本行 file-code / Agent 行 bot（分组即类型）
                let tico: &'static str = if agent { "bot" } else { "file-code" };
                list = list.child(task_row(
                    SharedString::from(format!("task-sp-{id}")),
                    def.title.clone(),
                    !def.enabled,
                    link,
                    Some(tico),
                    slot,
                    tip,
                    stamp,
                    sel.as_deref() == Some(id.as_str()),
                    weak,
                    Some(id.clone()),
                    t,
                ));
            }
        }
    }

    // 右详情（60%）：subagent 表单同构（label w96 左 + 控件右）
    let detail = div()
        .id("task-sp-detail")
        .flex_1()
        .min_w_0()
        .overflow_y_scroll()
        .p(px(15.))
        .flex()
        .flex_col()
        .child(form_head(chat, weak, t))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(crate::ui::tokens::space::SP7))
                .mt(px(crate::ui::tokens::space::SP7))
                .flex_1()
                .child(field("task.type", type_seg(chat, weak, t, window), false))
                .child(field("task.sched", sched_row(chat, weak, t), false))
                .children(kind_block(chat, weak, t, window, cx)),
        );

    div()
        .flex_1()
        .min_h_0()
        .flex()
        .child(list)
        .child(detail)
        .into_any_element()
}

/// 日志 tab：运行记录行（与任务列表行同语言）。
fn log_body(chat: &Chat, weak: &gpui::WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    let rows: Vec<AnyElement> = chat
        .tasks
        .runs
        .iter()
        .map(|r| {
            let gray_task = chat
                .tasks
                .defs
                .iter()
                .find(|d| d.def.id == r.def_id)
                .map(|d| !d.def.enabled)
                .unwrap_or(false);
            let skipped = r.outcome == Outcome::Skipped;
            let gray = gray_task || skipped;
            let link = r.session_path.clone().map(PathBuf::from);
            let slot = run_slot(r, t);
            let tip = Some(run_tip(r));
            let stamp = fmt_stamp(r.started_at);
            let id = SharedString::from(format!("task-log-{}-{}", r.def_id, r.started_at));
            let title = chat
                .tasks
                .defs
                .iter()
                .find(|d| d.def.id == r.def_id)
                .map(|d| d.def.title.clone())
                .unwrap_or_else(|| r.def_id.clone());
            let tico: Option<&'static str> = chat
                .tasks
                .defs
                .iter()
                .find(|d| d.def.id == r.def_id)
                .map(|d| {
                    if matches!(d.spec.kind, TaskKind::Agent) {
                        "bot"
                    } else {
                        "file-code"
                    }
                });
            task_row(
                id, title, gray, link, tico, slot, tip, stamp, false, weak, None, t,
            )
        })
        .collect();

    div()
        .id("task-log-wrap")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .pt(px(14.))
        .px(px(20.))
        .pb(px(20.))
        .child(
            div()
                .max_w(px(860.))
                .flex()
                .flex_col()
                .gap(px(1.))
                .children(rows),
        )
        .into_any_element()
}

/// 移除确认浮层（overlay_stack 挂载；040 卸载确认同款机制）。
pub(crate) fn remove_confirm_overlay(chat: &Chat, cx: &gpui::Context<Chat>) -> Option<AnyElement> {
    let (del_id, del_title) = chat.tasks.page.confirm_del.as_ref()?.clone();
    let weak = cx.entity().downgrade();
    let message = tf(tr("task.delete_confirm"), &[("title", del_title)]);
    let weak_cancel = weak.clone();
    let weak_confirm = weak.clone();
    Some(
        overlay::confirm(
            &chat.dlg.dialog_focus,
            message,
            move |_, cx| {
                let _ = weak_cancel.update(cx, |c, cx| {
                    c.tasks.page.confirm_del = None;
                    cx.notify();
                });
            },
            move |_, cx| {
                let _ = weak_confirm.update(cx, |c, cx| {
                    c.task_delete_confirmed(&del_id, cx);
                });
            },
        )
        .into_any_element(),
    )
}
