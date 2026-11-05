//! Message content blocks (`message.content` wire arrays -> typed [`Block`]).

use serde_json::Value;

/// Assistant content block kinds carried by `message.content` arrays.
///
/// Mirrors the wire blocks pi emits (json.md): `text`, `thinking`, `toolcall`,
/// `image` (v56: tool results and user messages may carry images).
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Text {
        content_index: usize,
        text: String,
    },
    Thinking {
        content_index: usize,
        text: String,
    },
    ToolCall {
        content_index: usize,
        id: String,
        name: String,
        args: String,
        /// Output text delivered by a following role="toolResult" message.
        result: String,
        /// ToolResultMessage.isError — set when the result message merges in.
        is_error: bool,
        /// Images delivered by the toolResult message content.
        images: Vec<ImageData>,
        /// result arrival − message start, seconds (pi-web {n}s duration)
        duration_s: Option<i64>,
        /// ToolResultMessage.details (write/edit patch, apply_patch preview)
        details: Option<Value>,
        /// args still streaming (toolcall_delta); flips false on toolcall_end
        args_partial: bool,
        /// a toolResult message has merged in (empty text ≠ "no result yet")
        result_arrived: bool,
    },
    /// ImageContent { type:"image", data: b64, mimeType } — user message
    /// attachments; tool-result images merge into the paired ToolCall.
    Image {
        content_index: usize,
        mime: String,
        data: String,
    },
}

/// Decoded image payload (`ImageContent` on the wire): b64 `data` + mime.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageData {
    pub mime: String,
    pub data: String,
}

impl Block {
    pub fn content_index(&self) -> usize {
        match self {
            Block::Text { content_index, .. }
            | Block::Thinking { content_index, .. }
            | Block::ToolCall { content_index, .. }
            | Block::Image { content_index, .. } => *content_index,
        }
    }
}

/// 剥除 pi 拼进消息文本尾部的图片坐标标注（pi dist/utils/image-resize.js
/// `formatDimensionNote`：图片超限缩放展示时，user 附件与 read 工具图结果
/// 的 text 块尾都会无开关追加
/// `[Image: original WxH, displayed at WxH. Multiply coordinates by N to map
/// to original image.]`）。那是给模型的元数据、非消息原文：PF 展示与回显
/// 同文去重（pending_echoes）都按「实际文本」口径，在 wire 解码出口统一剥。
/// 只剥尾部连续标注（多图多条连排）；形状不符（如用户手打同前缀文本）不剥
/// ——pi 升级若改了格式，这里静默退化为不剥，复核点记在 docs/agents/pi-rpc.md。
fn strip_image_dimension_note(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains("[Image: original ") {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut s = text.trim_end();
    let mut stripped = false;
    while let Some(start) = s.rfind("[Image: original ") {
        if !is_image_dimension_note(&s[start..]) {
            break;
        }
        s = s[..start].trim_end();
        stripped = true;
    }
    if stripped {
        std::borrow::Cow::Owned(s.to_string())
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

/// 形状校验，严格对齐 formatDimensionNote 输出格式。
fn is_image_dimension_note(seg: &str) -> bool {
    fn digits(s: &str) -> bool {
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
    }
    let Some(rest) = seg.strip_prefix("[Image: original ") else {
        return false;
    };
    let Some((w, rest)) = rest.split_once('x') else {
        return false;
    };
    if !digits(w) {
        return false;
    }
    let Some((h, rest)) = rest.split_once(", displayed at ") else {
        return false;
    };
    if !digits(h) {
        return false;
    }
    let Some((dw, rest)) = rest.split_once('x') else {
        return false;
    };
    if !digits(dw) {
        return false;
    }
    let Some((dh, rest)) = rest.split_once(". Multiply coordinates by ") else {
        return false;
    };
    if !digits(dh) {
        return false;
    }
    let Some(scale) = rest.strip_suffix(" to map to original image.]") else {
        return false;
    };
    !scale.is_empty() && scale.bytes().all(|b| b.is_ascii_digit() || b == b'.')
}

/// Extract ordered content blocks from a `message.content` value
/// (string form is treated as a single text block).
pub fn content_blocks(content: &Value) -> Vec<Block> {
    match content {
        Value::String(s) => vec![Block::Text {
            content_index: 0,
            text: strip_image_dimension_note(s).into_owned(),
        }],
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(ix, b)| {
                let idx = b["index"].as_u64().unwrap_or(ix as u64) as usize;
                match b["type"].as_str() {
                    Some("thinking") => Block::Thinking {
                        content_index: idx,
                        text: b["thinking"].as_str().unwrap_or("").to_string(),
                    },
                    Some("image") => Block::Image {
                        content_index: idx,
                        mime: b["mimeType"].as_str().unwrap_or("image/png").to_string(),
                        data: b["data"].as_str().unwrap_or("").to_string(),
                    },
                    Some("toolCall") | Some("toolcall") => {
                        let args = b["partialJson"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                            .unwrap_or_else(|| {
                                b["arguments"]
                                    .as_object()
                                    .filter(|o| !o.is_empty())
                                    .map(|o| Value::Object(o.clone()).to_string())
                                    .unwrap_or_default()
                            });
                        Block::ToolCall {
                            content_index: idx,
                            id: b["id"].as_str().unwrap_or("").to_string(),
                            name: b["name"]
                                .as_str()
                                .or_else(|| b["toolName"].as_str())
                                .unwrap_or("")
                                .to_string(),
                            args,
                            result: String::new(),
                            is_error: false,
                            images: Vec::new(),
                            duration_s: None,
                            details: None,
                            args_partial: false,
                            result_arrived: false,
                        }
                    }
                    _ => Block::Text {
                        content_index: idx,
                        text: strip_image_dimension_note(b["text"].as_str().unwrap_or(""))
                            .into_owned(),
                    },
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}
