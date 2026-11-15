//! 046 自动任务业务模块（state + store + sched 三件；run 执行链 046-S3）。
//!
//! - [`sched`]：next_run 五档计算 / claim / settle / recover 业务核心；
//! - [`store`]：TaskStore 封装（hub → context.db）+ CRUD 面 + 状态面缓存；
//! - ScheduleSource 实现（[`TasksSource`]）注册进共有调度泵
//!  （services/scheduler.rs），启动时序见 startup/pumps.rs。
//!
//! log 约定 `[tasks]`：创建/编辑/启停/删除、skip、结算、prune、recover；
//! 错误（会话创建失败/脚本起进程失败/DB 写失败）error 级带 task id（046-0）。

pub(crate) mod run;
pub(crate) mod sched;
pub(crate) mod store;
pub(crate) mod view;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};

use gpui::{AppContext as _, Context, Entity};

use pf_context::domain::tasks::{
    RUN_KEEP_LIMIT, ScheduleUnit, TaskDef, TaskKind, TaskRun, TaskSchedule, TaskSpec, Trigger,
};

use crate::Chat;
use crate::services::format::now_ms;
use crate::services::scheduler::{self, DueJob, JobOutcome};
use crate::ui::dropdown::DropdownState;
use crate::ui::text_input::TextInput;

/// prune 顺带跑周期（046-0：泵每小时 tick 计数器到点执行；启动 recover 已跑首轮）。
const PRUNE_EVERY_MS: i64 = 3_600_000;

fn tasks_log(msg: &str) {
    eprintln!("[tasks] {msg}");
}

fn tasks_err(msg: &str) {
    eprintln!("[tasks] {msg}");
}

// ---------------------------------------------------------------------------
// 状态面（UI 唯一读点；渲染只读缓存，不自建轮询——§1.5 状态驱动）
// ---------------------------------------------------------------------------

/// 一行任务状态面：定义 + 解析好的 spec（UI 不再重复反序列化；S4 大卡消费）。
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct DefRow {
    pub def: TaskDef,
    pub spec: TaskSpec,
}

/// 046 任务状态面（Chat 直属字段）。
#[derive(Default)]
pub(crate) struct TasksState {
    /// 任务定义缓存（refresh 点 = CRUD / 泵 tick / 结算回执）。
    pub defs: Vec<DefRow>,
    /// 运行中 def id（= 调度泵 in-flight 快照；spinner + ⋯菜单置灰读点）。
    pub running: Vec<String>,
    /// 在途派发上下文（run.rs；按 runtime key 索引，TurnSettled/FileBound
    /// 钩子消费——把会话事件翻译回 run 台账）。
    pub pending: run::PendingMap,
    /// 运行台账缓存（日志 tab 数据源；started_at 倒序全量 ≤20×50 行，
    /// refresh 点与 defs 同）。UI 渲染只读这份，不自查库。
    pub runs: Vec<TaskRun>,
    /// 任务管理页 UI 状态（2026-11-15 拍板：内容区页签 + 页头横向双 tab）
    pub page: TaskPage,
}

/// 任务管理页 UI 状态（随页签存活；文本输入实体懒创建——aux.market 同款）。
#[derive(Default)]
pub(crate) struct TaskPage {
    /// 页头横向 tab：0 = 任务、1 = 日志。
    pub tab: u8,
    /// 左列表选中 def（None = 新建空态，表单清空）。
    pub sel: Option<String>,
    // ---- 表单字段（选中回填 / 新建清空共用一套）
    pub name: Option<Entity<TextInput>>,
    pub desc: Option<Entity<TextInput>>,
    /// false = 脚本、true = Agent（HTML 原稿新建默认 Agent）。
    pub kind_agent: bool,
    pub unit_ix: usize,
    pub interval_ix: usize,
    pub hour: u32,
    pub minute: u32,
    /// 周一..周日（1–7，周一=1；weekly 单选落单元素）。
    pub weekdays: [bool; 7],
    pub month_day: u32,
    pub project: Option<PathBuf>,
    pub tools: &'static str,
    pub model: Option<(String, String)>,
    pub thinking: &'static str,
    pub script_path: Option<PathBuf>,
    pub script_err: Option<String>,
    /// 当前展开的下拉/拾取器（互斥；0 = 全关）。
    pub open_dd: u8,
    pub dd: Option<Entity<DropdownState>>,
    /// 移除确认（overlay::confirm 槽：(def_id, title)）。
    pub confirm_del: Option<(String, String)>,
}

impl Chat {
    /// 046 任务状态面刷新（写点 = CRUD / 泵 tick / 结算回执）。
    pub(crate) fn tasks_refresh(&mut self) {
        self.tasks.defs = store::list_cached();
        let mut running = scheduler::in_flight_keys();
        running.sort();
        self.tasks.running = running;
        self.tasks.runs = store::list_runs_cached();
    }
}

// ---------------------------------------------------------------------------
// ScheduleSource 实现（注册进共有调度泵）
// ---------------------------------------------------------------------------

/// 任务调度源：claim/settle/recover 走 [`sched`] 业务核心；execute 是 046-S3
/// 执行链的交棒点。
#[derive(Default)]
pub(crate) struct TasksSource {
    /// 上次 prune 时刻（ms；每小时计数器）。
    last_prune: AtomicI64,
}

impl scheduler::ScheduleSource for TasksSource {
    fn id(&self) -> &'static str {
        "tasks"
    }

    /// 启动自清：running 残留置 failed + 首轮 prune（046-0 recover）。
    fn recover(&self, now: i64) -> Result<usize, String> {
        let n = sched::recover(&store::open_store()?, now)?;
        self.last_prune.store(now, Ordering::Relaxed);
        Ok(n)
    }

    fn claim(&self, now: i64, in_flight: &HashSet<String>) -> Vec<DueJob> {
        // 每小时 prune 顺带跑（046-0：API 必须真挂泵，不留无人调用的删除口）
        if now - self.last_prune.load(Ordering::Relaxed) >= PRUNE_EVERY_MS {
            self.last_prune.store(now, Ordering::Relaxed);
            match store::open_store() {
                Ok(s) => match s.prune_runs(RUN_KEEP_LIMIT) {
                    Ok(0) => {}
                    Ok(n) => tasks_log(&format!("每小时 prune: 删除 {n} 条")),
                    Err(e) => tasks_err(&format!("prune 失败: {e}")),
                },
                Err(e) => tasks_err(&format!("prune 开库失败: {e}")),
            }
        }
        match store::open_store() {
            Ok(s) => sched::claim_due(&s, now, in_flight),
            Err(e) => {
                tasks_err(&format!("claim 开库失败: {e}"));
                Vec::new()
            }
        }
    }

    fn execute(
        &self,
        job: &DueJob,
        chat: &mut Chat,
        cx: &mut gpui::Context<Chat>,
    ) -> Result<scheduler::JobRx, String> {
        // 046-S3 执行链：agent 会话 / 脚本子进程（run.rs dispatch）。run 行
        // 落 started 于 dispatch 内；Err = 起执行体失败（run 行已由 dispatch
        // 结算为 failed，这里把 Err 交回统一失败路径重排周期）。
        run::dispatch(chat, &job.key, Trigger::Schedule, cx)
    }

    fn settle(&self, job: DueJob, outcome: JobOutcome, now: i64) {
        match store::open_store() {
            Ok(s) => {
                if let Err(e) = sched::settle_job(&s, &job.key, &outcome, now) {
                    tasks_err(&format!("结算失败: task={} {e}", job.key));
                }
            }
            Err(e) => tasks_err(&format!("结算开库失败: task={} {e}", job.key)),
        }
    }
}

/// 启动注册（startup/pumps.rs 调用，先于 scheduler::spawn 的 recover）。
/// S4 起 dev-flag `auto_tasks` 门住本调用（source 注册三处同源之一）。
pub(crate) fn register() {
    scheduler::register(std::sync::Arc::new(TasksSource::default()));
}

// ---------------------------------------------------------------------------
// 页面动作（2026-11-15 拍板：内容区页签；HTML 原稿 selectTask/goNew 语义）
// ---------------------------------------------------------------------------

/// 页头横向 tab 槽位。
pub(crate) const PAGE_TAB_TASKS: u8 = 0;
pub(crate) const PAGE_TAB_LOG: u8 = 1;

impl Chat {
    /// psp「任务」钮 / tasks.open op：打开内容区任务管理页签（复用既有
    /// PanelTab；open_pi_native_market 同款挂法）。
    pub(crate) fn open_tasks_page(&mut self, cx: &mut Context<Self>) {
        match self
            .ws
            .panel_tabs
            .iter()
            .position(|t| matches!(t, crate::PanelTab::Tasks))
        {
            Some(ix) => self.activate_panel_tab(ix, cx),
            None => {
                self.ws.panel_tabs.push(crate::PanelTab::Tasks);
                let ix = self.ws.panel_tabs.len() - 1;
                self.activate_panel_tab(ix, cx);
            }
        }
        self.set_content_view(crate::ContentView::Tasks);
        self.tasks_ensure_inputs(cx);
        cx.notify();
    }

    /// 任务页表单输入持焦自检（focus.rs 白名单；v60/v70 教训的结构化版）。
    pub(crate) fn tasks_page_input_focused(
        &self,
        window: &gpui::Window,
        cx: &gpui::Context<Self>,
    ) -> bool {
        [self.tasks.page.name.as_ref(), self.tasks.page.desc.as_ref()]
            .into_iter()
            .flatten()
            .any(|i| i.read(cx).focus_handle_in(cx).is_focused(window))
    }

    /// 表单文本输入 / 下拉守卫懒创建（aux.market 同款）。
    pub(crate) fn tasks_ensure_inputs(&mut self, cx: &mut Context<Self>) {
        if self.tasks.page.name.is_none() {
            // HTML name-input：14px 大字（h34 容器内）
            self.tasks.page.name = Some(cx.new(|cx| TextInput::new(cx).text_size(14.)));
        }
        if self.tasks.page.desc.is_none() {
            self.tasks.page.desc = Some(cx.new(|cx| TextInput::new(cx).multiline()));
        }
        if self.tasks.page.dd.is_none() {
            self.tasks.page.dd = Some(cx.new(|_| DropdownState::new()));
        }
    }

    /// 列表行点击 = 详情回填右侧表单（HTML selectTask；.t.link 点击不经过这）。
    pub(crate) fn task_select(&mut self, def_id: &str, cx: &mut Context<Self>) {
        let Some(row) = self.tasks.defs.iter().find(|d| d.def.id == def_id).cloned() else {
            return;
        };
        self.tasks_ensure_inputs(cx);
        let proj = row
            .spec
            .agent
            .as_ref()
            .map(|a| PathBuf::from(&a.project))
            .or_else(|| self.ws.cwd.clone());
        let units = [
            ScheduleUnit::Minute,
            ScheduleUnit::Hourly,
            ScheduleUnit::Daily,
            ScheduleUnit::Weekly,
            ScheduleUnit::Monthly,
        ];
        let sch = &row.spec.schedule;
        let (agent, script) = match &row.spec.kind {
            TaskKind::Agent => (
                Some(row.spec.agent.clone().unwrap_or_else(dummy_agent)),
                None,
            ),
            TaskKind::Script => (None, row.spec.script.clone()),
        };
        let desc = agent.as_ref().map(|a| a.prompt.clone()).unwrap_or_default();
        let script_path = script.as_ref().map(|s| PathBuf::from(&s.path));
        {
            let page = &mut self.tasks.page;
            page.sel = Some(row.def.id.clone());
            page.confirm_del = None;
            page.open_dd = 0;
            page.kind_agent = matches!(row.spec.kind, TaskKind::Agent);
            page.unit_ix = units.iter().position(|u| *u == sch.unit).unwrap_or(2);
            page.interval_ix = INTERVALS
                .iter()
                .position(|n| *n == sch.interval.max(1))
                .unwrap_or(0);
            page.hour = sch.hour as u32;
            page.minute = sch.minute as u32;
            let mut weekdays = [false; 7];
            for w in &sch.weekdays {
                if (1..=7).contains(w) {
                    weekdays[*w as usize - 1] = true;
                }
            }
            page.weekdays = weekdays;
            page.month_day = sch.month_day.max(1) as u32;
            page.project = proj;
            page.script_path = script_path;
            page.script_err = None;
            if let Some(a) = &agent {
                page.tools = pf_context::domain::tasks::TOOLS_PRESETS
                    .iter()
                    .find(|k| **k == a.tools)
                    .copied()
                    .unwrap_or("default");
                page.model = a.model.clone();
                page.thinking = pf_context::domain::tasks::THINKING_LEVELS
                    .iter()
                    .find(|k| **k == a.thinking)
                    .copied()
                    .unwrap_or("auto");
            }
        }
        if let Some(i) = &self.tasks.page.name {
            i.update(cx, |inp, icx| inp.set_value(row.def.title.clone(), icx));
        }
        if let Some(i) = &self.tasks.page.desc {
            i.update(cx, |inp, icx| inp.set_value(desc, icx));
        }
        cx.notify();
    }

    /// 页头【＋新建】= 表单清空（HTML goNew；默认 Agent / 每天 09:00）。
    pub(crate) fn task_new_form(&mut self, cx: &mut Context<Self>) {
        self.tasks_ensure_inputs(cx);
        let proj = self.ws.cwd.clone();
        let page = &mut self.tasks.page;
        page.sel = None;
        page.confirm_del = None;
        page.open_dd = 0;
        page.kind_agent = true;
        page.unit_ix = 2;
        page.interval_ix = 0;
        page.hour = 9;
        page.minute = 0;
        page.weekdays = [false; 7];
        page.month_day = 1;
        page.project = proj;
        page.tools = "full";
        page.model = None;
        page.thinking = "auto";
        page.script_path = None;
        page.script_err = None;
        if let Some(i) = &page.name {
            i.update(cx, |inp, icx| inp.set_value(String::new(), icx));
        }
        if let Some(i) = &page.desc {
            i.update(cx, |inp, icx| inp.set_value(String::new(), icx));
        }
        cx.notify();
    }

    /// 表单【保存】（新建 = 创建 / 选中 = 整行更新；改周期重锚在 save_edit 判定）。
    pub(crate) fn task_save_form(&mut self, cx: &mut Context<Self>) {
        self.tasks_ensure_inputs(cx);
        let name = self
            .tasks
            .page
            .name
            .as_ref()
            .map(|i| i.read(cx).value().to_string())
            .unwrap_or_default();
        let desc = self
            .tasks
            .page
            .desc
            .as_ref()
            .map(|i| i.read(cx).value().to_string())
            .unwrap_or_default();
        let page = &self.tasks.page;
        let snap = FormSnap {
            name,
            desc,
            kind_agent: page.kind_agent,
            unit_ix: page.unit_ix,
            interval_ix: page.interval_ix,
            hour: page.hour,
            minute: page.minute,
            weekdays: page.weekdays,
            month_day: page.month_day,
            project: page.project.clone(),
            tools: page.tools,
            model: page.model.clone(),
            thinking: page.thinking,
            script_path: page.script_path.clone(),
        };
        let sel = page.sel.clone();
        let Some(spec) = build_spec(&snap) else {
            return;
        };
        let now = now_ms();
        let result = match &sel {
            Some(id) => store::save_edit(id, &snap.name, &spec, true, now).map(|_| id.clone()),
            None => store::create_task(&snap.name, &spec, now),
        };
        match result {
            Ok(id) => {
                self.tasks.page.sel = Some(id);
                self.tasks_refresh();
                scheduler::request_tick();
            }
            Err(e) => {
                let msg = if e.contains("上限") {
                    crate::i18n::tr("task.limit_reached").to_string()
                } else {
                    e
                };
                self.dlg.status_toast = Some((msg, std::time::Instant::now()));
            }
        }
        cx.notify();
    }

    /// 表单 ghost【停用/启用】。
    pub(crate) fn task_toggle_from_page(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.tasks.page.sel.clone() else {
            return;
        };
        let enabled = self
            .tasks
            .defs
            .iter()
            .find(|d| d.def.id == id)
            .map(|d| !d.def.enabled)
            .unwrap_or(true);
        if let Err(e) = store::set_enabled_task(&id, enabled, now_ms()) {
            tasks_err(&format!("启停失败: {e}"));
        }
        self.tasks_refresh();
        cx.notify();
    }

    /// ghost【立即运行】（运行中 in-flight 守卫挡下，§1.5）。
    pub(crate) fn task_run_now(&mut self, def_id: &str, cx: &mut Context<Self>) {
        let Some(src) = scheduler::source_by_id("tasks") else {
            return;
        };
        let job = scheduler::DueJob {
            key: def_id.to_string(),
            scheduled_at: scheduler::now_ms(),
        };
        scheduler::dispatch_manual(src, job, self, cx);
    }

    /// 移除确认通过：运行中先中止再删（同事务连带删 runs）。
    pub(crate) fn task_delete_confirmed(&mut self, def_id: &str, cx: &mut Context<Self>) {
        if let Some(key) = run::pending_key_for(self, def_id) {
            if let Some(ctx) = self.tasks.pending.remove(&key) {
                if let Some(flag) = &ctx.kill {
                    flag.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                if let Some(rt) = &ctx.rt {
                    rt.update(cx, |r, rcx| r.interrupt_from_composer(rcx));
                }
            }
            scheduler::clear_in_flight(def_id);
        }
        if let Err(e) = store::delete_task(def_id) {
            tasks_err(&format!("删除失败: {e}"));
        }
        self.tasks.page.sel = None;
        self.tasks.page.confirm_del = None;
        self.tasks_refresh();
        cx.notify();
    }

    /// 项目选择（task_form 路由）：选中项目回填表单，不切工作区。
    pub(crate) fn set_task_form_project_inner(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.tasks.page.project = Some(path);
        cx.notify();
    }

    /// 表单「浏览…」：系统文件选择器 → 校验扩展名 → 回填。
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
                    c.tasks.page.script_err =
                        if crate::services::script_exec::exec_kind(&file).is_some() {
                            None
                        } else {
                            Some(crate::i18n::tr("task.script_err_ext").to_string())
                        };
                    c.tasks.page.script_path = Some(file);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 任务表单的目录选择器（gpui prompt；选完只回填表单）。
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
}

/// 表单快照（保存装配用）。
struct FormSnap {
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
}

/// 间隔档（§1.3 下拉清单；HTML cmin 菜单同源）。
pub(crate) const INTERVALS: [u32; 10] = [1, 2, 3, 5, 10, 15, 20, 30, 45, 60];

fn dummy_agent() -> pf_context::domain::tasks::AgentSpec {
    pf_context::domain::tasks::AgentSpec {
        prompt: String::new(),
        project: String::new(),
        tools: "default".into(),
        model: None,
        thinking: "auto".into(),
    }
}

/// 表单快照 → TaskSpec（非法组合不会出现——UI 互斥约束）。
fn build_spec(s: &FormSnap) -> Option<TaskSpec> {
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

/// 项目选择（task_form 路由）转接点（project_picker.rs 回调调）。
pub(crate) fn set_task_form_project(c: &mut Chat, path: PathBuf, cx: &mut gpui::Context<Chat>) {
    c.set_task_form_project_inner(path, cx);
}

/// 任务表单的目录选择（键盘 Enter / 【打开文件夹】行路由）。
pub(crate) fn pick_task_project_folder(c: &mut Chat, cx: &mut gpui::Context<Chat>) {
    c.pick_project_folder_for_task(cx);
}
