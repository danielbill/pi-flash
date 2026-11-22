//! 046 任务表单区（form.rs）：从 view.rs 拆出（行数红线 §1）。
//! 名称输入 + 类型 seg + 调度行 + 指令/脚本块 + freq/interval/time 弹层菜单。

use gpui::{AnyElement, MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::{Theme, theme};
use crate::appearance::ui_size;
use crate::i18n::{tf, tr};
use crate::tasks::INTERVALS;
use crate::tasks::popup::time_pick;
use crate::tasks::popup_slot;
use crate::ui::icon;
use crate::ui::kit::{Btn, config_button};
use crate::ui::tokens::{btn, input, radius, space};

pub(crate) fn form_head(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    _t: &'static Theme,
) -> AnyElement {
    let editing = chat.tasks.page.sel.is_some();
    let name = chat
        .tasks
        .page
        .name
        .clone()
        .expect("page open ensures inputs");
    // subagent 表单头同构（2026-11-16 拍板）：label w96 + 输入 w250 +
    // 弹性段 + 动作钮（运行中「立即运行」置灰；删除走确认浮层）
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .flex_shrink_0()
        .child(field_label("task.name"))
        .child(div().w(px(250.)).flex_shrink_0().child(name))
        .child(div().flex_1())
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
            d.child(config_button(
                "task-head-run",
                &weak_run,
                tr("task.run_now"),
                Btn::Secondary,
                true,
                running,
                move |c, cx| {
                    if let Some(id) = &sel_run {
                        c.task_run_now(id, cx);
                    }
                },
            ))
            .child(config_button(
                "task-head-toggle",
                &weak_toggle,
                if enabled {
                    tr("task.disable")
                } else {
                    tr("task.enable")
                },
                Btn::Secondary,
                true,
                false,
                move |c, cx| {
                    if sel_toggle.is_some() {
                        c.task_toggle_from_page(cx);
                    }
                },
            ))
            .child(config_button(
                "task-head-del",
                &weak_del,
                tr("task.remove"),
                Btn::Danger,
                true,
                false,
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
            Btn::Primary,
            true,
            false,
            |c, cx| c.task_save_form(cx),
        ))
        .into_any_element()
}

/// 行标签（subagent 表单同款：w96 固定左列 + 12px）。
fn field_label(label: &'static str) -> AnyElement {
    let t = theme();
    div()
        .w(px(96.))
        .flex_shrink_0()
        .text_size(ui_size(12.))
        .text_color(rgb(t.text))
        .child(tr(label).to_string())
        .into_any_element()
}

/// field 行（subagent `agent_row` 同款，2026-11-16 拍板从上下改左右）：
/// label w96 左 + 控件 flex_1 右；`top_align` = 多行控件 label 上沿对齐。
pub(crate) fn field(label: &'static str, control: AnyElement, top_align: bool) -> AnyElement {
    let t = theme();
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .flex_shrink_0()
        .child(
            div()
                .w(px(96.))
                .flex_shrink_0()
                .text_size(ui_size(12.))
                .text_color(rgb(t.text))
                .when(top_align, |d| d.items_start().pt(px(8.)))
                .child(tr(label).to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(control),
        )
        .into_any_element()
}

/// 任务类型 seg 分段【脚本|Agent】（HTML segType）。
/// 视觉（2026-11-22 用户拍板「凸/凹」）：轨道 = 凹槽（bg_panel 沉底 + 边框 +
/// SP1 内衬），选中段 = 凸起（t.bg 浮出槽底 + t.border 描边 + SEMIBOLD 加粗，
/// 圆角 INLINE 不被轨道裁切），未选段 = 凹陷（透明底/透明描边 + text_muted）。
/// 左右等宽：两段同取两标签排版宽的较大者（见 [`seg_label_w`]）+ §5.1 SM 档
/// 水平 padding，文本居中——「脚本/Agent」内容宽差不再让两段一宽一窄。
/// 芯片走 SM_H 24：SP1 内衬×2 + 边框×2 后轨道总高仍 30，与行内 sel-btn 对齐。
pub(crate) fn type_seg(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    window: &gpui::Window,
) -> AnyElement {
    let cur_agent = chat.tasks.page.kind_agent;
    let labels = [
        tr("task.type_script").to_string(),
        tr("task.type_agent").to_string(),
    ];
    // 等宽基准按较重字重（SEMIBOLD）实测——选中态加粗不溢出，未选态
    // NORMAL 更窄必装得下
    let seg_w = labels
        .iter()
        .map(|l| seg_label_w(l, window))
        .fold(0.0_f32, f32::max)
        + btn::SM_PX * 2.;
    div()
        .flex_none()
        .flex()
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(radius::CTRL))
        .overflow_hidden()
        .bg(rgb(t.bg_panel))
        .p(px(space::SP1))
        .children([false, true].iter().map(|&agent| {
            let on = cur_agent == agent;
            let weak = weak.clone();
            div()
                .id(SharedString::from(format!("task-seg-{agent}")))
                .min_w(px(seg_w))
                .flex()
                .items_center()
                .justify_center()
                .h(px(btn::SM_H))
                .px(px(btn::SM_PX))
                .text_size(ui_size(12.))
                .rounded(px(radius::INLINE))
                // 描边两态都占位（透明 vs 实线），选中切换不推挤文字
                .border_1()
                .cursor_pointer()
                .when(on, |d| {
                    d.bg(rgb(t.bg))
                        .border_color(rgb(t.border))
                        .text_color(rgb(t.text))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                })
                .when(!on, |d| {
                    d.border_color(gpui::transparent_black())
                        .text_color(rgb(t.text_muted))
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
                    labels[1].clone()
                } else {
                    labels[0].clone()
                })
        }))
        .into_any_element()
}

/// seg 标签排版宽（两段等宽的基准）。家族 = 界面 chrome 默认族（panel_font，
/// shell body 根容器同族），字号 = 渲染字号 ui_size(12)（与渲染同口径，
/// psp_row_views 溢出测量同款）；字重取两态中较重的 SEMIBOLD。
fn seg_label_w(label: &str, window: &gpui::Window) -> f32 {
    let font = gpui::Font {
        family: crate::appearance::panel_font().family.clone().into(),
        features: gpui::FontFeatures::default(),
        fallbacks: None,
        weight: gpui::FontWeight::SEMIBOLD,
        style: gpui::FontStyle::Normal,
    };
    let run = gpui::TextRun {
        len: label.len(),
        font,
        color: rgb(0x000000).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    f32::from(
        window
            .text_system()
            .layout_line(label, ui_size(12.), &[run], None)
            .width,
    )
}

// ---------------------------------------------------------------------------
// 调度行（HTML schedRow：sel-btn + 摘要；菜单 = anchored pmenu，2026-11-15
// 用户拍板「下方只沿用 InputPanel 的操作栏」——composer bar_trigger 同机制）
// ---------------------------------------------------------------------------

/// sel-btn（h28 px8 border rounded 6 bg_panel gap 8 + chevron；hover accent 框）。
pub(crate) fn sel_btn(
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
pub(crate) fn trg_wrap(
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

pub(crate) fn weekday_key(i: usize) -> &'static str {
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

/// 工具档显示名（runtime `tool_preset_label` 同映射：内部键 custom →
/// full+ 档名，其余键原样。任务页嵌入态 input_area 读它出 pill 文案）
pub(crate) fn tools_display(key: &str) -> String {
    if key == "custom" {
        tr("extensions.full").to_string()
    } else {
        key.to_string()
    }
}

/// 调度行（随 unit 重排；菜单槽位 popup_slot::*）。
pub(crate) fn sched_row(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
) -> AnyElement {
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
pub(crate) fn kind_block(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    window: &gpui::Window,
    cx: &mut gpui::Context<Chat>,
) -> Vec<AnyElement> {
    if chat.tasks.page.kind_agent {
        agent_block(chat, weak, t, window, cx)
    } else {
        script_block(chat, weak, t)
    }
}

/// Agent 块（2026-11-17 拍板「InputPanel 直接搬上来」）：【项目】chip 行
/// （新会话页 action_bar 项目组件同款）+【指令】= **真 InputPanel**
/// （session::input::input_area 嵌入态：统一菜单/工具档/扩展/MCP/模型/
/// 思考全是会话区同一份接线，仅改高度/宽度、去发送钮——原手搓胶囊与
/// tasks 侧菜单分支整链退役）。
fn agent_block(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    window: &gpui::Window,
    cx: &mut gpui::Context<Chat>,
) -> Vec<AnyElement> {
    let page = &chat.tasks.page;
    // 焦点态 = 真 composer 持焦（胶囊边框 accent，inputpanel 同判定）
    let focused = chat
        .composer
        .composer
        .as_ref()
        .is_some_and(|c| c.read(cx).focus_handle_in(cx).is_focused(window));

    // 项目 chip（新会话 action_bar ns-open-project 同款：folder 图标 +
    // 项目名/选择项目，h28 hover 洗底；点击开 ProjectPicker 回填表单）
    let proj_label = page
        .project
        .as_ref()
        .and_then(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .filter(|n| !n.is_empty())
        })
        .unwrap_or_else(|| tr("action.select_project").to_string());
    let weak_proj = weak.clone();
    let proj_chip = div()
        .id("task-open-project")
        .h(px(btn::MD_H))
        .px(px(space::SP4))
        .flex()
        .items_center()
        .gap(px(space::SP3))
        .rounded(px(radius::CARD))
        .cursor_pointer()
        .text_size(ui_size(12.))
        .text_color(rgb(t.text_muted))
        .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            let _ = weak_proj.update(cx, |c, cx| {
                c.open_project_picker_for_task(cx);
            });
        })
        .child(icon("folder", crate::ui::tokens::icon::MD, t.text_muted))
        .child(SharedString::from(proj_label))
        .into_any_element();

    // 指令 = 真 InputPanel（嵌入态：高度 200 / 宽度随详情列 / 无发送钮；
    // 菜单读写任务字段见 pill.rs / model_picker 的 task_form 分支）
    let panel = crate::session::input::input_area(chat, weak, false, focused, true, true, cx);

    vec![
        field("task.project", proj_chip, false),
        // 项目 ↔ 指令 间距拉大到 50（2026-11-19 用户令；容器 gap 16 + mt 34）
        div()
            .mt(px(34.))
            .w_full()
            .child(field("task.instr", panel, true))
            .into_any_element(),
    ]
}

/// 脚本块（field 行 + 说明行）：只读 input 形态选择器 + 浏览…，note 带
/// 扩展名校验错误。选择器与名称输入统一样式（TextInput 单行同款：h30 /
/// w250 / bg_panel / border / INLINE 圆角，2026-11-22 用户令），只读、无
/// 内部提示文案；点击即开文件选择（原 sel-btn 下拉形态无点击处理器，
/// chevron 是摆设——顺手修掉这个死控件）。
fn script_block(chat: &Chat, weak: &gpui::WeakEntity<Chat>, t: &'static Theme) -> Vec<AnyElement> {
    let page = &chat.tasks.page;
    let script_text = page
        .script_path
        .as_ref()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let weak_pick = weak.clone();
    let picker = div()
        .id("task-sel-script")
        .w(px(250.))
        .h(px(input::H))
        .px(px(space::SP4))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(radius::INLINE))
        .bg(rgb(t.bg_panel))
        .hover(|h| h.border_color(rgb(t.accent)))
        .flex()
        .items_center()
        .text_size(ui_size(12.))
        .text_color(rgb(t.text))
        .cursor_pointer()
        .overflow_hidden()
        .child(
            div()
                .min_w_0()
                .truncate()
                .child(SharedString::from(script_text)),
        )
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            let _ = weak_pick.update(cx, |c, cx| c.pick_task_script_file(cx));
        });
    let note = div()
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
        ));
    vec![
        field(
            "task.script",
            div()
                .flex()
                .items_center()
                .gap(px(space::SP4))
                .child(picker)
                .child(config_button(
                    "task-browse",
                    weak,
                    tr("task.browse"),
                    Btn::Secondary,
                    true,
                    false,
                    |c, cx| c.pick_task_script_file(cx),
                ))
                .into_any_element(),
            false,
        ),
        note.into_any_element(),
    ]
}

// ---------------------------------------------------------------------------
// ② 日志 tab（截图拍板版：行 = 任务列表行同语言）
// ---------------------------------------------------------------------------
