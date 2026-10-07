//! 扫码注册（P0 步骤 1/2）：`get_bot_qrcode` → `get_qrcode_status`。
//!
//! 对齐 ZCode `providers/weixinRegistration.ts`：
//! - `GET /ilink/bot/get_bot_qrcode?bot_type=3`（`:132`）
//! - `GET /ilink/bot/get_qrcode_status?qrcode=<enc>`，3s 间隔轮询（`:146-157`）
//! - 状态归一：数字 0/1/2/3|4 与字符串词表两套（`:103-130`）

use serde_json::Value;

use crate::wire::{first_str, num, str_of, Wire};

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
    let qrcode = first_str(&payload, &["qrcode", "qr_code"])
        .ok_or_else(|| format!("扫码接口未返回 qrcode，真实字段：{}", crate::wire::to_pretty(&payload)))?;
    let qr_url =
        first_str(&payload, &["qrcode_img_content", "qrcode_url"]).unwrap_or_else(|| qrcode.clone());
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
    Ready { url: String },
    /// 已扫码，等待手机确认
    Scanned,
    /// 扫码成功：token 已写进状态文件
    Success { bot_id: Option<String> },
    Expired,
    Error(String),
}

/// 扫码全流程（阻塞，**跑在专用线程**）：`begin` → 每 3s `poll` → 落状态。
///
/// 与 `wxprobe qr` + `wxprobe scan` 用的是同一条路径、同一份状态文件，
/// 所以设置页扫完码 CLI 工具立刻也能用，反之亦然。
///
/// `deadline_sec` = 超时秒数（ZCode/微信二维码约 120s 有效）。
pub fn run_qr_flow(
    tx: std::sync::mpsc::Sender<QrEvent>,
    deadline_sec: u64,
) {
    let mut w = crate::wire::Wire::new(crate::state::dump_dir());
    let begin = match begin(&mut w) {
        Ok(b) => b,
        Err(e) => {
            let _ = tx.send(QrEvent::Error(e));
            return;
        }
    };
    if tx.send(QrEvent::Ready { url: begin.qr_url.clone() }).is_err() {
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
                let _ = tx.send(QrEvent::Scanned);
            }
            Ok(QrStatus::Pending) => {}
            // 状态接口偶发报错不致命，退避后继续（§6 坑 4：不能让循环退出）
            Ok(QrStatus::Error(e)) | Err(e) => {
                let _ = tx.send(QrEvent::Error(e));
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

pub enum QrStatus {
    Pending,
    Scanned,
    Success { bot_token: String, bot_id: Option<String> },
    Expired,
    Error(String),
}

pub fn poll(wire: &mut Wire, qrcode: &str) -> Result<QrStatus, String> {
    // 418 与 weixinRegistration.ts:86 一致：扫码接口是 GET。
    let payload = wire.get_qr(
        "qr-status",
        "/get_qrcode_status",
        &[("qrcode", qrcode)],
    )?;
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
        "error" => QrStatus::Error(str_of(&payload, "errmsg").unwrap_or_else(|| "二维码状态接口报错".into())),
        other => {
            if other == "scanned" {
                QrStatus::Scanned
            } else {
                QrStatus::Pending
            }
        }
    })
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
            } else if ["expired", "timeout", "cancel", "cancelled", "canceled"].contains(&s.as_str()) {
                "expired".into()
            } else if ["error", "failed", "fail"].contains(&s.as_str()) {
                "error".into()
            } else {
                "pending".into()
            }
        }
        _ => "pending".into(),
    }
}
