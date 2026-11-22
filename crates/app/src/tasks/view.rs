//! 046 任务管理页（2026-11-16 用户五条拍板，不再参考设计 HTML）：
//!
//! ① 列表/详情 = 40%/60% 分栏（列表最小宽 400）；② 页头二级 tab = PF 插件
//! 市场简易 tab（MONO 大字 + `|` 分隔，弃融底卡避免与一级页签栏撞样式）；
//! ③ 详情 label/控件左右排（subagent 表单同款 w96/w250）；④ 任务列表默认
//! 分两组：上【脚本任务】下【Agent 任务】（subagent list 同款结构样式）；
//! ⑤ 空列表提示「暂无任务」。数据面读 `chat.tasks`（泵/CRUD 刷新，不自建
//! 轮询）。

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
        // 2026-11-22 拍板：页头只留【任务 | 日志】，「新增」挪列表标题栏右缘
        .child(pipe_tabs(page.tab, &weak_tab, t));

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
/// Agent=bot，None = 不出）+ 名（纯文本；2026-11-22 拍板：去下划线链接态，
/// 点行 = 详情回填，不再开话）+ 结果槽 + 右缘 mono 时间。`select` =
/// Some(def_id) 时行点击 = 详情回填（任务列表行），且行尾 hover 显影动作钮
/// （立即运行/停用/移除，2026-11-22 用户令从详情表头挪来，子代理行同款）。
#[allow(clippy::too_many_arguments)]
fn task_row(
    row_id: SharedString,
    title: String,
    gray: bool,
    tico: Option<&'static str>,
    slot: Option<AnyElement>,
    tip: Option<String>,
    stamp: String,
    selected: bool,
    weak: &gpui::WeakEntity<Chat>,
    select: Option<String>,
    running: bool,
    t: &'static Theme,
) -> AnyElement {
    let dot_id = SharedString::from(format!("{row_id}-dot"));
    // 行尾动作钮与行点击/行名各要一份 select/title（主体装配会按值吃掉）
    let select_act = select.clone();
    let title_act = title.clone();
    // 颜色继承 sidebar_item（active text / 非活动 text_muted，subagent 行同款）；
    // 仅停用/跳过灰名覆盖。纯文本不可点（点行由行容器 task_select 接管）
    let name_el = div()
        .id(SharedString::from(format!("{row_id}-name")))
        .flex_1()
        .min_w_0()
        .truncate()
        .text_size(ui_size(12.))
        .when(gray, |d| d.text_color(rgb(t.text_faint)))
        .child(SharedString::from(title));
    let weak_row = weak.clone();
    sidebar_item(row_id, selected)
        // subagent 列表同款：列表区与详情区同底，只留分组头与右分隔线
        .bg(rgb(t.bg))
        .when_some(select, |d, def_id| {
            // 显影 group 挂行容器（gpui 陷阱：group 与 group_hover 同元素
            // 永不生效——compute_style 先于自身 push，见 docs/agents/gpui.md）
            d.group(SharedString::from(format!("task-row-act-{def_id}")))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
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
        // 行尾 hover 显影动作钮（列表行专属；日志行 select=None 不出）
        .when_some(select_act, |d, def_id| {
            d.child(task_row_actions(
                &def_id, &title_act, !gray, running, weak, t,
            ))
        })
        .into_any_element()
}

/// 行尾 hover 显影动作钮（2026-11-22 用户令：立即运行/停用/移除从详情表头
/// 挪来）。显影 = settings agents 行 hover「复制」同款 group 显影；钮 =
/// queue_cards icon_btn 同款 22px hover 底小图标钮。运行中「立即运行」置灰
/// （原表头同守卫）；停用/启用按 def.enabled 翻图标（eye-off/eye）；移除走
/// 确认浮层（confirm_del）。
fn task_row_actions(
    def_id: &str,
    title: &str,
    enabled: bool,
    running: bool,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
) -> AnyElement {
    // group 在行容器（task_row），此处只挂 group_hover——同元素自引用永不
    // 显影（docs/agents/gpui.md「group/group_hover 必须两元素分离」）
    let reveal = SharedString::from(format!("task-row-act-{def_id}"));
    let weak_run = weak.clone();
    let id_run = def_id.to_string();
    let weak_tog = weak.clone();
    let id_tog = def_id.to_string();
    let weak_del = weak.clone();
    let id_del = def_id.to_string();
    let title_del = title.to_string();
    div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(crate::ui::tokens::space::SP2))
        .opacity(0.)
        .group_hover(reveal, |s| s.opacity(1.))
        .font_weight(gpui::FontWeight::NORMAL)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(task_act_btn(
            format!("task-act-run-{id_run}"),
            "play",
            "task.run_now",
            running,
            &weak_run,
            move |c, cx| c.task_run_now(&id_run, cx),
            t,
        ))
        .child(task_act_btn(
            format!("task-act-toggle-{id_tog}"),
            if enabled { "eye-off" } else { "eye" },
            if enabled {
                "task.disable"
            } else {
                "task.enable"
            },
            false,
            &weak_tog,
            move |c, cx| c.task_toggle(&id_tog, cx),
            t,
        ))
        .child(task_act_btn(
            format!("task-act-del-{id_del}"),
            "trash",
            "task.remove",
            false,
            &weak_del,
            move |c, cx| {
                c.tasks.page.confirm_del = Some((id_del.clone(), title_del.clone()));
                cx.notify();
            },
            t,
        ))
        .into_any_element()
}

/// 22px 小图标动作钮（queue_cards icon_btn 同款 hover 态 + Chat 回调形态）。
fn task_act_btn(
    id: String,
    name: &'static str,
    tip_key: &'static str,
    locked: bool,
    weak: &gpui::WeakEntity<Chat>,
    on_click: impl Fn(&mut Chat, &mut gpui::Context<Chat>) + 'static,
    t: &'static Theme,
) -> gpui::Stateful<gpui::Div> {
    let weak = weak.clone();
    div()
        .id(SharedString::from(id))
        .size(px(22.))
        .rounded(px(crate::ui::tokens::radius::INLINE))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .when(locked, |d| d.opacity(0.4))
        .when(!locked, |d| d.hover(|s| s.bg(rgb(t.bg_hover))))
        .with_tip(tip_key)
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            // 与行 on_mouse_down（选中）叠压，先拦防选中覆盖
            cx.stop_propagation();
            if locked {
                return;
            }
            let _ = weak.update(cx, |c, cx| on_click(c, cx));
        })
        .child(icon(name, crate::ui::tokens::icon::XS, t.text_muted))
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

    // 左列表（40% 最小宽 400，2026-11-16 拍板）：单一标题「全部任务 [n]」+
    // 右缘「新增」钮（2026-11-22 拍板：不再分脚本/Agent 两组，页头新建钮撤）；
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
    // 标题栏（空库也出——「新增」是唯一建任务入口）：全部任务 [n] + 右缘新增
    list = list.child(group_header(
        &format!("{} {}", tr("task.filter_all"), chat.tasks.defs.len()),
        Some(config_button(
            "task-list-new",
            weak,
            &tr("task.new"),
            Btn::Primary,
            true,
            false,
            |c, cx| c.task_new_form(cx),
        )),
    ));
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
        for DefRow { def, spec, .. } in &chat.tasks.defs {
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
            let id = def.id.clone();
            // 设计 HTML tico：脚本行 file-code / Agent 行 bot
            let tico: &'static str = if matches!(spec.kind, TaskKind::Agent) {
                "bot"
            } else {
                "file-code"
            };
            list = list.child(task_row(
                SharedString::from(format!("task-sp-{id}")),
                def.title.clone(),
                !def.enabled,
                Some(tico),
                slot,
                tip,
                stamp,
                sel.as_deref() == Some(id.as_str()),
                weak,
                Some(id.clone()),
                chat.tasks.running.iter().any(|k| k.as_str() == id),
                t,
            ));
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
                id, title, gray, tico, slot, tip, stamp, false, weak, None, false, t,
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
