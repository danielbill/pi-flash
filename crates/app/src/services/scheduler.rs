//! 046-S2 定时调度服务（共有，不属 046 专属；046-0「定时调度服务」节）。
//!
//! 调度只懂时间不懂任务：唯一 timer 20s 一 tick，[`ScheduleSource`] 注册表 +
//! in-flight 集合；**无存储、无业务语义、无 dev-flag**——空注册表时泵空转
//! 成本 ≈ 0（20s 查一次）。后续闲时队列等周期类需求同泵注册，不另起 timer。
//!
//! - ticking / tick_requested 两变量合并去重（ZCode scheduler/index.ts 同款）：
//!   tick 进行中到来的唤醒不丢弃，本轮结束立即补跑；tick 间隙到来的唤醒等
//!   下一轮 20s timer 兜底；
//! - 启动时序：[`spawn`] 先跑全部 source 的 recover（source 自清上个实例
//!   残留）再开 timer；
//! - 作业结算后统一 cx.notify() 驱动 UI 刷新（状态驱动，不自建轮询）；
//! - misfire 宽限等通用判定以纯函数 helper 提供在消费侧（tasks/sched.rs）。
//!
//! log 约定 `[scheduler]`：source 注册/移除、recover 清理条数、tick 认领/
//! 派发条数（仅变化时记，防刷屏）。

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::Chat;

/// tick 周期：最小档周期是分钟级，20s 轮询足以按时命中且开销低（ZCode 同值）。
const TICK_SECS: u64 = 20;

/// 一个到期作业（source 自描述：`key` = in-flight 身份，tasks = def id）。
#[derive(Debug, Clone)]
pub(crate) struct DueJob {
    pub key: String,
    /// 本轮理论触发时刻（runId / 台账语义用；misfire 判定已在 claim 内完成）。
    #[allow(dead_code)]
    pub scheduled_at: i64,
    /// 手动派发标记（run_now 走 [`dispatch_manual`] 置 true）：落台账 trigger
    /// 与结算语义（manual 不重排周期）。2026-11-22 修：此前 execute 不分来源
    /// 恒记 Schedule——手动运行被误标 schedule 且结算污染 next_run。
    pub manual: bool,
}

/// 作业执行结果（oneshot 送回；source 在 settle 里解释成自己的账）。
#[derive(Debug, Clone)]
pub(crate) struct JobOutcome {
    /// 终态细分键（tasks 解释为 ok / failed / timeout）。
    pub kind: &'static str,
    pub success: bool,
    /// 结果载荷（tasks → result_json；JSON 文本，None = 无）。
    pub detail: Option<String>,
}

impl JobOutcome {
    /// 046-S3 执行链回报成功用。
    #[allow(dead_code)]
    pub fn ok(detail: Option<String>) -> Self {
        Self {
            kind: "ok",
            success: true,
            detail,
        }
    }

    pub fn failed(kind: &'static str, detail: Option<String>) -> Self {
        Self {
            kind,
            success: false,
            detail,
        }
    }

    /// 派发失败（execute 返回 Err）的统一载荷新闻。
    pub(crate) fn dispatch_failed(err: &str) -> Self {
        Self::failed(
            "failed",
            Some(format!(
                "{{\"error\":\"派发失败: {}\"}}",
                err.replace('"', "'")
            )),
        )
    }

    /// oneshot 送回通道断开（执行方 drop 未回报）——按失败结算。
    fn lost() -> Self {
        Self::failed("failed", Some("{\"error\":\"执行链 oneshot 丢失\"}".into()))
    }
}

/// 派发回报通道（execute spawn 的执行体持有 Sender，泵 spawn 的结算协程
/// 持有 Receiver）。
pub(crate) type JobRx = futures::channel::oneshot::Receiver<JobOutcome>;

/// 调度源契约（实现方经 [`register`] 注册；main 线程调用）。
pub(crate) trait ScheduleSource: Send + Sync {
    /// source 标识（log 用）。
    fn id(&self) -> &'static str;
    /// 启动时自清上个实例残留 → 清理条数（log 用）。
    fn recover(&self, now: i64) -> Result<usize, String>;
    /// 认领到期作业。`in_flight` = 已派发未结算 key（防重复派发）；
    /// skip 类作业（in-flight 撞车 / misfire 超窗）由 source 自己落账后**不返回**。
    fn claim(&self, now: i64, in_flight: &HashSet<String>) -> Vec<DueJob>;
    /// 派发（spawn 执行体，oneshot 回报）。Err = 派发失败——v1 不自动重试，
    /// 直接结算 failed，下轮按周期继续。
    fn execute(
        &self,
        job: &DueJob,
        chat: &mut Chat,
        cx: &mut gpui::Context<Chat>,
    ) -> Result<JobRx, String>;
    /// 结算：写结果账（调度指针已由 claim 认领时消费前推，结算不碰）。
    fn settle(&self, job: DueJob, outcome: JobOutcome, now: i64);
}

static SOURCES: Mutex<Vec<Arc<dyn ScheduleSource>>> = Mutex::new(Vec::new());
/// 已派发未结算的作业 key（单飞集合；claim 时传给 source 防重复派发）。
/// 内部 Vec（`HashSet::new` 非 const 不能进 static；条目 ≤ 并发作业数）。
static IN_FLIGHT: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// tick 进行中（重入保护 + 合并去重的一半）。
static TICKING: AtomicBool = AtomicBool::new(false);
/// tick 进行中到来的唤醒（本轮结束立即补跑）。
static TICK_REQUESTED: AtomicBool = AtomicBool::new(false);
/// 仅变化时记 log：上次 (认领, 派发) 条数。
static LAST_TICK_LOG: Mutex<(usize, usize)> = Mutex::new((0, 0));

/// Unix ms（泵与业务侧共用的时间源；测试注入由纯函数参数承担）。
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// 注册调度源（幂等不查重——启动期一次调用；S4 dev-flag 门住注册点）。
pub(crate) fn register(source: Arc<dyn ScheduleSource>) {
    let id = source.id();
    SOURCES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(source);
    eprintln!("[scheduler] source 注册: {id}");
}

/// in-flight 快照（任务状态面 running 读点；已排序）。
pub(crate) fn in_flight_keys() -> Vec<String> {
    let mut v: Vec<String> = IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner()).clone();
    v.sort();
    v
}

/// in-flight 快照（claim 的防重复派发集合）。
fn in_flight_set() -> HashSet<String> {
    IN_FLIGHT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .cloned()
        .collect()
}

fn add_in_flight(key: &str) {
    let mut v = IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner());
    if !v.iter().any(|k| k == key) {
        v.push(key.to_string());
    }
}

fn remove_in_flight(key: &str) {
    IN_FLIGHT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|k| k != key);
}

/// 手动清除在途标记（046-S4 删除中止：run 已随任务删除，结算不会再发生）。
pub(crate) fn clear_in_flight(key: &str) {
    remove_in_flight(key);
}

/// 作业在途查询（run_now 手动派发的运行中守卫，§1.5 ⋯菜单置灰双保险）。
#[allow(dead_code)] // 046-S4 菜单/删除接线
pub(crate) fn is_in_flight(key: &str) -> bool {
    IN_FLIGHT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .any(|k| k == key)
}

/// 按 id 取 source（run_now 手动派发复用 tasks source 的 execute/settle 管线）。
#[allow(dead_code)] // 046-S4 接线
pub(crate) fn source_by_id(id: &str) -> Option<Arc<dyn ScheduleSource>> {
    sources().into_iter().find(|s| s.id() == id)
}

/// 外部唤醒（作业结算 / S4 任务增删改后即时排程）。tick 间隙到来的唤醒等
/// 下一轮 20s timer 兜底；tick 进行中到来的不丢弃（见 [`tick`]）。
#[allow(dead_code)] // 046-S4 任务 CRUD 接线
pub(crate) fn request_tick() {
    TICK_REQUESTED.store(true, Ordering::SeqCst);
}

/// 启动泵（startup/pumps.rs 拉起）：recover 编排 → 首轮 tick 即刻 →
/// 20s timer 循环。
pub(crate) fn spawn(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| {
        // 启动时序：先跑全部 source 的 recover，再开 timer
        let ok = this.update(cx, |_, _| recover_all()).is_ok();
        if !ok {
            return; // 实体已亡（窗口秒关）
        }
        // 首轮 tick 不等 timer 即刻跑（2026-11-22 修：原来任务状态面要等
        // 满 20s 第一 tick 才有数据——任务页开屏「全部任务 0」十几秒的根
        // 因）。tick 本身 µs 级（缓存 ≤20 行），claim/派发语义与首轮 timer
        // 完全一致，无额外成本。
        if !tick_round(&this, cx).await {
            return;
        }
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(TICK_SECS))
                .await;
            if !tick_round(&this, cx).await {
                return;
            }
        }
    })
    .detach();
}

/// 一轮合并去重 tick（tick 进行中到来的唤醒不丢弃，本轮结束立即补跑）。
/// 返回 false = 实体已亡。
async fn tick_round(this: &gpui::WeakEntity<Chat>, cx: &mut gpui::AsyncApp) -> bool {
    loop {
        let mut again = false;
        let ok = this
            .update(cx, |chat, cx| {
                again = tick(chat, cx);
            })
            .is_ok();
        if !ok {
            return false;
        }
        if !again {
            return true;
        }
    }
}

/// 启动 recover：逐 source 自清上个实例残留。
fn recover_all() {
    for source in sources() {
        match source.recover(now_ms()) {
            Ok(n) => eprintln!("[scheduler] recover {} 清理 {n} 条", source.id()),
            Err(e) => eprintln!("[scheduler] recover {} 失败: {e}", source.id()),
        }
    }
}

/// 一轮 tick：逐 source 认领 → 派发（spawn 结算协程）→ 统一 notify。
/// 返回 true = tick 进行中又来了唤醒（调用方立即补跑）。
fn tick(chat: &mut Chat, cx: &mut gpui::Context<Chat>) -> bool {
    if TICKING.swap(true, Ordering::SeqCst) {
        return false; // 重入保护（主线程串行下理论不可达）
    }
    TICK_REQUESTED.store(false, Ordering::SeqCst);
    let now = now_ms();
    let mut claimed = 0usize;
    let mut dispatched = 0usize;
    for source in sources() {
        for job in source.claim(now, &in_flight_set()) {
            claimed += 1;
            match source.execute(&job, chat, cx) {
                Ok(rx) => {
                    dispatched += 1;
                    add_in_flight(&job.key);
                    spawn_settle(Arc::clone(&source), job, rx, cx);
                }
                Err(e) => {
                    // 派发失败：v1 不自动重试，直接结算 failed，下轮按周期继续
                    // （ZCode automationRepo 退避重试列为 v2）
                    eprintln!("[scheduler] 派发失败 {} task={}: {e}", source.id(), job.key);
                    source.settle(job, JobOutcome::dispatch_failed(&e), now_ms());
                }
            }
        }
    }
    // tick 级状态面刷新 + 重绘（claim 里可能落了 skipped run；缓存 ≤20 行 µs 级）
    chat.tasks_refresh();
    cx.notify();
    log_tick_counts(claimed, dispatched);
    let more = TICK_REQUESTED.swap(false, Ordering::SeqCst);
    TICKING.store(false, Ordering::SeqCst);
    more
}

/// 结算协程（tick 定时派发与 run_now 手动派发共用）：等 oneshot → settle →
/// 出 in-flight → 刷任务状态面 + 统一 cx.notify()（状态驱动 UI，§1.5）。
pub(crate) fn spawn_settle(
    source: Arc<dyn ScheduleSource>,
    job: DueJob,
    rx: JobRx,
    cx: &mut gpui::Context<Chat>,
) {
    cx.spawn(async move |this, cx| {
        let outcome = rx.await.unwrap_or_else(|_| JobOutcome::lost());
        source.settle(job.clone(), outcome, now_ms());
        remove_in_flight(&job.key);
        let _ = this.update(cx, |chat, cx| {
            chat.tasks_refresh();
            cx.notify();
        });
    })
    .detach();
}

/// 手动派发（046-S3 run_now / §1.5「立即运行」）：进 in-flight → execute →
/// 结算协程；派发失败直接结算 failed（manual 不动周期，settle_job 保证）。
/// 运行中再触发被守卫挡下（静默返回——菜单已置灰，这里是双保险）。
pub(crate) fn dispatch_manual(
    source: Arc<dyn ScheduleSource>,
    job: DueJob,
    chat: &mut Chat,
    cx: &mut gpui::Context<Chat>,
) {
    if is_in_flight(&job.key) {
        return;
    }
    match source.execute(&job, chat, cx) {
        Ok(rx) => {
            add_in_flight(&job.key);
            spawn_settle(source, job, rx, cx);
        }
        Err(e) => {
            eprintln!("[scheduler] 手动派发失败 {}: {e}", job.key);
            source.settle(job, JobOutcome::dispatch_failed(&e), now_ms());
            chat.tasks_refresh();
            cx.notify();
        }
    }
}

fn sources() -> Vec<Arc<dyn ScheduleSource>> {
    SOURCES.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// tick 条数 log（仅变化时记，防刷屏——046-0 log 约定）。
fn log_tick_counts(claimed: usize, dispatched: usize) {
    let mut last = LAST_TICK_LOG.lock().unwrap_or_else(|e| e.into_inner());
    if *last != (claimed, dispatched) {
        eprintln!("[scheduler] tick 认领 {claimed} 派发 {dispatched}");
        *last = (claimed, dispatched);
    }
}
