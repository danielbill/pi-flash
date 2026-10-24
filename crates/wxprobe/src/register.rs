//! 扫码注册（P0 步骤 1/2）：`get_bot_qrcode` → `get_qrcode_status`。
//!
//! 对齐 ZCode `providers/weixinRegistration.ts`：
//! - `GET /ilink/bot/get_bot_qrcode?bot_type=3`（`:132`）
//! - `GET /ilink/bot/get_qrcode_status?qrcode=<enc>`，3s 间隔轮询（`:146-157`）
//! - 状态归一：数字 0/1/2/3|4 与字符串词表两套（`:103-130`）

use serde_json::Value;

use crate::wire::{Wire, first_str, num, str_of};

pub struct QrBegin {
    pub qrcode: String,
    /// 可能是 URL、data URI、base64 图片，也可能就是二维码内容本身——
    /// 真实形态由 dump 落盘后确认（P0 产出之一）。
    pub qr_url: String,
    pub interval_secs: u64,
    pub expires_at_ms: u64,
}

pub fn begin(wire: &mut Wire) -> Result<QrBegin, String> {
    let payload = wire.get_qr("qr-begin", "/get_bot_qrcode", &[("bot_type", "3")])?;
    let qrcode = first_str(&payload, &["qrcode", "qr_code"]).ok_or_else(|| {
        format!(
            "扫码接口未返回 qrcode，真实字段：{}",
            crate::wire::to_pretty(&payload)
        )
    })?;
    let qr_url = first_str(&payload, &["qrcode_img_content", "qrcode_url"])
        .unwrap_or_else(|| qrcode.clone());
    let interval = num(&payload, "expires_in").map(|v| v as u64);
    let expires_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
        + interval.unwrap_or(120) * 1000;
    Ok(QrBegin {
        qrcode,
        qr_url,
        interval_secs: 3,
        expires_at_ms,
    })
}

/// 设置页扫码流程发出的事件（跑在专用线程，app 侧非阻塞地 drain）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QrEvent {
    /// 拿到二维码 —— `url` 是 `qrcode_img_content`（要拿去编码成二维码）
    Ready {
        url: String,
    },
    /// 已扫码，等待手机确认
    Scanned,
    /// 扫码成功：token 已写进状态文件
    Success {
        bot_id: Option<String>,
    },
    Expired,
    Error(String),
}

/// 扫码全流程（阻塞，**跑在专用线程**）：`begin` → 每 3s `poll` → 落状态。
///
/// 与 `wxprobe qr` + `wxprobe scan` 用的是同一条路径、同一份状态文件，
/// 所以设置页扫完码 CLI 工具立刻也能用，反之亦然。
///
/// `deadline_sec` = 超时秒数（ZCode/微信二维码约 120s 有效）。
pub fn run_qr_flow(tx: std::sync::mpsc::Sender<QrEvent>, deadline_sec: u64) {
    let mut w = crate::wire::Wire::new(crate::state::dump_dir());
    let begin = match begin(&mut w) {
        Ok(b) => b,
        Err(e) => {
            let _ = tx.send(QrEvent::Error(e));
            return;
        }
    };
    if tx
        .send(QrEvent::Ready {
            url: begin.qr_url.clone(),
        })
        .is_err()
    {
        return;
    }

    let started = std::time::Instant::now();
    let mut st = crate::state::load();
    crate::state::set(&mut st, "qrcode", Some(&begin.qrcode));
    crate::state::save(&st);

    loop {
        match poll(&mut w, &begin.qrcode) {
            Ok(QrStatus::Success { bot_token, bot_id }) => {
                crate::state::set(&mut st, "bot_token", Some(&bot_token));
                crate::state::set(&mut st, "bot_id", bot_id.as_deref());
                // sendmessage 的 from_user_id 就是 bot 自己。换绑后不跟着改，
                // 出站会带着**旧 bot** 的 id（真机复现过：84e144 → 7240ca 没同步）
                crate::state::set(&mut st, "bot_user_id", bot_id.as_deref());
                reset_session_fields(&mut st);
                crate::state::set(&mut st, "qrcode", None);
                crate::state::save(&st);
                let _ = tx.send(QrEvent::Success { bot_id });
                return;
            }
            Ok(QrStatus::Expired) => {
                crate::state::set(&mut st, "qrcode", None);
                crate::state::save(&st);
                let _ = tx.send(QrEvent::Expired);
                return;
            }
            Ok(QrStatus::Scanned) => {
                // 接收端被丢（UI「停止」= cancel_qr）→ 清 qrcode 后退出；
                // 不检查的话 worker 会空转到 180s 死线（2026-10-24 页面重构
                // 加的取消语义，与开头 Ready send 的 is_err 同约定）
                if tx.send(QrEvent::Scanned).is_err() {
                    crate::state::set(&mut st, "qrcode", None);
                    crate::state::save(&st);
                    return;
                }
            }
            Ok(QrStatus::Pending) => {}
            // 状态接口偶发报错不致命，退避后继续（§6 坑 4：不能让循环退出）；
            // 接收端已丢（UI 取消）时同样退出
            Ok(QrStatus::Error(e)) | Err(e) => {
                if tx.send(QrEvent::Error(e)).is_err() {
                    crate::state::set(&mut st, "qrcode", None);
                    crate::state::save(&st);
                    return;
                }
            }
        }
        if started.elapsed().as_secs() >= deadline_sec {
            crate::state::set(&mut st, "qrcode", None);
            crate::state::save(&st);
            let _ = tx.send(QrEvent::Expired);
            return;
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 词表外字符串状态必须带安装指引透传（2026-10-24：用户要求识别
    /// 「微信未装插件」并告知，不能再吞成无限等待）。
    #[test]
    fn unknown_status_messages_carry_install_guidance() {
        // 含 install 关键词 → 直接判定未安装并给行动指引
        let m = unknown_status_message(Some(&json!("not_install"))).unwrap();
        assert!(m.contains("未安装") && m.contains("ClawBot"), "{m}");
        // 其它未知字符串 → 原文透传 + 同样的指引（首次出现即被看见）
        let m = unknown_status_message(Some(&json!("weird_state"))).unwrap();
        assert!(m.contains("weird_state") && m.contains("未安装"), "{m}");
        // 数字码与缺失 → 不产生错误（ZCode 语义归 pending）
        assert!(unknown_status_message(Some(&json!(5))).is_none());
        assert!(unknown_status_message(None).is_none());
    }

    #[test]
    fn reset_clears_old_bot_routing_fields() {
        let mut st = json!({
            "bot_token": "new-token",
            "bot_id": "new@im.bot",
            "buf": "OLD-BOT-CURSOR",
            "chat_id": "room@im.wechat",
            "user_id": "u@im.wechat",
            "context_token": "CT",
            "session_key": "C:\\sess\\a.jsonl",
        });
        reset_session_fields(&mut st);
        for k in ["buf", "chat_id", "user_id", "context_token"] {
            assert!(
                st.get(k).map_or(true, Value::is_null),
                "{k} 必须被清掉——留着就是旧 bot 会话的游标/对端"
            );
        }
        assert_eq!(st["session_key"], "C:\\sess\\a.jsonl", "PF 侧会话归属保留");
        assert_eq!(st["bot_token"], "new-token", "新 bot 字段不受影响");
    }
}

/// 扫码成功后清掉属于**上一个 bot 会话**的路由字段。
///
/// 游标/对端/上下文都绑在旧 token 上：换绑后带着它们用新 token 去
/// `getupdates`，iLink 以 `errcode:-14 session timeout` 拒之（2026-10-23
/// 真机事故：`buf` 里嵌着旧 bot `2b296a82465` 的游标，长轮询全天 -14，
/// 微信发消息没反应）。`session_key` 是 PF 侧会话归属，与 bot 无关，保留。
fn reset_session_fields(st: &mut Value) {
    for k in ["buf", "chat_id", "user_id", "context_token"] {
        crate::state::set(st, k, None);
    }
}

pub enum QrStatus {
    Pending,
    Scanned,
    Success {
        bot_token: String,
        bot_id: Option<String>,
    },
    Expired,
    Error(String),
}

pub fn poll(wire: &mut Wire, qrcode: &str) -> Result<QrStatus, String> {
    // 418 与 weixinRegistration.ts:86 一致：扫码接口是 GET。
    let payload = wire.get_qr("qr-status", "/get_qrcode_status", &[("qrcode", qrcode)])?;
    let raw = payload
        .get("status")
        .or_else(|| payload.get("qrcode_status"))
        .or_else(|| payload.get("qr_status"));
    let status = normalize(raw);
    Ok(match status.as_str() {
        "success" => {
            let bot_token = first_str(&payload, &["bot_token", "token"]).ok_or_else(|| {
                format!(
                    "扫码成功但未返回 bot_token，真实字段：{}",
                    crate::wire::to_pretty(&payload)
                )
            })?;
            QrStatus::Success {
                bot_id: first_str(&payload, &["ilink_bot_id", "bot_id"]),
                bot_token,
            }
        }
        "expired" => QrStatus::Expired,
        "error" => QrStatus::Error(
            str_of(&payload, "errmsg").unwrap_or_else(|| "二维码状态接口报错".into()),
        ),
        other => {
            if other == "scanned" {
                QrStatus::Scanned
            } else if let Some(msg) = unknown_status_message(raw) {
                // 词表外且非等待态的字符串：ZCode 静默按 pending 吞掉，但那会把
                // 「微信侧未装插件」这类新状态吞成无限等待——用户扫了码、微信
                // 提示未安装，PF 却永远显示「等待手机连接」（2026-10-24 用户
                // 要求识别并告知）。原文透传给 UI；流程照常轮询，后续状态正常
                // 翻转不受影响。
                QrStatus::Error(msg)
            } else {
                QrStatus::Pending
            }
        }
    })
}

/// 词表外的**字符串**状态 → 给 UI 的错误文案（含安装指引）。
/// - 含 `install` 关键词：直接判定为微信侧未装插件，给行动指引
/// - 其余：原文透传 + 同样的指引（我们只有 `wait`/词表词的实测样本，
///   「未安装」的真实词形还没见过——透传保证它第一次出现就被看见）
/// 数字码表外（None）按 ZCode 语义仍归 pending，不产生错误。
fn unknown_status_message(raw: Option<&Value>) -> Option<String> {
    let s = match raw {
        Some(Value::String(s)) => s.clone(),
        _ => return None,
    };
    if s.to_lowercase().contains("install") {
        Some(format!(
            "微信侧未安装「微信ClawBot」插件：请先在微信安装/启用插件，再重新扫码。（原始状态：{s}）"
        ))
    } else {
        Some(format!(
            "未识别的扫码状态：{s} —— 若微信提示未安装，请先安装「微信ClawBot」插件再扫。"
        ))
    }
}

/// ZCode `normalizeQrStatus`（`weixinRegistration.ts:103-130`）逐条对应。
fn normalize(status: Option<&Value>) -> String {
    match status {
        Some(Value::Number(n)) => match n.as_i64() {
            Some(0) => "pending".into(),
            Some(1) => "scanned".into(),
            Some(2) => "success".into(),
            Some(3) | Some(4) => "expired".into(),
            _ => "pending".into(),
        },
        Some(Value::String(s)) => {
            let s = s.to_lowercase();
            if ["confirmed", "confirm", "authorized", "success", "ok"].contains(&s.as_str()) {
                "success".into()
            } else if ["scaned", "scanned", "scan", "confirmed_wait"].contains(&s.as_str()) {
                "scanned".into()
            } else if ["expired", "timeout", "cancel", "cancelled", "canceled"]
                .contains(&s.as_str())
            {
                "expired".into()
            } else if ["error", "failed", "fail"].contains(&s.as_str()) {
                "error".into()
            } else if ["wait", "waiting", "pending", "created"].contains(&s.as_str()) {
                // 实测（2026-10-24 dump 503）：等待态返回 "wait"，不在 ZCode
                // 词表里，此前靠 unknown→pending 兜底；显式归一并在此挡住，
                // 让 unknown 分支只接真正的新状态
                "pending".into()
            } else {
                "pending".into()
            }
        }
        _ => "pending".into(),
    }
}
