//! wxprobe — P0 iLink 协议证伪探针（`docs/模块设计/060-远程控制-微信.md` §5 P0）。
//!
//! 用法：
//! ```text
//! wxprobe qr                发起扫码，打印真实字段并落盘二维码图片
//! wxprobe scan [max_sec]    轮询扫码状态，默认 180s，成功后保存 bot_token
//! wxprobe recv [rounds]     长轮询收消息，默认 2 轮；打印真实字段形状 + 解析结果
//! wxprobe send <text...>    回发一条文本（会话标识来自最近一次 recv）
//! wxprobe state             查看状态（token 打码）
//! wxprobe reset             清空状态
//! ```
//!
//! 所有请求/响应原样落盘到 `~/.pi-flash/wxprobe-dump/`（`PI_FLASH_DIR` 优先）。

mod lock;
mod poller;
mod register;
mod state;
mod wire;

use std::fs;
use std::io::Write;
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine as _;

use crate::wire::Wire;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("");
    let result = match cmd {
        "qr" => cmd_qr(),
        "scan" => cmd_scan(args.get(1).and_then(|s| s.parse().ok()).unwrap_or(180)),
        "recv" => cmd_recv(args.get(1).and_then(|s| s.parse().ok()).unwrap_or(2)),
        "loop" => cmd_loop(args.get(1).and_then(|s| s.parse().ok()).unwrap_or(300)),
        "send" if args.len() > 1 => cmd_send(&args[1..].join(" ")),
        "state" => cmd_state(),
        "reset" => cmd_reset(),
        _ => Err(usage()),
    };
    if let Err(e) = result {
        eprintln!("[wxprobe] 失败: {e}");
        std::process::exit(1);
    }
}

fn usage() -> String {
    "用法: wxprobe qr | scan [max_sec] | recv [rounds] | loop [sec] | send <text> | state | reset".into()
}

fn wire() -> Wire {
    Wire::new(state::dump_dir())
}

fn require(state: &serde_json::Value, key: &str, hint: &str) -> Result<String, String> {
    state::get_str(state, key).ok_or_else(|| format!("缺少 `{key}`：{hint}"))
}

// ── 步骤 1：发起扫码 ────────────────────────────────────────────────

fn cmd_qr() -> Result<(), String> {
    let mut w = wire();
    let begin = register::begin(&mut w)?;
    println!("真实字段 qrcode           = {}", begin.qrcode);
    println!("真实字段 qrcode_img_content = {}", begin.qr_url);
    println!("expires_in 解析           = 有效期至 {}ms", begin.expires_at_ms);
    println!("扫码状态轮询间隔          = {}s", begin.interval_secs);

    if let Some(path) = save_qr_image(&begin.qr_url) {
        println!("二维码图片已存: {path}");
    }
    // ZCode 用 `QRCode.toDataURL(result.qrUrl)` 把这个 URL 渲成二维码（BotsDialog.tsx:879-884），
    // 探针同样编码 qr_url —— 上方 qrcode 字段是十六进制会话 id，不是可扫码内容。
    render_qr(&begin.qr_url)?;

    let mut st = state::load();
    state::set(&mut st, "qrcode", Some(&begin.qrcode));
    state::set(&mut st, "qr_url", Some(&begin.qr_url));
    state::set(&mut st, "bot_token", None);
    state::set(&mut st, "buf", None);
    state::save(&st);

    println!("\n下一步: 用微信扫码，然后运行 `wxprobe scan`");
    Ok(())
}

// ── 步骤 2：等扫码 ────────────────────────────────────────────────

fn cmd_scan(max_sec: u64) -> Result<(), String> {
    let mut st = state::load();
    let qrcode = require(&st, "qrcode", "先跑 `wxprobe qr`")?;
    let mut w = wire();
    let deadline = Instant::now() + Duration::from_secs(max_sec);
    let mut last = String::new();

    loop {
        match register::poll(&mut w, &qrcode)? {
            register::QrStatus::Success { bot_token, bot_id } => {
                state::set(&mut st, "bot_token", Some(&bot_token));
                state::set(&mut st, "bot_id", bot_id.as_deref());
                state::set(&mut st, "qrcode", None);
                state::save(&st);
                println!("\n✅ 拿到 bot_token（{} 字符）", bot_token.len());
                if let Some(b) = bot_id {
                    println!("   bot_id = {b}");
                }
                println!("下一步: `wxprobe recv 2`（先在手机上给自己发一条测试消息）");
                return Ok(());
            }
            register::QrStatus::Expired => {
                state::set(&mut st, "qrcode", None);
                state::save(&st);
                return Err("二维码已过期，重新运行 `wxprobe qr`".into());
            }
            register::QrStatus::Error(e) => {
                println!("\n状态接口报错: {e}");
            }
            other => {
                let label = match other {
                    register::QrStatus::Scanned => "已扫码，等待手机确认",
                    _ => "等待扫码",
                };
                if label != last {
                    print!("\n{label}");
                    last = label.to_string();
                } else {
                    print!(".");
                }
                let _ = std::io::stdout().flush();
            }
        }
        if Instant::now() >= deadline {
            println!();
            return Err(format!("{max_sec}s 内未完成扫码"));
        }
        thread::sleep(Duration::from_secs(3));
    }
}

// ── 步骤 3：长轮询收消息 ──────────────────────────────────────────

fn cmd_recv(rounds: usize) -> Result<(), String> {
    let mut st = state::load();
    let token = require(&st, "bot_token", "先跑 `wxprobe qr` + `wxprobe scan`")?;
    let mut w = wire();
    let client_id = state::ensure_client_id(&mut st);

    for round in 1..=rounds {
        println!("\n=== 第 {round}/{rounds} 轮 getupdates (client_id={client_id}) ===");
        let buf = state::get_str(&st, "buf");
        let up = poller::get_updates(&mut w, &token, buf.as_deref())?;
        for d in &up.diagnostics {
            println!("  {d}");
        }
        println!("  原始消息数={} 解析成功={}", up.raw_count, up.messages.len());

        for m in &up.messages {
            println!(
                "  ├ msg_id={:?} from={} name={:?} chat={:?} ctx={:?} attach={}",
                m.msg_id, m.user_id, m.name, m.chat_id, m.context_token, m.attachments
            );
            if let Some(b) = &m.bot_user_id {
                println!("  │ to(机器人自身)={b}");
            }
            println!("  │ text: {}", wire::truncate(&m.text, 300));
            // 会话标识：以最近一条入站为准（与 ZCode「一 bot 一 context」同语义）
            state::set(&mut st, "user_id", Some(&m.user_id));
            state::set(&mut st, "chat_id", m.chat_id.as_deref());
            state::set(&mut st, "context_token", m.context_token.as_deref());
            if let Some(b) = &m.bot_user_id {
                state::set(&mut st, "bot_user_id", Some(b));
            }
        }

        // 游标**必须**等本批全部处理完再写（ZCode weixinChannelRuntime.ts:155-163）
        if let Some(nb) = &up.next_buf {
            state::set(&mut st, "buf", Some(nb));
        }
        state::save(&st);
        println!("  游标已落盘 ({} 字符)", state::get_str(&st, "buf").map(|s| s.len()).unwrap_or(0));
    }
    Ok(())
}

/// P1：带租约锁的长轮询循环。
/// 锁保证同一 `bot_token` 只有一个轮询者（ZCode 多窗口/多实例去重同款语义）；
/// 游标**在本批消息处理完之后**才写盘，进程中途挂掉不会跳过未处理消息。
fn cmd_loop(seconds: u64) -> Result<(), String> {
    let mut st = state::load();
    let token = require(&st, "bot_token", "先跑 `wxprobe qr` + `wxprobe scan`")?;
    let bot_id = state::get_str(&st, "bot_id").unwrap_or_default();
    let mut w = wire();

    let lock = match lock::acquire("weixin-polling", &token, &bot_id)? {
        Some(l) => l,
        None => {
            return Err(format!(
                "另一个轮询者已持有该 bot 的锁，见 {}",
                lock::lock_root_display()
            ));
        }
    };
    println!("已获取轮询锁，进入 {seconds}s 循环（超时或 Ctrl+C 结束，锁随作用域自动释放）");

    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut rounds = 0u32;
    let mut delivered = 0usize;
    while Instant::now() < deadline {
        rounds += 1;
        let buf = state::get_str(&st, "buf");
        let before = buf.clone();
        let up = poller::get_updates(&mut w, &token, buf.as_deref())?;
        for m in &up.messages {
            delivered += 1;
            println!(
                "  [{rounds}] {} <{}>",
                wire::truncate(&m.text, 200),
                m.user_id
            );
            state::set(&mut st, "user_id", Some(&m.user_id));
            state::set(&mut st, "context_token", m.context_token.as_deref());
            if let Some(b) = &m.bot_user_id {
                state::set(&mut st, "bot_user_id", Some(b));
            }
        }
        // 游标只在服务端给了新值时改写；没给就原样保留（启停不丢不重）。
        if let Some(nb) = up.next_buf.as_deref() {
            if Some(nb) != before.as_deref() {
                state::set(&mut st, "buf", Some(nb));
            }
        }
        state::save(&st);
    }

    lock.release();
    println!(
        "循环结束：{rounds} 轮 / 收到 {delivered} 条；游标 {} 字符，锁已释放",
        state::get_str(&st, "buf").map(|s| s.len()).unwrap_or(0)
    );
    Ok(())
}

// ── 步骤 4：回发 ──────────────────────────────────────────────────

fn cmd_send(text: &str) -> Result<(), String> {
    let mut st = state::load();
    let token = require(&st, "bot_token", "先跑 `wxprobe qr` + `wxprobe scan`")?;
    let to = require(&st, "user_id", "先跑 `wxprobe recv` 记录会话标识")?;
    let from = state::get_str(&st, "bot_user_id")
        .or_else(|| state::get_str(&st, "bot_id"))
        .unwrap_or_default();
    let ctx = state::get_str(&st, "context_token");
    let client_id = state::ensure_client_id(&mut st);
    state::save(&st);

    println!("from={from:?} to={to:?} ctx={ctx:?}");
    let mut w = wire();
    poller::send_text(&mut w, &token, &from, &to, text, ctx.as_deref(), &client_id)?;
    println!("✅ 已发送");
    Ok(())
}

fn cmd_state() -> Result<(), String> {
    let mut st = state::load();
    if let Some(t) = state::get_str(&st, "bot_token") {
        state::set(&mut st, "bot_token", Some(&mask(&t)));
    }
    println!("{}", wire::to_pretty(&st));
    println!("状态文件: {}", state::state_path().display());
    println!("dump 目录: {}", state::dump_dir().display());
    Ok(())
}

fn cmd_reset() -> Result<(), String> {
    let path = state::state_path();
    if path.exists() {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
        println!("已删除 {}", path.display());
    } else {
        println!("状态文件本就不存在");
    }
    Ok(())
}

fn mask(t: &str) -> String {
    if t.len() <= 8 {
        "*".repeat(t.len())
    } else {
        format!("{}…{}（{} 字符）", &t[..4], &t[t.len() - 4..], t.len())
    }
}

/// 把 `qr_url` 渲成二维码：SVG 落盘（浏览器可开）+ 终端直出。
/// 单元尺寸 2×2：Dense1x2 的 2 半行 = 1 字符高，2 字符宽 × 1 字符高 ≈ 方形模块。
fn render_qr(url: &str) -> Result<(), String> {
    let code =
        qrcode::QrCode::new(url.as_bytes()).map_err(|e| format!("二维码编码失败: {e}"))?;

    let svg = code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(320, 320)
        .build();
    let svg_path = state::base_dir().join("wxprobe-qr.svg");
    fs::write(&svg_path, svg).map_err(|e| format!("写 SVG 失败: {e}"))?;
    println!("二维码 SVG 已存: {}（浏览器打开后用手机微信扫）", svg_path.display());

    let art = code
        .render::<qrcode::render::unicode::Dense1x2>()
        .module_dimensions(2, 2)
        .build();
    println!();
    println!("{art}");
    println!("或直接在手机微信里打开: {url}");
    Ok(())
}

/// 把 `qrcode_img_content` 尽可能落成可扫码的图片：data URI / 裸 base64。
fn save_qr_image(src: &str) -> Option<String> {
    let base = state::base_dir();
    let bytes = if let Some(rest) = src.strip_prefix("data:") {
        let (head, b64) = rest.split_once(',')?;
        let mime = head.split(';').next().unwrap_or("image/png");
        let ext = mime.split('/').nth(1)?.split(';').next().unwrap_or("png");
        let raw = base64::engine::general_purpose::STANDARD.decode(b64.trim()).ok()?;
        let path = base.join(format!("wxprobe-qr.{ext}"));
        fs::write(&path, &raw).ok()?;
        return Some(path.display().to_string());
    } else {
        let cleaned: String = src.chars().filter(|c| !c.is_whitespace()).collect();
        if cleaned.len() < 64 || !cleaned.len().is_multiple_of(4) {
            return None;
        }
        if !cleaned
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
        {
            return None;
        }
        base64::engine::general_purpose::STANDARD.decode(&cleaned).ok()?
    };

    let ext = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "jpg"
    } else if bytes.starts_with(b"GIF8") {
        "gif"
    } else {
        return None;
    };
    let path = base.join(format!("wxprobe-qr.{ext}"));
    fs::write(&path, &bytes).ok()?;
    Some(path.display().to_string())
}
