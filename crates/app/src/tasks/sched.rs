//! 046-S2 任务业务核心：next_run 五档计算 + claim / settle / recover。
//!
//! 全部以 `&TaskStore` 为界、gpui 无关（单测直跑临时库）；调度泵
//!（services/scheduler.rs）只做时间编排，业务语义都在这里：
//!
//! - next_run：chrono Local 本地日历直算，移植 ZCode
//!   computeScheduleRuleNextRunAt（automationCron.ts:133-220）——minute 档从
//!   anchor 推进 k·interval **不对齐墙钟刻度**（11:46 建「每 10 分」不在
//!   11:50 抢跑）；weekly 锚周取周一逐周扫 weekdays；monthly 过滤溢出日
//!  （1/31 → 2 月无效）；hourly 只对齐 minute。恒返回**严格晚于 from** 的
//!   候选（认领前推以当下为基准 → 恒给未来值，跑超一个周期也不产生追债）。
//! - claim：due = enabled ∧ next_run_at ≤ now → **认领即消费**（指针当场
//!   前推到严格未来值；ZCode markDispatched 是「派发结算」而非运行完成
//!   结算，automationRepo.ts:998-1050）→ in-flight 撞车 / misfire 超窗各
//!   记**一条** skipped run（at = 计划触发时刻）+ 指针前推，一触发至多一
//!   行账 → 交调度泵派发。
//! - settle：只结算 running 台账行，**不碰调度指针**（2026-11-23 修：原
//!   settle 重算使运行期每 tick 重复判定到期，每 tick 落一条 skip 刷屏
//!   任务日志）。「manual 不污染 cron 节奏」由 manual 派发不走 claim 保证
//!  （ZCode automationRepo.ts:666-751）。
//! - recover：running 残留置 failed（app_exit 中断）+ 首轮 prune（046-0）。

use std::collections::HashSet;

use chrono::{Datelike, Days, Local, LocalResult, NaiveDate, TimeZone, Timelike};

use pf_context::domain::tasks::TaskStore;
use pf_context::domain::tasks::{Outcome, RUN_KEEP_LIMIT, ScheduleUnit, TaskSpec, Trigger};

use crate::services::scheduler::{DueJob, JobOutcome};

use super::{tasks_err, tasks_log};

/// misfire 宽限：next_run_at 早于 now 超过该值 → 视为「关机/休眠/退出期间
/// 错过的窗口」→ 记 skipped 不补跑。取值明显大于一次正常轮询延迟（ZCode
/// MISFIRE_GRACE_MS 同值）。
pub(crate) const MISFIRE_GRACE_MS: i64 = 5 * 60_000;

/// daily/weekly 搜索边界（100 年；ZCode 36_600/5_220 同值——无未来候选 =
/// None，任务进入休眠）。
const DAILY_SCAN_DAYS: i64 = 36_600;
const WEEKLY_SCAN_WEEKS: i64 = 5_220;
/// monthly 搜索边界（月数；ZCode 1200 同值）。
const MONTHLY_SCAN_MONTHS: i64 = 1_200;

// ---------------------------------------------------------------------------
// next_run 五档计算（纯函数）
// ---------------------------------------------------------------------------

/// 下次运行时刻（Unix ms，本地日历；恒 > from；无未来候选 = None）。
pub(crate) fn next_run_at(spec: &TaskSpec, anchor_at: i64, from: i64) -> Option<i64> {
    let s = &spec.schedule;
    let interval = s.interval.max(1) as i64;
    match s.unit {
        // 分钟间隔从锚点推进 k·interval，不提前命中墙钟刻度；始终从固定
        // anchor 推导还能避免晚派发后逐轮累积漂移（automationCron.ts:141-146）。
        ScheduleUnit::Minute => {
            let step = interval * 60_000;
            let steps = (floor_div(from - anchor_at, step) + 1).max(1);
            Some(anchor_at + steps * step)
        }
        // 每小时只对齐 minute（秒清零），从对齐基点按小时步进
        ScheduleUnit::Hourly => {
            let (y, mo, d, h, _) = local_parts(anchor_at)?;
            let base = local_ymdhm(y, mo, d, h, s.minute as u32)?;
            let step = interval * 3_600_000;
            let steps = (floor_div(from - base, step) + 1).max(0);
            Some(base + steps * step)
        }
        ScheduleUnit::Daily => {
            let anchor_date = local_date(anchor_at)?;
            for i in 0..DAILY_SCAN_DAYS {
                let date = anchor_date.checked_add_days(Days::new((i * interval) as u64))?;
                let cand = local_ymdhm(
                    date.year(),
                    date.month(),
                    date.day(),
                    s.hour as u32,
                    s.minute as u32,
                )?;
                if cand > from {
                    return Some(cand);
                }
            }
            None
        }
        // 锚周取周一（weekdays 1-7，周一=1，对齐 §1.7 weekday_{1..7}），逐周扫
        ScheduleUnit::Weekly => {
            let anchor_date = local_date(anchor_at)?;
            let monday = anchor_date.checked_sub_days(Days::new(
                anchor_date.weekday().num_days_from_monday() as u64,
            ))?;
            let mut wds: Vec<u8> = s.weekdays.clone();
            if wds.is_empty() {
                wds.push(1); // 防御：校验已拒空表，旧数据兜底周一
            }
            wds.sort_unstable();
            wds.dedup();
            for week in (0..WEEKLY_SCAN_WEEKS).step_by(interval as usize) {
                for wd in &wds {
                    let date =
                        monday.checked_add_days(Days::new((week * 7 + *wd as i64 - 1) as u64))?;
                    let cand = local_ymdhm(
                        date.year(),
                        date.month(),
                        date.day(),
                        s.hour as u32,
                        s.minute as u32,
                    )?;
                    if cand > from {
                        return Some(cand);
                    }
                }
            }
            None
        }
        // 按月扫 month_day；溢出日（如 2/31）构造失败即跳过（对齐 ZCode
        // getMonth 过滤语义），hour:minute 定时刻
        ScheduleUnit::Monthly => {
            let (y, mo, _, _, _) = local_parts(anchor_at)?;
            for offset in (0..=MONTHLY_SCAN_MONTHS).step_by(interval as usize) {
                let total = mo as i64 - 1 + offset;
                let y2 = y + (total / 12) as i32;
                let m2 = (total % 12) as u32 + 1;
                let Some(date) = NaiveDate::from_ymd_opt(y2, m2, s.month_day as u32) else {
                    continue; // 溢出日
                };
                let cand = local_ymdhm(
                    date.year(),
                    date.month(),
                    date.day(),
                    s.hour as u32,
                    s.minute as u32,
                )?;
                if cand > from {
                    return Some(cand);
                }
            }
            None
        }
    }
}

// ---------------------------------------------------------------------------
// claim / settle / recover（&TaskStore 界面）
// ---------------------------------------------------------------------------

/// 认领到期作业（046-0 claim 流程，2026-11-23 重构「认领即消费」）：
/// due（enabled ∧ next_run_at ≤ now）→ spec 解析失败入休眠 → in-flight
/// 撞车 / misfire 超窗各落一条 skipped run + 指针前推 → 正常认领当场把
/// 指针前推到严格未来值再交派发（派发失败/崩溃都不会重复触发）。
pub(crate) fn claim_due(
    store: &TaskStore<'_>,
    now: i64,
    in_flight: &HashSet<String>,
) -> Vec<DueJob> {
    let defs = match store.list_enabled() {
        Ok(d) => d,
        Err(e) => {
            tasks_err(&format!("claim 扫描失败: {e}"));
            return Vec::new();
        }
    };
    let mut jobs = Vec::new();
    for def in defs {
        let next = match def.next_run_at {
            Some(t) if t <= now => t,
            _ => continue, // 未到期 / 未排期
        };
        let Ok(spec) = TaskSpec::from_json(&def.spec_json) else {
            // 数据异常防刷屏：指针清空入休眠（否则每 tick 重触发反复落账）
            tasks_err(&format!("claim: task={} spec 解析失败，入休眠", def.id));
            let _ = store.reschedule(&def.id, None, None);
            continue;
        };
        let anchor = def.anchor_at.unwrap_or(next);
        // 运行中再触发（manual 占用 / 上轮超期压格）→ 真实被挤掉的触发：
        // 记一条 skipped（at = 计划触发时刻）+ 指针前推，与 misfire 同构。
        // 一条 skip 行 ↔ 一个被消费的触发（原实现指针结算才推进，运行期每
        // tick 重复判定到期 → 每 tick 落一条 skip，任务日志 14 条的根因）。
        if in_flight.contains(&def.id) {
            skip_run(store, &def.id, next, "上一轮仍在运行");
            advance_pointer(store, &def.id, &spec, anchor, now);
            continue;
        }
        // misfire：计划触发时刻早于 now-5min → 错过窗口，skip 不补跑
        if next < now - MISFIRE_GRACE_MS {
            skip_run(
                store,
                &def.id,
                next,
                "错过触发窗口（应用未运行期间），不补跑",
            );
            advance_pointer(store, &def.id, &spec, anchor, now);
            continue;
        }
        // 认领即消费：指针先推进，派发失败/崩溃也不会重复触发。崩溃落在
        // 认领与 dispatch 落 run 行之间的 µs 窗口 = 丢一次运行不重复（两害
        // 取其轻，原设计该窗口是重启后重复触发）。
        advance_pointer(store, &def.id, &spec, anchor, now);
        jobs.push(DueJob {
            key: def.id,
            scheduled_at: next,
            manual: false,
        });
    }
    jobs
}

/// 指针前推：以当下为基准从锚点网格取下一格（恒严格未来；锚点不动防漂移）。
fn advance_pointer(store: &TaskStore<'_>, def_id: &str, spec: &TaskSpec, anchor: i64, now: i64) {
    let fwd = next_run_at(spec, anchor, now);
    let _ = store.reschedule(def_id, fwd, Some(anchor));
}

/// 落一条 skipped run（结果点 text_faint；hover 显示原因摘要）。
fn skip_run(store: &TaskStore<'_>, def_id: &str, at: i64, reason: &str) {
    match store.record_skip(def_id, Trigger::Schedule, at, reason) {
        Ok(_) => tasks_log(&format!("skip: task={def_id} at={at} ({reason})")),
        Err(e) => tasks_err(&format!("skip 落账失败: task={def_id} {e}")),
    }
}

/// 结算作业（046-0 settle 流程，2026-11-23 重构）：**只写结果不碰调度
/// 指针**——认领即消费后指针恒指未来，settle 重算是「运行期每 tick 重复
/// 到期」的根因；manual/schedule 之分只存于 run 行 trigger（start_run 落
/// 行时已记账），结算语义两者一致。无 running 行（派发前置失败等）= 空转，
/// 周期已由 claim 前推。
pub(crate) fn settle_job(
    store: &TaskStore<'_>,
    def_id: &str,
    outcome: &JobOutcome,
    now: i64,
) -> Result<(), String> {
    if let Some(row) = store.running_runs()?.iter().find(|r| r.def_id == def_id) {
        let oc = map_outcome(outcome);
        store.settle_run(row.id, oc, now, outcome.detail.as_deref())?;
        tasks_log(&format!(
            "结算: task={def_id} outcome={} 耗时={}ms",
            oc.as_str(),
            now - row.started_at
        ));
    }
    Ok(())
}

/// JobOutcome → 台账 outcome（细分键 timeout → Timeout，余按 success）。
fn map_outcome(outcome: &JobOutcome) -> Outcome {
    if outcome.success {
        Outcome::Ok
    } else if outcome.kind == "timeout" {
        Outcome::Timeout
    } else {
        Outcome::Failed
    }
}

/// 启动扫描（046-0 recover）：running 残留置 failed（result_json 注明
/// app_exit 中断；无 CLAIM_STALE 类僵尸回收——单进程无跨进程持有者）+
/// 首轮 prune。返回清理条数（log 用）。
pub(crate) fn recover(store: &TaskStore<'_>, now: i64) -> Result<usize, String> {
    let mut n = 0usize;
    for row in store.running_runs()? {
        store.settle_run(
            row.id,
            Outcome::Failed,
            now,
            Some("{\"error\":\"app_exit 中断\"}"),
        )?;
        n += 1;
    }
    n += store.prune_runs(RUN_KEEP_LIMIT)?;
    Ok(n)
}

// ---------------------------------------------------------------------------
// 本地日历 helpers（chrono Local；DTS 缺口时刻返回 None → 候选跳过）
// ---------------------------------------------------------------------------

fn local_parts(ms: i64) -> Option<(i32, u32, u32, u32, u32)> {
    match Local.timestamp_millis_opt(ms) {
        LocalResult::Single(t) => Some((t.year(), t.month(), t.day(), t.hour(), t.minute())),
        _ => None,
    }
}

fn local_date(ms: i64) -> Option<NaiveDate> {
    match Local.timestamp_millis_opt(ms) {
        LocalResult::Single(t) => Some(t.date_naive()),
        _ => None,
    }
}

fn local_ymdhm(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> Option<i64> {
    let naive = NaiveDate::from_ymd_opt(y, mo, d)?.and_hms_opt(h, mi, 0)?;
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(t) => Some(t.timestamp_millis()),
        _ => None, // DST 缺口/歧义时刻：跳过候选
    }
}

/// 向下取整除（JS Math.floor 语义；from < anchor 时 Rust 截断除会差一轮）。
fn floor_div(a: i64, b: i64) -> i64 {
    let q = a / b;
    if (a % b != 0) && ((a < 0) != (b < 0)) {
        q - 1
    } else {
        q
    }
}

// ---------------------------------------------------------------------------
// 单测（S5 的 test/046-* 收口用例只挑边界补；核心矩阵在这里固化）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use pf_context::domain::DbSource;
    use pf_context::domain::tasks::{AgentSpec, TaskKind, TaskSchedule};

    /// 测试窗口全落 1–2 月（南北半球 DST 均不切换），本地时区无关断言。
    fn local_ms(y: i32, m: u32, d: u32, h: u32, mi: u32) -> i64 {
        match Local.with_ymd_and_hms(y, m, d, h, mi, 0) {
            LocalResult::Single(t) => t.timestamp_millis(),
            _ => panic!("测试窗口不应含 DST 缺口"),
        }
    }

    fn mk_spec(
        unit: ScheduleUnit,
        interval: u32,
        hour: u8,
        minute: u8,
        weekdays: Vec<u8>,
        month_day: u8,
    ) -> TaskSpec {
        TaskSpec {
            kind: TaskKind::Agent,
            schedule: TaskSchedule {
                unit,
                interval,
                hour,
                minute,
                weekdays,
                month_day,
            },
            agent: Some(AgentSpec {
                prompt: "写日报".into(),
                project: r"D:\work".into(),
                tools: "default".into(),
                model: None,
                thinking: "auto".into(),
            }),
            script: None,
        }
    }

    fn db() -> (tempfile::TempDir, DbSource) {
        let dir = tempfile::tempdir().unwrap();
        let db = DbSource::open(dir.path()).unwrap();
        (dir, db)
    }

    fn make_def(
        store: &TaskStore<'_>,
        id: &str,
        spec: &TaskSpec,
        next_run_at: Option<i64>,
        anchor_at: Option<i64>,
    ) {
        store
            .create(&pf_context::domain::tasks::TaskDef {
                id: id.into(),
                title: "任务".into(),
                spec_json: serde_json::to_string(spec).unwrap(),
                enabled: true,
                next_run_at,
                anchor_at,
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();
    }

    fn agent_def_spec(unit: ScheduleUnit) -> TaskSpec {
        mk_spec(unit, 1, 8, 30, vec![], 0)
    }

    // ---- next_run 五档 ----

    #[test]
    fn minute_from_anchor_not_wall_clock() {
        let spec = mk_spec(ScheduleUnit::Minute, 10, 0, 0, vec![], 0);
        let anchor = local_ms(2026, 1, 10, 11, 46);
        // 11:47 查询：下一轮 11:56——不对齐 11:50 墙钟刻度（防抢跑）
        assert_eq!(
            next_run_at(&spec, anchor, local_ms(2026, 1, 10, 11, 47)).unwrap(),
            local_ms(2026, 1, 10, 11, 56)
        );
        // 恰在网格点查询：严格未来（11:56 → 12:06，不返 11:56 本身）
        assert_eq!(
            next_run_at(&spec, anchor, local_ms(2026, 1, 10, 11, 56)).unwrap(),
            local_ms(2026, 1, 10, 12, 6)
        );
        // 晚派发不漂移：from 落进第 3 格 → 仍从锚点网格取整（防逐轮累积）
        assert_eq!(
            next_run_at(&spec, anchor, local_ms(2026, 1, 10, 11, 58)).unwrap(),
            local_ms(2026, 1, 10, 12, 6)
        );
        // from 早于 anchor（时钟回拨）：给 anchor + 1 步
        assert_eq!(
            next_run_at(&spec, anchor, local_ms(2026, 1, 10, 11, 40)).unwrap(),
            local_ms(2026, 1, 10, 11, 56)
        );
    }

    #[test]
    fn hourly_aligns_minute_only() {
        let spec = mk_spec(ScheduleUnit::Hourly, 1, 0, 30, vec![], 0);
        let anchor = local_ms(2026, 1, 10, 11, 46);
        // 锚点时刻查询：下一个 11:30 已过 → 12:30
        assert_eq!(
            next_run_at(&spec, anchor, local_ms(2026, 1, 10, 11, 46)).unwrap(),
            local_ms(2026, 1, 10, 12, 30)
        );
        // from 早于对齐基点（11:30）：基点本身即下一轮
        assert_eq!(
            next_run_at(&spec, anchor, local_ms(2026, 1, 10, 11, 20)).unwrap(),
            local_ms(2026, 1, 10, 11, 30)
        );
    }

    #[test]
    fn daily_rolls_to_next_day_when_today_passed() {
        let spec = agent_def_spec(ScheduleUnit::Daily);
        let anchor = local_ms(2026, 1, 10, 8, 0);
        // 当天 08:30 已过 → 明天 08:30
        assert_eq!(
            next_run_at(&spec, anchor, local_ms(2026, 1, 10, 9, 0)).unwrap(),
            local_ms(2026, 1, 11, 8, 30)
        );
        // 当天未到 → 今天 08:30
        assert_eq!(
            next_run_at(&spec, anchor, local_ms(2026, 1, 10, 7, 0)).unwrap(),
            local_ms(2026, 1, 10, 8, 30)
        );
    }

    #[test]
    fn weekly_monday_is_one_and_nearest_weekday_wins() {
        // 2026-01-05 是周一；weekdays [一=1, 三=3]
        let spec = mk_spec(ScheduleUnit::Weekly, 1, 8, 30, vec![1, 3], 0);
        // 锚点 = 周一 09:00（本周一 08:30 已过）→ 下一轮本周三
        let anchor = local_ms(2026, 1, 5, 9, 0);
        assert_eq!(
            next_run_at(&spec, anchor, local_ms(2026, 1, 5, 9, 0)).unwrap(),
            local_ms(2026, 1, 7, 8, 30)
        );
        // 周三过后 → 下周一（锚周取周一逐周扫）
        assert_eq!(
            next_run_at(&spec, anchor, local_ms(2026, 1, 7, 9, 0)).unwrap(),
            local_ms(2026, 1, 12, 8, 30)
        );
        // 周日=7
        let spec = mk_spec(ScheduleUnit::Weekly, 1, 8, 30, vec![7], 0);
        assert_eq!(
            next_run_at(&spec, anchor, local_ms(2026, 1, 5, 9, 0)).unwrap(),
            local_ms(2026, 1, 11, 8, 30)
        );
    }

    #[test]
    fn monthly_skips_overflow_day() {
        // month_day=31：2 月无 31 日 → 跳过，1 月锚 → 3 月 31
        let spec = mk_spec(ScheduleUnit::Monthly, 1, 8, 30, vec![], 31);
        let anchor = local_ms(2026, 1, 31, 9, 0);
        assert_eq!(
            next_run_at(&spec, anchor, local_ms(2026, 2, 1, 0, 0)).unwrap(),
            local_ms(2026, 3, 31, 8, 30)
        );
        // 普通日：2 月 10 日正常命中
        let spec = mk_spec(ScheduleUnit::Monthly, 1, 8, 30, vec![], 10);
        let anchor = local_ms(2026, 1, 10, 9, 0);
        assert_eq!(
            next_run_at(&spec, anchor, local_ms(2026, 2, 1, 0, 0)).unwrap(),
            local_ms(2026, 2, 10, 8, 30)
        );
    }

    // ---- claim ----

    #[test]
    fn claim_filters_inflight_misfire_and_due() {
        let (_dir, db) = db();
        let store = TaskStore::new(&db);
        let now = local_ms(2026, 2, 10, 12, 0);

        // due（宽限内 1 分钟前）
        make_def(
            &store,
            "due",
            &agent_def_spec(ScheduleUnit::Daily),
            Some(now - 60_000),
            Some(now - 86_400_000),
        );
        // misfire（超 5min 宽限）
        make_def(
            &store,
            "late",
            &agent_def_spec(ScheduleUnit::Daily),
            Some(now - 10 * 60_000),
            Some(now - 86_400_000),
        );
        // in-flight 撞车
        make_def(
            &store,
            "busy",
            &agent_def_spec(ScheduleUnit::Daily),
            Some(now - 60_000),
            Some(now - 86_400_000),
        );
        // 未到期 / 停用
        make_def(
            &store,
            "future",
            &agent_def_spec(ScheduleUnit::Daily),
            Some(now + 60_000),
            Some(now),
        );
        make_def(
            &store,
            "off",
            &agent_def_spec(ScheduleUnit::Daily),
            Some(now - 60_000),
            Some(now),
        );
        store.set_enabled("off", false).unwrap();

        let mut in_flight = HashSet::new();
        in_flight.insert("busy".to_string());
        let jobs = claim_due(&store, now, &in_flight);

        let keys: Vec<&str> = jobs.iter().map(|j| j.key.as_str()).collect();
        assert_eq!(keys, vec!["due"], "只有宽限内到期且不撞车的进派发");

        // busy：in-flight skip 落账（at = 计划触发时刻）+ 指针前推
        let busy = store.last_run("busy").unwrap().unwrap();
        assert_eq!(busy.outcome, Outcome::Skipped);
        assert_eq!(busy.started_at, now - 60_000);
        let busy_fwd =
            next_run_at(&agent_def_spec(ScheduleUnit::Daily), now - 86_400_000, now).unwrap();
        assert_eq!(
            store.get_def("busy").unwrap().unwrap().next_run_at,
            Some(busy_fwd)
        );

        // due：认领即消费——指针当场前推，且认领本身不落 run 行
        let due_fwd =
            next_run_at(&agent_def_spec(ScheduleUnit::Daily), now - 86_400_000, now).unwrap();
        assert_eq!(
            store.get_def("due").unwrap().unwrap().next_run_at,
            Some(due_fwd)
        );
        assert!(store.last_run("due").unwrap().is_none(), "认领不落 run 行");

        // late：misfire skip 落账（started_at = 计划触发时刻）+ next_run 前推
        let late = store.last_run("late").unwrap().unwrap();
        assert_eq!(late.outcome, Outcome::Skipped);
        assert_eq!(late.started_at, now - 10 * 60_000);
        let def = store.get_def("late").unwrap().unwrap();
        let anchor = def.anchor_at.unwrap();
        assert_eq!(
            def.next_run_at.unwrap(),
            next_run_at(&agent_def_spec(ScheduleUnit::Daily), anchor, now).unwrap(),
            "misfire 后前推到下一边界"
        );
    }

    /// 回归（2026-11-23 用户令）：运行中的任务每 20s tick 不再刷 skipped
    /// 行——认领即消费后指针恒指未来，运行期 claim 恒空转；台账只 1 行。
    #[test]
    fn running_task_ticks_do_not_spam_ledger() {
        let (_dir, db) = db();
        let store = TaskStore::new(&db);
        let t0 = local_ms(2026, 2, 10, 12, 0);
        let spec = mk_spec(ScheduleUnit::Minute, 20, 0, 0, vec![], 0);
        make_def(&store, "t", &spec, Some(t0), Some(t0));

        // tick1 认领派发：指针当场推进下一格（+20min）；run 行由 dispatch 落
        let jobs = claim_due(&store, t0, &HashSet::new());
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].scheduled_at, t0);
        assert_eq!(
            store.get_def("t").unwrap().unwrap().next_run_at,
            Some(t0 + 20 * 60_000),
            "认领即消费：指针前推到下一格"
        );
        store.start_run("t", Trigger::Schedule, t0, None).unwrap();

        // 运行期每 20s tick：claim 空转零新增行（原实现每 tick 一条 skipped）
        let inflight: HashSet<String> = ["t".to_string()].into();
        for s in [20_000, 40_000, 60_000, 120_000] {
            assert!(claim_due(&store, t0 + s, &inflight).is_empty());
        }
        assert_eq!(store.list_runs(Some("t"), -1).unwrap().len(), 1);

        // 结算只写结果：仍 1 行，指针不动
        settle_job(&store, "t", &JobOutcome::ok(None), t0 + 125_000).unwrap();
        assert_eq!(store.list_runs(Some("t"), -1).unwrap().len(), 1);
        assert_eq!(
            store.get_def("t").unwrap().unwrap().next_run_at,
            Some(t0 + 20 * 60_000)
        );

        // 下一格到期 → 正常进入下一轮
        let jobs = claim_due(&store, t0 + 20 * 60_000 + 1_000, &HashSet::new());
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].scheduled_at, t0 + 20 * 60_000);
    }

    /// in-flight 真实重叠（manual 占用 / 上轮超期压格）：一个被挤掉的触发
    /// 只落一条 skipped（at = 计划触发时刻）+ 指针前推，后续 tick 零新增。
    #[test]
    fn in_flight_overlap_skips_once_and_forwards() {
        let (_dir, db) = db();
        let store = TaskStore::new(&db);
        let now = local_ms(2026, 2, 10, 12, 0);
        let anchor = local_ms(2026, 2, 10, 8, 0);
        let spec = agent_def_spec(ScheduleUnit::Daily); // 每天 08:30
        make_def(&store, "t", &spec, Some(now), Some(anchor));

        let inflight: HashSet<String> = ["t".to_string()].into();
        let jobs = claim_due(&store, now, &inflight);
        assert!(jobs.is_empty());
        let skip = store.last_run("t").unwrap().unwrap();
        assert_eq!(skip.outcome, Outcome::Skipped);
        assert_eq!(skip.started_at, now, "skip 行挂计划触发时刻");
        let fwd = next_run_at(&spec, anchor, now).unwrap();
        assert_eq!(store.get_def("t").unwrap().unwrap().next_run_at, Some(fwd));

        // 后续 tick（仍在运行）：指针已前推，零新增行
        for s in [20_000, 40_000] {
            assert!(claim_due(&store, now + s, &inflight).is_empty());
        }
        assert_eq!(store.list_runs(Some("t"), -1).unwrap().len(), 1);
    }

    // ---- settle ----

    #[test]
    fn settle_writes_outcome_and_never_touches_pointer() {
        let (_dir, db) = db();
        let store = TaskStore::new(&db);
        let now = local_ms(2026, 2, 10, 12, 0);
        let anchor = local_ms(2026, 2, 10, 8, 0);
        let spec = mk_spec(ScheduleUnit::Minute, 20, 0, 0, vec![], 0);
        make_def(&store, "t", &spec, Some(anchor), Some(anchor));

        let run = store
            .start_run("t", Trigger::Schedule, now - 120_000, None)
            .unwrap();
        settle_job(&store, "t", &JobOutcome::ok(None), now).unwrap();

        let row = store.last_run("t").unwrap().unwrap();
        assert_eq!(row.id, run);
        assert_eq!(row.outcome, Outcome::Ok);
        assert_eq!(row.finished_at, Some(now));
        // settle 不碰指针：next_run 保持认领时前推的值（这里即建库值）
        let def = store.get_def("t").unwrap().unwrap();
        assert_eq!(def.next_run_at, Some(anchor));
        assert_eq!(def.anchor_at, Some(anchor));
    }

    #[test]
    fn settle_manual_never_touches_schedule() {
        let (_dir, db) = db();
        let store = TaskStore::new(&db);
        let now = local_ms(2026, 2, 10, 12, 0);
        make_def(
            &store,
            "t",
            &agent_def_spec(ScheduleUnit::Daily),
            Some(local_ms(2026, 2, 11, 8, 30)),
            Some(now),
        );

        let run = store.start_run("t", Trigger::Manual, now, None).unwrap();
        settle_job(
            &store,
            "t",
            &JobOutcome::failed("failed", None),
            now + 5_000,
        )
        .unwrap();

        let row = store.last_run("t").unwrap().unwrap();
        assert_eq!(row.id, run);
        assert_eq!(row.outcome, Outcome::Failed);
        let def = store.get_def("t").unwrap().unwrap();
        assert_eq!(
            def.next_run_at,
            Some(local_ms(2026, 2, 11, 8, 30)),
            "manual 不动 next_run"
        );
        assert_eq!(def.anchor_at, Some(now), "manual 不动锚点");
    }

    #[test]
    fn settle_without_running_row_is_noop() {
        let (_dir, db) = db();
        let store = TaskStore::new(&db);
        let now = local_ms(2026, 2, 10, 12, 0);
        let anchor = local_ms(2026, 2, 10, 8, 0);
        let spec = agent_def_spec(ScheduleUnit::Daily);
        make_def(&store, "t", &spec, Some(now - 60_000), Some(anchor));

        // 派发前置失败路径（无 running 行）：settle 空转不落账；周期已由
        // claim 认领时前推——这里不再兜底重排（防失败循环重触发）
        settle_job(
            &store,
            "t",
            &JobOutcome::failed("failed", Some("{\"error\":\"x\"}".into())),
            now,
        )
        .unwrap();
        assert!(store.last_run("t").unwrap().is_none());
        let def = store.get_def("t").unwrap().unwrap();
        assert_eq!(def.next_run_at, Some(now - 60_000), "settle 不碰指针");
    }

    #[test]
    fn settle_timeout_maps_by_kind() {
        let (_dir, db) = db();
        let store = TaskStore::new(&db);
        let now = local_ms(2026, 2, 10, 12, 0);
        make_def(
            &store,
            "t",
            &agent_def_spec(ScheduleUnit::Daily),
            Some(now),
            Some(now),
        );
        store.start_run("t", Trigger::Schedule, now, None).unwrap();
        settle_job(
            &store,
            "t",
            &JobOutcome::failed("timeout", None),
            now + 1000,
        )
        .unwrap();
        assert_eq!(
            store.last_run("t").unwrap().unwrap().outcome,
            Outcome::Timeout
        );
    }

    // ---- recover ----

    #[test]
    fn recover_marks_orphans_failed_and_prunes_first_round() {
        let (_dir, db) = db();
        let store = TaskStore::new(&db);
        let now = local_ms(2026, 2, 10, 12, 0);
        make_def(
            &store,
            "t",
            &agent_def_spec(ScheduleUnit::Daily),
            None,
            None,
        );
        // 上个实例残留：2 条 running（app 死时正在跑 → started_at 最新）+ 60 条已结算
        store.start_run("t", Trigger::Schedule, 500, None).unwrap();
        store.start_run("t", Trigger::Manual, 501, None).unwrap();
        for i in 0..60 {
            let id = store
                .start_run("t", Trigger::Schedule, 100 + i, None)
                .unwrap();
            store.settle_run(id, Outcome::Ok, 200 + i, None).unwrap();
        }

        let cleaned = recover(&store, now).unwrap();
        assert_eq!(
            cleaned, 14,
            "2 条残留置 failed + prune 删 12 条（62 行保最新 50）"
        );

        let running = store.running_runs().unwrap();
        assert!(running.is_empty(), "残留全部收口");
        // 残留行置 failed + 注明中断（残留最新，prune 不删）
        let orphan = store
            .list_runs(Some("t"), -1)
            .unwrap()
            .into_iter()
            .find(|r| r.started_at == 500)
            .unwrap();
        assert_eq!(orphan.outcome, Outcome::Failed);
        assert!(orphan.result_json.as_deref().unwrap().contains("app_exit"));
        assert_eq!(store.list_runs(Some("t"), -1).unwrap().len(), 50);
        assert_eq!(recover(&store, now).unwrap(), 0, "二轮幂等");
    }
}
