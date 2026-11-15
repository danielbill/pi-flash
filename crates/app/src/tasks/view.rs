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
use crate::session::menu_rows::{pill_row, thinking_rows, tools_preset_rows};
use crate::session::model_picker::{ModelPickAction, model_menu_body};
use crate::tasks::popup_slot;
use crate::tasks::{DefRow, INTERVALS, PAGE_TAB_LOG, PAGE_TAB_TASKS};
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
fn form_head(chat: &Chat, weak: &gpui::WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
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
        // HTML name-input 形制：内件 flat，容器 34px 行给 hover 底
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h(px(34.))
                .flex()
                .items_center()
                .rounded(px(6.))
                .hover(|h| h.bg(rgb(t.bg_panel)))
                .child(name),
        )
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
                        // 类型切换换 kind 区块：开着的目标菜单（工具/模型/
                        // 思考）锚点会悬空——顺手全关
                        c.tasks.page.popup = None;
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
// 调度行（HTML schedRow：sel-btn + 摘要；菜单 = anchored pmenu，2026-11-15
// 用户拍板「下方只沿用 InputPanel 的操作栏」——composer bar_trigger 同机制）
// ---------------------------------------------------------------------------

/// sel-btn（h28 px8 border rounded 6 bg_panel gap 8 + chevron；hover accent 框）。
fn sel_btn(
    id: &'static str,
    label: String,
    w: Option<f32>,
    open: bool,
    t: &'static Theme,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(28.))
        .px(px(8.))
        .border_1()
        .border_color(rgb(if open { t.accent } else { t.border }))
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
}

/// 触发钮包装（settings ag_trigger 同款）：点击记锚点 + 翻转 popup 槽位 +
/// paint 期 tracker 抓 bounds（弹层统一定位测量端）。**点击就地截断**——
/// composer 操作栏行内还有别的鼠标语义（项目 pill），菜单钮不冒泡
/// （2026-11-15 截图 bug：点工具钮冒泡把项目弹窗也拉开了）。
fn trg_wrap(
    slot: u8,
    weak: &gpui::WeakEntity<Chat>,
    chat: &Chat,
    visual: gpui::Stateful<gpui::Div>,
) -> AnyElement {
    let bounds = chat.tasks.page.anchors.get(slot).clone();
    let bounds_click = bounds.clone();
    let weak_click = weak.clone();
    visual
        .on_mouse_down(
            MouseButton::Left,
            move |ev: &gpui::MouseDownEvent, _, cx| {
                cx.stop_propagation();
                let at = ev.position;
                let _ = weak_click.update(cx, |c, _| {
                    // inputpanel 同款：锚点 = 按钮槽位 bounds（未就绪退点击坐标）
                    c.tasks.page.anchor = Some(crate::PillBtns::anchor(&bounds_click, at));
                    c.tasks.page.popup = if c.tasks.page.popup == Some(slot) {
                        None
                    } else {
                        Some(slot)
                    };
                });
            },
        )
        .child(crate::PillBtns::tracker(&bounds))
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

/// freq 标签。
fn freq_label(chat: &Chat) -> String {
    match chat.tasks.page.unit_ix {
        0 => format!(
            "{} {} {}",
            tr("task.sum_cmin_prefix"),
            INTERVALS[chat.tasks.page.interval_ix.min(9)],
            tr("task.minute_suffix")
        ),
        1 => tr("task.unit_hour").to_string(),
        2 => tr("task.unit_day").to_string(),
        3 => tr("task.unit_week").to_string(),
        _ => tr("task.unit_month").to_string(),
    }
}

/// 调度行（随 unit 重排；菜单槽位 popup_slot::*）。
fn sched_row(chat: &Chat, weak: &gpui::WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    let page = &chat.tasks.page;
    let summary = sched_summary(chat);

    // HTML：非分钟档先渲染 freqPill；分钟档（cmin）= 频率与间隔合一的单钮
    let mut row = div().flex().flex_wrap().items_center().gap(px(8.));
    if page.unit_ix != 0 {
        row = row.child(trg_wrap(
            popup_slot::FREQ,
            weak,
            chat,
            sel_btn(
                "task-sel-freq",
                freq_label(chat),
                None,
                page.popup == Some(popup_slot::FREQ),
                t,
            ),
        ));
    }

    row = match page.unit_ix {
        0 => {
            // 每 N 分钟：单钮「每 N 分钟」（HTML cmin 同款合一）
            row.child(trg_wrap(
                popup_slot::INTERVAL,
                weak,
                chat,
                sel_btn(
                    "task-sel-interval",
                    format!(
                        "{} {} {}",
                        tr("task.sum_cmin_prefix"),
                        INTERVALS[page.interval_ix.min(9)],
                        tr("task.minute_suffix")
                    ),
                    None,
                    page.popup == Some(popup_slot::INTERVAL),
                    t,
                ),
            ))
        }
        1 => {
            // 每小时：第 [M] 分
            row.child(sched_text("task.minute_nth"))
                .child(trg_wrap(
                    popup_slot::MINUTE,
                    weak,
                    chat,
                    sel_btn(
                        "task-sel-minute",
                        format!("{:02}", page.minute),
                        None,
                        page.popup == Some(popup_slot::MINUTE),
                        t,
                    ),
                ))
                .child(sched_text("task.minute_suffix"))
        }
        2 => row
            .child(sched_text("task.run_at"))
            .child(time_pick(chat, weak, t)),
        3 => {
            // 每周：[周X] 于 HH:MM（单选落 weekdays 单元素）
            row.child(trg_wrap(
                popup_slot::WD,
                weak,
                chat,
                sel_btn(
                    "task-sel-wd",
                    format!(
                        "{}{}",
                        tr("task.weekday_prefix"),
                        weekday_label(page.weekdays)
                    ),
                    None,
                    page.popup == Some(popup_slot::WD),
                    t,
                ),
            ))
            .child(sched_text("task.run_at"))
            .child(time_pick(chat, weak, t))
        }
        _ => {
            // 每月：[X 号] 于 HH:MM
            row.child(trg_wrap(
                popup_slot::MDAY,
                weak,
                chat,
                sel_btn(
                    "task-sel-md",
                    format!("{} {}", page.month_day, tr("task.day_num")),
                    None,
                    page.popup == Some(popup_slot::MDAY),
                    t,
                ),
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

/// 「于 HH:MM」时间拾取（HTML tpick 双列）。
fn time_pick(chat: &Chat, weak: &gpui::WeakEntity<Chat>, _t: &'static Theme) -> AnyElement {
    let page = &chat.tasks.page;
    let t = theme();
    trg_wrap(
        popup_slot::TIME,
        weak,
        chat,
        sel_btn(
            "task-sel-time",
            format!("{:02}:{:02}", page.hour, page.minute),
            None,
            page.popup == Some(popup_slot::TIME),
            &t,
        ),
    )
}

// ---------------------------------------------------------------------------
// 锚定菜单（menus_overlay 统一挂 overlay 栈；卡片内容复用 inputpanel 组件）
// ---------------------------------------------------------------------------

/// 菜单行（picker_card 内；label + ✓ 槽，pill_row 形制由各卡复用）。
fn menu_row(
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
        .mx(px(6.))
        .px(px(10.))
        .py(px(7.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(ui_size(12.5))
        .cursor_pointer()
        .text_color(rgb(if selected { t.text } else { t.text_muted }))
        .when(selected, |d| d.font_weight(gpui::FontWeight::SEMIBOLD))
        .hover(|h| h.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| {
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
        .when(selected, |d| d.child(icon("check", 13., t.accent)))
        .into_any_element()
}

/// 数值/档位列表卡（限高滚动）。
fn list_card(rows: Vec<AnyElement>, max_h: f32) -> AnyElement {
    div()
        .id(SharedString::from(format!("task-menu-{}", rows.len())))
        .max_h(px(max_h))
        .overflow_y_scroll()
        .py(px(4.))
        .flex()
        .flex_col()
        .children(rows)
        .into_any_element()
}

/// 菜单分组分隔线（HTML pm-sep：1px bd-soft，左右缩进）。
fn menu_sep() -> AnyElement {
    let t = theme();
    div()
        .mx(px(6.))
        .my(px(4.))
        .h(px(1.))
        .flex_none()
        .bg(rgba(theme::border_alpha(t, 0x45)))
        .into_any_element()
}

/// 周期档菜单行（五档共用：freq 主菜单与 cmin 间隔菜单的「换档」分组）。
fn freq_menu_row(ix: usize, chat: &Chat, weak: &gpui::WeakEntity<Chat>) -> AnyElement {
    let page = &chat.tasks.page;
    let sel = page.unit_ix == ix;
    let label = if ix == 0 {
        // 每 N 分钟档：主文案带当前间隔（INTERVALS 档位值）
        tf(
            tr("task.sum_cmin"),
            &[("n", INTERVALS[page.interval_ix.min(9)].to_string())],
        )
    } else {
        tr(match ix {
            1 => "task.unit_hour",
            2 => "task.unit_day",
            3 => "task.unit_week",
            _ => "task.unit_month",
        })
        .to_string()
    };
    menu_row(
        SharedString::from(format!("task-mi-freq-{ix}")),
        label,
        sel,
        move |c, _cx| {
            c.tasks.page.unit_ix = ix;
            // HTML setFreq 同款：切档重置到合理默认（每小时锚第 0 分；
            // 其余日档锚 09:00）；cmin 保留现间隔
            if ix == 1 {
                c.tasks.page.minute = 0;
            } else if ix > 1 {
                c.tasks.page.hour = 9;
                c.tasks.page.minute = 0;
            }
            c.tasks.page.popup = None;
        },
        weak,
    )
}

/// 页面锚定菜单总入口（overlay_stack 挂载；settings ag_menus_overlay 同款）。
pub(crate) fn menus_overlay(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    window: &gpui::Window,
) -> Option<AnyElement> {
    let slot = chat.tasks.page.popup?;
    let t = theme();
    let page = &chat.tasks.page;
    let menu_w = match slot {
        popup_slot::FREQ => 160.,
        popup_slot::INTERVAL => 160.,
        popup_slot::MINUTE => 96.,
        popup_slot::WD => 140.,
        popup_slot::MDAY => 140.,
        popup_slot::TIME => 132.,
        popup_slot::MODEL => 380.,
        popup_slot::TIER => 320.,
        popup_slot::THINKING => 320.,
        _ => return None,
    };
    let pos = crate::session::picker_shell::pill_anchor_edges(
        page.anchor,
        window.viewport_size(),
        px(menu_w),
    );
    let weak_c = weak.clone();

    // ---- 各槽位卡片内容
    let content: AnyElement = match slot {
        popup_slot::MODEL => {
            let (models, _total) = chat.picker_models_from(None, None);
            let mut items: Vec<AnyElement> = vec![menu_row(
                "task-mi-inherit",
                tr("task.model_inherit").to_string(),
                page.model.is_none(),
                |c, _cx| c.tasks.page.model = None,
                weak,
            )];
            let on_pick: ModelPickAction = std::rc::Rc::new(|c: &mut Chat, _cx, p, id| {
                c.tasks.page.model = Some((p.clone(), id.clone()));
            });
            items.push(
                div()
                    .id("task-model-list")
                    .max_h(px(300.))
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
            div()
                .flex()
                .flex_col()
                .py(px(4.))
                .children(items)
                .into_any_element()
        }
        popup_slot::TIER => {
            let rows: Vec<AnyElement> = tools_preset_rows(page.tools)
                .into_iter()
                .map(|r| {
                    let weak = weak.clone();
                    let key = r.key;
                    pill_row(&weak, move |c, _cx| c.tasks.page.tools = key, r)
                })
                .collect();
            list_card(rows, 300.)
        }
        popup_slot::THINKING => {
            let rows: Vec<AnyElement> = thinking_rows(page.thinking)
                .into_iter()
                .map(|r| {
                    let weak = weak.clone();
                    let key = r.key;
                    pill_row(&weak, move |c, _cx| c.tasks.page.thinking = key, r)
                })
                .collect();
            list_card(rows, 300.)
        }
        popup_slot::FREQ => {
            // 五档全列（HTML freqMenu 同源；2026-11-15 补「每 N 分钟」——
            // 原四档清单让 cmin 档不可达）
            let rows: Vec<AnyElement> = (0..5).map(|ix| freq_menu_row(ix, chat, &weak_c)).collect();
            list_card(rows, 240.)
        }
        popup_slot::INTERVAL => {
            // cmin 单钮菜单：间隔档清单 + 分组线 + 其余四档（逃出 cmin——
            // 单钮合一后这里是唯一换档口）
            let mut rows: Vec<AnyElement> = INTERVALS
                .iter()
                .enumerate()
                .map(|(ix, n)| {
                    let sel = page.interval_ix == ix;
                    menu_row(
                        SharedString::from(format!("task-mi-interval-{ix}")),
                        format!("{} {}", n, tr("task.minute_suffix")),
                        sel,
                        move |c, _cx| {
                            c.tasks.page.interval_ix = ix;
                            c.tasks.page.popup = None;
                        },
                        &weak_c,
                    )
                })
                .collect();
            rows.push(menu_sep());
            rows.extend((1..5).map(|ix| freq_menu_row(ix, chat, &weak_c)));
            list_card(rows, 360.)
        }
        popup_slot::MINUTE => {
            let rows: Vec<AnyElement> = (0..60)
                .map(|n| {
                    let sel = page.minute == n;
                    menu_row(
                        SharedString::from(format!("task-mi-minute-{n}")),
                        format!("{n:02}"),
                        sel,
                        move |c, _cx| {
                            c.tasks.page.minute = n;
                            c.tasks.page.popup = None;
                        },
                        &weak_c,
                    )
                })
                .collect();
            list_card(rows, 300.)
        }
        popup_slot::WD => {
            let rows: Vec<AnyElement> = (0..7)
                .map(|i| {
                    let sel = page.weekdays[i];
                    let key = weekday_key(i);
                    menu_row(
                        SharedString::from(format!("task-mi-wd-{i}")),
                        format!("{}{}", tr("task.weekday_prefix"), tr(key)),
                        sel,
                        move |c, _cx| {
                            let mut w = [false; 7];
                            w[i] = true;
                            c.tasks.page.weekdays = w;
                            c.tasks.page.popup = None;
                        },
                        &weak_c,
                    )
                })
                .collect();
            list_card(rows, 280.)
        }
        popup_slot::MDAY => {
            let rows: Vec<AnyElement> = (1u32..=31)
                .map(|n| {
                    let sel = page.month_day == n;
                    menu_row(
                        SharedString::from(format!("task-mi-md-{n}")),
                        format!("{} {}", n, tr("task.day_num")),
                        sel,
                        move |c, _cx| {
                            c.tasks.page.month_day = n;
                            c.tasks.page.popup = None;
                        },
                        &weak_c,
                    )
                })
                .collect();
            list_card(rows, 300.)
        }
        // 时间拾取（HTML tpick：双列 64px；点选不关卡，连续调时刻）
        popup_slot::TIME => {
            let hour_items: Vec<AnyElement> = (0..24)
                .map(|n| {
                    let weak = weak.clone();
                    let sel = page.hour == n;
                    div()
                        .id(SharedString::from(format!("task-tp-h-{n}")))
                        .h(px(36.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .font_family(MONO_FAMILY)
                        .text_size(ui_size(13.))
                        .cursor_pointer()
                        .when(sel, |d| {
                            d.bg(rgb(t.bg_selected))
                                .text_color(rgb(t.text))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                        })
                        .when(!sel, |d| {
                            d.text_color(rgb(t.text_dim))
                                .hover(|h| h.bg(rgb(t.bg_hover)))
                        })
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak.update(cx, |c, cx| {
                                c.tasks.page.hour = n;
                                cx.notify();
                            });
                        })
                        .child(SharedString::from(format!("{n:02}")))
                        .into_any_element()
                })
                .collect();
            let minute_items: Vec<AnyElement> = (0..60)
                .map(|n| {
                    let weak = weak.clone();
                    let sel = page.minute == n;
                    div()
                        .id(SharedString::from(format!("task-tp-m-{n}")))
                        .h(px(36.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .font_family(MONO_FAMILY)
                        .text_size(ui_size(13.))
                        .cursor_pointer()
                        .when(sel, |d| {
                            d.bg(rgb(t.bg_selected))
                                .text_color(rgb(t.text))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                        })
                        .when(!sel, |d| {
                            d.text_color(rgb(t.text_dim))
                                .hover(|h| h.bg(rgb(t.bg_hover)))
                        })
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak.update(cx, |c, cx| {
                                c.tasks.page.minute = n;
                                cx.notify();
                            });
                        })
                        .child(SharedString::from(format!("{n:02}")))
                        .into_any_element()
                })
                .collect();
            div()
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
                )
                .into_any_element()
        }
        _ => return None,
    };

    let layer = crate::ui::menu::menu_layer(None, weak, |c, cx| {
        if c.tasks.page.popup.take().is_some() {
            cx.notify();
        }
    });
    Some(
        layer
            .child(crate::ui::overlay::stop_click(
                crate::session::picker_shell::picker_card(pos, px(menu_w), t).child(content),
            ))
            .into_any_element(),
    )
}

// ---------------------------------------------------------------------------
// 表单 kind 区块（agent composer bar / script 文件）
// ---------------------------------------------------------------------------

fn kind_block(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    cx: &gpui::Context<Chat>,
) -> Vec<AnyElement> {
    if chat.tasks.page.kind_agent {
        vec![agent_block(chat, weak, t, cx)]
    } else {
        vec![script_block(chat, weak, t)]
    }
}

/// composer 操作栏 pill（**InputPanel bar_trigger 同款**，2026-11-15 用户
/// 拍板「下方只沿用 InputPanel 的操作栏」——禁止自造 pill 形制）。
fn bar_pill(
    slot: Option<u8>,
    weak: &gpui::WeakEntity<Chat>,
    chat: &Chat,
    id: &'static str,
    icon_name: Option<&'static str>,
    label: String,
    t: &'static Theme,
) -> AnyElement {
    let open = slot.is_some_and(|s| chat.tasks.page.popup == Some(s));
    let color = if open { t.accent } else { t.text_muted };
    let trg = crate::session::input::bar_trigger(id, icon_name, &label, true, color, true);
    match slot {
        Some(slot) => trg_wrap(slot, weak, chat, trg),
        None => trg.into_any_element(),
    }
}

/// stat pill（HTML pill.stat 同款：icon + [n] 常驻提示，非交互——不挂
/// cursor/hover，InputPanel 扩展/MCP 钮同形制但任务表单无菜单可开）。
fn stat_pill(id: &'static str, icon_name: &'static str, n: usize, t: &'static Theme) -> AnyElement {
    crate::session::input::bar_trigger(
        id,
        Some(icon_name),
        &format!("[{n}]"),
        false,
        t.text_muted,
        false,
    )
    .into_any_element()
}

/// 工具档显示名（runtime `tool_preset_label` 同映射：内部键 custom →
/// full+ 档名，其余键原样——TIER 菜单写回的是内部键）。
fn tools_display(key: &str) -> String {
    if key == "custom" {
        tr("extensions.full").to_string()
    } else {
        key.to_string()
    }
}

/// Agent 指令 composer 形制（max-w 620：textarea + **InputPanel 操作栏**：
/// 项目/工具档/扩展/MCP 统计 …… 模型/思考——HTML cm-bar 同序）。
fn agent_block(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    cx: &gpui::Context<Chat>,
) -> AnyElement {
    let page = &chat.tasks.page;
    let desc = page.desc.clone().expect("page open ensures inputs");

    let proj_label = page
        .project
        .as_ref()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| tr("task.project").to_string());
    // InputPanel 同源统计（HTML 两颗 stat pill）：扩展 = 会话清单数，
    // MCP = 全局启用数（mcp_button 同读点）
    let ext_n = chat.rt().read(cx).ext_sources.len();
    let mcp_n = chat
        .cfg
        .mcp_servers
        .iter()
        .filter(|s| s.scope == pi_link::mcp::Scope::Global && s.enabled)
        .count();
    let weak_proj = weak.clone();

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
            .child(div().child(desc))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .p(px(4.))
                    .px(px(6.))
                    // 项目钮：自身开 ProjectPicker（截断不冒泡——行内其余
                    // 钮各有菜单语义，2026-11-15 截图 bug 根因即冒泡误开）
                    .child(
                        crate::session::input::bar_trigger(
                            "task-pill-project",
                            Some("folder"),
                            &proj_label,
                            true,
                            t.text_muted,
                            true,
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            move |_, _, cx| {
                                cx.stop_propagation();
                                let _ = weak_proj.update(cx, |c, cx| {
                                    c.tasks.page.popup = None;
                                    c.open_project_picker_for_task(cx);
                                });
                            },
                        ),
                    )
                    .child(bar_pill(
                        Some(popup_slot::TIER),
                        weak,
                        chat,
                        "task-pill-tier",
                        Some("wrench"),
                        tools_display(page.tools),
                        t,
                    ))
                    .child(stat_pill("task-pill-ext", "plug", ext_n, t))
                    .child(stat_pill("task-pill-mcp", "server", mcp_n, t))
                    .child(div().flex_1())
                    .child(bar_pill(
                        Some(popup_slot::MODEL),
                        weak,
                        chat,
                        "task-pill-model",
                        None,
                        page.model
                            .as_ref()
                            .map(|(p, m)| format!("{p}/{m}"))
                            .unwrap_or_else(|| tr("task.model_inherit").to_string()),
                        t,
                    ))
                    .child(bar_pill(
                        Some(popup_slot::THINKING),
                        weak,
                        chat,
                        "task-pill-thinking",
                        Some("lightbulb"),
                        page.thinking.to_string(),
                        t,
                    )),
            )
            .into_any_element(),
    )
}

/// 脚本文件 field：sel-btn + 浏览… + note。
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
                .child(sel_btn("task-sel-script", label, Some(340.), false, t))
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
