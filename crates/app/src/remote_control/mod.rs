//! 微信 Bot 桥 —— 微信 ⇄ 会话（`docs/模块设计/060-远程控制-微信.md` §3、§4.1）。
//!
//! **正名（2026-10-22）**：本模块曾因早期调研误判被称作「远程控制」——把
//! ZCode 的 H5 工作台远控（扫码 → `/remote/v4` 页 + zcode relay 服务器透传）
//! 与它的微信 Bot 渠道（iLink 直连腾讯，无中间服务器）混为一谈。实核 ZCode
//! 源码后确认二者是两个独立功能；本模块对齐的是后者（Bot 渠道），故用户
//! 可见文案一律称「微信 Bot」。模块名 `remote_control` 与 i18n key 前缀
//! `remote.*` 系历史名保留（重命名纯属 diff 噪音，语义已由本注释锚定）。
//!
//! ```text
//! [出站] SessionEvent::ExtUi ──► extui::render ──► transport.send ──► 微信
//! [入站] 微信 ──► transport(Batch) ──► route ──► Command::{Prompt,Abort,ExtensionUiResponse}
//! ```
//!
//! 三块能力都在 `crates/wxprobe`（lib + bin，决策 7 修订），本模块只做**接线**：
//!
//! | 需求 | 用到 |
//! |---|---|
//! | 微信文本 → 动作 | `wxprobe::command::parse_bot_command` |
//! | ExtUi 请求 → 微信文本 / 回数 → 回填 | `wxprobe::extui::{render,parse}` |
//! | 长轮询收发 | `wxprobe::transport::{spawn,Batch}` |
//!
//! **会话归属（2026-10-23 定案）**：入站/出站投给**微信默认会话**
//! （`wx_session`，bot 语境独立于桌面选中态，对齐 ZCode「bot 自己的
//! activeTask」）。启用后第一拍认领当前活跃会话并持久化（state 的
//! `session_key`），重启后跟同一条线；`/新建` 给微信线开新草稿（不抢
//! 桌面焦点），`/task` 选会话改指向；目标被删回退活跃重新认领。

mod menu;
pub(crate) mod panel;
mod pipeline;
use std::path::PathBuf;

use pi_link::protocol::{Command, ExtensionUiRequest};

use wxprobe::command::BotCommand;
use wxprobe::extui::{self, ExtUiReply};
use wxprobe::format::messages::{Lang, t};

use crate::session::runtime::RemoteSend;

/// 启停通知（002 §4，W4）：每次启用/关停各一条。⚡ 顶替【pf icon】——
/// sendmessage 只走文本 item，图片要先上传拿 media id（同旧开机通知惯例）。
const WX_LINK_ON: &str = "⚡ Pi-Flash 通信已开启";
const WX_LINK_OFF: &str = "⚡ Pi-Flash 通信已关闭";
/// 档 1 尚未接入的命令回执 —— ZCode 文案表里的现成句子，不新造。
const CMD_NOT_ENABLED: &str = "当前 bot 未启用这个命令。";
/// 投递失败回执（wxprobe 文案表同名条目）。失败必须让微信端看见——
/// 静默丢弃正是「发消息没反应」屡查不出的原因。
const DELIVER_FAILED: &str = "消息未送达：目标会话没有可用的 pi 进程，稍后重试。";
/// 无项目/模型态回执（wxprobe 文案表同名条目）：放行条件（项目+模型）
/// 未满足时，需要会话的动作统一引导设置，不建会话、不投递（用户定夺）。
const NO_SETUP: &str = "请先设置项目和模型：回复 **/项目** 选择工作区、**/模型** 选择模型后再发消息。";

/// 运行沿判定（W4 启停通知的纯函数内核，便于单测）：
/// `Some(true)` = 升沿发「已开启」；`Some(false)` = 降沿发「已关闭」；
/// `None` = 无沿（平沿 / 首拍即停 / 换绑 force_restart 的计划性降沿）。
fn run_edge(prev: Option<bool>, running: bool, wants_start: bool) -> Option<bool> {
    match (prev, running) {
        // 首拍（prev=None）即 running 也算升沿：服务随窗口自启的渠道
        // 激活当下就能收到「已开启」
        (Some(false) | None, true) => Some(true),
        // 换绑 force_restart：wants_start 置位的降沿不发「已关闭」——
        // 紧接着的升沿发「已开启」，避免换绑刷两条
        (Some(true), false) if !wants_start => Some(false),
        _ => None,
    }
}
/// 绑定码无效的回执。ZCode 原句写的是「在 zcode UI 重新生成」—— 品牌换成
/// 本应用（2026-10-07 用户拍板，与 `/帮助` 标题同一决定），故**不走文案表**。
const BIND_INVALID: &[(&str, &str, &str)] = &[(
    "绑定码无效或已过期，请在 pi-flash 设置页重新生成。",
    "绑定码无效或已过期，请在 pi-flash 设置页重新生成。",
    "The bind code is invalid or expired. Generate a new one in the pi-flash settings page.",
)];

fn bind_invalid(lang: Lang) -> String {
    let e = &BIND_INVALID[0];
    match lang {
        Lang::En => e.2.to_string(),
        _ => e.0.to_string(),
    }
}

/// ZCode `taskRunning`。`/新建` 与 `/task`（列表、切换）共用 —— 同一个语义
/// 同一句话，ZCode 三处也都是 `msg(locale, "taskRunning")`。
pub(crate) const TASK_RUNNING: &str = "当前任务正在运行，稍后再试，或使用 **/停止** 停止当前任务。";

/// 一条入站文本解析出的动作，由 `Chat::run_wx_action` 执行。
#[derive(Debug)]
enum Action {
    /// 无事可做
    None,
    /// 回一句文本到微信
    Send(String),
    /// 普通对话
    Prompt(String),
    /// `/停止`
    Abort,
    /// `/状态` —— 需要会话状态，交给 `Chat` 现场组装
    Status,
    /// `/新建`（含 `/clear`）—— 需要 `Chat::new_session`
    New,
    /// 弹一个数字菜单（选项由 `Chat` 按 app 现场状态组装）；
    /// `sub` = 第二层菜单的上下文（`/模型` 选完供应商后带 provider）
    OpenMenu(menu::MenuKind, Option<String>),
    /// `/思考 <level>` 直接设置（不弹菜单）
    SetThink(String),
    /// `/bind <code>` —— 需要 Chat 比对并回执
    Bind(String),
    /// 菜单里选中了一项
    Select {
        kind: menu::MenuKind,
        sub: Option<String>,
        value: String,
    },
    /// ExtUi 回填（四字段与 `Command::ExtensionUiResponse` 一一对应）
    ExtUi {
        id: String,
        value: Option<String>,
        confirmed: Option<bool>,
        cancelled: bool,
    },
}

/// 正在等微信回数的阻塞型 ExtUi 请求。
struct Pending {
    id: String,
    method: pi_link::protocol::ExtUiMethod,
    /// 原始提示；回数解析不出来时原样重发一次
    prompt: String,
}

/// 设置页扫码面板的状态。
#[derive(Debug, Clone, PartialEq)]
pub enum QrState {
    Idle,
    Loading,
    /// `url` = `qrcode_img_content`，用 [`wxprobe::qr::qr_block_text`] 渲成可扫码字符画
    Ready {
        url: String,
    },
    /// 已扫码，等待手机确认
    Scanned,
    /// 成功：token 已写进状态文件
    Done {
        bot_id: Option<String>,
    },
    Expired,
    Error(String),
}

impl QrState {
    fn from_event(e: wxprobe::register::QrEvent) -> Self {
        use wxprobe::register::QrEvent as E;
        match e {
            E::Ready { url } => QrState::Ready { url },
            E::Scanned => QrState::Scanned,
            E::Success { bot_id } => QrState::Done { bot_id },
            E::Expired => QrState::Expired,
            E::Error(m) => QrState::Error(m),
        }
    }
}

/// 微信渠道句柄 + 待答请求。挂在 `Chat` 上，由 [`spawn_wx_pump`] 驱动。
pub struct RemoteControl {
    transport: Option<wxprobe::transport::Transport>,
    /// 一个 app 运行期内只试一次（P4 加开关与刷新）
    boot_attempted: bool,
    /// 活跃会话这会儿是否在跑（pump 每拍回填）。`route()` 没有 `cx`，
    /// 拿不到 runtime，所以在这儿留一份 —— `/task` 的两个守卫要读它。
    pub(crate) active_running: bool,
    pending: Option<Pending>,
    /// 当前 content index 的文本块（ZCode `assistantReplyBuffer` 的对应物）
    cur_ix: Option<usize>,
    cur_text: String,
    /// 一轮内已累积、待 force flush 的完整文本
    buf: String,
    /// 上一拍的 `agent_running`（下降沿 = 轮次结束）
    was_running: bool,
    /// 挂起中的数字选择菜单（060-c，对应 ZCode pendingSelectionsByContext）
    pending_menu: Option<menu::PendingMenu>,
    /// 设置页扫码面板状态
    pub qr: QrState,
    /// 扫码 worker 的回程（200ms 泵里 drain）
    qr_rx: Option<std::sync::mpsc::Receiver<wxprobe::register::QrEvent>>,
    /// 最近一次出码的 URL（`QrEvent::Ready` 的 `url`）。`QrState::Scanned`
    /// 不携带 url，页面靠它把二维码保持可见到确认完成；`cancel_qr` 清除。
    pub(crate) qr_url: Option<String>,
    /// `/绑定 <code>` 后钉住的会话 runtime key（§4.2 `active_session`）
    pub bound: Option<String>,
    /// **微信默认会话**（bot 语境，2026-10-23）：微信消息的固定归属，独立于
    /// 桌面选中态——否则 PF 一重启/一切会话，微信就不知道在跟谁说话。
    /// 首拍认领当前活跃会话并持久化到 state 的 `session_key`；`/新建` 换新线；
    /// `/task` 选会话改指向；目标被删则回退活跃。
    pub wx_session: Option<String>,
    /// 最近一个**非微信线**的活跃会话 key（switch_to 维护）：微信线是单开
    /// tab（2026-10-24 定夺），查看微信时 topbar 第一格（会话 tab）继续
    /// 显示并回到桌面会话，不被微信占用。
    pub desktop_key: Option<String>,
    /// 显式启用待执行标记（设置页开关 / `wx.enable` / 扫码成功置位，
    /// 泵下一拍统一执行 [`Self::start_transport_from_state`]）。执行在泵内
    /// 随「窗口激活」门控（2026-11-01），同时让测试可以断言意图而不真起线程。
    pub(crate) wants_start: bool,
    /// 渠道所用 bot token（`start_transport_from_state` 时记录）。扫码换绑
    /// 后与 state 现值不一致即触发强制重启——否则渠道拿着被服务端作废的
    /// 旧 token 干转（2026-10-23 真机事故，详见
    /// [`Self::force_restart_if_token_changed`]）。
    active_token: Option<String>,
    /// 最近一拍长轮询错误（泵每拍从 transport 同步；`None` = 通信正常）。
    /// 面板「运行中」下追加一行，治「状态撒谎」。
    pub(crate) poll_error: Option<String>,
    /// 微信服务侧会话过期（002）：检测到后自动回绑定页 + 二维码下标注。
    /// 重绑成功（QrState::Done）即清。
    pub(crate) expired_rebind: bool,
    /// 影子会话名（`pf-wxbot-<15 位时间戳>`，落盘后随 FileBound 一起记进
    /// 过滤清单）。
    pub(crate) wx_session_name: Option<String>,
    /// 渠道上一拍的运行态（启停通知的沿检测源，W4）。None = 本运行期首拍。
    run_edge_prev: Option<bool>,
    /// 待发的启停通知（W4）：`Some(true)` = 待发「已开启」，`Some(false)` =
    /// 待发「已关闭」。派发条件 = 对端路由就绪（`chat_id`/`user_id` 随
    /// 首条入站消息回写）——通知可以迟到，不能丢。
    run_notice_pending: Option<bool>,
    /// 微信模块**独立工作目录**（用户定夺，与桌面 `ws.cwd` 双向禁止联动）：
    /// 微信线影子会话一律建在 `wx_cwd` 下；首次进入设置页时从 PF 当前 cwd
    /// 继承一次，之后 cwd 变化不再赋值、wx_cwd 变化也不回写 cwd。
    pub(crate) wx_cwd: Option<PathBuf>,
    /// 「首进继承桌面 cwd」只许一次（002 §6）：[新建会话] 清空 wx_cwd 后
    /// 不得被设置页的「为空即继承」悄悄填回，否则更换工作目录失效。
    /// 内存态即可：重启归 false，恢复语义重新获得一次继承机会。
    pub(crate) wx_cwd_inherited: bool,
    /// 【微信通话】标签是否收起。**初值 true = 启动一律不出现**（2026-11-01
    /// 定夺）：它不是「要恢复的 tab」，而是点菜单（open_wx）才出现的功能
    /// 窗口；关掉即收起，重启一律没有——不落盘、不参与启动恢复，所以也
    /// 没有「关闭态要跨重启保持」这回事（那套 wxc 记账已删）。
    pub(crate) wx_tab_closed: bool,
    /// 最近一次渠道启动失败的错误（成功即清）。状态卡④「我方创建出错」
    /// 的数据源——扫码成功但渠道没起来的唯一可观察出口。
    pub(crate) start_error: Option<String>,
    /// 回复缓冲当前的来源会话 key。桌面活跃会话与微信线可能**并行流式**
    /// （出站门对两者都放行，2026-10-23），换来源时必须先把旧来源冲干净，
    /// 否则两个会话的回复会拼成一条发出去。
    buf_key: Option<String>,
    /// token 比对节流闸（泵 200ms 一拍，state 文件读不必每拍都做——
    /// Windows 实时杀软下偶发 IO 抖动直接打在 UI 线程上）。
    token_check_at: Option<std::time::Instant>,
}

impl RemoteControl {
    pub fn new() -> Self {
        // 独立工作目录持久化恢复（模块独立 + 重启后仍算「已设置」：否则
        // 每次重启 wx_cwd 归零，微信侧消息全被引导文案拦下）
        let wx_cwd = if cfg!(test) {
            None
        } else {
            wxprobe::state::get_str(&wxprobe::state::load(), "wx_cwd").map(PathBuf::from)
        };
        Self {
            transport: None,
            active_running: false,
            boot_attempted: false,
            pending: None,
            cur_ix: None,
            cur_text: String::new(),
            buf: String::new(),
            was_running: false,
            pending_menu: None,
            qr: QrState::Idle,
            qr_rx: None,
            qr_url: None,
            bound: None,
            wx_session: None,
            desktop_key: None,
            wants_start: false,
            active_token: None,
            poll_error: None,
            expired_rebind: false,
            wx_session_name: None,
            run_edge_prev: None,
            run_notice_pending: None,
            wx_cwd_inherited: wx_cwd.is_some(),
            wx_tab_closed: true,
            wx_cwd,
            start_error: None,
            buf_key: None,
            token_check_at: None,
        }
    }

    /// 服务随窗口（2026-11-01 定夺，取代 10-24「随应用存活」）：微信是
    /// 低频功能，收在菜单里——**桌面窗口没激活，渠道就不在后台收发**，
    /// 哪怕开关是 on（那是就绪态，不是运行态）。窗口激活（标签开）=
    /// 用户本轮明确要用，渠道才起；每轮只试一次，起不来不内卷
    ///（没扫码 / 锁被占只记日志，桌面端该干什么还干什么）。
    fn ensure_service(&mut self) {
        if self.boot_attempted || self.transport.is_some() {
            return;
        }
        self.boot_attempted = true;
        self.start_transport_from_state();
    }

    /// 显式启用/激活路径（设置页开关 / 扫码成功 / 窗口 ensure_service）。
    /// 无 SELF_START 门（2026-11-01 起「随应用自启」退役，渠道随窗口）；
    /// 修复史：曾因一刀切早退，扫码成功后轮询永不启动——弹窗正常关、
    /// 消息永远收不到。
    fn start_transport_from_state(&mut self) {
        self.start_error = None;
        let st = wxprobe::state::load();
        // 这里**不**恢复 state 的 `session_key`：续线归泵建线分支管
        //（真文件校验 + 过滤清单兜底，见 pump_wx），渠道起停不背会话归属
        let started = wxprobe::transport::config_from_state(&st).and_then(|cfg| {
            self.active_token = Some(cfg.token.clone());
            wxprobe::transport::spawn(&cfg)
        });
        match started {
            Ok(t) => {
                self.transport = Some(t);
                // 002 §5：会话只在**新建会话**时创建/绑定（用户指令），
                // 渠道连上不轮换、不动当前视图——启停没有跳转逻辑
            }
            // 静默失败是最坏的形态（§6 坑 4）：stderr 一行 + 状态卡④可显示
            Err(e) => {
                self.start_error = Some(format!("渠道未启动：{e}"));
                eprintln!("[wx] 渠道未启动：{e}");
            }
        }
    }

    /// 扫码换绑检测：运行中渠道的 token 与 state 现值不一致时停掉重拉。
    ///
    /// 2026-10-23 真机事故：渠道在扫码**前**被开关拉起（拿着即将作废的旧
    /// token），扫码成功后 pump 的 `wants_start && !is_running()` 见渠道还
    /// 活着就跳过——新 token 永远装不进去；旧 token 被新注册踢成
    /// `errcode:-14 session timeout`，长轮询死循环重试，消息一条进不来。
    /// 没在跑时直接返回（常规 `wants_start` 路径已覆盖）。
    pub(crate) fn force_restart_if_token_changed(&mut self) {
        if cfg!(test) {
            return; // 测试不起真渠道，也不碰真实 state 文件
        }
        if !self.is_running() {
            return;
        }
        // 节流：泵 200ms 一拍都同步读 state 文件太勤，1s 一次足够（换绑感知
        // 延迟人眼无感），杀软扫描的 IO 抖动也别想打上 UI 线程。
        let now = std::time::Instant::now();
        if self
            .token_check_at
            .is_some_and(|t| now.duration_since(t) < std::time::Duration::from_secs(1))
        {
            return;
        }
        self.token_check_at = Some(now);
        let st = wxprobe::state::load();
        let Some(new) = wxprobe::transport::require_token(&st).ok() else {
            return;
        };
        if self.active_token.as_deref() != Some(new.as_str()) {
            // Drop：join 线程 + 释放旧锁。坏 token 的请求即时失败，join 几乎
            // 不等；健康长轮询里最坏等一个 90s 超时（与开关「关」同代价）。
            self.transport = None;
            self.wants_start = true;
        }
    }

    /// 启停通知（002 §4，W4）：渠道运行沿检测，升/降沿各发一条到微信。
    /// 取代旧「每进程一条开机通知」——语义升格为「每次启停」。
    ///
    /// **通知可以迟到、不能丢**（2026-10-25 真机回归：升沿只发一次，路由
    /// 未就绪即永久丢）：沿只挂待发标记，派发要求路由就绪（`chat_id`/
    /// `user_id` 随**首条入站消息**回写）——未就绪每拍重试。
    ///
    /// 升沿派发：transport 出站队列入队即可，send 线程每次发送前重读状态。
    /// 降沿派发：transport 已 drop、send 线程 100ms 内退出——**另起后台
    /// 线程**用一次性 `Wire` 直发（15s 超时，失败只 stderr，绝不阻塞 UI
    /// 线程）；一次性不挂账（服务都没了不宜长期重试）。
    ///
    /// `wants_start` 置位（换绑 force_restart）时降沿不发「已关闭」——
    /// 紧接着的升沿发「已开启」，避免换绑刷两条；「已开启」还在待发就关
    /// 停的，同样不补发「已关闭」（凭空宣告关闭一个从未宣告开启的服务，
    /// 只会让人困惑）。
    ///
    /// 首拍（prev = None）即 running 也算升沿：服务随窗口自启的渠道在
    /// 激活当下就能收到「已开启」。
    fn sync_run_edges(&mut self) {
        if cfg!(test) {
            return; // 测试不起真渠道、不碰真实 state 文件
        }
        let running = self.transport.is_some();
        let prev = self.run_edge_prev.replace(running);
        if let Some(on) = run_edge(prev, running, self.wants_start) {
            if !on && self.run_notice_pending == Some(true) {
                self.run_notice_pending = None;
            } else {
                self.run_notice_pending = Some(on);
            }
        }
        // 待发派发：路由未就绪 → 留到下一拍重试
        let Some(on) = self.run_notice_pending else {
            return;
        };
        let st = wxprobe::state::load();
        let routed = wxprobe::state::get_str(&st, "chat_id").is_some()
            || wxprobe::state::get_str(&st, "user_id").is_some();
        if !routed {
            return;
        }
        let lang = Lang::from_ix(crate::i18n::lang_ix());
        match on {
            true => {
                let Some(tr) = &self.transport else {
                    return;
                };
                if tr.send(&t(lang, WX_LINK_ON)).is_ok() {
                    self.run_notice_pending = None;
                }
            }
            false => {
                self.run_notice_pending = None;
                let text = t(lang, WX_LINK_OFF);
                let spawned = std::thread::Builder::new()
                    .name("wx-stop-notice".into())
                    .spawn(move || {
                        let st = wxprobe::state::load();
                        let Ok(token) = wxprobe::transport::require_token(&st) else {
                            return;
                        };
                        let Some(to_user) = wxprobe::state::get_str(&st, "chat_id")
                            .or_else(|| wxprobe::state::get_str(&st, "user_id"))
                        else {
                            return;
                        };
                        let Some(from_user) = wxprobe::state::get_str(&st, "bot_user_id") else {
                            return;
                        };
                        let client_id = wxprobe::state::ensure_client_id(&mut st.clone());
                        let context = wxprobe::state::get_str(&st, "context_token");
                        let mut wire = wxprobe::wire::Wire::new(wxprobe::state::dump_dir());
                        if let Err(e) = wxprobe::poller::send_text(
                            &mut wire,
                            &token,
                            &from_user,
                            &to_user,
                            &text,
                            context.as_deref(),
                            &client_id,
                        ) {
                            eprintln!("[wx] 关停通知发送失败：{e}");
                        }
                    })
                    .is_ok();
                if !spawned {
                    eprintln!("[wx] 关停通知线程启动失败");
                }
            }
        }
    }

    /// 泵每拍同步最近轮询错误（面板显示）；渠道不在时为 `None`。
    ///
    /// `-14 session timeout` 洗完旧游标仍在 = token 本身失效（wxprobe 侧只
    /// 自愈一次）：这里**把渠道停掉**并亮「会话已过期」——继续长轮询只是
    /// 空转，而开关亮着会让人以为链路是通的（2026-10-24 真机）。
    /// 透传「最近一拍错误」供面板诊断行；并检测**服务侧会话过期**
    /// （002：清游标后空游标仍 -14 = 微信服务端已作废会话）。
    /// 返回 true = 刚检测到（跃迁沿，仅一次），调用方自动回绑定页。
    fn sync_poll_error(&mut self) -> bool {
        let dead = self.transport.as_ref().is_some_and(|t| t.session_dead());
        let transition = dead && !self.expired_rebind;
        if transition {
            self.expired_rebind = true;
        }
        self.poll_error = self.transport.as_ref().and_then(|t| t.poll_error());
        transition
    }

    /// 当前会话的绑定码（6 位数字）。由设置页展示、用户在微信里 `/bind` 回填。
    ///
    /// 用 runtime key 派生 —— 同一会话稳定、换会话即变；**不入状态文件**
    /// （draft 本来就是本运行期的，钉它没有意义）。
    pub fn bind_code(&self, active_key: &str) -> String {
        let mut h: u32 = 0x811c9dc5; // FNV-1a 32
        for b in active_key.as_bytes() {
            h ^= u32::from(*b);
            h = h.wrapping_mul(0x0100_0193);
        }
        format!("{:06}", (u64::from(h) % 1_000_000))
    }

    /// 校验并接受绑定码；成功时把 `active_key` 钉住（§4.2 `active_session`）。
    pub fn accept_bind(&mut self, code: &str, active_key: &str) -> bool {
        let want = self.bind_code(active_key);
        if code.trim() != want {
            return false;
        }
        self.bound = Some(active_key.to_string());
        true
    }

    /// 入站/出站的投递目标：**微信默认会话**优先（bot 语境，独立于桌面
    /// 选中态，对齐 ZCode「bot 自己的 activeTask」）；`/bind` 授权记录次之；
    /// 都没有（认领前）回落活跃会话。认领由泵第一拍完成（见
    /// [`Self::claim_default_session`]）。
    pub(crate) fn target_key(&self, active: &str) -> String {
        if let Some(k) = &self.wx_session {
            return k.clone();
        }
        if let Some(k) = &self.bound {
            return k.clone();
        }
        active.to_string()
    }

    /// 设置微信默认会话并持久化（state 的 `session_key`），重启后跟同一条线。
    pub(crate) fn set_wx_session(&mut self, key: impl Into<String>) {
        let k = key.into();
        self.wx_session = Some(k.clone());
        if cfg!(test) {
            return; // 测试不碰真实 state 文件（live 实例正持着它）
        }
        let mut st = wxprobe::state::load();
        wxprobe::state::set(&mut st, "session_key", Some(&k));
        wxprobe::state::save(&st);
    }

    /// 设置微信**独立工作目录**并持久化到状态文件（模块独立：不碰桌面
    /// `ws.cwd`，也不回写）。重启后自动恢复，`wx_cwd` + 模型齐即 ready。
    pub(crate) fn set_wx_cwd(&mut self, path: Option<PathBuf>) {
        self.wx_cwd = path.clone();
        if cfg!(test) {
            return; // 测试不碰真实 state 文件
        }
        let mut st = wxprobe::state::load();
        wxprobe::state::set(&mut st, "wx_cwd", path.as_deref().and_then(|p| p.to_str()));
        wxprobe::state::save(&st);
    }

    /// 认领语义锚点（**仅测试**）：wx_session 为空时置为给定 key。运行时的
    /// 归属创建已改为 pump 建专属草稿（2026-10-24 定夺：不再认领桌面活跃
    /// 会话），此处保留 set-if-none 语义供单测表达优先级规则。
    #[cfg(test)]
    pub(crate) fn claim_default_session(&mut self, active: &str) {
        if self.wx_session.is_some() {
            return;
        }
        self.set_wx_session(active.to_string());
    }

    /// 目标会话被删时清空微信线归属（下次投递回退活跃并重新认领）
    pub(crate) fn clear_wx_session_if(&mut self, key: &str) {
        if self.wx_session.as_deref() == Some(key) {
            self.wx_session = None;
            if cfg!(test) {
                return; // 同 set_wx_session
            }
            let mut st = wxprobe::state::load();
            wxprobe::state::set(&mut st, "session_key", None);
            wxprobe::state::save(&st);
        }
    }

    /// 开/关渠道。开 = 置 `wants_start`，泵下一拍统一走显式路径拉起
    /// （无 token 时起不来并打一行日志，不留死状态）；关 = `transport = None`
    /// 触发 `Drop` → **join 两条线程 + 释放轮询锁**（P4 验收：关停即停线程，
    /// 无悬挂）。
    pub fn set_enabled(&mut self, on: bool) {
        if on {
            // 显式启用走 wants_start → 泵下一拍（窗口激活时）
            // start_transport_from_state；boot_attempted 同步关掉自动路径
            self.boot_attempted = true;
            self.wants_start = true;
        } else {
            self.wants_start = false;
            self.transport = None;
            self.start_error = None;
            self.boot_attempted = true;
        }
    }

    /// 渠道是否活着（设置页开关的回显）。
    pub fn is_running(&self) -> bool {
        self.transport.is_some()
    }

    /// 发起扫码：专用线程跑 `begin → 3s 轮询`，结果由泵 drain。
    pub fn begin_qr(&mut self) {
        if matches!(
            self.qr,
            QrState::Loading | QrState::Ready { .. } | QrState::Scanned
        ) {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.qr_rx = Some(rx);
        self.qr = QrState::Loading;
        // 180s：二维码实际约 120s 有效，留出轮询余量
        std::thread::spawn(move || wxprobe::register::run_qr_flow(tx, 180));
    }

    /// 强制换一张新码（002 操作栏第 3 件「重新绑定」/ 码方框内的
    /// 【刷新二维码】）。与 [`Self::begin_qr`] 的幂等早退相反：手里那张码
    /// （Loading/Ready/Scanned）一律丢掉再起一轮，否则用户点「刷新」看到的
    /// 还是同一张旧码。被丢下的 worker 在下一次 `tx.send` 失败时自行退出
    ///（`wxprobe::register::run_qr_flow` 的取消约定）。
    pub fn refresh_qr(&mut self) {
        self.qr_rx = None;
        self.qr_url = None;
        self.qr = QrState::Idle;
        self.begin_qr();
    }

    /// 把扫码 worker 的事件抽干进 `self.qr`；有变化才返回 `true`（触发重绘）。
    pub fn poll_qr(&mut self) -> bool {
        let mut changed = false;
        if let Some(rx) = &self.qr_rx {
            while let Ok(ev) = rx.try_recv() {
                changed = true;
                if let wxprobe::register::QrEvent::Ready { url } = &ev {
                    self.qr_url = Some(url.clone());
                }
                self.qr = QrState::from_event(ev);
            }
        }
        changed
    }

    pub fn send(&self, text: &str) {
        if let Some(t) = &self.transport {
            let _ = t.send(text);
        }
    }

    /// 抽干入站队列（非阻塞）。整批返回，调用方处理完必须 [`Self::ack`]。
    fn take_batches(&mut self) -> Vec<wxprobe::transport::Batch> {
        let Some(t) = &self.transport else {
            return Vec::new();
        };
        let mut out = Vec::new();
        while let Ok(b) = t.try_recv() {
            out.push(b);
        }
        out
    }

    /// **处理动作之前**落路由字段——首条消息的回执（激活欢迎）在 route 时
    /// 就进了出站队列，send 线程随即读状态文件；对端没写好 = 「第一条没反应」
    /// （2026-10-24 真机）。
    fn ack_routing(&self, batch: &wxprobe::transport::Batch) {
        if let Some(t) = &self.transport {
            t.ack_routing(batch);
        }
    }

    /// 游标落盘（§6 坑 2：**处理完才写**）。
    fn ack_cursor(&self, batch: &wxprobe::transport::Batch) {
        if let Some(t) = &self.transport {
            t.ack_cursor(batch);
        }
    }

    /// ExtUi 请求 → 微信文本；阻塞型同时挂起 `pending` 等回数。
    ///
    /// 与桌面弹窗**并行**拿到同一请求（§4.1）：谁先应答算谁的，后到那份由
    /// [`Self::clear_pending`] 与 pi 侧的单次 resolve 一起丢弃。
    pub fn on_ext_ui(&mut self, req: &ExtensionUiRequest) {
        let lang = Lang::from_ix(crate::i18n::lang_ix());
        let Some(text) = extui::render(&req.method, lang) else {
            return;
        };
        if extui::is_blocking(&req.method) {
            self.pending = Some(Pending {
                id: req.id.clone(),
                method: req.method.clone(),
                prompt: text.clone(),
            });
        }
        self.send(&text);
    }

    /// 桌面端已应答 → 微信端放弃同一请求（§8 档 2「互不重复消费」）。
    pub fn clear_pending(&mut self, id: &str) {
        if self.pending.as_ref().is_some_and(|p| p.id == id) {
            self.pending = None;
        }
    }

    /// 微信入站文本 → 动作。**有待答请求时它优先**（与 ZCode 的
    /// pending selection 拦截同构）。
    fn route(&mut self, text: &str) -> Action {
        if let Some(p) = self.pending.take() {
            return match extui::parse(&p.method, text) {
                ExtUiReply::Select(v) => Action::ExtUi {
                    id: p.id,
                    value: Some(v),
                    confirmed: None,
                    cancelled: false,
                },
                ExtUiReply::Confirm(b) => Action::ExtUi {
                    id: p.id,
                    value: None,
                    confirmed: Some(b),
                    cancelled: false,
                },
                ExtUiReply::Text(s) => Action::ExtUi {
                    id: p.id,
                    value: Some(s),
                    confirmed: None,
                    cancelled: false,
                },
                ExtUiReply::Cancelled => Action::ExtUi {
                    id: p.id,
                    value: None,
                    confirmed: None,
                    cancelled: true,
                },
                ExtUiReply::Invalid => {
                    // 还没答上：留着 pending 并把提示原样重发一次
                    let prompt = p.prompt.clone();
                    self.pending = Some(p);
                    Action::Send(prompt)
                }
                ExtUiReply::NotApplicable => Action::None,
            };
        }

        let lang = Lang::from_ix(crate::i18n::lang_ix());
        // 数字菜单挂起时，回数优先被它吃掉（与 ExtUi pending 同序，
        // 对齐 ZCode pendingSelections 的拦截语义）
        if let Some(m) = self.pending_menu.take() {
            use menu::Outcome;
            return match m.resolve(text) {
                Outcome::Selected(value) => Action::Select {
                    kind: m.kind,
                    sub: m.sub.clone(),
                    value,
                },
                Outcome::Cancelled => Action::Send(t(lang, "已取消。")),
                Outcome::Retry => {
                    // 还没答上：菜单留着并把提示原样重发一次
                    let prompt = m.prompt.clone();
                    self.pending_menu = Some(m);
                    Action::Send(prompt)
                }
            };
        }

        match wxprobe::command::parse_bot_command(text) {
            BotCommand::Message { text: msg } => Action::Prompt(msg),
            BotCommand::Stop => Action::Abort,
            BotCommand::Help => Action::Send(pipeline::help_text(lang)),
            BotCommand::Status => Action::Status,
            BotCommand::New => Action::New,
            BotCommand::Bind { code } => Action::Bind(code),
            BotCommand::ThoughtLevelList => Action::OpenMenu(menu::MenuKind::Think, None),
            BotCommand::ThoughtLevelSet { value } => Action::SetThink(value),
            BotCommand::ModelList => Action::OpenMenu(menu::MenuKind::ModelProvider, None),
            // WeChat 侧只先展示供应商，模型放到下一层（ZCode 原 Bugfix）
            BotCommand::ModelProviderSet { value } => {
                Action::OpenMenu(menu::MenuKind::Model, Some(value))
            }
            // 直接给 id 时 provider 由 Chat 按模型清单反查
            BotCommand::ModelSet { value } => Action::Select {
                kind: menu::MenuKind::Model,
                sub: None,
                value,
            },
            // ZCode botsService.ts:5006 Bugfix：运行中连列表都不给 ——
            // 否则用户会继续点选其它 task，即使切换被拒也留下误导性 pending selection
            BotCommand::TaskList => {
                if self.active_running {
                    Action::Send(t(lang, TASK_RUNNING))
                } else {
                    Action::OpenMenu(menu::MenuKind::Task, None)
                }
            }
            BotCommand::WorkspaceList => Action::OpenMenu(menu::MenuKind::Project, None),
            // 直接给名字/路径，落地时由 Chat 反查
            BotCommand::WorkspaceSet { value } => Action::Select {
                kind: menu::MenuKind::Project,
                sub: None,
                value,
            },
            BotCommand::TaskSet { value } => Action::Select {
                kind: menu::MenuKind::Task,
                sub: None,
                value,
            },
            // `0` 在没有待答菜单时无意义，静默忽略（不打扰模型）
            BotCommand::SelectionCancel => Action::None,
            // 档 1 只接 Prompt/Abort；其余等 pipeline（P3 步骤 2-4）
            _ => Action::Send(t(lang, CMD_NOT_ENABLED)),
        }
    }
    // ── 出站：assistant 回复回推微信（060 档 1） ──────────────────────────

    /// 流式事件 → 待发文本。**返回的每一段都要调用方发出去**。
    ///
    /// `key` = 事件来源会话；与 [`Self::buf_key`] 不同源时先把旧来源冲干净
    /// （桌面活跃会话与微信线并行流式，回复各回各家）。
    ///
    /// flush 边界严格照 ZCode / §6 坑 1：
    /// * `TextDelta` 只累积，**不发**（provider chunk 常按词或子词到达）
    /// * `TextEnd` 用权威内容**覆盖**当前块（纠正丢 delta）
    /// * `ToolCallStart` → force flush（真正的发送边界之一）
    /// * 轮次结束由 [`Self::on_running`] 的下降沿触发
    pub fn on_assistant(
        &mut self,
        ev: &pi_link::protocol::AssistantEvent,
        key: &str,
    ) -> Vec<String> {
        use pi_link::protocol::AssistantEvent as E;
        let mut out = Vec::new();
        if self.buf_key.as_deref() != Some(key) {
            out.extend(self.flush());
            self.buf_key = Some(key.to_string());
        }
        match ev {
            E::TextDelta {
                content_index,
                delta,
            } => {
                if self.cur_ix != Some(*content_index) {
                    self.cur_ix = Some(*content_index);
                }
                self.cur_text.push_str(delta);
            }
            E::TextEnd {
                content_index,
                content,
            } => {
                self.cur_ix = Some(*content_index);
                // pi 冷启动横幅（「🔌 MCP: N server(s) enabled」）整条到达时
                // 直接内部消化，不进回复缓冲（2026-10-24 用户定夺）
                if is_startup_banner(content) {
                    self.cur_text.clear();
                } else {
                    self.cur_text = content.clone();
                }
            }
            E::ToolCallStart { .. } => out.extend(self.flush()),
            _ => {}
        }
        out
    }

    /// 轮次结束（`agent_running` 下降沿）→ 把剩下的发干净。
    ///
    /// 每拍都调用即可：`flush` 在缓冲为空时是 no-op，比只认下降沿更稳
    /// （200ms 采样可能整轮跨不过一次下降沿）。
    pub fn on_running(&mut self, running: bool) -> Vec<String> {
        self.was_running = running;
        if running { Vec::new() } else { self.flush() }
    }

    /// force flush：当前块并入缓冲 → 一次性切段发出。
    ///
    /// `extract_...(_, force=true)` = `split_long_reply_text`（>3500 字切段），
    /// 剩余缓冲恒空。
    fn flush(&mut self) -> Vec<String> {
        if !self.cur_text.is_empty() {
            self.buf.push_str(&self.cur_text);
            self.cur_text.clear();
        }
        // 横幅行兜底剔除：delta 分片拼进缓冲时按行再滤一遍
        self.buf = strip_startup_banner(&self.buf);
        if self.buf.trim().is_empty() {
            self.buf.clear();
            return Vec::new();
        }
        let buf = std::mem::take(&mut self.buf);
        let (msgs, rest) =
            wxprobe::format::reply::extract_bot_assistant_response_messages(&buf, true);
        self.buf = rest;
        msgs
    }
}

/// pi 冷启动横幅识别（如「🔌 MCP: 1 server enabled」）：会话冷启动的第一轮
/// 会把它混进 assistant 文本通道，对微信用户毫无意义——内部消化
/// （2026-10-24 用户定夺）。按内容特征识别，不锚定 emoji/ANSI（发送侧已剥）。
fn is_startup_banner(s: &str) -> bool {
    let low = s.to_lowercase();
    low.contains("mcp") && low.contains("server") && low.contains("enabled")
}

/// 缓冲按行剔除横幅（delta 分片与后续回复拼在同一行时无法分离——实测横幅
/// 以独立 TextEnd 到达，TextEnd 过滤是主路径，此处兜底整行场景）。
fn strip_startup_banner(buf: &str) -> String {
    buf.lines()
        .filter(|l| !is_startup_banner(l))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `/思考` 的 8 档 —— 与桌面 PillMenu 完全同一张表（key/label 一一对应）。
fn think_options() -> Vec<(String, String)> {
    [
        ("auto", "使用 pi 默认设置"),
        ("off", "关闭推理"),
        ("minimal", "最低限度推理"),
        ("low", "低强度推理"),
        ("medium", "中等强度推理"),
        ("high", "高强度推理"),
        ("xhigh", "超高强度推理"),
        ("max", "最高强度推理"),
    ]
    .iter()
    .map(|(k, d)| (crate::i18n::tr(d).to_string(), k.to_string()))
    .collect()
}

/// `/模型` 第一层：去重后的供应商列表（全局清单 ∪ 当前项目进程答案）。
/// 042【移除】墓碑同款过滤（与 catalog_for 一致，remote 菜单不列已移除商）。
fn provider_options(chat: &crate::Chat) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let live = chat
        .pool
        .models_by_cwd
        .get(&chat.project_key())
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    let removed = crate::services::workspace::removed_providers();
    for m in live.iter().chain(chat.pool.globals.models.iter()) {
        if removed.iter().any(|p| p == &m.provider) {
            continue;
        }
        if !out.iter().any(|(_, p)| p == &m.provider) {
            out.push((m.provider.clone(), m.provider.clone()));
        }
    }
    out
}

/// `/模型` 第二层：某供应商下的模型（入参来自第一层，已过移除墓碑）。
fn model_options(chat: &crate::Chat, provider: &str) -> Vec<(String, String)> {
    let live = chat
        .pool
        .models_by_cwd
        .get(&chat.project_key())
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    let mut out: Vec<(String, String)> = Vec::new();
    for m in live.iter().chain(chat.pool.globals.models.iter()) {
        if m.provider != provider || out.iter().any(|(_, id)| id == &m.id) {
            continue;
        }
        let label = if m.name.is_empty() {
            m.id.clone()
        } else {
            m.name.clone()
        };
        out.push((label, m.id.clone()));
    }
    out
}

/// `/项目`：工作区清单，标签 = 目录名，载荷 = 绝对路径。
fn project_options(chat: &crate::Chat) -> Vec<(String, String)> {
    chat.psp
        .projects
        .iter()
        .map(|p| (p.name.clone(), p.path.to_string_lossy().to_string()))
        .collect()
}

/// `/任务`：会话清单，标签优先 `name`，回落首条用户消息（与侧栏同一优先级）。
fn task_options(chat: &crate::Chat) -> Vec<(String, String)> {
    chat.pool
        .sessions
        .iter()
        .map(|s| {
            let label = s
                .name
                .clone()
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| {
                    let p = s.preview.replace('\n', " ");
                    if p.chars().count() > 40 {
                        let cut: String = p.chars().take(40).collect();
                        format!("{cut}...")
                    } else {
                        p
                    }
                });
            (label, s.id.clone())
        })
        .collect()
}

/// 200ms 轮询的微信入站泵（形态照 `startup::spawn_fs_watch_pump`：
/// executor 定时 + 主线程 update，后台线程的 HTTP 永不碰 GPUI）。
pub(crate) fn spawn_wx_pump(cx: &mut gpui::Context<crate::Chat>) {
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(200))
                .await;
            if this.update(cx, |chat, cx| chat.pump_wx(cx)).is_err() {
                break;
            }
        }
    })
    .detach();
}

impl crate::Chat {
    /// 微信线会话就位（幂等；2026-11-01 定夺：随标签休眠，不再无条件自启）。
    /// 无线时按序尝试：① `state.session_key` 指向的真会话文件 → 原线恢复；
    /// ② 过滤清单 owner 条目的 path（每次 FileBound 都钉着最后一条真会话，
    /// 兑底被旧进程覆盖死的草稿键）→ 同上；③ 都没有才新开专属草稿。
    /// detached 不抢桌面焦点，尾窗装载——微信端接着聊。
    /// 调用时机 = 标签打开（open_wx）/ 泵见标签开 / 首条入站（手机端启动
    /// 聊天，002）。独立工作目录 `wx_cwd` 未定时不建：入站由 run_wx_action
    /// 统一引导「请先设置项目和模型」。
    pub(crate) fn wx_ensure_session(&mut self, cx: &mut gpui::Context<Self>) {
        if self.aux.remote.wx_session.is_some() || self.aux.remote.wx_cwd.is_none() {
            return;
        }
        let cand = wxprobe::state::get_str(&wxprobe::state::load(), "session_key")
            .filter(|k| std::path::Path::new(k).is_file())
            .or_else(|| {
                pi_link::hidden_sessions::load()
                    .entries
                    .iter()
                    .find(|e| e.owner.as_deref() == Some(pi_link::hidden_sessions::OWNER_WX))
                    .and_then(|e| e.path.clone())
                    .filter(|p| std::path::Path::new(p).is_file())
            });
        let mut resumed = false;
        if let Some(p) = cand {
            let cwd = self.aux.remote.wx_cwd.clone().unwrap_or_default();
            if let Some(rt) = self.open_session_detached_with(std::path::PathBuf::from(&p), cwd, cx)
            {
                let key = rt.read(cx).key.clone();
                self.aux.remote.set_wx_session(key);
                resumed = true;
            }
        }
        if !resumed {
            if let Some(key) = self.new_draft_detached(cx) {
                self.aux.remote.set_wx_session(key);
            }
        }
    }

    /// 收一批 → 逐条解析并执行 → **整批处理完才 ack 游标**。
    pub(crate) fn pump_wx(&mut self, cx: &mut gpui::Context<Self>) {
        // 服务随窗口（2026-11-01 定夺，SELF_START 随应用自启退役）：窗口
        // 激活（标签开）渠道才在；关着 = 后台不收不发，哪怕开关 on（那是
        // 就绪态，不是运行态）。关停服务开关 / 标签中途关闭不在本门内：
        // 前者显式停，后者只收前台（本轮已激活则服务活到退出）。
        if !self.aux.remote.wx_tab_closed {
            self.aux.remote.ensure_service();
            // 显式启用（设置页开关 / wx.enable / 扫码成功）→ 泵这一拍统一
            // 执行；200ms 延迟不可感，换来测试可断言意图而不真起线程。
            // 取码仍是**按需**（2026-10-24 版式定夺）：信息栏【重制二维码】/
            // 未创建自动弹窗触发 `begin_qr`，泵不自动取码
            if self.aux.remote.wants_start && !self.aux.remote.is_running() {
                self.aux.remote.wants_start = false;
                self.aux.remote.start_transport_from_state();
            }
        }
        self.aux.remote.sync_run_edges();
        // 面板健康度：「运行中」背后可能正拿着死 token 干转。
        // 服务侧过期（002）→ **只置 expired_rebind 旗标，不动 UI**
        // （2026-11-01 用户定夺，修「每次启动标签都被顶回来」）：002 原文是
        // 「发送消息前检测到过期→回绑定页」，是模块使用中的行为；泵在启动
        // 几秒内就撞上 -14，此刻强行 refresh_qr + 重开标签 + 跳绑定页，
        // 等于每次启动都把用户关掉的标签掰开（低频功能不许抢焦点）。
        // 重绑引导改到 `open_wx`（菜单/标签点击 = 用户主动打开时）落地。
        self.aux.remote.sync_poll_error();
        // 微信线会话就位**不再无条件跑**（2026-11-01 定夺：不随标签休眠的
        // 特殊存在退役）：标签开 → 维持专线；标签关 → 休眠，唯独渠道仍在
        // 跑（本轮激活过、关标签不停服）且手机端来消息时唤醒接客
        //（002：微信聊天由手机端启动）。见 `wx_ensure_session`。
        let batches = self.aux.remote.take_batches();
        if !batches.is_empty() || !self.aux.remote.wx_tab_closed {
            self.wx_ensure_session(cx);
        }
        // Setup → Chat 自动放行已由 switch_project 的微信上下文钩子承担
        // （选完项目即回微信线），此处不再重复判
        // 060 档 1 回推：空闲/轮次结束时把 assistant 缓冲发干净。
        // 每拍都调，flush 在缓冲为空时是 no-op。
        // 运行态读**目标会话**（微信线 ≠ 桌面活跃时，/新建 /task 的守卫要看微信线的）
        let wx_target = self.aux.remote.target_key(&self.pool.active_key);
        let running = self
            .pool
            .runtimes
            .get(&wx_target)
            .map(|rt| rt.read(cx).agent_running)
            .unwrap_or(false);
        // 回填给 route() —— /task 的两个守卫要用（route 拿不到 cx）
        self.aux.remote.active_running = running;
        // 冲刷看**缓冲来源**的运行态：出站门放开后缓冲可能来自桌面活跃
        // 会话（轮内照常回推微信）。若仍按微信线判 idle，桌面流式中的文本
        // 会被 200ms 一拍地冲出去成碎片。
        let buf_running = self
            .aux
            .remote
            .buf_key
            .as_deref()
            .and_then(|k| self.pool.runtimes.get(k))
            .map(|rt| rt.read(cx).agent_running)
            .unwrap_or(running);
        for text in self.aux.remote.on_running(buf_running) {
            self.aux.remote.send(&text);
        }
        // 扫码结果（二维码在 Dialog::WxQr 弹窗内展示——泵只管事件回填、
        // 重绘与成功关窗）
        if self.aux.remote.poll_qr() {
            // 扫码成功 → 渠道起（wants_start 置位，本拍上方窗口激活分支
            // 统一执行）。若渠道已在跑且
            // 拿的是换绑前的旧 token，这里强制停掉、下一拍用新 token 重拉。
            if matches!(self.aux.remote.qr, QrState::Done { .. }) {
                // 扫到新 token = 会话重生：旧错与过期标注一并清掉
                self.aux.remote.poll_error = None;
                self.aux.remote.expired_rebind = false;
                self.aux.remote.force_restart_if_token_changed();
                self.aux.remote.wants_start = true;
                // 重绑成功 = 服务重新上线，**必发一条「已开启」**（用户定夺
                // 2026-10-25）：transport 是同一拍内停旧起新的「无缝替换」，
                // sync_run_edges 两拍之间看不到 running=false，自然升沿永远
                // 不出现——这里手工把上一拍压成「未运行」，下一拍必见升沿，
                // 走既有待发管道（路由未就绪则等首条入站，迟到不丢）
                self.aux.remote.run_edge_prev = Some(false);
                // 扫码成功 → 关二维码弹窗（重绑入口的收尾；首绑走功能页
                // Bind 段，本就无 Dialog）
                if matches!(self.dlg.dialog, Some(crate::Dialog::WxQr)) {
                    self.dlg.dialog = None;
                }
                // 首绑/重绑成功 → 功能页自动切下一段（Setup/Chat）
                if self.layout.content_view == crate::ContentView::Wx {
                    self.open_wx(cx);
                }
            }
            cx.notify();
        }
        let ensured = !batches.is_empty() || !self.aux.remote.wx_tab_closed;
        if batches.is_empty() {
            if ensured {
                cx.notify();
            }
            return;
        }
        for batch in &batches {
            // 路由字段先行：回执在 route 时就出队，send 线程马上读状态文件，
            // 对端没写好 = 首条回执被静默丢弃（「第一条没反应」根因）
            self.aux.remote.ack_routing(batch);
            for m in &batch.messages {
                let action = self.aux.remote.route(&m.text);
                self.run_wx_action(action, cx);
            }
            // 游标处理完才写（§6 坑 2）
            self.aux.remote.ack_cursor(batch);
        }
        cx.notify();
    }

    fn run_wx_action(&mut self, action: Action, cx: &mut gpui::Context<Self>) {
        // 放行门（002 §2，用户定夺：项目+模型必选）：条件未满足时不建会话、
        // 不投递，需要会话的动作（正文/状态/新建/停止）一律回引导文案。
        // 纯文案与菜单（/帮助 /项目 /模型 /绑定…）不在此列——/项目 /模型
        // 正是微信侧的自救入口，两项都设好后下一拍影子会话补建、自动放行。
        if !self.wx_chat_ready(cx)
            && matches!(
                action,
                Action::Prompt(_) | Action::Status | Action::New | Action::Abort
            )
        {
            let lang = Lang::from_ix(crate::i18n::lang_ix());
            self.aux.remote.send(&t(lang, NO_SETUP));
            return;
        }
        match action {
            Action::None => {}
            Action::Send(text) => self.aux.remote.send(&text),
            Action::Prompt(message) => self.send_wx(
                // 排队安全：behavior 交给 pi 原子判定（同 send_input）
                &Command::Prompt {
                    message,
                    images: Vec::new(),
                    streaming_behavior: Some("followUp".into()),
                },
                cx,
            ),
            // /停止与桌面停止按钮同口径：abort 必须配对 clear_queue 撤回滞留
            // 队列（pi 的 abort 只掐 run 不清队列，否则排队消息下轮照常冲出）
            Action::Abort => {
                self.send_wx(&Command::Abort, cx);
                self.send_wx(&Command::ClearQueue, cx);
            }
            Action::Status => {
                let lang = Lang::from_ix(crate::i18n::lang_ix());
                if let Some(text) = self.status_reply(cx, lang) {
                    self.aux.remote.send(&text);
                }
            }
            Action::New => {
                let lang = Lang::from_ix(crate::i18n::lang_ix());
                if self.agent_running_now(cx) {
                    // ZCode `case "new"`：任务运行中不让新建
                    self.aux.remote.send(&t(lang, TASK_RUNNING));
                } else {
                    // 微信 /新建 = 给 bot 语境开一条新线，**不抢桌面焦点**
                    //（微信线的 activeTask 独立于桌面选中态，对齐 ZCode）
                    if let Some(key) = self.new_draft_detached(cx) {
                        self.aux.remote.set_wx_session(key);
                    }
                    // 建完草稿回状态卡（`return createStatusReply(...)`）
                    if let Some(text) = self.status_reply(cx, lang) {
                        self.aux.remote.send(&text);
                    }
                }
            }
            Action::OpenMenu(kind, sub) => self.open_wx_menu(kind, sub, cx),
            Action::Bind(code) => {
                let lang = Lang::from_ix(crate::i18n::lang_ix());
                if self.aux.remote.accept_bind(&code, &self.pool.active_key) {
                    self.aux
                        .remote
                        .send(&t(lang, "绑定成功。发送 **/帮助** 查看可用命令。"));
                } else {
                    self.aux.remote.send(&bind_invalid(lang));
                }
            }
            Action::SetThink(level) => {
                self.set_thinking_level(&level, cx);
                self.reply_status(cx);
            }
            Action::Select { kind, sub, value } => self.apply_wx_selection(kind, sub, &value, cx),
            Action::ExtUi {
                id,
                value,
                confirmed,
                cancelled,
            } => self.send_wx(
                &Command::ExtensionUiResponse {
                    id,
                    value,
                    confirmed,
                    cancelled,
                },
                cx,
            ),
        }
    }

    /// 把命令投给**当前活跃会话**（1b 简化，见模块文档「会话归属」）。
    /// 没有 runtime / 进程未附着时静默丢弃 —— 微信侧拿不到回执，
    /// 但不会因为远程指令把桌面端搞崩。
    /// 现场组装一个菜单并挂起（060-c）。
    fn open_wx_menu(
        &mut self,
        kind: menu::MenuKind,
        sub: Option<String>,
        _cx: &mut gpui::Context<Self>,
    ) {
        let lang = Lang::from_ix(crate::i18n::lang_ix());
        let (title, options) = match kind {
            menu::MenuKind::Think => (
                crate::i18n::tr("remote.select_thinking").to_string(),
                think_options(),
            ),
            menu::MenuKind::ModelProvider => (
                crate::i18n::tr("remote.select_provider").to_string(),
                provider_options(self),
            ),
            menu::MenuKind::Model => {
                let p = sub.clone().unwrap_or_default();
                (
                    crate::i18n::tr("model.select_title").to_string(),
                    model_options(self, &p),
                )
            }
            menu::MenuKind::Task => (
                crate::i18n::tr("remote.switch_session").to_string(),
                task_options(self),
            ),
            menu::MenuKind::Project => (
                crate::i18n::tr("remote.switch_workspace").to_string(),
                project_options(self),
            ),
        };
        if options.is_empty() {
            // 没有可选项时给回执而不是发一张空菜单
            self.aux.remote.send(&t(lang, "未找到回复颗粒度。"));
            return;
        }
        let (prompt, pending) = menu::PendingMenu::build(kind, sub, &title, options, lang);
        self.aux.remote.pending_menu = Some(pending);
        self.aux.remote.send(&prompt);
    }

    /// 菜单选中后的落地动作。
    fn apply_wx_selection(
        &mut self,
        kind: menu::MenuKind,
        sub: Option<String>,
        value: &str,
        cx: &mut gpui::Context<Self>,
    ) {
        match kind {
            menu::MenuKind::Think => {
                self.set_thinking_level(value, cx);
                self.reply_status(cx);
            }
            menu::MenuKind::ModelProvider => {
                // 第一层选中供应商 → 直接弹第二层（不落地）
                self.open_wx_menu(menu::MenuKind::Model, Some(value.to_string()), cx);
            }
            menu::MenuKind::Model => {
                let provider = sub.clone().or_else(|| self.provider_of_model(value));
                if let Some(p) = provider {
                    self.rt()
                        .update(cx, |r, cx| r.select_model(p, value.to_string(), cx));
                }
                // 手机端 /模型 选完也走设置落地（项目+模型齐即建会话）
                self.wx_after_setup_change(cx);
                self.reply_status(cx);
            }
            menu::MenuKind::Project => {
                // 菜单载荷就是路径；直接 `/项目 <名字>` 时按名字反查。
                // 微信选项目 = 写独立工作目录 `wx_cwd`（002 模块独立：不碰
                // 桌面 cwd、不切桌面视图），轮换微信线到新目录
                let path = self
                    .psp
                    .projects
                    .iter()
                    .find(|p| p.path.to_string_lossy() == value || p.name == value)
                    .map(|p| p.path.clone());
                if let Some(p) = path {
                    self.wx_pick_project(p, cx);
                }
                self.reply_status(cx);
            }
            menu::MenuKind::Task => {
                // ZCode botsService.ts:6039 Bugfix：旧 task 在跑时改 activeTaskId
                // 会让后续输入落到新 task，但旧 task 输出仍回同一 bot 会话 ——
                // 用户会误以为消息串线。运行中一律拒绝切换。
                if self.aux.remote.active_running {
                    self.aux
                        .remote
                        .send(&t(Lang::from_ix(crate::i18n::lang_ix()), TASK_RUNNING));
                    return;
                }
                if let Some(info) = self.pool.sessions.iter().find(|s| s.id == value) {
                    let path_key = info.path.to_string_lossy().to_string();
                    if self.pool.runtimes.contains_key(&path_key) {
                        // 已开着 → 只改微信线归属，桌面不动（bot 语境独立）
                        self.aux.remote.set_wx_session(path_key);
                    } else {
                        // 没开着 → 打开即活跃（现存唯一无焦打开路径），
                        // 微信线跟到新活跃
                        let path = info.path.clone();
                        self.new_session_in(path, cx);
                        self.aux.remote.set_wx_session(self.pool.active_key.clone());
                    }
                }
                self.reply_status(cx);
            }
        }
    }

    /// 选完 / 设置完回一张状态卡（ZCode 各选择分支的 `createStatusReply`）。
    fn reply_status(&self, cx: &gpui::App) {
        let lang = Lang::from_ix(crate::i18n::lang_ix());
        if let Some(text) = self.status_reply(cx, lang) {
            self.aux.remote.send(&text);
        }
    }

    /// 直接给模型 id 时反查其供应商（`/模型 <id>` 不带 provider 前缀）。
    /// 移除墓碑过滤：已移除商的模型 id 不再可被指定。
    fn provider_of_model(&self, id: &str) -> Option<String> {
        let live = self
            .pool
            .models_by_cwd
            .get(&self.project_key())
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        let removed = crate::services::workspace::removed_providers();
        live.iter()
            .chain(self.pool.globals.models.iter())
            .filter(|m| !removed.iter().any(|p| p == &m.provider))
            .find(|m| m.id == id)
            .map(|m| m.provider.clone())
    }

    fn send_wx(&mut self, cmd: &Command, cx: &mut gpui::Context<Self>) {
        // 微信默认会话优先（bot 语境），/bind 记录次之，认领前回落活跃
        let key = self.aux.remote.target_key(&self.pool.active_key);
        // 目标只是「没打开」（会话文件还在盘上）→ **后台唤醒**再投递，不动
        // 桌面选中态。2026-10-23 真机：重启后微信线的会话没有 runtime，
        // 投递全靠回退活跃兜底——微信跟谁说话被桌面牵着漂移。
        let mut rt = match self.pool.runtimes.get(&key).cloned() {
            Some(rt) => Some(rt),
            None if std::path::Path::new(&key).is_file() => {
                self.open_session_detached(std::path::PathBuf::from(&key), cx)
            }
            None => None,
        };
        let rt = match rt.take() {
            Some(rt) => rt,
            None => {
                // 会话真被删了 → 清归属后本条按活跃会话兜底投递（target_key
                // 回退），专属草稿由泵下一拍重建。静默丢弃已经坑过一次
                // （微信没回执），这里必须自愈。
                self.aux.remote.clear_wx_session_if(&key);
                let fallback = self.aux.remote.target_key(&self.pool.active_key);
                match self.pool.runtimes.get(&fallback).cloned() {
                    Some(rt) => rt,
                    None => {
                        eprintln!("[wx] 命令未送达：没有 key = {key} 的会话，且无活跃会话可兜底");
                        self.deliver_failed_reply();
                        return;
                    }
                }
            }
        };
        let sent = rt.update(cx, |r, cx| match cmd {
            // 文本要走「没进程先 spawn」，否则草稿会话永远收不到
            Command::Prompt { message, .. } => Some(r.send_remote_text(message.clone(), cx)),
            _ => r.send_remote_command(cmd, cx).then_some(RemoteSend::Sent),
        });
        if matches!(sent, Some(RemoteSend::Failed) | None) {
            eprintln!("[wx] 命令未送达（目标会话没有 pi 进程或正在压缩）");
            self.deliver_failed_reply();
        }
        // Queued = 已入队等当前轮结束，正常路径不发回执（避免刷屏）
    }

    /// 投递失败的微信端回执——静默丢弃正是「发消息没反应」屡查不出的原因。
    fn deliver_failed_reply(&mut self) {
        let lang = Lang::from_ix(crate::i18n::lang_ix());
        self.aux.remote.send(&t(lang, DELIVER_FAILED));
    }
    /// 当前活跃会话是否在跑（`/新建` 的运行中分支与状态卡共用）。
    fn agent_running_now(&self, cx: &gpui::App) -> bool {
        // 读微信线的目标会话（≠ 桌面活跃时，/新建 的守卫要看微信线的）
        let key = self.aux.remote.target_key(&self.pool.active_key);
        self.pool
            .runtimes
            .get(&key)
            .map(|rt| rt.read(cx).agent_running)
            .unwrap_or(false)
    }

    /// 微信设置会话上下文：功能页管理段（Wx）或活跃会话即微信线。项目
    /// 选择在此上下文一律写独立工作目录 `wx_cwd`（禁止联动桌面 cwd）。
    pub(crate) fn wx_setup_ctx(&self) -> bool {
        self.layout.content_view == crate::ContentView::Wx
            || self.aux.remote.wx_session.as_deref() == Some(self.pool.active_key.as_str())
    }

    /// 微信聊天放行条件（002 §2，用户定夺：项目和模型都是必选项）：
    /// 微信独立工作目录已定 + 微信线目标会话模型已定。任一不满足，正文/
    /// 状态/新建/停止一律引导「请先设置项目和模型」，不建会话、不投递。
    fn wx_chat_ready(&self, cx: &gpui::App) -> bool {
        if self.aux.remote.wx_cwd.is_none() {
            return false;
        }
        let key = self.aux.remote.target_key(&self.pool.active_key);
        self.pool
            .runtimes
            .get(&key)
            .map(|rt| rt.read(cx).has_model())
            .unwrap_or(false)
    }

    /// 设置页就绪落地（002 §2 用户定夺）：`wx_cwd` + 模型齐 → **立即**建
    /// `wx_session`（ready for chat，手机端即可发起对话——微信聊天由手机
    /// 端启动，不能等桌面发消息来建会话），并自动跳到微信会话页。
    ///
    /// 由「选项目」「选模型」两条路径共同触发，两者先后顺序都能自愈：
    /// - 先项目后模型：选模型落在微信线上 → 此处判 ready → 建/跳；
    /// - 先模型后项目：选项目时 `wx_pick_project` 既轮换会话又承接模型。
    ///
    /// 只在设置页上下文生效（`wx_setup_ctx`），聊天页/桌面不受影响。
    pub(crate) fn wx_after_setup_change(&mut self, cx: &mut gpui::Context<Self>) {
        if !self.wx_setup_ctx() {
            return;
        }
        if !self.wx_chat_ready(cx) {
            return;
        }
        // open_wx 内部：wx_session 为空则新建，然后激活会话并落到 Chat 视图
        self.open_wx(cx);
    }

    /// 微信线目标会话 → `/状态` 卡片；没有 runtime 时返回 `None`。
    fn status_reply(&self, cx: &gpui::App, lang: Lang) -> Option<String> {
        let key = self.aux.remote.target_key(&self.pool.active_key);
        let rt = self.pool.runtimes.get(&key)?;
        let r = rt.read(cx);
        let workspace = r
            .cwd
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| r.cwd.to_string_lossy().to_string());
        let st = r.state.as_ref();
        // 任务名：pi 的 session_name 优先；草稿期用固定标题，否则回落 runtime 状态串
        let title = st.and_then(|s| s.session_name.clone()).unwrap_or_else(|| {
            if r.key.starts_with("draft-") {
                "新任务草稿".to_string()
            } else {
                r.status.clone()
            }
        });
        let id = st
            .and_then(|s| s.session_file.as_ref())
            .and_then(|f| {
                std::path::Path::new(f)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
            })
            .unwrap_or_else(|| r.key.clone());
        // ZCode 词表里没有 idle/draft 之外的「空闲」态，这里按会话形态映射
        let state = if r.agent_running {
            "running"
        } else if r.key.starts_with("draft-") {
            "draft"
        } else {
            "completed"
        };
        let elapsed_ms = r
            .agent_running
            .then(|| r.stream_started.map(|i| i.elapsed().as_millis() as u64))
            .flatten();
        Some(pipeline::status_text(
            lang,
            &pipeline::StatusInput {
                workspace,
                model: r.model_label_text(),
                task: Some((title, id)),
                state,
                elapsed_ms,
                progress: if r.agent_running {
                    r.status.clone()
                } else {
                    String::new()
                },
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 档 1 的命令覆盖面：只有 /停止 与纯文本真正接线，
    /// 其余一律回 ZCode 现成的「未启用」句，**不静默吞掉**。
    /// 测试里直接塞一个挂起菜单（真实路径走 `Chat::open_wx_menu`）。
    fn set_pending(rc: &mut RemoteControl, m: menu::PendingMenu) {
        rc.pending_menu = Some(m);
    }

    #[test]
    fn tier1_commands_are_all_wired() {
        let mut rc = RemoteControl::new();
        // 普通消息直接进模型（2026-10-24 定夺：撤自动欢迎+帮助菜单）
        assert!(matches!(rc.route("你好"), Action::Prompt(_)));
        assert!(matches!(rc.route("帮我看看这个报错"), Action::Prompt(_)));
        assert!(matches!(rc.route("/停止"), Action::Abort));
        assert!(matches!(rc.route("/stop"), Action::Abort));
        assert!(matches!(rc.route("0"), Action::None));
        // 档 1 三条命令（含英文别名）
        assert!(matches!(rc.route("/帮助"), Action::Send(_)));
        assert!(matches!(rc.route("/help"), Action::Send(_)));
        assert!(matches!(rc.route("/状态"), Action::Status));
        assert!(matches!(rc.route("/status"), Action::Status));
        assert!(matches!(rc.route("/新建"), Action::New));
        assert!(matches!(rc.route("/clear"), Action::New));
        // 档 2 的菜单命令
        assert!(matches!(
            rc.route("/思考"),
            Action::OpenMenu(menu::MenuKind::Think, None)
        ));
        assert!(matches!(
            rc.route("/thoughtLevel"),
            Action::OpenMenu(menu::MenuKind::Think, None)
        ));
        assert!(matches!(rc.route("/思考 low"), Action::SetThink(v) if v == "low"));
        assert!(matches!(
            rc.route("/模型"),
            Action::OpenMenu(menu::MenuKind::ModelProvider, None)
        ));
        // ZCode 的任务命令名是 `/task`（`/任务` 是 Unknown）
        assert!(matches!(
            rc.route("/task"),
            Action::OpenMenu(menu::MenuKind::Task, None)
        ));
        assert!(matches!(
            rc.route("/项目"),
            Action::OpenMenu(menu::MenuKind::Project, None)
        ));
        assert!(matches!(
            rc.route("/workspace"),
            Action::OpenMenu(menu::MenuKind::Project, None)
        ));
        // 仍未接的命令仍要回执，不能静默吞掉（/模式 /回复 是 ZCode 专有概念，
        // pi-flash 无对应产品语义）
        match rc.route("/模式") {
            Action::Send(s) => assert_eq!(s, "当前 bot 未启用这个命令。"),
            other => panic!("未接入的命令必须回执，不能静默：{other:?}"),
        }
    }

    #[test]
    fn pending_menu_intercepts_numbers_before_the_model() {
        use menu::{MenuKind, Outcome, PendingMenu};
        let mut rc = RemoteControl::new();
        let (_, m) = PendingMenu::build(
            MenuKind::Think,
            None,
            "选择思考档位",
            vec![("低".into(), "low".into()), ("高".into(), "high".into())],
            Lang::ZhCn,
        );
        set_pending(&mut rc, m);

        // 选中 → 清菜单并给出载荷
        assert!(matches!(rc.route("1"), Action::Select { value, .. } if value == "low"));
        assert!(rc.pending_menu.is_none(), "选中后必须清掉");

        // 挂新菜单：0 = 取消、越界 = 重发提示并保留
        let (_, m2) = PendingMenu::build(
            MenuKind::Task,
            None,
            "t",
            vec![("a".into(), "id".into())],
            Lang::ZhCn,
        );
        set_pending(&mut rc, m2);
        assert!(matches!(rc.route("0"), Action::Send(s) if s == "已取消。"));
        assert!(rc.pending_menu.is_none(), "取消也要清掉");

        let (_, m3) = PendingMenu::build(
            MenuKind::Task,
            None,
            "再试",
            vec![("a".into(), "id".into())],
            Lang::ZhCn,
        );
        let want = m3.prompt.clone();
        set_pending(&mut rc, m3);
        assert!(matches!(rc.route("9"), Action::Send(s) if s == want));
        assert!(rc.pending_menu.is_some(), "解析失败要保留菜单");
        let _ = Outcome::Cancelled;
    }

    #[test]
    fn set_enabled_toggles_and_drops_transport() {
        // P4 验收：关停即停线程 —— transport 置 None 触发 Drop(join)。
        // 开 = 走显式路径（wants_start → 泵在窗口激活时拉起；无 token 时
        // 起不来，但不再依赖开机自启路径）。
        let mut rc = RemoteControl::new();
        assert!(!rc.is_running(), "默认未启动");
        rc.set_enabled(true);
        assert!(rc.boot_attempted, "显式开启即关掉泵的自动路径");
        assert!(
            !rc.is_running(),
            "无 token 起不来（config_from_state 报错打日志）"
        );
        rc.set_enabled(false);
        assert!(!rc.is_running());
        assert!(rc.boot_attempted, "关停后必须禁止泵自动重开");
    }

    #[test]
    fn bind_code_is_stable_six_digits_and_session_scoped() {
        let rc = RemoteControl::new();
        let a = rc.bind_code("draft-1");
        assert_eq!(a.len(), 6, "6 位");
        assert!(
            a.chars().all(|c| c.is_ascii_digit()),
            "纯数字，避免大小写歧义"
        );
        assert_eq!(a, rc.bind_code("draft-1"), "同会话稳定");
        assert_ne!(a, rc.bind_code("draft-2"), "不同会话必须不同");
    }

    #[test]
    fn bind_accepts_match_rejects_others_and_pins_target() {
        let mut rc = RemoteControl::new();
        let code = rc.bind_code("draft-7");
        assert!(
            !rc.accept_bind("000000", "draft-7") || code == "000000",
            "错码不绑"
        );
        assert!(rc.bound.is_none());

        assert!(rc.accept_bind(&code, "draft-7"));
        assert_eq!(rc.bound.as_deref(), Some("draft-7"), "授权记录要留着");
    }

    #[test]
    fn routing_prefers_wx_session_then_bound_then_active() {
        // 微信默认会话（bot 语境）优先 —— 独立于桌面选中态；
        // /bind 记录次之；都没有 → 活跃会话（认领前的兑底）。
        let mut rc = RemoteControl::new();
        assert_eq!(rc.target_key("draft-9"), "draft-9");

        // 认领后固定指向微信线，桌面切换不再牵动
        rc.claim_default_session("draft-9");
        assert_eq!(rc.target_key("draft-11"), "draft-9", "认领后不跟桌面活跃");

        // /bind 是授权记录，不抢占微信线的路由地位（ZCode：绑 workspace，
        // 选会话走 /task）；此处仅验证优先级次序
        rc.wx_session = None;
        let code = rc.bind_code("draft-7");
        assert!(rc.accept_bind(&code, "draft-7"));
        assert_eq!(rc.target_key("draft-11"), "draft-7");
    }

    #[test]
    fn clear_wx_session_only_clears_the_wx_line() {
        let mut rc = RemoteControl::new();
        rc.claim_default_session("draft-9");
        assert!(rc.wx_session.is_some());
        // 删的不是微信线 → 不动
        rc.clear_wx_session_if("draft-5");
        assert!(rc.wx_session.is_some());
        // 删的正是微信线 → 清空，下拍重新认领
        rc.clear_wx_session_if("draft-9");
        assert!(rc.wx_session.is_none());
        rc.claim_default_session("draft-11");
        assert_eq!(rc.target_key("draft-11"), "draft-11");
    }

    #[test]
    fn help_reply_has_title_and_all_lines() {
        let mut rc = RemoteControl::new();
        match rc.route("/帮助") {
            Action::Send(s) => {
                assert!(s.starts_with(&pipeline::help_title(Lang::ZhCn)));
                assert_eq!(s.lines().count(), 10, "标题 + 9 条");
            }
            other => panic!("{other:?}"),
        }
    }

    /// 没有 pending 时，ExtUi 回填不该被触发。
    #[test]
    fn no_pending_means_plain_command_routing() {
        let mut rc = RemoteControl::new();
        assert!(rc.pending.is_none());
        assert!(matches!(rc.route("1"), Action::Prompt(_)));
    }

    // ── 普通消息不再有自动欢迎/帮助菜单（2026-10-24 所有者定夺）─────

    /// 之前每启动一次都会在首条消息上推一整份命令菜单（用户实测「每次
    /// 打开都弹」）——现在普通消息一律直接进模型，菜单只由 `/帮助` 拉。
    #[test]
    fn first_plain_message_goes_straight_to_the_model() {
        let mut rc = RemoteControl::new();
        assert!(matches!(rc.route("你好"), Action::Prompt(_)));
        assert!(matches!(rc.route("/帮助"), Action::Send(_)));
    }

    /// 活跃会话在跑时 `/task` 既不给列表也不许切换（ZCode 两处 Bugfix）。
    #[test]
    fn task_menu_is_refused_while_running() {
        let mut rc = RemoteControl::new();
        rc.active_running = true;
        match rc.route("/task") {
            Action::Send(t) => assert!(t.contains("正在运行"), "应拒，实际: {t}"),
            other => panic!("运行中 /task 应直接拒绝，实际 {other:?}"),
        }
        rc.active_running = false;
        assert!(matches!(
            rc.route("/task"),
            Action::OpenMenu(menu::MenuKind::Task, None)
        ));
    }

    // ── 060 档 1：assistant 回复回推的缓冲/flush 语义 ──────────────────

    use pi_link::protocol::AssistantEvent as AE;

    fn td(ix: usize, s: &str) -> AE {
        AE::TextDelta {
            content_index: ix,
            delta: s.to_string(),
        }
    }
    fn te(ix: usize, s: &str) -> AE {
        AE::TextEnd {
            content_index: ix,
            content: s.to_string(),
        }
    }
    fn tool_start() -> AE {
        AE::ToolCallStart {
            content_index: 1,
            id: "tc_1".into(),
            tool_name: "bash".into(),
        }
    }

    #[test]
    fn deltas_accumulate_and_send_nothing() {
        // §6 坑1：provider chunk 常按词或子词到达，非终态必须留在缓冲里
        let mut rc = RemoteControl::new();
        assert!(rc.on_assistant(&td(0, "你好"), "s").is_empty());
        assert!(rc.on_assistant(&td(0, "，我是"), "s").is_empty());
        assert!(rc.on_assistant(&td(0, "助手"), "s").is_empty());
        assert_eq!(rc.cur_text, "你好，我是助手");
        assert!(rc.buf.is_empty());
    }

    #[test]
    fn text_end_overrides_accumulated_deltas() {
        // TextEnd 是权威内容，用来纠正丢 delta；但不能清掉上一块
        let mut rc = RemoteControl::new();
        rc.on_assistant(&td(0, "块一"), "s");
        rc.on_assistant(&te(0, "块一（修正）"), "s");
        assert_eq!(rc.cur_text, "块一（修正）");
        // 下一块的 delta 累加在后面，不被 TextEnd 抹掉
        rc.on_assistant(&td(1, "块二"), "s");
        assert_eq!(rc.cur_text, "块一（修正）块二");
    }

    #[test]
    fn tool_call_start_force_flushes() {
        // 真正的发送边界之一 = tool_call（§6 坑1）
        let mut rc = RemoteControl::new();
        rc.on_assistant(&td(0, "先跑个命令"), "s");
        let out = rc.on_assistant(&tool_start(), "s");
        assert_eq!(out, vec!["先跑个命令".to_string()]);
        assert!(rc.cur_text.is_empty());
        assert!(rc.buf.is_empty(), "flush 后缓冲必须清空");
    }

    #[test]
    fn source_switch_flushes_previous_buffer() {
        // 双源防串线（2026-10-23 出站门放开后）：桌面活跃会话与微信线可能
        // 并行流式——换来源必须先把旧来源冲干净，两个会话的回复各回各家，
        // 不能拼成一条。
        let mut rc = RemoteControl::new();
        rc.on_assistant(&td(0, "微信线的回复"), "wx");
        let out = rc.on_assistant(&td(0, "桌面会话的"), "desktop");
        assert_eq!(out, vec!["微信线的回复".to_string()], "切换来源先冲旧来源");
        assert_eq!(rc.cur_text, "桌面会话的", "新来源从零累积");
        let out = rc.on_assistant(&tool_start(), "desktop");
        assert_eq!(out, vec!["桌面会话的".to_string()]);
    }

    #[test]
    fn idle_flush_drains_remaining_text_once() {
        let mut rc = RemoteControl::new();
        rc.on_assistant(&td(0, "最终答复"), "s");
        assert!(rc.on_running(true).is_empty(), "运行中不发");
        assert_eq!(rc.on_running(false), vec!["最终答复".to_string()]);
        assert!(rc.on_running(false).is_empty(), "只发一次，不重复");
    }

    #[test]
    fn nothing_is_sent_from_an_empty_buffer() {
        let mut rc = RemoteControl::new();
        assert!(rc.on_running(false).is_empty());
        assert!(rc.on_assistant(&tool_start(), "s").is_empty());
        rc.on_assistant(&td(0, "   "), "s");
        assert!(rc.on_running(false).is_empty(), "纯空白不发");
    }

    #[test]
    fn long_reply_is_split_into_chunks() {
        let mut rc = RemoteControl::new();
        let big: String = "很".repeat(9000);
        rc.on_assistant(&td(0, &big), "s");
        let out = rc.on_running(false);
        assert!(out.len() >= 3, "9000 字应切成多段，实际 {}", out.len());
        assert!(out.iter().all(|s| s.chars().count() <= 4000));
    }

    /// pi 冷启动横幅内部消化（2026-10-24）：与真实回复混轮时横幅整行剔除、
    /// 回复照发；只剩横幅时什么都不发。
    #[test]
    fn startup_banner_is_swallowed_not_sent() {
        let mut rc = RemoteControl::new();
        rc.on_assistant(&te(0, "🔌 MCP: 1 server enabled"), "s");
        rc.on_assistant(&td(1, "你好，我是助手"), "s");
        let out = rc.on_running(false);
        assert_eq!(out, vec!["你好，我是助手".to_string()], "横幅不得出站");

        let mut rc2 = RemoteControl::new();
        rc2.on_assistant(&te(0, "MCP: 2 servers enabled"), "s");
        assert!(rc2.on_running(false).is_empty(), "纯横幅 = 静默");

        // 缓冲行兜底：横幅以 delta 形式单独成行时同样剔除
        let mut rc3 = RemoteControl::new();
        rc3.on_assistant(&td(0, "mcp: 1 server enabled\n后续回复"), "s");
        assert_eq!(rc3.on_running(false), vec!["后续回复".to_string()]);

        // 变体：`server(s)`、大写、带色码残留都算横幅（只认 mcp+server+enabled）
        let mut rc4 = RemoteControl::new();
        rc4.on_assistant(&td(0, "🔌 MCP: 1 server(s) enabled"), "s");
        assert!(rc4.on_running(false).is_empty(), "变体同样不出站");
    }

    /// 影子会话名（用户定夺）：`pf-wxbot-` + **15 位**时间戳（纯数字）。
    #[test]
    fn wx_shadow_session_name_is_pf_wxbot_plus_15_digits() {
        let n = crate::Chat::wx_session_name();
        let suffix = n
            .strip_prefix("pf-wxbot-")
            .unwrap_or_else(|| panic!("名字必须以 pf-wxbot- 开头，实际 {n}"));
        assert_eq!(suffix.len(), 15, "15 位时间戳，实际 {suffix}");
        assert!(
            suffix.chars().all(|c| c.is_ascii_digit()),
            "纯数字，实际 {suffix}"
        );
    }

    /// 启停通知文案（002 §4）：⚡ 顶替【pf icon】，开/关两条都在。
    #[test]
    fn wx_link_notice_texts() {
        assert!(WX_LINK_ON.contains("Pi-Flash") && WX_LINK_ON.contains("已开启"));
        assert!(WX_LINK_OFF.contains("Pi-Flash") && WX_LINK_OFF.contains("已关闭"));
        assert!(!WX_LINK_ON.contains("MCP"), "不得泄漏 pi 横幅内容");
    }

    /// 无项目/模型态回执（用户定夺）：放行条件未满足时需要会话的动作
    /// 统一引导设项目+模型。
    #[test]
    fn no_setup_text_points_to_workspace_and_model_menus() {
        assert!(NO_SETUP.contains("请先设置项目和模型"));
        assert!(NO_SETUP.contains("/项目"), "必须指出项目自救入口");
        assert!(NO_SETUP.contains("/模型"), "必须指出模型自救入口");
    }

    /// 运行沿判定（W4）：升沿发开启、降沿发关闭、换绑降沿抑制、平沿不发。
    #[test]
    fn run_edge_detection() {
        // 首拍即 running（服务随窗口自启）= 升沿
        assert_eq!(run_edge(None, true, false), Some(true));
        // 首拍即停：无沿
        assert_eq!(run_edge(None, false, false), None);
        // 平沿不发
        assert_eq!(run_edge(Some(true), true, false), None);
        assert_eq!(run_edge(Some(false), false, false), None);
        // 正常升/降沿
        assert_eq!(run_edge(Some(false), true, false), Some(true));
        assert_eq!(run_edge(Some(true), false, false), Some(false));
        // 换绑 force_restart：wants_start 置位的降沿不发「已关闭」
        assert_eq!(run_edge(Some(true), false, true), None);
    }
}
