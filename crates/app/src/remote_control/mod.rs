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
use pi_link::protocol::{Command, ExtensionUiRequest};

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use wxprobe::command::BotCommand;
use wxprobe::extui::{self, ExtUiReply};
use wxprobe::format::messages::{Lang, t};

use crate::appearance::theme;
use crate::i18n::{tf, tr};

/// 档 1 尚未接入的命令回执 —— ZCode 文案表里的现成句子，不新造。
const CMD_NOT_ENABLED: &str = "当前 bot 未启用这个命令。";
/// ZCode `weixinActivatedWelcome`（首条消息激活回执的抬头）。
const ACTIVATED_WELCOME: &str = "微信 Bot 已激活。发送 **/帮助** 查看命令，或直接描述你要做的事。";
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
const TASK_RUNNING: &str = "当前任务正在运行，稍后再试，或使用 **/停止** 停止当前任务。";

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
    active_running: bool,
    /// 本运行期是否已确认过「激活」状态（真正事实来源是状态文件里的
    /// `activated_at`，这里只是省掉每条消息一次文件读）。
    activated: bool,
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
    /// `/绑定 <code>` 后钉住的会话 runtime key（§4.2 `active_session`）
    pub bound: Option<String>,
    /// **微信默认会话**（bot 语境，2026-10-23）：微信消息的固定归属，独立于
    /// 桌面选中态——否则 PF 一重启/一切会话，微信就不知道在跟谁说话。
    /// 首拍认领当前活跃会话并持久化到 state 的 `session_key`；`/新建` 换新线；
    /// `/task` 选会话改指向；目标被删则回退活跃。
    pub wx_session: Option<String>,
    /// 显式启用待执行标记（设置页开关 / `wx.enable` / 扫码成功置位，
    /// 泵下一拍统一执行 [`Self::start_transport_from_state`]）。绕开
    /// SELF_START 门控的同时，让测试可以断言意图而不真起线程。
    pub(crate) wants_start: bool,
}

impl RemoteControl {
    pub fn new() -> Self {
        Self {
            transport: None,
            active_running: false,
            activated: false,
            boot_attempted: false,
            pending: None,
            cur_ix: None,
            cur_text: String::new(),
            buf: String::new(),
            was_running: false,
            pending_menu: None,
            qr: QrState::Idle,
            qr_rx: None,
            bound: None,
            wx_session: None,
            wants_start: false,
        }
    }

    /// 惰性起渠道。没扫码 / 被别的轮询者持锁都只是**记一条日志**，
    /// 不 panic 不重试 —— 桌面端该干什么还干什么。
    /// ZCode `handleWeixinFirstActivation`（botsService.ts:957）。
    ///
    /// 源码注释的原话：微信扫码**只返回 bot token/id，不返回可投递的用户 id**；
    /// 第一条微信入站消息用于建立会话目标，因此只回激活说明，
    /// **不把「你好」这类激活文本误当成任务 prompt**。
    ///
    /// 只对**普通消息**触发（`command.type === "message"`），命令照常分发；
    /// **事实来源是 `chat_id`**（`ensure_transport` 里回填到 `self.activated`）：
    /// 这正是 ZCode 那段注释说的「还没有可投递的用户 id」；`ack` 落了
    /// `chat_id` 之后重启就不会重复欢迎，**不需要新增状态字段** ——
    /// 更重要的是避免和 `ack` 在同一文件上读改写互踩。
    /// 成功建立时返回要回的文案。
    fn take_first_activation(&mut self, lang: Lang) -> Option<String> {
        if self.activated {
            return None;
        }
        self.activated = true;
        // welcome + 帮助（ZCode: [msg(weixinActivatedWelcome), buildHelpText].join("\n\n")）
        Some(format!(
            "{}\n\n{}",
            t(lang, ACTIVATED_WELCOME),
            crate::remote_control::pipeline::help_text(lang)
        ))
    }

    /// 微信桥自启开关。**已断开**（2026-10-15 所有者定夺：回头抽成插件，
    /// 插件落地前不再随启动拉起——配置存在也不再碰轮询锁，多实例也不再有
    /// 「另一个轮询者」报错）。060 机器件全保留，恢复自启改回 true。
    const SELF_START: bool = false;

    fn ensure_transport(&mut self) {
        if self.boot_attempted {
            return;
        }
        self.boot_attempted = true;
        if !Self::SELF_START {
            return;
        }
        self.start_transport_from_state();
    }

    /// 显式启用（设置页开关 / 扫码成功）——**不受 SELF_START 门控**。
    /// SELF_START 只断「随启动自拉起」（2026-10-15 定夺）；开关与扫码是
    /// 用户当场明示意图，照常起渠道。修复：曾因一刀切早退，扫码成功后
    /// 轮询永不启动——弹窗正常关、消息永远收不到。
    fn start_transport_from_state(&mut self) {
        let st = wxprobe::state::load();
        // 微信默认会话归属随渠道恢复（重启后微信仍跟同一条线说话）
        self.wx_session = wxprobe::state::get_str(&st, "session_key");
        // 有 chat_id = 之前已经建立过投递目标，跳过激活欢迎（等价于 ZCode
        // 持久化的 `weixinActivatedAt`）
        self.activated = wxprobe::state::get_str(&st, "chat_id").is_some();
        let started = wxprobe::transport::config_from_state(&st)
            .and_then(|cfg| wxprobe::transport::spawn(&cfg));
        match started {
            Ok(t) => self.transport = Some(t),
            // 静默失败是最坏的形态（§6 坑 4），至少打一行
            Err(e) => eprintln!("[wx] 渠道未启动：{e}"),
        }
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

    /// 启动认领：微信线还没有归属时认当前活跃会话（幂等，每拍调用安全）。
    /// 「PF 启动时应有一个默认的微信对话，否则微信跟谁说话」——归属在
    /// 渠道启用后的第一拍就有，之后桌面切换不再牵动微信线。
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
            // 泵的自动路径关掉：ensure_transport 是 SELF_START 门控的开机自启，
            // 显式启用走 wants_start → 泵下一拍 start_transport_from_state
            self.boot_attempted = true;
            self.wants_start = true;
        } else {
            self.wants_start = false;
            self.transport = None;
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

    /// 把扫码 worker 的事件抽干进 `self.qr`；有变化才返回 `true`（触发重绘）。
    pub fn poll_qr(&mut self) -> bool {
        let mut changed = false;
        if let Some(rx) = &self.qr_rx {
            while let Ok(ev) = rx.try_recv() {
                changed = true;
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

    /// 游标落盘（§6 坑 2：**处理完才写**）。
    fn ack(&self, batch: &wxprobe::transport::Batch) {
        if let Some(t) = &self.transport {
            t.ack(batch);
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
            BotCommand::Message { text: msg } => {
                // 激活只拦「普通消息」，命令照常走（与 ZCode 同序）
                if let Some(reply) = self.take_first_activation(lang) {
                    return Action::Send(reply);
                }
                Action::Prompt(msg)
            }
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
    /// flush 边界严格照 ZCode / §6 坑 1：
    /// * `TextDelta` 只累积，**不发**（provider chunk 常按词或子词到达）
    /// * `TextEnd` 用权威内容**覆盖**当前块（纠正丢 delta）
    /// * `ToolCallStart` → force flush（真正的发送边界之一）
    /// * 轮次结束由 [`Self::on_running`] 的下降沿触发
    pub fn on_assistant(&mut self, ev: &pi_link::protocol::AssistantEvent) -> Vec<String> {
        use pi_link::protocol::AssistantEvent as E;
        match ev {
            E::TextDelta {
                content_index,
                delta,
            } => {
                if self.cur_ix != Some(*content_index) {
                    self.cur_ix = Some(*content_index);
                }
                self.cur_text.push_str(delta);
                Vec::new()
            }
            E::TextEnd {
                content_index,
                content,
            } => {
                self.cur_ix = Some(*content_index);
                self.cur_text = content.clone();
                Vec::new()
            }
            E::ToolCallStart { .. } => self.flush(),
            _ => Vec::new(),
        }
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
    /// 打开左列「微信」面板（titlebar 手机钮已绑路径 / automation
    /// `panel.dock wx`）。面板列收起时先展开——否则切了 dock 也看不见。
    pub(crate) fn open_wx_panel(&mut self, cx: &mut gpui::Context<Self>) {
        self.layout.panes_hidden = false;
        self.layout.dock_panel = crate::DockPanel::Wx;
        cx.notify();
    }

    /// 收一批 → 逐条解析并执行 → **整批处理完才 ack 游标**。
    pub(crate) fn pump_wx(&mut self, cx: &mut gpui::Context<Self>) {
        self.aux.remote.ensure_transport();
        // 显式启用（设置页开关 / wx.enable / 扫码成功）→ 泵这一拍统一执行；
        // 200ms 延迟不可感，换来测试可断言意图而不真起线程
        if self.aux.remote.wants_start && !self.aux.remote.is_running() {
            self.aux.remote.wants_start = false;
            self.aux.remote.start_transport_from_state();
        }
        // 微信默认会话认领（幂等）：启用后第一拍即有归属，重启后从 state 恢复
        self.aux.remote.claim_default_session(&self.pool.active_key);
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
        for text in self.aux.remote.on_running(running) {
            self.aux.remote.send(&text);
        }
        // 设置页扫码结果 —— **成功即关弹窗落回工作界面**（ZCode 扫完就进
        // 工作台选会话；留个弹窗把人挡在外面是错的）
        if self.aux.remote.poll_qr() {
            if matches!(self.aux.remote.qr, QrState::Done { .. })
                && matches!(self.dlg.dialog, Some(crate::Dialog::WxQr))
            {
                self.dlg.dialog = None;
            }
            // 扫码成功 → 渠道起（wants_start 置位，本拍上方统一执行；
            // 不受 SELF_START 门控，不用再摸设置页开关）
            if matches!(self.aux.remote.qr, QrState::Done { .. }) {
                self.aux.remote.wants_start = true;
            }
            cx.notify();
        }
        let batches = self.aux.remote.take_batches();
        if batches.is_empty() {
            return;
        }
        for batch in &batches {
            for m in &batch.messages {
                let action = self.aux.remote.route(&m.text);
                self.run_wx_action(action, cx);
            }
            self.aux.remote.ack(batch);
        }
        cx.notify();
    }

    fn run_wx_action(&mut self, action: Action, cx: &mut gpui::Context<Self>) {
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
                self.reply_status(cx);
            }
            menu::MenuKind::Project => {
                // 菜单载荷就是路径；直接 `/项目 <名字>` 时按名字反查
                let path = self
                    .psp
                    .projects
                    .iter()
                    .find(|p| p.path.to_string_lossy() == value || p.name == value)
                    .map(|p| p.path.clone());
                if let Some(p) = path {
                    self.switch_project(p, cx);
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
        let rt = match self.pool.runtimes.get(&key).cloned() {
            Some(rt) => rt,
            None => {
                // 目标会话（微信线）已被删 → 清归属、重新认领活跃，重试一次。
                // 静默丢弃已经坑过一次（微信没回执），这里必须自愈。
                self.aux.remote.clear_wx_session_if(&key);
                self.aux.remote.claim_default_session(&self.pool.active_key);
                let fallback = self.aux.remote.target_key(&self.pool.active_key);
                match self.pool.runtimes.get(&fallback).cloned() {
                    Some(rt) => rt,
                    None => {
                        eprintln!("[wx] 命令未送达：没有 key = {key} 的会话，且无活跃会话可兜底");
                        return;
                    }
                }
            }
        };
        let sent = rt.update(cx, |r, cx| match cmd {
            // 文本要走「没进程先 spawn」，否则草稿会话永远收不到
            Command::Prompt { message, .. } => r.send_remote_text(message.clone(), cx),
            _ => r.send_remote_command(cmd, cx),
        });
        if !sent {
            eprintln!("[wx] 命令未送达（目标会话没有 pi 进程或正在压缩）");
        }
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

/// 060 微信 Bot：扫码弹窗（原手机图标入口已隐藏 2026-10-10，自动化
/// WX_QR_OPEN 仍可打开）。渲染自 dialogs.rs 内切迁入（check_arch 文件
/// 1500 行上限）——弹窗本体就是微信 Bot 域的 UI，归位。
///
/// **窗体复用设置弹窗那套大卡片**（`ui::overlay::big_card`）—— top_panels 的
/// 头注写明「不要再自造窗体」，这里照做：`layer` 负责遮挡/外点/ESC，
/// `big_card` 负责 chrome 顶条 + × + 0.7×0.98 尺寸。
///
/// **不持数据** —— 内容全从 `chat.aux.remote.qr` 现读，扫码 worker 的事件由 200ms
/// 泵 drain 后 `cx.notify()`，弹窗随之重绘。
pub(crate) fn render_wx_qr(
    chat: &crate::Chat,
    weak: &gpui::WeakEntity<crate::Chat>,
    t: &theme::Theme,
) -> gpui::AnyElement {
    use crate::remote_control::panel::mono_lines;

    let body_text = |text: String, dim: bool| -> gpui::AnyElement {
        div()
            .text_size(crate::appearance::ui_size(12.))
            .text_color(rgb(if dim { t.text_dim } else { t.text }))
            .child(SharedString::from(text))
            .into_any_element()
    };
    let action_btn = |id: &'static str, label: &'static str, accent: bool, go: bool| {
        let weak_b = weak.clone();
        div()
            .id(id)
            .px(px(12.))
            .py(px(6.))
            .rounded(px(7.))
            .text_size(crate::appearance::ui_size(12.))
            .cursor_pointer()
            .when(accent, |d| {
                d.bg(rgb(t.accent)).text_color(rgb(t.accent_contrast))
            })
            .when(!accent, |d| d.border_1().border_color(rgb(t.border)))
            .hover(|s| s.bg(rgb(t.bg_hover)))
            .child(SharedString::from(label.to_string()))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = weak_b.update(cx, |c, cx| {
                    if go {
                        c.aux.remote.begin_qr(); // 幂等：已在扫码/已出码时不重发
                    } else {
                        c.dlg.dialog = None;
                    }
                    cx.notify();
                });
            })
            .into_any_element()
    };

    // QR 用**实心方块**画，不用字符画：字形抗锯齿会在模块之间留细缝，
    // 手机容易解不出来（用户实测扫不上）。按行把连续深色段合并成一个方块，
    // 49x49 的网格约 600 个元素，不是 2401 个。
    let qr_grid = |url: &str| -> gpui::AnyElement {
        const MOD: f32 = 8.; // 8px/模块；49 模块 = 392px，正好塞进 0.7 宽大卡片
        const DARK: u32 = 0x111827;
        let grid = match wxprobe::qr::qr_grid(url, 4) {
            Ok(g) => g,
            Err(e) => return body_text(tf("remote.qr_gen_failed", &[("e", e.to_string())]), false),
        };
        let side = grid.size as f32 * MOD;
        let mut canvas = div()
            .w(px(side))
            .h(px(side))
            .flex_shrink_0()
            .bg(rgb(0xff_ffff))
            .flex()
            .flex_col();
        for y in 0..grid.size {
            let mut row = div().h(px(MOD)).flex().flex_row().flex_shrink_0();
            let mut cursor = 0usize;
            for (x, len) in grid.runs(y) {
                if x > cursor {
                    row = row.child(div().w(px((x - cursor) as f32 * MOD)).h(px(MOD)));
                }
                row = row.child(div().w(px(len as f32 * MOD)).h(px(MOD)).bg(rgb(DARK)));
                cursor = x + len;
            }
            if cursor < grid.size {
                row = row.child(div().w(px((grid.size - cursor) as f32 * MOD)).h(px(MOD)));
            }
            canvas = canvas.child(row);
        }
        div()
            .flex()
            .justify_center()
            .child(canvas)
            .into_any_element()
    };

    let mut body = div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(12.))
        .py(px(8.))
        .child(body_text(tr("remote.dlg_title").into(), true));

    match &chat.aux.remote.qr {
        QrState::Idle => {
            body = body.child(body_text(tr("remote.dlg_idle_hint").into(), true));
            body = body.child(action_btn(
                "wx-qr-begin",
                tr("remote.dlg_qr_begin"),
                true,
                true,
            ));
        }
        QrState::Loading => body = body.child(body_text(tr("remote.qr_loading").into(), true)),
        QrState::Ready { url } => {
            body = body.child(qr_grid(url));
            body = body.child(body_text(tr("remote.dlg_qr_hint").into(), true));
            body = body.child(action_btn(
                "wx-qr-refresh",
                tr("remote.dlg_qr_refresh"),
                false,
                true,
            ));
        }
        QrState::Scanned => body = body.child(body_text(tr("remote.qr_scanned").into(), true)),
        QrState::Done { bot_id } => {
            let mut lines = vec![tr("remote.qr_done").to_string()];
            if let Some(b) = bot_id {
                lines.push(format!("bot_id = {b}"));
            }
            body = body.child(
                div()
                    .flex()
                    .justify_center()
                    .child(mono_lines(lines, 12., false))
                    .into_any_element(),
            );
            body = body.child(body_text(tr("remote.dlg_done_hint").into(), true));
        }
        QrState::Expired => {
            body = body.child(body_text(tr("remote.qr_expired").into(), false));
            body = body.child(action_btn("wx-qr-again", tr("remote.qr_begin"), true, true));
        }
        QrState::Error(m) => {
            body = body.child(body_text(
                format!("{}", tr("remote.dlg_error")).replace("{m}", m),
                false,
            ));
            body = body.child(action_btn(
                "wx-qr-retry",
                tr("remote.dlg_retry"),
                false,
                true,
            ));
        }
    }

    // 绑定码：微信里 `/bind <code>` 用
    let code = chat.aux.remote.bind_code(&chat.pool.active_key);
    // 上面的「绑定成功」说的是**渠道**（扫码授权）；这里说的是 `/bind`，
    // 两者别混 —— 用户被「已绑定/未绑定」并排显示搞糊涂过
    let bind_line = match chat.aux.remote.bound.as_deref() {
        Some(k) => tr("remote.dlg_bind_confirmed").replace("{k}", k),
        None => tr("remote.dlg_bind_optional").replace("{code}", &code),
    };
    body = body.child(body_text(bind_line, true));
    body = body.child(action_btn(
        "wx-qr-close",
        tr("remote.dlg_close"),
        false,
        false,
    ));

    let weak_dismiss = weak.clone();
    let dismiss = move |_w: &mut gpui::Window, cx: &mut gpui::App| {
        let _ = weak_dismiss.update(cx, |c, cx| {
            c.dlg.dialog = None;
            cx.notify();
        });
    };
    crate::ui::overlay::layer(true, Some(&chat.dlg.dialog_focus), dismiss.clone())
        .flex()
        .items_center()
        .justify_center()
        .child(crate::ui::overlay::big_card(
            tr("remote.dlg_title"),
            None,
            body.into_any_element(),
            t,
            dismiss,
        ))
        .into_any_element()
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
        // 首条普通消息 = 激活回执，不进模型（ZCode handleWeixinFirstActivation）
        assert!(matches!(rc.route("你好"), Action::Send(_)));
        rc.activated = true;
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
        // 2026-10-22 起：开 = 立即走显式路径（无 token 时起不来，但不再
        // 依赖泵下一拍的 ensure_transport——那条路被 SELF_START 门控死了）。
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
        rc.activated = true; // 跳过首条消息的激活回执，直接看路由
        assert!(matches!(rc.route("1"), Action::Prompt(_)));
    }

    // ── 首条消息激活（ZCode botsService.ts:957）────────────────────────

    /// 源码注释的原话：扫码只给 bot token/id、**不给投递目标**；
    /// 第一条入站消息用于建立目标，只回欢迎+帮助，**不把文本当 prompt**。
    #[test]
    fn first_plain_message_activates_instead_of_prompting() {
        let mut rc = RemoteControl::new();
        assert!(!rc.activated);
        match rc.route("你好") {
            Action::Send(text) => {
                assert!(
                    text.contains("已激活"),
                    "激活回执要带 weixinActivatedWelcome，实际: {text}"
                );
                assert!(
                    text.contains("/帮助"),
                    "要跟 ZCode 一样把 helpText 拼在欢迎后面，实际: {text}"
                );
            }
            other => panic!("首条普通消息应回激活文案，实际 {other:?}"),
        }
        // 只回这一次：运行期内部标记 + 后续正常进模型
        assert!(rc.activated);
        assert!(matches!(rc.route("第二条"), Action::Prompt(_)));
        // 激活只拦普通消息，命令照常分发
        let mut rc2 = RemoteControl::new();
        assert!(matches!(rc2.route("/帮助"), Action::Send(_)));
        // 命令不消耗激活：等第一条普通消息仍然欢迎
        assert!(!rc2.activated);
    }

    /// 活跃会话在跑时 `/task` 既不给列表也不许切换（ZCode 两处 Bugfix）。
    #[test]
    fn task_menu_is_refused_while_running() {
        let mut rc = RemoteControl::new();
        rc.activated = true;
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
        assert!(rc.on_assistant(&td(0, "你好")).is_empty());
        assert!(rc.on_assistant(&td(0, "，我是")).is_empty());
        assert!(rc.on_assistant(&td(0, "助手")).is_empty());
        assert_eq!(rc.cur_text, "你好，我是助手");
        assert!(rc.buf.is_empty());
    }

    #[test]
    fn text_end_overrides_accumulated_deltas() {
        // TextEnd 是权威内容，用来纠正丢 delta；但不能清掉上一块
        let mut rc = RemoteControl::new();
        rc.on_assistant(&td(0, "块一"));
        rc.on_assistant(&te(0, "块一（修正）"));
        assert_eq!(rc.cur_text, "块一（修正）");
        // 下一块的 delta 累加在后面，不被 TextEnd 抹掉
        rc.on_assistant(&td(1, "块二"));
        assert_eq!(rc.cur_text, "块一（修正）块二");
    }

    #[test]
    fn tool_call_start_force_flushes() {
        // 真正的发送边界之一 = tool_call（§6 坑1）
        let mut rc = RemoteControl::new();
        rc.on_assistant(&td(0, "先跑个命令"));
        let out = rc.on_assistant(&tool_start());
        assert_eq!(out, vec!["先跑个命令".to_string()]);
        assert!(rc.cur_text.is_empty());
        assert!(rc.buf.is_empty(), "flush 后缓冲必须清空");
    }

    #[test]
    fn idle_flush_drains_remaining_text_once() {
        let mut rc = RemoteControl::new();
        rc.on_assistant(&td(0, "最终答复"));
        assert!(rc.on_running(true).is_empty(), "运行中不发");
        assert_eq!(rc.on_running(false), vec!["最终答复".to_string()]);
        assert!(rc.on_running(false).is_empty(), "只发一次，不重复");
    }

    #[test]
    fn nothing_is_sent_from_an_empty_buffer() {
        let mut rc = RemoteControl::new();
        assert!(rc.on_running(false).is_empty());
        assert!(rc.on_assistant(&tool_start()).is_empty());
        rc.on_assistant(&td(0, "   "));
        assert!(rc.on_running(false).is_empty(), "纯空白不发");
    }

    #[test]
    fn long_reply_is_split_into_chunks() {
        let mut rc = RemoteControl::new();
        let big: String = "很".repeat(9000);
        rc.on_assistant(&td(0, &big));
        let out = rc.on_running(false);
        assert!(out.len() >= 3, "9000 字应切成多段，实际 {}", out.len());
        assert!(out.iter().all(|s| s.chars().count() <= 4000));
    }
}
