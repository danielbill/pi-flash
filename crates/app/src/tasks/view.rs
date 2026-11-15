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
use gpui::{AnyElement, Entity, MouseButton, SharedString, div, prelude::*, px, rgb, rgba};

use pf_context::domain::tasks::{Outcome, TaskKind, TaskRun};

use crate::Chat;
use crate::appearance::theme::{self, Theme, theme};
use crate::appearance::ui_size;
use crate::editor::markdown::MONO_FAMILY;
use crate::i18n::{tf, tr};
use crate::services::format::fmt_msg_time;
use crate::session::menu_rows::{pill_row, thinking_rows, tools_preset_rows};
use crate::session::model_picker::{ModelPickAction, model_menu_body, model_menu_content_h};
use crate::tasks::{DefRow, INTERVALS, PAGE_TAB_LOG, PAGE_TAB_TASKS};
use crate::ui::dropdown::{DropdownState, dropdown};
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
                                c.tasks.page.open_dd = 0;
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
        _ => tasks_body(chat, weak, t),
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
fn run_slot(r: &TaskRun, t: &'static Theme) -> Option<AnyElement> {
    match r.outcome {
        Outcome::Running => Some(spark_spinner(12., t.accent)),
        Outcome::Ok => Some(status_dot(theme::UNREAD)),
        Outcome::Failed => Some(status_dot(t.danger)),
        Outcome::Timeout => Some(status_dot(theme::WARN)),
        Outcome::Skipped => None,
    }
}

/// mono 时间戳 `MM-DD HH:MM:SS`（截图拍板格式）。
fn fmt_stamp(ms: i64) -> String {
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

fn tasks_body(chat: &Chat, weak: &gpui::WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
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
        .children(kind_block(chat, weak, t));

    div()
        .flex_1()
        .min_h_0()
        .flex()
        .child(list)
        .child(detail)
        .into_any_element()
}

/// 表单头：名称大字输入 + 右上动作（编辑态 ghost 组 + 保存）。
fn form_head(chat: &Chat, weak: &gpui::WeakEntity<Chat>, _t: &'static Theme) -> AnyElement {
    let editing = chat.tasks.page.sel.is_some();
    let name = chat
        .tasks
        .page
        .name
        .clone()
        .expect("page open ensures inputs");
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .mb(px(2.))
        .child(div().flex_1().min_w_0().h(px(34.)).child(name))
        .when(editing, |d| {
            let running = chat
                .tasks
                .running
                .iter()
                .any(|k| Some(k.as_str()) == chat.tasks.page.sel.as_deref());
            let enabled = chat
                .tasks
                .defs
                .iter()
                .find(|d| Some(d.def.id.as_str()) == chat.tasks.page.sel.as_deref())
                .map(|d| d.def.enabled)
                .unwrap_or(true);
            let sel_run = chat.tasks.page.sel.clone();
            let sel_toggle = chat.tasks.page.sel.clone();
            let weak_run = weak.clone();
            let weak_toggle = weak.clone();
            let weak_del = weak.clone();
            let sel_del = chat.tasks.page.sel.clone();
            d.child(ghost_btn(
                "task-ghost-run",
                tr("task.run_now"),
                false,
                running,
                weak_run,
                move |c, cx| {
                    if let Some(id) = &sel_run {
                        c.task_run_now(id, cx);
                    }
                },
            ))
            .child(ghost_btn(
                "task-ghost-toggle",
                if enabled {
                    tr("task.disable")
                } else {
                    tr("task.enable")
                },
                false,
                false,
                weak_toggle,
                move |c, cx| {
                    if sel_toggle.is_some() {
                        c.task_toggle_from_page(cx);
                    }
                },
            ))
            .child(ghost_btn(
                "task-ghost-del",
                tr("task.remove"),
                true,
                false,
                weak_del,
                move |c, cx| {
                    if let Some(id) = &sel_del {
                        let title = c
                            .tasks
                            .defs
                            .iter()
                            .find(|d| d.def.id == *id)
                            .map(|d| d.def.title.clone())
                            .unwrap_or_default();
                        c.tasks.page.confirm_del = Some((id.clone(), title));
                        cx.notify();
                    }
                },
            ))
        })
        .child(config_button(
            "task-save",
            weak,
            tr("task.save"),
            Btn::Secondary,
            true,
            false,
            |c, cx| c.task_save_form(cx),
        ))
        .into_any_element()
}

/// ghost 动作钮（HTML ghost-btn：px8 py3 rounded 6 f12；danger 变体）。
fn ghost_btn(
    id: &'static str,
    label: &str,
    danger: bool,
    disabled: bool,
    weak: gpui::WeakEntity<Chat>,
    on_click: impl Fn(&mut Chat, &mut gpui::Context<Chat>) + 'static,
) -> AnyElement {
    let t = theme();
    div()
        .id(id)
        .px(px(8.))
        .py(px(3.))
        .rounded(px(6.))
        .text_size(ui_size(12.))
        .text_color(rgb(if danger { t.danger } else { t.text_dim }))
        .cursor_pointer()
        .when(disabled, |d| d.opacity(0.5).cursor_default())
        .when(!disabled, |d| {
            d.hover(|h| {
                h.bg(rgb(if danger { t.danger_hover } else { t.bg_hover }))
                    .text_color(rgb(if danger { t.danger } else { t.text }))
            })
        })
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            if disabled {
                return;
            }
            let _ = weak.update(cx, |c, cx| on_click(c, cx));
        })
        .child(SharedString::from(label.to_string()))
        .into_any_element()
}

/// field 形制：label（fl2 11.5 text_dim）上 + 控件行（gap 8 wrap）下。
fn field(label: &'static str, control: AnyElement) -> AnyElement {
    let t = theme();
    div()
        .mt(px(14.))
        .child(
            div()
                .text_size(ui_size(11.5))
                .text_color(rgb(t.text_dim))
                .mb(px(5.))
                .child(tr(label).to_string()),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .child(control),
        )
        .into_any_element()
}

/// 任务类型 seg 分段【脚本|Agent】（HTML segType）。
fn type_seg(chat: &Chat, weak: &gpui::WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    let cur_agent = chat.tasks.page.kind_agent;
    div()
        .flex_none()
        .flex()
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(6.))
        .overflow_hidden()
        .bg(rgb(t.bg_panel))
        .children([false, true].iter().map(|&agent| {
            let on = cur_agent == agent;
            let weak = weak.clone();
            div()
                .id(SharedString::from(format!("task-seg-{agent}")))
                .flex()
                .items_center()
                .h(px(28.))
                .px(px(16.))
                .text_size(ui_size(12.))
                .cursor_pointer()
                .when(on, |d| {
                    d.bg(rgb(t.bg_selected))
                        .text_color(rgb(t.text))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                })
                .when(!on, |d| {
                    d.text_color(rgb(t.text_muted))
                        .hover(|h| h.text_color(rgb(t.text)))
                })
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak.update(cx, |c, cx| {
                        c.tasks.page.kind_agent = agent;
                        cx.notify();
                    });
                })
                .child(if agent {
                    tr("task.type_agent").to_string()
                } else {
                    tr("task.type_script").to_string()
                })
        }))
        .into_any_element()
}

// ---------------------------------------------------------------------------
// 调度行（HTML schedRow：freq/时间 sel-btn + 摘要；v1 五档）
// ---------------------------------------------------------------------------

/// sel-btn（h28 px8 border rounded 6 bg_panel gap 8 + chevron；hover accent 框）。
fn sel_btn(id: &'static str, label: String, w: Option<f32>) -> AnyElement {
    let t = theme();
    div()
        .id(id)
        .h(px(28.))
        .px(px(8.))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(6.))
        .bg(rgb(t.bg_panel))
        .hover(|h| h.border_color(rgb(t.accent)))
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(ui_size(12.))
        .text_color(rgb(t.text))
        .when_some(w, |d, w| d.w(px(w)))
        .child(div().min_w_0().truncate().child(SharedString::from(label)))
        .child(icon("chevron-down", 10., t.text_dim))
        .into_any_element()
}

/// 调度文本段（「第」「分」「于」）。
fn sched_text(s: &'static str) -> AnyElement {
    let t = theme();
    div()
        .text_size(ui_size(12.5))
        .text_color(rgb(t.text))
        .child(tr(s).to_string())
        .into_any_element()
}

/// 调度摘要（本地时区；HTML summary 去 GMT+8 硬编码）。
fn sched_summary(chat: &Chat) -> String {
    let p = &chat.tasks.page;
    let hm = format!("{:02}:{:02}", p.hour, p.minute);
    match p.unit_ix {
        0 => tf(
            tr("task.sum_cmin"),
            &[("n", INTERVALS[p.interval_ix.min(9)].to_string())],
        ),
        1 => tf(tr("task.sum_hourly"), &[("m", p.minute.to_string())]),
        2 => tf(tr("task.sum_daily"), &[("t", hm)]),
        3 => {
            let w = weekday_label(p.weekdays);
            tf(tr("task.sum_weekly"), &[("w", w), ("t", hm)])
        }
        _ => tf(
            tr("task.sum_monthly"),
            &[("d", p.month_day.to_string()), ("t", hm)],
        ),
    }
}

/// weekly 单选的周几文案（1–7 周一=1；空表回落「一」）。
fn weekday_label(weekdays: [bool; 7]) -> String {
    let ix = weekdays
        .iter()
        .position(|on| *on)
        .map(|i| i + 1)
        .unwrap_or(1);
    tr(match ix {
        1 => "task.weekday_1",
        2 => "task.weekday_2",
        3 => "task.weekday_3",
        4 => "task.weekday_4",
        5 => "task.weekday_5",
        6 => "task.weekday_6",
        _ => "task.weekday_7",
    })
    .to_string()
}

/// 调度行（随 unit 重排；互斥下拉槽位 open_dd：1=freq 2=interval 3=minute
/// 4=wd 5=mday 6=time 7=tools 8=model 9=thinking）。
fn sched_row(chat: &Chat, weak: &gpui::WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    let page = &chat.tasks.page;
    let dd = page.dd.clone().expect("page open ensures inputs");
    let summary = sched_summary(chat);

    // freq 菜单（每小时/每天/每周/每月；切档回落 HTML setFreq 默认时刻）
    let freq_items: Vec<AnyElement> = [
        (1usize, "task.unit_hour"),
        (2, "task.unit_day"),
        (3, "task.unit_week"),
        (4, "task.unit_month"),
    ]
    .iter()
    .map(|&(ix, key)| {
        let weak = weak.clone();
        dd_option(
            SharedString::from(format!("task-opt-freq-{ix}")),
            tr(key).to_string(),
            page.unit_ix == ix,
            move |c, cx| {
                c.tasks.page.unit_ix = ix;
                if ix == 1 {
                    c.tasks.page.minute = 0;
                } else {
                    c.tasks.page.hour = 9;
                    c.tasks.page.minute = 0;
                }
                c.tasks.page.open_dd = 0;
                cx.notify();
            },
            &weak,
        )
    })
    .collect();

    let row = div().flex().flex_wrap().items_center().gap(px(8.)).child({
        let weak = weak.clone();
        dropdown(
            "task-dd-freq",
            &dd,
            page.open_dd == 1,
            dd_toggle(1, &weak),
            dd_dismiss(&weak),
            sel_btn("task-sel-freq", freq_label(chat), None),
            move || dd_popup(freq_items, 160., 200.),
        )
    });

    let row = match page.unit_ix {
        0 => {
            // 每 N 分钟：间隔菜单
            let items: Vec<AnyElement> = INTERVALS
                .iter()
                .enumerate()
                .map(|(ix, n)| {
                    let weak = weak.clone();
                    let id = SharedString::from(format!("task-opt-interval-{ix}"));
                    dd_option(
                        id,
                        format!("{} {}", tr("task.minute_suffix"), n),
                        page.interval_ix == ix,
                        move |c, cx| {
                            c.tasks.page.interval_ix = ix;
                            c.tasks.page.open_dd = 0;
                            cx.notify();
                        },
                        &weak,
                    )
                })
                .collect();
            row.child(dropdown(
                "task-dd-interval",
                &dd,
                page.open_dd == 2,
                dd_toggle(2, weak),
                dd_dismiss(weak),
                sel_btn(
                    "task-sel-interval",
                    format!(
                        "{} {}",
                        tr("task.minute_suffix"),
                        INTERVALS[page.interval_ix.min(9)]
                    ),
                    None,
                ),
                move || dd_popup(items, 160., 220.),
            ))
        }
        1 => {
            // 每小时：第 [M] 分
            let items: Vec<AnyElement> = (0..60)
                .map(|n| {
                    let weak = weak.clone();
                    dd_option(
                        SharedString::from(format!("task-opt-minute-{n}")),
                        format!("{n:02}"),
                        page.minute == n,
                        move |c, cx| {
                            c.tasks.page.minute = n;
                            c.tasks.page.open_dd = 0;
                            cx.notify();
                        },
                        &weak,
                    )
                })
                .collect();
            row.child(sched_text("task.minute_nth"))
                .child(dropdown(
                    "task-dd-minute",
                    &dd,
                    page.open_dd == 3,
                    dd_toggle(3, weak),
                    dd_dismiss(weak),
                    sel_btn("task-sel-minute", format!("{:02}", page.minute), None),
                    move || dd_popup(items, 96., 246.),
                ))
                .child(sched_text("task.minute_suffix"))
        }
        2 => row
            .child(sched_text("task.run_at"))
            .child(time_pick(chat, weak, t)),
        3 => {
            // 每周：[周X] 于 HH:MM（单选落 weekdays 单元素）
            let items: Vec<AnyElement> = (0..7)
                .map(|i| {
                    let weak = weak.clone();
                    let key = weekday_key(i);
                    let on = page.weekdays[i];
                    dd_option(
                        SharedString::from(format!("task-opt-wd-{i}")),
                        format!("{}{}", tr("task.weekday_prefix"), tr(key)),
                        on,
                        move |c, cx| {
                            let mut w = [false; 7];
                            w[i] = true;
                            c.tasks.page.weekdays = w;
                            c.tasks.page.open_dd = 0;
                            cx.notify();
                        },
                        &weak,
                    )
                })
                .collect();
            row.child(dropdown(
                "task-dd-wd",
                &dd,
                page.open_dd == 4,
                dd_toggle(4, weak),
                dd_dismiss(weak),
                sel_btn(
                    "task-sel-wd",
                    format!(
                        "{}{}",
                        tr("task.weekday_prefix"),
                        weekday_label(page.weekdays)
                    ),
                    None,
                ),
                move || dd_popup(items, 140., 220.),
            ))
            .child(sched_text("task.run_at"))
            .child(time_pick(chat, weak, t))
        }
        _ => {
            // 每月：[X 号] 于 HH:MM
            let items: Vec<AnyElement> = (1u32..=31)
                .map(|n| {
                    let weak = weak.clone();
                    dd_option(
                        SharedString::from(format!("task-opt-md-{n}")),
                        format!("{} {}", n, tr("task.day_num")),
                        page.month_day == n,
                        move |c, cx| {
                            c.tasks.page.month_day = n;
                            c.tasks.page.open_dd = 0;
                            cx.notify();
                        },
                        &weak,
                    )
                })
                .collect();
            row.child(dropdown(
                "task-dd-md",
                &dd,
                page.open_dd == 5,
                dd_toggle(5, weak),
                dd_dismiss(weak),
                sel_btn(
                    "task-sel-md",
                    format!("{} {}", page.month_day, tr("task.day_num")),
                    None,
                ),
                move || dd_popup(items, 140., 246.),
            ))
            .child(sched_text("task.run_at"))
            .child(time_pick(chat, weak, t))
        }
    };
    row.child(
        div()
            .text_size(ui_size(11.5))
            .text_color(rgb(t.text_faint))
            .child(SharedString::from(summary)),
    )
    .into_any_element()
}

fn weekday_key(i: usize) -> &'static str {
    match i {
        0 => "task.weekday_1",
        1 => "task.weekday_2",
        2 => "task.weekday_3",
        3 => "task.weekday_4",
        4 => "task.weekday_5",
        5 => "task.weekday_6",
        _ => "task.weekday_7",
    }
}

/// 「于 HH:MM」时间拾取（HTML tpick：时/分双列 64px，互斥槽 6）。
fn time_pick(chat: &Chat, weak: &gpui::WeakEntity<Chat>, _t: &'static Theme) -> AnyElement {
    let page = &chat.tasks.page;
    let dd = page.dd.clone().expect("page open ensures inputs");
    let t = theme();
    let hour_items: Vec<AnyElement> = (0..24)
        .map(|n| {
            let weak = weak.clone();
            tp_item(
                SharedString::from(format!("task-tp-h-{n}")),
                format!("{n:02}"),
                page.hour == n,
                move |c, cx| {
                    c.tasks.page.hour = n;
                    cx.notify();
                },
                weak,
                &t,
            )
        })
        .collect();
    let minute_items: Vec<AnyElement> = (0..60)
        .map(|n| {
            let weak = weak.clone();
            tp_item(
                SharedString::from(format!("task-tp-m-{n}")),
                format!("{n:02}"),
                page.minute == n,
                move |c, cx| {
                    c.tasks.page.minute = n;
                    cx.notify();
                },
                weak,
                &t,
            )
        })
        .collect();
    let tpick = div()
        .flex()
        .child(
            div()
                .id("task-tp-h")
                .w(px(64.))
                .max_h(px(252.))
                .overflow_y_scroll()
                .border_r_1()
                .border_color(rgba(theme::border_alpha(t, 0x45)))
                .children(hour_items),
        )
        .child(
            div()
                .id("task-tp-m")
                .w(px(64.))
                .max_h(px(252.))
                .overflow_y_scroll()
                .children(minute_items),
        );
    dropdown(
        "task-dd-time",
        &dd,
        page.open_dd == 6,
        dd_toggle(6, weak),
        dd_dismiss(weak),
        sel_btn(
            "task-sel-time",
            format!("{:02}:{:02}", page.hour, page.minute),
            None,
        ),
        move || dd_popup_el(tpick.into_any_element()),
    )
}

/// 拾取器行（h36 mono 13 居中；on = bg_selected semibold）。
fn tp_item(
    id: impl Into<SharedString>,
    label: String,
    on: bool,
    on_pick: impl Fn(&mut Chat, &mut gpui::Context<Chat>) + 'static,
    weak: gpui::WeakEntity<Chat>,
    t: &'static Theme,
) -> AnyElement {
    div()
        .id(id.into())
        .h(px(36.))
        .flex()
        .items_center()
        .justify_center()
        .font_family(MONO_FAMILY)
        .text_size(ui_size(13.))
        .cursor_pointer()
        .when(on, |d| {
            d.bg(rgb(t.bg_selected))
                .text_color(rgb(t.text))
                .font_weight(gpui::FontWeight::SEMIBOLD)
        })
        .when(!on, |d| {
            d.text_color(rgb(t.text_dim))
                .hover(|h| h.bg(rgb(t.bg_hover)))
        })
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            let _ = weak.update(cx, |c, cx| {
                on_pick(c, cx);
            });
        })
        .child(SharedString::from(label))
        .into_any_element()
}

/// freq 标签。
fn freq_label(chat: &Chat) -> String {
    match chat.tasks.page.unit_ix {
        0 => format!(
            "{} {}",
            tr("task.minute_suffix"),
            INTERVALS[chat.tasks.page.interval_ix.min(9)]
        ),
        1 => tr("task.unit_hour").to_string(),
        2 => tr("task.unit_day").to_string(),
        3 => tr("task.unit_week").to_string(),
        _ => tr("task.unit_month").to_string(),
    }
}

/// 下拉互斥开关（写 page.open_dd）。
fn dd_toggle(
    slot: u8,
    weak: &gpui::WeakEntity<Chat>,
) -> impl Fn(&mut gpui::Window, &mut gpui::App) + 'static {
    let weak = weak.clone();
    move |_, cx| {
        let _ = weak.update(cx, |c, cx| {
            c.tasks.page.open_dd = slot;
            cx.notify();
        });
    }
}

fn dd_dismiss(
    weak: &gpui::WeakEntity<Chat>,
) -> impl Fn(&mut gpui::Window, &mut gpui::App) + 'static {
    let weak = weak.clone();
    move |_, cx| {
        let _ = weak.update(cx, |c, cx| {
            c.tasks.page.open_dd = 0;
            cx.notify();
        });
    }
}

/// 下拉弹层基座（菜单卡视觉 + 限高滚动）。
fn dd_popup(items: Vec<AnyElement>, min_w: f32, max_h: f32) -> AnyElement {
    let t = theme();
    div()
        .id("task-dd-pop")
        .min_w(px(min_w))
        .max_h(px(max_h))
        .overflow_y_scroll()
        .p(px(4.))
        .bg(rgb(t.bg))
        .border_1()
        .border_color(rgba(theme::border_alpha(t, 0x8c)))
        .rounded(px(8.))
        .shadow_lg()
        .flex()
        .flex_col()
        .children(items)
        .into_any_element()
}

/// 任意元素版弹层（tpick 复用）。
fn dd_popup_el(el: AnyElement) -> AnyElement {
    let t = theme();
    div()
        .id("task-dd-pop-el")
        .bg(rgb(t.bg))
        .border_1()
        .border_color(rgba(theme::border_alpha(t, 0x8c)))
        .rounded(px(8.))
        .shadow_lg()
        .overflow_hidden()
        .child(el)
        .into_any_element()
}

/// 下拉选项行（px10/py7 ui(12) gap8 + ✓ 槽）。
fn dd_option(
    id: impl Into<SharedString>,
    label: String,
    selected: bool,
    on_pick: impl Fn(&mut Chat, &mut gpui::Context<Chat>) + 'static,
    weak: &gpui::WeakEntity<Chat>,
) -> AnyElement {
    let t = theme();
    let weak = weak.clone();
    div()
        .id(id.into())
        .px(px(10.))
        .py(px(7.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(ui_size(12.))
        .cursor_pointer()
        .text_color(rgb(if selected { t.text } else { t.text_muted }))
        .when(selected, |d| d.font_weight(gpui::FontWeight::SEMIBOLD))
        .hover(|h| h.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            let _ = weak.update(cx, |c, cx| {
                c.tasks.page.open_dd = 0;
                on_pick(c, cx);
                cx.notify();
            });
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(SharedString::from(label)),
        )
        .when(selected, |d| d.child(icon("check", 12., t.accent)))
        .into_any_element()
}

// ---------------------------------------------------------------------------
// 表单 kind 区块（agent composer / script 文件）
// ---------------------------------------------------------------------------

fn kind_block(chat: &Chat, weak: &gpui::WeakEntity<Chat>, t: &'static Theme) -> Vec<AnyElement> {
    if chat.tasks.page.kind_agent {
        vec![agent_block(chat, weak, t)]
    } else {
        vec![script_block(chat, weak, t)]
    }
}

/// cm-bar pill（icon + label + chev；hover bg_hover）。
fn pill(
    id: &'static str,
    icon_name: Option<(&'static str, f32)>,
    label: String,
    chevron: bool,
    on_click: impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    t: &'static Theme,
) -> AnyElement {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(8.))
        .py(px(4.))
        .rounded(px(6.))
        .text_size(ui_size(12.))
        .text_color(rgb(t.text_muted))
        .cursor_pointer()
        .hover(|h| h.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
        .on_mouse_down(MouseButton::Left, on_click)
        .children(icon_name.map(|(name, size)| icon(name, size, t.text_dim)))
        .child(div().min_w_0().truncate().child(SharedString::from(label)))
        .when(chevron, |d| d.child(icon("chevron-down", 10., t.text_dim)))
        .into_any_element()
}

/// pill 视觉基座（菜单开 = accent；触发布局与 pill 同一形制）。
fn pill_base(
    icon_name: Option<(&'static str, f32)>,
    label: String,
    chevron: bool,
    open: bool,
    t: &'static Theme,
) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(8.))
        .py(px(4.))
        .rounded(px(6.))
        .text_size(ui_size(12.))
        .cursor_pointer()
        .when(open, |d| d.text_color(rgb(t.accent)))
        .when(!open, |d| {
            d.text_color(rgb(t.text_muted))
                .hover(|h| h.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
        })
        .children(icon_name.map(|(name, size)| icon(name, size, t.text_dim)))
        .child(div().min_w_0().truncate().child(SharedString::from(label)))
        .when(chevron, |d| d.child(icon("chevron-down", 10., t.text_dim)))
        .into_any_element()
}

/// 菜单态 pill（dropdown 触发器；弹层经 kit dropdown 挂 anchored）。
#[allow(clippy::too_many_arguments)]
fn pill_menu(
    id: &'static str,
    icon_name: Option<(&'static str, f32)>,
    label: String,
    open: bool,
    on_toggle: impl Fn(&mut gpui::Window, &mut gpui::App) + 'static,
    on_dismiss: impl Fn(&mut gpui::Window, &mut gpui::App) + 'static,
    popup: impl FnOnce() -> AnyElement + 'static,
    dd: &Entity<DropdownState>,
    t: &'static Theme,
) -> AnyElement {
    let inner = pill_base(icon_name, label, true, open, t);
    dropdown(id, dd, open, on_toggle, on_dismiss, inner, popup)
}

/// Agent 指令 composer 形制（max-w 620：textarea + pill 行）。
fn agent_block(chat: &Chat, weak: &gpui::WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    let page = &chat.tasks.page;
    let dd = page.dd.clone().expect("page open ensures inputs");
    let desc = page.desc.clone().expect("page open ensures inputs");

    let proj_label = page
        .project
        .as_ref()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| tr("task.project").to_string());
    let weak_proj = weak.clone();
    let project_pill = pill(
        "task-pill-project",
        Some(("folder", 13.)),
        proj_label,
        true,
        move |_, _, cx| {
            let _ = weak_proj.update(cx, |c, cx| {
                c.open_project_picker_for_task(cx);
            });
        },
        t,
    );

    let tool_rows: Vec<AnyElement> = tools_preset_rows(page.tools)
        .into_iter()
        .map(|r| {
            let weak = weak.clone();
            pill_row(
                &weak,
                {
                    let key = r.key;
                    move |c, cx| {
                        c.tasks.page.tools = pf_context::domain::tasks::TOOLS_PRESETS
                            .iter()
                            .find(|k| **k == key)
                            .copied()
                            .unwrap_or("default");
                        c.tasks.page.open_dd = 0;
                        cx.notify();
                    }
                },
                r,
            )
        })
        .collect();
    let weak_tools = weak.clone();
    let tools_pill = pill_menu(
        "task-pill-tools",
        Some(("wrench", 13.)),
        page.tools.to_string(),
        page.open_dd == 7,
        dd_toggle(7, &weak_tools),
        dd_dismiss(&weak_tools),
        move || dd_popup(tool_rows, 220., 220.),
        &dd,
        t,
    );

    let model_label = page
        .model
        .as_ref()
        .map(|(p, m)| format!("{p}/{m}"))
        .unwrap_or_else(|| tr("task.model_inherit").to_string());
    let (models, _total) = chat.picker_models_from(None, None);
    let mut model_items: Vec<AnyElement> = vec![dd_option(
        "task-opt-model-inherit",
        tr("task.model_inherit").to_string(),
        page.model.is_none(),
        move |c, cx| {
            c.tasks.page.model = None;
            c.tasks.page.open_dd = 0;
            cx.notify();
        },
        weak,
    )];
    let on_pick: ModelPickAction = std::rc::Rc::new(move |c: &mut Chat, cx, p, id| {
        c.tasks.page.model = Some((p.clone(), id.clone()));
        c.tasks.page.open_dd = 0;
        cx.notify();
    });
    model_items.push(
        div()
            .id("task-model-scroll")
            .max_h(px(model_menu_content_h(&models)))
            .overflow_y_scroll()
            .child(model_menu_body(
                weak,
                &models,
                page.model
                    .as_ref()
                    .map(|(p, m)| format!("{p}/{m}"))
                    .as_deref(),
                None,
                |_| false,
                Some(on_pick),
                t,
            ))
            .into_any_element(),
    );
    let weak_model = weak.clone();
    let model_pill = pill_menu(
        "task-pill-model",
        None,
        model_label,
        page.open_dd == 8,
        dd_toggle(8, &weak_model),
        dd_dismiss(&weak_model),
        move || dd_popup(model_items, 320., 360.),
        &dd,
        t,
    );

    let think_rows: Vec<AnyElement> = thinking_rows(page.thinking)
        .into_iter()
        .map(|r| {
            let weak = weak.clone();
            pill_row(
                &weak,
                {
                    let key = r.key;
                    move |c, cx| {
                        c.tasks.page.thinking = pf_context::domain::tasks::THINKING_LEVELS
                            .iter()
                            .find(|k| **k == key)
                            .copied()
                            .unwrap_or("auto");
                        c.tasks.page.open_dd = 0;
                        cx.notify();
                    }
                },
                r,
            )
        })
        .collect();
    let weak_think = weak.clone();
    let think_pill = pill_menu(
        "task-pill-think",
        Some(("lightbulb", 13.)),
        page.thinking.to_string(),
        page.open_dd == 9,
        dd_toggle(9, &weak_think),
        dd_dismiss(&weak_think),
        move || dd_popup(think_rows, 220., 260.),
        &dd,
        t,
    );

    field(
        "task.instr",
        div()
            .w_full()
            .max_w(px(620.))
            .border_1()
            .border_color(rgb(t.border))
            .rounded(px(10.))
            .bg(rgb(t.bg_panel))
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(div().min_h(px(120.)).child(desc))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .p(px(4.))
                    .px(px(6.))
                    .child(project_pill)
                    .child(tools_pill)
                    .child(div().flex_1())
                    .child(model_pill)
                    .child(think_pill),
            )
            .into_any_element(),
    )
}

/// 脚本文件 field：sel-btn w340 + 浏览… + note。
fn script_block(chat: &Chat, weak: &gpui::WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    let page = &chat.tasks.page;
    let label = page
        .script_path
        .as_ref()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| tr("task.script_ph").to_string());
    div()
        .mt(px(14.))
        .child(
            div()
                .text_size(ui_size(11.5))
                .text_color(rgb(t.text_dim))
                .mb(px(5.))
                .child(tr("task.script").to_string()),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .child(sel_btn("task-sel-script", label, Some(340.)))
                .child(config_button(
                    "task-browse",
                    weak,
                    tr("task.browse"),
                    Btn::Secondary,
                    true,
                    false,
                    |c, cx| c.pick_task_script_file(cx),
                )),
        )
        .child(
            div()
                .mt(px(6.))
                .text_size(ui_size(11.5))
                .text_color(rgb(t.text_faint))
                .child(div().child(tr("task.script_note").to_string()).when_some(
                    page.script_err.clone(),
                    |d, e| {
                        d.child(
                            div()
                                .mt(px(4.))
                                .text_color(rgb(t.danger))
                                .child(SharedString::from(e)),
                        )
                    },
                )),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// ② 日志 tab（截图拍板版：行 = 任务列表行同语言）
// ---------------------------------------------------------------------------

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
