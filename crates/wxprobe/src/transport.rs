//! 微信渠道传输：把 [`crate::poller`] 的长轮询包成 app 可直接消费的双向通道。
//!
//! **结构** —— 对齐 ZCode：JS 单线程下 90s 长轮询 `await` 期间
//! `sendMessage` 仍能并发发出，等价于 Rust 这里**两条线程各持一个 `Wire`**：
//!
//! ```text
//! [poll 线程]  getupdates(90s) ──► inbound: Sender<Batch>
//! [send 线程]  outbound: Receiver<String> ──► sendmessage
//! ```
//!
//! 若只用一条线程，出站消息最多要等 90s 长轮询结束才能发出 —— 用户在微信里回的
//! 每一句都会卡住，双向交互（菜单/扩展 UI）形同虚设。
//!
//! **游标（§6 坑 2「处理完再写」）**：线程内只在**内存**里推进 cursor，
//! 落盘由 [`Transport::ack`] 触发 —— app 处理完一批才 ack。进程中途崩溃则
//! 下次启动从上次 ack 的游标重拉，**可能重复、绝不丢失**（at-least-once）。
//! 反过来「先写游标再回复」正是 ZCode 注释里点名的坑：失败会跳过未完成消息。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::Value;

use crate::lock::BotRuntimeLock;
use crate::poller::{self, Inbound};
use crate::state;
use crate::wire::Wire;

/// 一次 `getupdates` 拉回的消息批。
///
/// **整批一起交付**：`next_buf` 是这批处理完之后应落盘的游标，
/// app 必须在**全部处理完**后再 [`Transport::ack`]，否则崩溃时会跳过未处理的消息。
#[derive(Debug)]
pub struct Batch {
    pub messages: Vec<Inbound>,
    pub next_buf: Option<String>,
}

/// 连接微信所需的全部参数 —— 从状态文件解出，**与 IO 解耦**（可单测）。
#[derive(Debug, Clone)]
pub struct Config {
    pub token: String,
    pub bot_id: String,
    /// `sendmessage` 的 `from_user_id`（bot 自己）
    pub bot_user: String,
    /// `to_user_id`（聊天）；None = 还没收到过任何消息，无从回发
    pub chat_user: Option<String>,
    pub context_token: Option<String>,
    pub client_id: String,
}

/// 状态文件缺字段时的确定性错误（供 [`config_from_state`] 与单测共用）。
pub fn require_token(state: &Value) -> Result<String, String> {
    state::get_str(state, "bot_token")
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| "未扫码：先跑 `wxprobe qr` + `wxprobe scan`".to_string())
}

/// 从 `~/.pi-flash/wxprobe-state.json` 解出连接配置。
pub fn config_from_state(state: &Value) -> Result<Config, String> {
    let token = require_token(state)?;
    Ok(Config {
        bot_id: state::get_str(state, "bot_id").unwrap_or_default(),
        bot_user: state::get_str(state, "bot_user_id").unwrap_or_default(),
        chat_user: state::get_str(state, "chat_id"),
        context_token: state::get_str(state, "context_token"),
        client_id: state::ensure_client_id(&mut state.clone()),
        token,
    })
}

/// 活着的渠道。`drop` 即停线程并释放轮询锁。
pub struct Transport {
    inbound: Receiver<Batch>,
    outbound: Sender<String>,
    stop: Arc<AtomicBool>,
    handles: Vec<JoinHandle<()>>,
    /// 释放轮询锁（单持有者，放在结构体里随 drop 释放）
    _lock: BotRuntimeLock,
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
        // _lock 在此 drop → 心跳停、租约目录拆除
    }
}

impl Transport {
    /// 非阻塞取一批入站消息；空则 `Err(TryRecvError)`。
    pub fn try_recv(&self) -> Result<Batch, TryRecvError> {
        self.inbound.try_recv()
    }

    /// 发一条文本到微信 —— 只入队，实际 HTTP 在 send 线程里跑，**绝不阻塞主线程**。
    pub fn send(&self, text: &str) -> Result<(), String> {
        self.outbound
            .send(text.to_string())
            .map_err(|_| "send 线程已退出".to_string())
    }

    /// 批次处理完后落盘游标（§6 坑 2：**处理完才写**）。
    ///
    /// 持久化在**调用方线程**同步完成 —— 这样崩溃窗口只存在于
    /// 「收到底批 → ack」之间，且方向是重复而非丢失。
    pub fn ack(&self, batch: &Batch) {
        let mut st = state::load();
        let mut dirty = false;
        if let Some(buf) = batch.next_buf.as_deref() {
            state::set(&mut st, "buf", Some(buf));
            dirty = true;
        }
        // **路由字段**：出站 sendmessage 靠 chat_id 找对端、靠 bot_user_id 当
        // from_user_id。P1 的 `wxprobe recv` 会写这两个，但 app 侧的入站路径
        // 一直没写 —— 结果是 send 线程每次读到 `chat_id = None` 就 `continue`，
        // 把所有出站**静默丢掉**（真机复现：消息进了 pi-flash，微信收不到回执）。
        // 这里随 ack 一起回写，写在「处理完」这一步也符合 §6 坑 2 的时机。
        if let Some(m) = batch.messages.first() {
            if m.chat_id.is_some() {
                state::set(&mut st, "chat_id", m.chat_id.as_deref());
                dirty = true;
            }
            if !m.user_id.is_empty() {
                state::set(&mut st, "user_id", Some(&m.user_id));
                dirty = true;
            }
            if let Some(b) = m.bot_user_id.as_deref().filter(|s| !s.is_empty()) {
                state::set(&mut st, "bot_user_id", Some(b));
                dirty = true;
            }
            if let Some(c) = m.context_token.as_deref().filter(|s| !s.is_empty()) {
                state::set(&mut st, "context_token", Some(c));
                dirty = true;
            }
        }
        if dirty {
            state::save(&st);
        }
    }

    /// 通道是否已断（send/poll 线程退出）。
    pub fn is_closed(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

/// 起渠道。失败（未扫码 / 已被别的轮询者持有锁）时不产生任何线程。
///
/// `cfg.chat_user` 为 None 时 poll 线程仍会收消息；首条入站会把 `chat_id` /
/// `bot_user_id` 写回状态文件，**send 线程每次发送前重读状态**，因此无须重启。
pub fn spawn(cfg: &Config) -> Result<Transport, String> {
    let st = state::load();
    let bot_id = cfg.bot_id.clone();
    let lock = match crate::lock::acquire("weixin-polling", &cfg.token, &bot_id)? {
        Some(l) => l,
        None => {
            return Err(format!(
                "另一个轮询者已持有该 bot 的锁，见 {}",
                crate::lock::lock_root_display()
            ))
        }
    };

    let stop = Arc::new(AtomicBool::new(false));
    let (in_tx, in_rx) = mpsc::channel::<Batch>();
    let (out_tx, out_rx) = mpsc::channel::<String>();

    // ── poll 线程 ──
    let p_stop = stop.clone();
    let p_token = cfg.token.clone();
    let start_cursor = state::get_str(&st, "buf");
    let poll_handle = std::thread::Builder::new()
        .name("wx-poll".into())
        .spawn(move || poll_loop(p_token, start_cursor, in_tx, p_stop))
        .map_err(|e| format!("起 poll 线程失败: {e}"))?;

    // ── send 线程 ──
    let s_stop = stop.clone();
    let send_handle = std::thread::Builder::new()
        .name("wx-send".into())
        .spawn(move || send_loop(out_rx, s_stop))
        .map_err(|e| format!("起 send 线程失败: {e}"))?;

    Ok(Transport {
        inbound: in_rx,
        outbound: out_tx,
        stop,
        handles: vec![poll_handle, send_handle],
        _lock: lock,
    })
}

/// 长轮询主循环。错误退避重试，**不退出**（§6 坑 4：一次失败退出 = bot 静默死掉）。
fn poll_loop(token: String, mut cursor: Option<String>, tx: Sender<Batch>, stop: Arc<AtomicBool>) {
    let mut wire = Wire::new(state::dump_dir());
    let mut failures = 0u32;
    while !stop.load(Ordering::Relaxed) {
        match poller::get_updates(&mut wire, &token, cursor.as_deref()) {
            Ok(up) => {
                failures = 0;
                // 游标只在内存推进；落盘交给 app 的 ack()
                cursor = up.next_buf.clone();
                if !up.messages.is_empty() && tx.send(Batch {
                    messages: up.messages,
                    next_buf: up.next_buf,
                }).is_err() {
                    break; // 消费方已亡
                }
            }
            Err(e) => {
                failures = failures.saturating_add(1);
                // 指数退避，封顶 30s；每次只睡一小段以便及时响应 stop
                let wait = Duration::from_millis(500u64 << failures.min(6)).min(Duration::from_secs(30));
                let _ = e; // 诊断串落盘见 Wire；此处只按退避重试
                let mut left = wait;
                while left > Duration::ZERO && !stop.load(Ordering::Relaxed) {
                    let step = Duration::from_millis(100).min(left);
                    std::thread::sleep(step);
                    left = left.saturating_sub(step);
                }
            }
        }
    }
}

/// 同类问题只提示一次（每条都刷会刷屏，一次不刷则永远查不出来）。
fn warn_once(msg: &str) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static ROUTE: AtomicBool = AtomicBool::new(false);
    static SEND: AtomicBool = AtomicBool::new(false);
    let flag = if msg.starts_with("sendmessage") { &SEND } else { &ROUTE };
    if !flag.swap(true, Ordering::Relaxed) {
        eprintln!("[wx] {msg}（此提示只出现一次）");
    }
}

/// 发送主循环：阻塞在 `out_rx` 上，收到即发，**不与长轮询抢线程**。
fn send_loop(rx: Receiver<String>, stop: Arc<AtomicBool>) {
    let mut wire = Wire::new(state::dump_dir());
    while !stop.load(Ordering::Relaxed) {
        // 100ms 轮询退出标志，避免 drop 后线程挂死在 recv 上
        let text = match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(t) => t,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        // 每次发送前重读状态：chat_id / bot_user_id 可能在首条入站后才被发现
        let st = state::load();
        let (Some(to_user), Some(from_user)) = (
            state::get_str(&st, "chat_id"),
            state::get_str(&st, "bot_user_id"),
        ) else {
            // **必须出声**：静默丢弃正是「微信收不到回执」迟迟查不出的原因
            // （真机只表现为「没反应」）。
            warn_once("出站消息被丢弃：状态文件缺 chat_id / bot_user_id（首条入站后随 ack 回写）");
            continue;
        };
        let client_id = state::ensure_client_id(&mut state::load());
        let context = state::get_str(&st, "context_token");
        let token = match require_token(&st) {
            Ok(t) => t,
            Err(_) => break,
        };
        if let Err(e) = poller::send_text(
            &mut wire,
            &token,
            &from_user,
            &to_user,
            &text,
            context.as_deref(),
            &client_id,
        ) {
            // 发送失败同样不能吞：对用户来说就是「发了没反应」
            warn_once(&format!("sendmessage 失败：{e}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn token_must_be_non_empty() {
        let empty = json!({ "bot_token": "" });
        assert!(require_token(&empty).is_err(), "空 token 视同未扫码");
        let ws = json!({ "bot_token": "   " });
        assert!(require_token(&ws).is_err(), "全空白也视为未扫码");
        let ok = json!({ "bot_token": "tk" });
        assert_eq!(require_token(&ok).unwrap(), "tk");
        let missing = json!({});
        assert!(
            require_token(&missing).unwrap_err().contains("wxprobe qr"),
            "错误信息要指路"
        );
    }

    #[test]
    fn batch_carries_cursor_for_ack() {
        // 游标语义：整批共用一个 next_buf，ack 落盘的就是它
        let b = Batch {
            messages: vec![],
            next_buf: Some("CURSOR-1".into()),
        };
        assert_eq!(b.next_buf.as_deref(), Some("CURSOR-1"));
        let b2 = Batch {
            messages: vec![],
            next_buf: None,
        };
        assert!(b2.next_buf.is_none(), "服务端没给游标时不落盘（沿用旧值）");
    }
}
