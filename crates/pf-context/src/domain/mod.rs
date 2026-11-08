//! domain 子集（001-0 §2）：域数据——任务（046）与管家（100）的定义与运行。
//!
//! **SQLite WAL 持久层（001-3 S5）；事务失败即回滚；settings 永不进 DB**
//! （001-0 §4 规则 3：同一 key 不允许既在文件又在数据库——架构测试断言
//! settings section 键与 DB 表名零交集，见 `tests::settings_never_in_db`）。
//!
//! DB 文件 = `<PF数据目录>/context.db`（[`DbSource::open`]；路径由 Hub 注入），
//! 连接懒开（首次 domain 访问才建库，启动热路径零成本，010 铁律）。备份/清理
//! 误删防区：PF 自有清理逻辑只扫 sessions/assets 缓存子目录，不碰数据目录
//! 根下的 `context.db`（与 app-settings.json 同级的常驻档）。
//!
//! 首个消费方 = 046 自动任务的持久层种子（[`tasks::TaskStore`]：任务定义 +
//! 运行记录）——046 特性模块尚未开建，schema 与读写 API 先行落此，届时
//! `pf_context::domain::TaskStore` 直接消费。

mod db;
pub mod tasks;

use std::path::PathBuf;
use std::sync::OnceLock;

pub use db::DbSource;

/// 域数据源 trait（001-3 S5：File/Memory 先行，DB 实现落 [`DbSource`]）。
/// S5 先立形状：实现者声明身份 + 健康可查；后续按消费方需要扩展装载方法。
pub trait Source: Send + Sync {
    /// 源标识（观测 / 排障 / context-dump）
    fn name(&self) -> &str;
    /// 健康检查（DB 打得开并迁移到位 / 内存源恒真）
    fn ping(&self) -> Result<(), String>;
}

/// 内存源（测试 / 无盘环境兜底）。
pub struct MemorySource;

impl Source for MemorySource {
    fn name(&self) -> &str {
        "memory"
    }
    fn ping(&self) -> Result<(), String> {
        Ok(())
    }
}

/// domain 门面：DbSource 懒开（首次访问才建 `<PF数据目录>/context.db`）。
pub struct DomainStore {
    path: Option<PathBuf>,
    db: OnceLock<Result<DbSource, String>>,
    /// pfctx.log（与 settings 共享 Hub 级日志器；None = 纯内存态）
    logger: Option<std::sync::Arc<crate::log::Logger>>,
}

impl DomainStore {
    /// `path` = `<PF数据目录>`（Hub 注入；None = 无盘态，domain 不可用）。
    pub(crate) fn new(
        dir: Option<PathBuf>,
        logger: Option<std::sync::Arc<crate::log::Logger>>,
    ) -> Self {
        Self {
            path: dir,
            db: OnceLock::new(),
            logger,
        }
    }

    /// DB 源（懒开；打不开 = Err 缓存，不反复重试打盘）。开库留痕 pfctx.log。
    pub fn db(&self) -> Result<&DbSource, &str> {
        let slot = self.db.get_or_init(|| {
            let result = match &self.path {
                Some(dir) => DbSource::open(dir).map_err(|e| e),
                None => Err("无 PF 数据目录，domain 不可用（纯内存态）".to_string()),
            };
            if let Some(logger) = &self.logger {
                match &result {
                    Ok(db) => logger.log(
                        "domain",
                        &format!("{} ready file={}", db.name(), db.file().display()),
                    ),
                    Err(e) => logger.log("domain", &format!("开库失败: {e}")),
                }
            }
            result
        });
        slot.as_ref().map_err(|e| e.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::tmp_dir;

    #[test]
    fn db_opens_lazily_and_pings() {
        let dir = tmp_dir("domain-lazy");
        let store = DomainStore::new(Some(dir), None);
        let db = store.db().expect("temp 目录可建库");
        assert_eq!(db.name(), "sqlite(context.db)");
        assert!(db.ping().is_ok());
    }

    #[test]
    fn memory_source_pings() {
        assert!(MemorySource.ping().is_ok());
        assert_eq!(MemorySource.name(), "memory");
    }

    #[test]
    fn no_dir_is_unavailable_not_panic() {
        let store = DomainStore::new(None, None);
        assert!(store.db().is_err());
    }

    /// 架构断言（001-0 §4 规则 3）：settings 的 section 键与 DB 表名零交集
    /// ——settings 是人许的愿（文件域），域数据是发生过的事（DB 域），
    /// 同名 = 双 owner，禁止。
    #[test]
    fn settings_never_in_db() {
        let dir = tmp_dir("domain-arch");
        let db = DbSource::open(&dir).unwrap();
        for def in crate::settings::sections::registered() {
            assert!(
                !db.has_table(def.key),
                "settings section `{}` 不得成为 DB 表（001-0 §4 规则 3）",
                def.key
            );
        }
    }
}
