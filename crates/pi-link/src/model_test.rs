//! 自定义模型「测试」请求（042 M2，对齐 pi-web
//! `app/api/models-config/test/route.ts` 的语义：向模型发一条
//! "Reply with OK only." 最小补全，通路/key/模型名三错当场可见）。
//!
//! pi-web 走 pi-ai SDK 的 completeSimple（四种 API 形状 SDK 内部消化）；
//! PF 没有 SDK，四种形状的请求体在这里手工构造——URL 拼接/鉴权头对齐
//! [`crate::probe`] 的同款规则，请求体对齐各家 API 的最小对话补全。

use std::time::Duration;

use serde_json::{json, Value};

use crate::probe::Auth;

const TEST_TIMEOUT_SECS: u64 = 20;
/// 与 pi-web 同款测试消息（maxTokens 16，maxRetries 0）。
pub const TEST_PROMPT: &str = "Reply with OK only.";
const MAX_TOKENS: u64 = 16;

/// 测试结果（对齐 pi-web 的 ok/error/latencyMs/status/responseText）。
#[derive(Debug, Clone, PartialEq)]
pub enum TestOutcome {
    Ok {
        latency_ms: u64,
        status: u16,
        text: String,
    },
    Err {
        message: String,
        status: Option<u16>,
        latency_ms: u64,
    },
}

/// 一次测试请求的完整描述（渲染/发送分离，构造可单测）。
#[derive(Debug, Clone, PartialEq)]
pub struct TestRequest {
    pub url: String,
    pub auth: Auth,
    pub body: Value,
}

/// 构造测试请求（请求头由调用方在发送时经 [`build_headers`] 合并）。
pub fn build_request(base_url: &str, api: &str, model_id: &str) -> Option<TestRequest> {
    if model_id.trim().is_empty() {
        return None;
    }
    let mut base = base_url.trim().trim_end_matches('/').to_string();
    if base.is_empty() || (!base.starts_with("http://") && !base.starts_with("https://")) {
        return None;
    }
    let (url, auth, body) = match api {
        "anthropic-messages" => {
            if !base.ends_with("/v1") {
                base.push_str("/v1");
            }
            (
                format!("{base}/messages"),
                Auth::Anthropic,
                json!({
                    "model": model_id,
                    "max_tokens": MAX_TOKENS,
                    "messages": [{ "role": "user", "content": TEST_PROMPT }],
                }),
            )
        }
        "google-generative-ai" => {
            if !base.ends_with("/v1beta") {
                base.push_str("/v1beta");
            }
            (
                format!("{base}/models/{model_id}:generateContent"),
                Auth::Google,
                json!({
                    "contents": [{ "role": "user", "parts": [{ "text": TEST_PROMPT }] }],
                    "generationConfig": { "maxOutputTokens": MAX_TOKENS }
                }),
            )
        }
        "openai-responses" => (
            format!("{base}/responses"),
            Auth::Bearer,
            json!({
                "model": model_id,
                "input": TEST_PROMPT,
                "max_output_tokens": MAX_TOKENS,
                "stream": false
            }),
        ),
        // openai-completions 兜底
        _ => (
            format!("{base}/chat/completions"),
            Auth::Bearer,
            json!({
                "model": model_id,
                "messages": [{ "role": "user", "content": TEST_PROMPT }],
                "max_tokens": MAX_TOKENS,
                "stream": false
            }),
        ),
    };
    Some(TestRequest { url, auth, body })
}

/// 请求头（鉴权 + 合并 provider/model headers，model 赢）。
pub fn build_headers(auth: Auth, key: &str, extra: &[(String, String)]) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = match auth {
        Auth::Bearer => vec![
            ("Authorization".into(), format!("Bearer {key}")),
            ("Content-Type".into(), "application/json".into()),
        ],
        Auth::Anthropic => vec![
            ("x-api-key".into(), key.to_string()),
            ("anthropic-version".into(), "2023-06-01".into()),
            ("Content-Type".into(), "application/json".into()),
        ],
        Auth::Google => vec![
            ("x-goog-api-key".into(), key.to_string()),
            ("Content-Type".into(), "application/json".into()),
        ],
    };
    for (name, value) in extra {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        if let Some(slot) = headers.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
            slot.1 = value.clone();
        } else {
            headers.push((name.to_string(), value.clone()));
        }
    }
    headers
}

/// 发送测试请求（阻塞，20s 超时）。2xx 解出文本回 [`TestOutcome::Ok`]；
/// 其余按 [`TestOutcome::Err`]（带 HTTP 状态与延迟）。
pub fn send(req: &TestRequest, key: &str, extra_headers: &[(String, String)]) -> TestOutcome {
    let started = std::time::Instant::now();
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(TEST_TIMEOUT_SECS))
        .build();
    let mut r = agent.post(&req.url);
    for (name, value) in build_headers(req.auth, key, extra_headers) {
        r = r.set(&name, &value);
    }
    let body = req.body.to_string();
    let resp = match r.send_string(&body) {
        Ok(resp) => resp,
        Err(ureq::Error::Status(status, resp)) => {
            let text = resp.into_string().unwrap_or_default();
            return TestOutcome::Err {
                message: extract_error(&status, &text),
                status: Some(status),
                latency_ms: started.elapsed().as_millis() as u64,
            };
        }
        Err(e) => {
            return TestOutcome::Err {
                message: e.to_string(),
                status: None,
                latency_ms: started.elapsed().as_millis() as u64,
            };
        }
    };
    let status = resp.status();
    let latency_ms = started.elapsed().as_millis() as u64;
    let text = resp.into_string().unwrap_or_default();
    let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    match extract_text(&parsed) {
        Some(t) if !t.trim().is_empty() => TestOutcome::Ok {
            latency_ms,
            status,
            text: t.chars().take(300).collect(),
        },
        _ => TestOutcome::Err {
            message: "模型返回了空响应".into(),
            status: Some(status),
            latency_ms,
        },
    }
}

/// 2xx 响应体里的助手文本（四种形状各自的返回结构）。
fn extract_text(v: &Value) -> Option<String> {
    // openai-completions: choices[0].message.content
    if let Some(t) = v
        .pointer("/choices/0/message/content")
        .and_then(|x| x.as_str())
    {
        return Some(t.to_string());
    }
    // openai-responses: output_text（SDK 聚合字段）或 output[].content[].text
    if let Some(t) = v.get("output_text").and_then(|x| x.as_str()) {
        return Some(t.to_string());
    }
    if let Some(arr) = v.get("output").and_then(|x| x.as_array()) {
        let mut out = String::new();
        for item in arr {
            if let Some(parts) = item.get("content").and_then(|c| c.as_array()) {
                for p in parts {
                    if p.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                        if let Some(t) = p.get("text").and_then(|t| t.as_str()) {
                            out.push_str(t);
                        }
                    }
                }
            }
        }
        if !out.is_empty() {
            return Some(out);
        }
    }
    // anthropic-messages: content[].text（拼接）
    if let Some(arr) = v.get("content").and_then(|x| x.as_array()) {
        let mut out = String::new();
        for item in arr {
            if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                    out.push_str(t);
                }
            }
        }
        if !out.is_empty() {
            return Some(out);
        }
    }
    // google-generative-ai: candidates[0].content.parts[].text
    if let Some(arr) = v.pointer("/candidates/0/content/parts").and_then(|x| x.as_array()) {
        let mut out = String::new();
        for p in arr {
            if let Some(t) = p.get("text").and_then(|t| t.as_str()) {
                out.push_str(t);
            }
        }
        if !out.is_empty() {
            return Some(out);
        }
    }
    None
}

/// 非 2xx 响应的错误文案：各家 API 的 error.message 优先，退回原文截断。
fn extract_error(status: &u16, text: &str) -> String {
    let parsed: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let msg = parsed
        .pointer("/error/message")
        .and_then(|m| m.as_str())
        .or_else(|| parsed.pointer("/error").and_then(|m| m.as_str()))
        .or_else(|| parsed.pointer("/0/error/message").and_then(|m| m.as_str()))
        .unwrap_or(text);
    let msg = msg.trim();
    if msg.is_empty() {
        format!("HTTP {status}")
    } else {
        format!("HTTP {status} · {}", msg.chars().take(200).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_shape_per_api() {
        let r = build_request("https://gw.example.com/v1", "openai-completions", "m-1")
            .unwrap();
        assert_eq!(r.url, "https://gw.example.com/v1/chat/completions");
        assert_eq!(r.body["model"], "m-1");
        assert_eq!(r.body["messages"][0]["content"], TEST_PROMPT);
        assert_eq!(r.body["max_tokens"], 16);

        let r = build_request("https://gw.example.com/v1", "openai-responses", "m-1")
            .unwrap();
        assert_eq!(r.url, "https://gw.example.com/v1/responses");
        assert_eq!(r.body["input"], TEST_PROMPT);
        assert_eq!(r.body["max_output_tokens"], 16);

        let r = build_request("https://gw.example.com", "anthropic-messages", "m-1")
            .unwrap();
        assert_eq!(r.url, "https://gw.example.com/v1/messages");
        assert_eq!(r.body["max_tokens"], 16, "占位符已换名");
        assert!(r.body.get("maxTokensPlaceholder").is_none());

        let r = build_request(
            "https://generativelanguage.googleapis.com",
            "google-generative-ai",
            "gemini-x",
        )
        .unwrap();
        assert_eq!(
            r.url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-x:generateContent"
        );
        assert_eq!(r.body["contents"][0]["parts"][0]["text"], TEST_PROMPT);
        // 版本段不重复补
        let r = build_request(
            "https://generativelanguage.googleapis.com/v1beta",
            "google-generative-ai",
            "g",
        )
        .unwrap();
        assert!(r.url.contains("/v1beta/models/g:generateContent"));
        // 空模型 id 拒绝
        assert!(build_request("https://gw", "openai-completions", "  ").is_none());
    }

    #[test]
    fn headers_merge_with_model_override() {
        let hs = build_headers(
            Auth::Bearer,
            "sk-1",
            &[
                ("X-Flag".into(), "provider".into()),
                ("User-Agent".into(), "pi-flash".into()),
                ("".into(), "dropped".into()),
            ],
        );
        assert_eq!(
            hs.iter().find(|(n, _)| n == "Authorization").unwrap().1,
            "Bearer sk-1"
        );
        assert_eq!(hs.iter().find(|(n, _)| n == "X-Flag").unwrap().1, "provider");
        // model 覆盖 provider 的同名 header
        let hs = build_headers(
            Auth::Bearer,
            "sk-1",
            &[
                ("User-Agent".into(), "provider-ua".into()),
                ("user-agent".into(), "model-ua".into()),
            ],
        );
        assert_eq!(hs.iter().find(|(n, _)| n == "User-Agent").unwrap().1, "model-ua");
        // anthropic 鉴权头形状
        let hs = build_headers(Auth::Anthropic, "sk-2", &[]);
        assert_eq!(hs.iter().find(|(n, _)| n == "x-api-key").unwrap().1, "sk-2");
        assert_eq!(
            hs.iter().find(|(n, _)| n == "anthropic-version").unwrap().1,
            "2023-06-01"
        );
    }

    #[test]
    fn text_extraction_covers_four_shapes() {
        assert_eq!(
            extract_text(&json!({"choices":[{"message":{"content":"OK"}}]})).as_deref(),
            Some("OK")
        );
        assert_eq!(
            extract_text(&json!({"output_text":"OK"})).as_deref(),
            Some("OK")
        );
        assert_eq!(
            extract_text(&json!({"output":[{"content":[{"type":"output_text","text":"OK"}]}]}))
                .as_deref(),
            Some("OK")
        );
        assert_eq!(
            extract_text(&json!({"content":[{"type":"text","text":"OK"}]})).as_deref(),
            Some("OK")
        );
        assert_eq!(
            extract_text(&json!({"candidates":[{"content":{"parts":[{"text":"OK"}]}}]}))
                .as_deref(),
            Some("OK")
        );
        assert_eq!(extract_text(&json!({"error":{"message":"bad"}})), None);
    }

    #[test]
    fn error_message_prefers_api_shape() {
        assert_eq!(
            extract_error(&400, r#"{"error":{"message":"Model not found"}}"#),
            "HTTP 400 · Model not found"
        );
        assert_eq!(extract_error(&500, ""), "HTTP 500");
        assert_eq!(extract_error(&502, "Bad Gateway"), "HTTP 502 · Bad Gateway");
    }
}
