//! 046 TaskStore 封装：hub → context.db → pf-context TaskStore 的业务面。
//!
//! CRUD 入口（S4 UI 消费）+ 状态面缓存来源（[`list_cached`]）+ 锚点/next_run
//! 业务规则（创建即锚、编辑 schedule 才重锚——ZCode automationService 同款）。
//! 存储行形状与校验在 pf-context（S1），本层不重复。

use std::sync::atomic::{AtomicU64, Ordering};

use pf_context::domain::tasks::{TaskDef, TaskRun, TaskSpec, TaskStore};

use super::sched;
use super::{DefRow, tasks_err, tasks_log};

/// 进程内 id 序号（时间戳已保唯一，序号防同 ms 内撞）。
#[allow(dead_code)] // 046-S4 创建任务接线
static ID_SEQ: AtomicU64 = AtomicU64::new(0);

/// 打开任务存储（hub 懒开 context.db；&'static 直接喂 TaskStore）。
pub(crate) fn open_store() -> Result<TaskStore<'static>, String> {
    let db = pf_context::hub().domain.db()?;
    Ok(TaskStore::new(db))
}

/// 任务定义缓存（状态面刷新源）：spec 解析失败的行跳过（单测可手工注入
/// 脏行；正常路径写前已校验）。
pub(crate) fn list_cached() -> Vec<DefRow> {
    let store = match open_store() {
        Ok(s) => s,
        Err(e) => {
            tasks_err(&format!("任务列表读取失败: {e}"));
            return Vec::new();
        }
    };
    match store.list_defs() {
        Ok(defs) => defs
            .into_iter()
            .filter_map(|def| match TaskSpec::from_json(&def.spec_json) {
                Ok(spec) => Some(DefRow { def, spec }),
                Err(e) => {
                    tasks_err(&format!("spec 解析失败（行跳过）: id={} {e}", def.id));
                    None
                }
            })
            .collect(),
        Err(e) => {
            tasks_err(&format!("任务列表查询失败: {e}"));
            Vec::new()
        }
    }
}

/// 运行台账缓存（状态面 runs 源；limit ≤ 0 = 全量，started_at 倒序）。
pub(crate) fn list_runs_cached() -> Vec<TaskRun> {
    match open_store() {
        Ok(s) => s.list_runs(None, -1).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// 创建任务：id 生成、创建即锚（anchor_at = now）、next_run 从锚点起算。
/// 返回 id。上限 20 与 spec 校验在存储层（非法值拒写）。
/// 046-S4 创建表单接线。
#[allow(dead_code)]
pub(crate) fn create_task(title: &str, spec: &TaskSpec, now: i64) -> Result<String, String> {
    let id = format!(
        "task-{:x}-{:x}",
        now,
        ID_SEQ.fetch_add(1, Ordering::Relaxed)
    );
    let next = sched::next_run_at(spec, now, now);
    let def = TaskDef {
        id: id.clone(),
        title: title.trim().to_string(),
        spec_json: serde_json::to_string(spec).map_err(|e| format!("spec 序列化失败: {e}"))?,
        enabled: true,
        next_run_at: next,
        anchor_at: Some(now),
        created_at: now,
        updated_at: now,
    };
    open_store()?.create(&def)?;
    tasks_log(&format!("创建任务: id={id} title={}", def.title));
    Ok(id)
}

/// 保存编辑：schedule 内容变化 → 重锚 now + next_run 重算；未变（只改名称/
/// 描述等）→ 保留原锚与原 next_run（ZCode automationService.ts:279-285）。
/// 046-S4 编辑表单接线。
#[allow(dead_code)]
pub(crate) fn save_edit(
    def_id: &str,
    title: &str,
    spec: &TaskSpec,
    enabled: bool,
    now: i64,
) -> Result<(), String> {
    let store = open_store()?;
    let old = store
        .get_def(def_id)?
        .ok_or_else(|| format!("任务不存在: {def_id}"))?;
    let old_schedule = TaskSpec::from_json(&old.spec_json).ok().map(|s| s.schedule);
    let (anchor, next) = if old_schedule.as_ref() == Some(&spec.schedule) {
        (old.anchor_at.unwrap_or(now), old.next_run_at)
    } else {
        // 改周期即重锚：anchor=now，next_run 按新规则从 now 重算
        (now, sched::next_run_at(spec, now, now))
    };
    let def = TaskDef {
        id: def_id.to_string(),
        title: title.trim().to_string(),
        spec_json: serde_json::to_string(spec).map_err(|e| format!("spec 序列化失败: {e}"))?,
        enabled,
        next_run_at: next,
        anchor_at: Some(anchor),
        created_at: old.created_at,
        updated_at: now,
    };
    store.update_def(&def)?;
    tasks_log(&format!("编辑任务: id={def_id} title={}", def.title));
    Ok(())
}

/// 启用⇄停用：启用时 next_run 缺失或已过期 → 从 now 重排（锚点保留，防
/// 漂移）；未来仍有排期则原样放行。
/// 046-S4 行菜单启停接线。
#[allow(dead_code)]
pub(crate) fn set_enabled_task(def_id: &str, enabled: bool, now: i64) -> Result<(), String> {
    let store = open_store()?;
    if enabled {
        if let Some(def) = store.get_def(def_id)? {
            let due = def.next_run_at.map_or(true, |t| t <= now);
            if due {
                let spec = TaskSpec::from_json(&def.spec_json)?;
                let anchor = def.anchor_at.unwrap_or(now);
                let next = sched::next_run_at(&spec, anchor, now);
                store.reschedule(def_id, next, Some(anchor))?;
            }
        }
    }
    store.set_enabled(def_id, enabled)?;
    tasks_log(&format!(
        "{}: id={def_id}",
        if enabled { "启用" } else { "停用" }
    ));
    Ok(())
}

/// 删除任务（同事务连带删 runs；删除量进 log）。运行中删除先中止再删的
/// abort 归 046-S3 执行链；本层直删后结算路径对已删任务记 error（不复活）。
/// 046-S4 删除接线。
#[allow(dead_code)]
pub(crate) fn delete_task(def_id: &str) -> Result<usize, String> {
    let n = open_store()?.delete(def_id)?;
    tasks_log(&format!("删除任务: id={def_id} 连带 run {n} 条"));
    Ok(n)
}
