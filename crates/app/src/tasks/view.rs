//! 046 任务管理页（2026-11-16 用户五条拍板，不再参考设计 HTML）：
//!
//! ① 列表/详情 = 40%/60% 分栏（列表最小宽 400）；② 页头二级 tab = PF 插件
//! 市场简易 tab（MONO 大字 + `|` 分隔，弃融底卡避免与一级页签栏撞样式）；
//! ③ 详情 label/控件左右排（subagent 表单同款 w96/w250）；④ 任务列表默认
//! 分两组：上【脚本任务】下【Agent 任务】（subagent list 同款结构样式）；
//! ⑤ 空列表提示「暂无任务」。数据面读 `chat.tasks`（泵/CRUD 刷新，不自建
//! 轮询）。

use chrono::TimeZone as _;
use std::path::PathBuf;

use gpui::{AnyElement, MouseButton, SharedString, div, prelude::*, px, rgb, rgba};

use pf_context::domain::tasks::{Outcome, TaskKind, TaskRun};

use crate::Chat;
use crate::appearance::theme::{self, Theme, theme};
use crate::appearance::ui_size;
use crate::editor::markdown::MONO_FAMILY;
use crate::i18n::{tf, tr};
use crate::services::format::fmt_msg_time;
use crate::tasks::DefRow;
use crate::tasks::form::{field, form_head, kind_block, sched_row, type_seg};
use crate::ui::icon;
use crate::ui::kit::{Btn, config_button, group_header, sidebar_item, status_dot};
use crate::ui::overlay;
use crate::ui::spark_spinner;
use crate::ui::tooltip::WithTip;

// ---------------------------------------------------------------------------
// 页装配
// ---------------------------------------------------------------------------

/// 任务管理页主体（content.rs `ContentView::Tasks` 分支）。2026-11-22 用户
/// 令改版：二级 tab 删除，单页三段式——上 = 40/60 列表+详情，下 40% =
/// 整宽运行日志（只呈现选中任务）。
pub(crate) fn page(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    window: &gpui::Window,
    cx: &mut gpui::Context<Chat>,
) -> AnyElement {
    let t = theme();
    let sel = chat.tasks.page.sel.clone();

    let top = tasks_body(chat, weak, t, window, cx);
    let log = run_log_panel(chat, weak, t, &sel);

    div()
        .id("task-page")
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        .child(top)
        // 上下段 1px 分隔
        .child(
            div()
                .flex_none()
                .h(px(1.))
                .bg(rgba(theme::border_alpha(t, 0x45))),
        )
        .child(log)
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

/// 行装配（两 tab 共用，subagent list 同款 30px sidebar_item）：类型 icon
/// （2026-11-17 拍板：icon 用设计 HTML 的 tico——脚本=file-code、
/// Agent=bot，None = 不出）+ 名（纯文本；2026-11-22 拍板：去下划线链接态，
/// 点行 = 详情回填，不再开话）+ 结果槽 + 右缘 mono 时间。`select` =
/// Some(def_id) 时行点击 = 详情回填（任务列表行），且行尾 hover 显影动作钮
/// （立即运行/停用/移除，2026-11-22 用户令从详情表头挪来，子代理行同款）。
/// `open_path` = Some(session_path) 时行名可点 = 开该次会话的只读观察窗
/// （日志行专属：任务会话已从用户列表隐身，这里是唯一入口；Agent 任务
/// 才落 session_path，脚本行自然不可点；与 select 互斥使用）。
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
    open_path: Option<PathBuf>,
    running: bool,
    t: &'static Theme,
) -> AnyElement {
    let dot_id = SharedString::from(format!("{row_id}-dot"));
    // 行尾动作钮与行点击/行名各要一份 select/title（主体装配会按值吃掉）
    let select_act = select.clone();
    let title_act = title.clone();
    // 显影组 id（hover 时 时间/动作钮 visibility 互斥，见右槽注释）
    let reveal = select_act
        .as_ref()
        .map(|def_id| SharedString::from(format!("task-row-act-{def_id}")));
    // 颜色继承 sidebar_item（active text / 非活动 text_muted，subagent 行同款）；
    // 仅停用/跳过灰名覆盖。日志行（open_path Some）名可点 = 开该次会话的
    // 只读观察窗（2026-11-22 用户令：点名开话；不可发送、无 InputPanel
    // ——open_subagent_session_window 观察者态自带这两条）
    let weak_open = weak.clone();
    let name_el = div()
        .id(SharedString::from(format!("{row_id}-name")))
        .flex_1()
        .min_w_0()
        .truncate()
        .text_size(ui_size(12.))
        .when(gray, |d| d.text_color(rgb(t.text_faint)))
        .when_some(open_path, |d, sp| {
            d.cursor_pointer()
                .hover(|h| h.text_color(rgb(t.accent)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_open.update(cx, |c, cx| {
                        c.open_subagent_session_window(sp.clone(), cx);
                    });
                })
        })
        .child(SharedString::from(title));
    let weak_row = weak.clone();
    sidebar_item(row_id, selected)
        // 046 任务行规格（2026-11-22 用户令）：行高 36 + 行间 4（§5.8 新档
        // row::TASK；基座 30/mb1 按后写覆盖）
        .h(px(crate::ui::tokens::row::TASK))
        .mb(px(crate::ui::tokens::space::SP2))
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
        // 设计 HTML sp-li 行序：tico → 名 → 结果槽 → 时间。行首 tico =
        // 类型图标（脚本 file-code / Agent bot，2026-11-22 用户澄清：状态
        // 图标在行尾动作钮，不在此处）；尺寸走规范 MD。
        .children(tico.map(|name| {
            icon(
                name,
                crate::ui::tokens::icon::MD,
                if gray { t.text_faint } else { t.text_dim },
            )
        }))
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
        // 右槽（2026-11-22 用户令）：时间恒右对齐；列表行 hover 时三动作钮
        // 盖在时间上——visibility 互斥（notification.rs 同款 invisible ⇄
        // visible；invisible 不吃命中，时间区点击照常选中行）。槽 min_w =
        // 三钮宽（22×3 + SP2×2，与 task_act_btn 尺寸同源），时间短于钮区时
        // 名字也不被 hover 钮盖住。日志行无动作钮，恒显时间。
        .child({
            // 时间在 hover 组内反向隐身（reveal 组挂行容器，见上）
            let stamp_el = div()
                .flex_none()
                .font_family(MONO_FAMILY)
                // 活动行 SEMIBOLD 不下渗进时间戳
                .font_weight(gpui::FontWeight::NORMAL)
                .text_size(ui_size(11.))
                .text_color(rgb(t.text_faint))
                .when_some(reveal, |d, r| d.group_hover(r, |s| s.invisible()))
                .child(SharedString::from(stamp));
            let mut slot = div()
                .relative()
                .h(px(crate::ui::tokens::row::TASK))
                .flex_none()
                .flex()
                .items_center()
                .child(stamp_el);
            if let Some(def_id) = select_act {
                slot = slot
                    .justify_end()
                    .min_w(px(22. * 3. + crate::ui::tokens::space::SP2 * 2.))
                    .child(task_row_actions(
                        &def_id, &title_act, !gray, running, weak, t,
                    ));
            }
            slot
        })
        .into_any_element()
}

/// 行尾 hover 显影动作钮（2026-11-22 用户令：立即运行/停用/移除从详情表头
/// 挪来；同日用户令：绝对定位盖时间——时间右对齐，hover 时钮/时间 visibility
/// 互斥）。钮 = queue_cards icon_btn 同款 22px hover 底小图标钮。运行中
/// 「立即运行」置灰（原表头同守卫）；停用/启用钮图标 = 当前状态（rotate-clock
/// / pause，2026-11-22 用户令：状态与 icon 对应）；移除走确认浮层（confirm_del）。
fn task_row_actions(
    def_id: &str,
    title: &str,
    enabled: bool,
    running: bool,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
) -> AnyElement {
    // group 在行容器（task_row），此处只挂 group_hover——同元素自引用永不
    // 显影（docs/agents/gpui.md「group/group_hover 必须两元素分离」）。
    // 显影 = visibility 互斥（invisible 默认 + hover visible，notification.rs
    // 同款）——绝对定位盖在时间上（右槽见 task_row），invisible 不吃命中
    let reveal = SharedString::from(format!("task-row-act-{def_id}"));
    let weak_run = weak.clone();
    let id_run = def_id.to_string();
    let weak_tog = weak.clone();
    let id_tog = def_id.to_string();
    let weak_del = weak.clone();
    let id_del = def_id.to_string();
    let title_del = title.to_string();
    div()
        .absolute()
        .top(px(0.))
        .bottom(px(0.))
        .right(px(0.))
        .flex()
        .items_center()
        .gap(px(crate::ui::tokens::space::SP2))
        .invisible()
        .group_hover(reveal, |s| s.visible())
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
            // 图标 = 当前状态（2026-11-22 用户令：状态与 icon 对应，不反着）
            // ：可执行态 rotate-clock / 暂停态 pause；点击翻状态
            if enabled { "rotate-clock" } else { "pause" },
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
        // ui-raw: ok 046 行尾动作钮 IB_SM 22 槽逐值复刻（§10.8 登记）
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
        .child(icon(name, crate::ui::tokens::icon::MD, t.text_muted))
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
            // 设计 HTML tico：脚本行 file-code / Agent 行 bot（状态图标在
            // 行尾动作钮，2026-11-22 用户澄清）
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
                None,
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

/// 下方任务日志区（2026-11-22 用户令）：45% 高，内容列 70% 居中，只呈现
/// 选中任务的运行记录（原日志 tab 收编，不再混杂全部任务）。行 = task_row
/// （点击开该次会话——任务会话已从用户列表隐身，这里是唯一入口）。行名 =
/// 任务名（def title，2026-11-22 用户令：不出 def_id）。未选中任务时整区
/// 空态。
fn run_log_panel(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    sel: &Option<String>,
) -> AnyElement {
    let rows: Vec<AnyElement> = chat
        .tasks
        .runs
        .iter()
        // 只出选中任务的日志（2026-11-22 用户令：不再混杂）
        .filter(|r| sel.as_deref() == Some(r.def_id.as_str()))
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
            let open_path = r.session_path.as_deref().map(PathBuf::from);
            let slot = run_slot(r, t);
            let tip = Some(run_tip(r));
            let stamp = fmt_stamp(r.started_at);
            let id = SharedString::from(format!("task-log-{}-{}", r.def_id, r.started_at));
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
            let title = chat
                .tasks
                .defs
                .iter()
                .find(|d| d.def.id == r.def_id)
                .map(|d| d.def.title.clone())
                .unwrap_or_else(|| r.def_id.clone());
            task_row(
                id, title, gray, tico, slot, tip, stamp, false, weak, None, open_path, false, t,
            )
        })
        .collect();

    let has_rows = !rows.is_empty();
    let empty = sel.is_none()
        || chat
            .tasks
            .defs
            .iter()
            .find(|d| Some(&d.def.id) == sel.as_ref())
            .is_none();

    div()
        .id("task-log-panel")
        .h(gpui::relative(0.45))
        .flex_none()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        // 标题：任务日志（2026-11-22 用户令改名，任务名后缀删）
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .px(px(20.))
                .pt(px(10.))
                .pb(px(6.))
                .child(
                    div()
                        .text_size(ui_size(12.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(t.text_muted))
                        .child(tr("task.log_title").to_string()),
                ),
        )
        .child(if has_rows {
            div()
                .id("task-log-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .pb(px(10.))
                .flex()
                .flex_col()
                // 内容列 70% 居中（2026-11-22 用户令）
                .child(
                    div()
                        .w(gpui::relative(0.7))
                        .mx_auto()
                        .flex()
                        .flex_col()
                        .children(rows),
                )
                .into_any_element()
        } else {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_size(ui_size(12.))
                .text_color(rgb(t.text_faint))
                .child(tr(if empty {
                    "task.log_none_sel"
                } else {
                    "task.log_empty"
                }))
                .into_any_element()
        })
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
