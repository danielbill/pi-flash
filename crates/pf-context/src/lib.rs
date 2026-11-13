//! 001 PFContext：settings / state / domain 三子集统一上下文（[001-0]）。
//!
//! 一句话：**settings 是人许的愿，state 是此刻的样子，domain 是发生过的事。**
//! 本 crate 解决的是「同一个值有几个 owner」与「改了之后谁在什么时候看见」
//! （001-0 §1 的 capacity 四处并存 bug），不是通用 DI / 服务定位器——只有
//! 三个具名子集，经 [`Hub`] 门面暴露：`hub.settings()` / `hub.state()` /
//! `hub.domain()`。
//!
//! 分层与优先级（001-0 §4，固定禁止加层）：读取 = L3 > L2 > L1 > L0 合并后
//! 的 typed snapshot，`get()` 零 I/O；每个 key 只有一个 primary source，其它
//! 层只做读覆盖永不回写；L4 env 只对 bootstrap 路径生效，永不传策略值。
//!
//! **禁止任何公开类型叫 `Context`**：`gpui::Context` 在本仓无处不在，需要
//! 局部别名时用 `PfHub`。
//!
//! # S1 切片范围（beads pi-flash-nndc.1）
//!
//! settings 子集最小闭环（Snapshot / 单写者 50ms coalesce / 原子写 / watcher）
//! + `subagents` section；`state/`、`domain/` 只建目录与 trait 形状（S4/S5
//! 填充）。不迁移其余 section（S2）、不做投影落盘（S3）、不碰 pi 的
//! models.json / auth.json / mcp.json（001-0 §6 边界）。
//!
//! 设计文档：`docs/模块设计/001-PFContext/001-0-PFContext总体设计.md`。

pub mod domain;
mod log;
pub mod settings;
pub mod state;

#[cfg(test)]
pub(crate) mod tests_support;

use std::sync::OnceLock;

/// 三子集统一上下文（门面，001-0 §3）。一个 `Arc` 生命周期，进程级单例。
pub struct Hub {
    /// 用户意图：app-settings.json 唯一 owner（S1 起通电）
    pub settings: settings::SettingsStore,
    /// 运行期状态（S4 最小集：当前会话/面板/子代理计数/窗口快照；内存为主，
    /// 不落盘不投影）
    pub state: state::StateStore,
    /// 域数据 SQLite WAL（S5：context.db，懒开；settings 永不进 DB）
    pub domain: domain::DomainStore,
}

static HUB: OnceLock<Hub> = OnceLock::new();

/// 进程内初始化（幂等）：路径取 pi-link 的 PF 数据目录，启动单写者线程 +
/// 外部改动监听。首次调用承担一次读盘（`[perf] settings.load` 打点，
/// `PI_FLASH_PERF=1` 门控）。**启动热路径只花这 µs 级一次读盘**（010 铁律）。
pub fn init() -> &'static Hub {
    hub()
}

/// 全局 Hub 句柄；未显式 [`init`] 时懒初始化（同路径、同行为）。
pub fn hub() -> &'static Hub {
    HUB.get_or_init(|| {
        let path = pi_link::paths::app_settings_file();
        let hub = Hub::open(path);
        hub.settings.start_watcher();
        hub
    })
}

pub use settings::sections;
pub use settings::{Layer, Origin, Section, SettingProblem};

/// pfctx.log 追加一行（宿主各模块观测留痕；Hub 未初始化 = no-op）。
/// tag 约定：`agent` = 宿主执行 agent 设置 op（001-4）。
pub fn log(tag: &str, msg: &str) {
    if let Some(hub) = HUB.get() {
        hub.settings.log_line(tag, msg);
    }
}

impl Hub {
    /// 指定 settings 文件路径构造（无文件监听）。测试与嵌入用；
    /// `None` = 纯内存态（set/flush 不落盘）。
    pub fn open(settings_path: Option<std::path::PathBuf>) -> Hub {
        // 写方职责（docs/paths.rs：写方负责 create_dir_all）——启动期显式建目录。
        if let Some(dir) = settings_path.as_ref().and_then(|p| p.parent()) {
            let _ = std::fs::create_dir_all(dir);
        }
        // pfctx.log：settings/domain 共享 Hub 级日志器（state 不落盘无日志点）
        let logger = std::sync::Arc::new(log::Logger::open(
            settings_path.as_ref().and_then(|p| p.parent()),
        ));
        Hub {
            settings: settings::SettingsStore::open_logged(
                settings_path.clone(),
                Some(logger.clone()),
            ),
            state: state::StateStore::default(),
            // domain DB 与 settings 同目录（<PF数据目录>/context.db），懒开
            domain: domain::DomainStore::new(
                settings_path
                    .as_ref()
                    .and_then(|p| p.parent().map(std::path::Path::to_path_buf)),
                Some(logger),
            ),
        }
    }
}

#[cfg(test)]
mod hub_tests {
    use crate::sections::Subagents;

    /// Hub::open 全装配（app `pf_context::init()` 同路径）：settings/domain
    /// 共享同一份 pfctx.log——boot 行一次、写盘行与投影行齐备、domain 懒开
    /// 留痕；日志落在数据目录（settings 文件同级）。
    #[test]
    fn hub_open_shares_one_log_and_records_lifecycle() {
        let dir = crate::tests_support::tmp_dir("hub-log");
        let settings = dir.join("app-settings.json");
        let hub = crate::Hub::open(Some(settings.clone()));

        hub.settings.set::<Subagents>(|s| s.capacity = 6);
        hub.settings.flush().unwrap();
        // domain 懒开（首次访问才建 context.db 并留痕）
        hub.domain.db().expect("temp 目录可建库");

        let text = std::fs::read_to_string(dir.join("pfctx.log")).unwrap();
        assert_eq!(
            text.matches("[boot]").count(),
            1,
            "settings/domain 共享一份日志器，boot 行只打一次：{text}"
        );
        assert!(text.contains("settings.load="), "{text}");
        assert!(text.contains("[write] subagents"), "{text}");
        assert!(text.contains("[export] ext-context.json"), "{text}");
        assert!(text.contains("[domain] sqlite(context.db)"), "{text}");
        assert!(dir.join("ext-context.json").is_file(), "投影落盘");
        assert!(dir.join("context.db").is_file(), "domain DB 落盘");
    }
}
