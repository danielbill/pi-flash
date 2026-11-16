//! 046 任务管理页（2026-11-15 拍板：内容区页签；UI 事实源 =
//! `docs/UI设计/任务管理-原型.html` + 用户截图改版）。
//!
//! 页头横向【任务】【日志】双 tab（tk-tab 顶部圆角融底卡）+ 右【＋新建】；
//! 任务 tab = set-split 两栏（左列表 w350 + 右表单 field 形制）；
//! 日志 tab 行 = 截图拍板版：类型图标 + 任务名（有会话→下划线可点开话；
//! 停用/跳过→灰名无点）+ 结果点/spinner + 右对齐 mono 时间；完成点 = 绿
//! （UNREAD）。数据面读 `chat.tasks`（泵/CRUD 刷新，不自建轮询）。

use std::path::PathBuf;

use chrono::TimeZone as _;
use gpui::{AnyElement, MouseButton, SharedString, div, prelude::*, px, rgb, rgba};

use pf_context::domain::tasks::{Outcome, TaskKind, TaskRun};

use crate::Chat;
use crate::appearance::theme::{self, Theme, theme};
use crate::appearance::ui_size;
use crate::editor::markdown::MONO_FAMILY;
use crate::i18n::{tf, tr};
use crate::services::format::fmt_msg_time;
use crate::tasks::form::{field, form_head, kind_block, sched_row, type_seg};
use crate::tasks::{DefRow, PAGE_TAB_LOG, PAGE_TAB_TASKS};
use crate::ui::kit::{Btn, config_button, status_dot};
use crate::ui::overlay;
use crate::ui::tooltip::WithTip;
use crate::ui::{icon, spark_spinner};

// ---------------------------------------------------------------------------
// 页装配
// ---------------------------------------------------------------------------

/// 任务管理页主体（content.rs `ContentView::Tasks` 分支）。
pub(crate) fn page(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    _window: &mut gpui::Window,
    cx: &gpui::Context<Chat>,
) -> AnyElement {
    let t = theme();
    let page = &chat.tasks.page;

    // 页头：左【任务】【日志】横向表头 + 右【＋新建】
    let tabs: [(u8, &'static str); 2] = [
        (PAGE_TAB_TASKS, "task.tab_main"),
        (PAGE_TAB_LOG, "task.tab_log"),
    ];
    let weak_tab = weak.clone();
    let head = div()
        .flex_none()
        .h(px(40.))
        .bg(rgb(t.nav))
        .border_b_1()
        .border_color(rgba(theme::border_alpha(t, 0x45)))
        .flex()
        .items_end()
        .px(px(12.))
        .pl(px(16.))
        .child(
            div()
                .h_full()
                .flex()
                .gap(px(4.))
                .items_end()
                .children(tabs.iter().map(|&(ix, key)| {
                    let on = page.tab == ix;
                    let weak = weak_tab.clone();
                    div()
                        // ui-raw: ok HTML tk-tab 页头表头逐值复刻（h34/顶圆角9/mb−1 融底；§10.8 登记）
                        .id(SharedString::from(format!("task-page-tab-{ix}")))
                        .flex_none()
                        .flex()
                        .items_center()
                        .h(px(34.))
                        .px(px(16.))
                        .rounded_t(px(9.))
                        .mb(px(-1.))
                        .border_1()
                        .border_b_0()
                        .border_color(if on {
                            rgba(theme::border_alpha(t, 0x55))
                        } else {
                            rgba(0x00000000)
                        })
                        .when(on, |d| {
                            d.bg(rgb(t.bg))
                                .text_color(rgb(t.text))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                        })
                        .when(!on, |d| {
                            d.text_color(rgb(t.text_muted))
                                .hover(|h| h.text_color(rgb(t.text)))
                        })
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak.update(cx, |c, cx| {
                                c.tasks.page.tab = ix;
                                c.tasks.page.popup = None;
                                cx.notify();
                            });
                        })
                        .child(tr(key).to_string())
                })),
        )
        .child(
            div()
                .ml_auto()
                .h_full()
                .flex()
                .items_center()
                .child(config_button(
                    "task-page-new",
                    weak,
                    &format!("＋ {}", tr("task.new")),
                    Btn::Secondary,
                    true,
                    false,
                    |c, cx| c.task_new_form(cx),
                )),
        );

    let body = match page.tab {
        PAGE_TAB_LOG => log_body(chat, weak, t),
        _ => tasks_body(chat, weak, t, cx),
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

/// 行装配（两 tab 共用）：图标 + 名（可点下划线开话）+ 结果槽 + mono 时间。
/// `select` = Some(def_id) 时行点击 = 详情回填（任务列表行）。
#[allow(clippy::too_many_arguments)]
fn task_row(
    row_id: SharedString,
    icon_name: &'static str,
    title: String,
    gray: bool,
    link: Option<PathBuf>,
    slot: Option<AnyElement>,
    tip: Option<String>,
    stamp: String,
    selected: bool,
    weak: &gpui::WeakEntity<Chat>,
    select: Option<String>,
    t: &'static Theme,
) -> AnyElement {
    let t2 = theme();
    let _ = t2;
    let dot_id = SharedString::from(format!("{row_id}-dot"));
    let name_color = if gray { t.text_faint } else { t.text };
    let mut name_el = div()
        .id(SharedString::from(format!("{row_id}-name")))
        .flex_1()
        .min_w_0()
        .truncate()
        .text_size(ui_size(12.5))
        .text_color(rgb(name_color))
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
    div()
        // ui-raw: ok 任务/日志列表行 HTML 拍板几何（gap9/py9 非 2px 网格；§10.8 登记）
        .id(row_id)
        .flex()
        .items_center()
        .gap(px(9.))
        .px(px(10.))
        .py(px(9.))
        .rounded(px(8.))
        .cursor_pointer()
        .when(selected, |d| d.bg(rgb(t.bg_selected)))
        .when(!selected, |d| d.hover(|h| h.bg(rgb(t.bg_hover))))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            if let Some(def_id) = &select {
                cx.stop_propagation();
                let _ = weak_row.update(cx, |c, cx| {
                    c.task_select(def_id, cx);
                });
            }
        })
        .child(icon(icon_name, 15., t.text_dim))
        .child(name_el)
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
        .child(
            div()
                .flex_none()
                .font_family(MONO_FAMILY)
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
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    cx: &gpui::Context<Chat>,
) -> AnyElement {
    let page = &chat.tasks.page;
    let sel = page.sel.clone();

    // 左列表（点击 = 详情回填；agent 名下划线 = 打开会话）
    let rows: Vec<AnyElement> = chat
        .tasks
        .defs
        .iter()
        .map(|DefRow { def, spec }| {
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
            task_row(
                SharedString::from(format!("task-sp-{id}")),
                if matches!(spec.kind, TaskKind::Agent) {
                    "bot"
                } else {
                    "file"
                },
                def.title.clone(),
                !def.enabled,
                link,
                slot,
                tip,
                stamp,
                sel.as_deref() == Some(id.as_str()),
                weak,
                Some(id.clone()),
                t,
            )
        })
        .collect();

    let list = div()
        .id("task-sp-list")
        .w(px(350.))
        .flex_none()
        .border_r_1()
        .border_color(rgba(theme::border_alpha(t, 0x45)))
        .pt(px(10.))
        .px(px(10.))
        .pb(px(14.))
        .pl(px(4.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(px(1.))
        .children(rows);

    let detail = div()
        .id("task-sp-detail")
        .flex_1()
        .min_w_0()
        .overflow_y_scroll()
        .pt(px(14.))
        .px(px(20.))
        .pb(px(16.))
        .pl(px(15.))
        .flex()
        .flex_col()
        .child(form_head(chat, weak, t))
        .child(field("task.type", type_seg(chat, weak, t)))
        .child(field("task.sched", sched_row(chat, weak, t)))
        .children(kind_block(chat, weak, t, cx));

    div()
        .flex_1()
        .min_h_0()
        .flex()
        .child(list)
        .child(detail)
        .into_any_element()
}

/// 表单头：名称大字输入 + 右上动作（编辑态 ghost 组 + 保存）。
fn log_body(chat: &Chat, weak: &gpui::WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    let rows: Vec<AnyElement> = chat
        .tasks
        .runs
        .iter()
        .map(|r| {
            let (icon_name, gray_task) = match chat.tasks.defs.iter().find(|d| d.def.id == r.def_id)
            {
                Some(DefRow { def, spec }) => (
                    if matches!(spec.kind, TaskKind::Agent) {
                        "bot"
                    } else {
                        "file"
                    },
                    !def.enabled,
                ),
                None => ("file", false),
            };
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
            task_row(
                id, icon_name, title, gray, link, slot, tip, stamp, false, weak, None, t,
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
