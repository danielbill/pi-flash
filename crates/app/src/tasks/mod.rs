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

use std::collections::HashSet;
use std::sync::atomic::{AtomicI64, Ordering};

use pf_context::domain::tasks::{RUN_KEEP_LIMIT, TaskDef, TaskRun, TaskSpec, Trigger};

use crate::Chat;
use crate::services::scheduler::{self, DueJob, JobOutcome};

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
#[derive(Debug, Default)]
pub(crate) struct TasksState {
    /// 任务定义缓存（refresh 点 = CRUD / 泵 tick / 结算回执）。
    pub defs: Vec<DefRow>,
    /// 运行中 def id（= 调度泵 in-flight 快照；spinner + ⋯菜单置灰读点）。
    pub running: Vec<String>,
    /// 在途派发上下文（run.rs；按 runtime key 索引，TurnSettled/FileBound
    /// 钩子消费——把会话事件翻译回 run 台账）。
    pub pending: run::PendingMap,
    /// 运行台账缓存（历史 tab 数据源；started_at 倒序全量 ≤20×50 行，
    /// refresh 点与 defs 同）。UI 渲染只读这份，不自查库。
    pub runs: Vec<TaskRun>,
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
