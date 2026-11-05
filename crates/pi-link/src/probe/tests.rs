use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;

#[test]
fn models_list_url_mechanical_shapes() {
    // openai 系带版本段（zai-coding-cn 实测：open.bigmodel.cn /v4）直接接 /models
    assert_eq!(
        models_list_url(
            "https://open.bigmodel.cn/api/coding/paas/v4",
            "openai-completions"
        ),
        Some((
            "https://open.bigmodel.cn/api/coding/paas/v4/models".into(),
            Auth::Bearer
        ))
    );
    // openai 形状不补版本段（pi-web 同款：只有 /models；DeepSeek 根域
    // /models 实测 200），探查阶梯兜 /v1/models 变体
    assert_eq!(
        models_list_url("https://api.deepseek.com", "openai-completions"),
        Some(("https://api.deepseek.com/models".into(), Auth::Bearer))
    );
    // anthropic：补 /v1 + limit=1000 + Anthropic 头
    assert_eq!(
        models_list_url("https://api.minimaxi.com/anthropic", "anthropic-messages"),
        Some((
            "https://api.minimaxi.com/anthropic/v1/models?limit=1000".into(),
            Auth::Anthropic
        ))
    );
    // anthropic 路径已带版本段：不重复补 /v1，limit 照加
    assert_eq!(
        models_list_url("https://gw.example.com/anthropic/v2", "anthropic-messages"),
        Some((
            "https://gw.example.com/anthropic/v2/models?limit=1000".into(),
            Auth::Anthropic
        ))
    );
    // google：补 /v1beta + pageSize=1000
    assert_eq!(
        models_list_url(
            "https://generativelanguage.googleapis.com",
            "google-generative-ai"
        ),
        Some((
            "https://generativelanguage.googleapis.com/v1beta/models?pageSize=1000".into(),
            Auth::Google
        ))
    );
    // 已以 /models 结尾：只定鉴权头，不重复拼
    assert_eq!(
        models_list_url("https://api.openai.com/v1/models/", "openai-completions"),
        Some(("https://api.openai.com/v1/models".into(), Auth::Bearer))
    );
    assert_eq!(models_list_url("", "openai-completions"), None);
}

#[test]
fn entry_parse_maps_each_provider_shape() {
    // openrouter：name/id/context_length + top_provider.max_completion_tokens
    let m = parse_entry(&serde_json::json!({
        "id": "deepseek/deepseek-r1", "name": "DeepSeek R1", "context_length": 128000,
        "top_provider": { "max_completion_tokens": 32768 }
    }))
    .unwrap();
    assert_eq!(m.id, "deepseek/deepseek-r1");
    assert_eq!(m.name, "DeepSeek R1");
    assert_eq!(m.context_window, Some(128000));
    assert_eq!(m.max_tokens, Some(32768));
    // google：name 带 models/ 前缀 + displayName + inputTokenLimit/outputTokenLimit
    let m = parse_entry(&serde_json::json!({
        "name": "models/gemini-2.0-flash", "displayName": "Gemini 2.0 Flash",
        "inputTokenLimit": 1048576, "outputTokenLimit": 8192
    }))
    .unwrap();
    assert_eq!(m.id, "gemini-2.0-flash");
    assert_eq!(m.name, "Gemini 2.0 Flash");
    assert_eq!(m.context_window, Some(1048576));
    assert_eq!(m.max_tokens, Some(8192));
    // 顶层 max_output_tokens / max_tokens 也认；0 视为没给
    assert_eq!(
        parse_entry(&serde_json::json!({ "id": "a", "max_output_tokens": 65536 }))
            .unwrap()
            .max_tokens,
        Some(65536)
    );
    assert_eq!(
        parse_entry(&serde_json::json!({ "id": "a", "max_tokens": 0 }))
            .unwrap()
            .max_tokens,
        None
    );
    // openai：只有 id（name=owned_by 字符串时回落空显示名）
    let m = parse_entry(&serde_json::json!({ "id": "gpt-4o", "object": "model" })).unwrap();
    assert_eq!(m.id, "gpt-4o");
    assert_eq!(m.name, "");
    assert_eq!(m.context_window, None);
    assert_eq!(m.max_tokens, None);
    assert_eq!(m.reasoning, None);
    assert_eq!(m.input, None);
    // DeepSeek（2026-11-02 curl 实录）：name/context_window/max_output_tokens/
    // input_modalities/effort 全给——0.3.2 之前 PF 只取三个键把其余全丢了
    let m = parse_entry(&serde_json::json!({
        "id": "deepseek-flash", "object": "model", "owned_by": "deepseek",
        "name": "DeepSeek-V4.1-Flash", "context_window": 1048576,
        "max_output_tokens": 393216,
        "input_modalities": ["text", "image"], "output_modalities": ["text"],
        "effort": { "supported_levels": ["low", "high", "max"], "default_level": "high" }
    }))
    .unwrap();
    assert_eq!(m.name, "DeepSeek-V4.1-Flash");
    assert_eq!(m.context_window, Some(1048576));
    assert_eq!(m.max_tokens, Some(393216));
    assert_eq!(
        m.input.as_deref(),
        Some(&["text".to_string(), "image".to_string()][..])
    );
    assert_eq!(m.reasoning, Some(true));
    // 非法模态被滤掉；空 supported_levels = 不支持推理
    let m = parse_entry(&serde_json::json!({
        "id": "x", "input_modalities": ["text", "video"],
        "effort": { "supported_levels": [] }
    }))
    .unwrap();
    assert_eq!(m.input.as_deref(), Some(&["text".to_string()][..]));
    assert_eq!(m.reasoning, Some(false));
    assert!(parse_entry(&serde_json::json!({ "object": "model" })).is_none());
    assert_eq!(
        parse_entry(&serde_json::json!({ "id": "x", "name": "x" }))
            .unwrap()
            .name,
        ""
    );
}

// ---- 模块②阶梯：本地 mock server 离线覆盖 -----------------------------

/// 极简 HTTP mock：按 (path, 鉴权样式, 请求体) 决定状态码；200 带 openai
/// 形状目录体、400 带「缺 model」校验错体。返回 base url + 记录到的请求体。
fn spawn_mock(
    rules: Arc<dyn Fn(&str, &str, &str) -> u16 + Send + Sync>,
    seen: Arc<std::sync::Mutex<Vec<(String, String, String)>>>,
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let rules = rules.clone();
            let seen = seen.clone();
            std::thread::spawn(move || {
                let mut stream = stream;
                let mut buf = Vec::new();
                let mut byte = [0u8; 1];
                loop {
                    match stream.read(&mut byte) {
                        Ok(1) => {
                            buf.push(byte[0]);
                            if buf.ends_with(b"\r\n\r\n") {
                                break;
                            }
                        }
                        _ => return,
                    }
                }
                let head = String::from_utf8_lossy(&buf);
                let mut lines = head.lines();
                let req_line = lines.next().unwrap_or("");
                let path = req_line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .to_string();
                let mut auth_kind = "none";
                let mut content_len = 0usize;
                for l in lines {
                    let lower = l.to_lowercase();
                    if lower.starts_with("authorization: bearer") {
                        auth_kind = "bearer";
                    }
                    if lower.starts_with("x-api-key:") {
                        auth_kind = "anthropic";
                    }
                    if lower.starts_with("x-goog-api-key:") {
                        auth_kind = "google";
                    }
                    if lower.starts_with("content-length:") {
                        content_len = l
                            .split(':')
                            .nth(1)
                            .and_then(|v| v.trim().parse().ok())
                            .unwrap_or(0);
                    }
                }
                let mut body = vec![0u8; content_len];
                if content_len > 0 {
                    stream.read_exact(&mut body).ok();
                }
                let body_s = String::from_utf8_lossy(&body).to_string();
                seen.lock()
                    .unwrap()
                    .push((path.clone(), auth_kind.into(), body_s.clone()));
                let status = rules(&path, auth_kind, &body_s);
                let (resp_body, reason) = match status {
                    200 => (r#"{"data":[{"id":"m-1","name":"M1"}]}"#, "OK"),
                    400 => (
                        r#"{"error":{"message":"model: Field required"}}"#,
                        "Bad Request",
                    ),
                    401 => ("{}", "Unauthorized"),
                    403 => ("{}", "Forbidden"),
                    _ => ("{}", "Not Found"),
                };
                let resp = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{resp_body}",
                    resp_body.len()
                );
                stream.write_all(resp.as_bytes()).ok();
            });
        }
    });
    format!("http://{addr}")
}

#[test]
fn probe_module3_get_catalog_stands_alone() {
    // 模块③（probe()）独立可用：内置商路径（mc_probe_start +
    // 的已知端点表）就是这一个函数；端点现由 registry dump 派生
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let base = spawn_mock(
        Arc::new(|path, kind, _body| {
            if path == "/v1/models" && kind == "bearer" {
                200
            } else {
                404
            }
        }),
        seen.clone(),
    );
    match probe(&format!("{base}/v1/models"), Auth::Bearer, "k") {
        ProbeOutcome::Ok {
            models, catalog, ..
        } => {
            assert_eq!(models, 1);
            assert_eq!(catalog[0].id, "m-1");
        }
        other => panic!("expected Ok, got {other:?}"),
    }
    // 401 → AuthRejected（内置商 key 错误的可见路径）
    let base401 = spawn_mock(
        Arc::new(|_path, _kind, _body| 401),
        Arc::new(std::sync::Mutex::new(Vec::new())),
    );
    assert_eq!(
        probe(&format!("{base401}/v1/models"), Auth::Bearer, "bad"),
        ProbeOutcome::AuthRejected { status: 401 }
    );
}

#[test]
fn connect_direct_catalog_hit() {
    // 候选 1（GET {base}/models Bearer）直接 200：连通 + 有目录
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let base = spawn_mock(
        Arc::new(|path, kind, _body| {
            if path.ends_with("/models") && kind == "bearer" {
                200
            } else {
                404
            }
        }),
        seen.clone(),
    );
    match probe_connect(&base, "k") {
        ProbeOutcome::Ok {
            models, catalog, ..
        } => {
            assert_eq!(models, 1);
            assert_eq!(catalog[0].id, "m-1");
        }
        other => panic!("expected Ok, got {other:?}"),
    }
    // 首个请求就是 GET /models，且 GET 不带请求体
    let first = &seen.lock().unwrap()[0];
    assert_eq!(first.0, "/models");
    assert_eq!(first.1, "bearer");
    assert_eq!(first.2, "");
}

#[test]
fn connect_handshake_400_is_protocol_reachable() {
    // 全部 GET 404（Messages-only 网关）：messages 握手 400 = 协议可达 →
    // Ok{空目录}（UI 的 PassedEmpty 源头）；握手体必须是 `{}`——无任何
    // 写死模型名
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let base = spawn_mock(
        Arc::new(|path, _kind, body| {
            if path.ends_with("/v1/messages") && body == "{}" {
                400
            } else {
                404
            }
        }),
        seen.clone(),
    );
    match probe_connect(&base, "k") {
        ProbeOutcome::Ok { models, .. } => assert_eq!(models, 0),
        other => panic!("expected Ok(empty), got {other:?}"),
    }
    let seen = seen.lock().unwrap();
    let handshake = seen.iter().find(|(p, ..)| p.ends_with("/v1/messages"));
    assert_eq!(handshake.unwrap().2, "{}", "握手体 = {{}}，无模型名");
    assert!(
        seen.iter().all(|(_, _, b)| b != "deepseek-chat"),
        "任何请求都不得携带写死模型名"
    );
}

#[test]
fn connect_handshake_422_validation_class_counts() {
    // DeepSeek 实测：协议校验错回 422（非 400）——校验类 4xx 整类都算
    // 协议可达
    let base = spawn_mock(
        Arc::new(|path, _kind, body| {
            if path.ends_with("/v1/messages") && body == "{}" {
                422
            } else {
                404
            }
        }),
        Arc::new(std::sync::Mutex::new(Vec::new())),
    );
    assert!(matches!(
        probe_connect(&base, "k"),
        ProbeOutcome::Ok { models: 0, .. }
    ));
}

#[test]
fn connect_handshake_404_route_missing_falls_through() {
    // 404 = 路由不存在，不算协议可达：全 404 → Unreachable
    let base = spawn_mock(
        Arc::new(|_path, _kind, _body| 404),
        Arc::new(std::sync::Mutex::new(Vec::new())),
    );
    assert!(matches!(
        probe_connect(&base, "k"),
        ProbeOutcome::Unreachable { .. }
    ));
}

#[test]
fn connect_auth_style_fallback() {
    // Bearer 全 401、Anthropic 样式 GET /v1/models 200：key 被拒证据
    // 不定论，换鉴权样式命中 → Ok{有目录}
    let base = spawn_mock(
        Arc::new(
            |_path, kind, _body| {
                if kind == "anthropic" { 200 } else { 401 }
            },
        ),
        Arc::new(std::sync::Mutex::new(Vec::new())),
    );
    match probe_connect(&base, "k") {
        ProbeOutcome::Ok { models, .. } => assert_eq!(models, 1),
        other => panic!("expected Ok, got {other:?}"),
    }
}

#[test]
fn connect_all_401_is_auth_rejected() {
    let base = spawn_mock(
        Arc::new(|_path, _kind, _body| 401),
        Arc::new(std::sync::Mutex::new(Vec::new())),
    );
    assert_eq!(
        probe_connect(&base, "bad"),
        ProbeOutcome::AuthRejected { status: 401 }
    );
}

#[test]
fn connect_dead_port_is_unreachable() {
    // 占住端口再释放：连接必然被拒
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    match probe_connect(&format!("http://{addr}"), "k") {
        ProbeOutcome::Unreachable { .. } => {}
        other => panic!("expected Unreachable, got {other:?}"),
    }
}

#[test]
fn connect_empty_base_is_unreachable() {
    assert!(matches!(
        probe_connect("", "k"),
        ProbeOutcome::Unreachable { .. }
    ));
}

#[test]
fn connect_version_segment_base_skips_dupe_gets() {
    // base 末段已是 /vN（智谱 coding /v4 实测样本）：{base}/models 直接
    // 命中候选 1，不产生 /v1/v1/models 之类的拼重请求
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let base = spawn_mock(
        Arc::new(|path, kind, _body| {
            if path.ends_with("/models") && kind == "bearer" {
                200
            } else {
                404
            }
        }),
        seen.clone(),
    );
    let versioned = format!("{base}/v4");
    match probe_connect(&versioned, "k") {
        ProbeOutcome::Ok { models, .. } => assert_eq!(models, 1),
        other => panic!("expected Ok, got {other:?}"),
    }
    let seen = seen.lock().unwrap();
    assert!(
        !seen.iter().any(|(p, _, _)| p.contains("/v1/")),
        "base 已带版本段不得再拼 /v1/*：{:?}",
        seen.iter().map(|(p, _, _)| p).collect::<Vec<_>>()
    );
}

#[test]
fn v1beta_base_tries_google_auth_on_direct_models() {
    // base 末段 /v1beta：Google 鉴权直连 {base}/models
    let base = spawn_mock(
        Arc::new(|path, kind, _body| {
            if path.ends_with("/models") && kind == "google" {
                200
            } else {
                404
            }
        }),
        Arc::new(std::sync::Mutex::new(Vec::new())),
    );
    assert!(matches!(
        probe_connect(&format!("{base}/v1beta"), "k"),
        ProbeOutcome::Ok { .. }
    ));
}
