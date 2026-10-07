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

    let container = &payload;
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

    let next_buf = read_next_buf(container).or_else(|| buf.map(str::to_string));

    Ok(Updates {
        raw_count: raw_msgs.len(),
        messages,
        next_buf,
        diagnostics: diagnose(container, &raw_msgs),
    })
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
