//! DbSource：rusqlite（bundled SQLite）WAL 持久层（001-3 S5）。
//!
//! - 连接 = `<PF数据目录>/context.db`，`journal_mode=WAL`（读写并发）+
//!   `busy_timeout=5s`（多实例/多线程争用排队，不硬失败）+
//!   `synchronous=NORMAL`（WAL 推荐档，掉电最多丢最后事务——域数据可重建）；
//! - schema 版本化迁移：`PRAGMA user_version` 逐版前进（v1 = 046 任务
//!   定义/运行记录，见 [`super::tasks`]）；
//! - 事务失败即回滚：写路径走 [`DbSource::with_tx`]，闭包 Err → 事务 drop
//!   → rusqlite 自动 ROLLBACK，半截数据不出现在库里；
//! - 线程模型：单连接 + Mutex（domain 写频率 = 任务/管家事件级，不是热路径）。

use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::Source;

/// schema 当前版本（迁移目标；每次加表/列 = 版本 +1 + 新分支）。
const SCHEMA_VERSION: i64 = 1;

pub struct DbSource {
    path: PathBuf,
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for DbSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbSource")
            .field("path", &self.path)
            .finish()
    }
}

impl DbSource {
    /// 打开（懒迁移：首次调用即建库 + 迁移到 [`SCHEMA_VERSION`]）。
    pub fn open(dir: &Path) -> Result<DbSource, String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let path = dir.join("context.db");
        let conn = Connection::open(&path).map_err(|e| e.to_string())?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| e.to_string())?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(|e| e.to_string())?;
        let db = DbSource {
            path,
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    /// 版本化迁移：user_version 逐版前进，迁移整体包事务（失败回滚到旧版）。
    fn migrate(&self) -> Result<(), String> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let v = conn
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .map_err(|e| e.to_string())?;
        if v == SCHEMA_VERSION {
            return Ok(());
        }
        if v > SCHEMA_VERSION {
            return Err(format!(
                "context.db schema v{v} 比本程序 v{SCHEMA_VERSION} 新——请先升级 PF"
            ));
        }
        conn.execute_batch("BEGIN").map_err(|e| e.to_string())?;
        let result = (v..SCHEMA_VERSION).try_for_each(|step| -> Result<(), String> {
            match step {
                0 => {
                    // v1：046 自动任务种子 schema（任务定义 + 运行记录）
                    conn.execute_batch(
                        "CREATE TABLE task_defs (
                            id         TEXT PRIMARY KEY,
                            title      TEXT NOT NULL,
                            spec_json  TEXT NOT NULL,
                            created_at INTEGER NOT NULL,
                            updated_at INTEGER NOT NULL
                        );
                        CREATE TABLE task_runs (
                            id          INTEGER PRIMARY KEY AUTOINCREMENT,
                            def_id      TEXT NOT NULL REFERENCES task_defs(id),
                            started_at  INTEGER NOT NULL,
                            finished_at INTEGER,
                            ok          INTEGER,
                            result_json TEXT
                        );
                        CREATE INDEX idx_task_runs_def ON task_runs(def_id, started_at);",
                    )
                    .map_err(|e| e.to_string())?;
                }
                other => return Err(format!("未知迁移步 {other}")),
            }
            Ok(())
        });
        match result {
            Ok(()) => conn
                .execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}; COMMIT"))
                .map_err(|e| e.to_string()),
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(format!("迁移失败（已回滚）: {e}"))
            }
        }
    }

    /// 事务写：闭包拿到事务，`Err` = 整体回滚（半截数据不出库）。
    pub fn with_tx<T>(
        &self,
        f: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        match f(&tx) {
            Ok(v) => {
                tx.commit().map_err(|e| e.to_string())?;
                Ok(v)
            }
            Err(e) => {
                // drop(tx) = ROLLBACK；显式回滚防早退歧义
                let _ = tx.rollback();
                Err(e)
            }
        }
    }

    /// 只读查询（借用连接）。
    pub fn with_conn<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, String>,
    ) -> Result<T, String> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        f(&conn)
    }

    /// 表存在性（架构断言 / 迁移测试用）。
    pub fn has_table(&self, name: &str) -> bool {
        self.with_conn(|c| {
            let n: i64 = c
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [name],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            Ok(n > 0)
        })
        .unwrap_or(false)
    }

    /// DB 文件路径（观测 / 排障）。
    pub fn file(&self) -> &Path {
        &self.path
    }
}

impl Source for DbSource {
    fn name(&self) -> &str {
        "sqlite(context.db)"
    }

    fn ping(&self) -> Result<(), String> {
        self.with_conn(|c| {
            let one: i64 = c
                .query_row("SELECT 1", [], |r| r.get(0))
                .map_err(|e| e.to_string())?;
            (one == 1)
                .then_some(())
                .ok_or_else(|| "sqlite ping 异常".to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pf-context-db-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn wal_mode_and_version_persist() {
        let dir = tmp_dir("wal");
        {
            let db = DbSource::open(&dir).unwrap();
            let mode: String = db
                .with_conn(|c| {
                    Ok(c.query_row("PRAGMA journal_mode", [], |r| r.get(0))
                        .unwrap())
                })
                .unwrap();
            assert_eq!(mode.to_lowercase(), "wal");
        } // drop 连接（WAL 下不持锁）
        let db = DbSource::open(&dir).unwrap(); // 重开：版本已迁移，不再动
        assert!(db.has_table("task_defs"));
        assert!(db.has_table("task_runs"));
    }

    #[test]
    fn newer_schema_file_is_rejected_with_clear_error() {
        let dir = tmp_dir("future");
        {
            let conn = Connection::open(dir.join("context.db")).unwrap();
            conn.execute_batch("PRAGMA user_version = 99").unwrap();
        }
        let err = DbSource::open(&dir).unwrap_err();
        assert!(err.contains("v99"), "{err}");
        assert!(err.contains("升级"), "{err}");
    }

    #[test]
    fn concurrent_writers_queue_via_busy_timeout() {
        let dir = tmp_dir("concurrent");
        let db1 = DbSource::open(&dir).unwrap();
        let db2 = DbSource::open(&dir).unwrap(); // 双实例同库（WAL 多连接）
        let t1 = std::thread::spawn(move || {
            for i in 0..20 {
                db1.with_tx(|tx| {
                    tx.execute(
                        "INSERT INTO task_defs VALUES (?1, 'x', '{}', 0, 0)",
                        [format!("a{i}")],
                    )
                    .map_err(|e| e.to_string())?;
                    Ok(())
                })
                .unwrap();
            }
            db1
        });
        let t2 = std::thread::spawn(move || {
            for i in 0..20 {
                db2.with_tx(|tx| {
                    tx.execute(
                        "INSERT INTO task_defs VALUES (?1, 'y', '{}', 0, 0)",
                        [format!("b{i}")],
                    )
                    .map_err(|e| e.to_string())?;
                    Ok(())
                })
                .unwrap();
            }
            db2
        });
        let (d1, d2) = (t1.join().unwrap(), t2.join().unwrap());
        let n: i64 = d1
            .with_conn(|c| {
                Ok(
                    c.query_row("SELECT COUNT(*) FROM task_defs", [], |r| r.get(0))
                        .unwrap(),
                )
            })
            .unwrap();
        assert_eq!(n, 40, "双写者经 WAL+busy_timeout 全部落库");
        drop(d2);
    }

    #[test]
    fn tx_error_rolls_back() {
        let dir = tmp_dir("rollback");
        let db = DbSource::open(&dir).unwrap();
        let r: Result<(), String> = db.with_tx(|tx| {
            tx.execute("INSERT INTO task_defs VALUES ('t1','x','{}',0,0)", [])
                .map_err(|e| e.to_string())?;
            Err("业务失败".into()) // 闭包 Err → 回滚
        });
        assert!(r.is_err());
        let n: i64 = db
            .with_conn(|c| {
                Ok(
                    c.query_row("SELECT COUNT(*) FROM task_defs", [], |r| r.get(0))
                        .unwrap(),
                )
            })
            .unwrap();
        assert_eq!(n, 0, "半截插入必须被回滚");
    }
}
