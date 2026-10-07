//! 远程控制 —— 微信 ⇄ 会话 桥（`docs/模块设计/060-远程控制-微信.md` §3、§4.1）。
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
//! **会话归属（1b 的已知简化）**：入站一律投给**当前活跃会话**
//! （`active_key`）。ZCode 是「一 bot 一 context + `/绑定 <code>` 选 workspace /
//! 会话」，§4.2 的 `active_session` 要等 P3 步骤 3 的绑定码落地后才精确。

use pi_link::protocol::{Command, ExtensionUiRequest};

use wxprobe::command::BotCommand;
use wxprobe::extui::{self, ExtUiReply};
use wxprobe::format::messages::{t, Lang};

/// 档 1 尚未接入的命令回执 —— ZCode 文案表里的现成句子，不新造。
const CMD_NOT_ENABLED: &str = "当前 bot 未启用这个命令。";

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

/// 微信渠道句柄 + 待答请求。挂在 `Chat` 上，由 [`spawn_wx_pump`] 驱动。
pub struct RemoteControl {
    transport: Option<wxprobe::transport::Transport>,
    /// 一个 app 运行期内只试一次（P4 加开关与刷新）
    boot_attempted: bool,
    pending: Option<Pending>,
}

impl RemoteControl {
    pub fn new() -> Self {
        Self {
            transport: None,
            boot_attempted: false,
            pending: None,
        }
    }

    /// 惰性起渠道。没扫码 / 被别的轮询者持锁都只是**记一条日志**，
    /// 不 panic 不重试 —— 桌面端该干什么还干什么。
    fn ensure_transport(&mut self) {
        if self.boot_attempted {
            return;
        }
        self.boot_attempted = true;
        let st = wxprobe::state::load();
        let started = wxprobe::transport::config_from_state(&st)
            .and_then(|cfg| wxprobe::transport::spawn(&cfg));
        match started {
            Ok(t) => self.transport = Some(t),
            // 静默失败是最坏的形态（§6 坑 4），至少打一行
            Err(e) => eprintln!("[wx] 渠道未启动：{e}"),
        }
    }

    fn send(&self, text: &str) {
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
        match wxprobe::command::parse_bot_command(text) {
            BotCommand::Message { text: msg } => Action::Prompt(msg),
            BotCommand::Stop => Action::Abort,
            // `0` 在没有待答菜单时无意义，静默忽略（不打扰模型）
            BotCommand::SelectionCancel => Action::None,
            // 档 1 只接 Prompt/Abort；其余等 pipeline（P3 步骤 2-4）
            _ => Action::Send(t(lang, CMD_NOT_ENABLED)),
        }
    }
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
    /// 收一批 → 逐条解析并执行 → **整批处理完才 ack 游标**。
    pub(crate) fn pump_wx(&mut self, cx: &mut gpui::Context<Self>) {
        self.remote.ensure_transport();
        let batches = self.remote.take_batches();
        if batches.is_empty() {
            return;
        }
        for batch in &batches {
            for m in &batch.messages {
                let action = self.remote.route(&m.text);
                self.run_wx_action(action, cx);
            }
            self.remote.ack(batch);
        }
        cx.notify();
    }

    fn run_wx_action(&mut self, action: Action, cx: &mut gpui::Context<Self>) {
        match action {
            Action::None => {}
            Action::Send(text) => self.remote.send(&text),
            Action::Prompt(message) => self.send_wx(
                &Command::Prompt {
                    message,
                    images: Vec::new(),
                },
                cx,
            ),
            Action::Abort => self.send_wx(&Command::Abort, cx),
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
    fn send_wx(&self, cmd: &Command, cx: &gpui::App) {
        let Some(rt) = self.runtimes.get(&self.active_key) else {
            return;
        };
        if let Some(session) = &rt.read(cx).agent.session {
            let _ = session.send(cmd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 档 1 的命令覆盖面：只有 /停止 与纯文本真正接线，
    /// 其余一律回 ZCode 现成的「未启用」句，**不静默吞掉**。
    #[test]
    fn only_prompt_and_abort_are_wired_in_tier1() {
        let mut rc = RemoteControl::new();
        assert!(matches!(rc.route("帮我看看这个报错"), Action::Prompt(_)));
        assert!(matches!(rc.route("/停止"), Action::Abort));
        assert!(matches!(rc.route("/stop"), Action::Abort));
        assert!(matches!(rc.route("0"), Action::None));
        match rc.route("/状态") {
            Action::Send(s) => assert_eq!(s, "当前 bot 未启用这个命令。"),
            other => panic!("未接入的命令必须回执，不能静默：{other:?}"),
        }
        match rc.route("/帮助") {
            Action::Send(_) => {}
            other => panic!("同上：{other:?}"),
        }
    }

    /// 没有 pending 时，ExtUi 回填不该被触发。
    #[test]
    fn no_pending_means_plain_command_routing() {
        let mut rc = RemoteControl::new();
        assert!(rc.pending.is_none());
        assert!(matches!(rc.route("1"), Action::Prompt(_)));
    }
}
