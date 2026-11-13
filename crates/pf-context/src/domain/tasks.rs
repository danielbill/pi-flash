//! 046 自动任务持久层种子（domain 首个消费方，001-3 S5）。
//!
//! 046 特性模块尚未开建——这里先落**任务定义 + 运行记录**的 schema 与读写
//! API（001-0 §2「domain = 任务定义与运行」），046 模块开建时直接消费
//! `pf_context::domain::TaskStore`，不再各写一份存储。周期调度、触发条件等
//! 归 046 模块本体，不在此处。
//!
//! 「消费方能读回上次运行结果」：[`TaskStore::record_run`] 写 +
//! [`TaskStore::last_run`] 读回，单测锁定。

use rusqlite::params;

use super::DbSource;

/// 任务定义（046：调度周期/触发条件随 046 模块定稿进 `spec_json`）。
#[derive(Debug, Clone, PartialEq)]
pub struct TaskDef {
    pub id: String,
    pub title: String,
    /// 任务定义 JSON（046 模块定 schema；此处透明存取）
    pub spec_json: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 一次运行记录。
#[derive(Debug, Clone, PartialEq)]
pub struct TaskRun {
    pub id: i64,
    pub def_id: String,
    pub started_at: i64,
    /// `None` = 运行中
    pub finished_at: Option<i64>,
    /// `None` = 运行中 / 无结论
    pub ok: Option<bool>,
    pub result_json: Option<String>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// 任务存储（046 种子消费方，挂 [`DbSource`]）。
pub struct TaskStore<'a> {
    db: &'a DbSource,
}

impl<'a> TaskStore<'a> {
    pub fn new(db: &'a DbSource) -> Self {
        Self { db }
    }

    /// 写入/更新任务定义（整份覆盖 spec）。
    pub fn upsert_def(&self, def: &TaskDef) -> Result<(), String> {
        self.db.with_tx(|tx| {
            tx.execute(
                "INSERT INTO task_defs (id, title, spec_json, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?4)
                 ON CONFLICT(id) DO UPDATE SET
                   title = excluded.title,
                   spec_json = excluded.spec_json,
                   updated_at = excluded.updated_at",
                params![def.id, def.title, def.spec_json, def.created_at],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })
    }

    pub fn list_defs(&self) -> Result<Vec<TaskDef>, String> {
        self.db.with_conn(|c| {
            let mut stmt = c
                .prepare("SELECT id, title, spec_json, created_at, updated_at FROM task_defs ORDER BY updated_at DESC")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(TaskDef {
                        id: r.get(0)?,
                        title: r.get(1)?,
                        spec_json: r.get(2)?,
                        created_at: r.get(3)?,
                        updated_at: r.get(4)?,
                    })
                })
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
        })
    }

    /// 记一次运行开始（finished/ok 待 [`finish_run`] 补）。
    pub fn record_run(&self, def_id: &str, result_json: Option<&str>) -> Result<i64, String> {
        let started = now_ms();
        self.db.with_tx(|tx| {
            tx.execute(
                "INSERT INTO task_runs (def_id, started_at, finished_at, ok, result_json)
                 VALUES (?1, ?2, ?2, ?3, ?4)",
                params![
                    def_id,
                    started,
                    result_json.map(|_| 1), // 有结果即视为已完成的一次运行
                    result_json,
                ],
            )
            .map_err(|e| e.to_string())?;
            Ok(tx.last_insert_rowid())
        })
    }

    /// 某任务**上次运行结果**（046 面板/管家显示用；无记录 = None）。
    pub fn last_run(&self, def_id: &str) -> Result<Option<TaskRun>, String> {
        self.db.with_conn(|c| {
            let mut stmt = c
                .prepare(
                    "SELECT id, def_id, started_at, finished_at, ok, result_json
                     FROM task_runs WHERE def_id = ?1
                     ORDER BY started_at DESC, id DESC LIMIT 1",
                )
                .map_err(|e| e.to_string())?;
            let mut rows = stmt
                .query_map([def_id], |r| {
                    Ok(TaskRun {
                        id: r.get(0)?,
                        def_id: r.get(1)?,
                        started_at: r.get(2)?,
                        finished_at: r.get(3)?,
                        ok: r.get(4)?,
                        result_json: r.get(5)?,
                    })
                })
                .map_err(|e| e.to_string())?;
            match rows.next() {
                Some(row) => row.map(Some).map_err(|e| e.to_string()),
                None => Ok(None),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::tmp_dir;

    /// 返回 (临时目录, DbSource)：目录必须活得比连接久，
    /// 否则 Drop 删目录时 rusqlite 还持有 context.db。
    fn db() -> (crate::tests_support::TestDir, DbSource) {
        let dir = tmp_dir("tasks");
        let db = DbSource::open(&dir).unwrap();
        (dir, db)
    }

    #[test]
    fn consumer_writes_and_reads_back_last_run() {
        let (_dir, db) = db();
        let tasks = TaskStore::new(&db);
        tasks
            .upsert_def(&TaskDef {
                id: "daily-digest".into(),
                title: "每日摘要".into(),
                spec_json: r#"{"cron":"0 9 * * *"}"#.into(),
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();

        assert!(tasks.last_run("daily-digest").unwrap().is_none());
        tasks
            .record_run("daily-digest", Some(r#"{"summary":"ok-1"}"#))
            .unwrap();
        tasks
            .record_run("daily-digest", Some(r#"{"summary":"ok-2"}"#))
            .unwrap();
        let last = tasks.last_run("daily-digest").unwrap().expect("有运行记录");
        assert_eq!(
            last.result_json.as_deref(),
            Some(r#"{"summary":"ok-2"}"#),
            "读回的是上次运行结果"
        );
        assert_eq!(last.def_id, "daily-digest");
        assert_eq!(tasks.last_run("no-such").unwrap(), None);
    }

    #[test]
    fn upsert_overwrites_spec_keeps_single_row() {
        let (_dir, db) = db();
        let tasks = TaskStore::new(&db);
        let def = |spec: &str| TaskDef {
            id: "t".into(),
            title: "任务".into(),
            spec_json: spec.into(),
            created_at: 7,
            updated_at: 7,
        };
        tasks.upsert_def(&def(r#"{"v":1}"#)).unwrap();
        tasks.upsert_def(&def(r#"{"v":2}"#)).unwrap();
        let defs = tasks.list_defs().unwrap();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].spec_json, r#"{"v":2}"#);
    }
}
