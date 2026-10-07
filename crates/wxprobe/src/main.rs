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

mod format;
mod command;
mod lock;
mod parity;
mod poller;
mod register;
mod state;
mod wire;

use std::fs;
use std::io::Write;
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde_json::json;

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
        "parse" if args.len() > 1 => cmd_parse(&args[1..].join(" ")),
        "status-demo" => cmd_status_demo(
            args.get(1).map(String::as_str).unwrap_or("zh"),
            args.get(2).map(String::as_str).unwrap_or("running"),
        ),
        "reply-demo" => {
            cmd_reply_demo(args.get(1).map(String::as_str).unwrap_or("zh"))
        }
        "parity" => crate::parity::run(args.iter().any(|a| a == "--update")),
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
    "用法: wxprobe qr | scan [max_sec] | recv [rounds] | loop [sec] | send <text> | parse <text> | status-demo [zh|tw|en] [state] | reply-demo [zh|tw|en] | parity [--update] | state | reset".into()
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
/// 调试/验证用：把一句话丢给命令解析层，打印结构化结果。
/// P3 接上 pipeline 后，这里就是「微信输入 → 会话动作」的最小可观测面。
fn cmd_parse(text: &str) -> Result<(), String> {
    println!("{:?}", command::parse_bot_command(text));
    Ok(())
}


/// 渲染一张 /status 卡片 —— 界面层 B 的排版冒烟与「逐字节对拍」基准。
/// 数据是写死的样例；P3 接上 pi 会话状态后，这里仍是回归用的稳定出口。
///
/// `state` 传 `disconnected` 时走 ZCode 的断连分支（`tf` 模板那条）。
fn cmd_status_demo(lang_arg: &str, state: &str) -> Result<(), String> {
    use crate::format::messages::{t, tf, Lang};
    use crate::format::status::{
        status_card, status_line, status_state_value, status_task_line, task_running_duration,
    };

    // 序号与 app::i18n::LANG_IX 对齐（0 zh-CN / 1 zh-TW / 2 en）
    let lang = Lang::from_ix(match lang_arg {
        "en" => 2,
        "tw" => 1,
        _ => 0,
    });
    let workspace = "pi-flash";

    if state == "disconnected" {
        // ZCode `buildStatusText` 的断连分支：只出 4 行 + 一句模板提示。
        println!(
            "{}",
            status_card(&[
                status_line(lang, "工作区", workspace),
                status_line(lang, "模型", "glm-5.3"),
                "------".to_string(),
                status_line(lang, "任务", "task-abc"),
                status_line(lang, "状态", &status_state_value(lang, "remote disconnected")),
                tf(
                    lang,
                    "当前远端项目 {workspacePath} 未连接。请发送 **/重连** 恢复连接。",
                    &[("workspacePath", workspace)],
                ),
            ])
        );
        return Ok(());
    }

    println!(
        "{}",
        status_card(&[
            status_line(lang, "工作区", workspace),
            status_line(lang, "模型", "glm-5.3"),
            "------".to_string(),
            status_task_line(&t(lang, "任务"), "重构传输层", "task-42"),
            status_line(lang, "状态", &status_state_value(lang, state)),
            status_line(lang, "已工作", &task_running_duration(5_400_000)),
            status_line(lang, "进展", "正在编辑 crates/wxprobe/src/wire.rs"),
        ])
    );
    Ok(())
}

/// 渲染一组回复块，覆盖切片 3 的全部出口：工具摘要行、变更摘要、
/// flush 边界、终态判定、超长分块 —— 排版冒烟 + 逐字节对拍基准。
fn cmd_reply_demo(lang_arg: &str) -> Result<(), String> {
    use crate::format::messages::Lang;
    use crate::format::permission::PermissionRequest;
    use crate::format::reply::{
        extract_bot_assistant_response_messages, format_bot_assistant_reply_blocks,
        format_bot_permission_request_summary, format_tool_status,
        is_bot_tool_call_reply_terminal, split_long_reply_text,
        BotAssistantReplyBlock, BotReplyToolCallState, ChangeSummary, FileChange, ToolStatus,
        MAX_REPLY_MESSAGE_LENGTH,
    };

    let lang = Lang::from_ix(match lang_arg {
        "en" => 2,
        "tw" => 1,
        _ => 0,
    });
    let workspace = "/proj/pi-flash";

    let bash = BotReplyToolCallState {
        tool_id: "call_1".into(),
        title: Some("Bash".into()),
        kind: Some("bash".into()),
        input: json!({ "command": "cargo test -p wxprobe" }),
        output: None,
        status: Some(ToolStatus::Completed),
        error: None,
        raw: None,
    };
    let edit = BotReplyToolCallState {
        tool_id: "call_2".into(),
        title: Some("Edit".into()),
        kind: Some("edit".into()),
        input: json!({
            "path": "/proj/pi-flash/crates/wxprobe/src/wire.rs",
            "old_string": "fn a() {}\n",
            "new_string": "fn a() {}\nfn b() {}\n",
        }),
        output: None,
        status: Some(ToolStatus::Completed),
        error: None,
        raw: None,
    };
    let pending = BotReplyToolCallState {
        tool_id: "call_3".into(),
        title: Some("Grep".into()),
        kind: Some("grep".into()),
        input: json!({ "path": "crates/wxprobe" }),
        output: None,
        status: Some(ToolStatus::Pending),
        error: None,
        raw: None,
    };

    let blocks = vec![
        BotAssistantReplyBlock::Content {
            content: "我来改这两处。".into(),
        },
        BotAssistantReplyBlock::ToolCall {
            tool_call: bash.clone(),
        },
        BotAssistantReplyBlock::ToolCall { tool_call: edit },
        BotAssistantReplyBlock::ChangeSummary {
            change_summary: ChangeSummary {
                file_count: 2,
                added: 12,
                removed: 3,
                files: vec![
                    FileChange {
                        path: "crates/wxprobe/src/wire.rs".into(),
                        added: 8,
                        removed: 2,
                    },
                    FileChange {
                        path: "crates/wxprobe/src/poller.rs".into(),
                        added: 4,
                        removed: 1,
                    },
                ],
            },
        },
    ];
    for (index, message) in
        format_bot_assistant_reply_blocks(&blocks, lang, Some(workspace))
            .iter()
            .enumerate()
    {
        println!("── 第 {} 条 ──", index + 1);
        println!("{message}");
    }

    // flush 边界：非终态不发（ZCode `extractBotAssistantResponseMessages`）
    let (pending_msgs, pending_rest) =
        extract_bot_assistant_response_messages("正在写 wire.rs", false);
    let (done_msgs, done_rest) = extract_bot_assistant_response_messages("写完了", true);
    println!(
        "\nflush 边界：非终态 {} 条(余 {} 字)，终态 {} 条(余 {} 字)",
        pending_msgs.len(),
        pending_rest.len(),
        done_msgs.len(),
        done_rest.len()
    );

    // 终态判定（stopped 也是终态，但状态文案落回「等待中」）
    for call in [&bash, &pending] {
        println!(
            "终态判定 {} ({:?}) = {}",
            call.tool_id,
            call.status,
            is_bot_tool_call_reply_terminal(call.status)
        );
    }

    // 状态文案：各变体各渲染一次，顺带覆盖 ZCode「stopped 落回等待中」的原样行为
    for status in [
        Some(ToolStatus::Pending),
        Some(ToolStatus::InProgress),
        Some(ToolStatus::Completed),
        Some(ToolStatus::Failed),
        Some(ToolStatus::Denied),
        Some(ToolStatus::Stopped),
        None,
    ] {
        println!("状态文案 {status:?} = {}", format_tool_status(status, None, lang));
    }

    // 权限请求排版：kind=edit，PC 端 PermissionDialog 与微信端共用同一份 preview。
    // 这条走 ZCode 的「泛化 Edit」兜底（raw 里没有 write/delete/update 词）。
    let permission = PermissionRequest {
        title: None,
        description: "edit crates/wxprobe/src/wire.rs".into(),
        kind: "edit".into(),
        raw: json!({ "input": { "path": "/proj/pi-flash/crates/wxprobe/src/wire.rs" } }),
    };
    println!(
        "\n权限请求（kind=edit → ZCode 泛化 Edit 兜底）：\n{}",
        format_bot_permission_request_summary(&permission, lang, Some(workspace))
    );

    let chunks = split_long_reply_text(&"A".repeat(7200));
    println!(
        "\n超长分块：7200 字 → {} 块（上限 {MAX_REPLY_MESSAGE_LENGTH} 字/块，首块 {} 字）",
        chunks.len(),
        chunks[0].chars().count()
    );
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
