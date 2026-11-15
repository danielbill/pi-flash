//! 046 任务管理大卡（§1 UI 唯一事实源落地）：模态大卡三 tab 左导航——
//! 任务列表 / 新建·编辑表单 / 运行历史。
//!
//! 装配对齐设置弹窗（settings/form.rs + `ui::overlay::big_card`）：状态住
//! 独立实体 [`TaskPanel`]（不占 `dlg.dialog` 单槽——项目/模型选择浮层可叠
//! 其上）；渲染前快照成 cx-free 的 [`TasksSnap`]；表单控件全取 `ui::kit`；
//! 焦点白名单走 [`TaskPanel::focus_within`] 结构化自检（focus.rs，v60/v70
//! 教训）。数据面读 `chat.tasks`（状态面缓存，泵/CRUD 刷新，不自建轮询）；
//! ⋯菜单走 PopupEdges 统一方向；tooltip 全 with_tip。

use gpui::{AnyElement, Entity, MouseButton, SharedString, div, prelude::*, px, rgba};

use pf_context::domain::tasks::{
    Outcome, ScheduleUnit, TaskKind, TaskRun, TaskSchedule, TaskSpec, Trigger,
};

use crate::appearance::theme::{self, Theme, theme};
use crate::appearance::ui_size;
use crate::i18n::{tf, tr};
use crate::services::format::{fmt_duration_ms, fmt_msg_time, now_ms};
use crate::session::menu_rows::{pill_row, thinking_rows, tools_preset_rows};
use crate::session::model_picker::{ModelPickAction, model_menu_body, model_menu_content_h};
use crate::tasks::DefRow;
use crate::tasks::store;
use crate::ui::dropdown::{DropdownState, dropdown};
use crate::ui::kit::{
    Btn, checkbox, config_button, error_note, note, radio, section_title, status_dot,
};
use crate::ui::menu::{menu_card, menu_item_sized, menu_layer, menu_sep};
use crate::ui::overlay::{self, big_card};
use crate::ui::popup::PopupEdges;
use crate::ui::text_input::TextInput;
use crate::ui::tokens::radius;
use crate::ui::tooltip::WithTip;
use crate::ui::vlist::{VListHeight, vlist};
use crate::ui::{icon, spark_spinner};
use crate::*;

const TAB_LIST: u8 = 0;
const TAB_FORM: u8 = 1;
const TAB_HISTORY: u8 = 2;

/// 间隔档（§1.3 下拉清单，默认 1）。
const INTERVALS: [u32; 10] = [1, 2, 3, 5, 10, 15, 20, 30, 45, 60];

/// 下拉互斥槽位号（panel.open_dd）。
const DD_NONE: u8 = 0;
const DD_UNIT: u8 = 1;
const DD_INTERVAL: u8 = 2;
const DD_HOUR: u8 = 3;
const DD_MINUTE: u8 = 4;
const DD_MONTH_DAY: u8 = 5;
const DD_TOOLS: u8 = 6;
const DD_THINKING: u8 = 7;
const DD_MODEL: u8 = 8;
const DD_RUN_FILTER: u8 = 9;

// ---------------------------------------------------------------------------
// 面板实体
// ---------------------------------------------------------------------------

/// 任务管理面板实体（打开期间常驻：输入框/滚动/弹层状态跨帧复用）。
pub(crate) struct TaskPanel {
    pub(crate) tab: u8,
    /// 编辑态 = Some(def_id)（同表单预填，标题/主钮换文案 + 删除钮）。
    pub(crate) edit_id: Option<String>,

    // ---- tab0 任务列表
    /// ⋯菜单展开中的 def id + 屏幕锚点（PopupEdges::around）。
    pub(crate) menu_open: Option<(String, (f32, f32))>,
    /// 删除确认（overlay::confirm 槽：(def_id, title)）。
    pub(crate) confirm_del: Option<(String, String)>,

    // ---- tab1 表单
    pub(crate) name: Entity<TextInput>,
    pub(crate) desc: Entity<TextInput>,
    /// false = 脚本（§1.3 单选默认脚本）、true = Agent。
    pub(crate) kind_agent: bool,
    pub(crate) unit_ix: usize,
    pub(crate) interval_ix: usize,
    pub(crate) hour: u32,
    pub(crate) minute: u32,
    /// 周一..周日（1–7，§1.7 weekday_{1..7}）。
    pub(crate) weekdays: [bool; 7],
    pub(crate) month_day: u32,
    pub(crate) project: Option<PathBuf>,
    pub(crate) tools: &'static str,
    pub(crate) model: Option<(String, String)>,
    pub(crate) thinking: &'static str,
    pub(crate) script_path: Option<PathBuf>,
    pub(crate) script_err: Option<String>,
    /// 当前展开的下拉（互斥；[`DD_NONE`] = 全关）。
    pub(crate) open_dd: u8,
    pub(crate) dd: Entity<DropdownState>,

    // ---- tab2 运行历史
    /// 任务过滤（None = 全部任务）。
    pub(crate) run_filter: Option<String>,
    /// 620 输出查看（脚本行点击）。
    pub(crate) out_view: Option<TaskRun>,
}

impl TaskPanel {
    pub(crate) fn new(cx: &mut gpui::Context<Self>) -> Self {
        Self {
            tab: TAB_LIST,
            edit_id: None,
            menu_open: None,
            confirm_del: None,
            name: cx.new(|cx| TextInput::new(cx)),
            desc: cx.new(|cx| TextInput::new(cx).multiline()),
            kind_agent: false,
            unit_ix: 2, // 每天
            interval_ix: 0,
            hour: 8,
            minute: 30,
            weekdays: [false; 7],
            month_day: 1,
            project: None,
            tools: "default",
            model: None,
            thinking: "auto",
            script_path: None,
            script_err: None,
            open_dd: 0,
            dd: cx.new(|_| DropdownState::new()),
            run_filter: None,
            out_view: None,
        }
    }

    /// 表单任意输入持焦自检（focus.rs 白名单；SettingsPanel::focus_within
    /// 同款结构化自检——白名单硬编码两度漏新输入框的教训）。
    pub(crate) fn focus_within(&self, window: &gpui::Window, cx: &gpui::App) -> bool {
        [&self.name, &self.desc]
            .iter()
            .any(|i| i.read(cx).focus_handle_in(cx).is_focused(window))
    }

    /// 清空表单回默认（新建态；项目默认 = 当前打开项目，§1.3）。
    fn reset_form(&mut self, default_project: Option<PathBuf>, cx: &mut gpui::Context<Self>) {
        self.edit_id = None;
        self.name
            .update(cx, |i, icx| i.set_value(String::new(), icx));
        self.desc
            .update(cx, |i, icx| i.set_value(String::new(), icx));
        self.kind_agent = false;
        self.unit_ix = 2;
        self.interval_ix = 0;
        self.hour = 8;
        self.minute = 30;
        self.weekdays = [false; 7];
        self.month_day = 1;
        self.project = default_project;
        self.tools = "default";
        self.model = None;
        self.thinking = "auto";
        self.script_path = None;
        self.script_err = None;
        self.open_dd = 0;
    }
}

fn upd_panel(
    c: &mut Chat,
    cx: &mut gpui::Context<Chat>,
    f: impl FnOnce(&mut TaskPanel, &mut gpui::Context<TaskPanel>),
) {
    if let Some(p) = c.dlg.tasks.clone() {
        let _ = p.update(cx, f);
    }
}

// ---------------------------------------------------------------------------
// 打开入口 + Chat 级动作
// ---------------------------------------------------------------------------

impl Chat {
    /// psp 控制栏「任务」钮：打开大卡（tab0 列表；dev-flag 由入口钮门住）。
    pub(crate) fn open_tasks_dialog(&mut self, cx: &mut Context<Self>) {
        let default_project = self.ws.cwd.clone();
        self.dlg.tasks = Some(cx.new(|cx| {
            let mut p = TaskPanel::new(cx);
            p.project = default_project;
            p
        }));
        cx.notify();
    }

    /// 项目选择（task_form 路由）：选中项目回填任务表单，不切工作区。
    pub(crate) fn set_task_form_project_inner(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if let Some(p) = self.dlg.tasks.clone() {
            let _ = p.update(cx, |p, _| p.project = Some(path));
        }
        cx.notify();
    }

    /// 任务表单「浏览…」：系统文件选择器 → 校验扩展名（script_exec 支持
    /// 集，§1.3 error_note）→ 回填。
    pub(crate) fn pick_task_script_file(&mut self, cx: &mut Context<Self>) {
        let opts = gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        };
        let rx = cx.prompt_for_paths(opts);
        cx.spawn(async move |this, cx| {
            let picked = rx.await.ok().and_then(|r| r.ok()).flatten();
            let Some(mut paths) = picked else { return };
            if let Some(file) = paths.pop() {
                let _ = this.update(cx, |c, cx| {
                    if let Some(p) = c.dlg.tasks.clone() {
                        let _ = p.update(cx, |p, _| {
                            p.script_err =
                                if crate::services::script_exec::exec_kind(&file).is_some() {
                                    None
                                } else {
                                    Some(tr("task.script_err_ext").to_string())
                                };
                            p.script_path = Some(file);
                        });
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 任务表单的项目目录选择器（gpui prompt；选完只回填表单）。
    pub(crate) fn pick_project_folder_for_task(&mut self, cx: &mut Context<Self>) {
        let opts = gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        };
        let rx = cx.prompt_for_paths(opts);
        cx.spawn(async move |this, cx| {
            let picked = rx.await.ok().and_then(|r| r.ok()).flatten();
            let Some(mut paths) = picked else { return };
            if let Some(dir) = paths.pop() {
                let _ = this.update(cx, |c, cx| {
                    c.set_task_form_project_inner(dir, cx);
                });
            }
        })
        .detach();
    }

    /// ⋯菜单「编辑」：同表单预填（§1.3 头注）。
    pub(crate) fn task_open_edit(&mut self, def_id: &str, cx: &mut Context<Self>) {
        let Some(row) = self.tasks.defs.iter().find(|d| d.def.id == def_id).cloned() else {
            return;
        };
        let DefRow { def, spec } = row;
        let sch = &spec.schedule;
        let units = [
            ScheduleUnit::Minute,
            ScheduleUnit::Hourly,
            ScheduleUnit::Daily,
            ScheduleUnit::Weekly,
            ScheduleUnit::Monthly,
        ];
        let unit_ix = units.iter().position(|u| *u == sch.unit).unwrap_or(2);
        let interval_ix = INTERVALS
            .iter()
            .position(|n| *n == sch.interval)
            .unwrap_or(0);
        let mut weekdays = [false; 7];
        for w in &sch.weekdays {
            if (1..=7).contains(w) {
                weekdays[*w as usize - 1] = true;
            }
        }
        let (kind_agent, model, thinking, tools, desc) = match &spec.kind {
            TaskKind::Agent => {
                let a =
                    spec.agent
                        .clone()
                        .unwrap_or_else(|| pf_context::domain::tasks::AgentSpec {
                            prompt: String::new(),
                            project: String::new(),
                            tools: "default".into(),
                            model: None,
                            thinking: "auto".into(),
                        });
                let thinking = pf_context::domain::tasks::THINKING_LEVELS
                    .iter()
                    .find(|k| **k == a.thinking)
                    .copied()
                    .unwrap_or("auto");
                let tools = pf_context::domain::tasks::TOOLS_PRESETS
                    .iter()
                    .find(|k| **k == a.tools)
                    .copied()
                    .unwrap_or("default");
                (true, a.model.clone(), thinking, tools, a.prompt.clone())
            }
            TaskKind::Script => (false, None, "auto", "default", String::new()),
        };
        let script_path = spec.script.clone().map(|s| PathBuf::from(s.path));
        if let Some(p) = self.dlg.tasks.clone() {
            let _ = p.update(cx, |p, pcx| {
                p.edit_id = Some(def.id.clone());
                p.name
                    .update(pcx, |i, icx| i.set_value(def.title.clone(), icx));
                p.desc.update(pcx, |i, icx| i.set_value(desc.clone(), icx));
                p.kind_agent = kind_agent;
                p.unit_ix = unit_ix;
                p.interval_ix = interval_ix;
                p.hour = sch.hour as u32;
                p.minute = sch.minute as u32;
                p.weekdays = weekdays;
                p.month_day = sch.month_day.max(1) as u32;
                p.project = Some(PathBuf::from(
                    spec.agent
                        .as_ref()
                        .map(|a| a.project.clone())
                        .unwrap_or_default(),
                ));
                p.model = model;
                p.thinking = thinking;
                p.tools = tools;
                p.script_path = script_path;
                p.script_err = None;
                p.open_dd = DD_NONE;
                p.tab = TAB_FORM;
            });
        }
        cx.notify();
    }

    /// 删除确认通过：运行中先中止（script = kill 旗标 / agent = Interrupt
    /// 途径）再删（同事务连带删 runs，store::delete_task）。
    pub(crate) fn task_delete_confirmed(&mut self, def_id: &str, cx: &mut Context<Self>) {
        if let Some(key) = crate::tasks::run::pending_key_for(self, def_id) {
            if let Some(ctx) = self.tasks.pending.remove(&key) {
                if let Some(flag) = &ctx.kill {
                    flag.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                if let Some(rt) = &ctx.rt {
                    rt.update(cx, |r, rcx| r.interrupt_from_composer(rcx));
                }
            }
            crate::services::scheduler::clear_in_flight(def_id);
        }
        if let Err(e) = store::delete_task(def_id) {
            eprintln!("[tasks] 删除失败: {e}");
        }
        upd_panel(self, cx, |p, _| {
            p.confirm_del = None;
            p.menu_open = None;
        });
        self.tasks_refresh();
        cx.notify();
    }

    /// 表单提交（创建/保存共用）：面板快照 → TaskSpec → 存储层（校验非法
    /// 拒写 / 创建事务内 COUNT≤20）。
    pub(crate) fn task_save_form(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = self.dlg.tasks.clone() else {
            return;
        };
        let s = snapshot(panel.read(cx), cx);
        if s.name.trim().is_empty() {
            return;
        }
        let spec = match build_spec(&s) {
            Some(sp) => sp,
            None => return,
        };
        let now = now_ms();
        let result = match &s.edit_id {
            Some(id) => store::save_edit(id, &s.name, &spec, true, now).map(|_| id.clone()),
            None => store::create_task(&s.name, &spec, now),
        };
        match result {
            Ok(_) => {
                let _ = panel.update(cx, |p, _| {
                    p.edit_id = None;
                    p.tab = TAB_LIST;
                });
                self.tasks_refresh();
                crate::services::scheduler::request_tick();
            }
            Err(e) => {
                // 上限等存储层错误 → toast（文案含 task.limit_reached 语义）
                let msg = if e.contains("上限") {
                    tr("task.limit_reached").to_string()
                } else {
                    e
                };
                self.dlg.status_toast = Some((msg, std::time::Instant::now()));
            }
        }
        cx.notify();
    }

    /// 表单「取消」：回列表。
    pub(crate) fn task_cancel_form(&mut self, cx: &mut Context<Self>) {
        upd_panel(self, cx, |p, _| {
            p.edit_id = None;
            p.tab = TAB_LIST;
            p.open_dd = DD_NONE;
        });
        cx.notify();
    }

    /// 编辑态「删除」钮：直接落确认槽（文案 + Danger 确认，§1.5）。
    pub(crate) fn task_confirm_delete_editing(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self
            .dlg
            .tasks
            .as_ref()
            .and_then(|p| p.read(cx).edit_id.clone())
        else {
            return;
        };
        let title = self
            .tasks
            .defs
            .iter()
            .find(|d| d.def.id == id)
            .map(|d| d.def.title.clone())
            .unwrap_or_default();
        upd_panel(self, cx, |p, _| p.confirm_del = Some((id, title)));
        cx.notify();
    }
}

/// 项目选择（task_form 路由）转接点（project_picker.rs 行点击/键盘 Enter 调）。
pub(crate) fn set_task_form_project(c: &mut Chat, path: PathBuf, cx: &mut Context<Chat>) {
    c.set_task_form_project_inner(path, cx);
}

/// 任务表单的目录选择（键盘 Enter / 【打开文件夹】行路由）。
pub(crate) fn pick_task_project_folder(c: &mut Chat, cx: &mut Context<Chat>) {
    c.pick_project_folder_for_task(cx);
}

// ---------------------------------------------------------------------------
// 快照
// ---------------------------------------------------------------------------

struct TasksSnap {
    tab: u8,
    edit_id: Option<String>,
    menu_open: Option<(String, (f32, f32))>,
    confirm_del: Option<(String, String)>,
    name: String,
    desc: String,
    kind_agent: bool,
    unit_ix: usize,
    interval_ix: usize,
    hour: u32,
    minute: u32,
    weekdays: [bool; 7],
    month_day: u32,
    project: Option<PathBuf>,
    tools: &'static str,
    model: Option<(String, String)>,
    thinking: &'static str,
    script_path: Option<PathBuf>,
    script_err: Option<String>,
    open_dd: u8,
    run_filter: Option<String>,
    out_view: Option<TaskRun>,
}

fn snapshot(p: &TaskPanel, cx: &gpui::App) -> TasksSnap {
    TasksSnap {
        tab: p.tab,
        edit_id: p.edit_id.clone(),
        menu_open: p.menu_open.clone(),
        confirm_del: p.confirm_del.clone(),
        name: p.name.read(cx).value().to_string(),
        desc: p.desc.read(cx).value().to_string(),
        kind_agent: p.kind_agent,
        unit_ix: p.unit_ix,
        interval_ix: p.interval_ix,
        hour: p.hour,
        minute: p.minute,
        weekdays: p.weekdays,
        month_day: p.month_day,
        project: p.project.clone(),
        tools: p.tools,
        model: p.model.clone(),
        thinking: p.thinking,
        script_path: p.script_path.clone(),
        script_err: p.script_err.clone(),
        open_dd: p.open_dd,
        run_filter: p.run_filter.clone(),
        out_view: p.out_view.clone(),
    }
}

/// 面板快照 → TaskSpec（§1.3 字段装配；非法组合不会出现——UI 互斥约束）。
fn build_spec(s: &TasksSnap) -> Option<TaskSpec> {
    let units = [
        ScheduleUnit::Minute,
        ScheduleUnit::Hourly,
        ScheduleUnit::Daily,
        ScheduleUnit::Weekly,
        ScheduleUnit::Monthly,
    ];
    let schedule = TaskSchedule {
        unit: *units.get(s.unit_ix)?,
        interval: INTERVALS[s.interval_ix.min(INTERVALS.len() - 1)],
        hour: s.hour.min(23) as u8,
        minute: s.minute.min(59) as u8,
        weekdays: s
            .weekdays
            .iter()
            .enumerate()
            .filter(|(_, v)| **v)
            .map(|(i, _)| i as u8 + 1)
            .collect(),
        month_day: s.month_day.clamp(1, 31) as u8,
    };
    Some(if s.kind_agent {
        TaskSpec {
            kind: TaskKind::Agent,
            schedule,
            agent: Some(pf_context::domain::tasks::AgentSpec {
                prompt: s.desc.clone(),
                project: s
                    .project
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default(),
                tools: s.tools.to_string(),
                model: s.model.clone(),
                thinking: s.thinking.to_string(),
            }),
            script: None,
        }
    } else {
        TaskSpec {
            kind: TaskKind::Script,
            schedule,
            agent: None,
            script: Some(pf_context::domain::tasks::ScriptSpec {
                path: s
                    .script_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default(),
            }),
        }
    })
}

// ---------------------------------------------------------------------------
// 大卡装配
// ---------------------------------------------------------------------------

pub(crate) fn render_tasks(
    chat: &mut Chat,
    panel: &Entity<TaskPanel>,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<Chat>,
) -> gpui::Div {
    let s = snapshot(panel.read(cx), cx);
    let vp = window.viewport_size();
    let vp = (f32::from(vp.width), f32::from(vp.height));

    let close = {
        let weak = weak.clone();
        move |_: &mut gpui::Window, cx: &mut gpui::App| {
            let _ = weak.update(cx, |c, cx| {
                c.dlg.tasks = None;
                cx.notify();
            });
        }
    };
    let editing = s.tab == TAB_FORM && s.edit_id.is_some();
    let title: &str = if editing {
        tr("task.tab_edit")
    } else {
        tr("task.manager")
    };

    let nav = nav_items(&s, weak);
    let body: AnyElement = match s.tab {
        TAB_FORM => form_body(chat, &s, panel, weak, t, cx),
        TAB_HISTORY => history_body(chat, &s, panel, weak, t, cx),
        _ => list_body(chat, weak, t),
    };
    let mut root = overlay::layer(true, Some(&chat.dlg.dialog_focus), close.clone())
        .flex()
        .items_center()
        .justify_center()
        .child(big_card(title, Some(nav), body, t, close));

    // ⋯行菜单（PopupEdges 统一方向；叠大卡之上）
    if s.menu_open.is_some() {
        if let Some(el) = row_menu_layer(chat, &s, weak, vp, t) {
            root = root.child(el);
        }
    }
    // 删除确认（§1.5：文案 + 右对齐按钮行，确认钮 Danger 变体）
    if let Some((del_id, del_title)) = &s.confirm_del {
        let message = tf(tr("task.delete_confirm"), &[("title", del_title.clone())]);
        let weak_cancel = weak.clone();
        let weak_confirm = weak.clone();
        let del_id = del_id.clone();
        root = root.child(overlay::confirm(
            &chat.dlg.dialog_focus,
            message,
            move |_, cx| {
                let _ = weak_cancel.update(cx, |c, cx| {
                    upd_panel(c, cx, |p, _| p.confirm_del = None);
                    cx.notify();
                });
            },
            move |_, cx| {
                let _ = weak_confirm.update(cx, |c, cx| {
                    c.task_delete_confirmed(&del_id, cx);
                });
            },
        ));
    }
    // 脚本输出查看（620 档：输出尾 + exit code）
    if let Some(run) = &s.out_view {
        root = root.child(out_view_dialog(run, weak, t));
    }
    root
}

/// 左导航三项（§1.1；样式照抄 settings/form.rs nav_items）。
fn nav_items(s: &TasksSnap, weak: &gpui::WeakEntity<Chat>) -> AnyElement {
    let t = theme();
    let items: [(u8, &'static str, &'static str); 3] = [
        (TAB_LIST, "file-text", "task.tab_list"),
        (TAB_FORM, "file-plus", "task.tab_new"),
        (TAB_HISTORY, "clock", "task.tab_history"),
    ];
    div()
        .w(px(160.))
        .flex_shrink_0()
        .bg(rgb(t.nav))
        .rounded_bl(px(10.))
        .border_r_1()
        .border_color(rgba(theme::border_alpha(t, 0x66)))
        .flex()
        .flex_col()
        .p(px(8.))
        .pt(px(12.))
        .children(items.iter().map(|&(ix, ic, key)| {
            let active = s.tab == ix;
            let weak = weak.clone();
            div()
                .id(SharedString::from(format!("task-nav-{ix}")))
                .flex()
                .items_center()
                .gap(px(9.))
                .px(px(10.))
                .py(px(7.5))
                .mb(px(1.))
                .rounded(px(8.))
                .cursor_pointer()
                .text_size(ui_size(13.))
                .when(active, |d| {
                    d.bg(rgb(t.bg_selected))
                        .text_color(rgb(t.text))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                })
                .when(!active, |d| {
                    d.text_color(rgb(t.text_muted))
                        .hover(|h| h.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                })
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak.update(cx, |c, cx| {
                        upd_panel(c, cx, |p, _| {
                            p.tab = ix;
                            p.open_dd = DD_NONE;
                            p.menu_open = None;
                        });
                        cx.notify();
                    });
                })
                .child(icon(ic, 15., if active { t.accent } else { t.text_muted }))
                .child(tr(key).to_string())
        }))
        .into_any_element()
}

// ---------------------------------------------------------------------------
// tab0 任务列表（§1.2）
// ---------------------------------------------------------------------------

/// 列表行渲染数据（vlist rows 闭包 own 全部所需）。
struct ListRow {
    id: String,
    title: String,
    kind_agent: bool,
    enabled: bool,
    running: bool,
    /// (结果点色, hover 摘要) —— 无记录 = None。
    dot: Option<(u32, String)>,
    session_path: Option<PathBuf>,
    last_time: Option<i64>,
}

fn list_body(chat: &Chat, weak: &gpui::WeakEntity<Chat>, _t: &'static Theme) -> AnyElement {
    let t = theme();
    let limit = pf_context::domain::tasks::TASK_CREATE_LIMIT;
    let count = chat.tasks.defs.len();
    let at_limit = count >= limit;

    // 头行：左计数 mono ui(10) text_dim；右「+ 新建任务」config_button small
    let header = div()
        .flex()
        .items_center()
        .mb(px(10.))
        .child(
            div()
                .flex_1()
                .text_size(ui_size(10.))
                .text_color(rgb(t.text_dim))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(tf(
                    tr("task.count"),
                    &[("n", count.to_string()), ("max", limit.to_string())],
                )),
        )
        .child(
            div()
                .id("task-new-tip")
                .when(at_limit, |d| d.with_tip("task.limit_reached"))
                .child(config_button(
                    "task-new-btn",
                    weak,
                    &format!("＋ {}", tr("task.new")),
                    Btn::Secondary,
                    true,
                    at_limit,
                    move |c, cx| {
                        let proj = c.ws.cwd.clone();
                        upd_panel(c, cx, |p, pcx| {
                            p.reset_form(proj, pcx);
                            p.tab = TAB_FORM;
                        });
                        cx.notify();
                    },
                )),
        );

    // 行数据（own 进 vlist 闭包）
    let running: std::collections::HashSet<&String> = chat.tasks.running.iter().collect();
    let mut last: std::collections::HashMap<&String, &TaskRun> = std::collections::HashMap::new();
    for r in &chat.tasks.runs {
        last.entry(&r.def_id).or_insert(r);
    }
    let rows: Vec<ListRow> = chat
        .tasks
        .defs
        .iter()
        .map(|DefRow { def, spec }| {
            let lr = last.get(&def.id).copied();
            ListRow {
                id: def.id.clone(),
                title: def.title.clone(),
                kind_agent: matches!(spec.kind, TaskKind::Agent),
                enabled: def.enabled,
                running: running.contains(&def.id),
                dot: lr.map(run_dot),
                session_path: lr.and_then(|r| r.session_path.clone()).map(PathBuf::from),
                last_time: lr.map(|r| r.started_at),
            }
        })
        .collect();

    let weak_rows = weak.clone();
    let list = vlist(
        "task-list-vlist",
        rows.len(),
        32.,
        VListHeight::Fill,
        true,
        true,
        "task.empty_list",
        None,
        move |ix, _, _| list_row_view(&rows[ix], &weak_rows, t),
    );

    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(header)
        .child(list)
        .into_any_element()
}

/// 运行结果 → (结果点色, hover 摘要：时间 + 结果 + 原因)。
fn run_dot(r: &TaskRun) -> (u32, String) {
    let t = theme();
    let time = fmt_msg_time(r.started_at);
    let (color, outcome) = match r.outcome {
        Outcome::Running => (t.accent, tr("task.running")),
        Outcome::Ok => (theme::NOTICE, tr("task.outcome_ok")),
        Outcome::Failed => (t.danger, tr("task.outcome_failed")),
        Outcome::Timeout => (theme::WARN, tr("task.outcome_timeout")),
        Outcome::Skipped => (t.text_faint, tr("task.outcome_skipped")),
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
    let mut tip = format!("{time} · {outcome}");
    if let Some(reason) = reason {
        tip.push_str(" · ");
        tip.push_str(&reason);
    }
    (color, tip)
}

/// 列表行（h32 主列表行形制，§1.2）。
fn list_row_view(row: &ListRow, weak: &gpui::WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    let icon_name = if row.kind_agent { "bot" } else { "file" };
    let name_color = if row.enabled { t.text } else { t.text_faint };

    let mut name_el = div()
        .id(SharedString::from(format!("task-row-name-{}", row.id)))
        .flex_1()
        .min_w_0()
        .truncate()
        .text_size(ui_size(12.))
        .text_color(rgb(name_color))
        .child(SharedString::from(row.title.clone()));
    // agent 任务名可点击 = 打开最近一次运行会话（§1.2；无运行记录不可点）
    if row.kind_agent {
        if let Some(sp) = &row.session_path {
            let sp = sp.clone();
            let weak_open = weak.clone();
            name_el = name_el
                .cursor_pointer()
                .hover(|h| h.text_color(rgb(t.accent)))
                .with_tip("task.open_session_tip")
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    cx.stop_propagation();
                    let _ = weak_open.update(cx, |c, cx| {
                        c.open_session(sp.clone(), false, cx);
                    });
                });
        }
    }

    let weak_menu = weak.clone();
    let menu_id = row.id.clone();
    div()
        .id(SharedString::from(format!("task-row-{}", row.id)))
        .h(px(32.))
        .px(px(10.))
        .gap(px(8.))
        .rounded(px(radius::INLINE))
        .flex()
        .items_center()
        .hover(|h| h.bg(rgb(t.bg_hover)))
        .child(icon(icon_name, 14., t.text_muted))
        .child(name_el)
        // 停用态（§1.2 变通：行不透明度不动，状态列转灰）
        .when(!row.enabled, |d| {
            d.child(
                div()
                    .text_size(ui_size(10.))
                    .text_color(rgb(t.text_faint))
                    .child(tr("task.disabled").to_string()),
            )
        })
        // 运行中指示 15px 槽 spinner（仅 running 显示，§8）
        .child(
            div()
                .w(px(15.))
                .flex()
                .justify_center()
                .children(row.running.then(|| spark_spinner(15., t.accent))),
        )
        // 最后执行结果 7px 通知点（无记录不显示；hover 走 with_tip）
        .child(
            div()
                .id(SharedString::from(format!("task-dot-{}", row.id)))
                .w(px(15.))
                .flex()
                .justify_center()
                .when_some(row.dot.clone(), |d, (color, tip)| {
                    d.with_tip_text(tip).child(status_dot(color))
                }),
        )
        // 最后运行时间：右对齐定宽列 ui(10) text_faint（面板 −2 档）
        .child(
            div()
                .w(px(96.))
                .flex()
                .justify_end()
                .text_size(ui_size(10.))
                .text_color(rgb(t.text_faint))
                .child(div().truncate().child(SharedString::from(
                    row.last_time.map(fmt_msg_time).unwrap_or_default(),
                ))),
        )
        // 行尾 ⋯ 菜单钮（IB_SM 22 槽 icon 14）
        .child(
            div()
                .id(SharedString::from(format!("task-row-menu-{menu_id}")))
                .size(px(22.))
                .rounded(px(radius::CTRL))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|h| h.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |ev, _, cx| {
                    cx.stop_propagation();
                    let pos = (f32::from(ev.position.x), f32::from(ev.position.y));
                    let _ = weak_menu.update(cx, |c, cx| {
                        upd_panel(c, cx, |p, _| p.menu_open = Some((menu_id.clone(), pos)));
                        cx.notify();
                    });
                })
                .child(icon("ellipsis", 14., t.text_muted)),
        )
        .into_any_element()
}

/// 行 ⋯菜单（§1.2 行菜单）：编辑 / 立即运行（运行中置灰）/ 启用⇄停用 /
/// ─ / 删除（danger 文字 + danger wash hover 由 menu_item_sized danger 承担）。
fn row_menu_layer(
    chat: &Chat,
    s: &TasksSnap,
    weak: &gpui::WeakEntity<Chat>,
    vp: (f32, f32),
    t: &'static Theme,
) -> Option<AnyElement> {
    let (def_id, anchor) = s.menu_open.as_ref()?;
    let def_id = def_id.clone();
    let def = chat.tasks.defs.iter().find(|d| d.def.id == def_id)?;
    let running = chat.tasks.running.iter().any(|k| *k == def_id);
    let enabled = def.def.enabled;

    let edges = PopupEdges::around(*anchor, vp, 0., 12.);
    let layer = menu_layer(Some(&chat.dlg.dialog_focus), weak, |c, cx| {
        upd_panel(c, cx, |p, _| p.menu_open = None);
    });

    let weak_edit = weak.clone();
    let weak_run = weak.clone();
    let weak_toggle = weak.clone();
    let weak_del = weak.clone();
    let (id_edit, id_run, id_toggle, id_del) = (
        def_id.clone(),
        def_id.clone(),
        def_id.clone(),
        def_id.clone(),
    );
    let menu = menu_card(edges, t)
        .child(
            menu_item_sized(
                "task-m-edit",
                "sliders-horizontal",
                14.,
                tr("task.edit_menu"),
                false,
                false,
            )
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                let _ = weak_edit.update(cx, |c, cx| {
                    upd_panel(c, cx, |p, _| p.menu_open = None);
                    c.task_open_edit(&id_edit, cx);
                });
            }),
        )
        .child(
            menu_item_sized("task-m-run", "send", 14., tr("task.run_now"), false, false)
                .when(running, |d| d.opacity(0.5))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    cx.stop_propagation();
                    if running {
                        return; // 运行中置灰双保险（§1.5）
                    }
                    let _ = weak_run.update(cx, |c, cx| {
                        upd_panel(c, cx, |p, _| p.menu_open = None);
                        c.task_run_now(&id_run, cx);
                        cx.notify();
                    });
                }),
        )
        .child(
            menu_item_sized(
                "task-m-toggle",
                if enabled { "eye-off" } else { "eye" },
                14.,
                if enabled {
                    tr("task.disable")
                } else {
                    tr("task.enable")
                },
                false,
                false,
            )
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                let _ = weak_toggle.update(cx, |c, cx| {
                    let on = !enabled;
                    if let Err(e) = store::set_enabled_task(&id_toggle, on, now_ms()) {
                        eprintln!("[tasks] 启停失败: {e}");
                    }
                    upd_panel(c, cx, |p, _| p.menu_open = None);
                    c.tasks_refresh();
                    cx.notify();
                });
            }),
        )
        .child(menu_sep())
        .child(
            menu_item_sized("task-m-del", "trash", 14., tr("task.delete"), true, false)
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    cx.stop_propagation();
                    let id_del = id_del.clone();
                    let _ = weak_del.update(cx, |c, cx| {
                        let title = c
                            .tasks
                            .defs
                            .iter()
                            .find(|d| d.def.id == id_del)
                            .map(|d| d.def.title.clone())
                            .unwrap_or_default();
                        upd_panel(c, cx, |p, _| {
                            p.menu_open = None;
                            p.confirm_del = Some((id_del, title));
                        });
                        cx.notify();
                    });
                }),
        );
    Some(layer.child(menu).into_any_element())
}

// ---------------------------------------------------------------------------
// 下拉小件（§10.4：触发器 250×30 r5 px9；弹层 min_w 限高滚动）
// ---------------------------------------------------------------------------

/// 下拉触发器。
fn dd_trigger(label: String, w: f32) -> AnyElement {
    let t = theme();
    div()
        .w(px(w))
        .h(px(30.))
        .px(px(9.))
        .rounded(px(radius::INLINE))
        .border_1()
        .border_color(rgba(theme::border_alpha(t, 0x73)))
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(ui_size(12.))
        .text_color(rgb(t.text))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(SharedString::from(label)),
        )
        .child(icon("chevron-down", 12., t.text_muted))
        .into_any_element()
}

/// 下拉弹层基座：限高滚动 + 菜单卡视觉（§5.6 容器 min_w200 p4 r8）。
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

/// 下拉选项行（px10/py7 ui(12) gap8 + ✓ 槽，§5.6 行形制）。
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
                upd_panel(c, cx, |p, _| p.open_dd = DD_NONE);
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

/// 开/合下拉的面板写回（触发钮 on_toggle）。
fn dd_toggle(
    slot: u8,
    weak: &gpui::WeakEntity<Chat>,
) -> impl Fn(&mut gpui::Window, &mut gpui::App) + 'static {
    let weak = weak.clone();
    move |_, cx| {
        let _ = weak.update(cx, |c, cx| {
            upd_panel(c, cx, |p, _| p.open_dd = slot);
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
            upd_panel(c, cx, |p, _| p.open_dd = DD_NONE);
            cx.notify();
        });
    }
}

// ---------------------------------------------------------------------------
// tab1 表单（§1.3）
// ---------------------------------------------------------------------------

/// 表单行：label 列 w96 + 控件（§10.4 表单骨架；label = section_title ui(11)
/// SEMIBOLD text_dim）。`top` = 控件高于单行时顶对齐（描述多行框）。
fn form_row(label: &'static str, control: AnyElement, top: bool) -> AnyElement {
    let t = theme();
    div()
        .flex()
        .when(top, |d| d.items_start())
        .when(!top, |d| d.items_center())
        .mb(px(8.))
        .child(
            div()
                .w(px(96.))
                .flex_shrink_0()
                .pt(px(if top { 5. } else { 0. }))
                .text_size(ui_size(11.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text_dim))
                .child(tr(label).to_string()),
        )
        .child(control)
        .into_any_element()
}

fn form_body(
    chat: &Chat,
    s: &TasksSnap,
    panel: &Entity<TaskPanel>,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    cx: &gpui::Context<Chat>,
) -> AnyElement {
    let t2 = theme();
    let dd = panel.read(cx).dd.clone();
    let name = panel.read(cx).name.clone();
    let desc = panel.read(cx).desc.clone();

    let units = [
        ("task.unit_minute", ScheduleUnit::Minute),
        ("task.unit_hour", ScheduleUnit::Hourly),
        ("task.unit_day", ScheduleUnit::Daily),
        ("task.unit_week", ScheduleUnit::Weekly),
        ("task.unit_month", ScheduleUnit::Monthly),
    ];
    let unit_label = |ix: usize| tr(units[ix.min(4)].0).to_string();
    let time_suffix = |kind: u8| -> &'static str {
        if kind == 0 {
            tr("task.hour_suffix")
        } else {
            tr("task.minute_suffix")
        }
    };

    // 任务类型单选组（§5.4）
    let kind_row = div()
        .flex()
        .gap(px(24.))
        .children([false, true].iter().map(|&agent| {
            let active = s.kind_agent == agent;
            let weak = weak.clone();
            div()
                .id(SharedString::from(format!("task-kind-{agent}")))
                .flex()
                .items_center()
                .gap(px(7.))
                .cursor_pointer()
                .font_weight(if active {
                    gpui::FontWeight::SEMIBOLD
                } else {
                    gpui::FontWeight::NORMAL
                })
                .text_color(rgb(t2.text_muted))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak.update(cx, |c, cx| {
                        upd_panel(c, cx, |p, _| p.kind_agent = agent);
                        cx.notify();
                    });
                })
                .child(radio(active))
                .child(div().text_size(ui_size(12.)).child(if agent {
                    tr("task.type_agent").to_string()
                } else {
                    tr("task.type_script").to_string()
                }))
        }));

    // ---- 定时设置（周期/时间随 unit 重排，§1.3）
    let weak_dd = weak.clone();
    let unit_dd = dropdown(
        "task-dd-unit",
        &dd,
        s.open_dd == DD_UNIT,
        dd_toggle(DD_UNIT, weak),
        dd_dismiss(weak),
        dd_trigger(unit_label(s.unit_ix), 250.),
        move || {
            dd_popup(
                units
                    .iter()
                    .enumerate()
                    .map(|(ix, (key, _))| {
                        let id = SharedString::from(format!("task-opt-unit-{ix}"));
                        dd_option(
                            id,
                            tr(key).to_string(),
                            s.unit_ix == ix,
                            move |c, cx| {
                                upd_panel(c, cx, |p, _| {
                                    // 切周期 = 重锚语义在保存时判定（§2.2）
                                    p.unit_ix = ix;
                                    p.open_dd = DD_NONE;
                                });
                                cx.notify();
                            },
                            &weak_dd,
                        )
                    })
                    .collect(),
                250.,
                200.,
            )
        },
    );

    // 时间行（随 unit）：每分钟=间隔；每小时=分；每天=时+分；每周=星期+时+分；
    // 每月=日+时+分
    // 选项急构（借 s/weak 调用；产出的行全部 'static，可 move 进 popup 闭包）
    let hour_items = || -> Vec<AnyElement> {
        (0..24)
            .map(|n| {
                dd_option(
                    SharedString::from(format!("task-opt-hour-{n}")),
                    format!("{n} {}", time_suffix(0)),
                    s.hour == n,
                    move |c, cx| {
                        upd_panel(c, cx, |p, _| {
                            p.hour = n;
                            p.open_dd = DD_NONE;
                        });
                        cx.notify();
                    },
                    weak,
                )
            })
            .collect()
    };
    let minute_items = || -> Vec<AnyElement> {
        (0..60)
            .map(|n| {
                dd_option(
                    SharedString::from(format!("task-opt-minute-{n}")),
                    format!("{n} {}", time_suffix(1)),
                    s.minute == n,
                    move |c, cx| {
                        upd_panel(c, cx, |p, _| {
                            p.minute = n;
                            p.open_dd = DD_NONE;
                        });
                        cx.notify();
                    },
                    weak,
                )
            })
            .collect()
    };

    let mut time_rows: Vec<AnyElement> = Vec::new();
    if s.unit_ix == 0 {
        // 每分钟：间隔下拉（1/2/3/5/10/15/20/30/45/60，默认 1）
        let interval_label = format!(
            "{} {}",
            INTERVALS[s.interval_ix.min(9)],
            tr("task.minute_suffix")
        );
        time_rows.push(form_row(
            "task.interval",
            dropdown(
                "task-dd-interval",
                &dd,
                s.open_dd == DD_INTERVAL,
                dd_toggle(DD_INTERVAL, weak),
                dd_dismiss(weak),
                dd_trigger(interval_label, 250.),
                move || {
                    dd_popup(
                        INTERVALS
                            .iter()
                            .enumerate()
                            .map(|(ix, n)| {
                                let id = SharedString::from(format!("task-opt-interval-{ix}"));
                                dd_option(
                                    id,
                                    format!("{n} {}", tr("task.minute_suffix")),
                                    s.interval_ix == ix,
                                    move |c, cx| {
                                        upd_panel(c, cx, |p, _| {
                                            p.interval_ix = ix;
                                            p.open_dd = DD_NONE;
                                        });
                                        cx.notify();
                                    },
                                    weak,
                                )
                            })
                            .collect(),
                        250.,
                        220.,
                    )
                },
            ),
            false,
        ));
    } else {
        // 时下拉（每天/每周/每月）
        if s.unit_ix >= 2 {
            time_rows.push(form_row(
                "task.time",
                div()
                    .flex()
                    .gap(px(8.))
                    .child(dropdown(
                        "task-dd-hour",
                        &dd,
                        s.open_dd == DD_HOUR,
                        dd_toggle(DD_HOUR, weak),
                        dd_dismiss(weak),
                        dd_trigger(format!("{} {}", s.hour, time_suffix(0)), 121.),
                        move || dd_popup(hour_items(), 121., 260.),
                    ))
                    .child(dropdown(
                        "task-dd-minute",
                        &dd,
                        s.open_dd == DD_MINUTE,
                        dd_toggle(DD_MINUTE, weak),
                        dd_dismiss(weak),
                        dd_trigger(format!("{} {}", s.minute, time_suffix(1)), 121.),
                        move || dd_popup(minute_items(), 121., 260.),
                    ))
                    .into_any_element(),
                false,
            ));
        } else {
            // 每小时：只对齐分
            time_rows.push(form_row(
                "task.time",
                dropdown(
                    "task-dd-minute",
                    &dd,
                    s.open_dd == DD_MINUTE,
                    dd_toggle(DD_MINUTE, weak),
                    dd_dismiss(weak),
                    dd_trigger(format!("{} {}", s.minute, time_suffix(1)), 250.),
                    move || dd_popup(minute_items(), 250., 260.),
                )
                .into_any_element(),
                false,
            ));
        }
        // 每周：星期 checkbox 组（14×14 r3、勾 10 accent、标签单字 ui(12)）
        if s.unit_ix == 3 {
            let week_row = div().flex().gap(px(8.)).children((0..7).map(|i| {
                let on = s.weekdays[i];
                let weak = weak.clone();
                let wd_key: &'static str = match i {
                    0 => "task.weekday_1",
                    1 => "task.weekday_2",
                    2 => "task.weekday_3",
                    3 => "task.weekday_4",
                    4 => "task.weekday_5",
                    5 => "task.weekday_6",
                    _ => "task.weekday_7",
                };
                div()
                    .id(SharedString::from(format!("task-wd-{i}")))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .cursor_pointer()
                    .text_size(ui_size(12.))
                    .text_color(rgb(if on { t2.text } else { t2.text_muted }))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let _ = weak.update(cx, |c, cx| {
                            upd_panel(c, cx, |p, _| p.weekdays[i] = !p.weekdays[i]);
                            cx.notify();
                        });
                    })
                    .child(checkbox(on))
                    .child(tr(wd_key).to_string())
            }));
            time_rows.push(form_row(
                "task.unit_week",
                week_row.into_any_element(),
                false,
            ));
        }
        // 每月：日下拉 + 时间
        if s.unit_ix == 4 {
            let day_label = format!("{} {}", s.month_day, tr("task.day_suffix"));
            time_rows.push(form_row(
                "task.month_day",
                dropdown(
                    "task-dd-monthday",
                    &dd,
                    s.open_dd == DD_MONTH_DAY,
                    dd_toggle(DD_MONTH_DAY, weak),
                    dd_dismiss(weak),
                    dd_trigger(day_label, 250.),
                    move || {
                        dd_popup(
                            (1..=31)
                                .map(|n| {
                                    dd_option(
                                        SharedString::from(format!("task-opt-monthday-{n}")),
                                        format!("{n} {}", tr("task.day_suffix")),
                                        s.month_day == n,
                                        move |c, cx| {
                                            upd_panel(c, cx, |p, _| {
                                                p.month_day = n;
                                                p.open_dd = DD_NONE;
                                            });
                                            cx.notify();
                                        },
                                        weak,
                                    )
                                })
                                .collect(),
                            250.,
                            260.,
                        )
                    },
                ),
                false,
            ));
        }
    }

    // ---- Agent 任务 / 脚本任务 区块（§1.3）
    let mut blocks: Vec<AnyElement> = Vec::new();
    blocks.push(section_title(tr("task.schedule")));
    blocks.push(form_row("task.unit", unit_dd, false));
    blocks.extend(time_rows);

    if s.kind_agent {
        blocks.push(section_title(tr("task.type_agent")));
        blocks.push(form_row(
            "task.desc",
            div().w(px(250.)).h(px(170.)).child(desc).into_any_element(),
            true,
        ));
        // 项目：触发器 ellipsis → 项目选择弹窗（复用；默认=当前项目）
        let proj_label = s
            .project
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| tr("task.project").to_string());
        let weak_proj = weak.clone();
        blocks.push(form_row(
            "task.project",
            div()
                .id("task-pick-project")
                .cursor_pointer()
                .hover(|h| h.bg(rgb(t2.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_proj.update(cx, |c, cx| {
                        c.open_project_picker_for_task(cx);
                    });
                })
                .child(dd_trigger(proj_label, 250.))
                .into_any_element(),
            false,
        ));
        // 工具权限（五档 + 说明，034 档位复用）
        let tool_rows: Vec<AnyElement> = tools_preset_rows(s.tools)
            .into_iter()
            .map(|r| {
                let weak = weak.clone();
                pill_row(
                    &weak,
                    {
                        let key = r.key;
                        move |c, cx| {
                            upd_panel(c, cx, |p, _| {
                                p.tools = pf_context::domain::tasks::TOOLS_PRESETS
                                    .iter()
                                    .find(|k| **k == key)
                                    .copied()
                                    .unwrap_or("default");
                                p.open_dd = DD_NONE;
                            });
                            cx.notify();
                        }
                    },
                    r,
                )
            })
            .collect();
        blocks.push(form_row(
            "task.tools",
            dropdown(
                "task-dd-tools",
                &dd,
                s.open_dd == DD_TOOLS,
                dd_toggle(DD_TOOLS, weak),
                dd_dismiss(weak),
                dd_trigger(s.tools.to_string(), 250.),
                move || dd_popup(tool_rows, 250., 220.),
            ),
            false,
        ));
        // 模型：继承新会话默认 / 覆盖（复用模型菜单体）
        let model_label = s
            .model
            .as_ref()
            .map(|(p, m)| format!("{p}/{m}"))
            .unwrap_or_else(|| tr("task.model_inherit").to_string());
        let (models, _total) = chat.picker_models_from(None, None);
        let weak_model = weak.clone();
        let mut model_items: Vec<AnyElement> = vec![dd_option(
            "task-opt-model-inherit",
            tr("task.model_inherit").to_string(),
            s.model.is_none(),
            move |c, cx| {
                upd_panel(c, cx, |p, _| {
                    p.model = None;
                    p.open_dd = DD_NONE;
                });
                cx.notify();
            },
            &weak_model,
        )];
        let on_pick: ModelPickAction = std::rc::Rc::new(move |c: &mut Chat, cx, p, id| {
            upd_panel(c, cx, |pd, _| {
                pd.model = Some((p.clone(), id.clone()));
                pd.open_dd = DD_NONE;
            });
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
                    s.model.as_ref().map(|(p, m)| format!("{p}/{m}")).as_deref(),
                    None,
                    |_| false,
                    Some(on_pick),
                    t,
                ))
                .into_any_element(),
        );
        blocks.push(form_row(
            "task.model",
            dropdown(
                "task-dd-model",
                &dd,
                s.open_dd == DD_MODEL,
                dd_toggle(DD_MODEL, weak),
                dd_dismiss(weak),
                dd_trigger(model_label, 250.),
                move || dd_popup(model_items, 320., 360.),
            ),
            false,
        ));
        // 思考强度（auto + 8 档，PillMenu::Thinking 同清单）
        let think_rows: Vec<AnyElement> = thinking_rows(s.thinking)
            .into_iter()
            .map(|r| {
                let weak = weak.clone();
                pill_row(
                    &weak,
                    {
                        let key = r.key;
                        move |c, cx| {
                            upd_panel(c, cx, |p, _| {
                                p.thinking = pf_context::domain::tasks::THINKING_LEVELS
                                    .iter()
                                    .find(|k| **k == key)
                                    .copied()
                                    .unwrap_or("auto");
                                p.open_dd = DD_NONE;
                            });
                            cx.notify();
                        }
                    },
                    r,
                )
            })
            .collect();
        blocks.push(form_row(
            "task.thinking",
            dropdown(
                "task-dd-thinking",
                &dd,
                s.open_dd == DD_THINKING,
                dd_toggle(DD_THINKING, weak),
                dd_dismiss(weak),
                dd_trigger(s.thinking.to_string(), 250.),
                move || dd_popup(think_rows, 250., 260.),
            ),
            false,
        ));
    } else {
        blocks.push(section_title(tr("task.type_script")));
        // 脚本文件：触发器（ellipsis/placeholder）+ 浏览… Secondary BTN_MD
        let script_label = s
            .script_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| tr("task.script_ph").to_string());
        blocks.push(form_row(
            "task.script",
            div()
                .flex()
                .gap(px(8.))
                .child(
                    div()
                        .w(px(250.))
                        .child(dd_trigger(script_label, 250.))
                        .when_some(s.script_err.clone(), |d, e| {
                            d.child(div().mt(px(6.)).child(error_note(&e)))
                        }),
                )
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
        ));
        blocks.push(
            div()
                .flex()
                .mb(px(8.))
                .ml(px(96.))
                .child(note(tr("task.script_note")))
                .into_any_element(),
        );
    }

    // ---- 提交钮行（右对齐 gap8 mt16；主钮 BTN_LG；不可提交 opacity 0.5）
    let can_submit = !s.name.trim().is_empty()
        && (if s.kind_agent {
            !s.desc.trim().is_empty() && s.project.is_some()
        } else {
            s.script_path.is_some()
        });
    let editing = s.edit_id.is_some();
    let mut actions = div()
        .flex()
        .justify_end()
        .items_center()
        .gap(px(8.))
        .mt(px(16.));
    if editing {
        actions = actions.child(config_button(
            "task-edit-del",
            weak,
            tr("task.delete"),
            Btn::Danger,
            true,
            false,
            |c, cx| c.task_confirm_delete_editing(cx),
        ));
    }
    actions = actions
        .child(config_button(
            "task-cancel",
            weak,
            tr("task.cancel"),
            Btn::Secondary,
            false,
            false,
            |c, cx| c.task_cancel_form(cx),
        ))
        .child(config_button(
            "task-submit",
            weak,
            if editing {
                tr("task.save")
            } else {
                tr("task.create")
            },
            Btn::Primary,
            false,
            !can_submit,
            |c, cx| c.task_save_form(cx),
        ));

    div()
        .id("task-form-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .pt(px(14.))
        .pb(px(16.))
        .px(px(20.))
        .flex()
        .flex_col()
        .child(form_row(
            "task.name",
            div().w(px(250.)).child(name).into_any_element(),
            false,
        ))
        .child(form_row("task.type", kind_row.into_any_element(), false))
        .children(blocks)
        .child(actions)
        .into_any_element()
}

// ---------------------------------------------------------------------------
// tab2 运行历史（§1.4）
// ---------------------------------------------------------------------------

/// 历史行渲染数据。
struct HistRow {
    def_id: String,
    title: String,
    outcome: Outcome,
    trigger: Trigger,
    started_at: i64,
    /// 耗时文案（结束行）/「跳过」；running = None（dot 槽转 spinner）。
    cost: Option<String>,
    session_path: Option<PathBuf>,
    is_script: bool,
    /// 结果点 hover 摘要。
    tip: String,
}

fn history_body(
    chat: &Chat,
    s: &TasksSnap,
    panel: &Entity<TaskPanel>,
    weak: &gpui::WeakEntity<Chat>,
    _t: &'static Theme,
    cx: &gpui::Context<Chat>,
) -> AnyElement {
    let t = theme();
    let dd = panel.read(cx).dd.clone();
    let title_of = |def_id: &str| -> String {
        chat.tasks
            .defs
            .iter()
            .find(|d| d.def.id == def_id)
            .map(|d| d.def.title.clone())
            .unwrap_or_else(|| def_id.to_string())
    };

    // 过滤行：任务过滤下拉 250×30（全部任务/单选某任务）
    let filter_label = s
        .run_filter
        .as_ref()
        .map(|id| title_of(id))
        .unwrap_or_else(|| tr("task.filter_all").to_string());
    let mut filter_items = vec![dd_option(
        "task-opt-filter-all",
        tr("task.filter_all").to_string(),
        s.run_filter.is_none(),
        move |c, cx| {
            upd_panel(c, cx, |p, _| {
                p.run_filter = None;
                p.open_dd = DD_NONE;
            });
            cx.notify();
        },
        weak,
    )];
    for DefRow { def, .. } in &chat.tasks.defs {
        let id = def.id.clone();
        let title = def.title.clone();
        let opt_id = SharedString::from(format!("task-opt-filter-{}", id));
        filter_items.push(dd_option(
            opt_id,
            title,
            s.run_filter.as_deref() == Some(id.as_str()),
            move |c, cx| {
                upd_panel(c, cx, |p, _| {
                    p.run_filter = Some(id.clone());
                    p.open_dd = DD_NONE;
                });
                cx.notify();
            },
            weak,
        ));
    }
    let filter_dd = dropdown(
        "task-dd-filter",
        &dd,
        s.open_dd == DD_RUN_FILTER,
        dd_toggle(DD_RUN_FILTER, weak),
        dd_dismiss(weak),
        dd_trigger(filter_label, 250.),
        move || dd_popup(filter_items, 250., 240.),
    );

    // 行数据
    let kind_of = |def_id: &str| -> bool {
        chat.tasks
            .defs
            .iter()
            .find(|d| d.def.id == def_id)
            .map(|d| matches!(d.spec.kind, TaskKind::Agent))
            .unwrap_or(true)
    };
    let rows: Vec<HistRow> = chat
        .tasks
        .runs
        .iter()
        .filter(|r| s.run_filter.as_deref().map_or(true, |id| r.def_id == id))
        .map(|r| {
            let (_, tip) = run_dot(r);
            let cost = match r.outcome {
                Outcome::Running => None,
                Outcome::Skipped => Some(tr("task.outcome_skipped").to_string()),
                _ => r
                    .finished_at
                    .map(|f| Some(fmt_duration_ms(f - r.started_at)))
                    .unwrap_or(None),
            };
            HistRow {
                def_id: r.def_id.clone(),
                title: title_of(&r.def_id),
                outcome: r.outcome,
                trigger: r.trigger,
                started_at: r.started_at,
                cost,
                session_path: r.session_path.clone().map(PathBuf::from),
                is_script: !kind_of(&r.def_id),
                tip,
            }
        })
        .collect();

    let weak_rows = weak.clone();
    let list = vlist(
        "task-history-vlist",
        rows.len(),
        32.,
        VListHeight::Fill,
        true,
        true,
        "task.empty_history",
        None,
        move |ix, _, _| hist_row_view(&rows[ix], &weak_rows, t),
    );

    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .pb(px(16.))
        .child(div().pb(px(10.)).child(filter_dd))
        .child(list)
        .into_any_element()
}

/// 历史行（h32）：结果点/spinner + 任务名 + 触发方式 + 开始时间右对齐 +
/// 耗时/结果右对齐。agent 行整行点击 = 打开会话；脚本行点击 = 620 输出查看。
fn hist_row_view(row: &HistRow, weak: &gpui::WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    let outcome_color = match row.outcome {
        Outcome::Running => None, // dot 槽转 spinner
        Outcome::Ok => Some(theme::NOTICE),
        Outcome::Failed => Some(t.danger),
        Outcome::Timeout => Some(theme::WARN),
        Outcome::Skipped => Some(t.text_faint),
    };
    let weak_click = weak.clone();
    let session = row.session_path.clone();
    let is_script = row.is_script;
    let (row_def, row_started) = (row.def_id.clone(), row.started_at);
    let mut el = div()
        .id(SharedString::from(format!(
            "task-hist-{}-{}",
            row.def_id, row.started_at
        )))
        .h(px(32.))
        .px(px(10.))
        .gap(px(8.))
        .rounded(px(radius::INLINE))
        .flex()
        .items_center()
        .hover(|h| h.bg(rgb(t.bg_hover)))
        .when(session.is_some() || is_script, |d| d.cursor_pointer())
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            if let Some(sp) = &session {
                let sp = sp.clone();
                let _ = weak_click.update(cx, |c, cx| {
                    c.open_session(sp, false, cx);
                });
            } else if is_script {
                let _ = weak_click.update(cx, |c, cx| {
                    // 从台账缓存找回整行（含 result_json）
                    if let Some(r) = c
                        .tasks
                        .runs
                        .iter()
                        .find(|r| r.def_id == row_def && r.started_at == row_started)
                        .cloned()
                    {
                        upd_panel(c, cx, |p, _| p.out_view = Some(r));
                        cx.notify();
                    }
                });
            }
        })
        // 结果点/spinner 7px 槽（running=spinner，§1.4 四态+黄）
        .child(
            div()
                .id(SharedString::from(format!(
                    "task-hist-dot-{}-{}",
                    row.def_id, row.started_at
                )))
                .w(px(15.))
                .flex()
                .justify_center()
                .when_some(outcome_color, |d, color| {
                    d.with_tip_text(row.tip.clone()).child(status_dot(color))
                })
                .when(outcome_color.is_none(), |d| {
                    d.with_tip_text(tr("task.running").to_string())
                        .child(spark_spinner(12., t.accent))
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(ui_size(12.))
                .text_color(rgb(t.text))
                .child(SharedString::from(row.title.clone())),
        )
        .child(
            div()
                .w(px(36.))
                .text_size(ui_size(10.))
                .text_color(rgb(t.text_faint))
                .child(SharedString::from(match row.trigger {
                    Trigger::Schedule => tr("task.trigger_schedule").to_string(),
                    Trigger::Manual => tr("task.trigger_manual").to_string(),
                })),
        )
        .child(
            div()
                .w(px(120.))
                .flex()
                .justify_end()
                .text_size(ui_size(10.))
                .text_color(rgb(t.text_faint))
                .child(
                    div()
                        .truncate()
                        .child(SharedString::from(fmt_msg_time(row.started_at))),
                ),
        )
        .child(
            div()
                .w(px(64.))
                .flex()
                .justify_end()
                .text_size(ui_size(10.))
                .text_color(rgb(t.text_faint))
                .child(
                    div()
                        .truncate()
                        .child(SharedString::from(row.cost.clone().unwrap_or_default())),
                ),
        );
    // 失败行加 ⚠（§1.4 失败 ⚠）
    if row.outcome == Outcome::Failed || row.outcome == Outcome::Timeout {
        el = el.child(icon("badge-info", 12., t.danger));
    }
    el.into_any_element()
}

/// 脚本输出查看（620 档，§1.4 脚本行点击）：exit code + 输出尾（mono 滚动）。
fn out_view_dialog(run: &TaskRun, weak: &gpui::WeakEntity<Chat>, _t: &'static Theme) -> AnyElement {
    let t = theme();
    let close = {
        let weak = weak.clone();
        move |_: &mut gpui::Window, cx: &mut gpui::App| {
            let _ = weak.update(cx, |c, cx| {
                upd_panel(c, cx, |p, _| p.out_view = None);
                cx.notify();
            });
        }
    };
    let weak_hdr = weak.clone();
    // 解析 result_json：{exit, timeout, tail} / {error} / {reason}
    let (exit_line, tail) = run
        .result_json
        .as_deref()
        .and_then(|j| serde_json::from_str::<serde_json::Value>(j).ok())
        .map(|v| {
            let exit = v.get("exit").and_then(|x| x.as_i64());
            let code_text = match (exit, v.get("timeout").and_then(|x| x.as_bool())) {
                (_, Some(true)) => tr("task.outcome_timeout").to_string(),
                (Some(c), _) => tf(tr("task.exit_code"), &[("code", c.to_string())]),
                _ => String::new(),
            };
            let tail = v
                .get("tail")
                .or_else(|| v.get("error"))
                .or_else(|| v.get("reason"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            (code_text, tail)
        })
        .unwrap_or_default();
    let header = div()
        .flex()
        .items_center()
        .justify_between()
        .h(px(36.))
        .px(px(16.))
        .bg(rgb(t.chrome))
        .border_b_1()
        .border_color(rgba(theme::border_alpha(t, 0x73)))
        .rounded_t(px(10.))
        .child(
            div()
                .text_size(ui_size(13.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text))
                .child(fmt_msg_time(run.started_at)),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .when(!exit_line.is_empty(), |d| {
                    d.child(
                        div()
                            .text_size(ui_size(11.))
                            .text_color(rgb(t.text_dim))
                            .child(SharedString::from(exit_line)),
                    )
                })
                .child(
                    div()
                        .id("task-out-close")
                        .size(px(30.))
                        .rounded(px(7.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(|h| h.bg(rgb(0xd8626a)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak_hdr.update(cx, |c, cx| {
                                upd_panel(c, cx, |p, _| p.out_view = None);
                                cx.notify();
                            });
                        })
                        .child(icon("x", 13., t.text_muted)),
                ),
        );
    overlay::layer(false, None, close)
        .flex()
        .items_center()
        .justify_center()
        .child(
            overlay::stop_click(
                div()
                    .w(px(620.))
                    .h(gpui::relative(0.6))
                    .bg(rgb(t.bg))
                    .border_1()
                    .border_color(rgb(t.border))
                    .rounded(px(10.))
                    .shadow_lg()
                    .overflow_hidden()
                    .flex()
                    .flex_col(),
            )
            .child(header)
            .child(
                div()
                    .id("task-out-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(14.))
                    .child(
                        div()
                            .text_size(ui_size(11.))
                            .text_color(rgb(t.text_dim))
                            .child(SharedString::from(tail)),
                    ),
            ),
        )
        .into_any_element()
}
