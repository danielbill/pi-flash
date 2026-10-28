//! 微信通话状态核心：**每个状态只有一个真身**，别的模块只读、只走这里的方法改。
//!
//! 历史包袱（2026-10-25 事故复盘，同批全清）：
//! - 服务活不活 = `boot_attempted` + `wants_start` + `transport` + `start_error`
//!   四个字段拼出来的隐式状态机（谁能起、谁不会再起，全靠注释）→ [`Service`]
//! - 链路错没错 = `start_error` / `poll_error` / `expired_rebind` 三条通道 →
//!   [`Health`]
//! - 会话指针 = 内存 `wx_session` + state 的 `session_key` + 过滤清单 path +
//!   `wx_session_name` 四处 → [`WxLine`]（落盘只剩 `session_key` 一个字段）
//! - UI 位置 = 标签真身在 `ws.panel_tabs` 的 `PanelTab::Wx`（开/关就
//!   是 tab 在不在，2026-10-26 归队后不再有独立 `tab_open` bool）
//! - state.json 每帧 `load()` 读盘解析（一次渲染最多 4 次 IO）→ [`Snapshot`]
//!   进程内镜像，泵一拍刷一次，渲染只读镜像

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

/// 启动失败的重试间隔：开机那次若撞上「轮询锁被另一个实例占着 / token 还没
/// 写进盘」，不该整个进程生命周期都不再试（旧实现 `boot_attempted` 一次性
/// 门就是这个毛病）。
const SERVICE_RETRY: Duration = Duration::from_secs(30);

/// 渠道生命周期。**句柄就在 `On` 里**——「在跑」= 句柄在，没有第二个 bool
/// 可以撒谎（旧的 `transport.is_some()` 与 `wants_start` 就常打架）。
pub(crate) enum Service {
    /// 待启动：泵下一拍起（开机首拍 / 用户开 / 换绑后重拉）
    Pending,
    /// 运行中。**句柄与它实际持有的 token 同行**——换绑检测只比这一个值，
    /// 不需要额外的 `active_token` 记账字段。
    On {
        transport: wxprobe::transport::Transport,
        token: String,
    },
    /// 用户显式关停：本进程不再自动拉起（扫码 / 开关 / 自动化可再开）
    Stopped,
    /// 启动失败：[`SERVICE_RETRY`] 后允许重试（详情只在 stderr，非产品状态）
    Failed { at: Instant },
}

impl Service {
    pub(crate) fn is_running(&self) -> bool {
        matches!(self, Service::On { .. })
    }

    pub(crate) fn transport(&self) -> Option<&wxprobe::transport::Transport> {
        match self {
            Service::On { transport, .. } => Some(transport),
            _ => None,
        }
    }

    /// 运行中渠道手里那份 token（换绑检测的唯一比对源）。
    pub(crate) fn started_token(&self) -> Option<&str> {
        match self {
            Service::On { token, .. } => Some(token),
            _ => None,
        }
    }

    /// 本拍该不该起渠道（唯一的「起不起」判据）。
    pub(crate) fn wants_start(&self, now: Instant) -> bool {
        match self {
            Service::Pending => true,
            Service::Failed { at, .. } => now.duration_since(*at) >= SERVICE_RETRY,
            Service::On { .. } | Service::Stopped => false,
        }
    }
}

/// 停服务时该挂哪条通知（纯规则，便于单测；由 [`Service`] 迁移点调用）。
///
/// 只有**用户显式关停**、且**已经宣告过「已开启」**才补发「已关闭」——凭空
/// 宣告关闭一个从未宣告开启的服务只会让人困惑；换绑落 `Pending` 时静默，
/// 紧接着的 `On` 会发「已开启」，不刷两条。
pub(crate) fn stop_notice(user_stop: bool, was_on: bool, announced: bool) -> Option<bool> {
    (user_stop && was_on && announced).then_some(false)
}

/// 链路健康度：只有「正常 / 服务端已作废本 bot 会话」两态。
/// （长轮询的偶发报错只进 stderr，不是产品状态——2026-10-25 定。）
pub(crate) enum Health {
    Ok,
    /// 服务侧已作废本 bot 会话（`-14` 洗完旧游标仍是它）：只能重新扫码
    Expired,
}

impl Health {
    pub(crate) fn expired(&self) -> bool {
        matches!(self, Health::Expired)
    }
}

/// 微信线（影子会话）指针。`key` = runtime key（草稿 `draft-N` 或会话文件
/// 路径）；`name` = `pf-wxbot-<15 位时间戳>`。
///
/// 落盘只有 state 的 `session_key` 一个字段；过滤清单（`hidden-sessions`）
/// 只在建线（[`crate::Chat::wx_new_session`]）与 FileBound
/// （`session::pool` 的 `on_file_bound`）各写一次。别的路径不许再各写一份
/// ——旧实现三处注入、清理不同步，靠「两级指针兜底」打补丁。
#[derive(Default, Clone)]
pub(crate) struct WxLine {
    pub key: Option<String>,
    pub name: Option<String>,
}

impl WxLine {
    pub(crate) fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    pub(crate) fn is(&self, key: &str) -> bool {
        self.key.as_deref() == Some(key)
    }
}

/// `state.json` 的进程内镜像。泵一拍 [`Snapshot::refresh`] 一次，渲染/listener
/// 只读镜像——渲染路径不再碰盘（旧实现 `page()` 每帧读盘，还在渲染里写盘）。
pub(crate) struct Snapshot {
    pub token: Option<String>,
    pub wx_cwd: Option<PathBuf>,
    /// `wx_cwd` 上锁：新建会话后置 true —— 进设置页**不得**拿 PF/桌面 cwd
    /// 自动赋值，必须用户手动选（002 §6）。用户选定项目后解锁。
    pub wx_cwd_lock: bool,
    pub session_key: Option<String>,
    pub chat_id: Option<String>,
    pub user_id: Option<String>,
    /// 盘上文件的 mtime：没变就不重读（`None` = 文件不存在，每拍重试探）
    mtime: Option<SystemTime>,
}

impl Snapshot {
    pub(crate) fn empty() -> Self {
        Self {
            token: None,
            wx_cwd: None,
            wx_cwd_lock: false,
            session_key: None,
            chat_id: None,
            user_id: None,
            mtime: None,
        }
    }

    /// 盘 → 镜像。`force` = 不看 mtime（刚写完盘时用）。
    /// 返回 true = 内容可能变了（调用方据此决定要不要重绘）。
    pub(crate) fn refresh(&mut self, force: bool) -> bool {
        let mtime = std::fs::metadata(wxprobe::state::state_path())
            .and_then(|m| m.modified())
            .ok();
        if !force && mtime.is_some() && mtime == self.mtime {
            return false;
        }
        self.mtime = mtime;
        let st = wxprobe::state::load();
        let s = |k: &str| wxprobe::state::get_str(&st, k);
        // 与 `wxprobe::transport::require_token` 同口径（空串 = 没 token）
        self.token = s("bot_token").filter(|t| !t.trim().is_empty());
        self.wx_cwd = s("wx_cwd").map(PathBuf::from);
        self.wx_cwd_lock = s("wx_cwd_lock").as_deref() == Some("1");
        self.session_key = s("session_key");
        self.chat_id = s("chat_id");
        self.user_id = s("user_id");
        true
    }

    pub(crate) fn has_token(&self) -> bool {
        self.token.is_some()
    }

    /// 对端投递路由就绪（`chat_id` / `user_id` 随首条入站消息回写）。
    pub(crate) fn routed(&self) -> bool {
        self.chat_id.is_some() || self.user_id.is_some()
    }
}
