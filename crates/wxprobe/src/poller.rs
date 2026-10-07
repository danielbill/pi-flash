//! getupdates 长轮询与 sendmessage 回发（P0 步骤 3/4）。
//!
//! 对齐 ZCode `providers/weixinProvider.ts`，**所有候选字段名逐条照搬**——
//! 这正是 P0 要用真实响应去证伪的部分：
//! - 消息数组 5 候选：`msgs | messages | updates | items | list`（`:466-476`）
//! - 游标 6 候选：`get_updates_buf | buf | next_buf | nextBuf | getUpdatesBuf | syncKey`（`:448-461`）
//! - 文本/用户/会话/昵称/消息 id 各自的多级候选（`:345-440`）

use serde_json::{json, Value};

use crate::wire::{first_str, num, str_of, Wire};

const MSG_LIST_KEYS: [&str; 5] = ["msgs", "messages", "updates", "items", "list"];
const BUF_KEYS: [&str; 6] = [
    "get_updates_buf",
    "buf",
    "next_buf",
    "nextBuf",
    "getUpdatesBuf",
    "syncKey",
];
/// 严格照搬 ZCode `readWeixinUserId` 的顶层候选（`:355-370`）——**不能加裸 `id`**，
/// 否则会把消息 id 当成发送者 id（ZCode 没有这条候选）。
const USER_KEYS: [&str; 7] = [
    "from_user_id",
    "from",
    "from_user",
    "fromUser",
    "user",
    "user_id",
    "userId",
];
/// ZCode 不读回发方向，但 sendmessage 需要 `from_user_id`=机器人自身，这里补一组候选。
const BOT_KEYS: [&str; 6] = ["to_user_id", "to", "to_user", "toUser", "receiver", "target_id"];
const CHAT_KEYS: [&str; 6] = ["room", "room_id", "roomId", "chat", "chat_id", "chatId"];
const NAME_KEYS: [&str; 10] = [
    "name",
    "displayName",
    "nickname",
    "nick_name",
    "remark",
    "sender_name",
    "from_name",
    "title",
    "account",
    "alias",
];
const MSG_ID_KEYS: [&str; 8] = ["id", "msgid", "msgId", "msg_id", "message_id", "new_msg_id", "svrMsgId", "localId"];

#[derive(Debug)]
pub struct Inbound {
    pub msg_id: Option<String>,
    pub text: String,
    pub user_id: String,
    /// 机器人自身 id（sendmessage 的 `from_user_id`），从入站消息反推。
    pub bot_user_id: Option<String>,
    pub chat_id: Option<String>,
    pub name: Option<String>,
    pub context_token: Option<String>,
    pub attachments: usize,
}

#[derive(Debug)]
pub struct Updates {
    pub raw_count: usize,
    pub messages: Vec<Inbound>,
    /// 服务端游标；无新消息时回落到入参旧游标（ZCode `readNextBuf(...) ?? buf`）。
    pub next_buf: Option<String>,
    pub diagnostics: Vec<String>,
}

pub fn get_updates(wire: &mut Wire, token: &str, buf: Option<&str>) -> Result<Updates, String> {
    let payload = wire.post_poll(
        "getupdates",
        "/getupdates",
        Some(token),
        json!({ "get_updates_buf": buf.unwrap_or("") }),
    )?;

    Ok(parse_updates(&payload, buf))
}

/// 纯解析：把 getupdates 响应映射成 `Updates`。
/// 从传输里拆出来，才能把**真实响应**落成黄金夹具做单测（P1 验收）。
pub fn parse_updates(payload: &Value, previous_buf: Option<&str>) -> Updates {
    let container = payload;
    let raw_msgs = MSG_LIST_KEYS
        .iter()
        .find_map(|k| container.get(*k).and_then(Value::as_array))
        .cloned()
        .unwrap_or_default();

    let mut messages = Vec::new();
    for m in &raw_msgs {
        if let Some(b) = read_inbound(m) {
            messages.push(b);
        }
    }

    Updates {
        raw_count: raw_msgs.len(),
        messages,
        // 服务端没给新游标就沿用旧值——保证「启停不丢不重」。
        next_buf: read_next_buf(container).or_else(|| previous_buf.map(str::to_string)),
        diagnostics: diagnose(container, &raw_msgs),
    }
}

pub fn send_text(
    wire: &mut Wire,
    token: &str,
    from_user: &str,
    to_user: &str,
    text: &str,
    context_token: Option<&str>,
    client_id: &str,
) -> Result<(), String> {
    // ZCode `buildWeixinText`：微信各客户端对 LF 处理不一致，统一 CRLF。
    let normalized = text.replace("\r\n", "\n").replace('\n', "\r\n");
    let mut msg = json!({
        "from_user_id": from_user,
        "to_user_id": to_user,
        "client_id": client_id,
        "message_type": 2,
        "message_state": 2,
        "item_list": [{ "type": 1, "text_item": { "text": normalized } }],
    });
    if let Some(ctx) = context_token.filter(|s| !s.is_empty()) {
        msg["context_token"] = json!(ctx);
    }
    wire.post("sendmessage", "/sendmessage", Some(token), json!({ "msg": msg }))?;
    Ok(())
}

/// ZCode `readNextBuf`（`weixinProvider.ts:448-461`）：6 个候选字段名。
fn read_next_buf(container: &Value) -> Option<String> {
    first_str(container, &BUF_KEYS)
}

/// ZCode `readInboundMessage`：文本/用户 id 缺一则丢弃，其余字段尽力取。
fn read_inbound(m: &Value) -> Option<Inbound> {
    let inner = nested(m);
    let text = read_text(m, inner);
    let attachments = count_attachments(m, inner);
    if text.trim().is_empty() && attachments == 0 {
        return None;
    }
    let user_id = read_user_id(m, inner);
    if user_id.is_empty() {
        return None;
    }
    Some(Inbound {
        msg_id: read_msg_id(m, inner),
        text,
        user_id,
        bot_user_id: read_bot_user_id(m, inner),
        chat_id: first_str(m, CHAT_KEYS).or_else(|| inner.and_then(|i| first_str(i, CHAT_KEYS))),
        name: first_str(m, NAME_KEYS).or_else(|| inner.and_then(|i| first_str(i, NAME_KEYS))),
        context_token: first_str(m, &["context_token", "contextToken", "context"])
            .or_else(|| inner.and_then(|i| first_str(i, &["context_token", "contextToken", "context"]))),
        attachments,
    })
}

fn nested(m: &Value) -> Option<&Value> {
    ["msg", "message"]
        .iter()
        .find_map(|k| m.get(*k))
        .filter(|v| v.is_object())
}

fn item_list<'a>(m: &'a Value, inner: Option<&'a Value>) -> Vec<&'a Value> {
    for src in [Some(m), inner].into_iter().flatten() {
        if let Some(list) = src.get("item_list").and_then(Value::as_array) {
            if !list.is_empty() {
                return list.iter().collect();
            }
        }
    }
    Vec::new()
}

/// ZCode `readWeixinText`：顶层 text/content/message → item_list 拼接 → inner.text/content。
fn read_text(m: &Value, inner: Option<&Value>) -> String {
    if let Some(t) = first_str(m, &["text", "content", "message"]) {
        return t;
    }
    let items: Vec<String> = item_list(m, inner)
        .iter()
        .filter_map(|it| read_text_item(it))
        .filter(|s| !s.is_empty())
        .collect();
    if !items.is_empty() {
        return items.join("\n");
    }
    inner
        .and_then(|i| first_str(i, &["text", "content"]))
        .unwrap_or_default()
}

fn read_text_item(item: &Value) -> Option<String> {
    let t = item
        .get("text_item")
        .filter(|v| v.is_object())
        .and_then(|t| str_of(t, "text"))
        .or_else(|| str_of(item, "text"))
        .or_else(|| str_of(item, "content"));
    t.filter(|s| !s.is_empty())
}

fn count_attachments(m: &Value, inner: Option<&Value>) -> usize {
    let direct = [m.get("attachments"), inner.and_then(|i| i.get("attachments"))]
        .into_iter()
        .flatten()
        .filter_map(Value::as_array)
        .map(Vec::len)
        .sum::<usize>();
    let from_items = item_list(m, inner)
        .iter()
        .filter(|it| read_text_item(it).is_none())
        .filter(|it| {
            ["image_item", "file_item", "video_item", "audio_item", "media_item"]
                .iter()
                .any(|k| it.get(k).is_some())
                || str_of(it, "kind").is_some()
        })
        .count();
    direct + from_items
}

/// ZCode `readWeixinUserId`：顶层 6 个 + `from.id/from.wxid` + `sender.id/sender.wxid`。
fn read_user_id(m: &Value, inner: Option<&Value>) -> String {
    let nested_obj = |src: &Value, keys: &[&str]| -> Option<String> {
        keys.iter().find_map(|k| {
            let o = src.get(*k)?;
            if !o.is_object() {
                return None;
            }
            first_str(o, &["id", "wxid", "user_id", "userId"])
        })
    };
    first_str(m, USER_KEYS)
        .or_else(|| nested_obj(m, &["from", "sender"]))
        .or_else(|| inner.and_then(|i| first_str(i, USER_KEYS)))
        .or_else(|| inner.and_then(|i| nested_obj(i, &["from", "sender"])))
        .unwrap_or_default()
}

/// 机器人自身的 id（`sendmessage.from_user_id`）。ZCode 由 `bot.providerUserId`
/// 保存，值来自配置/首次入站，这里直接从消息的回发方向反推。
fn read_bot_user_id(m: &Value, inner: Option<&Value>) -> Option<String> {
    let from_obj = |src: &Value, keys: &[&str]| -> Option<String> {
        keys.iter().find_map(|k| {
            let o = src.get(*k)?;
            if !o.is_object() {
                return None;
            }
            first_str(o, &["id", "wxid", "user_id", "userId"])
        })
    };
    first_str(m, BOT_KEYS)
        .or_else(|| from_obj(m, &BOT_KEYS))
        .or_else(|| inner.and_then(|i| first_str(i, BOT_KEYS)))
        .or_else(|| inner.and_then(|i| from_obj(i, &BOT_KEYS)))
}

/// ZCode `readWeixinMessageId`：先按字符串取（两级 4 候选），再退回数字。
fn read_msg_id(m: &Value, inner: Option<&Value>) -> Option<String> {
    if let Some(v) = first_str(m, MSG_ID_KEYS) {
        return Some(v);
    }
    if let Some(i) = inner {
        if let Some(v) = first_str(i, MSG_ID_KEYS) {
            return Some(v);
        }
    }
    MSG_ID_KEYS
        .iter()
        .find_map(|k| num(m, k).map(|n| n.to_string()))
        .or_else(|| inner.and_then(|i| MSG_ID_KEYS.iter().find_map(|k| num(i, k).map(|n| n.to_string()))))
}

/// P0 产出：把真实响应的字段形状打出来，对拍 ZCode 的候选嗅探。
fn diagnose(container: &Value, raw_msgs: &[Value]) -> Vec<String> {
    let mut out = Vec::new();
    out.push(format!("顶层键: {}", keys_of(container)));
    let picked = MSG_LIST_KEYS.iter().find(|k| container.get(**k).is_some());
    out.push(format!(
        "消息数组键: {}",
        picked.map(|k| k.to_string()).unwrap_or_else(|| "未命中任何候选".into())
    ));
    out.push(format!(
        "游标命中: {}",
        BUF_KEYS
            .iter()
            .filter(|k| container.get(**k).is_some())
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
            .if_empty("（无）")
    ));
    if let Some(m) = raw_msgs.first() {
        out.push(format!("首条消息键: {}", keys_of(m)));
        if let Some(i) = nested(m) {
            out.push(format!("  内层(msg/message)键: {}", keys_of(i)));
        }
        let items = item_list(m, nested(m));
        if !items.is_empty() {
            let shapes: Vec<String> = items.iter().map(|it| keys_of(it)).collect();
            out.push(format!("  item_list[{}]: {}", items.len(), shapes.join(" | ")));
        }
        out.push(format!(
            "  文本候选命中: {}",
            probe_text(m)
        ));
    }
    out
}

fn probe_text(m: &Value) -> String {
    let inner = nested(m);
    let mut hits = Vec::new();
    for k in ["text", "content", "message"] {
        if str_of(m, k).is_some() {
            hits.push(format!("顶层.{k}"));
        }
    }
    for (label, src) in [("顶层", Some(m)), ("内层", inner)] {
        if let Some(s) = src {
            if let Some(list) = s.get("item_list").and_then(Value::as_array) {
                for (ix, it) in list.iter().enumerate() {
                    if read_text_item(it).is_some() {
                        hits.push(format!("{label}.item_list[{ix}]"));
                    }
                }
            }
        }
    }
    hits.join(", ").if_empty("（无）")
}

fn keys_of(v: &Value) -> String {
    match v.as_object() {
        Some(m) if m.is_empty() => "（空）".into(),
        Some(m) => m.keys().cloned().collect::<Vec<_>>().join(", "),
        None => format!("非对象: {}", v),
    }
}

trait IfEmpty {
    fn if_empty(self, alt: &str) -> String;
}

impl IfEmpty for String {
    fn if_empty(self, alt: &str) -> String {
        if self.trim().is_empty() {
            alt.to_string()
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-07 真机 `getupdates` 响应，已脱敏（user id / context_token / client_id 换占位值）。
    /// P1 黄金夹具：iLink 协议一变，这里会最先红。
    const REAL_GETUPDATES: &str = include_str!("../tests/fixtures/getupdates-real.json");

    fn real_payload() -> Value {
        serde_json::from_str(REAL_GETUPDATES).expect("夹具必须是合法 JSON")
    }

    #[test]
    fn parses_real_getupdates_fixture() {
        let up = parse_updates(&real_payload(), None);
        assert_eq!(up.raw_count, 1, "原始消息数");
        assert_eq!(up.messages.len(), 1, "解析成功数");
        let m = &up.messages[0];
        assert_eq!(m.text, "测试文本");
        assert_eq!(m.user_id, "user_sandbox@im.wechat");
        assert_eq!(m.bot_user_id.as_deref(), Some("bot_sandbox@im.bot"));
        assert_eq!(m.msg_id.as_deref(), Some("7513480159106982024"));
        assert_eq!(m.context_token.as_deref(), Some("SANITIZED_CONTEXT_TOKEN"));
        assert_eq!(m.chat_id, None, "私聊没有 room/chat 字段");
        assert_eq!(m.attachments, 0);
    }

    #[test]
    fn cursor_taken_from_server_on_first_poll() {
        let expected = real_payload()["get_updates_buf"].as_str().unwrap().to_string();
        let up = parse_updates(&real_payload(), None);
        assert_eq!(up.next_buf.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn cursor_falls_back_to_previous_when_server_omits_it() {
        // 服务端没给新游标（或给空串）时必须沿用旧值，否则「启停」会把游标冲掉。
        let mut payload = real_payload();
        payload["get_updates_buf"] = serde_json::json!("");
        let up = parse_updates(&payload, Some("PREVIOUS_CURSOR"));
        assert_eq!(up.next_buf.as_deref(), Some("PREVIOUS_CURSOR"));
    }

    #[test]
    fn cursor_keeps_previous_when_field_absent_entirely() {
        let payload = serde_json::json!({ "msgs": [] });
        let up = parse_updates(&payload, Some("PREVIOUS_CURSOR"));
        assert_eq!(up.next_buf.as_deref(), Some("PREVIOUS_CURSOR"));
        assert_eq!(up.raw_count, 0);
    }

    #[test]
    fn message_without_sender_is_dropped() {
        // ZCode 同语义：`from_user_id` 缺失就丢弃，否则回发时 to 为空。
        let payload = serde_json::json!({
            "msgs": [{ "message_id": 1, "item_list": [{ "type": 1, "text_item": { "text": "hi" } }] }],
            "get_updates_buf": "X"
        });
        let up = parse_updates(&payload, None);
        assert_eq!(up.raw_count, 1, "原始数组仍计数");
        assert!(up.messages.is_empty(), "缺发送者的消息必须被丢弃");
    }

    #[test]
    fn attachment_item_is_counted_and_text_item_is_not() {
        // 纯图片消息没有 text，靠附件计数才不会被当成空消息丢掉。
        let payload = serde_json::json!({
            "msgs": [{
                "from_user_id": "user_sandbox@im.wechat",
                "item_list": [{
                    "type": 1,
                    "image_item": { "media": { "url": "https://x/img.png", "aes_key": "K" } }
                }]
            }],
            "get_updates_buf": "X"
        });
        let up = parse_updates(&payload, None);
        assert_eq!(up.messages.len(), 1);
        assert_eq!(up.messages[0].attachments, 1);
        assert_eq!(up.messages[0].text, "");
    }

    #[test]
    fn diagnostics_report_the_real_field_names() {
        // 直接对拍 ZCode 的候选嗅探：命中哪个候选必须打印出来。
        let up = parse_updates(&real_payload(), None);
        let joined = up.diagnostics.join("\n");
        assert!(joined.contains("消息数组键: msgs"), "候选 1 应命中：{joined}");
        assert!(joined.contains("游标命中: get_updates_buf"), "候选 1 应命中：{joined}");
        assert!(joined.contains("文本候选命中: 顶层.item_list[0]"), "{joined}");
        assert!(!joined.contains("未命中任何候选"), "{joined}");
    }

    #[test]
    fn text_item_wins_over_nested_content() {
        // 顶层 text 与 item_list 同时存在时，顶层优先（ZCode readWeixinText 同序）。
        let payload = serde_json::json!({
            "msgs": [{
                "from_user_id": "u@x",
                "text": "顶层文本",
                "item_list": [{ "type": 1, "text_item": { "text": "item 文本" } }]
            }]
        });
        let up = parse_updates(&payload, None);
        assert_eq!(up.messages[0].text, "顶层文本");
    }

    #[test]
    fn twenty_start_stop_cycles_never_lose_or_duplicate_the_cursor() {
        // 对应 P1 验收「重复启停 20 次游标不丢不重」的确定性版本：
        // 每个周期 = load → getupdates → **处理完** → 写回，跑 20 轮。
        let server_cursor = real_payload()["get_updates_buf"].as_str().unwrap().to_string();
        let mut stored: Option<String> = None;
        for cycle in 0..20 {
            // 只有第 1 轮服务端发了新游标，其余 19 轮都是空闲（不带该字段）。
            let payload = if cycle == 0 {
                real_payload()
            } else {
                serde_json::json!({ "msgs": [] })
            };
            let up = parse_updates(&payload, stored.as_deref());
            // 写回发生在本批消息全部处理完之后（ZCode weixinChannelRuntime.ts:155-163）
            if let Some(next) = up.next_buf {
                stored = Some(next);
            }
        }
        assert_eq!(
            stored.as_deref(),
            Some(server_cursor.as_str()),
            "20 次启停后游标必须恰好等于服务端最后一次下发的值——既不能丢，也不能被重复推进"
        );
    }

    #[test]
    fn idle_polls_do_not_advance_the_cursor() {
        let payload = serde_json::json!({ "msgs": [], "get_updates_buf": "" });
        let up = parse_updates(&payload, Some("CURSOR_V1"));
        assert_eq!(
            up.next_buf.as_deref(),
            Some("CURSOR_V1"),
            "空闲轮询不得改写游标"
        );
    }
}
