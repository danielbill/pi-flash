//! 微信 Bot 桥 —— 微信 ⇄ 会话。**设计文档唯一一份**：
//! `pi-flash-plugins/docs/插件设计/002-微信通信.md`（本仓原 060 设计文档已删，
//! 事件历史见 `docs/progress.md` 与 git log）。
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
//! **会话归属（2026-10-23 定案）**：入站/出站投给**微信线**
//! （影子会话，bot 语境独立于桌面选中态，对齐 ZCode「bot 自己的
//! activeTask」）。启用后第一拍认领当前活跃会话并持久化（state 的
//! `session_key`），重启后跟同一条线；`/新建` 给微信线开新草稿（不抢
//! 桌面焦点），`/task` 选会话改指向；目标被删回退活跃重新认领。

mod menu;
pub(crate) mod panel;
mod pipeline;
pub(crate) mod state;

use std::path::PathBuf;

use pi_link::protocol::{Command, ExtensionUiRequest};

use wxprobe::command::BotCommand;
use wxprobe::extui::{self, ExtUiReply};
use wxprobe::format::messages::{Lang, t};

use self::state::{Health, Service, Snapshot, WxLine, WxUi};
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

/// 绑定码无效的回执。ZCode 原句写的是「在 zcode UI 重新生成」—— 品牌换成
/// 本应用（2026-10-07 用户拍板，与 `/帮助` 标题同一决定），故**不走文案表**。
const BIND_INVALID: &[(&str, &str, &str)] = &[(
    "绑定码无效或已过期，请在 pi-flash 设置页重新生成。",
    "绑定码无效或已过期，请在 pi-flash 设置页重新生成。",
    "The bind code is invalid or expired. Generate a new one in the pi-flash settings page.",
)];

/// wxprobe 文案表（002 的微信侧用词）的唯一入口。语言索引只在这里读一次——
/// 旧实现 `wx_lang()` 在 10 个地方各写一遍。
fn wx_lang() -> Lang {
    Lang::from_ix(crate::i18n::lang_ix())
}

/// 取一条微信侧文案（`s` 同时是 wxprobe 文案表的键）。
fn wx_text(s: &str) -> String {
    t(wx_lang(), s)
}

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
///
/// **url 与阶段合一**（旧实现把 url 另存 `qr_url` 字段续命，两处必须同步，
/// 漏一次码就从屏幕消失——真机踩过）：`Scanned` 期手机还没确认，码必须留在
/// 屏幕上；`Expired` / `Error` 期蒙白要盖住**旧码**，所以这两个阶段也带 url。
#[derive(Debug, Clone, PartialEq)]
pub enum QrState {
    Idle,
    Loading,
    /// `url` = `qrcode_img_content`，用 [`wxprobe::qr::qr_block_text`] 渲成可扫码字符画
    Ready { url: String },
    /// 已扫码，等待手机确认（码继续显示）
    Scanned { url: String },
    /// 成功：token 已写进状态文件
    Done { bot_id: Option<String> },
    /// 超时：旧码上蒙白
    Expired { url: Option<String> },
    /// 微信侧出错：旧码上蒙白 + 文案
    Error { msg: String, url: Option<String> },
}

impl QrState {
    /// 手上那张码的 url（Ready/Scanned 有；Expired/Error 只用来垫背）。
    pub(crate) fn url(&self) -> Option<&str> {
        match self {
            QrState::Ready { url } | QrState::Scanned { url } => Some(url),
            QrState::Expired { url } | QrState::Error { url, .. } => url.as_deref(),
            _ => None,
        }
    }

    /// 本阶段是否「手里已经有一张活码」（begin_qr 的幂等判据）。
    fn live(&self) -> bool {
        matches!(
            self,
            QrState::Loading | QrState::Ready { .. } | QrState::Scanned { .. }
        )
    }
}

/// 微信渠道句柄 + 待答请求。挂在 `Chat` 上，由 [`spawn_wx_pump`] 驱动。
pub struct RemoteControl {
    /// 渠道生命周期（唯一真身：句柄就在 `On` 里，「在跑」不需要第二个 bool）
    svc: Service,
    /// 链路健康度（唯一真身：取代 start_error / poll_error / expired_rebind）
    health: Health,
    /// 微信线（影子会话）指针（唯一真身：取代内存 key + name 两份记账）
    line: WxLine,
    /// state.json 的进程内镜像：泵一拍刷一次，**渲染只读镜像**（不碰盘）
    snap: Snapshot,
    /// 用户开关意图（002 §3 第 2 件「switch + 说明文字」）：on = 允许服务跑；
    /// **能不能真跑还要看窗口激活**（002 §关于微信通话服务激活：不激活窗口
    /// 就不后台收消息，这是设计取舍）。
    enabled: bool,
    /// 【微信通话】标签是否展开——**唯一存下来的 UI 位**。前台三态
    /// （Closed/Tab/Page/Line）由 [`crate::Chat::wx_ui`] 现算，不再有第二份
    /// 状态可以撒谎（2026-10-25 事故：标签藏了、前台还停在微信页）。
    pub(crate) tab_open: bool,
    /// 最近一个**非微信线**的活跃会话 key：微信线是单开 tab，看微信时
    /// topbar 第一格（会话 tab）继续显示桌面会话；关标签也回它。
    /// **只许走 remember/take/desktop 三个方法**——别处直接读写会让「回桌面」
    /// 的去向莫名其妙。
    desktop_key: Option<String>,
    /// 设置页扫码面板状态（url 与阶段合一）
    pub qr: QrState,
    /// 扫码 worker 的回程（200ms 泵里 drain）
    qr_rx: Option<std::sync::mpsc::Receiver<wxprobe::register::QrEvent>>,
    /// `/bind <code>` 后钉住的会话 runtime key（§4.2 active_session）
    pub bound: Option<String>,
    /// 活跃会话这会儿是否在跑（泵每拍回填；`route()` 没有 `cx`，拿不到
    /// runtime——`/task` 的两个守卫要读它）。
    pub(crate) active_running: bool,
    /// 挂起中的数字选择菜单（060-c，对应 ZCode pendingSelectionsByContext）
    pending_menu: Option<menu::PendingMenu>,
    /// 正在等微信回数的阻塞型 ExtUi 请求
    pending: Option<Pending>,
    /// 当前 content index 的文本块
    cur_ix: Option<usize>,
    cur_text: String,
    /// 回复缓冲 + 来源会话 key。出站门对桌面与微信线都放行，换来源时必须
    /// 先把旧来源冲干净，否则两条线的回复会拼成一条发出去。
    buf: String,
    buf_src: Option<String>,
    /// 待发的启停通知：`Some(true)` = 待发「已开启」，`Some(false)` = 待发
    /// 「已关闭」。**由服务状态迁移点置位**（不再靠每拍沿检测 + 手工造假前态）。
    notice: Option<bool>,
    /// 「首进继承桌面 cwd」只许一次（002 §6：[新建会话] 清空 cwd 后不得被
    /// 设置页的「为空即继承」悄悄填回，否则更换工作目录失效）。
    pub(crate) cwd_inherited: bool,
    /// token 比对节流闸（换绑感知 1s 一次足够，别每拍读盘）
    token_check_at: Option<std::time::Instant>,
    /// 是否落盘。测试实例（[`Self::detached`]）= false：不碰真实 state 文件、
    /// 不起真渠道——**生产逻辑里不再有 `cfg!(test)` 分叉**。
    persist: bool,
}

impl RemoteControl {
    pub fn new() -> Self {
        let mut snap = Snapshot::empty();
        snap.refresh(true);
        let cwd_inherited = snap.wx_cwd.is_some();
        Self::with_snapshot(snap, true, cwd_inherited)
    }

    /// 测试实例：不读盘、不落盘、不起真渠道。生产分支里不再有 `cfg!(test)`
    /// 分叉——测试想知道的行为就是生产行为，只是没有副作用。
    #[cfg(test)]
    pub(crate) fn detached() -> Self {
        Self::with_snapshot(Snapshot::empty(), false, false)
    }

    fn with_snapshot(snap: Snapshot, persist: bool, cwd_inherited: bool) -> Self {
        Self {
            // 待启动：泵一拍按「开关 on 且窗口激活」决定起不起（002 §服务激活）
            enabled: true,
            svc: Service::Pending,
            health: Health::Ok,
            line: WxLine::default(),
            snap,
            tab_open: false,
            desktop_key: None,
            qr: QrState::Idle,
            qr_rx: None,
            bound: None,
            active_running: false,
            pending_menu: None,
            pending: None,
            cur_ix: None,
            cur_text: String::new(),
            buf: String::new(),
            buf_src: None,
            notice: None,
            cwd_inherited,
            token_check_at: None,
            persist,
        }
    }

    // ── 服务状态机（唯一启动/关停路径）──────────────────────────────

    /// 泵一拍：服务要不要在跑 = **用户开关 on 且窗口激活**（002 §关于微信通话
    /// 服务激活：低频功能，不激活桌面窗口就不后台收消息，这是设计取舍——
    /// 「哪怕设置为 on」）。顺带采样健康度。
    ///
    /// 关窗 = 静默停收发（**不是**「关停服务」，不发关停通知），重开窗口下一
    /// 拍自动重连；`Failed` 到点重试也只在激活期间进行。
    /// 返回 true = 有 UI 可见变化（调用方统一 notify 一次）。
    fn step_service(&mut self, activated: bool) -> bool {
        let want = self.enabled && activated;
        let mut changed = false;
        if !want {
            if self.svc.is_running() {
                self.stop(false);
                changed = true;
            }
        } else if self.svc.wants_start(std::time::Instant::now()) {
            self.start();
            changed = true;
        }
        changed | self.sample_health()
    }

    /// 起渠道。`persist=false`（测试实例）不读盘不起线程。
    ///
    /// 002 §5：会话只在**新建会话**时创建/绑定（用户指令）——渠道连上不轮换
    /// 影子会话、不动当前视图，启停没有跳转逻辑。
    fn start(&mut self) {
        if !self.persist {
            self.svc = Service::Failed {
                at: std::time::Instant::now(),
                err: "测试实例不启动真实渠道".into(),
            };
            return;
        }
        let st = wxprobe::state::load();
        let started = wxprobe::transport::config_from_state(&st).and_then(|cfg| {
            let token = cfg.token.clone();
            wxprobe::transport::spawn(&cfg).map(|t| (t, token))
        });
        match started {
            Ok((transport, token)) => {
                self.svc = Service::On { transport, token };
                // 升沿：进 On 就是要发一条「已开启」（路由未就绪则等首条入站）
                self.notice = Some(true);
                self.health = Health::Ok;
            }
            // 静默失败是最坏的形态（§6 坑 4）：文案留给状态卡④，30s 后重试
            Err(e) => {
                let err = format!("渠道未启动：{e}");
                eprintln!("[wx] {err}");
                self.svc = Service::Failed {
                    at: std::time::Instant::now(),
                    err,
                };
            }
        }
    }

    /// 停渠道。`user` = 用户显式关停（本进程不再自动拉起）；
    /// 否则落 `Pending`（换绑后立刻重拉）。
    fn stop(&mut self, user: bool) {
        let was_on = self.svc.is_running();
        let announced = self.notice.is_none();
        self.svc = if user { Service::Stopped } else { Service::Pending };
        // 降沿通知规则见 `state::stop_notice`（换绑落 Pending 时静默）
        self.notice = state::stop_notice(user, was_on, announced);
        self.health = Health::Ok;
    }

    /// 扫码成功 = 换绑定落地：旧错/过期标注一并清，渠道按新 token 重拉。
    ///
    /// 「重绑成功必发一条『已开启』」不需要手工造假前态：落 `Pending` 之后
    /// 下一拍 `start()` 进 `On` 就是一次真实的升迁移（旧实现靠把
    /// `run_edge_prev` 压成 false 骗出来）。
    fn on_rebind(&mut self) {
        self.health = Health::Ok;
        self.stop(false); // 落 Pending + 通知静默；下一拍 start() 必发「已开启」
    }

    // ── 落盘（state.json）：写盘 + 同步镜像，唯一出口 ────────────────

    /// 写 state.json 的单个键并同步镜像。`persist=false` 只改镜像。
    fn set_disk(&mut self, key: &str, value: Option<&str>) {
        if !self.persist {
            return;
        }
        let mut st = wxprobe::state::load();
        wxprobe::state::set(&mut st, key, value);
        wxprobe::state::save(&st);
        self.snap.refresh(true);
    }

    /// 设置微信**独立工作目录**并持久化（模块独立：不碰桌面 `ws.cwd`，也不
    /// 回写）。重启后自动恢复，`wx_cwd` + 模型齐即 ready。
    pub(crate) fn set_wx_cwd(&mut self, path: Option<PathBuf>) {
        self.snap.wx_cwd = path.clone();
        self.set_disk("wx_cwd", path.as_deref().and_then(|p| p.to_str()));
    }

    /// 微信线（影子会话）指针改指向 `key`（草稿键或会话文件路径）。
    pub(crate) fn set_wx_line(&mut self, key: impl Into<String>) {
        let key = key.into();
        self.line.key = Some(key.clone());
        self.set_disk("session_key", Some(&key));
    }

    /// 微信线指针失效（目标会话被删）：清内存与盘。
    pub(crate) fn clear_wx_line(&mut self) {
        self.line = WxLine::default();
        self.set_disk("session_key", None);
    }

    /// 目标会话被删时清空微信线归属（下次投递回退活跃并重建草稿）。
    pub(crate) fn clear_wx_line_if(&mut self, key: &str) {
        if self.line.is(key) {
            self.clear_wx_line();
        }
    }

    /// 扫码换绑检测：运行中渠道拿的 token 与 state 现值不一致 → 停掉重拉。
    ///
    /// 2026-10-23 真机事故：渠道在扫码**前**被拉起（拿着即将作废的旧 token），
    /// 扫码成功后泵见渠道还活着就跳过——新 token 永远装不进去；旧 token 被
    /// 新注册踢成 `errcode:-14`，长轮询死循环重试，消息一条进不来。
    pub(crate) fn force_restart_if_token_changed(&mut self) {
        let Some(running_token) = self.svc.started_token().map(str::to_string) else {
            return; // 没在跑：常规 Pending 路径已覆盖
        };
        // 节流：泵 200ms 一拍都读盘太勤，1s 一次足够（换绑感知人眼无感），
        //  Windows 实时杀软的 IO 抖动也别想打上 UI 线程。
        let now = std::time::Instant::now();
        if self
            .token_check_at
            .is_some_and(|t| now.duration_since(t) < std::time::Duration::from_secs(1))
        {
            return;
        }
        self.token_check_at = Some(now);
        if !self.persist {
            return;
        }
        self.snap.refresh(true);
        if self.snap.token.as_deref() != Some(running_token.as_str()) {
            // Drop 旧句柄：join 线程 + 释放轮询锁。坏 token 的请求即时失败，
            // join 几乎不等；健康长轮询里最坏等一个超时（与「关」同代价）。
            self.stop(false);
        }
    }

    /// 启停通知（002 §4，W4）派发：**由服务状态迁移点置位**，这里只负责把
    /// 待发那条送出去。
    ///
    /// 通知可以迟到、不能丢（2026-10-25 真机回归：升沿只发一次，路由未就绪
    /// 即永久丢）：置位时若对端路由还没回写（`chat_id`/`user_id` 随**首条
    /// 入站消息**到位），每拍重试到发出去为止。
    ///
    /// 升沿：transport 出站队列入队即可，send 线程每次发送前重读状态。
    /// 降沿：transport 已 drop、send 线程 100ms 内退出——**另起后台线程**用
    /// 一次性 `Wire` 直发（15s 超时，失败只 stderr，绝不阻塞 UI 线程）。
    fn flush_notice(&mut self) {
        let Some(on) = self.notice else {
            return;
        };
        if !self.snap.routed() {
            return; // 路由未就绪：留到下一拍
        }
        match on {
            true => {
                let Some(tr) = self.svc.transport() else {
                    return;
                };
                if tr.send(&wx_text(WX_LINK_ON)).is_ok() {
                    self.notice = None;
                }
            }
            false => {
                self.notice = None;
                let text = wx_text(WX_LINK_OFF);
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

    /// 泵一拍采样链路健康度（唯一真身：`Health`）。
    ///
    /// `Expired` 是**粘性**的：`-14 session timeout` 洗完旧游标仍是它 =
    /// 微信服务端已作废这个 bot 会话，继续长轮询只是空转（而开关亮着会让
    /// 人以为链路是通的，2026-10-24 真机），只有重新扫码能解。
    /// 过期**只在用户主动打开模块时**才落地成 UI 跳转（002 原文「发送消息前
    /// 检测到过期」）——泵只置旗标，绝不把用户正在看的界面掰走。
    fn sample_health(&mut self) -> bool {
        if self.health.expired() {
            return false;
        }
        let next = match self.svc.transport() {
            None => Health::Ok,
            Some(tr) if tr.session_dead() => Health::Expired,
            Some(tr) => match tr.poll_error() {
                Some(e) => Health::Poll(e),
                None => Health::Ok,
            },
        };
        let changed = match (&self.health, &next) {
            (Health::Ok, Health::Ok) => false,
            (Health::Poll(a), Health::Poll(b)) => a != b,
            _ => true,
        };
        if changed {
            if let Health::Expired = next {
                eprintln!("[wx] 会话已过期，需重新扫码绑定");
            }
            self.health = next;
        }
        changed
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

    /// 入站/出站的投递目标：**微信线**优先（bot 语境，独立于桌面选中态，
    /// 对齐 ZCode「bot 自己的 activeTask」）；`/bind` 授权记录次之；都没有
    /// （线还没建）回落活跃会话。
    pub(crate) fn target_key(&self, active: &str) -> String {
        if let Some(k) = self.line.key() {
            return k.to_string();
        }
        if let Some(k) = &self.bound {
            return k.clone();
        }
        active.to_string()
    }

    /// 认领语义锚点（**仅测试**）：线为空时钉住给定 key。运行时建线走
    /// `Chat::wx_ensure_session`（2026-10-24 定夺：不再认领桌面活跃会话）。
    #[cfg(test)]
    pub(crate) fn claim_default_session(&mut self, active: &str) {
        if self.line.key.is_none() {
            self.set_wx_line(active.to_string());
        }
    }

    /// 开/关渠道（操作栏开关 · 自动化 `wx.enable` · 微信端 `/启` `/停`）。
    ///
    /// 开 = 落 `Pending`，泵下一拍起（仍要求窗口激活；无 token 起不来 →
    /// `Failed` 文案 + 30s 后重试，不留死状态）；关 = 句柄释放（`Drop` →
    /// **join 两条线程 + 释放轮询锁**，P4 验收：关停即停线程，无悬挂），
    /// 并给微信端发一条「通信已关闭」（002 §4）。
    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        if on {
            self.svc = Service::Pending;
            self.notice = None;
            self.health = Health::Ok;
        } else {
            self.stop(true);
        }
    }

    /// 渠道是否活着（设置页开关的回显）。
    pub fn is_running(&self) -> bool {
        self.svc.is_running()
    }

    /// 记下桌面会话（切进微信线之前调用）。
    pub(crate) fn remember_desktop(&mut self, key: &str) {
        self.desktop_key = Some(key.to_string());
    }

    /// 取走桌面会话记忆（关微信内容、回桌面的唯一去处来源）。
    pub(crate) fn take_desktop(&mut self) -> Option<String> {
        self.desktop_key.take()
    }

    /// 看一眼桌面会话记忆（topbar 第一格的标题来源）。
    pub(crate) fn desktop(&self) -> Option<&str> {
        self.desktop_key.as_deref()
    }

    /// 盘镜像里是否已有 bot token（面板三态 / 绑定段判据）。
    pub(crate) fn has_token(&self) -> bool {
        self.snap.has_token()
    }

    /// 微信**独立工作目录**（模块独立，与桌面 cwd 双向禁止联动）。
    pub(crate) fn wx_cwd(&self) -> Option<&PathBuf> {
        self.snap.wx_cwd.as_ref()
    }

    /// `key` 是否就是微信线（跨模块只读判据；写侧走 set_wx_line/clear）。
    pub(crate) fn on_wx_line(&self, key: &str) -> bool {
        self.line.is(key)
    }

    /// 影子会话名（`pf-wxbot-*`；FileBound 时补进过滤清单用）。
    pub(crate) fn line_name(&self) -> Option<&str> {
        self.line.name.as_deref()
    }

    /// 失败文案（状态卡④）；成功即清（`Service` 状态本身就是清）。
    pub(crate) fn start_error(&self) -> Option<&str> {
        self.svc.error()
    }

    /// 重启判定节流用的记录（泵不需要直接读）。
    pub(crate) fn health(&self) -> &Health {
        &self.health
    }

    /// 发起扫码：专用线程跑 `begin → 3s 轮询`，结果由泵 drain。
    pub fn begin_qr(&mut self) {
        if self.qr.live() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.qr_rx = Some(rx);
        self.qr = QrState::Loading;
        // 180s：二维码实际约 120s 有效，留出轮询余量
        std::thread::spawn(move || wxprobe::register::run_qr_flow(tx, 180));
    }

    /// 强制换一张新码（操作栏「重新绑定」/ 码方框内【刷新二维码】）。
    /// 与 [`Self::begin_qr`] 的幂等早退相反：手里那张码一律丢掉再起一轮，
    /// 否则用户点「刷新」看到的还是同一张旧码。被丢下的 worker 在下一次
    /// `tx.send` 失败时自行退出（`run_qr_flow` 的取消约定）。
    pub fn refresh_qr(&mut self) {
        self.qr_rx = None;
        self.qr = QrState::Idle;
        self.begin_qr();
    }

    /// 把扫码 worker 的事件抽干进 `self.qr`；有变化才返回 `true`。
    pub fn poll_qr(&mut self) -> bool {
        let mut changed = false;
        let mut events = Vec::new();
        if let Some(rx) = &self.qr_rx {
            while let Ok(ev) = rx.try_recv() {
                events.push(ev);
            }
        }
        for ev in events {
            changed = true;
            self.advance_qr(ev);
        }
        changed
    }

    /// 扫码事件 → 面板状态（唯一迁移点：url 跟着阶段走，不需要旁路字段）。
    fn advance_qr(&mut self, ev: wxprobe::register::QrEvent) {
        use wxprobe::register::QrEvent as E;
        let url = self.qr.url().map(str::to_string);
        self.qr = match ev {
            E::Ready { url } => QrState::Ready { url },
            // 手机确认中：码必须留在屏幕上，所以带上 url
            E::Scanned => QrState::Scanned {
                url: url.unwrap_or_default(),
            },
            E::Success { bot_id } => QrState::Done { bot_id },
            E::Expired => QrState::Expired { url },
            E::Error(msg) => QrState::Error { msg, url },
        };
    }

    pub fn send(&self, text: &str) {
        if let Some(t) = self.svc.transport() {
            let _ = t.send(text);
        }
    }

    /// 抽干入站队列（非阻塞）。整批返回，调用方处理完必须 ack。
    fn take_batches(&mut self) -> Vec<wxprobe::transport::Batch> {
        let Some(t) = self.svc.transport() else {
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
        if let Some(t) = self.svc.transport() {
            t.ack_routing(batch);
        }
    }

    /// 游标落盘（§6 坑 2：**处理完才写**）。
    fn ack_cursor(&self, batch: &wxprobe::transport::Batch) {
        if let Some(t) = self.svc.transport() {
            t.ack_cursor(batch);
        }
    }

    /// ExtUi 请求 → 微信文本；阻塞型同时挂起 `pending` 等回数。
    ///
    /// 与桌面弹窗**并行**拿到同一请求（§4.1）：谁先应答算谁的，后到那份由
    /// [`Self::clear_pending`] 与 pi 侧的单次 resolve 一起丢弃。
    pub fn on_ext_ui(&mut self, req: &ExtensionUiRequest) {
        let lang = wx_lang();
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

        let lang = wx_lang();
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
    /// `key` = 事件来源会话；与 [`Self::buf_src`] 不同源时先把旧来源冲干净
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
        if self.buf_src.as_deref() != Some(key) {
            out.extend(self.flush());
            self.buf_src = Some(key.to_string());
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
    // ── 微信 UI 位置（唯一判据）──────────────────────────────────────

    /// 活跃会话是不是微信线（可见性/归属判据的**唯一真身**）。
    pub(crate) fn wx_line_active(&self) -> bool {
        self.aux.remote.line.is(&self.pool.active_key)
    }

    /// 微信 UI 现状。**只有标签开合是存的**（`RemoteControl::tab_open`），
    /// 前台三态每帧现算——所以标签、前台、归属不可能各说各话
    /// （2026-10-25 事故：标签被藏、前台还停在微信页 = 孤儿页）。
    pub(crate) fn wx_ui(&self) -> WxUi {
        if !self.aux.remote.tab_open {
            return WxUi::Closed;
        }
        match self.layout.content_view {
            crate::ContentView::Wx => WxUi::Page,
            crate::ContentView::Chat if self.wx_line_active() => WxUi::Line,
            _ => WxUi::Tab,
        }
    }

    /// 展开【微信通话】标签（幂等，不动前台）。独立工作目录首进从桌面 cwd
    /// 继承一次（002 模块独立：此后两边 cwd 互不联动）。
    pub(crate) fn wx_open_tab(&mut self) {
        self.aux.remote.tab_open = true;
        if !self.aux.remote.cwd_inherited && self.aux.remote.snap.wx_cwd.is_none() {
            if let Some(cwd) = self.ws.cwd.clone() {
                self.aux.remote.set_wx_cwd(Some(cwd));
            }
            self.aux.remote.cwd_inherited = true;
        }
    }

    // ── 微信线（影子会话）就位 ───────────────────────────────────────

    /// 微信线就位（幂等）。无线时按序尝试：① `state.session_key` 指向的真
    /// 会话文件 → 原线恢复；② 过滤清单 owner 条目的 path（每次 FileBound 都
    /// 钉着最后一条真会话，兜底被旧进程覆盖死的草稿键）→ 同上；③ 都没有才
    /// 新开专属草稿。detached 不抢桌面焦点，尾窗装载——微信端接着聊。
    ///
    /// 调用时机 = 标签展开 / 首条入站（手机端启动聊天，002）。独立工作目录
    /// 未定时不建：入站由 `run_wx_action` 统一引导「请先设置项目和模型」。
    pub(crate) fn wx_ensure_session(&mut self, cx: &mut gpui::Context<Self>) {
        if self.aux.remote.line.key.is_some() {
            return;
        }
        let Some(cwd) = self.aux.remote.snap.wx_cwd.clone() else {
            return;
        };
        // 两级指针兜底：真文件优先，清单里的 owner 条目兜底
        let target = self
            .aux
            .remote
            .snap
            .session_key
            .clone()
            .filter(|k| std::path::Path::new(k).is_file())
            .or_else(wx_owned_session_file)
            .map(PathBuf::from);
        if let Some(path) = target {
            if let Some(rt) = self.open_session_detached_with(path, cwd, cx) {
                let key = rt.read(cx).key.clone();
                self.aux.remote.set_wx_line(key);
                return;
            }
        }
        if let Some(key) = self.new_draft_detached(cx) {
            self.aux.remote.set_wx_line(key);
        }
    }

    // ── 泵：每拍五个幂等 step，末尾统一 notify 一次 ──────────────────

    /// 200ms 一拍（`spawn_wx_pump`）。旧实现是 120 行流水账，每段各自
    /// `cx.notify()`，还要用 `ensured` 布尔猜「这一拍要不要重绘」。
    pub(crate) fn pump_wx(&mut self, cx: &mut gpui::Context<Self>) {
        let mut dirty = false;
        // ① 盘 → 镜像（mtime 没变就不真读；渲染只读镜像，永不碰盘）
        dirty |= self.aux.remote.snap.refresh(false);
        // ② 服务状态机：用户开关 on 且窗口激活才在跑（002 §服务激活）
        let activated = self.aux.remote.tab_open;
        dirty |= self.aux.remote.step_service(activated);
        // ③ 换绑检测：扫码换了 token → 停旧句柄，下一拍用新 token 重拉
        self.aux.remote.force_restart_if_token_changed();
        // ④ 入站 → 线就位 → 逐批执行（标签关着时只在有入站时才建线）
        let batches = self.aux.remote.take_batches();
        if !batches.is_empty() || self.aux.remote.tab_open {
            let before = self.aux.remote.line.key.clone();
            self.wx_ensure_session(cx);
            dirty |= before != self.aux.remote.line.key;
        }
        dirty |= self.wx_step_reply(cx);
        dirty |= self.wx_step_qr(cx);
        if !batches.is_empty() {
            self.wx_run_batches(&batches, cx);
            dirty = true;
        }
        // ⑤ 启停通知派发（对端路由未就绪则留到下一拍，迟到不丢）
        self.aux.remote.flush_notice();
        if dirty {
            cx.notify();
        }
    }

    /// 回复缓冲冲刷：空闲/轮次结束把 assistant 缓冲发干净（空缓冲 no-op）。
    /// 返回 true = 发过东西（需要重绘状态栏/用量）。
    fn wx_step_reply(&mut self, cx: &mut gpui::Context<Self>) -> bool {
        let wx_target = self.aux.remote.target_key(&self.pool.active_key);
        let running = self
            .pool
            .runtimes
            .get(&wx_target)
            .map(|rt| rt.read(cx).agent_running)
            .unwrap_or(false);
        // 回填给 route()：`/task` 的两个守卫要读它（route 拿不到 cx）
        self.aux.remote.active_running = running;
        // 冲刷看**缓冲来源**的运行态：桌面流式中的文本不该被 200ms 一拍
        // 冲成碎片（两条线并行流式时各回各家）。
        let src_running = self
            .aux
            .remote
            .buf_src
            .as_deref()
            .and_then(|k| self.pool.runtimes.get(k))
            .map(|rt| rt.read(cx).agent_running)
            .unwrap_or(running);
        let mut sent = false;
        for text in self.aux.remote.on_running(src_running) {
            self.aux.remote.send(&text);
            sent = true;
        }
        sent
    }

    /// 扫码事件落地。扫码成功 = 换绑定：健康度清零、渠道按新 token 重拉、
    /// 关二维码弹窗、功能页自动切下一段（Setup/Chat）。
    fn wx_step_qr(&mut self, cx: &mut gpui::Context<Self>) -> bool {
        if !self.aux.remote.poll_qr() {
            return false;
        }
        if matches!(self.aux.remote.qr, QrState::Done { .. }) {
            self.aux.remote.on_rebind();
            if matches!(self.dlg.dialog, Some(crate::Dialog::WxQr)) {
                self.dlg.dialog = None;
            }
            if self.layout.content_view == crate::ContentView::Wx {
                self.open_wx(cx);
            }
        }
        true
    }

    /// 整批处理完才 ack 游标（§6 坑 2：中途 ack 会丢消息）。
    fn wx_run_batches(
        &mut self,
        batches: &[wxprobe::transport::Batch],
        cx: &mut gpui::Context<Self>,
    ) {
        for batch in batches {
            // 路由字段先行：回执在 route 时就出队，send 线程马上读状态文件，
            // 对端没写好 = 首条回执被静默丢弃（「第一条没反应」根因）
            self.aux.remote.ack_routing(batch);
            for m in &batch.messages {
                let action = self.aux.remote.route(&m.text);
                self.run_wx_action(action, cx);
            }
            self.aux.remote.ack_cursor(batch);
        }
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
            let lang = wx_lang();
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
                let lang = wx_lang();
                if let Some(text) = self.status_reply(cx, lang) {
                    self.aux.remote.send(&text);
                }
            }
            Action::New => {
                let lang = wx_lang();
                if self.agent_running_now(cx) {
                    // ZCode `case "new"`：任务运行中不让新建
                    self.aux.remote.send(&t(lang, TASK_RUNNING));
                } else {
                    // 微信 /新建 = 给 bot 语境开一条新线，**不抢桌面焦点**
                    //（微信线的 activeTask 独立于桌面选中态，对齐 ZCode）
                    if let Some(key) = self.new_draft_detached(cx) {
                        self.aux.remote.set_wx_line(key);
                    }
                    // 建完草稿回状态卡（`return createStatusReply(...)`）
                    if let Some(text) = self.status_reply(cx, lang) {
                        self.aux.remote.send(&text);
                    }
                }
            }
            Action::OpenMenu(kind, sub) => self.open_wx_menu(kind, sub, cx),
            Action::Bind(code) => {
                let lang = wx_lang();
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
        let lang = wx_lang();
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
                        .send(&t(wx_lang(), TASK_RUNNING));
                    return;
                }
                if let Some(info) = self.pool.sessions.iter().find(|s| s.id == value) {
                    let path_key = info.path.to_string_lossy().to_string();
                    if self.pool.runtimes.contains_key(&path_key) {
                        // 已开着 → 只改微信线归属，桌面不动（bot 语境独立）
                        self.aux.remote.set_wx_line(path_key);
                    } else {
                        // 没开着 → 打开即活跃（现存唯一无焦打开路径），
                        // 微信线跟到新活跃
                        let path = info.path.clone();
                        self.new_session_in(path, cx);
                        self.aux.remote.set_wx_line(self.pool.active_key.clone());
                    }
                }
                self.reply_status(cx);
            }
        }
    }

    /// 选完 / 设置完回一张状态卡（ZCode 各选择分支的 `createStatusReply`）。
    fn reply_status(&self, cx: &gpui::App) {
        let lang = wx_lang();
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
                self.aux.remote.clear_wx_line_if(&key);
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
        let lang = wx_lang();
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

    /// 微信设置会话上下文：功能页前台，或活跃会话即微信线。项目选择在此
    /// 上下文一律写独立工作目录 `wx_cwd`（禁止联动桌面 cwd）。
    pub(crate) fn wx_setup_ctx(&self) -> bool {
        self.wx_ui().front()
    }

    /// 微信聊天放行条件（002 §2，用户定夺：项目和模型都是必选项）：
    /// 微信独立工作目录已定 + 微信线目标会话模型已定。任一不满足，正文/
    /// 状态/新建/停止一律引导「请先设置项目和模型」，不建会话、不投递。
    fn wx_chat_ready(&self, cx: &gpui::App) -> bool {
        if self.aux.remote.snap.wx_cwd.is_none() {
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
    /// 微信线（ready for chat，手机端即可发起对话——微信聊天由手机
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
        // open_wx 内部：线为空则新建，然后激活会话并落到 Chat 视图
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
    use super::state::{Service, stop_notice};
    use super::*;

    /// 档 1 的命令覆盖面：只有 /停止 与纯文本真正接线，
    /// 其余一律回 ZCode 现成的「未启用」句，**不静默吞掉**。
    /// 测试里直接塞一个挂起菜单（真实路径走 `Chat::open_wx_menu`）。
    fn set_pending(rc: &mut RemoteControl, m: menu::PendingMenu) {
        rc.pending_menu = Some(m);
    }

    #[test]
    fn tier1_commands_are_all_wired() {
        let mut rc = RemoteControl::detached();
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
        let mut rc = RemoteControl::detached();
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
    fn set_enabled_drives_the_service_state_machine() {
        // 开 = 落 Pending（泵下一拍起）；关 = 落 Stopped（本进程不再自动拉起）。
        // 旧实现这两个意图挤在 boot_attempted + wants_start 两个 bool 里。
        let mut rc = RemoteControl::detached();
        assert!(!rc.is_running(), "默认没在跑");
        rc.set_enabled(true);
        assert!(matches!(rc.svc, Service::Pending), "开启 = 待启动");
        assert!(rc.svc.wants_start(std::time::Instant::now()));
        rc.set_enabled(false);
        assert!(matches!(rc.svc, Service::Stopped), "关停 = 用户显式表态");
        assert!(
            !rc.svc.wants_start(std::time::Instant::now()),
            "关停后泵不得自动重开"
        );
        // 窗口不激活时，「开关 on」也不跑（002 §服务激活：低频功能不后台收消息）
        rc.set_enabled(true);
        rc.step_service(false);
        assert!(
            matches!(rc.svc, Service::Pending),
            "未激活窗口绝不启动渠道"
        );
    }

    #[test]
    fn failed_service_retries_after_cooldown() {
        // 开机那次撞上「锁被占 / token 没写」不该整个进程都不再试：
        // Failed 到点自动给一次重试机会。
        let now = std::time::Instant::now();
        let fresh = Service::Failed {
            at: now,
            err: "x".into(),
        };
        assert!(!fresh.wants_start(now), "刚失败不立刻重试");
        assert!(
            fresh.wants_start(now + std::time::Duration::from_secs(31)),
            "冷却到点允许重试"
        );
        assert_eq!(fresh.error(), Some("x"));
    }

    #[test]
    fn stop_notice_only_after_an_announced_start() {
        // 用户关停 + 曾经宣告开启 → 发「已关闭」
        assert_eq!(stop_notice(true, true, true), Some(false));
        // 「已开启」还没发出去就关 → 不补发关闭
        assert_eq!(stop_notice(true, true, false), None);
        // 换绑（非用户关停）→ 静默，紧接着的 On 发「已开启」
        assert_eq!(stop_notice(false, true, true), None);
        // 本来就没跑 → 无通知
        assert_eq!(stop_notice(true, false, true), None);
    }

    #[test]
    fn bind_code_is_stable_six_digits_and_session_scoped() {
        let rc = RemoteControl::detached();
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
        let mut rc = RemoteControl::detached();
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
        let mut rc = RemoteControl::detached();
        assert_eq!(rc.target_key("draft-9"), "draft-9");

        // 认领后固定指向微信线，桌面切换不再牵动
        rc.claim_default_session("draft-9");
        assert_eq!(rc.target_key("draft-11"), "draft-9", "认领后不跟桌面活跃");

        // /bind 是授权记录，不抢占微信线的路由地位（ZCode：绑 workspace，
        // 选会话走 /task）；此处仅验证优先级次序
        rc.line.key = None;
        let code = rc.bind_code("draft-7");
        assert!(rc.accept_bind(&code, "draft-7"));
        assert_eq!(rc.target_key("draft-11"), "draft-7");
    }

    #[test]
    fn deleting_a_session_only_clears_the_wx_line() {
        let mut rc = RemoteControl::detached();
        rc.claim_default_session("draft-9");
        assert!(rc.line.key.is_some());
        // 删的不是微信线 → 不动
        rc.clear_wx_line_if("draft-5");
        assert!(rc.line.key.is_some());
        // 删的正是微信线 → 清空，下拍重建草稿
        rc.clear_wx_line_if("draft-9");
        assert!(rc.line.key.is_none());
        rc.claim_default_session("draft-11");
        assert_eq!(rc.target_key("draft-11"), "draft-11");
    }

    #[test]
    fn help_reply_has_title_and_all_lines() {
        let mut rc = RemoteControl::detached();
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
        let mut rc = RemoteControl::detached();
        assert!(rc.pending.is_none());
        assert!(matches!(rc.route("1"), Action::Prompt(_)));
    }

    // ── 普通消息不再有自动欢迎/帮助菜单（2026-10-24 所有者定夺）─────

    /// 之前每启动一次都会在首条消息上推一整份命令菜单（用户实测「每次
    /// 打开都弹」）——现在普通消息一律直接进模型，菜单只由 `/帮助` 拉。
    #[test]
    fn first_plain_message_goes_straight_to_the_model() {
        let mut rc = RemoteControl::detached();
        assert!(matches!(rc.route("你好"), Action::Prompt(_)));
        assert!(matches!(rc.route("/帮助"), Action::Send(_)));
    }

    /// 活跃会话在跑时 `/task` 既不给列表也不许切换（ZCode 两处 Bugfix）。
    #[test]
    fn task_menu_is_refused_while_running() {
        let mut rc = RemoteControl::detached();
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
        let mut rc = RemoteControl::detached();
        assert!(rc.on_assistant(&td(0, "你好"), "s").is_empty());
        assert!(rc.on_assistant(&td(0, "，我是"), "s").is_empty());
        assert!(rc.on_assistant(&td(0, "助手"), "s").is_empty());
        assert_eq!(rc.cur_text, "你好，我是助手");
        assert!(rc.buf.is_empty());
    }

    #[test]
    fn text_end_overrides_accumulated_deltas() {
        // TextEnd 是权威内容，用来纠正丢 delta；但不能清掉上一块
        let mut rc = RemoteControl::detached();
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
        let mut rc = RemoteControl::detached();
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
        let mut rc = RemoteControl::detached();
        rc.on_assistant(&td(0, "微信线的回复"), "wx");
        let out = rc.on_assistant(&td(0, "桌面会话的"), "desktop");
        assert_eq!(out, vec!["微信线的回复".to_string()], "切换来源先冲旧来源");
        assert_eq!(rc.cur_text, "桌面会话的", "新来源从零累积");
        let out = rc.on_assistant(&tool_start(), "desktop");
        assert_eq!(out, vec!["桌面会话的".to_string()]);
    }

    #[test]
    fn idle_flush_drains_remaining_text_once() {
        let mut rc = RemoteControl::detached();
        rc.on_assistant(&td(0, "最终答复"), "s");
        assert!(rc.on_running(true).is_empty(), "运行中不发");
        assert_eq!(rc.on_running(false), vec!["最终答复".to_string()]);
        assert!(rc.on_running(false).is_empty(), "只发一次，不重复");
    }

    #[test]
    fn nothing_is_sent_from_an_empty_buffer() {
        let mut rc = RemoteControl::detached();
        assert!(rc.on_running(false).is_empty());
        assert!(rc.on_assistant(&tool_start(), "s").is_empty());
        rc.on_assistant(&td(0, "   "), "s");
        assert!(rc.on_running(false).is_empty(), "纯空白不发");
    }

    #[test]
    fn long_reply_is_split_into_chunks() {
        let mut rc = RemoteControl::detached();
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
        let mut rc = RemoteControl::detached();
        rc.on_assistant(&te(0, "🔌 MCP: 1 server enabled"), "s");
        rc.on_assistant(&td(1, "你好，我是助手"), "s");
        let out = rc.on_running(false);
        assert_eq!(out, vec!["你好，我是助手".to_string()], "横幅不得出站");

        let mut rc2 = RemoteControl::detached();
        rc2.on_assistant(&te(0, "MCP: 2 servers enabled"), "s");
        assert!(rc2.on_running(false).is_empty(), "纯横幅 = 静默");

        // 缓冲行兜底：横幅以 delta 形式单独成行时同样剔除
        let mut rc3 = RemoteControl::detached();
        rc3.on_assistant(&td(0, "mcp: 1 server enabled\n后续回复"), "s");
        assert_eq!(rc3.on_running(false), vec!["后续回复".to_string()]);

        // 变体：`server(s)`、大写、带色码残留都算横幅（只认 mcp+server+enabled）
        let mut rc4 = RemoteControl::detached();
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

}

/// 过滤清单里微信线的 owner 条目 path（真文件兜底指针）。读盘，仅在线指针
/// 失效时调用（`wx_ensure_session`），不在渲染/热路径里。
fn wx_owned_session_file() -> Option<String> {
    pi_link::hidden_sessions::load()
        .entries
        .iter()
        .find(|e| e.owner.as_deref() == Some(pi_link::hidden_sessions::OWNER_WX))
        .and_then(|e| e.path.clone())
        .filter(|p| std::path::Path::new(p).is_file())
}
