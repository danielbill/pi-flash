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

impl Hub {
    /// 指定 settings 文件路径构造（无文件监听）。测试与嵌入用；
    /// `None` = 纯内存态（set/flush 不落盘）。
    pub fn open(settings_path: Option<std::path::PathBuf>) -> Hub {
        Hub {
            settings: settings::SettingsStore::open(settings_path.clone()),
            state: state::StateStore::default(),
            // domain DB 与 settings 同目录（<PF数据目录>/context.db），懒开
            domain: domain::DomainStore::new(
                settings_path
                    .as_ref()
                    .and_then(|p| p.parent().map(std::path::Path::to_path_buf)),
            ),
        }
    }
}
